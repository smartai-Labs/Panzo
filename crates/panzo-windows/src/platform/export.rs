use crate::platform::compositor::{CompositorError, D3d11Compositor};
use crate::platform::d3d11::D3d11Device;
use crate::platform::editor::{
    EditorSessionError, load_effective_background, load_effective_edit_state,
};
use crate::platform::fmp4_writer::{Fmp4ProbeError, Nv12Fmp4Writer, Nv12VideoConfig};
use crate::platform::media_decoder::{DecodedBgraFrame, MediaDecoderError, MfBgraDecoder};
use crate::platform::video_processor::{Nv12VideoProcessor, VideoProcessorProbeError};
use panzo_core::{
    CameraTrack, CursorEvent, CursorTimeline, FrameEvaluator, OutputDescriptor, ProjectIoError,
    ProjectLayout, RenderEvaluationError, SourceFrameTimeline, TICKS_PER_SECOND, TimeTick,
    WorkbenchSettings,
};
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Instant;
use thiserror::Error;

const EXPORT_WIDTH: u32 = 1_920;
const EXPORT_HEIGHT: u32 = 1_080;
const EXPORT_FPS: i64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    pub project_root: PathBuf,
    pub output_path: PathBuf,
    /// `None` exports the complete project. A limit is used by the short M3 acceptance probe.
    pub maximum_duration: Option<TimeTick>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    pub project_root: PathBuf,
    pub output_path: PathBuf,
    pub frames_written: u32,
    pub width: u32,
    pub height: u32,
    pub duration_tick: TimeTick,
    pub output_bytes: u64,
    pub fragment_count: usize,
    pub trailing_bytes: u64,
    pub encoder_name: String,
    pub hardware_encoder: bool,
    pub elapsed_ms: u128,
}

impl fmt::Display for ExportReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            formatter,
            "Panzo MP4 export — {}×{} / 60 fps",
            self.width, self.height
        )?;
        writeln!(formatter, "  Project: {}", self.project_root.display())?;
        writeln!(formatter, "  Output: {}", self.output_path.display())?;
        writeln!(formatter, "  Frames: {}", self.frames_written)?;
        writeln!(
            formatter,
            "  Duration tick: {}",
            self.duration_tick.as_i64()
        )?;
        writeln!(formatter, "  Output bytes: {}", self.output_bytes)?;
        writeln!(formatter, "  fMP4 fragments: {}", self.fragment_count)?;
        writeln!(formatter, "  Trailing bytes: {}", self.trailing_bytes)?;
        writeln!(
            formatter,
            "  Encoder: {} ({})",
            self.encoder_name,
            if self.hardware_encoder {
                "hardware"
            } else {
                "not identified as hardware"
            }
        )?;
        write!(formatter, "  Elapsed: {} ms", self.elapsed_ms)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ExportPreset {
    #[default]
    Native,
    Hd1080,
}

impl ExportPreset {
    pub fn dimensions(self, source: (u32, u32), settings: &WorkbenchSettings) -> (u32, u32) {
        if let Some(canvas) = settings.canvas.size {
            let size = if self == Self::Hd1080 {
                canvas.hd()
            } else {
                canvas
            };
            (size.width, size.height)
        } else if self == Self::Hd1080 {
            (EXPORT_WIDTH, EXPORT_HEIGHT)
        } else {
            source
        }
    }
}

/// Owned immutable editor revision; later UI edits never change an in-flight export.
#[derive(Debug, Clone)]
pub struct ExportSnapshot {
    pub camera: CameraTrack,
    pub settings: WorkbenchSettings,
}

pub struct ProjectExporter;

impl ProjectExporter {
    /// Compatibility entry for the historical 1080p acceptance probes.
    pub fn export(request: &ExportRequest) -> Result<ExportReport, ExportError> {
        let (camera, settings) = load_effective_edit_state(&request.project_root)?;
        Self::export_snapshot(
            request,
            &ExportSnapshot { camera, settings },
            ExportPreset::Hd1080,
            &std::sync::atomic::AtomicBool::new(false),
            |_, _| {},
        )
    }

