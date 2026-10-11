//! Client-drawn frame shared only by the two asset-library dialogs.
use super::*;

pub(super) fn message(h: HWND, m: u32, l: LPARAM, title_height: i32) -> Option<LRESULT> {
    match m {
        // Handle this during CreateWindowExW too, before dialog state exists.
        // Keeping WS_THICKFRAME retains native sizing; no native frame is painted
        // above our own title bar because the entire window is client area.
        WM_NCCALCSIZE => Some(LRESULT(0)),
        WM_NCHITTEST => {
            let mut p = POINT {
                x: l.0 as u16 as i16 as i32,
                y: (l.0 >> 16) as u16 as i16 as i32,
            };
            unsafe {
                let _ = ScreenToClient(h, &mut p);
            }
            let r = controls::rect(h);
            let edge = controls::scale(h, 6);
            let left = p.x < edge;
            let right = p.x >= r.right - edge;
            let top = p.y < edge;
            let bottom = p.y >= r.bottom - edge;
            let hit = if p.x < 0 || p.y < 0 || p.x >= r.right || p.y >= r.bottom {
                HTNOWHERE
            } else if top && left {
                HTTOPLEFT
            } else if top && right {
                HTTOPRIGHT
            } else if bottom && left {
                HTBOTTOMLEFT
            } else if bottom && right {
                HTBOTTOMRIGHT
            } else if top {
                HTTOP
            } else if bottom {
                HTBOTTOM
            } else if left {
                HTLEFT
            } else if right {
                HTRIGHT
            } else if p.y < controls::scale(h, title_height)
                && p.x < r.right - controls::scale(h, 60)
            {
                HTCAPTION
            } else {
                HTCLIENT
            };
            Some(LRESULT(hit as isize))
        }
        _ => None,
    }
}

#[cfg(test)]
pub(super) fn review(h: HWND, title_height: i32) {
    fn check(h: HWND, title_height: i32) {
        let mut outer = RECT::default();
        let mut origin = POINT::default();
        unsafe {
            GetWindowRect(h, &mut outer).unwrap();
            assert!(ClientToScreen(h, &mut origin).as_bool());
        }
        let r = controls::rect(h);
        assert_eq!(
            (origin.x, origin.y),
            (outer.left, outer.top),
            "no extra non-client strip"
        );
        assert_eq!(
            (r.right, r.bottom),
            (outer.right - outer.left, outer.bottom - outer.top)
        );
        let d = |n| controls::scale(h, n);
        for (x, y, expected) in [
            (1, 1, HTTOPLEFT),
            (r.right / 2, 1, HTTOP),
            (r.right - 2, 1, HTTOPRIGHT),
            (1, r.bottom / 2, HTLEFT),
            (r.right - 2, r.bottom / 2, HTRIGHT),
            (1, r.bottom - 2, HTBOTTOMLEFT),
            (r.right / 2, r.bottom - 2, HTBOTTOM),
            (r.right - 2, r.bottom - 2, HTBOTTOMRIGHT),
            (d(80), d(20), HTCAPTION),
            (r.right - d(30), d(25), HTCLIENT),
            (r.right / 2, d(title_height + 12), HTCLIENT),
        ] {
            let point = (((origin.y + y) as u16 as u32) << 16) | (origin.x + x) as u16 as u32;
            let hit = unsafe { SendMessageW(h, WM_NCHITTEST, None, Some(LPARAM(point as isize))) };
            assert_eq!(hit.0, expected as isize, "popup hit at {x},{y}");
        }
    }
    check(h, title_height);
    let r = controls::rect(h);
    unsafe {
        SetWindowPos(
            h,
            None,
            0,
            0,
            r.right + 24,
            r.bottom + 16,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .unwrap();
    }
    check(h, title_height);
    unsafe {
        SetWindowPos(
            h,
            None,
            0,
            0,
            r.right,
            r.bottom,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .unwrap();
    }
    check(h, title_height);
}
