//! Physical desktop selection, window snapping, magnification and direct output.
use super::{
    BeginPaint, COLORREF, CaptureIntent, CaptureOutcome, CreateWindowExW, DefWindowProcW,
    DestroyWindow, EndPaint, GWLP_USERDATA, GetCursorPos, GetKeyState, GetModuleHandleW,
    GetWindowLongPtrW, HDC, HWND, IDC_CROSS, LPARAM, LRESULT, LoadCursorW, OUTCOME, PAINTSTRUCT,
    POINT, PhysicalRect, PostMessageW, RECT, Raster, RegisterClassW, ReleaseCapture, SELECTING,
    SW_HIDE, SW_SHOW, SetCapture, SetFocus, SetForegroundWindow, SetWindowLongPtrW, ShowWindow,
    Surface, VK_CONTROL, VK_SHIFT, WM_CAPTURE_COMPLETE, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT, WNDCLASSW, WPARAM, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP, copy_image, copy_text, error, fill, frame, invalidate, pin_manager,
    point, screen, text, w,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BitBlt, GdiFlush, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowRect, IsIconic, IsWindowVisible, WM_DISPLAYCHANGE,
};
use windows::core::BOOL;

#[derive(Clone, Copy)]
struct Snap {
    rect: PhysicalRect,
}
unsafe extern "system" fn collect_window(hwnd: HWND, param: LPARAM) -> BOOL {
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() || unsafe { IsIconic(hwnd) }.as_bool() {
        return BOOL(1);
    }
    let mut cloaked = 0u32;
    unsafe {
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            size_of::<u32>() as u32,
        );
    }
    if cloaked != 0 {
        return BOOL(1);
    }
    let mut rect = RECT::default();
    if unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut rect).cast(),
            size_of::<RECT>() as u32,
        )
    }
    .is_err()
    {
        unsafe {
            let _ = GetWindowRect(hwnd, &raw mut rect);
        }
    }
    if rect.right > rect.left && rect.bottom > rect.top {
        unsafe { &mut *(param.0 as *mut Vec<Snap>) }.push(Snap {
            rect: PhysicalRect {
                x: rect.left,
                y: rect.top,
                width: rect.right.abs_diff(rect.left),
                height: rect.bottom.abs_diff(rect.top),
            },
        });
    }
    BOOL(1)
}
fn intersect(a: PhysicalRect, b: PhysicalRect) -> Option<PhysicalRect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (i64::from(a.x) + i64::from(a.width)).min(i64::from(b.x) + i64::from(b.width));
    let bottom = (i64::from(a.y) + i64::from(a.height)).min(i64::from(b.y) + i64::from(b.height));
    (right > i64::from(left) && bottom > i64::from(top)).then_some(PhysicalRect {
        x: left,
        y: top,
        width: (right - i64::from(left)) as u32,
        height: (bottom - i64::from(top)) as u32,
    })
}
#[derive(Clone, Copy)]
enum Drag {
    New(POINT),
    Move(POINT, PhysicalRect),
    Resize(POINT, PhysicalRect, u8),
}
struct Selection {
    owner: HWND,
    owner_visible: bool,
    desktop: PhysicalRect,
    image: Raster,
    dim: Raster,
    buffer: Surface,
    intent: CaptureIntent,
    bounds: Option<PhysicalRect>,
    drag: Option<Drag>,
    manual: bool,
    pointer: POINT,
    snaps: Vec<Snap>,
}
pub(super) fn begin(owner: HWND, intent: CaptureIntent) -> Result<(), String> {
    if SELECTING.with(|v| *v.borrow()) {
        return Err("请先完成当前选区".into());
    }
    let visible = unsafe { IsWindowVisible(crate::platform::workspace::window(owner)) }.as_bool();
    unsafe {
        let _ = ShowWindow(crate::platform::workspace::window(owner), SW_HIDE);
        let _ = windows::Win32::Graphics::Dwm::DwmFlush();
    }
    let result = (|| {
        let mut snaps = Vec::new();
        unsafe {
            let _ = EnumWindows(
                Some(collect_window),
                LPARAM(std::ptr::from_mut(&mut snaps) as isize),
            );
        }
        let (desktop, image) = screen()?;
        let mut dim = image.clone();
        for pixel in dim.pixels.chunks_exact_mut(4) {
            for channel in &mut pixel[..3] {
                *channel /= 2;
            }
        }
        // Allocate before creating the window so a failure restores the owner normally.
        let buffer = Surface::new(image.width, image.height)?;
        let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
        unsafe {
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(selection_proc),
                hInstance: module.into(),
                lpszClassName: w!("PanzoCaptureSelection"),
                hCursor: LoadCursorW(None, IDC_CROSS).map_err(|e| e.to_string())?,
                ..Default::default()
            });
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("PanzoCaptureSelection"),
                w!("Panzo 选择区域"),
                WS_POPUP,
                desktop.x,
                desktop.y,
                desktop.width as i32,
                desktop.height as i32,
                None,
                None,
                Some(module.into()),
                None,
            )
        }
        .map_err(|e| e.to_string())?;
        let mut pointer = POINT::default();
        unsafe {
            let _ = GetCursorPos(&raw mut pointer);
        }
        pointer.x -= desktop.x;
        pointer.y -= desktop.y;
        let mut state = Box::new(Selection {
            owner,
            owner_visible: visible,
            desktop,
            image,
            dim,
            buffer,
            intent,
            bounds: None,
            drag: None,
            manual: false,
            pointer,
            snaps,
        });
        state.hover();
        SELECTING.with(|v| *v.borrow_mut() = true);
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
        }
        Ok(())
    })();
    if result.is_err() && visible {
        unsafe {
            let _ = ShowWindow(crate::platform::workspace::window(owner), SW_SHOW);
        }
    }
    result
}
impl Selection {
    fn absolute(&self, rect: PhysicalRect) -> PhysicalRect {
        PhysicalRect {
            x: rect.x + self.desktop.x,
            y: rect.y + self.desktop.y,
            ..rect
        }
    }
    fn relative(&self, rect: PhysicalRect) -> Option<PhysicalRect> {
        intersect(rect, self.desktop).map(|rect| PhysicalRect {
            x: rect.x - self.desktop.x,
            y: rect.y - self.desktop.y,
            ..rect
        })
    }
    fn hover(&mut self) {
        let pointer = panzo_core::PhysicalPoint {
            x: self.pointer.x + self.desktop.x,
            y: self.pointer.y + self.desktop.y,
        };
        self.bounds = self
            .snaps
            .iter()
            .find(|candidate| candidate.rect.contains(pointer))
            .and_then(|candidate| self.relative(candidate.rect));
    }
    fn toolbar(&self) -> Option<RECT> {
        let rect = self.bounds?;
        if !self.manual || self.drag.is_some() {
            return None;
        }
        let width = 420.min(self.image.width as i32);
        let left = rect.x.clamp(0, self.image.width as i32 - width);
        let top = (rect.y + rect.height as i32 + 8)
            .min(self.image.height as i32 - 42)
            .max(0);
        Some(RECT {
            left,
            top,
            right: left + width,
            bottom: top + 34,
        })
    }
    fn action(&mut self, hwnd: HWND, action: usize) {
        if action == 6 {
            self.manual = false;
            self.bounds = None;
            self.hover();
            invalidate(hwnd);
            return;
        }
        let Some(rect) = self.bounds.filter(|r| r.width >= 1 && r.height >= 1) else {
            return;
        };
        let absolute = self.absolute(rect);
        let last = [
            i64::from(absolute.x),
            i64::from(absolute.y),
            i64::from(absolute.width),
            i64::from(absolute.height),
        ];
        if let Err(e) = pin_manager::save_last_region(last) {
            error(hwnd, &e);
            return;
        }
        if action == 5 {
            OUTCOME.with(|out| {
                *out.borrow_mut() = Some(if matches!(self.intent, CaptureIntent::Scroll) {
                    CaptureOutcome::Scroll(absolute)
                } else {
                    CaptureOutcome::Region(absolute)
                });
            });
        } else {
            let image = match self.image.crop(rect) {
                Ok(image) => image,
                Err(e) => {
                    error(hwnd, &e);
                    return;
                }
            };
            let result = match action {
                2 => copy_image(hwnd, &image)
                    .and_then(|()| pin_manager::remember_image(&image).map(|_| ())),
                3 => pin_manager::remember_image(&image).map(|_| ()),
                4 => {
                    OUTCOME.with(|out| *out.borrow_mut() = Some(CaptureOutcome::Pin(image)));
                    Ok(())
                }
                _ => {
                    OUTCOME.with(|out| {
                        *out.borrow_mut() = Some(if matches!(self.intent, CaptureIntent::Ocr) {
                            CaptureOutcome::Ocr(image)
                        } else {
                            CaptureOutcome::Image(image)
                        });
                    });
                    Ok(())
                }
            };
            if let Err(e) = result {
                error(hwnd, &e);
                return;
            }
        }
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
    fn drag_update(&mut self) {
        let point = self.pointer;
        let Some(drag) = self.drag else {
            return;
        };
        self.bounds = match drag {
            Drag::New(start) => Some(PhysicalRect {
                x: start.x.min(point.x),
                y: start.y.min(point.y),
                width: start.x.abs_diff(point.x),
                height: start.y.abs_diff(point.y),
            }),
            Drag::Move(start, rect) => Some(PhysicalRect {
                x: (rect.x + point.x - start.x)
                    .clamp(0, self.image.width as i32 - rect.width as i32),
                y: (rect.y + point.y - start.y)
                    .clamp(0, self.image.height as i32 - rect.height as i32),
                ..rect
            }),
            Drag::Resize(start, rect, edges) => {
                let dx = point.x - start.x;
                let dy = point.y - start.y;
                let mut left = rect.x;
                let mut top = rect.y;
                let mut right = rect.x + rect.width as i32;
                let mut bottom = rect.y + rect.height as i32;
                if edges & 1 != 0 {
                    left = (left + dx).clamp(0, right - 1);
                }
                if edges & 2 != 0 {
                    right = (right + dx).clamp(left + 1, self.image.width as i32);
                }
                if edges & 4 != 0 {
                    top = (top + dy).clamp(0, bottom - 1);
                }
                if edges & 8 != 0 {
                    bottom = (bottom + dy).clamp(top + 1, self.image.height as i32);
                }
                Some(PhysicalRect {
                    x: left,
                    y: top,
                    width: (right - left) as u32,
                    height: (bottom - top) as u32,
                })
            }
        };
    }
    fn pixel(&self) -> [u8; 3] {
        let offset = (self.pointer.y.clamp(0, self.image.height as i32 - 1) as usize
            * self.image.width as usize
            + self.pointer.x.clamp(0, self.image.width as i32 - 1) as usize)
            * 4;
        [
            self.image.pixels[offset + 2],
            self.image.pixels[offset + 1],
            self.image.pixels[offset],
        ]
    }
    fn compose(&mut self) {
        // Finish queued GDI reads/writes before reusing the DIB's memory. The frozen
        // desktop and dim mask are captured once; moving the mouse never recaptures it.
        unsafe {
            let _ = GdiFlush();
        }
        let pixels = unsafe { std::slice::from_raw_parts_mut(self.buffer.bits, self.buffer.bytes) };
        pixels.copy_from_slice(&self.dim.pixels);
        if let Some(rect) = self.bounds.filter(|r| r.width > 0 && r.height > 0) {
            let stride = self.image.width as usize * 4;
            let row_bytes = rect.width as usize * 4;
            for y in rect.y as usize..rect.y as usize + rect.height as usize {
                let start = y * stride + rect.x as usize * 4;
                pixels[start..start + row_bytes]
                    .copy_from_slice(&self.image.pixels[start..start + row_bytes]);
            }
        }
        self.paint_overlays(self.buffer.dc);
    }
    fn paint(&mut self, dc: HDC, dirty: RECT) {
        self.compose();
        // This is the only write to the window DC: never expose the dim-only frame
        // between drawing the mask, selection, toolbar and magnifier.
        unsafe {
            let _ = BitBlt(
                dc,
                dirty.left,
                dirty.top,
                dirty.right - dirty.left,
                dirty.bottom - dirty.top,
                Some(self.buffer.dc),
                dirty.left,
                dirty.top,
                SRCCOPY,
            );
        }
    }
    fn paint_overlays(&self, dc: HDC) {
        if let Some(rect) = self.bounds.filter(|r| r.width > 0 && r.height > 0) {
            let bounds = RECT {
                left: rect.x,
                top: rect.y,
                right: rect.x + rect.width as i32,
                bottom: rect.y + rect.height as i32,
            };
            frame(dc, bounds, COLORREF(0x00DD_9933));
            let label = RECT {
                left: rect.x.max(0),
                top: (rect.y - 30).max(0),
                right: (rect.x + 420).min(self.image.width as i32),
                bottom: (rect.y - 30).max(0) + 28,
            };
            fill(dc, label, COLORREF(0x0030_3030));
            text(
                dc,
                label,
                &format!(
                    " {} × {}  · {}",
                    rect.width,
                    rect.height,
                    if self.manual {
                        "方向键微调 / 拖边调整"
                    } else {
                        "点击选择窗口 / 拖动框选"
                    }
                ),
                COLORREF(0x00FF_FFFF),
            );
        }
        if let Some(toolbar) = self.toolbar() {
            fill(dc, toolbar, COLORREF(0x0030_3030));
            for (index, label) in [
                if matches!(self.intent, CaptureIntent::Ocr) {
                    "识别"
                } else {
                    "标注"
                },
                "复制",
                "保存",
                "贴图",
                if matches!(self.intent, CaptureIntent::Scroll) {
                    "长图"
                } else {
                    "录屏"
                },
                "重选",
            ]
            .iter()
            .enumerate()
            {
                text(
                    dc,
                    RECT {
                        left: toolbar.left + index as i32 * 70 + 10,
                        top: toolbar.top + 5,
                        right: toolbar.left + (index as i32 + 1) * 70,
                        bottom: toolbar.bottom,
                    },
                    label,
                    COLORREF(0x00FF_FFFF),
                );
            }
        }
        let side = 132
            .min(self.image.width as i32)
            .min(self.image.height as i32);
        let left = (self.pointer.x + 24)
            .min(self.image.width as i32 - side)
            .max(0);
        let top = (self.pointer.y + 28)
            .min(self.image.height as i32 - side - 66)
            .max(0);
        fill(
            dc,
            RECT {
                left,
                top,
                right: left + side,
                bottom: top + side + 66,
            },
            COLORREF(0x0022_2222),
        );
        let px = self.pointer.x;
        let py = self.pointer.y;
        let cell = side / 11;
        for row in 0..11 {
            for col in 0..11 {
                let x = (px + col - 5).clamp(0, self.image.width as i32 - 1);
                let y = (py + row - 5).clamp(0, self.image.height as i32 - 1);
                let offset = (y as usize * self.image.width as usize + x as usize) * 4;
                let color = COLORREF(
                    u32::from(self.image.pixels[offset + 2])
                        | (u32::from(self.image.pixels[offset + 1]) << 8)
                        | (u32::from(self.image.pixels[offset]) << 16),
                );
                fill(
                    dc,
                    RECT {
                        left: left + col * cell,
                        top: top + row * cell,
                        right: left + (col + 1) * cell,
                        bottom: top + (row + 1) * cell,
                    },
                    color,
                );
            }
        }
        frame(
            dc,
            RECT {
                left: left + 5 * cell,
                top: top + 5 * cell,
                right: left + 6 * cell,
                bottom: top + 6 * cell,
            },
            COLORREF(0x0000_FFFF),
        );
        let [red, green, blue] = self.pixel();
        text(
            dc,
            RECT {
                left: left + 4,
                top: top + side + 2,
                right: left + side,
                bottom: top + side + 64,
            },
            &format!("#{red:02X}{green:02X}{blue:02X}\nC: HEX\nShift+C: RGB"),
            COLORREF(0x00FF_FFFF),
        );
    }
}
unsafe extern "system" fn selection_proc(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Selection;
    if pointer.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wp, lp) };
    }
    if message == WM_NCDESTROY {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        }
        let state = unsafe { Box::from_raw(pointer) };
        SELECTING.with(|v| *v.borrow_mut() = false);
        OUTCOME.with(|v| {
            if v.borrow().is_none() {
                *v.borrow_mut() = Some(CaptureOutcome::Cancelled);
            }
        });
        unsafe {
            if state.owner_visible {
                let _ = ShowWindow(crate::platform::workspace::window(state.owner), SW_SHOW);
            }
            let _ = PostMessageW(Some(state.owner), WM_CAPTURE_COMPLETE, WPARAM(0), LPARAM(0));
        }
        return unsafe { DefWindowProcW(hwnd, message, wp, lp) };
    }
    let state = unsafe { &mut *pointer };
    match message {
        WM_MOUSEMOVE => {
            let mut next = point(lp);
            next.x = next.x.clamp(0, state.image.width as i32 - 1);
            next.y = next.y.clamp(0, state.image.height as i32 - 1);
            // Windows may send mouse moves without a changed position. Do not turn
            // those into another full-desktop repaint while the pointer is idle.
            if next.x == state.pointer.x && next.y == state.pointer.y {
                return LRESULT(0);
            }
            state.pointer = next;
            if state.drag.is_some() {
                state.drag_update();
            } else if !state.manual {
                state.hover();
            }
            invalidate(hwnd);
        }
        WM_LBUTTONDOWN => {
            let p = point(lp);
            if let Some(bar) = state.toolbar()
                && p.x >= bar.left
                && p.x < bar.right
                && p.y >= bar.top
                && p.y < bar.bottom
            {
                state.action(hwnd, ((p.x - bar.left) / 70 + 1) as usize);
                return LRESULT(0);
            }
            let mut mode = Drag::New(p);
            if state.manual
                && let Some(rect) = state.bounds
            {
                let mut edges = 0;
                if (p.x - rect.x).abs() <= 6 {
                    edges |= 1;
                }
                if (p.x - rect.x - rect.width as i32).abs() <= 6 {
                    edges |= 2;
                }
                if (p.y - rect.y).abs() <= 6 {
                    edges |= 4;
                }
                if (p.y - rect.y - rect.height as i32).abs() <= 6 {
                    edges |= 8;
                }
                if p.x >= rect.x - 6
                    && p.x <= rect.x + rect.width as i32 + 6
                    && p.y >= rect.y - 6
                    && p.y <= rect.y + rect.height as i32 + 6
                {
                    mode = if edges == 0 {
                        Drag::Move(p, rect)
                    } else {
                        Drag::Resize(p, rect, edges)
                    };
                }
            }
            state.drag = Some(mode);
            unsafe {
                SetCapture(hwnd);
            }
        }
        WM_LBUTTONUP => {
            if let Some(Drag::New(start)) = state.drag
                && start.x.abs_diff(point(lp).x) + start.y.abs_diff(point(lp).y) < 4
            {
                state.hover();
            } else {
                state.pointer = point(lp);
                state.pointer.x = state.pointer.x.clamp(0, state.image.width as i32);
                state.pointer.y = state.pointer.y.clamp(0, state.image.height as i32);
                state.drag_update();
            }
            state.drag = None;
            state.manual = true;
            unsafe {
                let _ = ReleaseCapture();
            }
            invalidate(hwnd);
        }
        WM_KEYDOWN => {
            let ctrl = unsafe { GetKeyState(i32::from(VK_CONTROL.0)) } < 0;
            let shift = unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0;
            match wp.0 {
                27 => unsafe {
                    let _ = DestroyWindow(hwnd);
                },
                13 => state.action(
                    hwnd,
                    if matches!(state.intent, CaptureIntent::Region | CaptureIntent::Scroll) {
                        5
                    } else {
                        1
                    },
                ),
                0x52 => state.action(hwnd, 5),
                0x50 => state.action(hwnd, 4),
                0x53 => state.action(hwnd, 3),
                0x43 if ctrl => state.action(hwnd, 2),
                0x43 => {
                    let [r, g, b] = state.pixel();
                    let value = if shift {
                        format!("rgb({r}, {g}, {b})")
                    } else {
                        format!("#{r:02X}{g:02X}{b:02X}")
                    };
                    if let Err(e) = copy_text(hwnd, &value) {
                        error(hwnd, &e);
                    } else {
                        unsafe {
                            let _ = DestroyWindow(hwnd);
                        }
                    }
                }
                0x41 => {
                    state.bounds = Some(PhysicalRect {
                        x: 0,
                        y: 0,
                        width: state.image.width,
                        height: state.image.height,
                    });
                    state.manual = true;
                }
                0x46 => {
                    let monitor = unsafe {
                        MonitorFromPoint(
                            POINT {
                                x: state.pointer.x + state.desktop.x,
                                y: state.pointer.y + state.desktop.y,
                            },
                            MONITOR_DEFAULTTONEAREST,
                        )
                    };
                    let mut info = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    if unsafe { GetMonitorInfoW(monitor, &raw mut info) }.as_bool() {
                        state.bounds = state.relative(PhysicalRect {
                            x: info.rcMonitor.left,
                            y: info.rcMonitor.top,
                            width: info.rcMonitor.right.abs_diff(info.rcMonitor.left),
                            height: info.rcMonitor.bottom.abs_diff(info.rcMonitor.top),
                        });
                        state.manual = true;
                    }
                }
                0x4C => {
                    if let Some([x, y, width, height]) = pin_manager::load_last_region() {
                        state.bounds = state.relative(PhysicalRect {
                            x: x as i32,
                            y: y as i32,
                            width: width as u32,
                            height: height as u32,
                        });
                        state.manual = true;
                    }
                }
                0x25..=0x28 => {
                    if let Some(rect) = &mut state.bounds {
                        let (dx, dy) = match wp.0 {
                            0x25 => (-1, 0),
                            0x26 => (0, -1),
                            0x27 => (1, 0),
                            _ => (0, 1),
                        };
                        if shift {
                            rect.width = (rect.width as i32 + dx)
                                .clamp(1, state.image.width as i32 - rect.x)
                                as u32;
                            rect.height = (rect.height as i32 + dy)
                                .clamp(1, state.image.height as i32 - rect.y)
                                as u32;
                        } else {
                            rect.x = (rect.x + dx)
                                .clamp(0, state.image.width as i32 - rect.width as i32);
                            rect.y = (rect.y + dy)
                                .clamp(0, state.image.height as i32 - rect.height as i32);
                        }
                        state.manual = true;
                    }
                }
                _ => {}
            }
            if unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)) }.as_bool() {
                invalidate(hwnd);
            }
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(hwnd, &raw mut paint) };
            state.paint(dc, paint.rcPaint);
            unsafe {
                let _ = EndPaint(hwnd, &raw const paint);
            }
            return LRESULT(0);
        }
        WM_DISPLAYCHANGE => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        WM_ERASEBKGND => return LRESULT(1),
        _ => return unsafe { DefWindowProcW(hwnd, message, wp, lp) },
    }
    LRESULT(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Selection {
        let mut image = Raster {
            width: 800,
            height: 600,
            pixels: vec![255; 800 * 600 * 4],
        };
        for (index, pixel) in image.pixels.chunks_exact_mut(4).enumerate() {
            pixel[..3].copy_from_slice(&[
                80 + (index % 800 % 128) as u8,
                80 + (index / 800 % 128) as u8,
                200,
            ]);
        }
        let mut dim = image.clone();
        for pixel in dim.pixels.chunks_exact_mut(4) {
            for channel in &mut pixel[..3] {
                *channel /= 2;
            }
        }
        Selection {
            owner: HWND::default(),
            owner_visible: false,
            desktop: PhysicalRect {
                x: -800,
                y: -200,
                width: 800,
                height: 600,
            },
            image,
            dim,
            buffer: Surface::new(800, 600).unwrap(),
            intent: CaptureIntent::Screenshot,
            bounds: None,
            drag: None,
            manual: true,
            pointer: POINT { x: 0, y: 599 },
            snaps: Vec::new(),
        }
    }
    fn rgb(image: &Raster, x: usize, y: usize) -> &[u8] {
        let offset = (y * image.width as usize + x) * 4;
        &image.pixels[offset..offset + 3]
    }
    #[test]
    fn buffered_frames_restore_moved_and_resized_selections_without_ghosts() {
        let mut state = fixture();
        let target = Surface::new(800, 600).unwrap();
        let full = RECT {
            left: 0,
            top: 0,
            right: 800,
            bottom: 600,
        };
        let source = state.image.pixels.clone();
        for step in 0..60 {
            let x = 40 + (step % 3) * 220;
            let width = if step % 2 == 0 { 180 } else { 120 };
            state.bounds = Some(PhysicalRect {
                x,
                y: 100,
                width,
                height: 140,
            });
            state.paint(target.dc, full);
            let displayed = target.raster(800, 600);
            assert_eq!(displayed.pixels, state.buffer.raster(800, 600).pixels);
            assert_eq!(
                rgb(&displayed, x as usize + 50, 180),
                rgb(&state.image, x as usize + 50, 180)
            );
            // Old selections and the area removed by resizing must return to the mask.
            let outside = if x == 40 { 530 } else { 90 };
            assert_eq!(rgb(&displayed, outside, 180), rgb(&state.dim, outside, 180));
            if width == 120 {
                assert_eq!(
                    rgb(&displayed, x as usize + 150, 180),
                    rgb(&state.dim, x as usize + 150, 180)
                );
            }
            // Composing the next frame cannot alter the currently presented frame.
            state.bounds = None;
            state.compose();
            assert_eq!(target.raster(800, 600).pixels, displayed.pixels);
        }
        state.bounds = Some(PhysicalRect {
            x: 50,
            y: 100,
            width: 0,
            height: 0,
        });
        state.paint(target.dc, full);
        assert_eq!(
            rgb(&target.raster(800, 600), 90, 180),
            rgb(&state.dim, 90, 180)
        );
        assert_eq!(
            state.image.pixels, source,
            "selection feedback must not modify captured pixels"
        );
    }
    #[test]
    fn partial_exposure_presents_only_the_invalid_rectangle() {
        let mut state = fixture();
        let target = Surface::new(800, 600).unwrap();
        super::super::fill(
            target.dc,
            RECT {
                left: 0,
                top: 0,
                right: 800,
                bottom: 600,
            },
            COLORREF(0x0003_0201),
        );
        state.bounds = Some(PhysicalRect {
            x: 300,
            y: 100,
            width: 180,
            height: 140,
        });
        let dirty = RECT {
            left: 320,
            top: 120,
            right: 420,
            bottom: 220,
        };
        state.paint(target.dc, dirty);
        let displayed = target.raster(800, 600);
        for y in 0..600 {
            for x in 0..800 {
                if (320..420).contains(&x) && (120..220).contains(&y) {
                    assert_eq!(rgb(&displayed, x, y), rgb(&state.image, x, y));
                } else {
                    assert_eq!(rgb(&displayed, x, y), &[3, 2, 1]);
                }
            }
        }
    }
    #[test]
    fn stationary_mouse_messages_do_not_schedule_more_paints() {
        use windows::Win32::Graphics::Gdi::{GetUpdateRect, ValidateRect};
        use windows::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SendMessageW, WS_EX_NOACTIVATE,
            WS_VISIBLE,
        };
        // Keep this native window outside the virtual desktop so repaint tests are not visible.
        struct OffscreenSelector(HWND, Box<Selection>);
        impl Drop for OffscreenSelector {
            fn drop(&mut self) {
                unsafe {
                    SetWindowLongPtrW(self.0, GWLP_USERDATA, 0);
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        let module = unsafe { GetModuleHandleW(None) }.unwrap();
        unsafe {
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(selection_proc),
                hInstance: module.into(),
                lpszClassName: w!("PanzoSelectionRepaintTest"),
                ..Default::default()
            });
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("PanzoSelectionRepaintTest"),
                w!(""),
                WS_POPUP | WS_VISIBLE,
                GetSystemMetrics(SM_XVIRTUALSCREEN) - 801,
                GetSystemMetrics(SM_YVIRTUALSCREEN) - 601,
                800,
                600,
                None,
                None,
                Some(module.into()),
                None,
            )
        }
        .unwrap();
        let mut window = OffscreenSelector(hwnd, Box::new(fixture()));
        window.1.manual = false;
        window.1.snaps.push(Snap {
            rect: PhysicalRect {
                x: -750,
                y: -100,
                width: 200,
                height: 150,
            },
        });
        unsafe {
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                std::ptr::from_mut(window.1.as_mut()) as isize,
            );
        }
        let send = |x: i32, y: i32| unsafe {
            SendMessageW(
                hwnd,
                WM_MOUSEMOVE,
                Some(WPARAM(0)),
                Some(LPARAM(
                    ((y as u32 & 0xffff) << 16 | (x as u32 & 0xffff)) as isize,
                )),
            )
        };
        send(80, 150);
        assert!(unsafe { GetUpdateRect(hwnd, None, false) }.as_bool());
        assert_eq!(
            window.1.bounds,
            Some(PhysicalRect {
                x: 50,
                y: 100,
                width: 200,
                height: 150
            })
        );
        unsafe {
            let _ = ValidateRect(Some(hwnd), None);
        }
        for _ in 0..32 {
            send(80, 150);
        }
        assert!(!unsafe { GetUpdateRect(hwnd, None, false) }.as_bool());
        window.1.manual = true;
        window.1.drag = Some(Drag::New(POINT { x: 20, y: 30 }));
        send(240, 190);
        assert_eq!(
            window.1.bounds,
            Some(PhysicalRect {
                x: 20,
                y: 30,
                width: 220,
                height: 160
            })
        );
        assert!(unsafe { GetUpdateRect(hwnd, None, false) }.as_bool());
        unsafe {
            let _ = ValidateRect(Some(hwnd), None);
        }
        for _ in 0..32 {
            send(240, 190);
        }
        assert!(!unsafe { GetUpdateRect(hwnd, None, false) }.as_bool());
    }
    #[test]
    fn snap_intersection_uses_physical_negative_desktop_coordinates() {
        let desktop = PhysicalRect {
            x: -1920,
            y: -200,
            width: 4480,
            height: 1640,
        };
        let window = PhysicalRect {
            x: -2100,
            y: -100,
            width: 600,
            height: 900,
        };
        assert_eq!(
            intersect(window, desktop),
            Some(PhysicalRect {
                x: -1920,
                y: -100,
                width: 420,
                height: 900
            })
        );
        assert!(
            intersect(
                PhysicalRect {
                    x: 5000,
                    y: 0,
                    width: 100,
                    height: 100
                },
                desktop
            )
            .is_none()
        );
    }
}
