use super::*;
use std::path::PathBuf;

#[test]
#[ignore = "explicit synthetic project and UI review output directory required"]
#[allow(clippy::too_many_lines)] // Exercise selection, gestures, fields, history and visible crop in one native workflow.
fn video_crop_ui_review() {
    let project = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
    let output = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
    let report = PreviewWindow::run_internal_notifying(
        &project,
        Some(Duration::from_millis(300)),
        SmokeAction::None,
        Some(Box::new(move |raw| {
            let hwnd = HWND(raw as *mut std::ffi::c_void);
            let state = unsafe { state_mut(hwnd) }.unwrap();
            let original = state.editor.video_edit();
            let right = state
                .editor
                .split_video(TimeTick::from_millis(2000))
                .unwrap();
            state.apply_editor_state().unwrap();
            state.session.seek(TimeTick::from_millis(1000)).unwrap();
            state.selected_video = Some(right.clone());
            state.activate_control(hwnd, ControlId::VideoTab);
            state.activate_control(hwnd, ControlId::CropSquare);
            assert!(state.session.snapshot().project_tick >= TimeTick::from_millis(2000));
            assert!(state.editor.video_edit().clips[0].crop.is_full());
            let square = state.selected_crop().unwrap();
            assert!(!square.is_full());
            assert_eq!(state.editor.project_duration(), original.duration());
            state.inspector_scroll = 10_000;
            state.begin_inline_input(hwnd, ControlId::CropLeft);
            let field = state.inline_input.as_ref().unwrap().hwnd;
            unsafe {
                SetWindowTextW(field, w!("99")).unwrap();
            }
            assert!(!state.finish_inline_input(hwnd, true));
            assert_eq!(state.selected_crop().unwrap(), square);
            unsafe {
                SetWindowTextW(field, w!("12.5")).unwrap();
            }
            assert!(state.finish_inline_input(hwnd, true));
            assert_eq!(state.widget_value(ControlId::CropLeft), Some(12.5));
            let before = state.selected_crop().unwrap();
            let rect = state
                .control_layout(client_rect(hwnd))
                .control_rect(ControlId::CropTop);
            let y = rect.bottom - state.theme.scale(8);
            state.on_mouse_down(hwnd, rect.left + state.theme.scale(5), y);
            assert_eq!(state.property_drag, Some(ControlId::CropTop));
            state.on_mouse_move(hwnd, rect.right - state.theme.scale(5), y);
            state.cancel_gesture(hwnd);
            assert_eq!(state.selected_crop().unwrap(), before);
            state.on_mouse_down(hwnd, rect.left + state.theme.scale(5), y);
            state.on_mouse_move(hwnd, i32::midpoint(rect.left, rect.right), y);
            state.on_mouse_up(hwnd, i32::midpoint(rect.left, rect.right), y);
            assert!(state.selected_crop().unwrap().top > 0);
            state.undo();
            assert_eq!(state.selected_crop().unwrap(), before);
            state.redo();
            assert!(state.selected_crop().unwrap().top > 0);
            state.activate_control(hwnd, ControlId::CropReset);
            assert!(state.selected_crop().unwrap().is_full());
            state.activate_control(hwnd, ControlId::CropPortrait);
            state.activate_control(hwnd, ControlId::CanvasTab);
            assert_eq!(
                state
                    .control_layout(client_rect(hwnd))
                    .control_rect(ControlId::CropTop)
                    .right,
                0
            );
            state.activate_control(hwnd, ControlId::VideoTab);
            state.frame = state.session.prepare_current().unwrap();
            state.fallback_frame = Some(state.session.compose_prepared(&state.frame).unwrap());
            for mode in [
                crate::platform::ui_preferences::ThemeMode::Dark,
                crate::platform::ui_preferences::ThemeMode::Light,
            ] {
                for (dpi, width, height) in [(96, 1440, 1000), (144, 1920, 1200), (192, 1920, 1400)]
                {
                    state.theme = EditorTheme::for_mode(dpi, mode);
                    for scroll in [0, 10_000] {
                        state.inspector_scroll = scroll;
                        let _drawing = crate::platform::editor_ui::DrawingScale::enter(dpi);
                        crate::platform::editor_ui::render_review_png(
                            &output.join(format!("crop-{mode:?}-{dpi}-{scroll}.png")),
                            width,
                            height,
                            |dc, rect| draw_scene_region(dc, rect, rect, state),
                        );
                    }
                }
            }
            while state.editor.can_undo() {
                state.undo();
            }
            assert_eq!(state.editor.video_edit(), original);
            assert!(!state.editor.is_dirty());
            state.activate_control(hwnd, ControlId::WindowClose);
        })),
    )
    .unwrap();
    assert!(report.graceful_close);
    assert_eq!(report.media_errors, 0);
}
