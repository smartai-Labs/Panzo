//! Opt-in native Stage D evidence uses generated images and a disposable library only.
use super::*;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
#[test]
#[ignore = "requires desktop and new PANZO_STAGE_D_DIR; synthetic images only"]
fn stage_d_native_image_tools() {
    let root = PathBuf::from(std::env::var_os("PANZO_STAGE_D_DIR").expect("isolated output"));
    fs::create_dir_all(&root).unwrap();
    let library = root.join("library");
    fs::create_dir_all(&library).unwrap();
    let rounds = if std::env::var_os("PANZO_STAGE_D_RESTORE_ONLY").is_some() {
        2..3
    } else {
        0..2
    };
    for round in rounds {
        let output = root.clone();
        let assets = library.clone();
        RecorderWindow::run_internal(
            &library,
            Automation::None,
            None,
            Some(Box::new(move |raw| {
                let hwnd = HWND(raw as *mut _);
                let state = unsafe { state_mut(hwnd) }.unwrap();
                if round == 0 {
                    let _drawing = crate::platform::editor_ui::DrawingScale::enter(state.theme.dpi);
                    let client = client_rect(hwnd);
                    crate::platform::editor_ui::render_review_png(
                        &output.join("home.png"),
                        client.right,
                        client.bottom,
                        |dc, rect| draw_scene(dc, rect, state),
                    );
                    let layout = ControlLayout::for_state(hwnd, state);
                    for id in [RecorderControlId::ImageTools, RecorderControlId::Ocr] {
                        let r = layout.control_rect(id).0;
                        assert!(r.bottom < client.bottom);
                        assert_eq!(
                            layout.hit_control(
                                i32::midpoint(r.left, r.right),
                                i32::midpoint(r.top, r.bottom)
                            ),
                            Some(id)
                        );
                    }
                }
                crate::platform::capture_tools::review_stage_d(hwnd, &assets, &output, round);
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            })),
        )
        .unwrap();
    }
}
