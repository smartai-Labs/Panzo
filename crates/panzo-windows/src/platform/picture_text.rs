//! Native in-canvas text entry, with a single editable draft until commit.
use super::*;
use crate::platform::workspace_ui as ui;
use windows::Win32::{
    Graphics::Gdi as gdi,
    UI::{
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
pub(super) const INPUT: usize = 2000;
pub(super) const FINISH: u32 = WM_APP + 392;
pub(super) const MOVE: u32 = WM_APP + 393;
pub(super) struct InlineText {
    pub index: Option<usize>,
    pub point: POINT,
    pub size: Option<[u32; 2]>,
}
pub(super) struct TextDrag {
    start: POINT,
    crop: super::image_interaction::Crop,
    handle: usize,
}
const CARET_TIMER: usize = 0x5041_399;
fn handles(r: RECT) -> [(i32, i32); 8] {
    let (l, t, rr, b) = (3, 3, (r.right - 4).max(3), (r.bottom - 4).max(3));
    [
        (l, t),
        ((l + rr) / 2, t),
        (rr, t),
        (rr, (t + b) / 2),
        (rr, b),
        ((l + rr) / 2, b),
        (l, b),
        (l, (t + b) / 2),
    ]
}
fn hit_handle(h: HWND, x: i32, y: i32) -> Option<usize> {
    let r = ui::rect(h);
    handles(r)
        .iter()
        .position(|(hx, hy)| (x - hx).abs() <= 5 && (y - hy).abs() <= 5)
        .or_else(|| (x < 5 || y < 5 || x >= r.right - 5 || y >= r.bottom - 5).then_some(8))
}
impl Picture {
    pub(super) fn inline_mark(&self) -> Option<Mark> {
        let edit = self.inline_text.as_ref()?;
        Some(Mark {
            tool: Tool::Text,
            points: vec![edit.point],
            text: ui::value(self.inline_input),
            text_box: edit.size,
            color: self.color,
            stroke: self.stroke,
            filled: self.filled,
            font_size: self.font_size,
            font: self.font_style.clone(),
        })
    }

    pub(super) fn inline_text_changed(&self) -> bool {
        let Some(edit) = &self.inline_text else {
            return false;
        };
        let text = ui::value(self.inline_input);
        if let Some(index) = edit.index {
            let document = self.document.borrow();
            document.state.marks.get(index).is_some_and(|mark| {
                mark.text != text
                    || mark.color != self.color
                    || mark.font_size != self.font_size
                    || mark.font != self.font_style
                    || mark.text_box != edit.size
                    || mark.points[0].x != edit.point.x
                    || mark.points[0].y != edit.point.y
            })
        } else {
            !text.trim().is_empty()
        }
    }
    pub(super) fn make_inline_text(&mut self, hwnd: HWND) -> Result<(), String> {
        unsafe {
            windows::Win32::System::LibraryLoader::LoadLibraryW(w!("Msftedit.dll"))
                .map_err(|e| e.to_string())?;
        }
        self.inline_input = ui::child(
            hwnd,
            w!("RICHEDIT50W"),
            "",
            INPUT,
            WS_TABSTOP | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32),
        )?;
        unsafe {
            SetWindowLongPtrW(
                self.inline_input,
                GWL_EXSTYLE,
                GetWindowLongPtrW(self.inline_input, GWL_EXSTYLE) | WS_EX_TRANSPARENT.0 as isize,
            );
            SendMessageW(
                self.inline_input,
                windows::Win32::UI::Controls::RichEdit::EM_SETEVENTMASK,
                None,
                Some(LPARAM(
                    windows::Win32::UI::Controls::RichEdit::ENM_CHANGE as isize,
                )),
            );
        }
        unsafe {
            SendMessageW(self.inline_input, EM_SETLIMITTEXT, Some(WPARAM(4096)), None);
            let _ = SetWindowSubclass(self.inline_input, Some(input_proc), INPUT, hwnd.0 as usize);
        }
        ui::show(self.inline_input, false);
        Ok(())
    }
    pub(super) fn begin_inline_text(&mut self, hwnd: HWND, index: Option<usize>, point: POINT) {
        let _ = self.finish_inline_text(hwnd, true);
        let _ = self.commit_properties(hwnd);
        self.selected = index;
        let value = if let Some(index) = index {
            let mark = self.document.borrow().state.marks[index].clone();
            self.color = mark.color;
            self.font_size = mark.font_size;
            self.font_style = mark.font.clone();
            self.sync_font_controls();
            mark.text
        } else {
            String::new()
        };
        let size = if let Some(i) = index {
            self.document.borrow().state.marks[i].text_box
        } else {
            let d = self.document.borrow();
            Some([
                240.min(d.original.width.saturating_sub(point.x as u32))
                    .max(1),
                (self.font_size * 3)
                    .max(72)
                    .min(d.original.height.saturating_sub(point.y as u32))
                    .max(1),
            ])
        };
        self.inline_text = Some(InlineText { index, point, size });
        self.inline_caret_visible = true;
        self.syncing_text_fields = true;
        ui::text(self.text_input, &value);
        ui::text(self.inline_input, &value);
        self.syncing_text_fields = false;
        self.property_preview = None;
        self.display_cache = None;
        self.desktop_layout(hwnd);
        self.layout_inline_text(hwnd);
        unsafe {
            let _ = SetFocus(Some(self.inline_input));
            SendMessageW(
                self.inline_input,
                windows::Win32::UI::Controls::EM_SETSEL,
                Some(WPARAM(value.encode_utf16().count())),
                Some(LPARAM(value.encode_utf16().count() as isize)),
            );
        }
        invalidate(hwnd);
        crate::platform::desktop_ui::invalidate(self.inline_input);
    }
    pub(super) fn layout_inline_text(&mut self, hwnd: HWND) {
        let Some(edit) = &self.inline_text else {
            return;
        };
        let view = self.view(hwnd);
        let area = self.canvas_area(hwnd);
        let document = self.document.borrow();
        let p = document.display_point(edit.point);
        let (iw, ih) = document.size();
        let x =
            view.left + (p.x as f64 * (view.right - view.left) as f64 / iw as f64).round() as i32;
        let y =
            view.top + (p.y as f64 * (view.bottom - view.top) as f64 / ih as f64).round() as i32;
        drop(document);
        let font = ((self.font_size as f64) * (view.right - view.left) as f64 / iw as f64)
            .round()
            .clamp(1., 4096.) as i32;
        let font_changed = self.inline_font_height != font;
        if font_changed {
            let next = crate::platform::font_catalog::create_font(&self.font_style, font);
            unsafe {
                SendMessageW(
                    self.inline_input,
                    WM_SETFONT,
                    Some(WPARAM(next.0 as usize)),
                    Some(LPARAM(1)),
                );
                if !self.inline_font.is_invalid() {
                    let _ = gdi::DeleteObject(self.inline_font.into());
                }
            }
            self.inline_font = next;
            self.inline_font_height = font;
        }
        let scale = (view.right - view.left) as f64 / iw as f64;
        let size = edit.size.map(|s| {
            (
                (s[0] as f64 * scale).round() as i32,
                (s[1] as f64 * scale).round() as i32,
            )
        });
        let (content_w, content_h) = size.unwrap_or_else(|| {
            let (tw, th) = crate::platform::font_catalog::measure(
                &ui::value(self.inline_input),
                &self.font_style,
                font,
            );
            ((tw + 2).max(16), th.max(font + 4))
        });
        // The 4px frame is outside the text's image coordinates.
        let width = (content_w + 8).max(16).min((area.right - x + 4).max(16));
        let height = (content_h + 8).max(16).min((area.bottom - y + 4).max(16));
        ui::place(self.inline_input, x - 4, y - 4, width, height);
        // Apply zoom before installing physical-pixel frame insets; RichEdit
        // rescales an existing formatting rectangle when its zoom changes.
        if font_changed
            || (!self
                .inline_move
                .as_ref()
                .is_some_and(|drag| drag.handle == 8)
                && self.pan_drag.is_none())
        {
            self.apply_inline_format(scale);
        }
        let formatting = RECT {
            left: 4,
            top: 4,
            right: width - 4,
            bottom: height - 4,
        };
        unsafe {
            SendMessageW(
                self.inline_input,
                windows::Win32::UI::Controls::EM_SETRECTNP,
                None,
                Some(LPARAM((&formatting as *const RECT) as isize)),
            );
        }
        // A panned input must not cover the toolbar or property inspector.
        unsafe {
            let region = gdi::CreateRectRgn(
                (area.left - x + 4).clamp(0, width),
                (area.top - y + 4).clamp(0, height),
                (area.right - x + 4).clamp(0, width),
                (area.bottom - y + 4).clamp(0, height),
            );
            if !region.is_invalid()
                && gdi::SetWindowRgn(self.inline_input, Some(region), false) == 0
            {
                let _ = gdi::DeleteObject(region.into());
            }
        }
        ui::show(self.inline_input, true);
        crate::platform::desktop_ui::invalidate(self.inline_input);
    }
    fn apply_inline_format(&mut self, scale: f64) {
        if self.inline_ime {
            return;
        }
        let old = self.syncing_text_fields;
        self.syncing_text_fields = true;
        crate::platform::capture_tools::image_text::format(
            self.inline_input,
            &self.font_style,
            self.font_size as i32,
            self.color,
            self.dpi,
        );
        let ratio = (scale * 10000.).round().max(1.) as usize;
        unsafe {
            SendMessageW(
                self.inline_input,
                windows::Win32::UI::Controls::RichEdit::EM_SETZOOM,
                Some(WPARAM(ratio)),
                Some(LPARAM(10000)),
            );
            let origin = POINT::default();
            SendMessageW(
                self.inline_input,
                windows::Win32::UI::Controls::RichEdit::EM_SETSCROLLPOS,
                None,
                Some(LPARAM((&origin as *const _) as isize)),
            );
        }
        self.syncing_text_fields = old;
    }
    pub(super) fn sync_inline_text(&mut self, hwnd: HWND, from_canvas: bool) {
        if self.syncing_text_fields {
            return;
        }
        self.syncing_text_fields = true;
        if from_canvas {
            ui::text(self.text_input, &ui::value(self.inline_input));
        } else if self.inline_text.is_some() {
            ui::text(self.inline_input, &ui::value(self.text_input));
        }
        self.syncing_text_fields = false;
        if self.inline_text.is_some() {
            self.layout_inline_text(hwnd);
        } else {
            self.preview_properties();
        }
        invalidate(hwnd);
    }
    pub(super) fn finish_inline_text(&mut self, hwnd: HWND, commit: bool) -> Result<(), String> {
        let Some(edit) = self.inline_text.take() else {
            return Ok(());
        };
        let value = ui::value(self.inline_input);
        self.inline_move = None;
        unsafe {
            let _ = KillTimer(Some(self.inline_input), CARET_TIMER);
        }
        ui::show(self.inline_input, false);
        self.property_preview = None;
        let mut changed = false;
        if commit {
            let mark = Mark {
                text_box: edit.size,
                tool: Tool::Text,
                points: vec![edit.point],
                color: self.color,
                text: value.clone(),
                stroke: self.stroke,
                filled: self.filled,
                font_size: self.font_size,
                font: self.font_style.clone(),
            };
            let mut document = self.document.borrow_mut();
            if let Some(index) = edit.index {
                if value.trim().is_empty() {
                    document.delete(index);
                    self.selected = None;
                    changed = true;
                } else {
                    changed = document.replace_mark(index, mark);
                    self.selected = Some(index);
                }
            } else if !value.trim().is_empty() {
                let index = document.state.marks.len();
                document.commit(mark);
                self.selected = Some(index);
                changed = true;
            }
        } else {
            self.selected = edit.index;
        }
        self.cached = None;
        self.display_cache = None;
        if changed {
            self.dirty = true;
            notify_document(hwnd, &self.document)?;
        }
        self.select_properties(hwnd);
        invalidate(hwnd);
        Ok(())
    }
    pub(super) fn inline_message(&mut self, hwnd: HWND, m: u32, w: WPARAM) -> bool {
        if m == MOVE {
            let mut screen = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut screen);
                let _ = gdi::ScreenToClient(hwnd, &mut screen);
            }
            if let Some(p) = self.image_point(hwnd, screen, true) {
                if w.0 >= 10 {
                    if let Some(edit) = &self.inline_text {
                        let d = self.document.borrow();
                        let size = edit.size.unwrap_or_else(|| {
                            let (w, h) = crate::platform::font_catalog::measure(
                                &ui::value(self.inline_input),
                                &self.font_style,
                                self.font_size as i32,
                            );
                            [w.max(8) as u32, h.max(8) as u32]
                        });
                        self.inline_move = Some(TextDrag {
                            start: p,
                            handle: w.0 - 10,
                            crop: super::image_interaction::Crop {
                                rect: [
                                    edit.point.x as f64,
                                    edit.point.y as f64,
                                    (edit.point.x as f64 + size[0] as f64)
                                        .min(d.original.width as f64),
                                    (edit.point.y as f64 + size[1] as f64)
                                        .min(d.original.height as f64),
                                ],
                                size: (d.original.width, d.original.height),
                                ratio: None,
                            },
                        });
                    }
                } else if let Some(drag) = &self.inline_move {
                    let next = drag.crop.dragged(
                        drag.handle,
                        (p.x - drag.start.x) as f64,
                        (p.y - drag.start.y) as f64,
                    );
                    if let Some(edit) = &mut self.inline_text {
                        edit.point = POINT {
                            x: next.rect[0].round() as i32,
                            y: next.rect[1].round() as i32,
                        };
                        edit.size = Some([
                            (next.rect[2] - next.rect[0]).round().max(1.) as u32,
                            (next.rect[3] - next.rect[1]).round().max(1.) as u32,
                        ]);
                    }
                    self.layout_inline_text(hwnd);
                    invalidate(hwnd);
                }
            }
            if w.0 == 2 {
                self.inline_move = None;
            }
            return true;
        }
        if m == FINISH {
            let _ = self.finish_inline_text(hwnd, w.0 != 0);
            unsafe {
                let _ = SetFocus(Some(hwnd));
            }
            return true;
        }
        false
    }
}
unsafe extern "system" fn input_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    owner: usize,
) -> LRESULT {
    let parent = HWND(owner as *mut _);
    if m == WM_SETFOCUS {
        let result = unsafe { DefSubclassProc(h, m, w, l) };
        crate::platform::workspace::remember_text_focus(h);
        if let Some(s) = unsafe { picture_mut(parent) } {
            s.inline_caret_visible = true;
        }
        unsafe {
            let _ = HideCaret(Some(h));
            SetTimer(
                Some(h),
                CARET_TIMER,
                GetCaretBlinkTime().clamp(250, 1200),
                None,
            );
        }
        crate::platform::desktop_ui::invalidate(h);
        return result;
    }
    if m == WM_KILLFOCUS {
        unsafe {
            let _ = KillTimer(Some(h), CARET_TIMER);
        }
        crate::platform::desktop_ui::invalidate(h);
    }
    if m == WM_TIMER && w.0 == CARET_TIMER {
        if let Some(s) = unsafe { picture_mut(parent) } {
            s.inline_caret_visible = !s.inline_caret_visible;
        }
        crate::platform::desktop_ui::invalidate(h);
        return LRESULT(0);
    }
    if matches!(m, WM_CHAR | WM_KEYDOWN | WM_LBUTTONDOWN) {
        if let Some(s) = unsafe { picture_mut(parent) } {
            s.inline_caret_visible = true;
        }
    }
    if m == 0x10D {
        if let Some(s) = unsafe { picture_mut(parent) } {
            s.inline_ime = true;
        }
    }
    if m == 0x10E {
        let result = unsafe { DefSubclassProc(h, m, w, l) };
        if let Some(s) = unsafe { picture_mut(parent) } {
            s.inline_ime = false;
            s.layout_inline_text(parent);
        }
        return result;
    }
    if m == WM_CHAR && matches!(w.0, 2 | 9 | 21) && unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0
    {
        return LRESULT(0);
    }
    if m == WM_CHAR && (w.0 == 27 || (w.0 == 13 && unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0))
    {
        return LRESULT(0);
    }
    if m == WM_KEYDOWN
        && matches!(w.0, 0x42 | 0x49 | 0x55)
        && unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0
    {
        if let Some(s) = unsafe { picture_mut(parent) } {
            match w.0 {
                0x42 => s.font_style.weight = if s.font_style.weight >= 600 { 400 } else { 700 },
                0x49 => s.font_style.italic = !s.font_style.italic,
                _ => s.font_style.underline = !s.font_style.underline,
            };
            s.sync_font_controls();
            s.inline_font_height = 0;
            s.layout_inline_text(parent);
            s.preview_properties();
            invalidate(parent);
        }
        return LRESULT(0);
    }
    if m == WM_ERASEBKGND {
        return LRESULT(1);
    }
    if m == WM_PAINT {
        let origin = POINT::default();
        unsafe {
            SendMessageW(
                h,
                windows::Win32::UI::Controls::RichEdit::EM_SETSCROLLPOS,
                None,
                Some(LPARAM((&origin as *const _) as isize)),
            );
        }
        let mut ps = gdi::PAINTSTRUCT::default();
        let target = unsafe { gdi::BeginPaint(h, &mut ps) };
        if let Some(state) = unsafe { picture_mut(parent) } {
            let r = ui::rect(h);
            let mut origin = POINT::default();
            unsafe {
                let _ = gdi::ClientToScreen(h, &mut origin);
                let _ = gdi::ScreenToClient(parent, &mut origin);
            }
            if let Ok(frame) = Surface::new(r.right.max(1) as u32, r.bottom.max(1) as u32) {
                // Paint into the small input surface using parent coordinates.
                // The DIB clips the output; no full-window scratch bitmap is needed.
                unsafe {
                    let _ = gdi::SetViewportOrgEx(frame.dc, -origin.x, -origin.y, None);
                }
                state.paint(parent, frame.dc);
                unsafe {
                    let _ = gdi::SetViewportOrgEx(frame.dc, 0, 0, None);
                }
                // The parent already contains the source-space draft. Never draw a
                // second set of glyphs with independently rounded screen metrics.
                // Keep native selection geometry, but tint the shared glyph raster
                // rather than drawing a second, differently scaled text layer.
                let mut selection = windows::Win32::UI::Controls::RichEdit::CHARRANGE::default();
                unsafe {
                    SendMessageW(
                        h,
                        windows::Win32::UI::Controls::RichEdit::EM_EXGETSEL,
                        None,
                        Some(LPARAM((&mut selection as *mut _) as isize)),
                    );
                }
                if selection.cpMin != selection.cpMax {
                    if let Ok(mask) = Surface::new(r.right as u32, r.bottom as u32) {
                        unsafe {
                            std::ptr::write_bytes(mask.bits, 0, mask.bytes);
                            DefSubclassProc(
                                h,
                                WM_PRINTCLIENT,
                                WPARAM(mask.dc.0 as usize),
                                LPARAM(PRF_CLIENT as isize),
                            );
                        }
                        let highlight = unsafe { gdi::GetSysColor(gdi::COLOR_HIGHLIGHT) };
                        let rgb = [
                            (highlight >> 16) as u8,
                            (highlight >> 8) as u8,
                            highlight as u8,
                        ];
                        let mask = mask.raster(r.right as u32, r.bottom as u32);
                        let mut pixels = frame.raster(r.right as u32, r.bottom as u32);
                        for (p, m) in pixels
                            .pixels
                            .chunks_exact_mut(4)
                            .zip(mask.pixels.chunks_exact(4))
                        {
                            if m[..3] == rgb {
                                for c in 0..3 {
                                    p[c] = ((u16::from(p[c]) + u16::from(rgb[c])) / 2) as u8;
                                }
                            }
                        }
                        frame.write_pixels(&pixels.pixels);
                    }
                }
                let edge = editor_ui::rgb(165, 182, 204);
                for rect in [
                    RECT {
                        left: 3,
                        top: 3,
                        right: r.right - 3,
                        bottom: 4,
                    },
                    RECT {
                        left: 3,
                        top: r.bottom - 4,
                        right: r.right - 3,
                        bottom: r.bottom - 3,
                    },
                    RECT {
                        left: 3,
                        top: 3,
                        right: 4,
                        bottom: r.bottom - 3,
                    },
                    RECT {
                        left: r.right - 4,
                        top: 3,
                        right: r.right - 3,
                        bottom: r.bottom - 3,
                    },
                ] {
                    fill(frame.dc, rect, edge);
                }
                for (x, y) in handles(r) {
                    fill(
                        frame.dc,
                        RECT {
                            left: x - 2,
                            top: y - 2,
                            right: x + 3,
                            bottom: y + 3,
                        },
                        edge,
                    );
                }
                if unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() } == h
                    && state.inline_caret_visible
                {
                    let mut range = windows::Win32::UI::Controls::RichEdit::CHARRANGE::default();
                    unsafe {
                        SendMessageW(
                            h,
                            windows::Win32::UI::Controls::RichEdit::EM_EXGETSEL,
                            None,
                            Some(LPARAM((&mut range as *mut _) as isize)),
                        );
                    }
                    if range.cpMin == range.cpMax || ui::value(h).is_empty() {
                        let mut p = POINT { x: 4, y: 4 };
                        unsafe {
                            let _ = GetCaretPos(&mut p);
                        }
                        let x = p.x.clamp(4, (r.right - 5).max(4));
                        let y = p.y.clamp(4, (r.bottom - 5).max(4));
                        let bottom = (y + state.inline_font_height.max(8)).min(r.bottom - 4);
                        fill(
                            frame.dc,
                            RECT {
                                left: x - 1,
                                top: y,
                                right: x + 2,
                                bottom,
                            },
                            editor_ui::rgb(255, 255, 255),
                        );
                        fill(
                            frame.dc,
                            RECT {
                                left: x,
                                top: y,
                                right: x + 1,
                                bottom,
                            },
                            editor_ui::rgb(24, 24, 24),
                        );
                    }
                }
                frame.opaque_rows(r.right as u32, r.bottom as u32);
                unsafe {
                    let _ = gdi::BitBlt(
                        target,
                        0,
                        0,
                        r.right,
                        r.bottom,
                        Some(frame.dc),
                        0,
                        0,
                        gdi::SRCCOPY,
                    );
                }
            }
        }
        unsafe {
            let _ = gdi::EndPaint(h, &ps);
        }
        return LRESULT(0);
    }
    if m == WM_SETCURSOR && (l.0 as u32 & 0xffff) == HTBORDER {
        let mut p = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut p);
            let _ = gdi::ScreenToClient(h, &mut p);
        }
        let cursor = match hit_handle(h, p.x, p.y) {
            Some(0 | 4) => IDC_SIZENWSE,
            Some(2 | 6) => IDC_SIZENESW,
            Some(1 | 5) => IDC_SIZENS,
            Some(3 | 7) => IDC_SIZEWE,
            _ => IDC_SIZEALL,
        };
        unsafe {
            SetCursor(LoadCursorW(None, cursor).ok());
        }
        return LRESULT(1);
    }
    if m == WM_NCHITTEST {
        let mut p = POINT {
            x: l.0 as i16 as i32,
            y: (l.0 >> 16) as i16 as i32,
        };
        unsafe {
            let _ = gdi::ScreenToClient(h, &mut p);
        }
        if hit_handle(h, p.x, p.y).is_some() {
            return LRESULT(HTBORDER as isize);
        }
    }
    if m == WM_NCLBUTTONDOWN && w.0 == HTBORDER as usize {
        let mut p = POINT {
            x: l.0 as i16 as i32,
            y: (l.0 >> 16) as i16 as i32,
        };
        unsafe {
            let _ = gdi::ScreenToClient(h, &mut p);
        }
        let handle = hit_handle(h, p.x, p.y).unwrap_or(8);
        unsafe {
            SendMessageW(parent, MOVE, Some(WPARAM(handle + 10)), None);
            SetCapture(h);
        }
        return LRESULT(0);
    }
    if matches!(m, WM_MBUTTONDOWN | WM_NCMBUTTONDOWN) {
        let mut p = POINT {
            x: l.0 as i16 as i32,
            y: (l.0 >> 16) as i16 as i32,
        };
        unsafe {
            if m == WM_MBUTTONDOWN {
                let _ = gdi::ClientToScreen(h, &mut p);
            }
            let _ = gdi::ScreenToClient(parent, &mut p);
            SendMessageW(
                parent,
                WM_MBUTTONDOWN,
                Some(w),
                Some(LPARAM(
                    ((p.y as u32 & 0xffff) << 16 | (p.x as u32 & 0xffff)) as isize,
                )),
            );
        }
        return LRESULT(0);
    }
    let dragging = unsafe { picture_mut(parent) }.is_some_and(|p| p.inline_move.is_some());
    if dragging && matches!(m, WM_MOUSEMOVE | WM_LBUTTONUP) {
        unsafe {
            SendMessageW(
                parent,
                MOVE,
                Some(WPARAM(if m == WM_LBUTTONUP { 2 } else { 1 })),
                None,
            );
            if m == WM_LBUTTONUP {
                let _ = ReleaseCapture();
            }
        }
        return LRESULT(0);
    }
    if m == WM_KEYDOWN
        && (w.0 == 27 || (w.0 == 13 && unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0))
    {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(owner as *mut _)),
                FINISH,
                WPARAM(usize::from(w.0 != 27)),
                LPARAM(0),
            );
        }
        return LRESULT(0);
    }
    unsafe { DefSubclassProc(h, m, w, l) }
}
