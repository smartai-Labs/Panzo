//! Share caption geometry with non-client hit testing and native system commands.
#![allow(clippy::cast_possible_wrap)] // Win32 HT* values are small positive constants.
use super::{HitRect, client_rect, signed_high, signed_low};
use crate::platform::editor_ui::{self, IconKind};
use crate::platform::editor_ui::{EditorTheme, HeaderAction, HeaderLayout, window_dpi};
use std::{cell::RefCell, collections::HashMap};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_BUTTON_BOUNDS, DwmDefWindowProc, DwmExtendFrameIntoClientArea,
    DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{HDC, InvalidateRect, ScreenToClient};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetWindowRect, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT,
    HTCLOSE, HTLEFT, HTMAXBUTTON, HTMINBUTTON, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IsZoomed,
    SM_CXPADDEDBORDER, SM_CYFRAME, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SetWindowPos, WM_ACTIVATE, WM_DPICHANGED, WM_DWMCOMPOSITIONCHANGED,
    WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP,
    WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_SIZE,
};

thread_local! {
    static HOT_BUTTON: RefCell<HashMap<isize, u32>> = RefCell::new(HashMap::new());
}

/// Painting and non-client hit testing share client coordinates; Windows owns commands.
pub(crate) fn paint_system_buttons(hwnd: HWND, dc: HDC, theme: EditorTheme) {
    let Some(bounds) = caption_bounds(hwnd) else {
        return;
    };
    let hot = HOT_BUTTON.with(|buttons| buttons.borrow().get(&(hwnd.0 as isize)).copied());
    let width = bounds.right - bounds.left;
    let maximize = if unsafe { IsZoomed(hwnd) }.as_bool() {
        IconKind::WindowRestore
    } else {
        IconKind::Maximize
    };
    for (index, (code, icon)) in [
        (HTMINBUTTON, IconKind::Minimize),
        (HTMAXBUTTON, maximize),
        (HTCLOSE, IconKind::Close),
    ]
    .into_iter()
    .enumerate()
    {
        let index = i32::try_from(index).unwrap_or_default();
        let rect = RECT {
            left: bounds.left + width * index / 3,
            right: bounds.left + width * (index + 1) / 3,
            ..bounds
        };
        let mut ink = theme.palette.text;
        if hot == Some(code) {
            let fill = if code == HTCLOSE {
                ink = editor_ui::rgb(255, 255, 255);
                editor_ui::rgb(196, 43, 28)
            } else {
                theme.palette.hover
            };
            editor_ui::fill_opaque_rect(dc, rect, fill);
        }
        let half = theme.scale(9);
        let x = i32::midpoint(rect.left, rect.right);
        let y = i32::midpoint(rect.top, rect.bottom);
        editor_ui::draw_icon(
            dc,
            RECT {
                left: x - half,
                top: y - half,
                right: x + half,
                bottom: y + half,
            },
            icon,
            ink,
        );
    }
}

fn update_caption_visuals(hwnd: HWND, message: u32, wparam: WPARAM) {
    let key = hwnd.0 as isize;
    let changed = HOT_BUTTON.with(|buttons| {
        let mut buttons = buttons.borrow_mut();
        match message {
            WM_NCMOUSEMOVE => {
                let code = u32::try_from(wparam.0).unwrap_or_default();
                if [HTMINBUTTON, HTMAXBUTTON, HTCLOSE].contains(&code) {
                    let mut tracking = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE | TME_NONCLIENT,
                        hwndTrack: hwnd,
                        ..Default::default()
                    };
                    unsafe {
                        let _ = TrackMouseEvent(&raw mut tracking);
                    }
                    buttons.insert(key, code) != Some(code)
                } else {
                    buttons.remove(&key).is_some()
                }
            }
            WM_NCMOUSELEAVE | WM_NCDESTROY | WM_ACTIVATE => buttons.remove(&key).is_some(),
            _ => false,
        }
    });
    if changed || [WM_NCACTIVATE, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP].contains(&message) {
        let rect = HeaderLayout::new(
            client_rect(hwnd),
            EditorTheme::current(window_dpi(hwnd)),
            caption_bounds(hwnd),
        )
        .bar;
        unsafe {
            let _ = InvalidateRect(Some(hwnd), Some(&raw const rect), false);
        }
    }
}

