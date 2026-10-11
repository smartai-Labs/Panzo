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
#[cfg(test)]
const EXPORT_FPS: i64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Mp4,
    Gif,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExportQuality {
    Clear,
    #[default]
    Balanced,
    Small,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportOptions {
    pub format: ExportFormat,
    pub fps: u32,
    pub range: Option<(TimeTick, TimeTick)>,
    pub audio: bool,
    pub quality: ExportQuality,
}
impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::Mp4,
            fps: 60,
            range: None,
            audio: true,
            quality: ExportQuality::Balanced,
        }
    }
}
impl ExportOptions {
    pub fn extension(self) -> &'static str {
        match self.format {
            ExportFormat::Mp4 => "mp4",
            ExportFormat::Gif => "gif",
        }
    }
    fn validate(self) -> Result<(), ExportError> {
        let valid = match self.format {
            ExportFormat::Mp4 => matches!(self.fps, 30 | 60),
            ExportFormat::Gif => matches!(self.fps, 10 | 15 | 20 | 25 | 30),
        };
        if valid {
            Ok(())
        } else {
            Err(ExportError::InvalidOptions)
        }
    }
}

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
    pub frames_per_second: u32,
    pub format: ExportFormat,
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
            "Panzo {:?} export — {}×{} / {} fps",
            self.format, self.width, self.height, self.frames_per_second
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
    Share720,
    Compact480,
}

impl ExportPreset {
    pub fn dimensions(self, source: (u32, u32), settings: &WorkbenchSettings) -> (u32, u32) {
        if matches!(self, Self::Share720 | Self::Compact480) {
            let source = settings.canvas.size.map_or(source, |c| (c.width, c.height));
            let bound = if self == Self::Share720 { 1280 } else { 640 };
            return fit_dimensions(source, bound);
        }
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
        Self::export_snapshot_with_options(
            request,
            snapshot,
            preset,
            ExportOptions::default(),
            cancel,
            progress,
        )
    }

