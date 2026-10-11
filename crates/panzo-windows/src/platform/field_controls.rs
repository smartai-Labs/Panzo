use windows::Win32::UI::Controls::{
    COMBOBOXINFO, EM_GETFIRSTVISIBLELINE, EM_GETLINECOUNT, EM_LINESCROLL, EM_REPLACESEL,
    EM_SETMARGINS, EM_SETRECTNP, GetComboBoxInfo,
};
// Borderless native text fields and themed dropdown lists. Native controls retain text,
// selection, IME, accessibility and keyboard semantics; only chrome is replaced.
use super::{
    editor_ui::{self, EditorTheme},
    invalidate, rect, text,
};
use std::{cell::RefCell, collections::HashMap};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::{self, HDC},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
thread_local! {static CUES:RefCell<HashMap<isize,String>>=RefCell::new(HashMap::new());}
const FIELD: usize = 0x5041_5a13;
const LIST: usize = 0x5041_5a14;
#[derive(Clone, Copy)]
struct Drag {
    start: i32,
    first: i32,
}
thread_local! { static DRAGS: RefCell<HashMap<isize, Drag>> = RefCell::new(HashMap::new()); }
pub(super) fn is_edit(h: HWND) -> bool {
    class(h).eq_ignore_ascii_case("edit")
}
fn class(h: HWND) -> String {
    let mut c = [0; 32];
    let n = unsafe { GetClassNameW(h, &mut c) };
    String::from_utf16_lossy(&c[..n.max(0) as usize])
}
fn message(h: HWND, m: u32, w: usize, l: isize) -> isize {
    unsafe { SendMessageW(h, m, Some(WPARAM(w)), Some(LPARAM(l))).0 }
}
thread_local! { static HIDING: RefCell<std::collections::HashSet<isize>>=RefCell::new(std::collections::HashSet::new()); }
fn hide_native_scroll(h: HWND) {
    if !HIDING.with(|v| v.borrow_mut().insert(h.0 as isize)) {
        return;
    }
    unsafe {
        let _ = windows::Win32::UI::Controls::ShowScrollBar(h, SB_VERT, false);
    }
    HIDING.with(|v| {
        v.borrow_mut().remove(&(h.0 as isize));
    });
}
fn strip_frame(h: HWND) {
    unsafe {
        let style = GetWindowLongPtrW(h, GWL_STYLE);
        SetWindowLongPtrW(
            h,
            GWL_STYLE,
            style & !((WS_BORDER | WS_VSCROLL | WS_HSCROLL).0 as isize),
        );
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        SetWindowLongPtrW(
            h,
            GWL_EXSTYLE,
            ex & !((WS_EX_CLIENTEDGE | WS_EX_STATICEDGE).0 as isize),
        );
        let _ = SetWindowPos(
            h,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}
thread_local! {static EDITS:RefCell<std::collections::HashSet<isize>>=RefCell::new(std::collections::HashSet::new());}
pub(crate) fn edit(h: HWND) {
    if !EDITS.with(|v| v.borrow_mut().insert(h.0 as isize)) {
        return;
    }
    unsafe {
        let _ = SetWindowSubclass(h, Some(edit_proc), FIELD, 0);
    }
    strip_frame(h);
    layout(h);
}
fn multi(h: HWND) -> bool {
    unsafe { GetWindowLongPtrW(h, GWL_STYLE) & ES_MULTILINE as isize != 0 }
}
fn layout(h: HWND) {
    let t = EditorTheme::current(editor_ui::window_dpi(h));
    if multi(h) {
        let mut r = super::super::workspace_ui::rect(h);
        r.left += t.scale(8);
        r.top += t.scale(6);
        r.right -= t.scale(14);
        r.bottom -= t.scale(6);
        message(h, EM_SETRECTNP, 0, (&raw const r) as isize);
    } else {
        message(
            h,
            EM_SETMARGINS,
            3,
            (t.scale(8) | (t.scale(8) << 16)) as isize,
        );
    }
    invalidate(h);
}
#[derive(Clone, Copy)]
struct Scroll {
    total: i32,
    page: i32,
    first: i32,
    height: i32,
    thumb: i32,
    y: i32,
    width: i32,
}
fn metrics(h: HWND, list: bool) -> Scroll {
    let t = EditorTheme::current(editor_ui::window_dpi(h));
    let r = super::super::workspace_ui::rect(h);
    let (total, page, first) = if list {
        let row = message(h, LB_GETITEMHEIGHT, 0, 0).max(t.scale(24) as isize) as i32;
        (
            message(h, LB_GETCOUNT, 0, 0) as i32,
            (r.bottom / row).max(1),
            message(h, LB_GETTOPINDEX, 0, 0) as i32,
        )
    } else {
        let dc = unsafe { Gdi::GetDC(Some(h)) };
        let font = message(h, WM_GETFONT, 0, 0);
        let old = unsafe { Gdi::SelectObject(dc, Gdi::HGDIOBJ(font as *mut _)) };
        let mut tm = Gdi::TEXTMETRICW::default();
        unsafe {
            let _ = Gdi::GetTextMetricsW(dc, &raw mut tm);
            Gdi::SelectObject(dc, old);
            Gdi::ReleaseDC(Some(h), dc);
        }
        let line = (tm.tmHeight + tm.tmExternalLeading).max(t.scale(16));
        (
            message(h, EM_GETLINECOUNT, 0, 0) as i32,
            ((r.bottom - t.scale(12)) / line).max(1),
            message(h, EM_GETFIRSTVISIBLELINE, 0, 0) as i32,
        )
    };
    let height = (r.bottom - t.scale(8)).max(1);
    let thumb = (height * page.max(1) / total.max(1)).clamp(t.scale(24).min(height), height);
    let first = first.max(0);
    Scroll {
        total,
        page,
        first,
        height,
        thumb,
        y: t.scale(4) + (height - thumb) * first / (total - page).max(1),
        width: t.scale(10),
    }
}
fn scroll_to(h: HWND, list: bool, first: i32) {
    let s = metrics(h, list);
    let first = first.clamp(0, (s.total - s.page).max(0));
    if list {
        message(h, LB_SETTOPINDEX, first as usize, 0);
    } else {
        message(h, EM_LINESCROLL, 0, (first - s.first) as isize);
    }
    invalidate(h);
}
fn draw_scroll(h: HWND, dc: HDC, list: bool) {
    let t = EditorTheme::current(editor_ui::window_dpi(h));
    let r = super::super::workspace_ui::rect(h);
    let s = metrics(h, list);
    if s.total <= s.page { return; }
    editor_ui::fill_rect(
        dc,
        &rect(r.right - s.width, 0, s.width, r.bottom),
        t.palette.raised,
    );
    if s.total > s.page {
        let thumb = rect(
            r.right - s.width + t.scale(2),
            s.y,
            t.scale(4).max(3),
            s.thumb,
        );
        editor_ui::rounded_surface(
            dc,
            thumb,
            if t.mode == super::ThemeMode::Dark {
                editor_ui::rgb(103, 110, 121)
            } else {
                editor_ui::rgb(144, 152, 165)
            },
            if t.mode == super::ThemeMode::Dark {
                editor_ui::rgb(103, 110, 121)
            } else {
                editor_ui::rgb(144, 152, 165)
            },
            t.scale(2),
        );
    }
}
fn handle_scroll(h: HWND, m: u32, l: LPARAM, list: bool) -> bool {
    // Querying line/item counts sends messages back through this subclass. Only
    // pointer gestures may enter the metrics path, otherwise they recurse.
    if matches!(m, WM_CAPTURECHANGED | WM_NCDESTROY) {
        DRAGS.with(|v| {
            v.borrow_mut().remove(&(h.0 as isize));
        });
        return false;
    }
    if !matches!(m, WM_LBUTTONDOWN | WM_MOUSEMOVE | WM_LBUTTONUP) {
        return false;
    }
    let x = l.0 as i16 as i32;
    let y = (l.0 >> 16) as i16 as i32;
    let r = super::super::workspace_ui::rect(h);
    let s = metrics(h, list);
    match m {
        WM_LBUTTONDOWN if x >= r.right - s.width && s.total > s.page => {
            if y >= s.y && y < s.y + s.thumb {
                DRAGS.with(|d| {
                    d.borrow_mut().insert(
                        h.0 as isize,
                        Drag {
                            start: y,
                            first: s.first,
                        },
                    );
                });
                unsafe {
                    SetCapture(h);
                }
            } else {
                scroll_to(h, list, s.first + if y < s.y { -s.page } else { s.page });
            }
            true
        }
        WM_MOUSEMOVE => {
            let drag = DRAGS.with(|d| d.borrow().get(&(h.0 as isize)).copied());
            if let Some(drag) = drag {
                scroll_to(
                    h,
                    list,
                    drag.first
                        + (y - drag.start) * (s.total - s.page) / (s.height - s.thumb).max(1),
                );
                true
            } else {
                false
            }
        }
        WM_LBUTTONUP => {
            if DRAGS
                .with(|d| d.borrow_mut().remove(&(h.0 as isize)))
                .is_some()
            {
                unsafe {
                    let _ = ReleaseCapture();
                }
                true
            } else {
                false
            }
        }
        WM_CAPTURECHANGED | WM_NCDESTROY => {
            DRAGS.with(|d| {
                d.borrow_mut().remove(&(h.0 as isize));
            });
            false
        }
        _ => false,
    }
}
fn edit_menu(h: HWND, l: LPARAM) {
    use crate::platform::{
        app_menu::{self, Row},
        editor_ui::IconKind as I,
    };
    use windows::Win32::UI::Controls::{EM_CANUNDO, EM_GETSEL, EM_SETSEL};
    let mut start = 0u32;
    let mut end = 0u32;
    message(
        h,
        EM_GETSEL,
        (&raw mut start) as usize,
        (&raw mut end) as isize,
    );
    let writable = unsafe { GetWindowLongPtrW(h, GWL_STYLE) } & ES_READONLY as isize == 0;
    let selected = start != end;
    let rows = vec![
        Row::command(
            1,
            "撤销",
            I::Undo,
            writable && message(h, EM_CANUNDO, 0, 0) != 0,
        ),
        Row::separator(),
        Row::command(2, "剪切", I::Cut, writable && selected),
        Row::command(3, "复制", I::Copy, selected),
        Row::command(4, "粘贴", I::Paste, writable),
        Row::separator(),
        Row::command(
            5,
            "全选",
            I::Cursor,
            !crate::platform::workspace_ui::value(h).is_empty(),
        ),
    ];
    let mut point = windows::Win32::Foundation::POINT {
        x: l.0 as i16 as i32,
        y: (l.0 >> 16) as i16 as i32,
    };
    if l.0 == -1 {
        point.x = 8;
        point.y = 12;
    } else {
        unsafe {
            let _ = Gdi::ScreenToClient(h, &raw mut point);
        }
    }
    if let Ok(Some(command)) = app_menu::show_rows(
        h,
        rect(point.x, point.y, 0, 0),
        EditorTheme::current(editor_ui::window_dpi(h)),
        rows,
        false,
    ) {
        match command {
            1 => {
                message(h, WM_UNDO, 0, 0);
            }
            2 => {
                message(h, WM_CUT, 0, 0);
            }
            3 => {
                message(h, WM_COPY, 0, 0);
            }
            4 => {
                message(h, WM_PASTE, 0, 0);
            }
            5 => {
                message(h, EM_SETSEL, 0, -1);
            }
            _ => {}
        }
    }
}
#[derive(Clone, Copy)]
struct NumberField {
    min: f64,
    max: f64,
    step: f64,
    hover: bool,
    held: i64,
}
thread_local! {static NUMBERS:RefCell<HashMap<isize,NumberField>>=RefCell::new(HashMap::new());}
const SPIN_TIMER: usize = 0x5041_5a15;
pub(crate) fn number_field(h: HWND, min: i64, max: i64, step: i64) {
    decimal_field(h, min as f64, max as f64, step as f64);
}
pub(crate) fn decimal_field(h: HWND, min: f64, max: f64, step: f64) {
    edit(h);
    let first = NUMBERS.with(|v| {
        let mut v = v.borrow_mut();
        let first = !v.contains_key(&(h.0 as isize));
        let n = v.entry(h.0 as isize).or_insert(NumberField {
            min,
            max,
            step,
            hover: false,
            held: 0,
        });
        n.min = min;
        n.max = max;
        n.step = step;
        first
    });
    if first {
        unsafe {
            let _ = SetWindowPos(
                h,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }
}
fn step_number(h: HWND, direction: i64) {
    let Some(n) = NUMBERS.with(|v| v.borrow().get(&(h.0 as isize)).copied()) else {
        return;
    };
    if !unsafe { windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(h) }.as_bool() {
        return;
    }
    let old = crate::platform::workspace_ui::value(h)
        .parse::<f64>()
        .unwrap_or(n.min);
    let value = (old + direction as f64 * n.step).clamp(n.min, n.max);
    let precision=if n.step.fract()==0.0 {0} else if (n.step*10.0).fract()==0.0 {1} else if (n.step*100.0).fract()==0.0 {2} else {3};
    crate::platform::workspace_ui::text(h, &format!("{value:.precision$}"));
    message(h, windows::Win32::UI::Controls::EM_SETSEL, usize::MAX, -1);
    paint_edit_padding(h);
}
fn pointer_in_field(h: HWND) -> bool {
    let mut p = windows::Win32::Foundation::POINT::default();
    let mut r = windows::Win32::Foundation::RECT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
        let _ = GetWindowRect(h, &mut r);
    }
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}
fn number_message(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> Option<LRESULT> {
    use windows::Win32::UI::Controls::WM_MOUSELEAVE;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetFocus, SetFocus, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent,
    };
    if !NUMBERS.with(|v| v.borrow().contains_key(&(h.0 as isize))) {
        return None;
    }
    if matches!(
        m,
        WM_MOUSEMOVE
            | WM_NCMOUSEMOVE
            | WM_MOUSELEAVE
            | WM_NCMOUSELEAVE
            | WM_SETFOCUS
            | WM_KILLFOCUS
    ) {
        let hover = pointer_in_field(h);
        NUMBERS.with(|v| {
            if let Some(n) = v.borrow_mut().get_mut(&(h.0 as isize)) {
                n.hover = hover;
            }
        });
        if matches!(m, WM_MOUSEMOVE | WM_NCMOUSEMOVE) {
            let mut track = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: if m == WM_NCMOUSEMOVE {
                    TME_LEAVE | TME_NONCLIENT
                } else {
                    TME_LEAVE
                },
                hwndTrack: h,
                ..Default::default()
            };
            unsafe {
                let _ = TrackMouseEvent(&mut track);
            }
        }
        paint_edit_padding(h);
    }
    if m == WM_NCHITTEST {
        let mut r = windows::Win32::Foundation::RECT::default();
        unsafe {
            let _ = GetWindowRect(h, &mut r);
        }
        let x = l.0 as i16 as i32;
        if x >= r.right - editor_ui::scale_dip(14, editor_ui::window_dpi(h)) {
            return Some(LRESULT(HTBORDER as isize));
        }
    }
    if m == WM_NCLBUTTONDOWN {
        let mut r = windows::Win32::Foundation::RECT::default();
        unsafe {
            let _ = GetWindowRect(h, &mut r);
            let _ = SetFocus(Some(h));
            SetCapture(h);
        }
        let direction = if ((l.0 >> 16) as i16 as i32) < (r.top + r.bottom) / 2 {
            1
        } else {
            -1
        };
        NUMBERS.with(|v| {
            if let Some(n) = v.borrow_mut().get_mut(&(h.0 as isize)) {
                n.held = direction;
            }
        });
        step_number(h, direction);
        unsafe {
            SetTimer(Some(h), SPIN_TIMER, 400, None);
        }
        return Some(LRESULT(0));
    }
    if m == WM_TIMER && w.0 == SPIN_TIMER {
        let held = NUMBERS.with(|v| v.borrow().get(&(h.0 as isize)).map_or(0, |n| n.held));
        if held != 0 && pointer_in_field(h) {
            step_number(h, held);
            unsafe {
                SetTimer(Some(h), SPIN_TIMER, 80, None);
            }
        }
        return Some(LRESULT(0));
    }
    if matches!(
        m,
        WM_LBUTTONUP | WM_NCLBUTTONUP | WM_CAPTURECHANGED | WM_CANCELMODE | WM_NCDESTROY
    ) {
        let held = NUMBERS.with(|v| {
            v.borrow_mut()
                .get_mut(&(h.0 as isize))
                .map(|n| {
                    let held = n.held;
                    n.held = 0;
                    held
                })
                .unwrap_or(0)
        });
        if held != 0 {
            unsafe {
                let _ = KillTimer(Some(h), SPIN_TIMER);
                let _ = ReleaseCapture();
            }
        }
    }
    if m == WM_KEYDOWN && matches!(w.0, 0x26 | 0x28) && unsafe { GetFocus() } == h {
        step_number(h, if w.0 == 0x26 { 1 } else { -1 });
        return Some(LRESULT(0));
    }
    if m == WM_NCDESTROY {
        NUMBERS.with(|v| v.borrow_mut().remove(&(h.0 as isize)));
    }
    None
}
// A single-line EDIT ignores EM_SETRECT. Center its native client line by
// reserving symmetric non-client padding, preserving caret, selection and IME.
fn edit_line_height(h: HWND) -> i32 {
    let dc = unsafe { Gdi::GetDC(Some(h)) };
    let font = message(h, WM_GETFONT, 0, 0);
    let old = if font != 0 {
        Some(unsafe { Gdi::SelectObject(dc, Gdi::HGDIOBJ(font as *mut _)) })
    } else {
        None
    };
    let mut metrics = Gdi::TEXTMETRICW::default();
    unsafe {
        let _ = Gdi::GetTextMetricsW(dc, &mut metrics);
        if let Some(old) = old {
            Gdi::SelectObject(dc, old);
        }
        Gdi::ReleaseDC(Some(h), dc);
    }
    metrics.tmHeight.max(1)
}
fn paint_edit_padding(h: HWND) {
    let mut outer = windows::Win32::Foundation::RECT::default();
    if unsafe { GetWindowRect(h, &mut outer) }.is_err() {
        return;
    }
    let mut origin = windows::Win32::Foundation::POINT::default();
    unsafe {
        let _ = Gdi::ClientToScreen(h, &mut origin);
    }
    let client = crate::platform::workspace_ui::rect(h);
    let dc = unsafe { Gdi::GetWindowDC(Some(h)) };
    if dc.is_invalid() {
        return;
    }
    unsafe {
        Gdi::ExcludeClipRect(
            dc,
            origin.x - outer.left,
            origin.y - outer.top,
            origin.x - outer.left + client.right,
            origin.y - outer.top + client.bottom,
        );
    }
    let theme = EditorTheme::current(editor_ui::window_dpi(h));
    editor_ui::fill_rect(
        dc,
        &rect(0, 0, outer.right - outer.left, outer.bottom - outer.top),
        theme.palette.raised,
    );
    if let Some(n) = NUMBERS.with(|v| v.borrow().get(&(h.0 as isize)).copied()) {
        if n.hover || unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() } == h {
            let d = |v| theme.scale(v);
            let height = outer.bottom - outer.top;
            let x = outer.right - outer.left - d(12);
            let value = crate::platform::workspace_ui::value(h)
                .parse::<f64>()
                .unwrap_or(n.min);
            let ink = |enabled| {
                if enabled {
                    theme.palette.text_secondary
                } else {
                    theme.palette.text_muted
                }
            };
            editor_ui::draw_icon(
                dc,
                rect(x, height / 2 - d(8), d(10), d(10)),
                editor_ui::IconKind::Prototype(
                    "<path d='M5 15 12 8 19 15Z' fill='currentColor' stroke='none'/>",
                ),
                ink(value < n.max),
            );
            editor_ui::draw_icon(
                dc,
                rect(x, height / 2 - d(2), d(10), d(10)),
                editor_ui::IconKind::Prototype(
                    "<path d='M5 9 12 16 19 9Z' fill='currentColor' stroke='none'/>",
                ),
                ink(value > n.min),
            );
        }
    }
    unsafe {
        Gdi::ReleaseDC(Some(h), dc);
    }
}
unsafe extern "system" fn edit_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    if let Some(result) = crate::platform::app_menu::window_message(m, l) {
        return result;
    }
    if let Some(result) = number_message(h, m, w, l) {
        return result;
    }
    if !multi(h) {
        if m == WM_NCCALCSIZE && l.0 != 0 {
            let result = unsafe { DefSubclassProc(h, m, w, l) };
            let r = if w.0 != 0 {
                unsafe { &mut (*(l.0 as *mut NCCALCSIZE_PARAMS)).rgrc[0] }
            } else {
                unsafe { &mut *(l.0 as *mut windows::Win32::Foundation::RECT) }
            };
            let height = (r.bottom - r.top).max(1);
            let line = edit_line_height(h).min(height);
            r.top += (height - line) / 2;
            r.bottom = r.top + line;
            if NUMBERS.with(|v| v.borrow().contains_key(&(h.0 as isize))) {
                r.right =
                    (r.right - editor_ui::scale_dip(14, editor_ui::window_dpi(h))).max(r.left + 1);
            }
            return result;
        }
        if m == WM_NCPAINT {
            paint_edit_padding(h);
            return LRESULT(0);
        }
        if m == WM_NCHITTEST {
            return LRESULT(HTCLIENT as isize);
        }
    }
    if m == windows::Win32::UI::Controls::EM_SETCUEBANNER && l.0 != 0 {
        if let Ok(value) = unsafe { windows::core::PCWSTR(l.0 as *const u16).to_string() } {
            CUES.with(|v| v.borrow_mut().insert(h.0 as isize, value));
            invalidate(h);
        }
    }
    if m == WM_CONTEXTMENU {
        edit_menu(h, l);
        return LRESULT(0);
    }
    if multi(h) && handle_scroll(h, m, l, false) {
        return LRESULT(0);
    }
    if m == WM_NCDESTROY {
        unsafe {
            CUES.with(|v| v.borrow_mut().remove(&(h.0 as isize)));
            EDITS.with(|v| {
                v.borrow_mut().remove(&(h.0 as isize));
            });
            let _ = RemoveWindowSubclass(h, Some(edit_proc), id);
        }
    }
    if m == WM_SETFOCUS {
        super::super::workspace::remember_text_focus(h);
    }
    let result = unsafe { DefSubclassProc(h, m, w, l) };
    // EN_CHANGE can synchronously relayout the parent during native text updates.
    // Queue a final client repaint after that nested work, so the value cannot
    // remain blank until focus/hover sends another paint message.
    if matches!(m, WM_SETTEXT | WM_CHAR | EM_REPLACESEL) {
        invalidate(h);
    }
    if matches!(m, WM_SIZE | WM_SETFONT) {
        layout(h);
    }
    if m == WM_SETFONT && !multi(h) {
        unsafe {
            let _ = SetWindowPos(
                h,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }
    if m == WM_PAINT && !multi(h) {
        paint_edit_padding(h);
    }
    if m == WM_PAINT && !multi(h) && crate::platform::workspace_ui::value(h).is_empty() {
        if let Some(cue) = CUES.with(|v| v.borrow().get(&(h.0 as isize)).cloned()) {
            let dc = unsafe { Gdi::GetDC(Some(h)) };
            let t = EditorTheme::current(editor_ui::window_dpi(h));
            let mut r = crate::platform::workspace_ui::rect(h);
            r.left += t.scale(8);
            text(dc, r, &cue, t.palette.text_muted, t.dpi, false);
            unsafe {
                Gdi::ReleaseDC(Some(h), dc);
            }
        }
    }
    if multi(h)
        && unsafe { IsWindowVisible(h) }.as_bool()
        && matches!(
            m,
            WM_PAINT
                | WM_VSCROLL
                | WM_MOUSEWHEEL
                | WM_KEYUP
                | WM_CHAR
                | WM_SETTEXT
                | EM_REPLACESEL
                | EM_LINESCROLL
                | WM_SETFOCUS
                | WM_KILLFOCUS
        )
    {
        let dc = unsafe { Gdi::GetDC(Some(h)) };
        draw_scroll(h, dc, false);
        unsafe {
            Gdi::ReleaseDC(Some(h), dc);
        }
    }
    result
}
pub(super) fn dropdown(combo: HWND) {
    let mut info = COMBOBOXINFO {
        cbSize: std::mem::size_of::<COMBOBOXINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetComboBoxInfo(combo, &raw mut info) }.is_err() || info.hwndList.is_invalid() {
        return;
    }
    unsafe {
        let _ = SetWindowSubclass(info.hwndList, Some(list_proc), LIST, combo.0 as usize);
    }
    let first = unsafe { GetWindowLongPtrW(info.hwndList, GWL_STYLE) }
        & (WS_BORDER | WS_VSCROLL).0 as isize
        != 0;
    if first {
        strip_frame(info.hwndList);
    }
    message(
        combo,
        CB_SETITEMHEIGHT,
        0,
        EditorTheme::current(editor_ui::window_dpi(combo)).scale(30) as isize,
    );
}
unsafe extern "system" fn list_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    combo: usize,
) -> LRESULT {
    if m == WM_MOUSEWHEEL {
        let delta = (w.0 >> 16) as i16 as i32;
        let remainder = WHEELS.with(|v| {
            let mut v = v.borrow_mut();
            let n = v.entry(h.0 as isize).or_default();
            *n += delta;
            let steps = *n / 120;
            *n %= 120;
            steps
        });
        if remainder != 0 {
            let first = message(h, LB_GETTOPINDEX, 0, 0) as i32;
            scroll_to(h, true, first - remainder * 3);
        }
        return LRESULT(0);
    }
    if handle_scroll(h, m, l, true) {
        return LRESULT(0);
    }
    if m == WM_ERASEBKGND || m == WM_NCPAINT {
        return LRESULT(1);
    }
    if m == WM_PAINT || m == WM_PRINTCLIENT {
        let mut ps = Gdi::PAINTSTRUCT::default();
        let dc = if m == WM_PAINT {
            unsafe { Gdi::BeginPaint(h, &raw mut ps) }
        } else {
            HDC(w.0 as *mut _)
        };
        let r = super::super::workspace_ui::rect(h);
        let frame = super::BufferedFrame::new(dc, r);
        let dc = frame.dc;
        let t = EditorTheme::current(editor_ui::window_dpi(HWND(combo as *mut _)));
        editor_ui::fill_rect(dc, &r, t.palette.raised);
        let s = metrics(h, true);
        let row = message(h, LB_GETITEMHEIGHT, 0, 0).max(t.scale(24) as isize) as i32;
        let selected = message(h, LB_GETCURSEL, 0, 0) as i32;
        for index in s.first..s.total {
            let y = (index - s.first) * row;
            if y >= r.bottom {
                break;
            }
            let len = message(HWND(combo as *mut _), CB_GETLBTEXTLEN, index as usize, 0);
            if len < 0 {
                continue;
            }
            let mut label = vec![0u16; len as usize + 1];
            message(
                HWND(combo as *mut _),
                CB_GETLBTEXT,
                index as usize,
                label.as_mut_ptr() as isize,
            );
            let item = rect(t.scale(3), y + 1, r.right - s.width - t.scale(6), row - 2);
            if index == selected {
                editor_ui::rounded_surface(dc, item, t.palette.hover, t.palette.hover, t.scale(3));
            }
            let icon = super::combo_glyph(HWND(combo as *mut _));
            if let Some(icon) = icon {
                editor_ui::draw_icon(
                    dc,
                    rect(
                        t.scale(10),
                        y + (row - t.scale(14)) / 2,
                        t.scale(14),
                        t.scale(14),
                    ),
                    icon,
                    t.palette.text_secondary,
                );
            }
            let left = t.scale(if icon.is_some() { 32 } else { 10 });
            text(
                dc,
                rect(left, y, r.right - s.width - left - t.scale(8), row),
                &String::from_utf16_lossy(&label[..len as usize]),
                t.palette.text,
                t.dpi,
                false,
            );
        }
        draw_scroll(h, dc, true);
        drop(frame);
        if m == WM_PAINT {
            unsafe {
                let _ = Gdi::EndPaint(h, &raw const ps);
            }
        }
        return LRESULT(0);
    }
    if m == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(h, Some(list_proc), id);
        }
    }
    let result = unsafe { DefSubclassProc(h, m, w, l) };
    if matches!(
        m,
        LB_ADDSTRING
            | LB_INSERTSTRING
            | LB_DELETESTRING
            | LB_RESETCONTENT
            | LB_SETCOUNT
            | LB_SETITEMHEIGHT
            | WM_SIZE
            | WM_SHOWWINDOW
    ) {
        hide_native_scroll(h);
    }
    if matches!(
        m,
        WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_KEYDOWN | WM_MOUSEWHEEL | LB_SETCURSEL | LB_SETTOPINDEX
    ) {
        invalidate(h);
    }
    result
}