pub(crate) fn initialize(hwnd: HWND) {
    if crate::platform::workspace::window(hwnd) != hwnd {
        return;
    }
    extend_frame(hwnd);
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
    }
}

fn extend_frame(hwnd: HWND) {
    let margins = MARGINS {
        cyTopHeight: if crate::platform::workspace::custom_caption(hwnd) {
            crate::platform::workspace::caption_height(hwnd)
        } else {
            EditorTheme::current(window_dpi(hwnd))
                .metrics
                .toolbar_height
        },
        ..MARGINS::default()
    };
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(hwnd, &raw const margins);
    }
}

pub(crate) fn caption_bounds(hwnd: HWND) -> Option<RECT> {
    if crate::platform::workspace::window(hwnd) != hwnd {
        return None;
    }
    if crate::platform::workspace::custom_caption(hwnd) {
        return Some(custom_caption_bounds(
            client_rect(hwnd),
            EditorTheme::current(window_dpi(hwnd)),
        ));
    }
    let mut buttons = RECT::default();
    let mut window = RECT::default();
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_BUTTON_BOUNDS,
            (&raw mut buttons).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
        .ok()?;
        GetWindowRect(hwnd, &raw mut window).ok()?;
    }
    if buttons.right <= buttons.left || buttons.bottom <= buttons.top {
        return None;
    }
    let mut start = POINT {
        x: window.left + buttons.left,
        y: window.top + buttons.top,
    };
    unsafe {
        let _ = ScreenToClient(hwnd, &raw mut start);
    }
    let client = client_rect(hwnd);
    (start.x > client.right / 2 && start.x < client.right).then_some(RECT {
        left: start.x,
        top: start.y,
        right: start.x + buttons.right - buttons.left,
        bottom: start.y + buttons.bottom - buttons.top,
    })
}

fn custom_caption_bounds(client: RECT, theme: EditorTheme) -> RECT {
    RECT {
        left: (client.right - theme.scale(138)).max(client.left),
        right: client.right,
        top: client.top,
        bottom: client.top + theme.scale(32),
    }
}

fn caption_button_hit(bounds: RECT, point: POINT) -> Option<u32> {
    if !HitRect(bounds).contains(point.x, point.y) {
        return None;
    }
    let width = bounds.right - bounds.left;
    [HTMINBUTTON, HTMAXBUTTON, HTCLOSE]
        .into_iter()
        .enumerate()
        .find_map(|(index, code)| {
            let index = index as i32;
            (point.x >= bounds.left + width * index / 3
                && point.x < bounds.left + width * (index + 1) / 3)
                .then_some(code)
        })
}

