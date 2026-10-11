//! One nonactivating, click-through tooltip surface for native and painted controls.
use super::{
    desktop_ui as desk,
    editor_ui::{self, EditorTheme, TextStyle},
    workspace_ui as ui,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::*,
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
    core::w,
};
const OWNER_SUBCLASS: usize = 0x50415a82;
thread_local! {
    static WINDOWS:RefCell<HashMap<isize,HWND>>=RefCell::new(HashMap::new());
    static ACTIVE:Cell<HWND>=const{Cell::new(HWND(std::ptr::null_mut()))};
}
struct State {
    owner: HWND,
    root: HWND,
    anchor: RECT,
    screen_anchor: RECT,
    label: String,
    wrapped: String,
    shown: Instant,
    foreground: HWND,
    theme: EditorTheme,
}
unsafe fn state(h: HWND) -> Option<&'static mut State> {
    unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State).as_mut() }
}
fn screen_rect(h: HWND, mut r: RECT) -> RECT {
    let mut p = POINT::default();
    unsafe {
        let _ = ClientToScreen(h, &mut p);
    }
    r.left += p.x;
    r.right += p.x;
    r.top += p.y;
    r.bottom += p.y;
    r
}
fn contains(r: RECT, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}
/// Above first; beside the anchor at the top edge. Never follow the pointer.
fn placement(bounds: RECT, anchor: RECT, width: i32, height: i32, gap: i32) -> Option<RECT> {
    let width = width.min(bounds.right - bounds.left);
    let height = height.min(bounds.bottom - bounds.top);
    if width <= 0 || height <= 0 {
        return None;
    }
    let x = ((anchor.left + anchor.right - width) / 2).clamp(bounds.left, bounds.right - width);
    let y = ((anchor.top + anchor.bottom - height) / 2).clamp(bounds.top, bounds.bottom - height);
    let points = [
        (x, anchor.top - gap - height),
        (anchor.right + gap, y),
        (anchor.left - gap - width, y),
        (x, anchor.bottom + gap),
    ];
    points
        .into_iter()
        .map(|(x, y)| RECT {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        })
        .find(|r| {
            r.left >= bounds.left
                && r.right <= bounds.right
                && r.top >= bounds.top
                && r.bottom <= bounds.bottom
        })
}
fn wrap_text(dc: HDC, text: &str, width: i32) -> String {
    let mut result = String::new();
    let mut line = String::new();
    for c in text.chars() {
        if c == '\r' {
            continue;
        }
        if c == '\n' {
            result.push_str(&line);
            result.push('\n');
            line.clear();
            continue;
        }
        let mut next = line.clone();
        next.push(c);
        if !line.is_empty() && editor_ui::measure_text_width(dc, &next, TextStyle::Caption) > width
        {
            result.push_str(&line);
            result.push('\n');
            line.clear();
        }
        line.push(c);
    }
    result.push_str(&line);
    result
}
fn layout(s: &mut State) -> Option<RECT> {
    let t = s.theme;
    let d = |n| t.scale(n);
    let mut bounds = screen_rect(s.root, ui::rect(s.root));
    let monitor = unsafe { MonitorFromWindow(s.root, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        bounds.left = bounds.left.max(info.rcWork.left);
        bounds.top = bounds.top.max(info.rcWork.top);
        bounds.right = bounds.right.min(info.rcWork.right);
        bounds.bottom = bounds.bottom.min(info.rcWork.bottom);
    }
    bounds.left += d(6);
    bounds.top += d(6);
    bounds.right -= d(6);
    bounds.bottom -= d(6);
    let _scale = editor_ui::DrawingScale::enter(t.dpi);
    let dc = unsafe { GetDC(Some(s.owner)) };
    let width = (editor_ui::measure_text_width(dc, &s.label, TextStyle::Caption) + d(20))
        .clamp(d(48), d(380))
        .min(bounds.right - bounds.left);
    s.wrapped = wrap_text(dc, &s.label, (width - d(20)).max(1));
    let mut text = ui::wide(&s.wrapped);
    text.pop();
    let mut calc = RECT {
        right: (width - d(20)).max(1),
        ..Default::default()
    };
    let old = unsafe { SelectObject(dc, editor_ui::font_handle(TextStyle::Caption, t.dpi)) };
    unsafe {
        DrawTextW(
            dc,
            &mut text,
            &mut calc,
            DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
        );
        SelectObject(dc, old);
        ReleaseDC(Some(s.owner), dc);
    }
    let height = (calc.bottom + d(16)).max(d(30)).min(d(150));
    placement(bounds, s.screen_anchor, width, height, d(10))
}
pub(crate) fn show(owner: HWND, anchor: RECT, label: &str) {
    if label.trim().is_empty()
        || anchor.right <= anchor.left
        || anchor.bottom <= anchor.top
        || !unsafe { IsWindowVisible(owner) }.as_bool()
    {
        hide(owner);
        return;
    }
    let theme = EditorTheme::current(editor_ui::window_dpi(owner));
    let existing = WINDOWS.with(|v| v.borrow().get(&(owner.0 as isize)).copied());
    if let Some(h) = existing {
        if let Some(s) = unsafe { state(h) } {
            if s.anchor == anchor
                && s.label == label
                && s.theme.mode == theme.mode
                && s.theme.dpi == theme.dpi
            {
                return;
            }
        }
    }
    let root = unsafe { GetAncestor(owner, GA_ROOT) };
    let h = if let Some(h) = existing {
        h
    } else {
        if ui::register(w!("PanzoHoverTip"), Some(proc)).is_err() {
            return;
        }
        let Ok(h) = (unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                w!("PanzoHoverTip"),
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                Some(root),
                None,
                None,
                None,
            )
        }) else {
            return;
        };
        let s = Box::new(State {
            owner,
            root,
            anchor,
            screen_anchor: screen_rect(owner, anchor),
            label: String::new(),
            wrapped: String::new(),
            shown: Instant::now(),
            foreground: unsafe { GetForegroundWindow() },
            theme,
        });
        unsafe {
            SetWindowLongPtrW(h, GWLP_USERDATA, Box::into_raw(s) as isize);
            let _ = SetWindowSubclass(owner, Some(owner_proc), OWNER_SUBCLASS, 0);
        }
        WINDOWS.with(|v| v.borrow_mut().insert(owner.0 as isize, h));
        h
    };
    let Some(s) = (unsafe { state(h) }) else {
        return;
    };
    s.anchor = anchor;
    s.screen_anchor = screen_rect(owner, anchor);
    s.label = label.into();
    s.theme = theme;
    s.shown = Instant::now();
    s.foreground = unsafe { GetForegroundWindow() };
    let Some(r) = layout(s) else {
        hide(owner);
        return;
    };
    let previous = ACTIVE.with(|a| a.replace(h));
    if previous != h && unsafe { IsWindow(Some(previous)) }.as_bool() {
        unsafe {
            let _ = ShowWindow(previous, SW_HIDE);
        }
    }
    ui::text(h, label);
    unsafe {
        let _ = SetWindowPos(
            h,
            Some(HWND_TOPMOST),
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let region = CreateRoundRectRgn(
            0,
            0,
            r.right - r.left + 1,
            r.bottom - r.top + 1,
            theme.scale(8),
            theme.scale(8),
        );
        if !region.is_invalid() && SetWindowRgn(h, Some(region), true) == 0 {
            let _ = DeleteObject(region.into());
        }
        SetTimer(Some(h), 1, 50, None);
    }
    desk::invalidate(h);
}
pub(crate) fn hide(owner: HWND) {
    let h = WINDOWS.with(|v| v.borrow().get(&(owner.0 as isize)).copied());
    if let Some(h) = h {
        if let Some(s) = unsafe { state(h) } {
            s.label.clear();
        }
        unsafe {
            let _ = KillTimer(Some(h), 1);
            let _ = ShowWindow(h, SW_HIDE);
        }
        ACTIVE.with(|a| {
            if a.get() == h {
                a.set(HWND::default());
            }
        });
    }
}
pub(crate) fn refresh_label(owner: HWND) {
    let h = WINDOWS.with(|v| v.borrow().get(&(owner.0 as isize)).copied());
    if let Some(h) = h {
        if unsafe { IsWindowVisible(h) }.as_bool() {
            let anchor = unsafe { state(h) }.map(|s| s.anchor);
            if let Some(anchor) = anchor {
                show(owner, anchor, &ui::value(owner));
            }
        }
    }
}
fn paint(h: HWND, dc: HDC, s: &State) {
    let r = ui::rect(h);
    let t = s.theme;
    let _scale = editor_ui::DrawingScale::enter(t.dpi);
    let f = desk::BufferedFrame::new(dc, r);
    editor_ui::rounded_surface(f.dc, r, t.palette.raised, t.palette.divider, t.scale(4));
    let mut text = ui::wide(&s.wrapped);
    text.pop();
    let mut body = desk::rect(
        t.scale(10),
        t.scale(8),
        r.right - t.scale(20),
        r.bottom - t.scale(16),
    );
    unsafe {
        let old = SelectObject(f.dc, editor_ui::font_handle(TextStyle::Caption, t.dpi));
        SetBkMode(f.dc, TRANSPARENT);
        SetTextColor(f.dc, t.palette.text);
        DrawTextW(
            f.dc,
            &mut text,
            &mut body,
            DT_LEFT | DT_WORDBREAK | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
        SelectObject(f.dc, old);
    }
}
unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Some(s) = (unsafe { state(h) }) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    match m {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST => return LRESULT(HTTRANSPARENT as isize),
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &mut ps) };
            paint(h, dc, s);
            unsafe {
                let _ = EndPaint(h, &ps);
            }
            return LRESULT(0);
        }
        WM_PRINTCLIENT => {
            paint(h, HDC(w.0 as *mut _), s);
            return LRESULT(0);
        }
        WM_TIMER => {
            let foreground = unsafe { GetForegroundWindow() };
            // Hovering an inactive window is valid. Dismiss on an actual activation
            // change after showing, not merely because the owner is not foreground.
            if foreground != s.foreground
                || unsafe { IsIconic(s.root) }.as_bool()
                || !unsafe { IsWindowVisible(s.owner) }.as_bool()
                || screen_rect(s.owner, s.anchor) != s.screen_anchor
                || s.shown.elapsed() >= Duration::from_secs(6)
            {
                // Keep label/anchor until the next hover, so polling cannot re-open an expired tip.
                unsafe {
                    let _ = KillTimer(Some(h), 1);
                    let _ = ShowWindow(h, SW_HIDE);
                }
            }
            return LRESULT(0);
        }
        WM_NCDESTROY => {
            WINDOWS.with(|v| {
                v.borrow_mut().remove(&(s.owner.0 as isize));
            });
            ACTIVE.with(|a| {
                if a.get() == h {
                    a.set(HWND::default());
                }
            });
            unsafe {
                SetWindowLongPtrW(h, GWLP_USERDATA, 0);
                drop(Box::from_raw(std::ptr::from_mut(s)));
            }
        }
        _ => {}
    }
    unsafe { DefWindowProcW(h, m, w, l) }
}
unsafe extern "system" fn owner_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    if matches!(
        m,
        WM_LBUTTONDOWN
            | WM_RBUTTONDOWN
            | WM_MBUTTONDOWN
            | WM_KEYDOWN
            | WM_MOUSEWHEEL
    ) {
        hide(h);
    }
    if (m == WM_SHOWWINDOW && w.0 == 0) || m == WM_MOUSEMOVE {
        let tip = WINDOWS.with(|v| v.borrow().get(&(h.0 as isize)).copied());
        if let Some(s) = tip.and_then(|tip| unsafe { state(tip) }) {
            let p = POINT { x: l.0 as i16 as i32, y: (l.0 >> 16) as i16 as i32 };
            if m == WM_SHOWWINDOW || !contains(s.anchor, p) { hide(h); }
        }
    }
    if m == WM_NCDESTROY {
        let tip = WINDOWS.with(|v| v.borrow_mut().remove(&(h.0 as isize)));
        if let Some(tip) = tip {
            unsafe {
                let _ = DestroyWindow(tip);
            }
        }
        unsafe {
            let _ = RemoveWindowSubclass(h, Some(owner_proc), id);
        }
    }
    unsafe { DefSubclassProc(h, m, w, l) }
}
#[cfg(test)]
pub(crate) fn review_handle(owner: HWND) -> Option<HWND> {
    WINDOWS.with(|v| v.borrow().get(&(owner.0 as isize)).copied())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn placement_prefers_above_and_never_covers_anchor_across_dpi() {
        for dpi in [96, 120, 144, 192, 288] {
            let t = EditorTheme::for_dpi(dpi);
            let d = |n| t.scale(n);
            let b = desk::rect(-1800, 50, d(900), d(700));
            for (x, y) in [(20, 20), (820, 20), (20, 630), (820, 630), (400, 300)] {
                let a = desk::rect(b.left + d(x), b.top + d(y), d(28), d(28));
                let r = placement(b, a, d(160), d(32), d(10)).unwrap();
                assert!(
                    r.left >= b.left
                        && r.right <= b.right
                        && r.top >= b.top
                        && r.bottom <= b.bottom
                );
                assert!(
                    r.right <= a.left
                        || r.left >= a.right
                        || r.bottom <= a.top
                        || r.top >= a.bottom
                );
                if y >= 100 {
                    assert!(r.bottom < a.top);
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn review_diagnostics(owner: HWND) -> String {
    let tip=review_handle(owner).unwrap();let s=unsafe{state(tip)}.unwrap();
    let fg=unsafe{GetForegroundWindow()};
    format!("visible={} owner_visible={} root={:?} fg={:?} fg_root={:?} root_owner={:?} iconic={} anchor={:?} actual={:?} age={:?} label={:?}",unsafe{IsWindowVisible(tip)}.as_bool(),unsafe{IsWindowVisible(owner)}.as_bool(),s.root,fg,unsafe{GetAncestor(fg,GA_ROOTOWNER)},unsafe{GetAncestor(s.root,GA_ROOTOWNER)},unsafe{IsIconic(s.root)}.as_bool(),s.screen_anchor,screen_rect(s.owner,s.anchor),s.shown.elapsed(),s.label)
}
