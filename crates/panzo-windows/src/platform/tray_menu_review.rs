//! Native tray menu verification in a disposable host and profile; no capture is started.
use super::*;
use crate::platform::{app_menu, app_shell, ui_preferences, workspace};
use windows::Win32::UI::WindowsAndMessaging::{IsWindowVisible, SW_HIDE, WM_RBUTTONUP};

#[test]
#[ignore = "opens native menus in an isolated PANZO_TRAY_REVIEW_DIR profile"]
fn tray_menu_native_review() {
    let out = PathBuf::from(std::env::var_os("PANZO_TRAY_REVIEW_DIR").unwrap());
    assert!(PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).starts_with(&out));
    assert!(unsafe { windows::Win32::UI::Input::Ime::ImmDisableIME(0) }.as_bool());
    workspace::enable_review();
    let library = out.join("library");
    fs::create_dir_all(&library).unwrap();
    let report = RecorderWindow::run_internal(&library, Automation::None, None,
        Some(Box::new(move |raw| {
            let hwnd = HWND(raw as *mut std::ffi::c_void);
            let root = workspace::window(hwnd);
            unsafe { let _ = ShowWindow(root, SW_HIDE); }
            for (mode, name, checked) in [
                (ui_preferences::ThemeMode::Dark, "dark", true),
                (ui_preferences::ThemeMode::Light, "light", false),
            ] {
                ui_preferences::set_theme(mode).unwrap();
                ui_preferences::set_close_to_tray(checked).unwrap();
                let image = out.join(format!("tray-menu-{name}.png"));
                app_menu::review::next_popup(image.clone());
                assert_eq!(app_shell::tray_command(hwnd, WM_RBUTTONUP), None);
                assert!(image.is_file());
                assert!(!app_menu::is_open());
                assert!(!unsafe {IsWindowVisible(root)}.as_bool());
                assert_eq!(ui_preferences::close_to_tray(), checked);
            }
            // The exit command still exits the disposable app even when close-to-tray is on.
            ui_preferences::set_close_to_tray(true).unwrap();
            unsafe {state_mut(hwnd)}.unwrap().shell_command(hwnd, 12);
            fs::write(out.join("review.txt"), "PASS: real tray-command popup from a hidden workspace in dark/light themes; cancel preserves hidden state and tray preference; exit requested through normal shell command 12; no recording or user documents opened.\n").unwrap();
        }))).unwrap();
    assert!(report.window_created && report.graceful_close);
    assert!(!report.recording_started);
}