    pub fn export_snapshot_with_options(
        request: &ExportRequest,
        snapshot: &ExportSnapshot,
        preset: ExportPreset,
        options: ExportOptions,
        cancel: &std::sync::atomic::AtomicBool,
        progress: impl Fn(u32, u32),
    ) -> Result<ExportReport, ExportError> {
        options.validate()?;
        if request.output_path.extension().and_then(|v| v.to_str()) != Some(options.extension()) {
            return Err(ExportError::InvalidOutputExtension(
                request.output_path.clone(),
            ));
        }
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
            options,
            cancel,
            &progress,
        )
        .and_then(|mut report| {
            if options.format == ExportFormat::Gif {
                fs::rename(&encoded_output, &temporary_output).map_err(Fmp4ProbeError::from)?;
                return Ok(report);
            }
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

#[allow(clippy::too_many_lines)] // One cancellation-safe streaming export transaction.
fn export_to_temporary(
    request: &ExportRequest,
    temporary_output: &Path,
    snapshot: &ExportSnapshot,
    preset: ExportPreset,
    options: ExportOptions,
    cancel: &std::sync::atomic::AtomicBool,
    progress: &impl Fn(u32, u32),
) -> Result<ExportReport, ExportError> {
    std::thread::scope(|scope| {
        let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(cancel.load(std::sync::atomic::Ordering::Acquire)));
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        struct Finish(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Drop for Finish { fn drop(&mut self) { self.0.store(true, std::sync::atomic::Ordering::Release); } }
        let guard = Finish(done.clone());
        let worker_token = token.clone();
        scope.spawn(move || {
            while !done.load(std::sync::atomic::Ordering::Acquire) {
                if cancel.load(std::sync::atomic::Ordering::Acquire) { worker_token.store(true, std::sync::atomic::Ordering::Release); break; }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        let result = export_to_temporary_inner(request, temporary_output, snapshot, preset, options, &token, progress);
        drop(guard);
        result
    })
}

fn export_to_temporary_inner(
    request: &ExportRequest,
    temporary_output: &Path,
    snapshot: &ExportSnapshot,
    preset: ExportPreset,
    options: ExportOptions,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    progress: &impl Fn(u32, u32),
) -> Result<ExportReport, ExportError> {
    let started = Instant::now();
    let layout = ProjectLayout::open(&request.project_root)?;
    let manifest = layout.load_manifest()?;
    let media_path = layout.root().join(&manifest.media.screen);
    reject_source_overwrite(&media_path, &request.output_path)?;
    let mut export_settings=snapshot.settings.clone();
    export_settings.video=Some(export_settings.video_edit(TimeTick(manifest.media.duration_tick)).map_err(EditorSessionError::from)?);
    let settings=&export_settings;
    settings.validate().map_err(EditorSessionError::from)?;
    let video = settings
        .video_edit(TimeTick(manifest.media.duration_tick))
        .map_err(EditorSessionError::from)?;
    let project_duration = settings.project_duration(TimeTick(manifest.media.duration_tick));
    let (range_start, range_end) = options.range.unwrap_or((TimeTick::ZERO, project_duration));
    if range_start < TimeTick::ZERO || range_end <= range_start || range_end > project_duration {
        return Err(ExportError::InvalidDuration(range_end));
    }
    let selected_duration = TimeTick(range_end.0 - range_start.0);
    let duration = request
        .maximum_duration
        .map_or(selected_duration, |limit| limit.min(selected_duration));
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
    let frame_count = frame_count_at_rate(duration, options.fps)?;

    let compositor = D3d11Compositor::create()?;
    compositor.set_project_root_with_cancel(layout.root(),cancel.clone());
    let conversion_device = if options.format == ExportFormat::Mp4 {
        Some(D3d11Device::create_hardware().map_err(ExportError::D3dDevice)?)
    } else {
        None
    };
    let converter = conversion_device
        .as_ref()
        .map(|device| Nv12VideoProcessor::create_with_output(device, width, height, width, height))
        .transpose()?;
    let mut audio =
        if options.format == ExportFormat::Mp4 && options.audio && settings.audio.enabled {
            crate::platform::audio::AudioMixer::with_clips(layout.root(), &settings.mixing_clips()).map_err(ExportError::Audio)?
        } else {
            crate::platform::audio::AudioMixer::default()
        };
    let with_audio = options.audio && settings.audio.enabled && audio.has_audio();
    let mut encoding = Nv12VideoConfig::sharing(width, height, options.fps)?;
    encoding.target_bitrate = match options.quality {
        ExportQuality::Clear => encoding.target_bitrate.saturating_mul(2).min(80_000_000),
        ExportQuality::Balanced => encoding.target_bitrate,
        ExportQuality::Small => (encoding.target_bitrate / 2).max(300_000),
    };
    let mut written_audio = 0u64;
    let mut writer = if options.format == ExportFormat::Mp4 {
        Some(Nv12Fmp4Writer::create_with_audio(
            temporary_output,
            encoding,
            with_audio,
        )?)
    } else {
        None
    };
    let mut gif = if options.format == ExportFormat::Gif {
        let file = fs::File::create(temporary_output).map_err(Fmp4ProbeError::from)?;
        let mut encoder = image::codecs::gif::GifEncoder::new_with_speed(file, 10);
        encoder.set_repeat(image::codecs::gif::Repeat::Infinite)?;
        Some(encoder)
    } else {
        None
    };
    let mut decoded_index = 0_usize;
    let mut current_frame = decoder.read_next()?.ok_or(ExportError::NoSourceFrame)?;

    for output_index in 0..frame_count {
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err(ExportError::Cancelled);
        }
        let project_tick =
            TimeTick(range_start.0 + frame_tick_at_rate(output_index, options.fps).0);
        let evaluation = evaluator.evaluate_edit(project_tick, output_descriptor, settings)?;
        if settings.source_video_enabled && decoded_index>evaluation.source_frame.frame_index{
            decoder.seek(evaluation.source_frame.presentation_tick)?;
            loop{if cancel.load(std::sync::atomic::Ordering::Acquire){return Err(ExportError::Cancelled);}match decoder.read_next_toward(evaluation.source_frame.presentation_tick)?{super::media_decoder::TargetFrameRead::Before(_)=>{},super::media_decoder::TargetFrameRead::AtOrAfter(f)=>{current_frame=f;decoded_index=evaluation.source_frame.frame_index;break;},super::media_decoder::TargetFrameRead::End=>return Err(ExportError::NoSourceFrame)}}
        }
        while settings.source_video_enabled && decoded_index < evaluation.source_frame.frame_index {
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                return Err(ExportError::Cancelled);
            }
            current_frame = decoder.read_next()?.ok_or(ExportError::SourceEndedEarly {
                required_index: evaluation.source_frame.frame_index,
                decoded_index,
            })?;
            decoded_index += 1;
        }
        if settings.source_video_enabled{assert_selected_frame(&current_frame, &evaluation)?;}
        let bgra = compose_export_frame(
            &compositor,
            &current_frame,
            &evaluation,
            settings,
            background.as_ref(),
        )?;
        let sample_start = frame_tick_at_rate(output_index, options.fps);
        let sample_end = frame_tick_at_rate(output_index + 1, options.fps).min(duration);
        if let (Some(writer), Some(converter)) = (&mut writer, &converter) {
            let nv12 = converter.convert_bgra_bytes(&bgra.pixels)?;
            writer.write_nv12(&nv12, sample_start.0, sample_end.0 - sample_start.0)?;
            if with_audio {
                let end = crate::platform::audio::frames_at(sample_end);
                while written_audio < end {
                    let count = (end - written_audio).min(1024) as usize;
                    let samples = audio
                        .render(written_audio, count, range_start, &video, settings.audio)
                        .map_err(ExportError::Audio)?;
                    writer.write_audio(&samples, written_audio)?;
                    written_audio += count as u64;
                }
            }
        }
        if let Some(encoder) = &mut gif {
            let mut pixels = bgra.pixels;
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
                pixel[3] = 255;
            }
            let image = image::RgbaImage::from_raw(width, height, pixels)
                .ok_or(ExportError::InvalidOptions)?;
            let delay = gif_delay_ms(sample_start, sample_end);
            encoder.encode_frame(image::Frame::from_parts(
                image,
                0,
                0,
                image::Delay::from_numer_denom_ms(delay, 1),
            ))?;
        }
        progress(output_index + 1, frame_count);
    }

    if options.format == ExportFormat::Gif {
        drop(gif);
        return Ok(ExportReport {
            project_root: layout.root().into(),
            output_path: temporary_output.into(),
            frames_written: frame_count,
            frames_per_second: options.fps,
            format: options.format,
            width,
            height,
            duration_tick: duration,
            output_bytes: fs::metadata(temporary_output)
                .map_err(Fmp4ProbeError::from)?
                .len(),
            fragment_count: 0,
            trailing_bytes: 0,
            encoder_name: "Panzo GIF".into(),
            hardware_encoder: false,
            elapsed_ms: started.elapsed().as_millis(),
        });
    }
    let finalized = writer.ok_or(ExportError::InvalidOptions)?.finish()?;
    if finalized.inspection.trailing_bytes != 0 {
        return Err(ExportError::TrailingOutputBytes(
            finalized.inspection.trailing_bytes,
        ));
    }
    Ok(ExportReport {
        project_root: layout.root().into(),
        output_path: temporary_output.into(),
        frames_written: finalized.frames_written,
        frames_per_second: options.fps,
        format: options.format,
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
    if settings.source_video_enabled{assert_selected_frame(frame, evaluation)?;}
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

#[cfg(test)]
fn export_frame_count(duration: TimeTick) -> Result<u32, ExportError> {
    frame_count_at_rate(duration, EXPORT_FPS as u32)
}
fn frame_count_at_rate(duration: TimeTick, fps: u32) -> Result<u32, ExportError> {
    let frames = (i128::from(duration.as_i64()) * i128::from(fps)
        + i128::from(TICKS_PER_SECOND - 1))
        / i128::from(TICKS_PER_SECOND);
    u32::try_from(frames).map_err(|_| ExportError::DurationTooLong(duration))
}

#[cfg(test)]
fn export_frame_tick(frame_index: u32) -> TimeTick {
    frame_tick_at_rate(frame_index, EXPORT_FPS as u32)
}
fn frame_tick_at_rate(frame_index: u32, fps: u32) -> TimeTick {
    TimeTick(i64::from(frame_index) * TICKS_PER_SECOND / i64::from(fps))
}
fn gif_delay_ms(start: TimeTick, end: TimeTick) -> u32 {
    let centisecond = TICKS_PER_SECOND / 100;
    let rounded = |tick: TimeTick| (tick.0 + centisecond / 2) / centisecond;
    u32::try_from((rounded(end) - rounded(start)).max(1) * 10).unwrap_or(u32::MAX)
}
#[allow(clippy::cast_sign_loss)] // Valid positive dimensions, clamped to at least two pixels.
fn fit_dimensions(source: (u32, u32), width: u32) -> (u32, u32) {
    let ratio = f64::from(width) / f64::from(source.0.max(1));
    let ratio = ratio.min(1.0);
    (
        ((f64::from(source.0) * ratio) as u32 & !1).max(2),
        ((f64::from(source.1) * ratio) as u32 & !1).max(2),
    )
}

fn temporary_output_path(output: &Path) -> Result<PathBuf, ExportError> {
    if output
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|v| v != "mp4" && v != "gif")
    {
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
    #[error("音频导出失败：{0}")]
    Audio(String),
    #[error("无效的导出格式或帧率")]
    InvalidOptions,
    #[error(transparent)]
    Image(#[from] image::ImageError),
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
    #[error("export extension must match the selected MP4/GIF format: {0}")]
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
    fn selectable_rates_preserve_total_duration_and_size() {
        for fps in [10, 15, 20, 25, 30, 60] {
            assert_eq!(
                frame_count_at_rate(TimeTick::from_millis(1000), fps).unwrap(),
                fps
            );
            assert_eq!(frame_tick_at_rate(fps, fps), TimeTick::from_millis(1000));
            if fps <= 30 {
                let milliseconds: u32 = (0..fps)
                    .map(|index| {
                        gif_delay_ms(
                            frame_tick_at_rate(index, fps),
                            frame_tick_at_rate(index + 1, fps),
                        )
                    })
                    .sum();
                assert_eq!(milliseconds, 1000);
            }
        }
        assert_eq!(fit_dimensions((1920, 1080), 640), (640, 360));
        assert_eq!(fit_dimensions((720, 1280), 640), (640, 1136));
    }

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
        editor.set_canvas_inset(100.0/1080.0).unwrap();
        editor.set_video_scale(&second,750).unwrap();
        editor.set_video_scale(&third,1255).unwrap();
        let background=directory.join("export-background.png");
        image::RgbaImage::from_pixel(64,64,image::Rgba([45,75,110,255])).save(&background).unwrap();
        editor.import_background_image(&background).unwrap();
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
                samples.push(serde_json::json!({"frame":count,"crop":prepared.evaluation.camera_transform.crop,"contentScale":prepared.evaluation.camera_transform.content_scale,"meanRgbError":error}));
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