    pub fn export_snapshot(
        request: &ExportRequest,
        snapshot: &ExportSnapshot,
        preset: ExportPreset,
        cancel: &std::sync::atomic::AtomicBool,
        progress: impl Fn(u32, u32),
    ) -> Result<ExportReport, ExportError> {
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err(ExportError::Cancelled);
        }
        // Refuse replacement: never remove a previously successful video.
        if request.output_path.exists() {
            return Err(ExportError::OutputExists(request.output_path.clone()));
        }
        let temporary_output = temporary_output_path(&request.output_path)?;
        let encoded_output = temporary_output.with_extension("encoding.mp4");
        let result = export_to_temporary(
            request,
            &encoded_output,
            snapshot,
            preset,
            cancel,
            &progress,
        )
        .and_then(|mut report| {
            let started = Instant::now();
            crate::platform::fmp4_writer::normalize_export_timing(
                &encoded_output,
                &temporary_output,
                cancel,
            )?;
            report.output_bytes = fs::metadata(&temporary_output)
                .map_err(Fmp4ProbeError::from)?
                .len();
            report.elapsed_ms += started.elapsed().as_millis();
            Ok(report)
        });
        let _ = fs::remove_file(&encoded_output);
        let report = match result {
            Ok(report) => report,
            Err(error) => {
                let _ = fs::remove_file(&temporary_output);
                return Err(if cancel.load(std::sync::atomic::Ordering::Acquire) {
                    ExportError::Cancelled
                } else {
                    error
                });
            }
        };
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            let _ = fs::remove_file(&temporary_output);
            return Err(ExportError::Cancelled);
        }
        // On Windows rename fails if another operation has created the destination.
        if let Err(source) = fs::rename(&temporary_output, &request.output_path) {
            let _ = fs::remove_file(&temporary_output);
            return Err(ExportError::CommitOutput {
                from: temporary_output,
                to: request.output_path.clone(),
                source,
            });
        }
        Ok(ExportReport {
            output_path: request.output_path.clone(),
            ..report
        })
    }
}

