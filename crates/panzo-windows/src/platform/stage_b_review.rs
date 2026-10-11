use super::*;
#[test]
#[ignore = "requires interactive desktop and a new PANZO_STAGE_B_DIR; uses only synthetic image content"]
fn stage_b_native_image_and_pin_restart() {
    let directory =
        PathBuf::from(std::env::var_os("PANZO_STAGE_B_DIR").expect("isolated stage B directory"));
    fs::create_dir_all(&directory).unwrap();
    let library = directory.join("library");
    fs::create_dir_all(&library).unwrap();
    let rounds = if std::env::var_os("PANZO_STAGE_B_RESTORE_ONLY").is_some() {
        2..3
    } else {
        0..2
    };
    for round in rounds {
        let output = directory.clone();
        let library_copy = library.clone();
        RecorderWindow::run_internal(
            &library,
            Automation::None,
            None,
            Some(Box::new(move |raw| {
                let hwnd = HWND(raw as *mut std::ffi::c_void);
                crate::platform::capture_tools::review_stage_b(hwnd, &library_copy, &output, round);
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        Some(hwnd),
                        WM_CLOSE,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            })),
        )
        .unwrap();
    }
}
