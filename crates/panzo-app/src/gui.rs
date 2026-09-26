#![windows_subsystem = "windows"]

use panzo_app::default_project_library;
use panzo_windows::platform::recorder_window::RecorderWindow;
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::PCWSTR;

fn main() {
    std::panic::set_hook(Box::new(|panic| {
        show_error(&format!("Panzo 遇到未处理错误：\n\n{panic}"));
    }));
    let result = if let Some(project) = std::env::args_os().nth(1) {
        RecorderWindow::run_project(
            &default_project_library(),
            &std::path::PathBuf::from(project),
        )
        .map_err(|e| e.to_string())
    } else {
        RecorderWindow::run(default_project_library()).map_err(|e| e.to_string())
    };
    if let Err(error) = result {
        show_error(&format!("Panzo 无法启动：\n\n{error}"));
    }
}

fn show_error(message: &str) {
    let message = wide(message);
    let title = wide("Panzo 启动错误");
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
