//! Opt-in acceptance against the synthetic project produced by test-media-release.py.
use super::{
    audio_decode::AudioDecoder,
    editor::ProjectEditorSession,
    export::{ExportOptions, ExportPreset, ExportRequest, ExportSnapshot, ProjectExporter},
    media_decoder::MfBgraDecoder,
};
use std::{path::PathBuf, sync::atomic::AtomicBool};
#[test]
#[ignore = "requires PANZO_MEDIA_PROJECT synthetic native smoke project and PANZO_MEDIA_OUTPUT"]
fn media_audio_subtitle_export_roundtrip() {
    let project =
        PathBuf::from(std::env::var_os("PANZO_MEDIA_PROJECT").expect("synthetic project"));
    let out = PathBuf::from(std::env::var_os("PANZO_MEDIA_OUTPUT").expect("acceptance output"));
    std::fs::create_dir_all(&out).unwrap();
    let editor = ProjectEditorSession::open(&project).unwrap();
    assert_eq!(editor.settings().audio_clips.len(), 1);
    assert_eq!(editor.settings().subtitles.len(), 2);
    let mut views = Vec::new();
    for enabled in [true, false] {
        let mut settings = editor.settings().clone();
        settings.subtitle_style.enabled = enabled;
        // The original fixture is silent; post-production audio remains usable with no plugin process.
        settings.audio.system_gain = 0.0;
        settings.audio.microphone_gain = 0.0;
        settings.audio.imported_gain = 0.0;
        let output = out.join(if enabled {
            "subtitled.mp4"
        } else {
            "without-subtitles.mp4"
        });
        let report = ProjectExporter::export_snapshot_with_options(
            &ExportRequest {
                project_root: project.clone(),
                output_path: output.clone(),
                maximum_duration: None,
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
            &AtomicBool::new(false),
            |_, _| {},
        )
        .unwrap();
        assert_eq!(report.frames_written, 180);
        assert_eq!(report.duration_tick.0, 60_000_000);
        let mut decoder = MfBgraDecoder::open(&output).unwrap();
        let mut frames = 0;
        let mut selected = Vec::new();
        while let Some(frame) = decoder.read_next().unwrap() {
            if frames == 30 || frames == 150 {
                selected.push(frame);
            }
            frames += 1;
        }
        assert_eq!(frames, 180);
        assert_eq!(selected.len(), 2);
        views.push(selected);
    }
    let mut difference = Vec::new();
    for index in 0..2 {
        let a = &views[0][index];
        let b = &views[1][index];
        assert_eq!((a.width, a.height), (b.width, b.height));
        let offset = (a.width * a.height * 3 / 4 * 4) as usize;
        let changed = a.pixels[offset..]
            .iter()
            .zip(&b.pixels[offset..])
            .filter(|(x, y)| x.abs_diff(**y) > 35)
            .count();
        difference.push(changed);
        let rgba = a
            .pixels
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect::<Vec<_>>();
        image::save_buffer(
            out.join(format!("export-frame-{index}.png")),
            &rgba,
            a.width,
            a.height,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
    assert!(
        difference[0] > 250,
        "caption absent from encoded image: {difference:?}"
    );
    assert!(
        difference[1] < difference[0] / 4,
        "caption remains after its end: {difference:?}"
    );
    let mut audio = AudioDecoder::open(&out.join("subtitled.mp4"))
        .unwrap()
        .unwrap();
    let mut energies = [0f64; 3];
    let mut counts = [0usize; 3];
    let mut end = 0;
    while let Some(p) = audio.read().unwrap() {
        end = p.tick.0 + (p.samples.len() / p.channels) as i64 * 10_000_000 / i64::from(p.rate);
        for (i, samples) in p.samples.chunks_exact(p.channels).enumerate() {
            let seconds = p.tick.0 as f64 / 1e7 + i as f64 / f64::from(p.rate);
            let bin = if (0.05..0.3).contains(&seconds) {
                Some(0)
            } else if (1.0..2.5).contains(&seconds) {
                Some(1)
            } else if (4.5..5.5).contains(&seconds) {
                Some(2)
            } else {
                None
            };
            if let Some(bin) = bin {
                for v in samples {
                    energies[bin] += f64::from(*v).powi(2);
                    counts[bin] += 1;
                }
            }
        }
    }
    let rms = std::array::from_fn::<_, 3, _>(|i| (energies[i] / counts[i].max(1) as f64).sqrt());
    assert!(
        rms[0] < 50.0 && rms[2] < 50.0,
        "audio outside clip: {rms:?}"
    );
    assert!(
        (2500.0..4500.0).contains(&rms[1]),
        "wrong gain or missing imported audio: {rms:?}"
    );
    assert!((end - 60_000_000).abs() < 1_000_000);
    std::fs::write(out.join("export-proof.json"),serde_json::to_vec_pretty(&serde_json::json!({"passed":true,"videoFrames":180,"videoSeconds":6,"audioEndTick":end,"audioRmsBeforeInsideAfter":rms,"captionPixelsChangedActiveInactive":difference,"pluginRequiredForExport":false})).unwrap()).unwrap();
}
