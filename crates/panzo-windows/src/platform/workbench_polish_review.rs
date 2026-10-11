use super::*;
use panzo_core::{media_edit::{AudioClip,AudioRole,TimeAnchor,SubtitleCue},timeline::{VisualAsset,VisualKind}};
use windows::Win32::UI::WindowsAndMessaging::*;
pub(super) fn run(h:HWND,root:HWND,out:&Path) {
    let s=unsafe{state_mut(h)}.unwrap();
    let fixture=s.editor.project_root().to_path_buf();
    let mut next=panzo_core::WorkbenchSettings::default();
    next.camera_enabled=false;next.canvas.inset=0.0;
    next.canvas.size=Some(panzo_core::CanvasSize{width:640,height:360});
    let mut video=panzo_core::VideoEdit::full(s.editor.duration()).unwrap();
    let video_id=video.clips[0].id.clone();video.trim(&video_id,TimeTick::ZERO,TimeTick(10_000_000)).unwrap();next.video=Some(video);
    std::fs::create_dir_all(fixture.join("media/audio-clips")).unwrap();
    let audio_path=format!("media/audio-clips/{}.pcm",uuid::Uuid::new_v4());
    let pcm=fixture.join(&audio_path);let mut bytes=Vec::new();
    for i in 0..144000 {let v:i16=if (48000..72000).contains(&i){0}else if i%120<60{10000}else{-10000};bytes.extend(v.to_le_bytes());bytes.extend(v.to_le_bytes());}
    std::fs::write(&pcm,bytes).unwrap();
    let clip=AudioClip{speed_percent:100,id:"audio-cue".into(),name:"旁白 · 测试录音".into(),path:audio_path,asset_frames:144000,start:TimeTick::ZERO,source_in:TimeTick::ZERO,source_out:TimeTick(30_000_000),role:AudioRole::Music,gain:1.0,muted:false,fade_in:TimeTick::ZERO,fade_out:TimeTick::ZERO,waveform:(0..192).map(|i|if (64..96).contains(&i){0.0}else{0.3}).collect()};
    next.audio_assets.push(panzo_core::media_edit::AudioAsset::from_clip(&clip));
    next.audio_clips.push(clip);
    let audio_id=next.audio_assets[0].id.clone();
    let image_path=fixture.join("media/polish-picture.png");image::RgbaImage::from_pixel(320,240,image::Rgba([40,180,95,255])).save(&image_path).unwrap();
    next.visual_assets.push(VisualAsset{id:"image".into(),name:"绿色图片".into(),path:"media/polish-picture.png".into(),kind:VisualKind::Image,duration:TimeTick(30_000_000),width:320,height:240,audio_path:None,audio_frames:0,source_audio:vec![]});
    s.editor.replace_media_settings(next).unwrap();s.apply_editor_state().unwrap();s.refresh(h,true);pump();
    s.seek_absolute(TimeTick(5_000_000));pump();
    let project_tick=s.session.snapshot().project_tick;let unchanged=s.editor.settings().clone();
    let surface=s.preview_surface.as_ref().unwrap().hwnd;
    let readbacks=s.session.compositor_cache_stats().readbacks;
    let mut costs=vec![];
    for id in [&audio_id,"image","__source__",&audio_id,"image","__source__",&audio_id,"image"] {
        let start=Instant::now();
        project_bin::review_preview_asset(s.project_bin.as_ref().unwrap(),id);pump();
        costs.push(start.elapsed().as_micros() as u64);
        assert!(s.session.is_asset_preview());assert_eq!(s.preview_surface.as_ref().unwrap().hwnd,surface,"reuse the same GPU window");
        assert_eq!(s.editor.settings(),&unchanged,"audition must not edit project");
        assert_eq!(s.session.project_playhead(),project_tick);
        s.activate_control(h,ControlId::Home);assert_eq!(s.session.snapshot().project_tick,TimeTick::ZERO);
        let seek=s.preview_seek_rect(client_rect(h));let x=(seek.left+seek.right)/2;let y=seek.top+1;
        s.on_mouse_down(h,x,y);s.on_mouse_up(h,x,y);
        assert!((s.session.snapshot().project_tick.0-s.session.snapshot().duration_tick.0/2).abs()<=s.session.snapshot().duration_tick.0/i64::from((seek.right-seek.left).max(1))+1,"seek is accurate within one display pixel");

        s.activate_control(h,ControlId::Last);assert_eq!(s.session.snapshot().project_tick,s.session.snapshot().duration_tick);
        s.select_preview_source(false).unwrap();pump();
        assert_eq!(s.session.snapshot().project_tick,project_tick);
        assert_eq!(s.preview_surface.as_ref().unwrap().hwnd,surface);
    }
    assert_eq!(s.session.compositor_cache_stats().readbacks,readbacks,"normal switching stays on GPU");
    let c=s.control_layout(client_rect(h));assert!(c.home.left>c.transport.left+s.theme.scale(190));assert!(c.time.right<=c.preview_mute.left);
    snap(root,out,"shared-player");
    // A conflicting insert is atomic; another audio track remains legal.
    let mut clash=s.editor.settings().clone();clash.insert_audio_asset(&audio_id,"music",TimeTick(10_000_000)).unwrap();
    assert!(s.editor.replace_media_settings(clash).is_err());assert_eq!(s.editor.settings(),&unchanged);
    // Editing a selected project clip while auditioning uses the saved project playhead.
    media_panel::select_item(h,false,"audio-cue");pump();
    project_bin::review_preview_asset(s.project_bin.as_ref().unwrap(),&audio_id);pump();
    s.activate_control(h,ControlId::Last);
    assert!(s.control_enabled(ControlId::Split));
    s.activate_control(h,ControlId::Split);pump();
    assert!(!s.session.is_asset_preview());
    assert_eq!(s.editor.settings().audio_clips.len(),2);
    assert_eq!(s.editor.settings().audio_clips[0].end(),project_tick);
    s.undo();pump();let mut restored=s.editor.settings().clone();restored.revision=unchanged.revision;assert_eq!(restored,unchanged);
    // Audio values update the model immediately, without moving source ranges.
    media_panel::select_item(h,false,"audio-cue");pump();let panel=inspector_panel(h).unwrap();let body=media_panel::review_body(panel);
    let field=|id|unsafe{GetDlgItem(Some(body),id)}.unwrap();
    for id in [120,121,122] {assert!(!unsafe{IsWindowVisible(field(id))}.as_bool());}
    let apply=unsafe{GetDlgItem(Some(panel),50)}.unwrap();assert!(!unsafe{IsWindowVisible(apply)}.as_bool());
    ui::text(field(123),"78.5");ui::text(field(124),"0.400");ui::text(field(125),"0.500");pump();
    let audio=&s.editor.settings().audio_clips[0];assert_eq!(audio.gain,0.785);assert_eq!(audio.fade_in,TimeTick(4_000_000));assert_eq!(audio.fade_out,TimeTick(5_000_000));assert_eq!(audio.start,TimeTick::ZERO);
    for value in ["90.0","100.0","45.0","78.5"] {ui::text(field(123),value);pump();}
    snap(root,out,"live-audio-and-waveform");
    // The panel's labels remain fully painted and aligned after focus, edits and resizing.
    for width in [272,400,304] {s.theme.metrics.inspector_width=s.theme.scale(width);s.refresh(h,true);pump();ui::text(field(124),"0.600");pump();}
    snap(root,out,"audio-after-resize");
    select(h,ControlId::SubtitleTab);let panel=inspector_panel(h).unwrap();let body=media_panel::review_body(panel);
    let input=|id|unsafe{GetDlgItem(Some(body),id)}.unwrap();
    assert!(!unsafe{IsWindowVisible(input(161))}.as_bool());assert!(!unsafe{IsWindowVisible(input(160))}.as_bool());
    assert!(!unsafe{IsWindowVisible(input(170))}.as_bool(),"style only with selection");
    unsafe{SendMessageW(panel,WM_COMMAND,Some(WPARAM(31)),None);}pump();
    let id=s.media_selected.as_ref().unwrap().1.clone();let at=s.editor.settings().subtitles[0].start;
    let typed=ui::wide("即时字幕");
    unsafe{SendMessageW(input(140),0xb1,Some(WPARAM(0)),Some(LPARAM(-1)));SendMessageW(input(140),0xc2,Some(WPARAM(1)),Some(LPARAM(typed.as_ptr() as isize)));}
    ui::text(input(122),"32");
    unsafe{SendMessageW(panel,WM_COMMAND,Some(WPARAM(122|((CBN_EDITCHANGE as usize)<<16))),None);}
    pump();assert_eq!(s.editor.settings().subtitles[0].text,"即时字幕");
    assert!((s.editor.settings().subtitle_style_for(&id).font_size*360.-32.).abs()<0.01);
    assert_eq!(s.editor.settings().subtitle_style,panzo_core::media_edit::SubtitleStyle::default(),"default must remain unchanged");
    unsafe{SendMessageW(panel,WM_COMMAND,Some(WPARAM(172)),None);}pump();
    assert!(s.editor.settings().subtitle_style_for(&id).font.italic);
    assert!(unsafe{IsWindowVisible(input(170))}.as_bool());assert!(!unsafe{IsWindowVisible(input(36))}.as_bool());
    // Adjacent subtitle, inherited default, and overlap rejection while typing.
    let mut next=s.editor.settings().clone();next.subtitles.push(SubtitleCue{id:"second".into(),start:TimeTick(at.0+20_000_000),end:TimeTick(at.0+40_000_000),text:"第二条".into(),anchor:TimeAnchor::Project});
    s.editor.replace_media_settings(next).unwrap();s.apply_editor_state().unwrap();
    media_panel::select_item(h,true,&id);pump();
    assert_ne!(s.editor.settings().subtitle_style_for(&id),s.editor.settings().subtitle_style_for("second"));
    unsafe{let _=windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(input(121)));}
    ui::text(input(121),&format!("{:.3}",(at.0+30_000_000) as f64/10_000_000.));pump();
    assert_eq!(s.editor.settings().subtitles[0].end,TimeTick(at.0+20_000_000));
    unsafe{let _=windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(input(140)));}
    pump();assert_eq!(ui::value(input(121)),format!("{:.3}",(at.0+20_000_000) as f64/10_000_000.));
    snap(root,out,"selected-subtitle-style");
    unsafe{SendMessageW(panel,WM_VSCROLL,Some(WPARAM(7)),None);}pump();snap(root,out,"subtitle-style-scrolled");
    s.editor.save(CameraEditKind::Update,None).unwrap();
    let reopened=crate::platform::editor::ProjectEditorSession::open(&fixture).unwrap();assert_eq!(reopened.settings().subtitle_styles,s.editor.settings().subtitle_styles);
    // A cancellable countdown is a child of the preview, no modal loop.
    let cancel=Arc::new(AtomicBool::new(false));let overlay=crate::platform::workbench_overlays::Overlay::countdown(h,cancel).unwrap();overlay.count(3);snap(root,out,"narration-countdown");drop(overlay);
    // Native drag capture persists until mouse-up; cancellation removes its card.
    let bin=s.project_bin.as_ref().unwrap().hwnd;let list=unsafe{GetDlgItem(Some(bin),2)}.unwrap();
    unsafe{SendMessageW(list,WM_LBUTTONDOWN,Some(WPARAM(1)),Some(LPARAM(10|(10<<16))));}
    let mut origin=windows::Win32::Foundation::POINT::default();unsafe{let _=GetCursorPos(&mut origin);let _=SetCursorPos(origin.x+40,origin.y+40);SendMessageW(list,WM_MOUSEMOVE,Some(WPARAM(1)),Some(LPARAM(50|(50<<16))));}
    pump();project_bin::review_drag_card(s.project_bin.as_ref().unwrap(),out);
    unsafe{SendMessageW(list,WM_CANCELMODE,None,None);let _=windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();let _=SetCursorPos(origin.x,origin.y);}
    s.select_preview_source(false).unwrap();
    std::fs::write(out.join("polish-review.json"),serde_json::to_vec_pretty(&serde_json::json!({"passed":true,"sourceSwitches":costs.len(),"switchWith350msPumpUs":costs,"gpuReadbacksDuringSwitch":s.session.compositor_cache_stats().readbacks-readbacks,"singleGpuWindow":true,"liveAudio":true,"perCueStylePersistence":true,"atomicOverlapRejection":true})).unwrap()).unwrap();
}
