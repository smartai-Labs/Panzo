//! Explicit native test with an isolated fixture/profile.
use super::*;
use crate::platform::{preview_quality::PreviewQuality as Q,export_window};
pub(super) fn run(h:HWND,root:HWND,out:&Path) {
    let s=unsafe{state_mut(h)}.unwrap();
    let mut settings=panzo_core::WorkbenchSettings::default();
    settings.camera_enabled=false;settings.canvas.inset=0.;settings.cursor.visible=false;
    let mut video=panzo_core::VideoEdit::full(s.editor.duration()).unwrap();let id=video.clips[0].id.clone();
    video.trim(&id,TimeTick::ZERO,TimeTick(30_000_000)).unwrap();settings.video=Some(video);
    s.editor.replace_media_settings(settings).unwrap();s.apply_editor_state().unwrap();pump();
    let original=s.editor.settings().clone();let surface=s.preview_surface.as_ref().unwrap().hwnd;
    let base=s.session.compositor_cache_stats().readbacks;
    let mut results=Vec::new();
    for q in [Q::Quarter,Q::Half,Q::Original,Q::Quarter,Q::Original] {
        project_bin::review_select_quality(s.project_bin.as_ref().unwrap(),q);
        let wanted=match q{Q::Quarter=>(640,360),Q::Half=>(1280,720),Q::Original=>(2560,1440)};
        for _ in 0..12 {pump();if s.session.review_source_size()==Some(wanted){break;}}
        assert_eq!(s.session.review_source_size(),Some(wanted));
        assert_eq!(s.preview_surface.as_ref().unwrap().hwnd,surface);
        assert_eq!(s.editor.settings(),&original);
        results.push(serde_json::json!({"path":"project-source","quality":q,"decoded":wanted}));
    }
    // Keep playback intent and scrub quality when the decoder is reconfigured.
    let errors=s.telemetry.media_errors.load(Ordering::Relaxed);
    s.toggle_playback_intent();pump();
    assert_eq!(s.session.snapshot().playback,PlaybackState::Playing);
    for q in [Q::Quarter,Q::Original] {
        s.change_preview_quality(q);
        for _ in 0..8 {pump();if s.session.snapshot().playback==PlaybackState::Playing{break;}}
        assert_eq!(s.session.snapshot().playback,PlaybackState::Playing);
    }
    s.toggle_playback_intent();pump();s.change_preview_quality(Q::Half);pump();
    let c=s.control_layout(client_rect(h));let x=timeline_x(c.ruler,TimeTick(10_000_000),s.timeline);
    s.begin_timeline_scrub(h,c.ruler,x);pump();
    s.on_mouse_up(h,x,(c.ruler.top+c.ruler.bottom)/2);
    for _ in 0..12{pump();if s.session.review_source_size()==Some((1280,720))&&!s.session.interactive_render_pending(){break;}}
    assert_eq!(s.session.review_source_size(),Some((1280,720)));
    assert!((s.session.snapshot().project_tick.0-10_000_000).abs()<500_000);
    assert_eq!(s.telemetry.media_errors.load(Ordering::Relaxed),errors);
    s.change_preview_quality(Q::Original);s.session.seek(TimeTick::ZERO).unwrap();s.render().unwrap();pump();
    // The placed source and asset viewer both use Visuals, rather than the source worker.
    let mut next=s.editor.settings().clone();ensure_source_asset(s.editor.project_root(),&mut next).unwrap();
    next.source_video_enabled=false;next.insert_visual_asset("__source__","video",TimeTick::ZERO).unwrap();
    next.visual_clips[0].source_out=TimeTick(30_000_000);let clip=next.visual_clips[0].id.clone();
    s.editor.replace_media_settings(next).unwrap();s.apply_editor_state().unwrap();pump();
    for source in [false,true] {
        if source {project_bin::review_preview_asset(s.project_bin.as_ref().unwrap(),"__source__");pump();
            // Preview an ordinary imported video ID, not the special original-source shortcut.
            s.select_preview_source(false).unwrap();let mut n=s.editor.settings().clone();let mut a=n.visual_assets[0].clone();a.id="ordinary-video".into();n.visual_assets.push(a);
            s.editor.replace_media_settings(n).unwrap();s.apply_editor_state().unwrap();s.refresh(h,true);pump();
            project_bin::review_preview_asset(s.project_bin.as_ref().unwrap(),"ordinary-video");pump();
        }
        let slot=if source{"__preview_asset__"}else{clip.as_str()};
        for q in [Q::Quarter,Q::Half,Q::Original] {
            project_bin::review_select_quality(s.project_bin.as_ref().unwrap(),q);
            let wanted=match q{Q::Quarter=>(640,360),Q::Half=>(1280,720),Q::Original=>(2560,1440)};
            for _ in 0..18 {pump();if s.session.review_visual_size(slot)==Some(wanted){break;}}
            assert_eq!(s.session.review_visual_size(slot),Some(wanted));
            assert_eq!(s.preview_surface.as_ref().unwrap().hwnd,surface);
            results.push(serde_json::json!({"path":if source{"asset-preview"}else{"timeline-placed"},"quality":q,"decoded":wanted}));
        }
    }
    s.select_preview_source(false).unwrap();s.refresh(h,true);pump();
    let c=s.control_layout(client_rect(h));assert!(c.preview_quality.right<c.home.left);assert!(c.time.left>=c.last.right);
    snap(root,out,"quality-and-preview-tabs");
    assert_eq!(s.session.compositor_cache_stats().readbacks,base);
    export_window::review_layout_and_advanced(h,out);
    std::fs::write(out.join("quality-review.json"),serde_json::to_vec_pretty(&serde_json::json!({"passed":true,"sameGpuWindow":true,"gpuReadbacks":0,"playbackAndScrub":true,"frames":results})).unwrap()).unwrap();
}
