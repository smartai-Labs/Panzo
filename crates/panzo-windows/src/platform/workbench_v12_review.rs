//! Opt-in regression on an isolated synthetic project, with its own native host.
use super::*;
use crate::platform::{editor_ui, ui_preferences::ThemeMode, workspace, workspace_ui as ui};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardState, SetKeyboardState};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetDlgItem, MSG, PM_REMOVE, PeekMessageW, SendMessageW, TranslateMessage,
    WM_COMMAND,
};

fn pump() {
    let mut msg = MSG::default();
    for _ in 0..100 {
        if !unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
fn render(state: &mut WindowState, hwnd: HWND, path: &Path) {
    state.frame = state.session.prepare_current().unwrap();
    let frame = state.session.compose_prepared(&state.frame).unwrap();
    let previous = state.fallback_frame.replace(frame);
    let r = client_rect(hwnd);
    editor_ui::render_review_png(path, r.right, r.bottom, |dc, rect| {
        draw_scene_region(dc, rect, rect, state)
    });
    state.fallback_frame = previous;
}
#[test]
#[ignore = "requires an isolated synthetic project, LOCALAPPDATA and native review directory"]
fn workbench_v12_native_review() {
    // Optional isolation for third-party TSF DLLs that crash when a test UI thread exits.
    // This affects this test thread only; production IME behavior is unchanged.
    if std::env::var_os("PANZO_UI_REVIEW_DISABLE_IME").is_some() {
        assert!(unsafe { windows::Win32::UI::Input::Ime::ImmDisableIME(0) }.as_bool());
    }
    let project = std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
    let out = std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
    assert!(std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).starts_with(&out));
    let package_path =
        std::path::PathBuf::from(std::env::var_os("PANZO_MEDIA_REVIEW_PACKAGE").unwrap());
    let package = crate::platform::plugins::read_package(&package_path).unwrap();
    crate::platform::plugins::Store::open(
        std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("Panzo/Plugins"),
    )
    .unwrap()
    .install(&package)
    .unwrap();
    assert!(crate::platform::plugins::enabled(
        panzo_plugin_api::media::PLUGIN_ID
    ));
    crate::platform::ui_preferences::set_theme(ThemeMode::Dark).unwrap();
    workspace::enable_review();
    let report = PreviewWindow::run_internal_notifying(&project, Some(Duration::from_mins(3)), SmokeAction::None, Some(Box::new(move |raw| {
        let hwnd = HWND(raw as *mut c_void);
        let state = unsafe {state_mut(hwnd)}.unwrap();
        state.theme = EditorTheme::for_mode(window_dpi(hwnd), ThemeMode::Dark);
        // Media rows impose a readable minimum; the GPU surface must follow it,
        // including after the last media item is removed or restored.
        let media_settings = state.editor.settings().clone();
        let mut without_media = media_settings.clone();
        without_media.audio_clips.clear(); without_media.subtitles.clear();
        state.theme.metrics.timeline_height = state.theme.scale(216);
        for settings in [without_media, media_settings] {
            state.editor.replace_media_settings(settings).unwrap();
            state.apply_editor_state().unwrap(); state.refresh(hwnd, true);
            let c = state.control_layout(client_rect(hwnd));
            let surface = state.preview_surface.as_ref().unwrap();
            assert_eq!(surface.bounds, state.preview_rect(client_rect(hwnd)));
            assert_eq!(surface.well, c.preview_well);
            for row in c.media_tracks.into_iter().filter(|r| r.bottom > r.top) {
                assert!(row.bottom - row.top >= state.theme.scale(24));
            }
        }
        state.theme.metrics.timeline_height = state.theme.scale(440);
        state.resize_preview_surface(hwnd);
        assert!(state.preview_surface.is_some(), "GPU preview must remain active");
        let visible: Vec<_> = state.visible_thumbnails(state.control_layout(client_rect(hwnd)).video_track).iter().map(|(_,t)|*t).collect();
        let started=Instant::now();
        loop {
            state.thumbnails.poll(state.scrub_preview.ready_proxy(),&visible,true);
            if visible.iter().all(|t|state.thumbnails.get(*t).is_some()) {break;}
            assert!(started.elapsed()<Duration::from_mins(2));
            std::thread::sleep(Duration::from_millis(5));
        }
        let original = state.editor.settings().clone();
        let camera = state.editor.camera().clone();
        let output = state.session.output();
        let initial = state.preview_rect(client_rect(hwnd));
        let controls = state.control_layout(client_rect(hwnd));
        let x = (initial.left + initial.right)/2 + state.theme.scale(30);
        let y = (initial.top + initial.bottom)/2;
        let mut keys = [0u8;256];
        unsafe {GetKeyboardState(&mut keys).unwrap();}
        let mut ctrl = keys; ctrl[usize::from(VK_CONTROL.0)] = 0x80;
        unsafe {SetKeyboardState(&ctrl).unwrap();}
        state.on_mouse_wheel(hwnd, 240, x, y);
        unsafe {SetKeyboardState(&keys).unwrap();}
        assert!(state.preview_view.zoom > 1.4);
        let zoomed = state.preview_rect(client_rect(hwnd));
        assert!(zoomed.right-zoomed.left > initial.right-initial.left);
        state.selected_segment = None;
        state.on_mouse_down(hwnd, x, y);
        assert!(state.preview_pan.is_some());
        state.on_mouse_move(hwnd, x + 70, y + 30);
        state.on_mouse_up(hwnd, x + 70, y + 30);
        let moved = state.preview_rect(client_rect(hwnd));
        assert_eq!((moved.left-zoomed.left,moved.top-zoomed.top),(70,30));
        assert_eq!(state.editor.settings(),&original);
        assert_eq!(state.editor.camera(),&camera);
        assert_eq!(state.session.output(),output);
        render(state,hwnd,&out.join("preview-zoom-pan-dark.png"));
        // The native GPU child is clipped to the well, even though its view is larger.
        let surface = state.preview_surface.as_ref().unwrap();
        let region = unsafe {windows::Win32::Graphics::Gdi::CreateRectRgn(0,0,0,0)};
        assert!(unsafe {windows::Win32::Graphics::Gdi::GetWindowRgn(surface.hwnd,region)}.0 > 0);
        let mut bounds = RECT::default();
        unsafe {windows::Win32::Graphics::Gdi::GetRgnBox(region,&mut bounds); let _=DeleteObject(region.into());}
        assert!(bounds.left+moved.left >= controls.preview_well.left);
        assert!(bounds.right+moved.left <= controls.preview_well.right);
        assert!(bounds.top+moved.top >= controls.preview_well.top);
        assert!(bounds.bottom+moved.top <= controls.preview_well.bottom);
        state.crop_editing=true;
        assert!(state.crop_hit(client_rect(hwnd),controls.subtitle_panel.left+4,controls.subtitle_panel.top+4).is_none());
        state.crop_editing=false;
        let fit = controls.preview_fit;
        state.on_mouse_down(hwnd,fit.left+4,fit.top+4);
        state.on_mouse_up(hwnd,fit.left+4,fit.top+4);
        assert_eq!(state.preview_view,preview_view::PreviewView::default());
        // A visible separator corresponds to the actual resize gesture.
        let split=controls.splitter;
        let sy=(split.top+split.bottom)/2;
        state.on_mouse_down(hwnd,split.left+40,sy);
        assert!(state.timeline_resize.is_some());
        state.on_mouse_move(hwnd,split.left+40,sy+110);
        state.on_mouse_up(hwnd,split.left+40,sy+110);
        assert!(state.theme.metrics.timeline_height < state.theme.scale(440));
        for (mode,name) in [(ThemeMode::Dark,"dark"),(ThemeMode::Light,"light")] {
            state.theme.set_mode(mode);
            state.theme.metrics.timeline_height = state.theme.scale(440);
            state.resize_preview_surface(hwnd);
            render(state,hwnd,&out.join(format!("workbench-{name}.png")));
            crate::platform::app_menu::review::next_popup(out.join(format!("menu-{name}.png")));
            state.application_menu(hwnd);
        }
        state.theme.set_mode(ThemeMode::Dark);
        let c=state.control_layout(client_rect(hwnd));
        state.on_mouse_down(hwnd,c.subtitle_panel.left+4,c.subtitle_panel.top+4);
        state.on_mouse_up(hwnd,c.subtitle_panel.left+4,c.subtitle_panel.top+4);
        pump();
        let panel=workspace::review_form(1);
        assert!(!panel.is_invalid(), "subtitle toolbar must directly open the panel");
        let add=unsafe {GetDlgItem(Some(panel),31)}.unwrap();
        let ai=unsafe {GetDlgItem(Some(panel),32)}.unwrap();
        assert_eq!(ui::value(add), "添加字幕");
        assert_eq!(ui::value(ai), "AI 自动字幕");
        crate::platform::capture_tools::review_window_png(panel,&out.join("subtitle-panel.png"));
        let before=state.editor.settings().subtitles.len();
        unsafe {SendMessageW(panel,WM_COMMAND,Some(WPARAM(31)),None);}
        assert_eq!(state.editor.settings().subtitles.len(),before+1);
        let text=unsafe {GetDlgItem(Some(panel),140)}.unwrap();
        ui::text(text,"字幕入口回归测试");
        unsafe {SendMessageW(panel,WM_COMMAND,Some(WPARAM(50)),None);}
        assert!(state.editor.settings().subtitles.iter().any(|c|c.text=="字幕入口回归测试"));
        unsafe {SendMessageW(panel,WM_COMMAND,Some(WPARAM(32)),None);}
        // The isolated profile has no API key: validation stays inline and sends no request.
        assert!(!ui::value(unsafe {GetDlgItem(Some(panel),51)}.unwrap()).is_empty());
        state.activate_control(hwnd,ControlId::AudioPanel);
        pump();
        assert_eq!(ui::value(unsafe {GetDlgItem(Some(panel),30)}.unwrap()),"导入音频");
        crate::platform::capture_tools::review_window_png(panel,&out.join("audio-panel.png"));
        state.editor.replace_media_settings(original).unwrap();
        state.apply_editor_state().unwrap();
        state.close_at=Some(Instant::now()+Duration::from_millis(200));
        std::fs::write(out.join("workbench-v12-review.txt"),"PASS: direct toolbar audio/subtitle entry; manual caption insertion/edit; AI validation inline; native menu frames; Ctrl-wheel view zoom; preview drag; GPU region clipping; fit reset; divider resize; unchanged project settings/camera/output during view gestures.\n").unwrap();
    }))).unwrap();
    assert!(report.window_created);
    assert!(report.gpu_direct_preview);
    assert!(report.graceful_close);
}