fn export_to_temporary(
    request: &ExportRequest,
    temporary_output: &Path,
    snapshot: &ExportSnapshot,
    preset: ExportPreset,
    cancel: &std::sync::atomic::AtomicBool,
    progress: &impl Fn(u32, u32),
) -> Result<ExportReport, ExportError> {
    let started = Instant::now();
    let layout = ProjectLayout::open(&request.project_root)?;
    let manifest = layout.load_manifest()?;
    let media_path = layout.root().join(&manifest.media.screen);
    reject_source_overwrite(&media_path, &request.output_path)?;
    let settings = &snapshot.settings;
    settings.validate().map_err(EditorSessionError::from)?;
    let video = settings
        .video_edit(TimeTick(manifest.media.duration_tick))
        .map_err(EditorSessionError::from)?;
    let project_duration = video.duration();
    let duration = request
        .maximum_duration
        .map_or(project_duration, |limit| limit.min(project_duration));
    if duration <= TimeTick::ZERO {
        return Err(ExportError::InvalidDuration(duration));
    }

    let camera = &snapshot.camera;
    let background = load_effective_background(layout.root(), settings)?;
    let cursor_path = layout.root().join(&manifest.tracks.cursor);
    let cursor = read_cursor_events(&cursor_path)?;
    let timeline =
        read_source_timeline(&media_path, TimeTick(manifest.media.duration_tick), cancel)?;
    let evaluator = FrameEvaluator::new(&timeline, camera, &cursor)?;
    let mut decoder = MfBgraDecoder::open(&media_path)?;
    let (width, height) = preset.dimensions((decoder.width(), decoder.height()), settings);
    let output_descriptor = OutputDescriptor::bgra8(width, height)?;
    let frame_count = export_frame_count(duration)?;

    let compositor = D3d11Compositor::create()?;
    let conversion_device = D3d11Device::create_hardware().map_err(ExportError::D3dDevice)?;
    let converter =
        Nv12VideoProcessor::create_with_output(&conversion_device, width, height, width, height)?;
    let mut writer = Nv12Fmp4Writer::create_with_config(
        temporary_output,
        Nv12VideoConfig::native_screen_60(width, height)?,
    )?;
    let mut decoded_index = 0_usize;
    let mut current_frame = decoder.read_next()?.ok_or(ExportError::NoSourceFrame)?;

    for output_index in 0..frame_count {
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err(ExportError::Cancelled);
        }
        let project_tick = export_frame_tick(output_index);
        let evaluation = evaluator.evaluate_edit(project_tick, output_descriptor, settings)?;
        while decoded_index < evaluation.source_frame.frame_index {
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                return Err(ExportError::Cancelled);
            }
            current_frame = decoder.read_next()?.ok_or(ExportError::SourceEndedEarly {
                required_index: evaluation.source_frame.frame_index,
                decoded_index,
            })?;
            decoded_index += 1;
        }
        assert_selected_frame(&current_frame, &evaluation)?;
        let bgra = compose_export_frame(
            &compositor,
            &current_frame,
            &evaluation,
            settings,
            background.as_ref(),
        )?;
        let nv12 = converter.convert_bgra_bytes(&bgra.pixels)?;
        let sample_start = export_frame_tick(output_index);
        let sample_end = export_frame_tick(output_index + 1).min(duration);
        writer.write_nv12(
            &nv12,
            sample_start.as_i64(),
            sample_end.as_i64() - sample_start.as_i64(),
        )?;
        progress(output_index + 1, frame_count);
    }

    let finalized = writer.finish()?;
    if finalized.inspection.trailing_bytes != 0 {
        return Err(ExportError::TrailingOutputBytes(
            finalized.inspection.trailing_bytes,
        ));
    }
    Ok(ExportReport {
        project_root: layout.root().into(),
        output_path: temporary_output.into(),
        frames_written: finalized.frames_written,
        width,
        height,
        duration_tick: TimeTick(finalized.duration_tick),
        output_bytes: finalized.output_bytes,
        fragment_count: finalized.inspection.fragment_count,
        trailing_bytes: finalized.inspection.trailing_bytes,
        encoder_name: finalized.encoder_name,
        hardware_encoder: finalized.hardware_encoder,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

pub(crate) fn compose_export_frame(
    compositor: &D3d11Compositor,
    frame: &DecodedBgraFrame,
    evaluation: &panzo_core::FrameEvaluation,
    settings: &panzo_core::WorkbenchSettings,
    background: Option<&crate::platform::background_image::DecodedBackgroundImage>,
) -> Result<crate::platform::compositor::CompositedFrame, ExportError> {
    assert_selected_frame(frame, evaluation)?;
    compositor
        .compose_with_background(frame, evaluation, settings, background)
        .map_err(ExportError::from)
}

fn assert_selected_frame(
    frame: &DecodedBgraFrame,
    evaluation: &panzo_core::FrameEvaluation,
) -> Result<(), ExportError> {
    if frame.pts != evaluation.source_frame.presentation_tick {
        return Err(ExportError::SelectionMismatch {
            evaluator_tick: evaluation.source_frame.presentation_tick,
            decoded_tick: frame.pts,
        });
    }
    Ok(())
}

fn read_source_timeline(
    media_path: &Path,
    maximum_tick: TimeTick,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<SourceFrameTimeline, ExportError> {
    let ticks = MfBgraDecoder::native_index(media_path, || {
        cancel.load(std::sync::atomic::Ordering::Acquire)
    })?
    .into_iter()
    .take_while(|frame| frame.pts <= maximum_tick)
    .map(|frame| frame.pts)
    .collect();
    SourceFrameTimeline::new(ticks).map_err(ExportError::Evaluation)
}

fn read_cursor_events(path: &Path) -> Result<CursorTimeline, ExportError> {
    let file = fs::File::open(path).map_err(|source| ExportError::Read {
        path: path.into(),
        source,
    })?;
    let mut events = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|source| ExportError::Read {
            path: path.into(),
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let event = serde_json::from_str::<CursorEvent>(&line).map_err(|source| {
            ExportError::InvalidCursorRecord {
                path: path.into(),
                line: index + 1,
                source,
            }
        })?;
        events.push(event);
    }
    CursorTimeline::new(events).map_err(ExportError::Evaluation)
}

fn export_frame_count(duration: TimeTick) -> Result<u32, ExportError> {
    let frames = (i128::from(duration.as_i64()) * i128::from(EXPORT_FPS)
        + i128::from(TICKS_PER_SECOND - 1))
        / i128::from(TICKS_PER_SECOND);
    u32::try_from(frames).map_err(|_| ExportError::DurationTooLong(duration))
}

fn export_frame_tick(frame_index: u32) -> TimeTick {
    TimeTick(i64::from(frame_index) * TICKS_PER_SECOND / EXPORT_FPS)
}

fn temporary_output_path(output: &Path) -> Result<PathBuf, ExportError> {
    if output.extension().and_then(|value| value.to_str()) != Some("mp4") {
        return Err(ExportError::InvalidOutputExtension(output.into()));
    }
    let file_name = output
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| ExportError::InvalidOutputPath(output.into()))?;
    Ok(output.with_file_name(format!(
        "{file_name}.{}.panzo-part.mp4",
        uuid::Uuid::new_v4()
    )))
}

fn reject_source_overwrite(source: &Path, output: &Path) -> Result<(), ExportError> {
    let canonical_source = fs::canonicalize(source).map_err(|source_error| ExportError::Read {
        path: source.into(),
        source: source_error,
    })?;
    let output_parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .or_else(|| Some(Path::new(".")))
        .ok_or_else(|| ExportError::InvalidOutputPath(output.into()))?;
    let canonical_parent =
        fs::canonicalize(output_parent).map_err(|source_error| ExportError::Read {
            path: output_parent.into(),
            source: source_error,
        })?;
    let resolved_output = canonical_parent.join(
        output
            .file_name()
            .ok_or_else(|| ExportError::InvalidOutputPath(output.into()))?,
    );
    if canonical_source == resolved_output {
        return Err(ExportError::SourceOverwrite(output.into()));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("导出已取消，原视频未改变")]
    Cancelled,
    #[error("输出文件已存在，请选择新的文件名：{0}")]
    OutputExists(PathBuf),
    #[error(transparent)]
    Project(#[from] ProjectIoError),
    #[error(transparent)]
    Editor(#[from] EditorSessionError),
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid cursor JSONL record {path}:{line}: {source}")]
    InvalidCursorRecord {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error(transparent)]
    Decoder(#[from] MediaDecoderError),
    #[error(transparent)]
    Evaluation(#[from] RenderEvaluationError),
    #[error(transparent)]
    Compositor(#[from] CompositorError),
    #[error("D3D11 export device creation failed: {0}")]
    D3dDevice(windows::core::Error),
    #[error(transparent)]
    VideoProcessor(#[from] VideoProcessorProbeError),
    #[error(transparent)]
    Writer(#[from] Fmp4ProbeError),
    #[error("export duration must be positive: {0:?}")]
    InvalidDuration(TimeTick),
    #[error("export duration exceeds supported frame count: {0:?}")]
    DurationTooLong(TimeTick),
    #[error("source video has no decoded frame")]
    NoSourceFrame,
    #[error(
        "source ended at decoded index {decoded_index}, before required index {required_index}"
    )]
    SourceEndedEarly {
        required_index: usize,
        decoded_index: usize,
    },
    #[error("evaluator selected {evaluator_tick:?}, decoder retained {decoded_tick:?}")]
    SelectionMismatch {
        evaluator_tick: TimeTick,
        decoded_tick: TimeTick,
    },
    #[error("export output contains {0} trailing bytes")]
    TrailingOutputBytes(u64),
    #[error("export output path must end in .mp4: {0}")]
    InvalidOutputExtension(PathBuf),
    #[error("invalid export output path: {0}")]
    InvalidOutputPath(PathBuf),
    #[error("export output cannot overwrite project source media: {0}")]
    SourceOverwrite(PathBuf),
    #[error("failed to remove output {path}: {source}")]
    RemoveOutput {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to commit export {from} to {to}: {source}")]
    CommitOutput {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires explicit synthetic project, GPU and output directory"]
    #[allow(clippy::too_many_lines)] // Validate three output sizes through complete encode/decode and preview comparison.
    fn canvas_output_export_ui_review() {
        use crate::platform::{editor::ProjectEditorSession, preview::ProjectPreviewSession};
        use panzo_core::CanvasSize;
        let project = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
        let directory = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
        let mut editor = ProjectEditorSession::open(&project).unwrap();
        let id = editor.video_edit().clips[0].id.clone();
        editor
            .trim_video(&id, TimeTick::ZERO, TimeTick::from_millis(500))
            .unwrap();
        editor
            .set_video_crop(
                &id,
                panzo_core::VideoCrop::centered_aspect(2560, 1440, 9.0 / 16.0),
            )
            .unwrap();
        let mut results = Vec::new();
        for (name, size, preset, expected) in [
            (
                "portrait-native",
                CanvasSize {
                    width: 1440,
                    height: 2560,
                },
                ExportPreset::Native,
                (1440, 2560),
            ),
            (
                "portrait-hd",
                CanvasSize {
                    width: 1440,
                    height: 2560,
                },
                ExportPreset::Hd1080,
                (1080, 1920),
            ),
            (
                "custom-square",
                CanvasSize {
                    width: 640,
                    height: 640,
                },
                ExportPreset::Native,
                (640, 640),
            ),
        ] {
            editor.set_canvas_size(Some(size)).unwrap();
            let snapshot = ExportSnapshot {
                camera: editor.camera().clone(),
                settings: editor.settings().clone(),
            };
            let request = ExportRequest {
                project_root: project.clone(),
                output_path: directory.join(format!("{name}-{}.mp4", uuid::Uuid::new_v4())),
                maximum_duration: None,
            };
            let report = ProjectExporter::export_snapshot(
                &request,
                &snapshot,
                preset,
                &std::sync::atomic::AtomicBool::new(false),
                |_, _| {},
            )
            .unwrap();
            assert_eq!(report.frames_written, 30);
            let mut preview = ProjectPreviewSession::open(
                &project,
                OutputDescriptor::bgra8(expected.0, expected.1).unwrap(),
            )
            .unwrap();
            preview
                .apply_edit_state(snapshot.camera, snapshot.settings)
                .unwrap();
            // Leaving crop mode clears both the temporary source view and its editing overlay.
            preview.set_crop_editing(true);
            preview.set_crop_overlay(Some([0.1, 0.1, 0.9, 0.9]));
            preview.set_crop_editing(false);
            let mut decoder = MfBgraDecoder::open(&request.output_path).unwrap();
            let mut count = 0;
            let mut errors = Vec::new();
            while let Some(frame) = decoder.read_next().unwrap() {
                assert_eq!((frame.width, frame.height), expected);
                if [0, 29].contains(&count) {
                    preview.seek(frame.pts).unwrap();
                    let prepared = preview.prepare_current().unwrap();
                    let clean = preview.compose_prepared(&prepared).unwrap().composited;
                    assert_eq!(frame.pixels.len(), clean.pixels.len());
                    let difference: u64 = frame
                        .pixels
                        .chunks_exact(4)
                        .zip(clean.pixels.chunks_exact(4))
                        .step_by(13)
                        .map(|(a, b)| (0..3).map(|c| u64::from(a[c].abs_diff(b[c]))).sum::<u64>())
                        .sum();
                    let n = frame.pixels.chunks_exact(4).step_by(13).count() * 3;
                    let error = difference as f64 / n as f64;
                    // The 640px checkerboard has proportionally more chroma edges after
                    // subsampling; allow 1.5/255 mean error for this lossy H.264 comparison.
                    assert!(error < 1.5, "{name} frame {count}: {error}");
                    errors.push(error);
                }
                count += 1;
            }
            assert_eq!(count, 30);
            results.push(serde_json::json!({"name":name,"width":expected.0,"height":expected.1,"decodedFrames":count,"meanRgbErrors":errors}));
        }
        fs::write(
            directory.join("canvas-export.json"),
            serde_json::to_vec_pretty(&results).unwrap(),
        )
        .unwrap();
    }

    #[test]
    #[ignore = "requires explicit synthetic project, GPU and output directory"]
    fn cropped_clips_export_ui_review() {
        use crate::platform::{editor::ProjectEditorSession, preview::ProjectPreviewSession};
        let project = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
        let directory = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
        let mut editor = ProjectEditorSession::open(&project).unwrap();
        let first = editor.video_edit().clips[0].id.clone();
        editor
            .trim_video(&first, TimeTick::ZERO, TimeTick::from_millis(3000))
            .unwrap();
        let second = editor.split_video(TimeTick::from_millis(1000)).unwrap();
        let third = editor.split_video(TimeTick::from_millis(2000)).unwrap();
        editor
            .set_video_crop(
                &second,
                panzo_core::VideoCrop::centered_aspect(2560, 1440, 1.0),
            )
            .unwrap();
        editor
            .set_video_crop(
                &third,
                panzo_core::VideoCrop {
                    left: 100,
                    top: 100,
                    right: 450,
                    bottom: 200,
                },
            )
            .unwrap();
        editor.set_video_speed(&second, 200).unwrap();
        let snapshot = ExportSnapshot {
            camera: editor.camera().clone(),
            settings: editor.settings().clone(),
        };
        let request = ExportRequest {
            project_root: project.clone(),
            output_path: directory.join(format!("cropped-{}.mp4", uuid::Uuid::new_v4())),
            maximum_duration: None,
        };
        let report = ProjectExporter::export_snapshot(
            &request,
            &snapshot,
            ExportPreset::Hd1080,
            &std::sync::atomic::AtomicBool::new(false),
            |_, _| {},
        )
        .unwrap();
        assert_eq!(report.frames_written, 150);
        let mut preview =
            ProjectPreviewSession::open(&project, OutputDescriptor::bgra8(1920, 1080).unwrap())
                .unwrap();
        preview
            .apply_edit_state(snapshot.camera, snapshot.settings)
            .unwrap();
        let mut decoder = MfBgraDecoder::open(&request.output_path).unwrap();
        let mut samples = Vec::new();
        let mut count = 0;
        while let Some(frame) = decoder.read_next().unwrap() {
            if [0, 59, 60, 89, 90, 149].contains(&count) {
                preview.seek(frame.pts).unwrap();
                let prepared = preview.prepare_current().unwrap();
                let expected = preview.compose_prepared(&prepared).unwrap().composited;
                let difference: u64 = frame
                    .pixels
                    .chunks_exact(4)
                    .zip(expected.pixels.chunks_exact(4))
                    .step_by(13)
                    .map(|(a, b)| (0..3).map(|c| u64::from(a[c].abs_diff(b[c]))).sum::<u64>())
                    .sum();
                let n = frame.pixels.chunks_exact(4).step_by(13).count() * 3;
                let error = difference as f64 / n as f64;
                assert!(error < 1.0, "frame {count}: {error}");
                samples.push(serde_json::json!({"frame":count,"crop":prepared.evaluation.camera_transform.crop,"meanRgbError":error}));
            }
            count += 1;
        }
        assert_eq!(count, 150);
        fs::write(
            directory.join("crop-export.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"report":report.to_string(),"frames":count,"samples":samples}),
            )
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    #[ignore = "requires explicit synthetic project, GPU and output directory"]
    #[allow(clippy::too_many_lines)] // Verify final encoded output against preview across three speed boundaries.
    fn mixed_speed_export_ui_review() {
        use crate::platform::{editor::ProjectEditorSession, preview::ProjectPreviewSession};
        use panzo_core::TimeMapping;
        let project = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
        let output = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
        let mut editor = ProjectEditorSession::open(&project).unwrap();
        let id = editor.video_edit().clips[0].id.clone();
        editor
            .trim_video(&id, TimeTick::ZERO, TimeTick::from_millis(6000))
            .unwrap();
        editor.set_camera_enabled(true);
        let baseline = ExportSnapshot {
            camera: editor.camera().clone(),
            settings: editor.settings().clone(),
        };
        let middle = editor.split_video(TimeTick::from_millis(2000)).unwrap();
        let last = editor.split_video(TimeTick::from_millis(4000)).unwrap();
        editor.set_video_speed(&id, 200).unwrap();
        editor.set_video_speed(&middle, 50).unwrap();
        editor.set_video_speed(&last, 125).unwrap();
        editor.set_camera_enabled(true);
        let snapshot = ExportSnapshot {
            camera: editor.camera().clone(),
            settings: editor.settings().clone(),
        };
        let run = uuid::Uuid::new_v4();
        let request = ExportRequest {
            project_root: project.clone(),
            output_path: output.join(format!("mixed-speed-{run}.mp4")),
            maximum_duration: None,
        };
        let report = ProjectExporter::export_snapshot(
            &request,
            &snapshot,
            ExportPreset::Hd1080,
            &std::sync::atomic::AtomicBool::new(false),
            |_, _| {},
        )
        .unwrap();
        assert_eq!(report.duration_tick, TimeTick::from_millis(6600));
        assert_eq!(report.frames_written, 396);
        let baseline_request = ExportRequest {
            output_path: output.join(format!("original-speed-{run}.mp4")),
            ..request.clone()
        };
        ProjectExporter::export_snapshot(
            &baseline_request,
            &baseline,
            ExportPreset::Hd1080,
            &std::sync::atomic::AtomicBool::new(false),
            |_, _| {},
        )
        .unwrap();
        // Check both preview colour fidelity and like-for-like source selection
        // after encoding at different timeline speeds.
        let sample_pairs = [
            (30, 60),
            (59, 118),
            (60, 120),
            (150, 165),
            (298, 239),
            (300, 240),
            (384, 345),
        ];
        let mut baseline_decoder = MfBgraDecoder::open(&baseline_request.output_path).unwrap();
        let mut baseline_frames = std::collections::HashMap::new();
        let mut baseline_count = 0;
        while let Some(frame) = baseline_decoder.read_next().unwrap() {
            if sample_pairs
                .iter()
                .any(|(_, index)| *index == baseline_count)
            {
                baseline_frames.insert(baseline_count, frame);
            }
            baseline_count += 1;
        }
        assert_eq!(baseline_count, 360);
        let descriptor = OutputDescriptor::bgra8(report.width, report.height).unwrap();
        let mut preview = ProjectPreviewSession::open(&project, descriptor).unwrap();
        preview
            .apply_edit_state(snapshot.camera, snapshot.settings)
            .unwrap();
        let mut decoded = MfBgraDecoder::open(&request.output_path).unwrap();
        let video = editor.video_edit();
        let mut count = 0;
        let mut errors = Vec::new();
        let mut previous = None;
        while let Some(frame) = decoded.read_next().unwrap() {
            assert!(previous.is_none_or(|pts| frame.pts > pts));
            previous = Some(frame.pts);
            if let Some((_, baseline_index)) =
                sample_pairs.iter().find(|(index, _)| *index == count)
            {
                preview.seek(frame.pts).unwrap();
                let prepared = preview.prepare_current().unwrap();
                assert_eq!(
                    prepared.evaluation.source_tick,
                    video.source_time(frame.pts).unwrap()
                );
                let expected = preview.compose_prepared(&prepared).unwrap().composited;
                assert_eq!(expected.pixels.len(), frame.pixels.len());
                let mut difference = 0_u64;
                let mut samples = 0_u64;
                for (actual, expected) in frame
                    .pixels
                    .chunks_exact(4)
                    .zip(expected.pixels.chunks_exact(4))
                    .step_by(13)
                {
                    for channel in 0..3 {
                        difference += u64::from(actual[channel].abs_diff(expected[channel]));
                        samples += 1;
                    }
                }
                let error = difference as f64 / samples as f64;
                assert!(
                    error < 1.0,
                    "preview/export frame {count} mean RGB error {error}"
                );
                let baseline = &baseline_frames[baseline_index];
                assert!((baseline.pts.0 - prepared.evaluation.source_tick.0).abs() <= 2);
                let encoded_difference: u64 = frame
                    .pixels
                    .chunks_exact(4)
                    .zip(baseline.pixels.chunks_exact(4))
                    .step_by(13)
                    .map(|(actual, expected)| {
                        (0..3)
                            .map(|channel| u64::from(actual[channel].abs_diff(expected[channel])))
                            .sum::<u64>()
                    })
                    .sum();
                let encoded_error = encoded_difference as f64 / samples as f64;
                assert!(
                    encoded_error < 1.0,
                    "encoded speed/reference frame {count} mean error {encoded_error}"
                );
                errors.push(serde_json::json!({"frame":count,"projectTick":frame.pts.0,"sourceTick":prepared.evaluation.source_tick.0,
                    "previewToEncodedMeanRgbError":error,"encodedNormalSpeedReferenceMeanRgbError":encoded_error}));
            }
            count += 1;
        }
        assert_eq!(count, report.frames_written);
        let result = serde_json::json!({"frames":count,"durationTick":report.duration_tick.0,"size":[report.width, report.height],"comparisons":errors,"report":report.to_string()});
        fs::write(
            output.join("mixed-speed-export.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn cfr_frame_ticks_are_exact_at_one_second() {
        assert_eq!(export_frame_tick(0), TimeTick::ZERO);
        assert_eq!(export_frame_tick(1), TimeTick(166_666));
        assert_eq!(export_frame_tick(60), TimeTick(TICKS_PER_SECOND));
        assert_eq!(export_frame_count(TimeTick(TICKS_PER_SECOND)).unwrap(), 60);
        assert_eq!(
            export_frame_count(TimeTick(TICKS_PER_SECOND + 1)).unwrap(),
            61
        );
    }
}
