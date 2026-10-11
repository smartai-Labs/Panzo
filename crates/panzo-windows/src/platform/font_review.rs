//! Native font properties, save/reopen and encoded subtitle regression.
use super::*;
use crate::platform::{capture_tools, media_panel, workspace, workspace_ui as ui};
use std::path::PathBuf;
use windows::Win32::UI::WindowsAndMessaging::*;
fn pump_fonts() {
    let end = Instant::now() + Duration::from_millis(200);
    while Instant::now() < end {
        let mut m = MSG::default();
        unsafe {
            while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
        std::thread::sleep(Duration::from_millis(3));
    }
}
#[test]
#[ignore = "requires isolated PANZO_UI_REVIEW_PROJECT/LOCALAPPDATA/PANZO_UI_REVIEW_DIR"]
fn subtitle_font_controls_save_reopen_and_export() {
    let project = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
    let out = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
    workspace::enable_review();
    crate::platform::ui_preferences::set_theme(crate::platform::ui_preferences::ThemeMode::Dark)
        .unwrap();
    let output = out.clone();
    let report = PreviewWindow::run_internal_notifying(
        &project,
        Some(Duration::from_secs(30)),
        SmokeAction::None,
        Some(Box::new(move |raw| {
            let h = HWND(raw as *mut c_void);
            let root = workspace::window(h);
            unsafe {
                SetWindowPos(
                    root,
                    None,
                    30,
                    30,
                    1440,
                    1000,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
                .unwrap();
            }
            let s = unsafe { state_mut(h) }.unwrap();
            let mut next = s.editor.settings().clone();
            next.audio_clips.clear();
            next.subtitles = vec![panzo_core::media_edit::SubtitleCue {
                id: "font-cue".into(),
                start: TimeTick::ZERO,
                end: TimeTick::from_millis(1500),
                text: "Panzo WWW iii font test".into(),
                anchor: panzo_core::media_edit::TimeAnchor::Project,
            }];
            s.editor.replace_media_settings(next).unwrap();
            s.apply_editor_state().unwrap();
            s.activate_control(h, ControlId::SubtitlePanel);
            pump_fonts();
            let panel = workspace::review_form(1);
            unsafe {
                SendMessageW(panel, WM_COMMAND, Some(WPARAM(161)), None);
            }
            pump_fonts();
            let controls = media_panel::review_font_handles(panel);
            let family = controls[0];
            let _weight = controls[1];
            let choose = |control: HWND, label: &str, id: usize| {
                let text = ui::wide(label);
                let index = unsafe {
                    SendMessageW(
                        control,
                        CB_FINDSTRING,
                        Some(WPARAM(usize::MAX)),
                        Some(LPARAM(text.as_ptr() as isize)),
                    )
                    .0
                };
                assert!(index >= 0, "missing {label}");
                unsafe {
                    SendMessageW(control, CB_SETCURSEL, Some(WPARAM(index as usize)), None);
                    SendMessageW(
                        panel,
                        WM_COMMAND,
                        Some(WPARAM(id | ((CBN_SELCHANGE as usize) << 16))),
                        Some(LPARAM(control.0 as isize)),
                    );
                }
            };
            choose(family, "Arial", 170);
            unsafe {
                SendMessageW(panel, WM_COMMAND, Some(WPARAM(171)), None);
            }
            unsafe {
                SendMessageW(panel, WM_COMMAND, Some(WPARAM(172)), None);
                SendMessageW(panel, WM_COMMAND, Some(WPARAM(173)), None);
            }
            unsafe {
                SendMessageW(panel, WM_COMMAND, Some(WPARAM(181)), None);
            }
            ui::text(controls[7], "70");
            ui::text(controls[8], "8");
            ui::text(controls[9], "#32A0E0");
            unsafe {
                SendMessageW(
                    panel,
                    WM_COMMAND,
                    Some(WPARAM(124 | ((CBN_EDITCHANGE as usize) << 16))),
                    Some(LPARAM(controls[9].0 as isize)),
                );
            }
            pump_fonts();
            let style = &unsafe { state_mut(h) }
                .unwrap()
                .editor
                .settings()
                .subtitle_style;
            assert_eq!(style.font.family, "Arial");
            assert_eq!(style.font.weight, 700);
            assert!(style.font.italic && style.font.underline);
            assert_eq!(
                style.font.alignment,
                panzo_core::text_style::TextAlignment::Right
            );
            assert_eq!(style.opacity, 179);
            assert_eq!(style.color, "#32A0E0");
            assert_eq!(style.font_size, 0.08);
            unsafe {
                SendMessageW(panel, WM_VSCROLL, Some(WPARAM(7)), None);
            }
            pump_fonts();
            capture_tools::review_present(root);
            capture_tools::review_window_png(root, &output.join("subtitle-font-controls.png"));
            let s = unsafe { state_mut(h) }.unwrap();
            let expected = s.editor.settings().subtitle_style.clone();
            s.undo();
            assert_ne!(s.editor.settings().subtitle_style, expected);
            s.redo();
            assert_eq!(s.editor.settings().subtitle_style, expected);
            s.editor
                .save(panzo_core::CameraEditKind::Style, None)
                .unwrap();
            s.close_at = Some(Instant::now() + Duration::from_millis(200));
        })),
    )
    .unwrap();
    assert!(report.graceful_close);
    let editor = crate::platform::editor::ProjectEditorSession::open(&project).unwrap();
    let expected = editor.settings().subtitle_style.clone();
    assert_eq!(expected.font.family, "Arial");
    assert!(expected.font.underline);
    use crate::platform::export::{
        ExportOptions, ExportPreset, ExportRequest, ExportSnapshot, ProjectExporter,
    };
    let mut images = Vec::new();
    for alternative in [false, true] {
        let mut settings = editor.settings().clone();
        if alternative {
            settings.subtitle_style.font.family = "Courier New".into();
            settings.subtitle_style.font.weight = 400;
            settings.subtitle_style.font.italic = false;
            settings.subtitle_style.font.underline = false;
        }
        let file = out.join(if alternative {
            "font-alternative.mp4"
        } else {
            "font-selected.mp4"
        });
        let result = ProjectExporter::export_snapshot_with_options(
            &ExportRequest {
                project_root: project.clone(),
                output_path: file.clone(),
                maximum_duration: Some(TimeTick::from_millis(1000)),
            },
            &ExportSnapshot {
                camera: editor.camera().clone(),
                settings,
            },
            ExportPreset::Native,
            ExportOptions {
                fps: 30,
                ..Default::default()
            },
            &std::sync::atomic::AtomicBool::new(false),
            |_, _| {},
        )
        .unwrap();
        assert_eq!(result.frames_written, 30);
        let mut decoder = crate::platform::media_decoder::MfBgraDecoder::open(&file).unwrap();
        let frame = decoder.read_next().unwrap().unwrap();
        let rgba = frame
            .pixels
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect::<Vec<_>>();
        image::save_buffer(
            out.join(if alternative {
                "font-alternative.png"
            } else {
                "font-selected.png"
            }),
            &rgba,
            frame.width,
            frame.height,
            image::ColorType::Rgba8,
        )
        .unwrap();
        images.push(frame.pixels);
    }
    let difference = images[0]
        .iter()
        .zip(images[1].iter())
        .filter(|(a, b)| a.abs_diff(**b) > 30)
        .count();
    assert!(
        difference > 300,
        "font did not reach encoded video: {difference}"
    );
    std::fs::write(out.join("font-video-report.json"),serde_json::to_vec_pretty(&serde_json::json!({"passed":true,"style":expected,"encodedPixelDifference":difference,"framesPerExport":30})).unwrap()).unwrap();
}
