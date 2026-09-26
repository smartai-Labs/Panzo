use super::*;
use std::path::PathBuf;

fn settle(state: &mut WindowState) {
    let frame = state.session.prepare_current().unwrap();
    state.accept_prepared(frame, 0).unwrap();
}

#[test]
#[ignore = "explicit synthetic project and native UI output required"]
#[allow(clippy::too_many_lines)] // A single crop/canvas workflow validates dependent native state transitions.
fn workflow_ui_review() {
    let project = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
    let output = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
    let report = PreviewWindow::run_internal_notifying(
        &project,
        Some(Duration::from_millis(300)),
        SmokeAction::None,
        Some(Box::new(move |raw| {
            let hwnd = HWND(raw as *mut c_void);
            let state = unsafe { state_mut(hwnd) }.unwrap();
            let original = state.editor.settings().clone();
            state.selected_video = Some(state.editor.video_edit().clips[0].id.clone());
            state.activate_control(hwnd, ControlId::VideoTab);
            state.activate_control(hwnd, ControlId::CropSquare);
            settle(state);
            state.activate_control(hwnd, ControlId::CropEdit);
            settle(state);
            assert!(state.crop_editing);
            assert!(state.frame.evaluation.camera_transform.crop.is_full());
            let overlay = state.crop_overlay(&state.frame).unwrap();
            let video = preview_video_rect(client_rect(hwnd), &state.frame, state.theme);
            let x = video.left + (overlay[0] * f64::from(video.right - video.left)).round() as i32;
            let y = video.top
                + (f64::midpoint(overlay[1], overlay[3]) * f64::from(video.bottom - video.top))
                    .round() as i32;
            let before = state.selected_crop().unwrap();
            state.on_mouse_down(hwnd, x, y);
            assert!(state.crop_drag.is_some());
            state.on_mouse_move(hwnd, x + 32, y);
            assert!(state.selected_crop().unwrap().left > before.left);
            state.on_key(hwnd, VK_ESCAPE.0);
            assert_eq!(state.selected_crop().unwrap(), before);
            state.on_mouse_down(hwnd, x, y);
            state.on_mouse_move(hwnd, x + 32, y);
            state.on_mouse_up(hwnd, x + 32, y);
            assert!(state.selected_crop().unwrap().left > before.left);
            state.undo();
            settle(state);
            assert_eq!(state.selected_crop().unwrap(), before);
            state.redo();
            settle(state);
            state.activate_control(hwnd, ControlId::CropLock);
            assert!(state.crop_locked);
            let locked = state.selected_crop().unwrap();
            state.apply_widget_range(
                hwnd,
                ControlId::CropLeft,
                f64::from(locked.left) / 10.0 + 2.0,
            );
            let resized = state.selected_crop().unwrap();
            let ratio = |crop: panzo_core::VideoCrop| {
                f64::from(1000 - crop.left - crop.right) / f64::from(1000 - crop.top - crop.bottom)
            };
            assert!((ratio(locked) - ratio(resized)).abs() < 0.004);
            settle(state);
            for mode in [
                crate::platform::ui_preferences::ThemeMode::Dark,
                crate::platform::ui_preferences::ThemeMode::Light,
            ] {
                for dpi in [96, 144, 192] {
                    state.theme = EditorTheme::for_mode(dpi, mode);
                    state
                        .session
                        .set_crop_overlay(state.crop_overlay(&state.frame));
                    state.fallback_frame =
                        Some(state.session.compose_prepared(&state.frame).unwrap());
                    let _drawing = crate::platform::editor_ui::DrawingScale::enter(dpi);
                    crate::platform::editor_ui::render_review_png(
                        &output.join(format!("workflow-crop-{mode:?}-{dpi}.png")),
                        1920,
                        1200,
                        |dc, rect| draw_scene_region(dc, rect, rect, state),
                    );
                }
            }
            state.theme = EditorTheme::current(window_dpi(hwnd));
            state.on_key(hwnd, VK_RETURN.0);
            settle(state);
            assert!(!state.crop_editing);
            assert!(!state.frame.evaluation.camera_transform.crop.is_full());
            state.activate_control(hwnd, ControlId::CropEdit);
            settle(state);
            let tracks = state.control_layout(client_rect(hwnd));
            state.on_mouse_down(
                hwnd,
                tracks.video_track.left + 30,
                tracks.video_track.top + 12,
            );
            state.on_mouse_up(
                hwnd,
                tracks.video_track.left + 30,
                tracks.video_track.top + 12,
            );
            assert!(
                !state.crop_editing,
                "selecting a clip must close the source editing view"
            );
            settle(state);
            state.activate_control(hwnd, ControlId::SpeedSection);
            let layout = state.control_layout(client_rect(hwnd));
            assert_eq!(layout.video_speed.right, 0);
            assert!(layout.crop_presets[0].top < state.theme.scale(230));
            state.activate_control(hwnd, ControlId::CropPortrait);
            settle(state);
            state.activate_control(hwnd, ControlId::CanvasTab);
            state.activate_control(hwnd, ControlId::CanvasPortrait);
            settle(state);
            assert_eq!(
                (state.session.output().width, state.session.output().height),
                (1440, 2560)
            );
            state.inspector_scroll = 10_000;
            state.fallback_frame = Some(state.session.compose_prepared(&state.frame).unwrap());
            crate::platform::editor_ui::render_review_png(
                &output.join("workflow-portrait.png"),
                1440,
                1000,
                |dc, rect| draw_scene_region(dc, rect, rect, state),
            );
            state.inspector_scroll = 10_000;
            state.begin_inline_input(hwnd, ControlId::CanvasWidth);
            let field = state.inline_input.as_ref().unwrap().hwnd;
            unsafe {
                SetWindowTextW(field, w!("1919")).unwrap();
            }
            assert!(!state.finish_inline_input(hwnd, true));
            unsafe {
                SetWindowTextW(field, w!("1080")).unwrap();
            }
            assert!(state.finish_inline_input(hwnd, true));
            settle(state);
            assert_eq!(state.session.output().width, 1080);
            state.fallback_frame = Some(state.session.compose_prepared(&state.frame).unwrap());
            crate::platform::editor_ui::render_review_png(
                &output.join("workflow-custom-canvas.png"),
                1440,
                1000,
                |dc, rect| draw_scene_region(dc, rect, rect, state),
            );
            state.seek_absolute(TimeTick::ZERO);
            state.step_frame(true);
            assert_eq!(state.session.snapshot().project_tick, TimeTick(166_667));
            state.step_frame(false);
            assert_eq!(state.session.snapshot().project_tick, TimeTick::ZERO);
            state
                .editor
                .split_video(TimeTick::from_millis(2000))
                .unwrap();
            state.apply_editor_state().unwrap();
            state.seek_clip_boundary(true);
            assert_eq!(
                state.session.snapshot().project_tick,
                TimeTick::from_millis(2000)
            );
            state.seek_clip_boundary(false);
            assert_eq!(state.session.snapshot().project_tick, TimeTick::ZERO);
            while state.editor.can_undo() {
                state.undo();
            }
            settle(state);
            assert_eq!(state.editor.settings(), &original);
            assert_eq!(state.session.output().width, 2560);
        })),
    )
    .unwrap();
    assert_eq!(report.media_errors, 0);
}
