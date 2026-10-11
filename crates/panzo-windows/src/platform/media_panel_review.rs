//! Opt-in native checks: isolated project/profile, synthetic audio only; never starts a microphone.
use super::*;
use crate::platform::{
    capture_tools, media_panel, ui_preferences::ThemeMode, workspace, workspace_ui as ui,
};
use std::path::PathBuf;
use windows::Win32::UI::WindowsAndMessaging::*;
fn pump() {
    let mut message = MSG::default();
    for _ in 0..100 {
        if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
fn control(panel: HWND, id: i32) -> HWND {
    unsafe { GetDlgItem(Some(panel), id) }.unwrap()
}
#[test]
#[ignore = "requires isolated synthetic fixture, profile, native desktop and output directory"]
fn media_panel_native_review() {
    let project = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
    let out = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
    assert!(PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).starts_with(&out));
    let package = crate::platform::plugins::read_package(&PathBuf::from(
        std::env::var_os("PANZO_MEDIA_REVIEW_PACKAGE").unwrap(),
    ))
    .unwrap();
    crate::platform::plugins::Store::open(
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("Panzo/Plugins"),
    )
    .unwrap()
    .install(&package)
    .unwrap();
    workspace::enable_review();
    crate::platform::ui_preferences::set_theme(ThemeMode::Dark).unwrap();
    let report=PreviewWindow::run_internal_notifying(&project,Some(Duration::from_mins(2)),SmokeAction::None,Some(Box::new(move |raw| {
        let hwnd=HWND(raw as *mut c_void);
        let state=unsafe{state_mut(hwnd)}.unwrap();
        let original=state.editor.settings().clone();
        let original_video=state.editor.video_edit();
        let duration=state.editor.project_duration();
        let span=TimeTick(duration.0/2);
        let id=uuid::Uuid::new_v4().to_string();
        let path=format!("media/audio-clips/{id}.pcm");
        let frames=crate::platform::audio::frames_at(span);
        std::fs::create_dir_all(state.editor.project_root().join("media/audio-clips")).unwrap();
        std::fs::write(state.editor.project_root().join(&path),vec![0_u8;frames as usize*4]).unwrap();
        let audio=panzo_core::media_edit::AudioClip {speed_percent:100,id:id.clone(),name:"旁白示例".into(),path,asset_frames:frames,start:TimeTick::ZERO,source_in:TimeTick::ZERO,source_out:crate::platform::audio::tick_at(frames),role:panzo_core::media_edit::AudioRole::Narration,gain:1.0,muted:false,fade_in:TimeTick::ZERO,fade_out:TimeTick::ZERO,waveform:vec![0.2;100]};
        let mut settings=original.clone(); settings.audio_clips.push(audio);
        settings.subtitles=vec![panzo_core::media_edit::SubtitleCue{id:"review-subtitle".into(),start:TimeTick::ZERO,end:span,text:"在一个工作台完成剪辑与字幕".into(),anchor:panzo_core::media_edit::TimeAnchor::Project}];
        state.editor.replace_media_settings(settings.clone()).unwrap();state.apply_editor_state().unwrap();
        state.activate_control(hwnd,ControlId::AudioPanel);pump();
        let panel=workspace::review_form(1);assert!(!panel.is_invalid());
        for id in [33,34,50,120,121,122,123,124,125,140] {assert!(!unsafe{IsWindowVisible(control(panel,id))}.as_bool(),"old editing controls must be hidden: {id}");}
        let devices=crate::platform::audio_capture::devices(true).unwrap();
        let source=control(panel,61);
        for (index,device) in devices.iter().enumerate() {
            unsafe{SendMessageW(source,CB_SETCURSEL,Some(WPARAM(index+1)),None);}
            assert_eq!(ui::value(source),device.name);
            assert_eq!(media_panel::review_device(panel).unwrap().as_deref(),Some(device.id.as_str()));
            // Resolve the actual endpoint ID; activation does not start capture or save audio.
            crate::platform::audio_capture::open_client(true,Some(&device.id)).unwrap();
        }
        if !devices.is_empty() {
            unsafe{SendMessageW(source,CB_SETCURSEL,Some(WPARAM(1)),None);SendMessageW(panel,WM_COMMAND,Some(WPARAM(38)),None);}
            assert_eq!(media_panel::review_device(panel).unwrap().as_deref(),Some(devices[0].id.as_str()));
        }
        for (theme,name) in [(ThemeMode::Dark,"dark"),(ThemeMode::Light,"light")] {
            crate::platform::ui_preferences::set_theme(theme).unwrap();
            crate::platform::desktop_ui::refresh(panel);pump();
            capture_tools::review_present(workspace::root().unwrap());
            capture_tools::review_window_png(panel,&out.join(format!("audio-panel-{name}.png")));
        }
        crate::platform::ui_preferences::set_theme(ThemeMode::Dark).unwrap();
        crate::platform::desktop_ui::refresh(panel);pump();
        // Selection stays on the timeline and does not switch the capture panel.
        let c=state.control_layout(client_rect(hwnd));
        let x=timeline_x(c.media_tracks[0],TimeTick(span.0/3),state.timeline);
        let y=(c.media_tracks[0].top+c.media_tracks[0].bottom)/2;
        state.on_mouse_down(hwnd,x,y);state.on_mouse_up(hwnd,x,y);pump();
        assert_eq!(state.media_selected,Some((false,id.clone())));
        assert_eq!(workspace::review_form(1),panel);
        assert!(state.control_enabled(ControlId::Delete));
        state.session.seek(TimeTick(span.0/2)).unwrap();
        assert!(state.control_enabled(ControlId::Split));
        state.activate_control(hwnd,ControlId::Split);
        assert_eq!(state.editor.settings().audio_clips.len(),settings.audio_clips.len()+1);
        assert_eq!(state.editor.video_edit(),original_video);
        workspace_command(hwnd,240);
        assert!(!state.editor.settings().audio_clips.iter().any(|c|c.id==id));
        state.undo();state.undo();
        assert_eq!(state.editor.settings().audio_clips,settings.audio_clips);
        // Subtitle split/delete use the same toolbar commands and preserve video.
        select_workspace_media(hwnd,true,Some("review-subtitle"));
        state.activate_control(hwnd,ControlId::Split);
        assert_eq!(state.editor.settings().subtitles.len(),2);
        workspace_command(hwnd,240);assert_eq!(state.editor.settings().subtitles.len(),1);
        state.undo();state.undo();assert_eq!(state.editor.settings().subtitles,settings.subtitles);
        // Source-anchored captions retain their anchor when split on the project timeline.
        let mut anchored=state.editor.settings().clone();
        anchored.subtitles[0].anchor=panzo_core::media_edit::TimeAnchor::Source;
        let anchored_before=anchored.subtitles.clone();
        state.editor.replace_media_settings(anchored).unwrap(); state.apply_editor_state().unwrap();
        select_workspace_media(hwnd,true,Some("review-subtitle"));
        state.split_media();
        assert_eq!(state.editor.settings().subtitles.len(),2);
        assert!(state.editor.settings().subtitles.iter().all(|cue|cue.anchor==panzo_core::media_edit::TimeAnchor::Source));
        state.undo();assert_eq!(state.editor.settings().subtitles,anchored_before);
        state.undo();
        // Audio dragging and edge trimming remain timeline gestures with one undo step.
        let c=state.control_layout(client_rect(hwnd));
        let row=c.media_tracks[0];
        let x=timeline_x(row,TimeTick(span.0/3),state.timeline);
        let y=(row.top+row.bottom)/2;
        assert_eq!(state.media_cursor(client_rect(hwnd),x,y),Some(IDC_HAND));
        state.on_mouse_down(hwnd,x,y);state.on_mouse_move(hwnd,x+40,y);state.on_mouse_up(hwnd,x+40,y);
        assert!(state.editor.settings().audio_clips.iter().find(|c|c.id==id).unwrap().start>TimeTick::ZERO);
        state.undo();
        let audio=state.editor.settings().audio_clips.iter().find(|c|c.id==id).unwrap().clone();
        let end=timeline_x(row,audio.end(),state.timeline)-2;
        assert_eq!(state.media_cursor(client_rect(hwnd),end,y),Some(IDC_SIZEWE));
        state.on_mouse_down(hwnd,end,y);state.on_mouse_move(hwnd,end-30,y);state.on_mouse_up(hwnd,end-30,y);
        assert!(state.editor.settings().audio_clips.iter().find(|c|c.id==id).unwrap().source_out<audio.source_out);
        state.undo();
        assert_eq!(state.editor.settings().audio_clips,settings.audio_clips);
        assert_eq!(state.editor.video_edit(),original_video);
        // Audio properties originate from a track action, not from the recording panel.
        state.media_audio_properties(hwnd,&id).unwrap();pump();
        let properties=workspace::review_form(1);
        ui::text(control(properties,103),"80");
        unsafe{SendMessageW(properties,WM_COMMAND,Some(WPARAM(1)),None);}
        // Workspace forms assign apply command IDs starting at 1.
        assert_eq!(state.editor.settings().audio_clips.iter().find(|c|c.id==id).unwrap().gain,0.8);
        state.undo();
        state.activate_control(hwnd,ControlId::SubtitlePanel);pump();
        media_panel::select_item(hwnd,true,"review-subtitle");pump();
        let text=control(panel,140);ui::text(text,"字幕内容已校对");
        unsafe{SendMessageW(panel,WM_COMMAND,Some(WPARAM(50)),None);}
        assert_eq!(state.editor.settings().subtitles[0].text,"字幕内容已校对");
        for (theme,name) in [(ThemeMode::Dark,"dark"),(ThemeMode::Light,"light")] {
            crate::platform::ui_preferences::set_theme(theme).unwrap();crate::platform::desktop_ui::refresh(panel);
            unsafe{SendMessageW(panel,WM_VSCROLL,Some(WPARAM(6)),None);}pump();
            capture_tools::review_present(workspace::root().unwrap());
            capture_tools::review_window_png(panel,&out.join(format!("subtitles-panel-{name}.png")));
        }
        // Removing a selected subtitle clears the editor, including after a text draft.
        select_workspace_media(hwnd,true,Some("review-subtitle"));state.delete_selected();
        unsafe{SendMessageW(panel,WM_TIMER,Some(WPARAM(1)),None);}assert_eq!(ui::value(text),"");
        assert_eq!(state.editor.video_edit(),original_video);
        state.editor.replace_media_settings(original).unwrap();state.apply_editor_state().unwrap();
        state.close_at=Some(Instant::now()+Duration::from_millis(200));
        std::fs::write(out.join("review.txt"),format!("PASS: themed media panels and separator; friendly names and stable IDs for {} active inputs; endpoint activation without recording; device refresh preserves selection; timeline audio/subtitle selection, split, delete and undo; source-anchored subtitle split; audio drag/trim and pointer feedback; audio properties, subtitle editing and stale selection cleanup. No microphone capture or AI network request.\n",devices.len())).unwrap();
    }))).unwrap();
    assert!(report.window_created && report.graceful_close);
}
