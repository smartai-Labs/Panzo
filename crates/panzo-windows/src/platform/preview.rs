use crate::platform::background_image::DecodedBackgroundImage;
use crate::platform::camera_regeneration::{
    CameraRegenerationError, CameraRegenerationReport, CameraTrackRegenerator,
};
use crate::platform::compositor::{
    CompositedFrame, CompositorCacheStats, CompositorError, D3d11Compositor, GpuPreviewPresenter,
};
use crate::platform::decode_worker::{DecodePlan, DecodeWorker};
use crate::platform::editor::{
    EditorSessionError, load_effective_background, load_effective_edit_state,
};
use crate::platform::media_decoder::{DecodedBgraFrame, MediaDecoderError, MfBgraDecoder};
use panzo_core::{
    CameraSegmentKind, CameraState, CameraTrack, CursorEvent, CursorTimeline, FrameEvaluation,
    FrameEvaluator, OutputDescriptor, PreviewController, PreviewControllerError, PreviewSnapshot,
    ProjectIoError, ProjectLayout, RenderEvaluationError, SourceFrameSelection,
    SourceFrameTimeline, TimeMapping, TimeTick, WorkbenchSettings, WorkbenchValidationError,
};
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use thiserror::Error;

const INITIAL_RANDOM_SEEK_PREROLL_FRAMES: usize = 30;

#[cfg(test)]
#[path = "speed_stability_tests.rs"]
mod speed_stability_tests;

#[derive(Debug, Clone, PartialEq)]
pub struct PreviewFrame {
    pub snapshot: PreviewSnapshot,
    pub evaluation: FrameEvaluation,
    pub composited: CompositedFrame,
}

/// A decoded/evaluated preview frame that is ready for either CPU readback or direct GPU
/// presentation. Keeping this stage explicit lets probes retain deterministic BGRA hashes while
/// the interactive Editor avoids the GPU -> CPU -> GDI round trip.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedPreviewFrame {
    pub snapshot: PreviewSnapshot,
    pub evaluation: FrameEvaluation,
    // Pin the pixels evaluated for this presentation. A pending seek may replace
    // the session cache before a resize or overlay redraws the displayed frame.
    decoded: Arc<DecodedBgraFrame>,
}

impl PreparedPreviewFrame {
    pub fn source_size(&self) -> (u32, u32) {
        self.decoded
            .native_size
            .unwrap_or((self.decoded.width, self.decoded.height))
    }

