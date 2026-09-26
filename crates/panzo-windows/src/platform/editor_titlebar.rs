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

/// Only visuals are client-painted. DWM still owns hit testing and system commands.
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
        cyTopHeight: EditorTheme::current(window_dpi(hwnd))
            .metrics
            .toolbar_height,
        ..MARGINS::default()
    };
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(hwnd, &raw const margins);
    }
}

pub(crate) fn caption_bounds(hwnd: HWND) -> Option<RECT> {
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

#[allow(clippy::too_many_lines)] // Native non-client routing and resize hit testing share one dispatch.
pub(crate) fn window_message(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    update_caption_visuals(hwnd, message, wparam);
    if let Some(result) = crate::platform::app_menu::window_message(message, lparam) {
        return Some(result);
    }
    if message == WM_NCCALCSIZE && wparam.0 != 0 {
        // The caption and resize border become client area; native window commands remain.
        return Some(LRESULT(0));
    }
    let mut result = LRESULT(0);
    if unsafe { DwmDefWindowProc(hwnd, message, wparam, lparam, &raw mut result) }.as_bool() {
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
    let native = unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
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
    let layout = HeaderLayout::new(client_rect(hwnd), theme, caption_bounds(hwnd));
    if layout.hit_action(point.x, point.y).is_some_and(|action| {
        !matches!(
            action,
            HeaderAction::Minimize | HeaderAction::Maximize | HeaderAction::Close
        )
    }) {
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
    Some(if HitRect(layout.bar).contains(point.x, point.y) {
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
    use super::super::{ControlId, state_mut};
    use super::super::{PreviewWindow, SmokeAction};
    use super::*;
    use std::fmt::Write as _;
    use std::time::Duration;
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    use windows::Win32::UI::WindowsAndMessaging::{
        HTCLOSE, HTMAXBUTTON, HTMINBUTTON, IsIconic, SC_MINIMIZE, SC_RESTORE, SW_MAXIMIZE,
        SW_RESTORE, SendMessageW, ShowWindow, WM_SYSCOMMAND,
    };

    fn hit(hwnd: HWND, x: i32, y: i32) -> isize {
        let mut p = POINT { x, y };
        unsafe {
            let _ = ClientToScreen(hwnd, &raw mut p);
        }
        let packed = (p.x & 0xffff) | ((p.y & 0xffff) << 16);
        unsafe { SendMessageW(hwnd, WM_NCHITTEST, None, Some(LPARAM(packed as isize))) }.0
    }

    #[test]
    #[ignore = "explicit disposable project and native window required"]
    #[allow(clippy::too_many_lines)]
    fn native_titlebar_ui_review() {
        let project =
            std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
        let output = std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
        let report = PreviewWindow::run_internal_notifying(&project, Some(Duration::from_secs(3)), SmokeAction::None,
            Some(Box::new(move |raw| {
                let hwnd = HWND(raw as *mut std::ffi::c_void);
                let mut notes = String::new();
                for (label, mode) in [("normal", SW_RESTORE), ("maximized", SW_MAXIMIZE), ("restored", SW_RESTORE)] {
                    unsafe { let _ = ShowWindow(hwnd, mode); }
                    let settling = std::time::Instant::now();
                    while settling.elapsed() < Duration::from_millis(350) {
                        use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, PM_REMOVE, MSG, DispatchMessageW, TranslateMessage};
                        let mut msg = MSG::default();
                        unsafe { while PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE).as_bool() { let _ = TranslateMessage(&raw const msg); DispatchMessageW(&raw const msg); } }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    unsafe { let _ = windows::Win32::Graphics::Gdi::UpdateWindow(hwnd); let _ = windows::Win32::Graphics::Dwm::DwmFlush(); }
                    let state = unsafe { state_mut(hwnd) }.unwrap();
                    state.resize_preview_surface(hwnd);
                    let theme = state.theme;
                    let layout = state.control_layout(client_rect(hwnd));
                    let end = caption_bounds(hwnd).expect("DWM caption bounds");
                    assert!(layout.diagnostics.right <= end.left);
                    assert_eq!(layout.back.left, layout.back.right);
                    assert_eq!(layout.preview_card.top, theme.metrics.toolbar_height + theme.metrics.outer_margin);
                    assert_eq!(layout.command_title, RECT::default());
                    assert!(layout.window_buttons.iter().all(|r| r.0 == RECT::default()));
                    for id in [ControlId::Menu, ControlId::Diagnostics] {
                        let r = layout.control_rect(id);
                        let x = i32::midpoint(r.left, r.right);
                        let y = i32::midpoint(r.top, r.bottom);
                        assert!((y - i32::midpoint(end.top, end.bottom)).abs() <= 1);
                        assert_eq!(hit(hwnd, x, y), HTCLIENT as isize, "{label}: {id:?}");
                        let state = unsafe { state_mut(hwnd) }.unwrap();
                        assert_eq!(state.control_layout(client_rect(hwnd)).hit_control(x, y), Some(id));
                        state.on_mouse_move(hwnd, x, y);
                        assert_eq!(state.hot_control, Some(id));
                        assert!(state.tooltip_deadline.is_some());
                        state.on_mouse_leave(hwnd);
                    }
                    assert_eq!(hit(hwnd, theme.scale(300), theme.scale(16)), HTCAPTION as isize);
                    if label != "maximized" {
                        assert_eq!(hit(hwnd, theme.scale(300), 1), HTTOP as isize);
                    }
                    let width = (end.right - end.left) / 3;
                    let mut window = RECT::default();
                    unsafe { GetWindowRect(hwnd, &raw mut window).unwrap(); }
                    crate::platform::editor_ui::render_review_png(&output.join(format!("native-titlebar-{label}.png")), window.right - window.left, theme.scale(110), |dc, _| {
                        assert!(unsafe { PrintWindow(hwnd, dc, PRINT_WINDOW_FLAGS(2)) }.as_bool());
                    });
                    let pixels = image::open(output.join(format!("native-titlebar-{label}.png"))).unwrap().to_rgb8();
                    for (i, code) in [HTMINBUTTON, HTMAXBUTTON, HTCLOSE].iter().enumerate() {
                        let x = end.left + width * i32::try_from(i).unwrap() + width / 2;
                        let y = i32::midpoint(end.top, end.bottom);
                        assert_eq!(hit(hwnd, x, y), *code as isize, "{label}: system button {i}");
                        let background = pixels.get_pixel((x - width / 2 + theme.scale(3)).cast_unsigned(), (end.top + theme.scale(3)).cast_unsigned()).0;
                        assert_eq!(background, pixels.get_pixel(theme.scale(300).cast_unsigned(), (end.top + theme.scale(3)).cast_unsigned()).0, "{label}: caption button {i} must share the header background");
                        assert!((y - theme.scale(9)..y + theme.scale(9)).any(|py| {
                            (x - theme.scale(12)..x + theme.scale(12)).any(|px| {
                                pixels.get_pixel(px.cast_unsigned(), py.cast_unsigned()).0.iter().zip(background).any(|(a, b)| a.abs_diff(b) > 24)
                            })
                        }), "{label}: native button {i} must be visible, not just hit-testable");
                    }
                    writeln!(notes, "{label}: window {window:?}, caption bounds {end:?}, action hover/tooltip + HTCLIENT, caption drag + native min/max/close hit tests and visible glyph checks passed").unwrap();
                    review_activation_colors(hwnd, &output, label);
                }
                unsafe { SendMessageW(hwnd, WM_SYSCOMMAND, Some(WPARAM(SC_MINIMIZE as usize)), Some(LPARAM(0))); }
                assert!(unsafe { IsIconic(hwnd) }.as_bool());
                unsafe { SendMessageW(hwnd, WM_SYSCOMMAND, Some(WPARAM(SC_RESTORE as usize)), Some(LPARAM(0))); }
                assert!(!unsafe { IsIconic(hwnd) }.as_bool());
                let state = unsafe { state_mut(hwnd) }.unwrap();
                assert!(!state.editor.is_dirty());
                for mode in [crate::platform::ui_preferences::ThemeMode::Dark, crate::platform::ui_preferences::ThemeMode::Light] {
                    state.theme.set_mode(mode);
                    let menu_image = output.join(format!("native-main-menu-{mode:?}.png"));
                    crate::platform::app_menu::review::next_popup(menu_image.clone());
                    state.activate_control(hwnd, ControlId::Menu);
                    assert!(menu_image.is_file(), "native menu must enter its message loop and draw");
                    let settings_image = output.join(format!("native-settings-menu-{mode:?}.png"));
                    // The real timer may restore the persisted theme during the first popup.
                    state.theme.set_mode(mode);
                    crate::platform::app_menu::review::next_popup(settings_image.clone());
                    state.activate_control(hwnd, ControlId::Diagnostics);
                    assert!(settings_image.is_file(), "settings must use the same native themed popup");
                    assert!(!crate::platform::app_menu::is_open());
                    assert!(state.pending_navigation.is_none());
                }
                // Release over a different button must cancel without opening a dialog.
                let c = state.control_layout(client_rect(hwnd));
                let menu = c.control_rect(ControlId::Menu);
                state.on_mouse_down(hwnd, menu.left + 10, menu.top + 10);
                state.on_mouse_up(hwnd, c.diagnostics.left + 10, c.diagnostics.top + 10);
                assert!(state.export_panel.is_none());
                state.activate_control(hwnd, ControlId::Export);
                assert!(state.export_panel.is_some());
                drop(state.export_panel.take());
                notes.push_str("Native main-menu open/draw/cancel in both themes, programmatic minimize/restore, canceled pointer release and existing export command passed; physical native button clicks were not exercised by this test. Project unchanged.\n");
                std::fs::write(output.join("native-titlebar-review.txt"), notes).unwrap();
            }))).unwrap();
        assert!(report.graceful_close);
        assert_eq!(report.media_errors, 0);
    }
}