/// Retain an owner-drawn list's item renderer while replacing its native scrollbar.
pub(crate) fn list(h: HWND) {
    unsafe {
        let _ = SetWindowSubclass(h, Some(plain_list_proc), LIST + 1, 0);
    }
    strip_frame(h);
}
unsafe extern "system" fn plain_list_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    if m == WM_ERASEBKGND { return LRESULT(1); }
    if m == WM_PAINT {
        let mut ps = Gdi::PAINTSTRUCT::default();
        let target = unsafe { Gdi::BeginPaint(h, &mut ps) };
        let r = crate::platform::workspace_ui::rect(h);
        let frame = crate::platform::desktop_ui::BufferedFrame::new(target, r);
        let t = EditorTheme::current(editor_ui::window_dpi(h));
        editor_ui::fill_rect(frame.dc, &r, t.palette.panel);
        unsafe {
            DefSubclassProc(h, WM_PRINTCLIENT, WPARAM(frame.dc.0 as usize), LPARAM(PRF_CLIENT as isize));
        }
        draw_scroll(h, frame.dc, true);
        drop(frame);
        unsafe { let _ = Gdi::EndPaint(h, &ps); }
        return LRESULT(0);
    }
    if handle_scroll(h, m, l, true) {
        return LRESULT(0);
    }
    if m == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(h, Some(plain_list_proc), id);
        }
    }
    let result = unsafe { DefSubclassProc(h, m, w, l) };
    if matches!(
        m,
        LB_ADDSTRING
            | LB_INSERTSTRING
            | LB_DELETESTRING
            | LB_RESETCONTENT
            | LB_SETCOUNT
            | LB_SETITEMHEIGHT
            | WM_SIZE
    ) {
        hide_native_scroll(h);
    }
    if unsafe { IsWindowVisible(h) }.as_bool()
        && matches!(
            m,
            WM_PAINT | WM_VSCROLL | WM_MOUSEWHEEL | WM_KEYUP | LB_SETTOPINDEX | WM_SIZE
        )
    {
        // Scrollbar and rows must arrive together; never paint from WM_SIZE using GetDC.
        invalidate(h);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "system" fn review_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(h, m, w, l) }
    }
    #[test]
    #[ignore = "native desktop review; requires isolated LOCALAPPDATA and PANZO_UI_REVIEW_DIR"]
    fn native_fields_and_expanded_lists_keep_theme_and_scroll() {
        use crate::platform::{
            capture_tools,
            ui_preferences::{self, ThemeMode},
            workspace_ui as ui,
        };
        use windows::core::w;
        let out = std::path::PathBuf::from(
            std::env::var_os("PANZO_UI_REVIEW_DIR").expect("review output"),
        );
        std::fs::create_dir_all(&out).unwrap();
        ui::register(w!("PanzoControlReview"), Some(review_proc)).unwrap();
        let h = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("PanzoControlReview"),
                w!("Panzo 控件状态验收"),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                70,
                70,
                510,
                650,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        let source = (0..100)
            .map(|n| format!("第 {n:02} 行：识别结果可直接校对、选择和复制。\r\n"))
            .collect::<String>();
        let field = ui::edit(h, &source, 1, true).unwrap();
        ui::place(field, 16, 70, 450, 490);
        let values = (0..30)
            .map(|n| format!("选项 {n:02} · 原生键盘选择"))
            .collect::<Vec<_>>();
        let combo = ui::combo(
            h,
            &values.iter().map(String::as_str).collect::<Vec<_>>(),
            3,
            2,
        )
        .unwrap();
        ui::place(combo, 16, 16, 450, 32);
        let single = ui::edit(h, "960", 3, false).unwrap();
        let numeric = ui::number(h, "540", &["256", "540", "960"], 4, None).unwrap();
        number_field(single, 1, 1000, 1);
        for dpi in [96, 120, 144, 192] {
            let scale = |n| editor_ui::scale_dip(n, dpi);
            let font = editor_ui::font_handle(editor_ui::TextStyle::Body, dpi).0 as usize;
            message(single, WM_SETFONT, font, 0);
            message(numeric, WM_SETFONT, font, 0);
            ui::place(single, 16, 570, scale(100), scale(28));
            ui::place(numeric, 240, 570, scale(100), scale(30));
            let mut info = COMBOBOXINFO {
                cbSize: std::mem::size_of::<COMBOBOXINFO>() as u32,
                ..Default::default()
            };
            unsafe {
                GetComboBoxInfo(numeric, &mut info).unwrap();
            }
            message(info.hwndItem, WM_SETFONT, font, 0);
            for (owner, edit) in [(single, single), (numeric, info.hwndItem)] {
                let mut outer = windows::Win32::Foundation::RECT::default();
                let mut origin = windows::Win32::Foundation::POINT::default();
                unsafe {
                    GetWindowRect(owner, &mut outer).unwrap();
                    let _ = Gdi::ClientToScreen(edit, &mut origin);
                }
                let height = edit_line_height(edit);
                assert!(
                    (2 * (origin.y - outer.top) + height - (outer.bottom - outer.top)).abs() <= 2,
                    "native text line off center at {dpi} DPI: owner={owner:?} edit={edit:?} outer={outer:?} origin={origin:?} line={height}"
                );
            }
        }
        for field in [single, numeric] {
            message(
                field,
                WM_SETFONT,
                editor_ui::font_handle(editor_ui::TextStyle::Body, 96).0 as usize,
                0,
            );
        }
        ui::place(single, 16, 570, 100, 28);
        ui::place(numeric, 240, 570, 100, 30);
        message(single, windows::Win32::UI::Controls::EM_SETSEL, 0, -1);
        message(single, WM_CHAR, '7' as usize, 0);
        assert_eq!(
            ui::value(single),
            "7",
            "centered field retains native editing"
        );
        ui::text(single, "960");
        unsafe {
            let _ = ShowWindow(h, SW_SHOW);
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(single));
        }
        message(single, WM_KEYDOWN, 0x26, 0);
        assert_eq!(ui::value(single), "961");
        ui::text(single, "1000");
        message(single, WM_KEYDOWN, 0x26, 0);
        assert_eq!(ui::value(single), "1000");
        let mut spin = windows::Win32::Foundation::RECT::default();
        unsafe {
            GetWindowRect(single, &mut spin).unwrap();
        }
        let click = ((spin.bottom - 5) as u32 & 0xffff) << 16 | ((spin.right - 6) as u32 & 0xffff);
        message(single, WM_NCLBUTTONDOWN, HTBORDER as usize, click as isize);
        message(single, WM_LBUTTONUP, 0, 0);
        assert_eq!(ui::value(single), "999", "lower spin arrow decrements");
        ui::text(single, "1");
        message(single, WM_KEYDOWN, 0x28, 0);
        assert_eq!(ui::value(single), "1");
        decimal_field(single,0.0,2.0,0.1);
        ui::text(single,"1.0");
        message(single,WM_KEYDOWN,0x26,0);
        assert_eq!(ui::value(single),"1.1");
        message(single,WM_KEYDOWN,0x28,0);
        assert_eq!(ui::value(single),"1");
        ui::text(single,"2");message(single,WM_KEYDOWN,0x26,0);
        assert_eq!(ui::value(single),"2");
        number_field(single,1,1000,1);
        ui::text(single, "960");

        assert_eq!(
            unsafe { GetWindowLongPtrW(field, GWL_STYLE) } & (WS_BORDER | WS_VSCROLL).0 as isize,
            0
        );
        assert_eq!(
            unsafe { GetWindowLongPtrW(field, GWL_EXSTYLE) } & WS_EX_CLIENTEDGE.0 as isize,
            0
        );
        message(field, EM_LINESCROLL, 0, 30);
        assert!(
            message(field, EM_GETFIRSTVISIBLELINE, 0, 0) > 0,
            "borderless edit still scrolls"
        );
        scroll_to(field, false, 0);
        assert_eq!(message(field, EM_GETFIRSTVISIBLELINE, 0, 0), 0);
        let old = ui_preferences::theme();
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            ui_preferences::set_theme(mode).unwrap();
            super::super::refresh(h);
            unsafe {
                let _ = ShowWindow(h, SW_SHOW);
                let _ = Gdi::UpdateWindow(h);
            }
            let suffix = if mode == ThemeMode::Dark {
                "dark"
            } else {
                "light"
            };
            capture_tools::review_window_png(h, &out.join(format!("native-fields-{suffix}.png")));
            message(combo, CB_SHOWDROPDOWN, 1, 0);
            let mut info = COMBOBOXINFO {
                cbSize: std::mem::size_of::<COMBOBOXINFO>() as u32,
                ..Default::default()
            };
            unsafe {
                GetComboBoxInfo(combo, &raw mut info).unwrap();
                let _ = Gdi::UpdateWindow(info.hwndList);
            }
            assert!(unsafe { IsWindowVisible(info.hwndList) }.as_bool());
            assert_eq!(
                unsafe { GetWindowLongPtrW(info.hwndList, GWL_STYLE) }
                    & (WS_BORDER | WS_VSCROLL).0 as isize,
                0
            );
            capture_tools::review_window_png(
                info.hwndList,
                &out.join(format!("native-dropdown-{suffix}.png")),
            );
            scroll_to(info.hwndList, true, 12);
            assert!(message(info.hwndList, LB_GETTOPINDEX, 0, 0) > 0);
            message(combo, CB_SHOWDROPDOWN, 0, 0);
        }
        unsafe {
            let _ = DestroyWindow(h);
        }
        ui_preferences::set_theme(old).unwrap();
    }
}

