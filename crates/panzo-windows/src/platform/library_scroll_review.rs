//! Native regression: observe intermediate scrollbar paints, not only settled state.
use super::*;
use std::cell::{Cell, RefCell};
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{GetCapture, SetCapture},
    Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
};
const PROBE: usize = 0x4c535243;
#[derive(Clone, Copy, Debug, Default)]
struct Samples {
    messages: u32,
    paints: u32,
    nonclient: u32,
    native_bar: u32,
    white: u32,
}
thread_local! {
    static SAMPLES:RefCell<Samples>=RefCell::new(Samples::default());
    static SAMPLING:Cell<bool>=const {Cell::new(false)};
}
unsafe extern "system" fn observe(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    let result = unsafe { DefSubclassProc(h, m, w, l) };
    if !matches!(
        m,
        WM_SETREDRAW
            | WM_NCPAINT
            | WM_ERASEBKGND
            | WM_STYLECHANGED
            | WM_PAINT
            | LB_SETTOPINDEX
            | WM_MOUSEMOVE
            | WM_MOUSEWHEEL
    ) {
        return result;
    }
    if SAMPLING.with(|v| v.replace(true)) {
        return result;
    }
    let native = unsafe { GetWindowLongPtrW(h, GWL_STYLE) } & WS_VSCROLL.0 as isize != 0;
    let mut white = false;
    if editor_ui::EditorTheme::current(editor_ui::window_dpi(h)).mode
        == crate::platform::ui_preferences::ThemeMode::Dark
    {
        let r = controls::rect(h);
        let dc = unsafe { GetDC(Some(h)) };
        for y in (0..r.bottom).step_by(80) {
            let c = unsafe { GetPixel(dc, (r.right - 5).max(0), y) }.0;
            white |= c != u32::MAX
                && (c & 255) > 235
                && ((c >> 8) & 255) > 235
                && ((c >> 16) & 255) > 235;
        }
        unsafe {
            ReleaseDC(Some(h), dc);
        }
    }
    SAMPLES.with(|v| {
        let mut s = v.borrow_mut();
        s.messages += 1;
        s.paints += u32::from(m == WM_PAINT);
        s.nonclient += u32::from(m == WM_NCPAINT);
        s.native_bar += u32::from(native);
        s.white += u32::from(white);
    });
    SAMPLING.with(|v| v.set(false));
    result
}
fn at(x: i32, y: i32) -> LPARAM {
    LPARAM(((y as u16 as u32) << 16 | x as u16 as u32) as isize)
}
fn drag_to(parent: HWND, s: &History, x: i32) {
    let mut p = POINT { x, y: s.dip(160) };
    unsafe {
        let _ = ClientToScreen(parent, &mut p);
        let _ = ScreenToClient(s.splitter, &mut p);
        SendMessageW(s.splitter, WM_MOUSEMOVE, None, Some(at(p.x, p.y)));
    }
}
pub(super) fn run(parent: HWND, s: &mut History) {
    let mut report = String::new();
    for grid in [false, true] {
        s.command(parent, if grid { 30 } else { 31 }).unwrap();
        let surface = if grid { s.grid } else { s.list };
        assert!(unsafe { IsWindowVisible(surface) }.as_bool());
        unsafe {
            SendMessageW(surface, WM_VSCROLL, Some(WPARAM(SB_TOP.0 as usize)), None);
            assert!(SetWindowSubclass(surface, Some(observe), PROBE, 0).as_bool());
        }
        SAMPLES.with(|v| *v.borrow_mut() = Samples::default());
        let (thumb, _, _) = s.review_scrollbar(surface);
        unsafe {
            SendMessageW(
                surface,
                WM_LBUTTONDOWN,
                None,
                Some(at(thumb.left + 1, thumb.top + 2)),
            );
            for n in 0..40 {
                SendMessageW(
                    surface,
                    WM_MOUSEMOVE,
                    None,
                    Some(at(
                        thumb.left + 1,
                        8 + n * 37 % (controls::rect(surface).bottom - 16),
                    )),
                );
                if n % 12 == 0 {
                    SendMessageW(parent, WM_TIMER, Some(WPARAM(81)), None);
                }
            }
            SendMessageW(surface, WM_LBUTTONUP, None, None);
            for n in 0..40 {
                let delta = if n % 20 < 10 { -120i16 } else { 120i16 };
                SendMessageW(
                    surface,
                    WM_MOUSEWHEEL,
                    Some(WPARAM((delta as u16 as usize) << 16)),
                    None,
                );
            }
            let _ = RemoveWindowSubclass(surface, Some(observe), PROBE);
        }
        assert!(unsafe { IsWindowVisible(surface) }.as_bool());
        let samples = SAMPLES.with(|v| *v.borrow());
        report.push_str(&format!("grid={grid}: {samples:?}\n"));
        assert!(samples.paints > 0, "probe must observe actual repaints");
        assert_eq!(
            samples.native_bar, 0,
            "native scrollbars must never revive mid-message"
        );
        assert_eq!(
            samples.nonclient, 0,
            "scrolling must not repaint native frame chrome"
        );
        assert_eq!(
            samples.white, 0,
            "no intermediate white scrollbar rectangle"
        );
    }
    // A press without movement must not resize; moving a native child must not
    // shift the drag origin. Exercise successful release, capture loss and Escape.
    let saved = s.inspector_width;
    s.inspector_width = Some(304);
    s.layout(parent);
    let x = s.geometry(parent).right - s.dip(6) + s.dip(2);
    unsafe {
        SendMessageW(
            s.splitter,
            WM_LBUTTONDOWN,
            None,
            Some(at(s.dip(2), s.dip(60))),
        );
    }
    drag_to(parent, s, x);
    assert_eq!(s.inspector_width, Some(304));
    drag_to(parent, s, x - s.dip(80));
    assert!((s.inspector_width.unwrap() - 384).abs() <= 1);
    unsafe {
        SendMessageW(s.splitter, WM_LBUTTONUP, None, None);
    }
    assert!(s.splitter_drag.is_none());
    assert_ne!(unsafe { GetCapture() }, s.splitter);
    let committed = s.inspector_width;
    for original in [committed, None] {
        s.inspector_width = original;
        s.layout(parent);
        let x = s.geometry(parent).right - s.dip(6);
        unsafe {
            SendMessageW(s.splitter, WM_LBUTTONDOWN, None, None);
        }
        drag_to(parent, s, x - s.dip(40));
        unsafe {
            SetCapture(parent);
        }
        assert!(s.splitter_drag.is_none());
        assert_eq!(s.inspector_width, original);
        assert_eq!(
            unsafe { GetCapture() },
            parent,
            "cancel must not release someone else's capture"
        );
        ui::release_capture_owned_by(parent);
    }
    let x = s.geometry(parent).right - s.dip(6);
    unsafe {
        SendMessageW(s.splitter, WM_LBUTTONDOWN, None, None);
    }
    drag_to(parent, s, x - s.dip(50));
    assert!(dialog_message(&MSG {
        hwnd: s.splitter,
        message: WM_KEYDOWN,
        wParam: WPARAM(27),
        ..Default::default()
    }));
    assert!(s.splitter_drag.is_none());
    assert_eq!(s.inspector_width, None);
    s.inspector_width = saved;
    s.layout(parent);
    report.push_str("PASS: shared divider press/move/release, parent-space origin, capture loss, Escape, original automatic-width restoration.\n");
    eprintln!("{report}");
    if let Some(out) = std::env::var_os("PANZO_LIBRARY_SCROLL_REVIEW_DIR") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(PathBuf::from(out).join("scroll-and-splitter.txt"), report).unwrap();
    }
}
