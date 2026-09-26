//! End-to-end RE-MVP media checks. Always make an isolated copy before editing.
use crate::platform::editor::ProjectEditorSession;
use crate::platform::export::{
    ExportError, ExportPreset, ExportRequest, ExportSnapshot, ProjectExporter,
};
use crate::platform::media_decoder::{DecoderProbe, MfBgraDecoder};
use crate::platform::preview::ProjectPreviewSession;
use panzo_core::{CameraEditKind, ProjectLayout, TimeMapping, TimeTick};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct EditingMediaReport {
    pub copy: PathBuf,
    pub source_preserved: bool,
    pub split_identity: bool,
    pub removed_range_mapping: bool,
    pub trimmed_ranges: bool,
    pub save_reopen: bool,
    pub snapshot_immutable: bool,
    pub existing_output_preserved: bool,
    pub cancellation_cleans_partial: bool,
    pub exports: Vec<ExportCheck>,
    pub elapsed_ms: u128,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportCheck {
    path: PathBuf,
    width: u32,
    height: u32,
    frames: u32,
    duration_tick: i64,
    expected_duration_tick: i64,
    duration_within_one_frame: bool,
    complete_decode: bool,
    increasing_pts: bool,
    keyframes: Vec<KeyframeCheck>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyframeCheck {
    output_index: u32,
    project_tick: i64,
    source_tick: i64,
    uncompressed_hashes_match: bool,
    decoded_mean_absolute_error: f64,
}

#[allow(clippy::too_many_lines)] // One ordered acceptance scenario with independent assertions.
pub fn exercise(source: &Path, output_root: &Path) -> Result<EditingMediaReport, String> {
    let started = Instant::now();
    let layout = ProjectLayout::open(source).map_err(|e| e.to_string())?;
    let manifest = layout.load_manifest().map_err(|e| e.to_string())?;
    let hashes = immutable_hashes(layout.root(), &manifest)?;
    let copy = output_root.join(format!("re-mvp-edit-{}.panzo", uuid::Uuid::new_v4()));
    fs::create_dir_all(&copy).map_err(|e| e.to_string())?;
    for relative in [
        "project.json",
        &manifest.media.screen,
        &manifest.tracks.camera,
        &manifest.tracks.cursor,
        &manifest.tracks.clicks,
    ] {
        copy_file(layout.root(), &copy, relative)?;
    }
    for relative in [
        "edit/workbench.json",
        "tracks/camera.auto.json",
        "edit/history.jsonl",
    ] {
        if source.join(relative).exists() {
            copy_file(source, &copy, relative)?;
        }
    }
    // Preserve imported image data without touching the external image file.
    let mut editor = ProjectEditorSession::open(&copy).map_err(|e| e.to_string())?;
    if let Some(image) = &editor.settings().background.image {
        copy_file(source, &copy, image)?;
    }
    let native =
        ProjectPreviewSession::native_output_descriptor(&copy).map_err(|e| e.to_string())?;
    let mut preview = ProjectPreviewSession::open(&copy, native).map_err(|e| e.to_string())?;
    let before = preview
        .render_current()
        .map_err(|e| e.to_string())?
        .composited
        .sha256_hex();
    let duration = editor.project_duration().as_i64();
    if duration < 30_000_000 {
        return Err("RE fixture must be at least three seconds".into());
    }
    let a = TimeTick(duration / 3);
    let b = TimeTick(duration * 2 / 3);
    let middle = editor.split_video(a).map_err(|e| e.to_string())?;
    editor.split_video(b).map_err(|e| e.to_string())?;
    preview
        .apply_edit_state(editor.camera().clone(), editor.settings().clone())
        .map_err(|e| e.to_string())?;
    let split_identity = before
        == preview
            .render_current()
            .map_err(|e| e.to_string())?
            .composited
            .sha256_hex();
    editor.delete_video(&middle).map_err(|e| e.to_string())?;
    let video = editor.video_edit();
    let removed_range_mapping =
        video.source_time(a).map_err(|e| e.to_string())? == b && video.project_time(a).is_err();
    // Exercise both ends on real media, not only the middle-delete mapping.
    // These deliberately non-frame-aligned edges must still export the correct
    // first/last selected source frames without extending the edited duration.
    let trim = TimeTick(duration / 10);
    editor
        .trim_video(&video.clips[0].id, trim, a)
        .map_err(|e| e.to_string())?;
    editor
        .trim_video(&video.clips[1].id, b, TimeTick(duration - trim.0))
        .map_err(|e| e.to_string())?;
    let video = editor.video_edit();
    let expected_duration = duration - (b.0 - a.0) - 2 * trim.0;
    let cut = TimeTick(a.0 - trim.0);
    let trimmed_ranges = video.duration().0 == expected_duration
        && video
            .source_time(TimeTick::ZERO)
            .map_err(|e| e.to_string())?
            == trim
        && video
            .source_time(TimeTick(expected_duration - 1))
            .map_err(|e| e.to_string())?
            == TimeTick(duration - trim.0 - 1)
        && video.project_time(TimeTick::ZERO).is_err()
        && video.project_time(TimeTick(duration - trim.0)).is_err();
    editor.set_camera_enabled(false);
    editor
        .set_cursor_style(false, 1.25)
        .map_err(|e| e.to_string())?;
    editor.set_canvas_inset(0.05).map_err(|e| e.to_string())?;
    let snapshot = ExportSnapshot {
        camera: editor.camera().clone(),
        settings: editor.settings().clone(),
    };
    editor.set_canvas_inset(0.10).map_err(|e| e.to_string())?;
    let snapshot_immutable =
        snapshot.settings.canvas.inset == 0.05 && editor.settings().canvas.inset == 0.10;
    assert_check(editor.undo(), "undo after snapshot")?;
    editor
        .save(CameraEditKind::Update, None)
        .map_err(|e| e.to_string())?;
    let reopened = ProjectEditorSession::open(&copy).map_err(|e| e.to_string())?;
    let save_reopen = reopened.video_edit() == video
        && !reopened.settings().camera_enabled
        && !reopened.settings().cursor.visible;
    let mut exports = Vec::new();
    for (name, preset, size) in [
        (
            "native",
            ExportPreset::Native,
            (native.width, native.height),
        ),
        ("1080p", ExportPreset::Hd1080, (1920, 1080)),
    ] {
        let request = ExportRequest {
            project_root: copy.clone(),
            output_path: copy.join(format!("{name}.mp4")),
            maximum_duration: None,
        };
        let report = ProjectExporter::export_snapshot(
            &request,
            &snapshot,
            preset,
            &AtomicBool::new(false),
            |_, _| {},
        )
        .map_err(|e| e.to_string())?;
        let decoded =
            DecoderProbe::inspect(&request.output_path, u32::MAX).map_err(|e| e.to_string())?;
        let index = MfBgraDecoder::native_index(&request.output_path, || false)
            .map_err(|e| e.to_string())?;
        let increasing = index.windows(2).all(|w| w[0].pts < w[1].pts);
        assert_check((report.width, report.height) == size, "export dimensions")?;
        assert_check(
            index.len() == report.frames_written as usize,
            "full output decode length",
        )?;
        let complete_decode = decoded.decoded_frames == report.frames_written;
        let expected_frames = (expected_duration * 60 + 9_999_999) / 10_000_000;
        let duration_within_one_frame = (report.duration_tick.0 - expected_duration).abs()
            <= 166_667
            && i64::from(report.frames_written) == expected_frames;
        let keyframes = check_export_keyframes(
            &copy,
            &request.output_path,
            size,
            report.frames_written,
            cut,
        )?;
        exports.push(ExportCheck {
            path: request.output_path,
            width: report.width,
            height: report.height,
            frames: report.frames_written,
            duration_tick: report.duration_tick.as_i64(),
            expected_duration_tick: expected_duration,
            duration_within_one_frame,
            complete_decode,
            increasing_pts: increasing,
            keyframes,
        });
    }
    let existing = copy.join("native.mp4");
    let before_output = hash(&existing)?;
    let existing_request = ExportRequest {
        project_root: copy.clone(),
        output_path: existing.clone(),
        maximum_duration: None,
    };
    let rejected = ProjectExporter::export_snapshot(
        &existing_request,
        &snapshot,
        ExportPreset::Native,
        &AtomicBool::new(false),
        |_, _| {},
    );
    let existing_output_preserved =
        matches!(rejected, Err(ExportError::OutputExists(_))) && before_output == hash(&existing)?;
    let cancel = AtomicBool::new(false);
    let cancel_request = ExportRequest {
        project_root: copy.clone(),
        output_path: copy.join("cancelled.mp4"),
        maximum_duration: None,
    };
    let cancelled = ProjectExporter::export_snapshot(
        &cancel_request,
        &snapshot,
        ExportPreset::Native,
        &cancel,
        |done, _| {
            if done >= 5 {
                cancel.store(true, Ordering::Release);
            }
        },
    );
    let cancellation_cleans_partial = matches!(cancelled, Err(ExportError::Cancelled))
        && !cancel_request.output_path.exists()
        && !fs::read_dir(&copy)
            .map_err(|e| e.to_string())?
            .flatten()
            .any(|e| e.file_name().to_string_lossy().ends_with("panzo-part.mp4"));
    let source_preserved = hashes == immutable_hashes(source, &manifest)?
        && hashes == immutable_hashes(&copy, &manifest)?;
    let report = EditingMediaReport {
        copy,
        source_preserved,
        split_identity,
        removed_range_mapping,
        trimmed_ranges,
        save_reopen,
        snapshot_immutable,
        existing_output_preserved,
        cancellation_cleans_partial,
        exports,
        elapsed_ms: started.elapsed().as_millis(),
    };
    fs::write(
        report.copy.join("acceptance.json"),
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    assert_check(
        report.source_preserved
            && report.split_identity
            && report.removed_range_mapping
            && report.trimmed_ranges
            && report.save_reopen
            && report.snapshot_immutable
            && report.existing_output_preserved
            && report.cancellation_cleans_partial
            && report
                .exports
                .iter()
                .all(|e| e.complete_decode && e.increasing_pts && e.duration_within_one_frame),
        "RE media acceptance",
    )?;
    Ok(report)
}

fn check_export_keyframes(
    project: &Path,
    output: &Path,
    size: (u32, u32),
    frames: u32,
    cut: TimeTick,
) -> Result<Vec<KeyframeCheck>, String> {
    let cut_frame = u32::try_from((i128::from(cut.0) * 60 + 9_999_999) / 10_000_000)
        .map_err(|e| e.to_string())?;
    let mut targets = vec![
        0,
        cut_frame.saturating_sub(1),
        cut_frame.min(frames - 1),
        frames - 1,
    ];
    targets.sort_unstable();
    targets.dedup();
    let descriptor =
        panzo_core::OutputDescriptor::bgra8(size.0, size.1).map_err(|e| e.to_string())?;
    let mut preview =
        ProjectPreviewSession::open(project, descriptor).map_err(|e| e.to_string())?;
    let mut decoder = MfBgraDecoder::open(output).map_err(|e| e.to_string())?;
    let mut checks = Vec::new();
    for index in 0..frames {
        let encoded_frame = decoder
            .read_next()
            .map_err(|e| e.to_string())?
            .ok_or("encoded output ended early")?;
        if !targets.contains(&index) {
            continue;
        }
        let tick = TimeTick(i64::from(index) * 10_000_000 / 60);
        preview.seek(tick).map_err(|e| e.to_string())?;
        let expected = preview.render_current().map_err(|e| e.to_string())?;
        assert_check(
            expected.composited.pixels.len() == encoded_frame.pixels.len(),
            "keyframe geometry",
        )?;
        let difference: u64 = encoded_frame
            .pixels
            .chunks_exact(4)
            .zip(expected.composited.pixels.chunks_exact(4))
            .map(|(actual, expected)| {
                (0..3)
                    .map(|c| u64::from(actual[c].abs_diff(expected[c])))
                    .sum::<u64>()
            })
            .sum();
        let mae = difference as f64 / (f64::from(size.0) * f64::from(size.1) * 3.0);
        let hashes =
            crate::platform::compositor::RenderHashProbe::project_frame(project, tick, descriptor)
                .map_err(|e| e.to_string())?;
        assert_check(
            hashes.hashes_match,
            "independent preview/export uncompressed hash",
        )?;
        // H.264 / NV12 are lossy: do not compare the decoded output with an exact hash.
        assert_check(
            mae <= 18.0,
            "decoded export content differs from source-mapped preview",
        )?;
        checks.push(KeyframeCheck {
            output_index: index,
            project_tick: tick.0,
            source_tick: expected.evaluation.source_tick.0,
            uncompressed_hashes_match: hashes.hashes_match,
            decoded_mean_absolute_error: mae,
        });
    }
    Ok(checks)
}
fn assert_check(check: bool, label: &str) -> Result<(), String> {
    if check {
        Ok(())
    } else {
        Err(format!("failed: {label}"))
    }
}
fn copy_file(source: &Path, target: &Path, relative: &str) -> Result<(), String> {
    // ProjectLayout/settings validation owns relative-path validation; never traverse outside copy.
    if Path::new(relative)
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("invalid project-relative path".into());
    }
    let destination = target.join(relative);
    fs::create_dir_all(destination.parent().ok_or("invalid destination")?)
        .map_err(|e| e.to_string())?;
    fs::copy(source.join(relative), destination).map_err(|e| e.to_string())?;
    Ok(())
}
fn hash(path: &Path) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(fs::read(path).map_err(|e| e.to_string())?)
    ))
}
fn immutable_hashes(
    root: &Path,
    manifest: &panzo_core::ProjectManifest,
) -> Result<Vec<String>, String> {
    [
        &manifest.media.screen,
        &manifest.tracks.cursor,
        &manifest.tracks.clicks,
    ]
    .iter()
    .map(|p| hash(&root.join(p)))
    .collect()
}