    pub fn content_rect(&self, settings: &WorkbenchSettings) -> panzo_core::SourceRect {
        self.evaluation
            .content_rect(self.source_size(), settings.canvas.inset)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrubDecodePlan {
    pub project_tick: TimeTick,
    pub target_pts: TimeTick,
    pub fallback_pts: Option<TimeTick>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraDiagnostics {
    pub visible: bool,
    pub project_tick: TimeTick,
    pub state: CameraState,
    pub active_segment: Option<CameraSegmentDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraSegmentDiagnostics {
    pub id: String,
    pub kind: CameraSegmentKind,
    pub start_tick: TimeTick,
    pub end_tick: TimeTick,
    pub source_click_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewSessionProbeReport {
    pub project_root: PathBuf,
    pub source_frames: usize,
    pub duration_tick: TimeTick,
    pub scrub_project_tick: TimeTick,
    pub scrub_source_pts: TimeTick,
    pub scrub_poll_count: u32,
    pub seek_project_tick: TimeTick,
    pub seek_source_pts: TimeTick,
    pub advanced_project_tick: TimeTick,
    pub start_hash: String,
    pub returned_start_hash: String,
    pub deterministic_start: bool,
    pub camera_diagnostics_visible: bool,
}

impl fmt::Display for PreviewSessionProbeReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Panzo M3 project preview session probe")?;
        writeln!(formatter, "  Project: {}", self.project_root.display())?;
        writeln!(formatter, "  Source frames: {}", self.source_frames)?;
        writeln!(
            formatter,
            "  Duration tick: {}",
            self.duration_tick.as_i64()
        )?;
        writeln!(
            formatter,
            "  Scrub project/source tick: {}/{}",
            self.scrub_project_tick.as_i64(),
            self.scrub_source_pts.as_i64()
        )?;
        writeln!(formatter, "  Scrub poll count: {}", self.scrub_poll_count)?;
        writeln!(
            formatter,
            "  Seek project/source tick: {}/{}",
            self.seek_project_tick.as_i64(),
            self.seek_source_pts.as_i64()
        )?;
        writeln!(
            formatter,
            "  Play+advance project tick: {}",
            self.advanced_project_tick.as_i64()
        )?;
        writeln!(formatter, "  Start hash: {}", self.start_hash)?;
        writeln!(
            formatter,
            "  Returned start hash: {}",
            self.returned_start_hash
        )?;
        write!(
            formatter,
            "  Deterministic start frame: {}\n  Camera diagnostics visible: {}",
            self.deterministic_start, self.camera_diagnostics_visible
        )
    }
}

pub struct PreviewSessionProbe;

impl PreviewSessionProbe {
    pub fn inspect(
        project_root: impl AsRef<Path>,
        output: OutputDescriptor,
    ) -> Result<PreviewSessionProbeReport, PreviewSessionError> {
        Self::inspect_at(project_root, output, TimeTick::from_millis(1_000))
    }

    pub fn inspect_at(
        project_root: impl AsRef<Path>,
        output: OutputDescriptor,
        requested_seek_tick: TimeTick,
    ) -> Result<PreviewSessionProbeReport, PreviewSessionError> {
        let mut session = ProjectPreviewSession::open(project_root, output)?;
        let source_frames = session.source_frame_count();
        let duration_tick = session.snapshot().duration_tick;
        let start = session.render_current()?;

        let seek_project_tick = requested_seek_tick.min(duration_tick);
        session.seek(seek_project_tick)?;
        let (scrub, scrub_poll_count) = if let Some(frame) = session.begin_scrub_render()? {
            (frame, 0)
        } else {
            let mut polls = 0_u32;
            loop {
                polls = polls.saturating_add(1);
                if let Some(frame) = session.poll_interactive_render(4)? {
                    break (frame, polls);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        session.seek(seek_project_tick)?;
        let seek = if let Some(frame) = session.begin_interactive_render()? {
            frame
        } else {
            loop {
                if let Some(frame) = session.poll_interactive_render(4)? {
                    break frame;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        session.toggle_camera_diagnostics();
        let camera_diagnostics_visible = session.camera_diagnostics().visible;
        session.play();
        session.advance(TimeTick::from_millis(100))?;
        let advanced = session.render_current()?;
        session.pause();
        session.jump_to_start();
        let returned_start = session.render_current()?;

        let start_hash = start.composited.sha256_hex();
        let returned_start_hash = returned_start.composited.sha256_hex();
        let deterministic_start = start_hash == returned_start_hash;
        Ok(PreviewSessionProbeReport {
            project_root: session.project_root().into(),
            source_frames,
            duration_tick,
            scrub_project_tick: scrub.snapshot.project_tick,
            scrub_source_pts: scrub.evaluation.source_frame.presentation_tick,
            scrub_poll_count,
            seek_project_tick: seek.snapshot.project_tick,
            seek_source_pts: seek.evaluation.source_frame.presentation_tick,
            advanced_project_tick: advanced.snapshot.project_tick,
            start_hash,
            returned_start_hash,
            deterministic_start,
            camera_diagnostics_visible,
        })
    }
}

/// Owns the project data and Windows render resources needed for deterministic interactive
/// preview. Both preview and export use `FrameEvaluator` and `D3d11Compositor`.
pub struct ProjectPreviewSession {
    source_duration: TimeTick,
    project_root: PathBuf,
    media_path: PathBuf,
    output: OutputDescriptor,
    crop_editing: bool,
    controller: PreviewController,
    source_timeline: SourceFrameTimeline,
    camera: CameraTrack,
    settings: WorkbenchSettings,
    background: Option<DecodedBackgroundImage>,
    background_decodes: u64,
    cursor: CursorTimeline,
    decoder: DecodeWorker,
    compositor: D3d11Compositor,
    decoded_frame: Option<Arc<DecodedBgraFrame>>,
    decoded_frame_native: bool,
    pending_render: Option<PendingPreviewRender>,
}

/// CPU data and media-worker handles prepared before the UI creates render resources.
/// No apartment-bound decoder, compositor, presenter, or window belongs to this value.
pub(crate) struct PreparedPreviewSession {
    source_duration: TimeTick,
    project_root: PathBuf,
    media_path: PathBuf,
    output: OutputDescriptor,
    controller: PreviewController,
    source_timeline: SourceFrameTimeline,
    camera: CameraTrack,
    settings: WorkbenchSettings,
    background: Option<DecodedBackgroundImage>,
    background_decodes: u64,
    cursor: CursorTimeline,
    decoder: DecodeWorker,
    decoded_frame: Option<Arc<DecodedBgraFrame>>,
}

impl PreparedPreviewSession {
    fn open(
        project_root: &Path,
        output: OutputDescriptor,
        edit_state: Option<(CameraTrack, WorkbenchSettings)>,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self, PreviewSessionError> {
        check_preparation_cancelled(cancelled)?;
        let layout = ProjectLayout::open(project_root)?;
        let manifest = layout.load_manifest()?;
        let duration_tick = TimeTick(manifest.media.duration_tick);
        let (camera, settings) = match edit_state {
            Some(state) => state,
            None => load_effective_edit_state(layout.root())?,
        };
        camera.validate().map_err(RenderEvaluationError::from)?;
        settings.validate()?;
        let duration = settings
            .video_edit(duration_tick)
            .map_err(EditorSessionError::from)?
            .duration();
        let controller = PreviewController::new(duration)?;
        let background = load_effective_background(layout.root(), &settings)?;
        check_preparation_cancelled(cancelled)?;
        let background_decodes = u64::from(background.is_some());
        let cursor_path = layout.root().join(&manifest.tracks.cursor);
        let cursor = read_cursor_events(&cursor_path, cancelled)?;
        let media_path = layout.root().join(&manifest.media.screen);
        let source_timeline = read_source_timeline(&media_path, cancelled)?;
        check_preparation_cancelled(cancelled)?;
        let decoder =
            DecodeWorker::start(&media_path, None).map_err(PreviewSessionError::Worker)?;
        Ok(Self {
            source_duration: duration_tick,
            project_root: layout.root().into(),
            media_path,
            output,
            controller,
            source_timeline,
            camera,
            settings,
            background,
            background_decodes,
            cursor,
            decoder,
            decoded_frame: None,
        })
    }

    /// Run on the load worker. The first frame reflects restored edits, including trims.
    pub(crate) fn prepare_editor_cancellable(
        project_root: &Path,
        output: OutputDescriptor,
        camera: CameraTrack,
        settings: WorkbenchSettings,
        cancelled: &AtomicBool,
    ) -> Result<Self, PreviewSessionError> {
        let is_cancelled = || cancelled.load(Ordering::Acquire);
        let mut prepared = Self::open(
            project_root,
            output,
            Some((camera, settings)),
            &is_cancelled,
        )?;
        let snapshot = prepared.controller.snapshot();
        let evaluation = FrameEvaluator::new(
            &prepared.source_timeline,
            &prepared.camera,
            &prepared.cursor,
        )?
        .evaluate_edit(snapshot.project_tick, prepared.output, &prepared.settings)?;
        check_preparation_cancelled(&is_cancelled)?;
        let decoded = decode_exact_frame(
            &prepared.decoder,
            &prepared.source_timeline,
            snapshot,
            evaluation.source_frame.frame_index,
        )?;
        check_preparation_cancelled(&is_cancelled)?;
        if decoded.pts != evaluation.source_frame.presentation_tick {
            return Err(PreviewSessionError::SelectionMismatch {
                evaluator_tick: evaluation.source_frame.presentation_tick,
                decoded_tick: decoded.pts,
            });
        }
        prepared.decoded_frame = Some(decoded);
        Ok(prepared)
    }

    pub(crate) fn source_media_path(&self) -> &Path {
        &self.media_path
    }
}

struct PendingPreviewRender {
    snapshot: PreviewSnapshot,
    evaluation: FrameEvaluation,
    target_index: usize,
    target_pts: TimeTick,
    generation: u64,
    started: Instant,
    mode: PendingPreviewMode,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingPreviewMode {
    Exact,
    Scrub,
}

impl ProjectPreviewSession {
    /// Returns the highest useful preview resolution for this project: the decoded source
    /// video's native dimensions. Preview must not silently downsample a high-resolution
    /// recording, and upscaling beyond the source would not add detail.
    pub fn native_output_descriptor(
        project_root: impl AsRef<Path>,
    ) -> Result<OutputDescriptor, PreviewSessionError> {
        let layout = ProjectLayout::open(project_root)?;
        let manifest = layout.load_manifest()?;
        let media_path = layout.root().join(&manifest.media.screen);
        let decoder = MfBgraDecoder::open(media_path)?;
        OutputDescriptor::bgra8(decoder.width(), decoder.height())
            .map_err(PreviewSessionError::Evaluation)
    }

    pub fn open(
        project_root: impl AsRef<Path>,
        output: OutputDescriptor,
    ) -> Result<Self, PreviewSessionError> {
        Self::from_prepared(PreparedPreviewSession::open(
            project_root.as_ref(),
            output,
            None,
            &|| false,
        )?)
    }

    /// Complete on the UI thread. File reads and initial decoding have already finished
    /// for editor loads; only render resources are created here.
    pub(crate) fn from_prepared(
        prepared: PreparedPreviewSession,
    ) -> Result<Self, PreviewSessionError> {
        let compositor = D3d11Compositor::create()?;
        let decoded_frame_native = prepared.decoded_frame.is_some();
        Ok(Self {
            source_duration: prepared.source_duration,
            project_root: prepared.project_root,
            media_path: prepared.media_path,
            output: prepared.output,
            crop_editing: false,
            controller: prepared.controller,
            source_timeline: prepared.source_timeline,
            camera: prepared.camera,
            settings: prepared.settings,
            background: prepared.background,
            background_decodes: prepared.background_decodes,
            cursor: prepared.cursor,
            decoder: prepared.decoder,
            compositor,
            decoded_frame: prepared.decoded_frame,
            decoded_frame_native,
            pending_render: None,
        })
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    pub fn source_media_path(&self) -> &Path {
        &self.media_path
    }

    pub fn background_preview(&self) -> Option<&Arc<image::RgbaImage>> {
        self.background.as_ref().map(|image| &image.preview)
    }

    pub fn set_frame_ready_callback(&self, callback: impl Fn() + Send + Sync + 'static) {
        self.decoder.set_completion_callback(callback);
    }

    pub const fn output(&self) -> OutputDescriptor {
        self.output
    }

    pub fn set_crop_editing(&mut self, enabled: bool) {
        if self.crop_editing != enabled {
            self.cancel_interactive_render();
        }
        self.crop_editing = enabled;
        if !enabled {
            self.compositor.set_crop_overlay(None);
        }
    }
    pub fn set_crop_overlay(&self, rect: Option<[f64; 4]>) {
        self.compositor.set_crop_overlay(rect);
    }
    fn preview_evaluation(&self, mut frame: FrameEvaluation) -> FrameEvaluation {
        if self.crop_editing {
            frame.camera_state = CameraState::BASE;
            frame.camera_transform = CameraState::BASE.into();
            frame.cursor = self
                .cursor
                .evaluate(frame.source_tick, frame.camera_transform);
        }
        frame
    }

    pub fn set_output(&mut self, output: OutputDescriptor) {
        if self.output != output {
            self.cancel_interactive_render();
            self.output = output;
        }
    }

    pub fn frame_step_tick(&self, forward: bool) -> TimeTick {
        panzo_core::preview::step_frame_tick(
            self.snapshot().project_tick,
            self.snapshot().duration_tick,
            forward,
        )
    }

    pub const fn snapshot(&self) -> PreviewSnapshot {
        self.controller.snapshot()
    }

    pub fn source_frame_count(&self) -> usize {
        self.source_timeline.presentation_ticks().len()
    }

    pub const fn decoded_frame_is_native(&self) -> bool {
        self.decoded_frame_native
    }

    pub fn next_video_frame_delay(&self) -> Option<Duration> {
        let snapshot = self.controller.snapshot();
        if snapshot.playback != panzo_core::PlaybackState::Playing {
            return None;
        }
        let video = self.settings.video_edit(self.source_duration).ok()?;
        next_video_delay(&video, &self.source_timeline, snapshot.project_tick)
    }

    pub fn play(&mut self) -> PreviewSnapshot {
        self.controller.play()
    }

    pub fn pause(&mut self) -> PreviewSnapshot {
        self.controller.pause()
    }

    pub fn toggle_playback(&mut self) -> PreviewSnapshot {
        self.controller.toggle_playback()
    }

    pub fn seek(&mut self, project_tick: TimeTick) -> Result<PreviewSnapshot, PreviewSessionError> {
        self.cancel_interactive_render();
        self.controller
            .seek(project_tick)
            .map_err(PreviewSessionError::Controller)
    }

    pub fn jump_to_start(&mut self) -> PreviewSnapshot {
        self.controller.jump_to_start()
    }

    pub fn advance(
        &mut self,
        elapsed_tick: TimeTick,
    ) -> Result<PreviewSnapshot, PreviewSessionError> {
        self.controller
            .advance(elapsed_tick)
            .map_err(PreviewSessionError::Controller)
    }

    pub fn toggle_camera_diagnostics(&mut self) -> PreviewSnapshot {
        self.controller.toggle_camera_diagnostics()
    }

    pub fn camera_diagnostics(&self) -> CameraDiagnostics {
        let snapshot = self.controller.snapshot();
        // Diagnostic values must follow the same source-anchored mapping as
        // the displayed frame, including edits and the explicit effect bypass.
        let source_tick =
            effective_camera_tick(&self.settings, self.source_duration, snapshot.project_tick);
        let active_segment = self
            .camera
            .segments
            .iter()
            .find(|segment| {
                source_tick
                    .is_some_and(|tick| segment.start_tick <= tick && tick < segment.end_tick)
            })
            .map(|segment| CameraSegmentDiagnostics {
                id: segment.id.clone(),
                kind: segment.kind,
                start_tick: segment.start_tick,
                end_tick: segment.end_tick,
                source_click_ids: segment.source_click_ids.clone(),
            });
        CameraDiagnostics {
            visible: snapshot.camera_diagnostics_visible,
            project_tick: snapshot.project_tick,
            state: source_tick.map_or(CameraState::BASE, |tick| self.camera.evaluate(tick)),
            active_segment,
        }
    }

    pub const fn camera_track(&self) -> &CameraTrack {
        &self.camera
    }

    pub const fn workbench_settings(&self) -> &WorkbenchSettings {
        &self.settings
    }

    pub const fn background_decodes(&self) -> u64 {
        self.background_decodes
    }

    pub fn compositor_cache_stats(&self) -> CompositorCacheStats {
        self.compositor.cache_stats()
    }

    pub fn create_presenter(
        &self,
        hwnd: windows::Win32::Foundation::HWND,
    ) -> Result<GpuPreviewPresenter, PreviewSessionError> {
        self.compositor
            .create_presenter(hwnd, self.output.width, self.output.height)
            .map_err(Into::into)
    }

    pub fn apply_edit_state(
        &mut self,
        camera: CameraTrack,
        settings: WorkbenchSettings,
    ) -> Result<(), PreviewSessionError> {
        self.cancel_interactive_render();
        camera.validate().map_err(RenderEvaluationError::from)?;
        settings.validate()?;
        let background_changed = self.settings.background != settings.background;
        let background = if background_changed {
            self.compositor.invalidate_background_cache();
            let background = load_effective_background(&self.project_root, &settings)?;
            self.background_decodes += u64::from(background.is_some());
            background
        } else {
            self.background.take()
        };
        self.apply_edit_state_with_background(camera, settings, background)
    }

    pub fn background_matches(&self, settings: &WorkbenchSettings) -> bool {
        self.settings.background == settings.background
    }

    pub fn apply_edit_state_with_background(
        &mut self,
        camera: CameraTrack,
        settings: WorkbenchSettings,
        background: Option<DecodedBackgroundImage>,
    ) -> Result<(), PreviewSessionError> {
        self.cancel_interactive_render();
        camera.validate().map_err(RenderEvaluationError::from)?;
        settings.validate()?;
        if self.settings.background != settings.background {
            self.compositor.invalidate_background_cache();
        }
        let video = settings
            .video_edit(self.source_duration)
            .map_err(EditorSessionError::from)?;
        let previous = self
            .settings
            .video_edit(self.source_duration)
            .map_err(EditorSessionError::from)?;
        let speed_changed = previous.clips.iter().any(|old| {
            video
                .clips
                .iter()
                .any(|clip| old.id == clip.id && old.speed_percent != clip.speed_percent)
        });
        let tick = self.controller.snapshot().project_tick;
        let duration = video.duration();
        // Keep the same source image under the playhead when speed changes, including undo.
        let anchored_tick = if speed_changed {
            if tick == previous.duration() {
                Some(duration)
            } else {
                previous
                    .source_time(tick)
                    .and_then(|source| video.project_time(source))
                    .ok()
            }
        } else {
            None
        };
        self.controller.set_duration(duration)?;
        if let Some(tick) = anchored_tick {
            self.controller.seek(tick)?;
        }
        self.camera = camera;
        self.settings = settings;
        self.background = background;
        Ok(())
    }

    pub fn regenerate_camera(&mut self) -> Result<CameraRegenerationReport, PreviewSessionError> {
        self.pending_render = None;
        let report = CameraTrackRegenerator::regenerate(&self.project_root)?;
        let (camera, settings) = load_effective_edit_state(&self.project_root)?;
        let background = load_effective_background(&self.project_root, &settings)?;
        self.background_decodes += u64::from(background.is_some());
        self.compositor.invalidate_background_cache();
        camera.validate().map_err(RenderEvaluationError::from)?;
        self.camera = camera;
        self.settings = settings;
        self.background = background;
        Ok(report)
    }

    pub const fn interactive_render_pending(&self) -> bool {
        self.pending_render.is_some()
    }

    pub fn cancel_interactive_render(&mut self) {
        self.pending_render = None;
        self.decoder.cancel();
    }

    pub fn scrub_decode_plan(&self) -> Result<ScrubDecodePlan, PreviewSessionError> {
        let snapshot = self.controller.snapshot();
        let evaluation = FrameEvaluator::new(&self.source_timeline, &self.camera, &self.cursor)?
            .evaluate_edit(snapshot.project_tick, self.output, &self.settings)?;
        let evaluation = self.preview_evaluation(evaluation);
        let target_index = evaluation.source_frame.frame_index;
        let target_pts = *self
            .source_timeline
            .presentation_ticks()
            .get(target_index)
            .ok_or(PreviewSessionError::InvalidFrameIndex(target_index))?;
        let fallback_index = seek_anchor_index(target_index, INITIAL_RANDOM_SEEK_PREROLL_FRAMES);
        Ok(ScrubDecodePlan {
            project_tick: snapshot.project_tick,
            target_pts,
            fallback_pts: (fallback_index != target_index)
                .then(|| self.source_timeline.presentation_ticks()[fallback_index]),
        })
    }

    fn project_time_for_frame(&self, pts: TimeTick, desired: TimeTick) -> TimeTick {
        self.settings
            .video
            .as_ref()
            .map_or(pts, |video| video.project_time(pts).unwrap_or(desired))
            .min(self.controller.snapshot().duration_tick)
    }

    pub fn prepare_external_scrub_frame(
        &mut self,
        frame: Arc<DecodedBgraFrame>,
    ) -> Result<PreparedPreviewFrame, PreviewSessionError> {
        self.cancel_interactive_render();
        let mut snapshot = self.controller.snapshot();
        // Approximate video, camera and cursor share the displayed frame's time. The
        // playhead remains at the desired pointer position in the controller.
        snapshot.project_tick = self.project_time_for_frame(frame.pts, snapshot.project_tick);
        let evaluation = FrameEvaluator::new(&self.source_timeline, &self.camera, &self.cursor)?
            .evaluate_edit(snapshot.project_tick, self.output, &self.settings)?;
        let mut evaluation = self.preview_evaluation(evaluation);
        let decoded_index = self.source_timeline.select(frame.pts).frame_index;
        evaluation.source_frame = SourceFrameSelection {
            frame_index: decoded_index,
            presentation_tick: frame.pts,
        };
        self.decoded_frame = Some(frame);
        self.decoded_frame_native = false;
        self.prepare_preview(snapshot, evaluation)
    }

    pub fn begin_interactive_render(
        &mut self,
    ) -> Result<Option<PreviewFrame>, PreviewSessionError> {
        let prepared = self.begin_interactive_prepare()?;
        prepared
            .map(|prepared| self.compose_prepared(&prepared))
            .transpose()
    }

    pub fn begin_scrub_render(&mut self) -> Result<Option<PreviewFrame>, PreviewSessionError> {
        let prepared = self.begin_scrub_prepare()?;
        prepared
            .map(|prepared| self.compose_prepared(&prepared))
            .transpose()
    }

    pub fn begin_interactive_prepare(
        &mut self,
    ) -> Result<Option<PreparedPreviewFrame>, PreviewSessionError> {
        self.begin_pending_prepare(PendingPreviewMode::Exact)
    }

    pub fn begin_scrub_prepare(
        &mut self,
    ) -> Result<Option<PreparedPreviewFrame>, PreviewSessionError> {
        self.begin_pending_prepare(PendingPreviewMode::Scrub)
    }

    fn begin_pending_prepare(
        &mut self,
        mode: PendingPreviewMode,
    ) -> Result<Option<PreparedPreviewFrame>, PreviewSessionError> {
        let snapshot = self.controller.snapshot();
        let evaluation = FrameEvaluator::new(&self.source_timeline, &self.camera, &self.cursor)?
            .evaluate_edit(snapshot.project_tick, self.output, &self.settings)?;
        let evaluation = self.preview_evaluation(evaluation);
        let target_index = evaluation.source_frame.frame_index;
        let target_pts = *self
            .source_timeline
            .presentation_ticks()
            .get(target_index)
            .ok_or(PreviewSessionError::InvalidFrameIndex(target_index))?;
        if self.decoded_frame_native
            && self
                .decoded_frame
                .as_ref()
                .is_some_and(|frame| frame.pts == target_pts)
        {
            self.cancel_interactive_render();
            return self.prepare_preview(snapshot, evaluation).map(Some);
        }
        let generation = self
            .decoder
            .submit(self.decode_plan(snapshot.project_tick, target_index, mode))
            .map_err(PreviewSessionError::Worker)?;
        self.pending_render = Some(PendingPreviewRender {
            snapshot,
            evaluation,
            target_index,
            target_pts,
            generation,
            started: Instant::now(),
            mode,
        });
        Ok(None)
    }

    pub fn poll_interactive_render(
        &mut self,
        frame_budget: usize,
    ) -> Result<Option<PreviewFrame>, PreviewSessionError> {
        let prepared = self.poll_interactive_prepare(frame_budget)?;
        prepared
            .map(|prepared| self.compose_prepared(&prepared))
            .transpose()
    }

    /// Non-blocking: MF seek/read/convert and preroll run on the media owner thread.
    /// The budget argument is retained for CLI compatibility; no decoding runs here.
    pub fn poll_interactive_prepare(
        &mut self,
        _frame_budget: usize,
    ) -> Result<Option<PreparedPreviewFrame>, PreviewSessionError> {
        let Some(mut pending) = self.pending_render.take() else {
            return Ok(None);
        };
        let result = self
            .decoder
            .take_latest()
            .filter(|result| result.generation == pending.generation);
        let Some(result) = result else {
            if pending.started.elapsed() > Duration::from_secs(5) {
                self.decoder.cancel();
                return Err(PreviewSessionError::Worker(
                    "preview decode timed out (5 seconds)".into(),
                ));
            }
            self.pending_render = Some(pending);
            return Ok(None);
        };
        let frame = result.frame.map_err(PreviewSessionError::Worker)?;
        if pending.mode == PendingPreviewMode::Exact && frame.pts != pending.target_pts {
            return Err(PreviewSessionError::SelectionMismatch {
                evaluator_tick: pending.target_pts,
                decoded_tick: frame.pts,
            });
        }
        if pending.mode == PendingPreviewMode::Scrub {
            pending.snapshot.project_tick =
                self.project_time_for_frame(frame.pts, pending.snapshot.project_tick);
            pending.evaluation =
                FrameEvaluator::new(&self.source_timeline, &self.camera, &self.cursor)?
                    .evaluate_edit(pending.snapshot.project_tick, self.output, &self.settings)?;
            pending.evaluation = self.preview_evaluation(pending.evaluation);
        }
        pending.evaluation.source_frame = SourceFrameSelection {
            frame_index: if pending.mode == PendingPreviewMode::Exact {
                pending.target_index
            } else {
                self.source_timeline.select(frame.pts).frame_index
            },
            presentation_tick: frame.pts,
        };
        self.decoded_frame = Some(frame);
        self.decoded_frame_native = true;
        self.prepare_preview(pending.snapshot, pending.evaluation)
            .map(Some)
    }

    fn decode_plan(
        &self,
        project_tick: TimeTick,
        target_index: usize,
        mode: PendingPreviewMode,
    ) -> DecodePlan {
        build_decode_plan(
            &self.source_timeline,
            self.controller.snapshot(),
            project_tick,
            target_index,
            mode,
        )
    }

    pub fn render_current(&mut self) -> Result<PreviewFrame, PreviewSessionError> {
        let prepared = self.prepare_current()?;
        self.compose_prepared(&prepared)
    }

    pub fn prepare_current(&mut self) -> Result<PreparedPreviewFrame, PreviewSessionError> {
        self.cancel_interactive_render();
        let snapshot = self.controller.snapshot();
        let evaluation = FrameEvaluator::new(&self.source_timeline, &self.camera, &self.cursor)?
            .evaluate_edit(snapshot.project_tick, self.output, &self.settings)?;
        let evaluation = self.preview_evaluation(evaluation);
        self.decode_selected(evaluation.source_frame.frame_index)?;
        self.prepare_preview(snapshot, evaluation)
    }

    fn prepare_preview(
        &self,
        snapshot: PreviewSnapshot,
        evaluation: FrameEvaluation,
    ) -> Result<PreparedPreviewFrame, PreviewSessionError> {
        let decoded = self
            .decoded_frame
            .as_ref()
            .ok_or(PreviewSessionError::NoSourceFrame)?;
        if decoded.pts != evaluation.source_frame.presentation_tick {
            return Err(PreviewSessionError::SelectionMismatch {
                evaluator_tick: evaluation.source_frame.presentation_tick,
                decoded_tick: decoded.pts,
            });
        }
        Ok(PreparedPreviewFrame {
            snapshot,
            evaluation,
            decoded: Arc::clone(decoded),
        })
    }

    pub fn compose_prepared(
        &self,
        prepared: &PreparedPreviewFrame,
    ) -> Result<PreviewFrame, PreviewSessionError> {
        let decoded = &prepared.decoded;
        if decoded.pts != prepared.evaluation.source_frame.presentation_tick {
            return Err(PreviewSessionError::SelectionMismatch {
                evaluator_tick: prepared.evaluation.source_frame.presentation_tick,
                decoded_tick: decoded.pts,
            });
        }
        let composited = self.compositor.compose_with_background(
            decoded,
            &prepared.evaluation,
            &self.settings,
            self.background.as_ref(),
        )?;
        Ok(PreviewFrame {
            snapshot: prepared.snapshot,
            evaluation: prepared.evaluation,
            composited,
        })
    }

    pub fn present_prepared(
        &self,
        presenter: &GpuPreviewPresenter,
        prepared: &PreparedPreviewFrame,
        focus_overlay: Option<[f64; 2]>,
    ) -> Result<(), PreviewSessionError> {
        let decoded = &prepared.decoded;
        if decoded.pts != prepared.evaluation.source_frame.presentation_tick {
            return Err(PreviewSessionError::SelectionMismatch {
                evaluator_tick: prepared.evaluation.source_frame.presentation_tick,
                decoded_tick: decoded.pts,
            });
        }
        self.compositor.present_with_background(
            presenter,
            decoded,
            &prepared.evaluation,
            &self.settings,
            self.background.as_ref(),
            focus_overlay,
        )?;
        Ok(())
    }

    fn decode_selected(&mut self, target_index: usize) -> Result<(), PreviewSessionError> {
        let target_pts = *self
            .source_timeline
            .presentation_ticks()
            .get(target_index)
            .ok_or(PreviewSessionError::InvalidFrameIndex(target_index))?;
        if self.decoded_frame_native
            && self
                .decoded_frame
                .as_ref()
                .is_some_and(|frame| frame.pts == target_pts)
        {
            return Ok(());
        }
        self.decoded_frame = Some(decode_exact_frame(
            &self.decoder,
            &self.source_timeline,
            self.controller.snapshot(),
            target_index,
        )?);
        self.decoded_frame_native = true;
        Ok(())
    }
}

fn decode_exact_frame(
    decoder: &DecodeWorker,
    timeline: &SourceFrameTimeline,
    snapshot: PreviewSnapshot,
    target_index: usize,
) -> Result<Arc<DecodedBgraFrame>, PreviewSessionError> {
    if timeline.presentation_ticks().get(target_index).is_none() {
        return Err(PreviewSessionError::InvalidFrameIndex(target_index));
    }
    let generation = decoder
        .submit(build_decode_plan(
            timeline,
            snapshot,
            snapshot.project_tick,
            target_index,
            PendingPreviewMode::Exact,
        ))
        .map_err(PreviewSessionError::Worker)?;
    decoder
        .wait(generation)
        .map_err(PreviewSessionError::Worker)
}

fn build_decode_plan(
    timeline: &SourceFrameTimeline,
    snapshot: PreviewSnapshot,
    project_tick: TimeTick,
    target_index: usize,
    mode: PendingPreviewMode,
) -> DecodePlan {
    let ticks = timeline.presentation_ticks();
    let mut fallback_pts = Vec::new();
    let mut preroll = 0;
    while preroll < target_index {
        preroll = next_seek_preroll(target_index, preroll);
        fallback_pts.push(ticks[seek_anchor_index(target_index, preroll)]);
        if mode == PendingPreviewMode::Scrub {
            break;
        }
    }
    DecodePlan {
        project_tick,
        target_pts: ticks[target_index],
        fallback_pts,
        exact: mode == PendingPreviewMode::Exact,
        prefetch_until: (snapshot.playback == panzo_core::PlaybackState::Playing)
            .then(|| ticks[(target_index + 2).min(ticks.len() - 1)]),
    }
}

const fn seek_anchor_index(target_index: usize, preroll_frames: usize) -> usize {
    target_index.saturating_sub(preroll_frames)
}

fn next_seek_preroll(target_index: usize, current_preroll: usize) -> usize {
    if current_preroll == 0 {
        INITIAL_RANDOM_SEEK_PREROLL_FRAMES.min(target_index)
    } else {
        current_preroll.saturating_mul(4).min(target_index)
    }
}

fn check_preparation_cancelled(cancelled: &impl Fn() -> bool) -> Result<(), PreviewSessionError> {
    if cancelled() {
        Err(PreviewSessionError::PreparationCancelled)
    } else {
        Ok(())
    }
}

fn read_source_timeline(
    media_path: &Path,
    cancelled: &impl Fn() -> bool,
) -> Result<SourceFrameTimeline, PreviewSessionError> {
    check_preparation_cancelled(cancelled)?;
    let ticks = MfBgraDecoder::native_index(media_path, cancelled)?
        .into_iter()
        .map(|frame| frame.pts)
        .collect();
    SourceFrameTimeline::new(ticks).map_err(PreviewSessionError::Evaluation)
}

fn read_cursor_events(
    path: &Path,
    cancelled: &impl Fn() -> bool,
) -> Result<CursorTimeline, PreviewSessionError> {
    let file = fs::File::open(path).map_err(|source| PreviewSessionError::Read {
        path: path.into(),
        source,
    })?;
    let mut events = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        check_preparation_cancelled(cancelled)?;
        let line = line.map_err(|source| PreviewSessionError::Read {
            path: path.into(),
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }
        events.push(
            serde_json::from_str::<CursorEvent>(&line).map_err(|source| {
                PreviewSessionError::InvalidCursorRecord {
                    path: path.into(),
                    line: index + 1,
                    source,
                }
            })?,
        );
    }
    CursorTimeline::new(events).map_err(PreviewSessionError::Evaluation)
}

#[derive(Debug, Error)]
pub enum PreviewSessionError {
    #[error("preview preparation cancelled")]
    PreparationCancelled,
    #[error("preview media service: {0}")]
    Worker(String),
    #[error(transparent)]
    Project(#[from] ProjectIoError),
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid cursor JSONL record {path}:{line}: {source}")]
    InvalidCursorRecord {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error(transparent)]
    Controller(#[from] PreviewControllerError),
    #[error(transparent)]
    Decoder(#[from] MediaDecoderError),
    #[error(transparent)]
    Evaluation(#[from] RenderEvaluationError),
    #[error(transparent)]
    Compositor(#[from] CompositorError),
    #[error(transparent)]
    CameraRegeneration(#[from] CameraRegenerationError),
    #[error(transparent)]
    Editor(#[from] EditorSessionError),
    #[error(transparent)]
    WorkbenchSettings(#[from] WorkbenchValidationError),
    #[error("source video has no decoded frame")]
    NoSourceFrame,
    #[error("preview selected source frame index {0}, which is outside the source timeline")]
    InvalidFrameIndex(usize),
    #[error("source ended before required frame index {required_index}")]
    SourceEndedEarly { required_index: usize },
    #[error("evaluator selected {evaluator_tick:?}, decoder returned {decoded_tick:?}")]
    SelectionMismatch {
        evaluator_tick: TimeTick,
        decoded_tick: TimeTick,
    },
}

fn effective_camera_tick(
    settings: &WorkbenchSettings,
    duration: TimeTick,
    project: TimeTick,
) -> Option<TimeTick> {
    if !settings.camera_enabled {
        return None;
    }
    settings
        .video_edit(duration)
        .ok()?
        .source_time(project)
        .ok()
}

fn next_video_delay(
    video: &panzo_core::VideoEdit,
    timeline: &SourceFrameTimeline,
    project: TimeTick,
) -> Option<Duration> {
    let span = video.clip_at(project)?;
    let source = video.source_time(project).ok()?;
    let ticks = timeline.presentation_ticks();
    let index = ticks.partition_point(|pts| *pts <= source);
    let delta = ticks
        .get(index)
        .filter(|pts| **pts < span.clip.source_out_tick)
        .map_or(span.project_out.0 - project.0, |pts| {
            span.project_in.0
                + span
                    .clip
                    .project_offset(TimeTick(pts.0 - span.clip.source_in_tick.0))
                    .0
                - project.0
        });
    Some(
        Duration::from_nanos(delta.clamp(0, 500_000).cast_unsigned() * 100)
            + Duration::from_micros(250),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires PANZO_LOAD_PROJECT with a short readable Windows media fixture; no GPU"]
    fn background_load_prepares_trimmed_first_frame_without_gpu() {
        let project = PathBuf::from(std::env::var_os("PANZO_LOAD_PROJECT").unwrap());
        let prepared = std::thread::spawn(move || {
            let editor = crate::platform::editor::ProjectEditorSession::open(&project).unwrap();
            let output = ProjectPreviewSession::native_output_descriptor(&project).unwrap();
            let mut settings = editor.settings().clone();
            let duration = editor.project_duration();
            let mut video = panzo_core::VideoEdit::full(duration).unwrap();
            video
                .trim("source-0", TimeTick(duration.0 / 2), duration)
                .unwrap();
            settings.video = Some(video);
            settings.schema_version = panzo_core::WORKBENCH_SCHEMA_VERSION;
            PreparedPreviewSession::prepare_editor_cancellable(
                &project,
                output,
                editor.camera().clone(),
                settings,
                &AtomicBool::new(false),
            )
            .unwrap()
        })
        .join()
        .unwrap();
        let snapshot = prepared.controller.snapshot();
        let evaluation = FrameEvaluator::new(
            &prepared.source_timeline,
            &prepared.camera,
            &prepared.cursor,
        )
        .unwrap()
        .evaluate_edit(snapshot.project_tick, prepared.output, &prepared.settings)
        .unwrap();
        let frame = prepared
            .decoded_frame
            .as_ref()
            .expect("worker must decode before returning");
        assert_eq!(snapshot.project_tick, TimeTick(0));
        assert!(
            evaluation.source_frame.frame_index > 0,
            "trim must change the initial source frame"
        );
        assert_eq!(frame.pts, evaluation.source_frame.presentation_tick);
        assert_eq!(
            frame.pixels.len(),
            frame.width as usize * frame.height as usize * 4
        );
    }

    #[test]
    #[ignore = "requires PANZO_UI_REVIEW_PROJECT and a Windows GPU"]
    fn retained_preview_survives_seek_ui_review() {
        let path = std::env::var_os("PANZO_UI_REVIEW_PROJECT").expect("review project");
        let output = OutputDescriptor::bgra8(640, 360).unwrap();
        let mut session = ProjectPreviewSession::open(PathBuf::from(path), output).unwrap();
        let first = session.prepare_current().unwrap();
        let first_pixels = session.compose_prepared(&first).unwrap();
        let duration = session.snapshot().duration_tick.0;
        // A repaint/resize may still refer to the displayed frame while the next
        // seek has already prepared its pixels. Both snapshots must remain valid.
        for fraction in [73, 12, 99, 38, 100, 0, 57, 18] {
            session.seek(TimeTick(duration * fraction / 100)).unwrap();
            let next = session.prepare_current().unwrap();
            session.compose_prepared(&next).unwrap();
            assert_eq!(
                session.compose_prepared(&first).unwrap().composited,
                first_pixels.composited,
                "retained frame changed after seeking to {fraction}%"
            );
        }
        for fraction in [98, 3, 85, 100, 0] {
            let target = TimeTick(duration * fraction / 100);
            session.seek(target).unwrap();
            let next = if let Some(frame) = session.begin_interactive_prepare().unwrap() {
                frame
            } else {
                loop {
                    if let Some(frame) = session.poll_interactive_prepare(1).unwrap() {
                        break frame;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            };
            assert_eq!(next.snapshot.project_tick, target);
            session.compose_prepared(&next).unwrap();
            let scrub = session
                .prepare_external_scrub_frame(Arc::clone(&next.decoded))
                .unwrap();
            session.compose_prepared(&scrub).unwrap();
            assert_eq!(
                session.compose_prepared(&first).unwrap().composited,
                first_pixels.composited
            );
        }
    }

    #[test]
    fn camera_diagnostics_follow_source_mapping_and_effect_bypass() {
        let mut settings = WorkbenchSettings::default();
        let mut video = panzo_core::VideoEdit::full(TimeTick(100)).unwrap();
        video.split(TimeTick(20), "middle".into()).unwrap();
        video.split(TimeTick(60), "tail".into()).unwrap();
        video.delete("middle").unwrap();
        settings.video = Some(video);
        assert_eq!(
            effective_camera_tick(&settings, TimeTick(100), TimeTick(25)),
            Some(TimeTick(65))
        );
        assert_eq!(
            effective_camera_tick(&settings, TimeTick(100), TimeTick(60)),
            Some(TimeTick(99))
        );
        settings.camera_enabled = false;
        assert_eq!(
            effective_camera_tick(&settings, TimeTick(100), TimeTick(25)),
            None
        );
    }

    #[test]
    fn playback_deadlines_scale_with_clip_speed() {
        let timeline = SourceFrameTimeline::new(
            [0, 20, 40, 50, 70, 90, 100]
                .map(TimeTick::from_millis)
                .to_vec(),
        )
        .unwrap();
        for (speed, at_ms, expected_us) in [
            (25, 80, 50_250),
            (50, 40, 40_250),
            (200, 10, 10_250),
            (400, 5, 5_250),
        ] {
            let mut video = panzo_core::VideoEdit::full(TimeTick::from_millis(120)).unwrap();
            video.set_speed_percent("source-0", speed).unwrap();
            assert_eq!(
                next_video_delay(&video, &timeline, TimeTick::from_millis(at_ms)),
                Some(Duration::from_micros(expected_us))
            );
        }
    }

    #[test]
    fn playback_deadline_uses_vfr_pts_and_wakes_at_deleted_cut() {
        let ticks = [0, 20, 40, 50, 70, 90, 100].map(TimeTick::from_millis);
        let timeline = SourceFrameTimeline::new(ticks.to_vec()).unwrap();
        let mut video = panzo_core::VideoEdit::full(TimeTick::from_millis(120)).unwrap();
        assert_eq!(
            next_video_delay(&video, &timeline, TimeTick::from_millis(20)),
            Some(Duration::from_micros(20_250))
        );
        assert_eq!(
            next_video_delay(&video, &timeline, TimeTick::from_millis(40)),
            Some(Duration::from_micros(10_250))
        );
        video
            .split(TimeTick::from_millis(45), "middle".into())
            .unwrap();
        video
            .split(TimeTick::from_millis(90), "tail".into())
            .unwrap();
        video.delete("middle").unwrap();
        // The next frame at source 50 ms is deleted. Wake at the project cut at
        // 45 ms, not at deleted PTS and not at its old source position of 90 ms.
        assert_eq!(
            next_video_delay(&video, &timeline, TimeTick::from_millis(40)),
            Some(Duration::from_micros(5_250))
        );
        assert_eq!(
            next_video_delay(&video, &timeline, TimeTick::from_millis(45)),
            Some(Duration::from_micros(10_250))
        );
        assert_eq!(next_video_delay(&video, &timeline, video.duration()), None);
    }

    #[test]
    fn random_seek_starts_from_bounded_preroll() {
        assert_eq!(
            next_seek_preroll(142, 0),
            INITIAL_RANDOM_SEEK_PREROLL_FRAMES
        );
        assert_eq!(
            seek_anchor_index(142, INITIAL_RANDOM_SEEK_PREROLL_FRAMES),
            112
        );
        assert_eq!(next_seek_preroll(60, 0), 30);
        assert_eq!(seek_anchor_index(60, 30), 30);
        assert_eq!(next_seek_preroll(20, 0), 20);
        assert_eq!(seek_anchor_index(20, 20), 0);
    }

    #[test]
    fn expanded_preroll_can_reach_stream_start() {
        let target_index = 1_000;
        let first = next_seek_preroll(target_index, 0);
        let second = next_seek_preroll(target_index, first);
        let third = next_seek_preroll(target_index, second);
        let fourth = next_seek_preroll(target_index, third);
        assert_eq!(seek_anchor_index(target_index, first), 970);
        assert_eq!(seek_anchor_index(target_index, second), 880);
        assert_eq!(seek_anchor_index(target_index, third), 520);
        assert_eq!(seek_anchor_index(target_index, fourth), 0);
    }
}