#[allow(clippy::too_many_lines)] // Native non-client routing and resize hit testing share one dispatch.
pub(crate) fn window_message(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    if let Some(result) = crate::platform::app_menu::window_message(message, lparam) {
        return Some(result);
    }
    if crate::platform::workspace::window(hwnd) != hwnd {
        return None;
    }
    update_caption_visuals(hwnd, message, wparam);
    let custom = crate::platform::workspace::custom_caption(hwnd);
    if message == WM_NCCALCSIZE && wparam.0 != 0 {
        // Keep the workspace inside the work area when the invisible resize frame
        // extends past the monitor after maximizing.
        if crate::platform::workspace::custom_caption(hwnd) && unsafe { IsZoomed(hwnd) }.as_bool() {
            let dpi = window_dpi(hwnd);
            let edge = unsafe {
                GetSystemMetricsForDpi(SM_CYFRAME, dpi)
                    + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
            };
            let params = unsafe {
                &mut *(lparam.0 as *mut windows::Win32::UI::WindowsAndMessaging::NCCALCSIZE_PARAMS)
            };
            params.rgrc[0].left += edge;
            params.rgrc[0].top += edge;
            params.rgrc[0].right -= edge;
            params.rgrc[0].bottom -= edge;
        }
        // The caption and resize border become client area; native window commands remain.
        return Some(LRESULT(0));
    }
    let mut result = LRESULT(0);
    // DWM reports window-relative button bounds, which move above the client when
    // maximized. Use our painted client rectangles for all custom-caption hits.
    if !(custom && message == WM_NCHITTEST)
        && unsafe { DwmDefWindowProc(hwnd, message, wparam, lparam, &raw mut result) }.as_bool()
    {
        return Some(result);
    }
    if [
        WM_ACTIVATE,
        WM_DWMCOMPOSITIONCHANGED,
        WM_DPICHANGED,
        WM_SIZE,
    ]
    .contains(&message)
    {
        extend_frame(hwnd);
    }
    if message != WM_NCHITTEST {
        return None;
    }
    let native = if custom {
        LRESULT(HTCLIENT as isize)
    } else {
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    };
    if native.0 != HTCLIENT as isize {
        return Some(native);
    }
    let theme = EditorTheme::current(window_dpi(hwnd));
    let mut point = POINT {
        x: signed_low(lparam.0),
        y: signed_high(lparam.0),
    };
    unsafe {
        let _ = ScreenToClient(hwnd, &raw mut point);
    }
    if custom && let Some(code) = caption_bounds(hwnd).and_then(|r| caption_button_hit(r, point)) {
        return Some(LRESULT(code as isize));
    }
    if crate::platform::workspace::caption_control_at(hwnd, point) {
        return Some(LRESULT(HTCLIENT as isize));
    }
    let layout = HeaderLayout::new(client_rect(hwnd), theme, caption_bounds(hwnd));
    if !crate::platform::workspace::custom_caption(hwnd)
        && layout.hit_action(point.x, point.y).is_some_and(|action| {
            !matches!(
                action,
                HeaderAction::Minimize | HeaderAction::Maximize | HeaderAction::Close
            )
        })
    {
        return Some(LRESULT(HTCLIENT as isize));
    }
    if !unsafe { IsZoomed(hwnd) }.as_bool() {
        let dpi = window_dpi(hwnd);
        let edge = unsafe {
            GetSystemMetricsForDpi(SM_CYFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
        };
        let client = client_rect(hwnd);
        let left = point.x < edge;
        let right = point.x >= client.right - edge;
        let top = point.y < theme.scale(5);
        let bottom = point.y >= client.bottom - edge;
        let code = match (left, right, top, bottom) {
            (true, _, true, _) => HTTOPLEFT,
            (_, true, true, _) => HTTOPRIGHT,
            (true, _, _, true) => HTBOTTOMLEFT,
            (_, true, _, true) => HTBOTTOMRIGHT,
            (true, _, _, _) => HTLEFT,
            (_, true, _, _) => HTRIGHT,
            (_, _, true, _) => HTTOP,
            (_, _, _, true) => HTBOTTOM,
            _ => HTCLIENT,
        };
        if code != HTCLIENT {
            return Some(LRESULT(code as isize));
        }
    }
    let caption = if crate::platform::workspace::custom_caption(hwnd) {
        RECT {
            bottom: crate::platform::workspace::caption_height(hwnd),
            ..client_rect(hwnd)
        }
    } else {
        layout.bar
    };
    Some(if HitRect(caption).contains(point.x, point.y) {
        LRESULT(HTCAPTION as isize)
    } else {
        native
    })
}

#[cfg(test)]
pub(crate) fn review_activation_colors(hwnd: HWND, output: &std::path::Path, label: &str) {
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    use windows::Win32::UI::WindowsAndMessaging::SendMessageW;
    let theme = EditorTheme::current(window_dpi(hwnd));
    let color = editor_ui::header_background(theme).0;
    let expected = [color as u8, (color >> 8) as u8, (color >> 16) as u8];
    let bounds = caption_bounds(hwnd).unwrap();
    for (state, active) in [("inactive", 0), ("active", 1), ("inactive-again", 0)] {
        unsafe {
            SendMessageW(hwnd, WM_NCACTIVATE, Some(WPARAM(active)), Some(LPARAM(0)));
            let _ = windows::Win32::Graphics::Gdi::UpdateWindow(hwnd);
            let _ = windows::Win32::Graphics::Dwm::DwmFlush();
        }
        let path = output.join(format!("{label}-{state}.png"));
        editor_ui::render_review_png(&path, client_rect(hwnd).right, theme.scale(40), |dc, _| {
            assert!(unsafe { PrintWindow(hwnd, dc, PRINT_WINDOW_FLAGS(2)) }.as_bool());
        });
        let pixels = image::open(path).unwrap().to_rgb8();
        // Maximized windows include an off-screen resize frame in PrintWindow.
        // Sample within the visible caption and outside the centered glyphs.
        for x in [
            theme.scale(300),
            bounds.left + theme.scale(10),
            bounds.left + (bounds.right - bounds.left) / 3 + theme.scale(10),
            bounds.left + (bounds.right - bounds.left) * 2 / 3 + theme.scale(10),
        ] {
            assert_eq!(
                pixels
                    .get_pixel(
                        x.cast_unsigned(),
                        (bounds.top + theme.scale(10)).cast_unsigned()
                    )
                    .0,
                expected,
                "{label}/{state}: fixed gray caption including system buttons"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ControlId, PreviewWindow, SmokeAction, state_mut};
    use super::*;
    use crate::platform::{capture_tools, ui_preferences, workspace};
    use std::fmt::Write as _;
    use std::time::Duration;
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetDlgItem, IsIconic, SC_MINIMIZE, SC_RESTORE, SW_MAXIMIZE, SW_RESTORE, SendMessageW,
        ShowWindow, WM_SYSCOMMAND,
    };

    fn hit(hwnd: HWND, x: i32, y: i32) -> isize {
        let mut p = POINT { x, y };
        unsafe {
            let _ = ClientToScreen(hwnd, &raw mut p);
        }
        let packed = (p.x & 0xffff) | ((p.y & 0xffff) << 16);
        unsafe { SendMessageW(hwnd, WM_NCHITTEST, None, Some(LPARAM(packed as isize))) }.0
    }

    fn settle() {
        use windows::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
        };
        let deadline = std::time::Instant::now() + Duration::from_millis(150);
        while std::time::Instant::now() < deadline {
            let mut msg = MSG::default();
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn custom_caption_paint_and_hit_bounds_match_at_each_dpi() {
        for dpi in [96, 120, 144, 168, 192] {
            let theme = EditorTheme::for_dpi(dpi);
            for width in [960, 1280, 1919] {
                let client = RECT {
                    right: width,
                    bottom: 900,
                    ..RECT::default()
                };
                let r = custom_caption_bounds(client, theme);
                assert_eq!(r.top, 0);
                assert_eq!(r.bottom, theme.scale(32));
                assert_eq!(r.right, width);
                for (i, code) in [HTMINBUTTON, HTMAXBUTTON, HTCLOSE].into_iter().enumerate() {
                    let x = r.left + (r.right - r.left) * (i as i32 * 2 + 1) / 6;
                    for y in [r.top, (r.top + r.bottom) / 2, r.bottom - 1] {
                        assert_eq!(caption_button_hit(r, POINT { x, y }), Some(code));
                    }
                }
                for p in [
                    POINT {
                        x: r.left - 1,
                        y: 16,
                    },
                    POINT { x: r.right, y: 16 },
                    POINT {
                        x: r.left,
                        y: r.top - 1,
                    },
                    POINT {
                        x: r.left,
                        y: r.bottom,
                    },
                ] {
                    assert_eq!(caption_button_hit(r, p), None);
                }
            }
        }
    }

    #[test]
    #[ignore = "explicit disposable project/profile and native window required"]
    fn native_titlebar_ui_review() {
        let project =
            std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
        let output = std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
        assert!(
            std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
                .starts_with(&output)
        );
        assert!(unsafe { windows::Win32::UI::Input::Ime::ImmDisableIME(0) }.as_bool());
        workspace::enable_review();
        ui_preferences::set_theme(ui_preferences::ThemeMode::Dark).unwrap();
        let report = PreviewWindow::run_internal_notifying(&project, Some(Duration::from_secs(30)), SmokeAction::None,
            Some(Box::new(move |raw| {
                let editor = HWND(raw as *mut std::ffi::c_void);
                let root = workspace::window(editor);
                assert_ne!(root, editor, "exercise the actual application title bar");
                let mut notes = String::new();
                for (label, mode, light) in [("normal-dark", SW_RESTORE, false),
                    ("maximized-dark", SW_MAXIMIZE, false), ("restored-light", SW_RESTORE, true)] {
                    if light {
                        unsafe { SendMessageW(root, windows::Win32::UI::WindowsAndMessaging::WM_COMMAND,
                            Some(WPARAM(110)), Some(LPARAM(0))); }
                    }
                    unsafe { let _ = ShowWindow(root, mode); }
                    settle();
                    capture_tools::review_present(root);
                    let state = unsafe { state_mut(editor) }.unwrap();
                    assert_eq!(state.theme.mode, if light { ui_preferences::ThemeMode::Light } else { ui_preferences::ThemeMode::Dark });
                    state.resize_preview_surface(editor);
                    let c = state.control_layout(client_rect(editor));
                    let menu = c.control_rect(ControlId::Menu);
                    assert_eq!((menu.top, menu.bottom), (c.status.top, c.status.bottom));
                    assert_eq!((menu.top, menu.bottom), (c.preview_fit.top, c.preview_fit.bottom));
                    assert_eq!(c.status.left, menu.right + state.theme.scale(8));
                    assert_eq!(c.diagnostics.0, RECT::default());
                    assert_eq!(c.preview_well.top, c.preview_card.top);
                    assert!(state.preview_surface.is_some());
                    assert_eq!(state.preview_surface.as_ref().unwrap().well, c.preview_well);
                    assert!(!state.editor.is_dirty());
                    let bounds = caption_bounds(root).unwrap();
                    let caption_center = i32::midpoint(bounds.top, bounds.bottom);
                    for id in [110, 111, 112, 113, 114, 115, 116] {
                        let child = unsafe { GetDlgItem(Some(root), id) }.unwrap();
                        let mut r = RECT::default();
                        unsafe { GetWindowRect(child, &mut r).unwrap(); }
                        let mut center = POINT { x: i32::midpoint(r.left, r.right), y: i32::midpoint(r.top, r.bottom) };
                        unsafe { let _ = ScreenToClient(root, &mut center); }
                        assert!((center.y - caption_center).abs() <= 1, "{label}: control {id} must align");
                        assert_eq!(hit(root, center.x, center.y), HTCLIENT as isize);
                    }
                    for (i, code) in [HTMINBUTTON, HTMAXBUTTON, HTCLOSE].into_iter().enumerate() {
                        let x = bounds.left + (bounds.right - bounds.left) * (i as i32 * 2 + 1) / 6;
                        for y in [bounds.top + 2, caption_center, bounds.bottom - 2] {
                            assert_eq!(hit(root, x, y), code as isize, "{label} button {i}");
                        }
                    }
                    assert_eq!(hit(root, bounds.left - state.theme.scale(100), caption_center), HTCAPTION as isize);
                    capture_tools::review_present(root);
                    capture_tools::review_window_png(root, &output.join(format!("{label}.png")));
                    writeln!(notes, "{label}: menu/theme/system buttons center y={caption_center}; matching native hit tests; page menu/status/fit aligned; no gear or extra project row; GPU bounds follow enlarged well.").unwrap();
                }
                unsafe { SendMessageW(root, WM_SYSCOMMAND, Some(WPARAM(SC_MINIMIZE as usize)), Some(LPARAM(0))); }
                assert!(unsafe { IsIconic(root) }.as_bool());
                unsafe { SendMessageW(root, WM_SYSCOMMAND, Some(WPARAM(SC_RESTORE as usize)), Some(LPARAM(0))); }
                assert!(!unsafe { IsIconic(root) }.as_bool());
                // Dirty state is rendered in the same compact row; restore the fixture before closing.
                let state = unsafe { state_mut(editor) }.unwrap();
                let settings = state.editor.settings().cursor.clone();
                state.editor.set_cursor_style(!settings.visible, settings.scale).unwrap();
                assert!(state.editor.is_dirty());
                state.set_status(editor_ui::StatusSeverity::Information, "已选择视频 · B 分割 · Delete 删除 · 拖动两端裁剪".into());
                state.refresh(editor, true);
                capture_tools::review_present(root);
                capture_tools::review_window_png(root, &output.join("unsaved-light.png"));
                let state = unsafe { state_mut(editor) }.unwrap();
                state.undo();
                assert!(!state.editor.is_dirty());
                state.close_at = Some(std::time::Instant::now() + Duration::from_millis(200));
                notes.push_str("Programmatic minimize/restore passed; dirty state then undo passed. Physical native button clicks and cross-monitor DPI switching were not exercised. Project left unchanged.\n");
                std::fs::write(output.join("review.txt"), notes).unwrap();
            }))).unwrap();
        assert!(report.graceful_close);
        assert_eq!(report.media_errors, 0);
    }
}