#[derive(Clone, Copy)]
struct PanelScroll {
    total: i32,
    page: i32,
    position: i32,
    drag: Option<Drag>,
}
thread_local! {static PANELS:RefCell<HashMap<isize,PanelScroll>>=RefCell::new(HashMap::new());}
/// Application pixel-scrolling panels use this instead of SetScrollInfo. No
/// native non-client scrollbar is created, including after content/layout changes.
pub(crate) fn scroll_panel(h: HWND, total: i32, page: i32, position: i32) {
    let unchanged = PANELS.with(|v| v.borrow().get(&(h.0 as isize)).is_some_and(|s|
        (s.total,s.page,s.position)==(total.max(0),page.max(1),position.clamp(0,(total-page).max(0)))));
    if unchanged { return; }
    PANELS.with(|v| {
        let mut v = v.borrow_mut();
        let drag = v.get(&(h.0 as isize)).and_then(|s| s.drag);
        v.insert(
            h.0 as isize,
            PanelScroll {
                total: total.max(0),
                page: page.max(1),
                position: position.clamp(0, (total - page).max(0)),
                drag,
            },
        );
    });
    unsafe {
        let _ = SetWindowSubclass(h, Some(panel_proc), LIST + 2, 0);
    }
    hide_native_scroll(h);
    invalidate(h);
}
pub(crate) fn panel_scroll_info(h: HWND) -> (i32, i32, i32) {
    PANELS.with(|v| {
        v.borrow()
            .get(&(h.0 as isize))
            .map_or((0, 1, 0), |s| (s.total, s.page, s.position))
    })
}
fn panel_move(h: HWND, value: i32) {
    PANELS.with(|v| {
        if let Some(s) = v.borrow_mut().get_mut(&(h.0 as isize)) {
            s.position = value.clamp(0, (s.total - s.page).max(0));
        }
    });
    unsafe {
        let _ = PostMessageW(
            Some(h),
            WM_VSCROLL,
            WPARAM(SB_THUMBTRACK.0 as usize),
            LPARAM(0),
        );
    }
    invalidate(h);
}
unsafe extern "system" fn panel_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    if m == WM_NCDESTROY {
        PANELS.with(|v| {
            v.borrow_mut().remove(&(h.0 as isize));
        });
        unsafe {
            let _ = RemoveWindowSubclass(h, Some(panel_proc), id);
        }
        return unsafe { DefSubclassProc(h, m, w, l) };
    }
    if matches!(
        m,
        WM_LBUTTONDOWN
            | WM_MOUSEMOVE
            | WM_LBUTTONUP
            | WM_CAPTURECHANGED
            | WM_PAINT
            | WM_PRINTCLIENT
    ) {
        let state = PANELS.with(|v| v.borrow().get(&(h.0 as isize)).copied());
        if let Some(s) = state {
            let t = EditorTheme::current(editor_ui::window_dpi(h));
            let r = super::super::workspace_ui::rect(h);
            let x = l.0 as i16 as i32;
            let y = (l.0 >> 16) as i16 as i32;
            let height = (r.bottom - t.scale(8)).max(1);
            let thumb = (height * s.page / s.total.max(1)).clamp(t.scale(24).min(height), height);
            let top = t.scale(4) + (height - thumb) * s.position / (s.total - s.page).max(1);
            if m == WM_LBUTTONDOWN && x >= r.right - t.scale(10) && s.total > s.page {
                if y >= top && y < top + thumb {
                    PANELS.with(|v| {
                        if let Some(p) = v.borrow_mut().get_mut(&(h.0 as isize)) {
                            p.drag = Some(Drag {
                                start: y,
                                first: s.position,
                            });
                        }
                    });
                    unsafe {
                        SetCapture(h);
                    }
                } else {
                    panel_move(h, s.position + if y < top { -s.page } else { s.page });
                }
                return LRESULT(0);
            }
            if m == WM_MOUSEMOVE
                && let Some(drag) = s.drag
            {
                panel_move(
                    h,
                    drag.first + (y - drag.start) * (s.total - s.page) / (height - thumb).max(1),
                );
                return LRESULT(0);
            }
            if matches!(m, WM_LBUTTONUP | WM_CAPTURECHANGED) && s.drag.is_some() {
                PANELS.with(|v| {
                    if let Some(p) = v.borrow_mut().get_mut(&(h.0 as isize)) {
                        p.drag = None;
                    }
                });
                if m == WM_LBUTTONUP {
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                }
                return LRESULT(0);
            }
            if m == WM_PAINT || m == WM_PRINTCLIENT {
                let result = unsafe { DefSubclassProc(h, m, w, l) };
                let dc = if m == WM_PRINTCLIENT {
                    HDC(w.0 as *mut _)
                } else {
                    unsafe { Gdi::GetDC(Some(h)) }
                };
                editor_ui::fill_rect(
                    dc,
                    &rect(r.right - t.scale(10), 0, t.scale(10), r.bottom),
                    t.palette.panel,
                );
                if s.total > s.page {
                    let color = if t.mode == super::ThemeMode::Dark {
                        editor_ui::rgb(103, 110, 121)
                    } else {
                        editor_ui::rgb(144, 152, 165)
                    };
                    editor_ui::rounded_surface(
                        dc,
                        rect(r.right - t.scale(8), top, t.scale(4), thumb),
                        color,
                        color,
                        t.scale(2),
                    );
                }
                if m == WM_PAINT {
                    unsafe {
                        Gdi::ReleaseDC(Some(h), dc);
                    }
                }
                return result;
            }
        }
    }
    unsafe { DefSubclassProc(h, m, w, l) }
}

thread_local! {static WHEELS:RefCell<HashMap<isize,i32>>=RefCell::new(HashMap::new());}
