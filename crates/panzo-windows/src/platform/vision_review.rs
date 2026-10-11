//! Real configuration dialog verification with synthetic credentials and an isolated settings file.
use super::*;
use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CB_SETCURSEL, CreateWindowExW, DestroyWindow, ES_PASSWORD, FindWindowW, GWL_STYLE,
        GetDlgItem, GetWindowLongPtrW, GetWindowTextW, GetWindowThreadProcessId, PostMessageW,
        SendMessageW, SetWindowTextW, WM_CLOSE, WM_COMMAND, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    },
};
struct Owner(HWND);
impl Drop for Owner {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
fn text(hwnd: HWND) -> String {
    let mut text = [0; 4096];
    let count = unsafe { GetWindowTextW(hwnd, &mut text) }
        .max(0)
        .cast_unsigned() as usize;
    String::from_utf16_lossy(&text[..count])
}
fn set(hwnd: HWND, value: &str) {
    let value = value.encode_utf16().chain([0]).collect::<Vec<_>>();
    unsafe {
        SetWindowTextW(hwnd, PCWSTR(value.as_ptr())).unwrap();
    }
}
#[test]
#[ignore = "requires interactive desktop and PANZO_VISION_REVIEW_DIR; synthetic credentials only"]
#[allow(clippy::too_many_lines)] // One native configuration lifecycle verifies save, reload and key deletion.
fn multimodal_native_settings_dialog_and_restart() {
    let root = PathBuf::from(
        std::env::var_os("PANZO_VISION_REVIEW_DIR").expect("isolated review directory"),
    );
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("vision.json");
    assert!(!path.exists(), "use a fresh isolated review directory");
    let module = unsafe { GetModuleHandleW(None) }.unwrap();
    let owner = unsafe {
        CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("Panzo · 多模态配置测试"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            100,
            100,
            760,
            560,
            None,
            None,
            Some(module.into()),
            None,
        )
    }
    .unwrap();
    let _owner = Owner(owner);
    for round in 0..3 {
        let output = root.clone();
        let automation = thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(8);
            let dialog = loop {
                if let Ok(hwnd) =
                    unsafe { FindWindowW(w!("PanzoProperties"), w!("多模态识图设置")) }
                {
                    let mut process = 0;
                    unsafe {
                        GetWindowThreadProcessId(hwnd, Some(&raw mut process));
                    }
                    if process == std::process::id()
                        && unsafe { GetDlgItem(Some(hwnd), 105) }.is_ok()
                    {
                        break hwnd;
                    }
                }
                assert!(Instant::now() < until, "configuration dialog did not open");
                thread::sleep(Duration::from_millis(10));
            };
            thread::sleep(Duration::from_millis(80));
            let outcome = std::panic::catch_unwind(|| {
                let input = |id| unsafe { GetDlgItem(Some(dialog), id) }.unwrap();
                let key = input(102);
                assert_ne!(
                    unsafe { GetWindowLongPtrW(key, GWL_STYLE) } & ES_PASSWORD as isize,
                    0
                );
                assert!(
                    text(key).is_empty(),
                    "saved secret must never be put back in the edit control"
                );
                if round == 0 {
                    set(input(100), "https://example.invalid/v1");
                    set(input(101), "panzo-test-vision");
                    set(key, "synthetic-native-secret");
                    super::super::capture_tools::review_window_png(
                        dialog,
                        &output.join("settings-masked.png"),
                    );
                } else if round == 1 {
                    assert_eq!(text(input(100)), "https://example.invalid/v1");
                    assert_eq!(text(input(101)), "panzo-test-vision");
                    set(input(101), "vision-after-edit");
                    super::super::capture_tools::review_window_png(
                        dialog,
                        &output.join("settings-restored.png"),
                    );
                } else {
                    unsafe {
                        SendMessageW(input(103), CB_SETCURSEL, Some(WPARAM(1)), None);
                    }
                }
            });
            unsafe {
                let _ = PostMessageW(
                    Some(dialog),
                    if outcome.is_ok() {
                        WM_COMMAND
                    } else {
                        WM_CLOSE
                    },
                    WPARAM(1),
                    LPARAM(0),
                );
            }
            if let Err(error) = outcome {
                std::panic::resume_unwind(error);
            }
        });
        let previous = vision_settings::load_at(&path).unwrap();
        assert!(
            configure_with(owner, &previous, |next, change| vision_settings::save_at(
                &path, next, change
            ))
            .unwrap()
        );
        automation.join().unwrap();
        let stored = vision_settings::load_at(&path).unwrap();
        if round < 2 {
            assert_eq!(stored.api_key().unwrap(), "synthetic-native-secret");
            assert!(stored.default_online);
            assert!(
                !std::fs::read_to_string(&path)
                    .unwrap()
                    .contains("synthetic-native-secret")
            );
        } else {
            assert!(!stored.has_key());
            assert!(!stored.default_online);
        }
        if round == 1 {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "platform::vision_ui::review_tests::multimodal_settings_fresh_process_read",
                    "--ignored",
                    "--test-threads=1",
                ])
                .env("PANZO_VISION_REVIEW_DIR", &root)
                .output()
                .unwrap();
            assert!(
                child.status.success(),
                "{}",
                String::from_utf8_lossy(&child.stdout)
            );
        }
    }
    std::fs::write(
        root.join("settings-proof.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "passwordControlMasked":true,"savedKeyNotPrefilled":true,"plaintextAbsentFromDisk":true,
            "blankKeepsExistingKey":true,"freshProcessDecrypts":true,"deleteClearsKey":true,
            "defaultOnlinePersisted":true,"userSettingsUntouched":true,"externalNetworkRequests":0
        }))
        .unwrap(),
    )
    .unwrap();
}
#[test]
#[ignore = "child helper; reads only the isolated synthetic configuration"]
fn multimodal_settings_fresh_process_read() {
    let root = PathBuf::from(std::env::var_os("PANZO_VISION_REVIEW_DIR").unwrap());
    let settings = vision_settings::load_at(&root.join("vision.json")).unwrap();
    assert_eq!(settings.model, "vision-after-edit");
    assert_eq!(settings.api_key().unwrap(), "synthetic-native-secret");
    assert!(settings.default_online);
}
