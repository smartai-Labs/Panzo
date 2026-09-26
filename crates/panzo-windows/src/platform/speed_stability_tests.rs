use super::*;
use crate::platform::editor::ProjectEditorSession;
use panzo_core::CameraEditKind;

fn finish_exact(session: &mut ProjectPreviewSession) -> PreparedPreviewFrame {
    let frame = if let Some(frame) = session.begin_interactive_prepare().unwrap() {
        frame
    } else {
        loop {
            if let Some(frame) = session.poll_interactive_prepare(1).unwrap() {
                break frame;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    assert_eq!(frame.snapshot.project_tick, session.snapshot().project_tick);
    assert_eq!(
        frame.decoded.pts,
        frame.evaluation.source_frame.presentation_tick
    );
    let expected = FrameEvaluator::new(&session.source_timeline, &session.camera, &session.cursor)
        .unwrap()
        .evaluate_edit(
            frame.snapshot.project_tick,
            session.output,
            &session.settings,
        )
        .unwrap();
    assert_eq!(frame.evaluation, expected);
    session.compose_prepared(&frame).unwrap();
    frame
}

#[test]
#[ignore = "requires disposable PANZO_STABILITY_PROJECT, GPU and PANZO_UI_REVIEW_DIR; saves edits"]
#[allow(clippy::too_many_lines)] // Stress the real worker and persisted edit graph as a continuous workflow.
fn mixed_speed_seek_stability_ui_review() {
    let path = PathBuf::from(std::env::var_os("PANZO_STABILITY_PROJECT").unwrap());
    let output = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
    let mut editor = ProjectEditorSession::open(&path).unwrap();
    let original = editor.video_edit();
    assert_eq!(
        original.clips.len(),
        1,
        "use an unedited disposable project copy"
    );
    let source_duration = editor.duration().0;
    let first = original.clips[0].id.clone();
    let middle = editor.split_video(TimeTick(source_duration / 5)).unwrap();
    let tail = editor.split_video(TimeTick(source_duration / 2)).unwrap();
    editor.set_camera_enabled(true);
    let mut session =
        ProjectPreviewSession::open(&path, OutputDescriptor::bgra8(640, 360).unwrap()).unwrap();
    session
        .apply_edit_state(editor.camera().clone(), editor.settings().clone())
        .unwrap();
    let source_anchor = TimeTick(source_duration * 3 / 4);
    session.seek(source_anchor).unwrap();
    let retained = finish_exact(&mut session);
    let retained_pixels = session.compose_prepared(&retained).unwrap().composited;
    let mut rate_changes = 0;
    // Changing a preceding clip also moves the playhead in project time. Every
    // update invalidates an in-flight request; the source image must remain anchored.
    for id in [&first, &middle, &tail] {
        editor.begin_edit_group();
        for speed in [25, 50, 75, 100, 125, 150, 200, 300, 400]
            .into_iter()
            .cycle()
            .take(36)
        {
            editor.set_video_speed(id, speed).unwrap();
            session
                .apply_edit_state(editor.camera().clone(), editor.settings().clone())
                .unwrap();
            let actual = editor
                .video_edit()
                .source_time(session.snapshot().project_tick)
                .unwrap();
            assert!(
                (actual.0 - source_anchor.0).abs() <= 108 * 4,
                "source anchor drifted: {actual:?}"
            );
            session.begin_interactive_prepare().unwrap();
            rate_changes += 1;
        }
        editor.finish_edit_group();
        finish_exact(&mut session);
        assert!(editor.undo());
        session
            .apply_edit_state(editor.camera().clone(), editor.settings().clone())
            .unwrap();
        finish_exact(&mut session);
        assert!(editor.redo());
        session
            .apply_edit_state(editor.camera().clone(), editor.settings().clone())
            .unwrap();
    }
    editor.set_video_speed(&first, 200).unwrap();
    editor.set_video_speed(&middle, 25).unwrap();
    editor.set_video_speed(&tail, 125).unwrap();
    session
        .apply_edit_state(editor.camera().clone(), editor.settings().clone())
        .unwrap();
    let video = editor.video_edit();
    let mut seeks = Vec::new();
    let mut seed = 71_u64;
    // A burst of stale requests followed by a completed exact request, at widely
    // separated points including the final frame. Only the latest may be displayed.
    for index in 0..64 {
        for _ in 0..3 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let target = TimeTick((seed % video.duration().0.cast_unsigned()).cast_signed());
            session.seek(target).unwrap();
            session.begin_interactive_prepare().unwrap();
        }
        let target = if index % 8 == 0 {
            video.duration()
        } else {
            session.snapshot().project_tick
        };
        session.seek(target).unwrap();
        let started = Instant::now();
        let frame = finish_exact(&mut session);
        seeks.push(serde_json::json!({"projectTick":target.0,"sourceTick":frame.evaluation.source_tick.0,"milliseconds":started.elapsed().as_millis()}));
    }
    // Advance the actual playback controller over each speed boundary and EOS.
    for boundary in video.spans().map(|span| span.project_out) {
        session.seek(TimeTick(boundary.0 - 500_000)).unwrap();
        session.play();
        for _ in 0..5 {
            session.advance(TimeTick(200_000)).unwrap();
            finish_exact(&mut session);
        }
        session.pause();
    }
    assert_eq!(
        session.compose_prepared(&retained).unwrap().composited,
        retained_pixels
    );
    // Persist and rebuild both editor and decoder from disk, including the tail.
    editor.save(CameraEditKind::Update, None).unwrap();
    let reopened = ProjectEditorSession::open(&path).unwrap();
    assert_eq!(reopened.video_edit(), video);
    assert_eq!(reopened.camera(), editor.camera());
    assert!(!reopened.is_dirty());
    drop(session);
    let mut reopened_preview =
        ProjectPreviewSession::open(&path, OutputDescriptor::bgra8(640, 360).unwrap()).unwrap();
    for tick in [
        TimeTick::ZERO,
        TimeTick(video.duration().0 / 2),
        TimeTick(video.duration().0 - 1),
        video.duration(),
    ] {
        reopened_preview.seek(tick).unwrap();
        finish_exact(&mut reopened_preview);
    }
    fs::write(
        output.join("speed-stability.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "sourceDurationTick":source_duration,"projectDurationTick":video.duration().0,
            "continuousRateChanges":rate_changes,"supersededSeekRequests":192,
            "completedRandomSeeks":seeks,"playbackBoundaryFrames":15,
            "retainedFrameUnchanged":true,"saveReopen":true,"mediaErrors":0
        }))
        .unwrap(),
    )
    .unwrap();
}
