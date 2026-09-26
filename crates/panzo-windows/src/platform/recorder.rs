use crate::platform::apartment::WinRtApartment;
use crate::platform::capture_probe::{CaptureProbeError, primary_monitor, primary_monitor_item};
use crate::platform::d3d11::D3d11Device;
use crate::platform::editor_transaction::lock_project_edit;
use crate::platform::fmp4_writer::{Fmp4ProbeError, Nv12Fmp4Writer, Nv12VideoConfig};
use crate::platform::input_capture::{InputCaptureError, InputCaptureService};
use crate::platform::manifest_store::{
    ManifestStore, ManifestStoreError, move_file_new_atomically, persist_bytes_new_atomically,
    persist_json_atomically,
};
use crate::platform::preflight::{PreflightError, PreflightReport};
use crate::platform::qpc::{QpcSource, QpcSourceError};
use crate::platform::video_processor::{Nv12VideoProcessor, VideoProcessorProbeError};
use crate::platform::window_capture::{
    WindowCaptureError, WindowCaptureTarget, window_capture_item,
};
use crossbeam_queue::ArrayQueue;
use panzo_core::diagnostics::ProjectMetrics;
use panzo_core::project::{CaptureKind, TimebaseDescriptor};
use panzo_core::time::TimeError;
use panzo_core::{
    CameraPlanner, CameraSegmentKind, CaptureDescriptor, InputEventNormalizer,
    InputNormalizationError, JournalOperation, JournalResult, PersistedInputEvent, PhysicalRect,
    PhysicalSize, PlannerInput, PlannerParams, ProjectIoError, ProjectLayout, ProjectManifest,
    ProjectState, ProjectWriter, ProjectWriterError, RawInputKind, RecordingLock, SessionClock,
    TimeTick, VideoFrameRateDecision, VideoFrameRateLimiter, VideoTimestampDecision,
    VideoTimestampNormalizer,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;
use uuid::Uuid;
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{CloseHandle, HANDLE, SYSTEMTIME};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX,
    D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Device1,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    DXGI_SHARED_RESOURCE_READ, DXGI_SHARED_RESOURCE_WRITE, IDXGIKeyedMutex, IDXGIResource1,
};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::SystemInformation::GetSystemTime;
use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
use windows::core::{IInspectable, Interface, PCWSTR};

const CAPTURE_QUEUE_CAPACITY: usize = 4;
const CAPTURE_TEXTURE_COUNT: usize = CAPTURE_QUEUE_CAPACITY + 1;
const MINIMUM_FREE_BYTES: u64 = 2 * 1_024 * 1_024 * 1_024;
const DEFAULT_LAST_FRAME_DURATION_TICK: i64 = 166_667;
const TARGET_CAPTURE_FPS: u32 = 60;
const CAPTURE_MUTEX_KEY: u64 = 0;
const PROCESSING_MUTEX_KEY: u64 = 1;
const SHARED_TEXTURE_WAIT_MS: u32 = 2_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingReport {
    pub project_root: PathBuf,
    pub duration_tick: i64,
    pub frames_received: u64,
    pub frames_encoded: u64,
    pub dropped_by_backpressure: u64,
    pub dropped_by_frame_rate_limit: u64,
    pub duplicate_timestamp: u64,
    pub frame_gap_p99_tick: i64,
    pub max_frame_gap_tick: i64,
    pub cursor_events_persisted: u64,
    pub click_events: u64,
    pub fmp4_fragments: usize,
    pub encoder_name: String,
    pub hardware_encoder: bool,
}

struct SessionLog {
    path: PathBuf,
    file: File,
    started: Instant,
}

impl SessionLog {
    fn create(path: PathBuf) -> Result<Self, RecorderError> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| RecorderError::WriteFile {
                path: path.clone(),
                source,
            })?;
        Ok(Self {
            path,
            file,
            started: Instant::now(),
        })
    }

    fn stage(&mut self, stage: &str) -> Result<(), RecorderError> {
        self.stage_with_detail(stage, "")
    }

    fn stage_with_detail(&mut self, stage: &str, detail: &str) -> Result<(), RecorderError> {
        let elapsed_ms = self.started.elapsed().as_millis();
        let write_result = if detail.is_empty() {
            eprintln!("Panzo recorder [{elapsed_ms} ms]: {stage}");
            writeln!(self.file, "elapsedMs={elapsed_ms} stage={stage}")
        } else {
            eprintln!("Panzo recorder [{elapsed_ms} ms]: {stage} ({detail})");
            writeln!(self.file, "elapsedMs={elapsed_ms} stage={stage} {detail}")
        };
        write_result
            .and_then(|()| self.file.flush())
            .map_err(|source| RecorderError::WriteFile {
                path: self.path.clone(),
                source,
            })
    }

    fn trace_with_detail(&mut self, stage: &str, detail: &str) -> Result<(), RecorderError> {
        let elapsed_ms = self.started.elapsed().as_millis();
        writeln!(self.file, "elapsedMs={elapsed_ms} stage={stage} {detail}").map_err(|source| {
            RecorderError::WriteFile {
                path: self.path.clone(),
                source,
            }
        })
    }
}

pub struct PrimaryMonitorRecorder;

pub struct CaptureRecorder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingSource {
    PrimaryMonitor,
    Window(WindowCaptureTarget),
}

impl RecordingSource {
    pub fn label(&self) -> String {
        match self {
            Self::PrimaryMonitor => "主显示器".into(),
            Self::Window(target) => format!("窗口：{}", target.title),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RecordingControl {
    stop_requested: Arc<AtomicBool>,
    recording_started: Arc<AtomicBool>,
}

impl RecordingControl {
    pub fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::Release);
    }

    pub fn stop_requested(&self) -> bool {
        self.stop_requested.load(Ordering::Acquire)
    }

    pub fn recording_started(&self) -> bool {
        self.recording_started.load(Ordering::Acquire)
    }

    fn mark_recording_started(&self) {
        self.recording_started.store(true, Ordering::Release);
    }
}

impl PrimaryMonitorRecorder {
    pub fn record(
        project_root: impl AsRef<Path>,
        duration: Duration,
    ) -> Result<RecordingReport, RecorderError> {
        CaptureRecorder::record(project_root, duration, &RecordingSource::PrimaryMonitor)
    }

    #[allow(clippy::too_many_lines)]
    pub fn record_with_control(
        project_root: impl AsRef<Path>,
        maximum_duration: Duration,
        control: &RecordingControl,
    ) -> Result<RecordingReport, RecorderError> {
        CaptureRecorder::record_with_control(
            project_root,
            maximum_duration,
            control,
            &RecordingSource::PrimaryMonitor,
        )
    }
}

impl CaptureRecorder {
    pub fn record(
        project_root: impl AsRef<Path>,
        duration: Duration,
        source: &RecordingSource,
    ) -> Result<RecordingReport, RecorderError> {
        Self::record_with_control(project_root, duration, &RecordingControl::default(), source)
    }

    #[allow(clippy::too_many_lines)]
    pub fn record_with_control(
        project_root: impl AsRef<Path>,
        maximum_duration: Duration,
        control: &RecordingControl,
        source: &RecordingSource,
    ) -> Result<RecordingReport, RecorderError> {
        if maximum_duration < Duration::from_millis(250) {
            return Err(RecorderError::InvalidDuration);
        }
        let project_root = project_root.as_ref();
        preflight_destination(project_root)?;
        let preflight = PreflightReport::collect()?;
        if !preflight.is_supported() {
            return Err(RecorderError::UnsupportedPreflight(format!("{preflight}")));
        }

        eprintln!("Panzo recorder: platform preflight passed");
        let _apartment = WinRtApartment::multi_threaded().map_err(RecorderError::Windows)?;
        let qpc = QpcSource::new()?;
        let capture_d3d = D3d11Device::create_hardware().map_err(RecorderError::Windows)?;
        let processing_d3d = D3d11Device::create_hardware().map_err(RecorderError::Windows)?;
        let prepared = prepare_capture_source(source)?;
        let item = prepared.item;
        let size = prepared.capture_size;
        let capture_width =
            u32::try_from(size.Width).map_err(|_| RecorderError::InvalidCaptureSize)?;
        let capture_height =
            u32::try_from(size.Height).map_err(|_| RecorderError::InvalidCaptureSize)?;
        if capture_width < 2 || capture_height < 2 {
            return Err(RecorderError::InvalidCaptureSize);
        }
        let width = capture_width & !1;
        let height = capture_height & !1;
        let processor = Nv12VideoProcessor::create_with_output(
            &processing_d3d,
            capture_width,
            capture_height,
            width,
            height,
        )?;
        let shared = Arc::new(CaptureShared::new(
            &capture_d3d,
            &processing_d3d,
            capture_width,
            capture_height,
        )?);

        let desktop_rect = PhysicalRect {
            x: prepared.desktop_rect.x,
            y: prepared.desktop_rect.y,
            width,
            height,
        };
        let project_id = Uuid::new_v4();
        let mut manifest = ProjectManifest::new_v01(
            project_id,
            env!("CARGO_PKG_VERSION"),
            utc_now(),
            TimebaseDescriptor {
                ticks_per_second: panzo_core::time::TICKS_PER_SECOND,
                session_start_qpc: 0,
                qpc_frequency: qpc.frequency(),
            },
            CaptureDescriptor {
                kind: prepared.kind,
                monitor_id: prepared.monitor_id,
                window_id: prepared.window_id,
                window_title: prepared.window_title,
                desktop_rect_px: desktop_rect,
                content_size_px: PhysicalSize {
                    width: desktop_rect.width,
                    height: desktop_rect.height,
                },
                target_fps: 60,
                pixel_format: "BGRA8".into(),
                color_mode: "sdr-srgb".into(),
                cursor_captured_in_video: false,
            },
        );
        let mut recording_lock = RecordingLock {
            schema_version: 1,
            process_id: std::process::id(),
            session_id: project_id.to_string(),
            last_checkpoint_tick: TimeTick::ZERO,
        };
        let layout = ProjectLayout::create_new(project_root, &manifest, &recording_lock)?;
        // Shared with recovery and editor saves; held through the final durable commit.
        let _project_lock = lock_project_edit(project_root)?;
        let store = ManifestStore::new(project_root);
        let mut project_writer = ProjectWriter::create(&layout)?;
        let mut session_log = SessionLog::create(layout.session_log_path())?;
        session_log.stage("project-created")?;
        project_writer.append_journal(
            TimeTick::ZERO,
            JournalOperation::ProjectCreated,
            JournalResult::Success,
            None,
        )?;
        project_writer.flush()?;

        // Initialize the hardware encoder before WGC starts producing frames. Some drivers can
        // deadlock when encoder activation and an active capture session compete for the GPU.
        session_log.stage("media-writer-create-begin")?;
        let part_media_path = project_root.join("media/screen.part.mp4");
        let encoding = Nv12VideoConfig::native_screen_60(width, height)?;
        let mut media_writer = Nv12Fmp4Writer::create_with_config(&part_media_path, encoding)?;
        session_log.stage_with_detail(
            "media-writer-create-complete",
            &format!(
                "{} {}x{} {}bps",
                media_writer.encoder_name(),
                encoding.width,
                encoding.height,
                encoding.target_bitrate
            ),
        )?;
        if !media_writer.hardware_encoder() {
            return Err(RecorderError::HardwareEncoderRequired);
        }

        session_log.stage("input-capture-start-begin")?;
        let mut input_service = InputCaptureService::start()?;
        session_log.stage("input-capture-start-complete")?;
        session_log.stage("wgc-start-begin")?;
        let mut capture = RecordingCapture::start(&capture_d3d, &item, size, Arc::clone(&shared))?;
        session_log.stage("wgc-start-complete")?;
        session_log.stage("first-frame-wait-begin")?;
        let first_task = wait_for_first_frame(&shared, Duration::from_secs(5))?;
        let clock = SessionClock::establish(qpc.frequency(), first_task.system_relative_tick)?;
        manifest.timebase.session_start_qpc = clock.session_start_qpc;
        store.persist(&manifest)?;
        project_writer.append_journal(
            TimeTick::ZERO,
            JournalOperation::EpochEstablished,
            JournalResult::Success,
            Some(format!("sessionStartQpc={}", clock.session_start_qpc)),
        )?;
        project_writer.flush()?;
        session_log.stage("epoch-established")?;
        let mut diagnostic_writer = CaptureDiagnosticWriter::create(layout.capture_frames_path())?;

        let mut metrics = ProjectMetrics::default();
        let mut input_normalizer = InputEventNormalizer::new(clock, desktop_rect, 0)?;
        let mut timestamp_normalizer = VideoTimestampNormalizer::default();
        let mut frame_rate_limiter = VideoFrameRateLimiter::new(TARGET_CAPTURE_FPS)?;
        let mut capture_gap_tracker = CaptureGapTracker::new(TimeTick::ZERO);
        let mut pending_frame = Some(PendingFrame {
            task: first_task,
            tick: TimeTick::ZERO,
        });
        timestamp_normalizer.observe(TimeTick::ZERO)?;
        frame_rate_limiter.observe(TimeTick::ZERO)?;
        let mut raw_input_buffer = Vec::new();
        let mut click_events = Vec::new();
        let target_tick = duration_to_tick(maximum_duration)?;
        let started = Instant::now();
        let mut last_heartbeat = Instant::now();
        let mut next_geometry_refresh = Instant::now();
        let mut first_frame_encoded = false;
        let mut stop_tick;
        let stopped_by_request;
        session_log.stage("recording-loop-entered")?;
        control.mark_recording_started();

        loop {
            if shared.callback_failed.load(Ordering::Acquire) {
                return Err(RecorderError::CaptureCallbackFailed);
            }
            if shared.geometry_changed.load(Ordering::Acquire) {
                return Err(RecorderError::CaptureGeometryChanged);
            }
            drain_capture_queue(
                &shared,
                &processor,
                &clock,
                &mut timestamp_normalizer,
                &mut frame_rate_limiter,
                &mut capture_gap_tracker,
                &mut pending_frame,
                &mut media_writer,
                &mut diagnostic_writer,
                &mut metrics,
                &mut session_log,
                &mut first_frame_encoded,
            )?;
            if next_geometry_refresh.elapsed() >= Duration::from_millis(100) {
                if let RecordingSource::Window(target) = &source {
                    let current = target.current_desktop_rect()?;
                    input_normalizer.update_capture_rect(PhysicalRect {
                        x: current.x,
                        y: current.y,
                        width,
                        height,
                    })?;
                }
                next_geometry_refresh = Instant::now();
            }
            drain_input(
                &input_service,
                &mut raw_input_buffer,
                &mut input_normalizer,
                &mut project_writer,
                &mut click_events,
                &mut metrics,
                Some(qpc.now()?.saturating_sub(qpc.frequency() / 20)),
            )?;
            let now_qpc = qpc.now()?;
            let now_tick = clock.recording_tick(now_qpc)?;
            let service = project_writer.service_due(now_tick)?;
            if service.flushed {
                diagnostic_writer.flush()?;
            }
            if service.checkpointed {
                recording_lock.last_checkpoint_tick = now_tick;
                store.persist_recording_lock(&recording_lock)?;
                project_writer.append_journal(
                    now_tick,
                    JournalOperation::Checkpoint,
                    JournalResult::Success,
                    None,
                )?;
            }
            metrics.input.queue_peak = metrics
                .input
                .queue_peak
                .max(u32::try_from(input_service.queued_len()).unwrap_or(u32::MAX));
            if input_service.overflowed() {
                return Err(RecorderError::InputQueueOverflow);
            }
            if last_heartbeat.elapsed() >= Duration::from_secs(1) {
                session_log.stage_with_detail(
                    "recording-heartbeat",
                    &format!(
                        "tick={} framesReceived={} framesEncoded={} frameRateDrops={} captureQueue={}",
                        now_tick.0,
                        shared.frames_received.load(Ordering::Relaxed),
                        metrics.capture.frames_encoded,
                        metrics.capture.dropped_by_frame_rate_limit,
                        shared.queue.len()
                    ),
                )?;
                last_heartbeat = Instant::now();
            }
            if control.stop_requested() {
                stop_tick = now_tick;
                stopped_by_request = true;
                break;
            }
            if now_tick >= target_tick {
                stop_tick = now_tick;
                stopped_by_request = false;
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }

        session_log.stage(if stopped_by_request {
            "recording-stop-requested"
        } else {
            "recording-deadline-reached"
        })?;
        manifest.transition_to(ProjectState::Finalizing)?;
        store.persist(&manifest)?;
        project_writer.append_journal(
            stop_tick,
            JournalOperation::StateTransition,
            JournalResult::Success,
            Some("recording->finalizing".into()),
        )?;
        capture.stop();
        input_service.stop()?;
        session_log.stage("capture-stopped")?;
        drain_capture_queue(
            &shared,
            &processor,
            &clock,
            &mut timestamp_normalizer,
            &mut frame_rate_limiter,
            &mut capture_gap_tracker,
            &mut pending_frame,
            &mut media_writer,
            &mut diagnostic_writer,
            &mut metrics,
            &mut session_log,
            &mut first_frame_encoded,
        )?;
        drain_input(
            &input_service,
            &mut raw_input_buffer,
            &mut input_normalizer,
            &mut project_writer,
            &mut click_events,
            &mut metrics,
            None,
        )?;
        if let Some(pending) = pending_frame.take() {
            let duration = stop_tick
                .0
                .saturating_sub(pending.tick.0)
                .max(DEFAULT_LAST_FRAME_DURATION_TICK);
            encode_pending(
                &shared,
                &processor,
                pending,
                duration,
                &mut media_writer,
                &mut metrics,
                &mut FirstFrameTrace {
                    session_log: &mut session_log,
                    encoded: &mut first_frame_encoded,
                },
            )?;
            stop_tick = TimeTick(stop_tick.0.max(pending.tick.0.saturating_add(duration)));
        }
        diagnostic_writer.checkpoint()?;
        session_log.stage("media-writer-finalize-begin")?;
        let media_report = media_writer.finish()?;
        session_log.stage_with_detail(
            "media-writer-finalize-complete",
            &format!(
                "frames={} fragments={} durationTick={}",
                media_report.frames_written,
                media_report.inspection.fragment_count,
                media_report.inspection.last_video_end_tick
            ),
        )?;
        metrics.capture.frames_received = shared.frames_received.load(Ordering::Acquire);
        metrics.capture.dropped_by_backpressure =
            shared.dropped_by_backpressure.load(Ordering::Acquire);
        metrics.capture.capture_queue_peak =
            u32::try_from(shared.capture_queue_peak.load(Ordering::Acquire)).unwrap_or(u32::MAX);
        capture_gap_tracker.finish(&mut metrics);

        let final_media_path = project_root.join("media/screen.mp4");
        move_file_new_atomically(&part_media_path, &final_media_path)?;
        manifest.media.screen = "media/screen.mp4".into();
        manifest.media.duration_tick = media_report.inspection.last_video_end_tick;
        manifest.transition_to(ProjectState::Processing)?;
        store.persist(&manifest)?;
        project_writer.append_journal(
            stop_tick,
            JournalOperation::FinalizeCompleted,
            JournalResult::Success,
            Some(format!(
                "fragments={}",
                media_report.inspection.fragment_count
            )),
        )?;
        let camera_track = ensure_recording_edit_files(
            &layout,
            &click_events,
            TimeTick(media_report.inspection.last_video_end_tick),
            desktop_rect,
        )?;
        metrics.camera.trigger_clicks = click_events
            .iter()
            .filter(|event| event.triggers_camera())
            .count() as u64;
        metrics.camera.segments = camera_track.segments.len() as u64;
        for segment in &camera_track.segments {
            match segment.kind {
                CameraSegmentKind::ZoomIn => metrics.camera.zoom_in += 1,
                CameraSegmentKind::Pan => metrics.camera.pan += 1,
                CameraSegmentKind::ReframeOut | CameraSegmentKind::ReframeIn => {
                    metrics.camera.reframe += 1;
                }
                CameraSegmentKind::ZoomOut => metrics.camera.zoom_out += 1,
            }
        }
        write_json_durable(layout.metrics_path(), &metrics)?;
        manifest.transition_to(ProjectState::Ready)?;
        store.persist(&manifest)?;
        project_writer.append_journal(
            stop_tick,
            JournalOperation::StateTransition,
            JournalResult::Success,
            Some("processing->ready".into()),
        )?;
        let writer_stats = project_writer.finish()?;
        store.remove_recording_lock()?;
        let _elapsed_ms = started.elapsed().as_millis();
        session_log.stage("project-ready")?;
        Ok(RecordingReport {
            project_root: project_root.into(),
            duration_tick: media_report.inspection.last_video_end_tick,
            frames_received: metrics.capture.frames_received,
            frames_encoded: metrics.capture.frames_encoded,
            dropped_by_backpressure: metrics.capture.dropped_by_backpressure,
            dropped_by_frame_rate_limit: metrics.capture.dropped_by_frame_rate_limit,
            duplicate_timestamp: metrics.capture.duplicate_timestamp,
            frame_gap_p99_tick: metrics.capture.frame_gap_p99_tick,
            max_frame_gap_tick: metrics.capture.max_frame_gap_tick,
            cursor_events_persisted: writer_stats.cursor_records,
            click_events: writer_stats.click_records,
            fmp4_fragments: media_report.inspection.fragment_count,
            encoder_name: media_report.encoder_name,
            hardware_encoder: media_report.hardware_encoder,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct CaptureTask {
    texture_index: usize,
    system_relative_tick: i64,
    queue_depth: usize,
}

struct SharedCaptureTexture {
    capture_texture: ID3D11Texture2D,
    processing_texture: ID3D11Texture2D,
    capture_mutex: IDXGIKeyedMutex,
    processing_mutex: IDXGIKeyedMutex,
}

struct CaptureShared {
    queue: ArrayQueue<CaptureTask>,
    free_textures: ArrayQueue<usize>,
    textures: Vec<SharedCaptureTexture>,
    immediate_context: ID3D11DeviceContext,
    context_gate: Mutex<()>,
    frames_received: AtomicU64,
    dropped_by_backpressure: AtomicU64,
    capture_queue_peak: AtomicU64,
    callback_failed: AtomicBool,
    geometry_changed: AtomicBool,
    width: u32,
    height: u32,
}

impl CaptureShared {
    fn new(
        capture_device: &D3d11Device,
        processing_device: &D3d11Device,
        width: u32,
        height: u32,
    ) -> Result<Self, RecorderError> {
        let mut textures = Vec::with_capacity(CAPTURE_TEXTURE_COUNT);
        let free_textures = ArrayQueue::new(CAPTURE_TEXTURE_COUNT);
        for index in 0..CAPTURE_TEXTURE_COUNT {
            textures.push(create_shared_capture_texture(
                capture_device,
                processing_device,
                width,
                height,
            )?);
            free_textures
                .push(index)
                .map_err(|_| RecorderError::CaptureQueueInvariant)?;
        }
        Ok(Self {
            queue: ArrayQueue::new(CAPTURE_QUEUE_CAPACITY),
            free_textures,
            textures,
            immediate_context: capture_device.immediate_context().clone(),
            context_gate: Mutex::new(()),
            frames_received: AtomicU64::new(0),
            dropped_by_backpressure: AtomicU64::new(0),
            capture_queue_peak: AtomicU64::new(0),
            callback_failed: AtomicBool::new(false),
            geometry_changed: AtomicBool::new(false),
            width,
            height,
        })
    }

    fn acquire_processing_texture(&self, index: usize) -> Result<&ID3D11Texture2D, RecorderError> {
        let texture = self
            .textures
            .get(index)
            .ok_or(RecorderError::CaptureQueueInvariant)?;
        unsafe {
            texture
                .processing_mutex
                .AcquireSync(PROCESSING_MUTEX_KEY, SHARED_TEXTURE_WAIT_MS)
        }
        .map_err(|source| RecorderError::SharedTextureSync {
            stage: "processing acquire",
            source,
        })?;
        Ok(&texture.processing_texture)
    }

    fn recycle_processing_texture(&self, index: usize) -> Result<(), RecorderError> {
        let texture = self
            .textures
            .get(index)
            .ok_or(RecorderError::CaptureQueueInvariant)?;
        unsafe { texture.processing_mutex.ReleaseSync(CAPTURE_MUTEX_KEY) }.map_err(|source| {
            RecorderError::SharedTextureSync {
                stage: "processing release",
                source,
            }
        })?;
        self.free_textures
            .push(index)
            .map_err(|_| RecorderError::CaptureQueueInvariant)
    }

    fn discard_captured_texture(&self, index: usize) -> Result<(), RecorderError> {
        self.acquire_processing_texture(index)?;
        self.recycle_processing_texture(index)
    }
}

struct RecordingCapture {
    frame_pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    token: i64,
    stopped: bool,
}

impl RecordingCapture {
    fn start(
        d3d: &D3d11Device,
        item: &windows::Graphics::Capture::GraphicsCaptureItem,
        size: windows::Graphics::SizeInt32,
        shared: Arc<CaptureShared>,
    ) -> Result<Self, RecorderError> {
        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            d3d.winrt_device(),
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            3,
            size,
        )
        .map_err(RecorderError::Windows)?;
        let session = frame_pool
            .CreateCaptureSession(item)
            .map_err(RecorderError::Windows)?;
        session
            .SetIsCursorCaptureEnabled(false)
            .map_err(RecorderError::Windows)?;
        if session
            .IsCursorCaptureEnabled()
            .map_err(RecorderError::Windows)?
        {
            return Err(RecorderError::CursorCaptureStillEnabled);
        }
        let handler =
            TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(move |pool, _| {
                let Some(pool) = pool.as_ref() else {
                    shared.callback_failed.store(true, Ordering::Release);
                    return Ok(());
                };
                match pool.TryGetNextFrame() {
                    Ok(frame) => {
                        if capture_frame(&shared, &frame).is_err() {
                            shared.callback_failed.store(true, Ordering::Release);
                        }
                        let _ = frame.Close();
                    }
                    Err(_) => shared.callback_failed.store(true, Ordering::Release),
                }
                Ok(())
            });
        let token = frame_pool
            .FrameArrived(&handler)
            .map_err(RecorderError::Windows)?;
        session.StartCapture().map_err(RecorderError::Windows)?;
        Ok(Self {
            frame_pool,
            session,
            token,
            stopped: false,
        })
    }

    fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        let _ = self.frame_pool.RemoveFrameArrived(self.token);
        let _ = self.session.Close();
        let _ = self.frame_pool.Close();
    }
}

impl Drop for RecordingCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn capture_frame(
    shared: &CaptureShared,
    frame: &Direct3D11CaptureFrame,
) -> windows::core::Result<()> {
    shared.frames_received.fetch_add(1, Ordering::Relaxed);
    let size = frame.ContentSize()?;
    if u32::try_from(size.Width).ok() != Some(shared.width)
        || u32::try_from(size.Height).ok() != Some(shared.height)
    {
        shared.geometry_changed.store(true, Ordering::Release);
        return Ok(());
    }
    let system_relative_tick = frame.SystemRelativeTime()?.Duration;
    let surface = frame.Surface()?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
    let source: ID3D11Texture2D = unsafe { access.GetInterface()? };
    let Ok(_guard) = shared.context_gate.lock() else {
        shared.callback_failed.store(true, Ordering::Release);
        return Ok(());
    };
    if shared.queue.is_full() {
        shared
            .dropped_by_backpressure
            .fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }
    let Some(texture_index) = shared.free_textures.pop() else {
        shared
            .dropped_by_backpressure
            .fetch_add(1, Ordering::Relaxed);
        return Ok(());
    };
    let texture = &shared.textures[texture_index];
    if let Err(source) = unsafe { texture.capture_mutex.AcquireSync(CAPTURE_MUTEX_KEY, 0) } {
        let _ = shared.free_textures.push(texture_index);
        return Err(source);
    }
    unsafe {
        shared
            .immediate_context
            .CopyResource(&texture.capture_texture, &source);
        shared.immediate_context.Flush();
    }
    unsafe {
        texture.capture_mutex.ReleaseSync(PROCESSING_MUTEX_KEY)?;
    }
    let task = CaptureTask {
        texture_index,
        system_relative_tick,
        queue_depth: shared.queue.len() + 1,
    };
    if shared.queue.push(task).is_err() {
        shared.callback_failed.store(true, Ordering::Release);
        return Ok(());
    }
    shared
        .capture_queue_peak
        .fetch_max(shared.queue.len() as u64, Ordering::Relaxed);
    Ok(())
}

struct OwnedSharedHandle(HANDLE);

impl Drop for OwnedSharedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn create_shared_capture_texture(
    capture_device: &D3d11Device,
    processing_device: &D3d11Device,
    width: u32,
    height: u32,
) -> Result<SharedCaptureTexture, RecorderError> {
    let description = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX)
            .0
            .cast_unsigned(),
    };
    let mut capture_texture = None;
    unsafe {
        capture_device.native_device().CreateTexture2D(
            &raw const description,
            None,
            Some(&raw mut capture_texture),
        )
    }
    .map_err(RecorderError::Windows)?;
    let capture_texture = capture_texture.ok_or(RecorderError::NullCaptureTexture)?;
    let capture_resource: IDXGIResource1 =
        capture_texture.cast().map_err(RecorderError::Windows)?;
    let access = DXGI_SHARED_RESOURCE_READ.0 | DXGI_SHARED_RESOURCE_WRITE.0;
    let handle = OwnedSharedHandle(
        unsafe { capture_resource.CreateSharedHandle(None, access, PCWSTR::null()) }
            .map_err(RecorderError::Windows)?,
    );
    let processing_device1: ID3D11Device1 = processing_device
        .native_device()
        .cast()
        .map_err(RecorderError::Windows)?;
    let processing_texture: ID3D11Texture2D =
        unsafe { processing_device1.OpenSharedResource1(handle.0) }
            .map_err(RecorderError::Windows)?;
    let capture_mutex: IDXGIKeyedMutex = capture_texture.cast().map_err(RecorderError::Windows)?;
    let processing_mutex: IDXGIKeyedMutex =
        processing_texture.cast().map_err(RecorderError::Windows)?;
    Ok(SharedCaptureTexture {
        capture_texture,
        processing_texture,
        capture_mutex,
        processing_mutex,
    })
}

fn wait_for_first_frame(
    shared: &CaptureShared,
    timeout: Duration,
) -> Result<CaptureTask, RecorderError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(task) = shared.queue.pop() {
            return Ok(task);
        }
        if shared.callback_failed.load(Ordering::Acquire) {
            return Err(RecorderError::CaptureCallbackFailed);
        }
        if Instant::now() >= deadline {
            return Err(RecorderError::FirstFrameTimeout);
        }
        thread::sleep(Duration::from_millis(2));
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingFrame {
    task: CaptureTask,
    tick: TimeTick,
}

struct FirstFrameTrace<'a> {
    session_log: &'a mut SessionLog,
    encoded: &'a mut bool,
}

struct CaptureGapTracker {
    previous_tick: TimeTick,
    gaps: Vec<i64>,
}

impl CaptureGapTracker {
    fn new(first_tick: TimeTick) -> Self {
        Self {
            previous_tick: first_tick,
            gaps: Vec::new(),
        }
    }

    fn observe(&mut self, tick: TimeTick) {
        let gap = tick.0.saturating_sub(self.previous_tick.0);
        if gap > 0 {
            self.gaps.push(gap);
            self.previous_tick = tick;
        }
    }

    fn finish(mut self, metrics: &mut ProjectMetrics) {
        self.gaps.sort_unstable();
        metrics.capture.frame_gap_sample_count = self.gaps.len() as u64;
        metrics.capture.max_frame_gap_tick = self.gaps.last().copied().unwrap_or_default();
        if !self.gaps.is_empty() {
            let rank = self.gaps.len().saturating_mul(99).div_ceil(100);
            metrics.capture.frame_gap_p99_tick = self.gaps[rank.saturating_sub(1)];
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn drain_capture_queue(
    shared: &CaptureShared,
    processor: &Nv12VideoProcessor,
    clock: &SessionClock,
    timestamp_normalizer: &mut VideoTimestampNormalizer,
    frame_rate_limiter: &mut VideoFrameRateLimiter,
    capture_gap_tracker: &mut CaptureGapTracker,
    pending: &mut Option<PendingFrame>,
    media_writer: &mut Nv12Fmp4Writer,
    diagnostic_writer: &mut CaptureDiagnosticWriter,
    metrics: &mut ProjectMetrics,
    session_log: &mut SessionLog,
    first_frame_encoded: &mut bool,
) -> Result<(), RecorderError> {
    // Consume only the frames that were queued when this service pass started. Otherwise a
    // continuously producing WGC callback can keep this loop non-empty forever and starve input,
    // checkpoints, and the recording deadline.
    let service_budget = shared.queue.len();
    for _ in 0..service_budget {
        let Some(task) = shared.queue.pop() else {
            break;
        };
        let tick = clock.frame_tick(task.system_relative_tick)?;
        capture_gap_tracker.observe(tick);
        diagnostic_writer.append(&CaptureFrameDiagnostic {
            system_relative_tick: task.system_relative_tick,
            source_time_tick: tick.0,
            width: shared.width,
            height: shared.height,
            queue_depth: task.queue_depth,
        })?;
        match timestamp_normalizer.observe(tick)? {
            VideoTimestampDecision::ReplacePrevious(_) => {
                metrics.capture.duplicate_timestamp += 1;
                if pending.is_some_and(|selected| selected.tick == tick) {
                    if let Some(replaced) = pending.replace(PendingFrame { task, tick }) {
                        shared.discard_captured_texture(replaced.task.texture_index)?;
                    }
                } else {
                    metrics.capture.dropped_by_frame_rate_limit += 1;
                    shared.discard_captured_texture(task.texture_index)?;
                }
            }
            VideoTimestampDecision::Accept(_) => match frame_rate_limiter.observe(tick)? {
                VideoFrameRateDecision::Drop(_) => {
                    metrics.capture.dropped_by_frame_rate_limit += 1;
                    shared.discard_captured_texture(task.texture_index)?;
                }
                VideoFrameRateDecision::Accept(_) => {
                    if let Some(previous) = pending.replace(PendingFrame { task, tick }) {
                        let duration = tick.0 - previous.tick.0;
                        let mut first_frame_trace = FirstFrameTrace {
                            session_log,
                            encoded: first_frame_encoded,
                        };
                        encode_pending(
                            shared,
                            processor,
                            previous,
                            duration,
                            media_writer,
                            metrics,
                            &mut first_frame_trace,
                        )?;
                    }
                }
            },
        }
    }
    Ok(())
}

fn encode_pending(
    shared: &CaptureShared,
    processor: &Nv12VideoProcessor,
    pending: PendingFrame,
    duration: i64,
    media_writer: &mut Nv12Fmp4Writer,
    metrics: &mut ProjectMetrics,
    first_frame_trace: &mut FirstFrameTrace<'_>,
) -> Result<(), RecorderError> {
    if !*first_frame_trace.encoded {
        first_frame_trace
            .session_log
            .stage("first-frame-processing-acquire-begin")?;
    }
    if !*first_frame_trace.encoded {
        first_frame_trace
            .session_log
            .stage("first-frame-convert-begin")?;
    }
    let processing_texture = shared.acquire_processing_texture(pending.task.texture_index)?;
    let nv12 = if *first_frame_trace.encoded {
        let mut trace_error = None;
        let result =
            convert_on_processing_device(processor, processing_texture, |conversion_stage| {
                if trace_error.is_none() {
                    trace_error = first_frame_trace
                        .session_log
                        .trace_with_detail(
                            "frame-convert-stage",
                            &format!("tick={} {conversion_stage}", pending.tick.0),
                        )
                        .err();
                }
            });
        if let Some(error) = trace_error {
            return Err(error);
        }
        result?
    } else {
        let mut trace_error = None;
        let result =
            convert_on_processing_device(processor, processing_texture, |conversion_stage| {
                if trace_error.is_none() {
                    trace_error = first_frame_trace
                        .session_log
                        .stage_with_detail("first-frame-convert-stage", conversion_stage)
                        .err();
                }
            });
        if let Some(error) = trace_error {
            return Err(error);
        }
        result?
    };
    shared.recycle_processing_texture(pending.task.texture_index)?;
    if *first_frame_trace.encoded {
        first_frame_trace.session_log.trace_with_detail(
            "frame-write-begin",
            &format!("tick={} duration={duration}", pending.tick.0),
        )?;
    } else {
        first_frame_trace
            .session_log
            .stage("first-frame-convert-complete")?;
        first_frame_trace
            .session_log
            .stage("first-frame-write-begin")?;
    }
    media_writer.write_nv12(&nv12, pending.tick.0, duration)?;
    if *first_frame_trace.encoded {
        first_frame_trace.session_log.trace_with_detail(
            "frame-write-complete",
            &format!("tick={} duration={duration}", pending.tick.0),
        )?;
    } else {
        first_frame_trace
            .session_log
            .stage("first-frame-write-complete")?;
        *first_frame_trace.encoded = true;
    }
    metrics.capture.frames_encoded += 1;
    Ok(())
}

fn convert_on_processing_device(
    processor: &Nv12VideoProcessor,
    input: &ID3D11Texture2D,
    mut trace: impl FnMut(&'static str),
) -> Result<Vec<u8>, RecorderError> {
    trace("processing-device-submit-begin");
    processor.submit_with_trace(input, &mut trace)?;
    trace("processing-device-submit-complete");
    processor
        .copy_completed_with_trace(&mut trace)
        .map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
fn drain_input(
    service: &InputCaptureService,
    buffer: &mut Vec<panzo_core::StampedInputEvent>,
    normalizer: &mut InputEventNormalizer,
    writer: &mut ProjectWriter,
    click_events: &mut Vec<panzo_core::ClickEvent>,
    metrics: &mut ProjectMetrics,
    cutoff_qpc: Option<i64>,
) -> Result<(), RecorderError> {
    let drained = service.drain();
    metrics.input.cursor_events_raw += drained
        .iter()
        .filter(|event| matches!(event.kind, RawInputKind::Cursor { .. }))
        .count() as u64;
    buffer.extend(drained);
    let ready = if let Some(cutoff) = cutoff_qpc {
        let mut recent = Vec::new();
        let mut ready = Vec::new();
        for event in buffer.drain(..) {
            if event.qpc <= cutoff {
                ready.push(event);
            } else {
                recent.push(event);
            }
        }
        *buffer = recent;
        ready
    } else {
        std::mem::take(buffer)
    };
    for event in normalizer.normalize_batch(ready)? {
        match &event {
            PersistedInputEvent::Cursor(_) => metrics.input.cursor_events_persisted += 1,
            PersistedInputEvent::Click(click) => {
                metrics.input.click_events += 1;
                click_events.push(click.clone());
            }
        }
        writer.append_input(&event)?;
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureFrameDiagnostic {
    system_relative_tick: i64,
    source_time_tick: i64,
    width: u32,
    height: u32,
    queue_depth: usize,
}

struct CaptureDiagnosticWriter {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl CaptureDiagnosticWriter {
    fn create(path: PathBuf) -> Result<Self, RecorderError> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| RecorderError::WriteFile {
                path: path.clone(),
                source,
            })?;
        Ok(Self {
            path,
            writer: BufWriter::new(file),
        })
    }

    fn append(&mut self, value: &CaptureFrameDiagnostic) -> Result<(), RecorderError> {
        serde_json::to_writer(&mut self.writer, value).map_err(RecorderError::Serialize)?;
        self.writer
            .write_all(b"\n")
            .map_err(|source| RecorderError::WriteFile {
                path: self.path.clone(),
                source,
            })
    }

    fn flush(&mut self) -> Result<(), RecorderError> {
        self.writer
            .flush()
            .map_err(|source| RecorderError::WriteFile {
                path: self.path.clone(),
                source,
            })
    }

    fn checkpoint(&mut self) -> Result<(), RecorderError> {
        self.flush()?;
        self.writer
            .get_ref()
            .sync_data()
            .map_err(|source| RecorderError::WriteFile {
                path: self.path.clone(),
                source,
            })
    }
}

/// Complete interrupted recording initialization without replacing saved edits.
/// The caller owns the project edit lock. Each missing file is published atomically,
/// so repeating this step after an interrupted write preserves every valid result.
pub(crate) fn ensure_recording_edit_files(
    layout: &ProjectLayout,
    clicks: &[panzo_core::ClickEvent],
    duration: TimeTick,
    geometry: PhysicalRect,
) -> Result<panzo_core::CameraTrack, RecorderError> {
    let manifest = layout.load_manifest()?;
    let camera_path = layout.root().join(&manifest.tracks.camera);
    let track = if camera_path.exists() {
        read_recording_json(&camera_path)?
    } else {
        let track = plan_camera_track(layout, clicks, duration, geometry)?;
        let bytes = serde_json::to_vec_pretty(&track).map_err(RecorderError::Serialize)?;
        persist_bytes_new_atomically(&camera_path, &bytes)?;
        track
    };
    panzo_core::CameraTrackEditor::new(track.clone(), duration)
        .map_err(|error| RecorderError::InvalidEditState(error.to_string()))?;
    let edit_directory = layout.root().join("edit");
    fs::create_dir_all(&edit_directory).map_err(|source| RecorderError::WriteFile {
        path: edit_directory.clone(),
        source,
    })?;
    let workbench_path = edit_directory.join("workbench.json");
    let settings: panzo_core::WorkbenchSettings = if workbench_path.exists() {
        read_recording_json(&workbench_path)?
    } else {
        let settings = panzo_core::WorkbenchSettings {
            camera_enabled: false,
            video: Some(
                panzo_core::VideoEdit::full(duration)
                    .map_err(|error| RecorderError::InvalidEditState(error.to_string()))?,
            ),
            ..panzo_core::WorkbenchSettings::default()
        };
        let bytes = serde_json::to_vec_pretty(&settings).map_err(RecorderError::Serialize)?;
        persist_bytes_new_atomically(&workbench_path, &bytes)?;
        settings
    };
    settings
        .validate()
        .map_err(|error| RecorderError::InvalidEditState(error.to_string()))?;
    settings
        .video_edit(duration)
        .map_err(|error| RecorderError::InvalidEditState(error.to_string()))?;
    Ok(track)
}

fn read_recording_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, RecorderError> {
    let bytes = fs::read(path).map_err(|source| RecorderError::ReadFile {
        path: path.into(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| RecorderError::InvalidEditFile {
        path: path.into(),
        source,
    })
}

fn plan_camera_track(
    layout: &ProjectLayout,
    clicks: &[panzo_core::ClickEvent],
    duration: TimeTick,
    geometry: PhysicalRect,
) -> Result<panzo_core::CameraTrack, RecorderError> {
    let click_json = serde_json::to_vec(clicks).map_err(RecorderError::Serialize)?;
    let geometry_json = serde_json::to_vec(&geometry).map_err(RecorderError::Serialize)?;
    let click_hash = sha256(&click_json);
    let geometry_hash = sha256(&geometry_json);
    let planned = CameraPlanner::new(PlannerParams::default(), "planner-v1").and_then(|planner| {
        planner.plan(PlannerInput {
            recording_duration: duration,
            clicks,
            click_track_hash: &click_hash,
            capture_geometry_hash: &geometry_hash,
        })
    });
    let track = match planned {
        Ok(track) => track,
        Err(error) => {
            persist_json_atomically(
                layout
                    .root()
                    .join("diagnostics/camera-planning-warning.json"),
                &serde_json::json!({"warning": error.to_string(), "fallback": "original-video"}),
            )?;
            panzo_core::CameraTrack {
                schema_version: panzo_core::camera::CAMERA_SCHEMA_VERSION,
                generator: panzo_core::GeneratorMetadata {
                    name: "original-video".into(),
                    version: "1".into(),
                    parameter_set: "fallback".into(),
                },
                source_revision: panzo_core::SourceRevision {
                    click_track_hash: click_hash,
                    capture_geometry_hash: geometry_hash,
                },
                base_state: panzo_core::CameraState::BASE,
                segments: Vec::new(),
            }
        }
    };
    Ok(track)
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

fn write_json_durable<T: Serialize>(path: PathBuf, value: &T) -> Result<(), RecorderError> {
    let mut json = serde_json::to_vec_pretty(value).map_err(RecorderError::Serialize)?;
    json.push(b'\n');
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|source| RecorderError::WriteFile {
            path: path.clone(),
            source,
        })?;
    file.write_all(&json)
        .and_then(|()| file.sync_all())
        .map_err(|source| RecorderError::WriteFile { path, source })
}

fn duration_to_tick(duration: Duration) -> Result<TimeTick, RecorderError> {
    let tick =
        i128::try_from(duration.as_nanos()).map_err(|_| RecorderError::DurationOverflow)? / 100;
    i64::try_from(tick)
        .map(TimeTick)
        .map_err(|_| RecorderError::DurationOverflow)
}

fn utc_now() -> String {
    let time: SYSTEMTIME = unsafe { GetSystemTime() };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        time.wYear,
        time.wMonth,
        time.wDay,
        time.wHour,
        time.wMinute,
        time.wSecond,
        time.wMilliseconds
    )
}

fn preflight_destination(project_root: &Path) -> Result<(), RecorderError> {
    if project_root.exists() {
        return Err(RecorderError::ProjectAlreadyExists(project_root.into()));
    }
    let parent = project_root
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(RecorderError::MissingParent(parent.into()));
    }
    let wide: Vec<u16> = parent.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0_u64;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&raw mut available), None, None) }
        .map_err(RecorderError::Windows)?;
    if available < MINIMUM_FREE_BYTES {
        return Err(RecorderError::InsufficientDiskSpace { available });
    }
    Ok(())
}

struct PreparedCaptureSource {
    item: GraphicsCaptureItem,
    capture_size: windows::Graphics::SizeInt32,
    desktop_rect: PhysicalRect,
    kind: CaptureKind,
    monitor_id: String,
    window_id: Option<String>,
    window_title: Option<String>,
}

fn prepare_capture_source(
    source: &RecordingSource,
) -> Result<PreparedCaptureSource, RecorderError> {
    match source {
        RecordingSource::PrimaryMonitor => {
            let monitor = primary_monitor().map_err(RecorderError::Windows)?;
            let item = primary_monitor_item(monitor)?;
            let capture_size = item.Size().map_err(RecorderError::Windows)?;
            let desktop_rect = PhysicalRect {
                x: monitor.rect[0],
                y: monitor.rect[1],
                width: u32::try_from(monitor.rect[2].saturating_sub(monitor.rect[0]))
                    .map_err(|_| RecorderError::InvalidCaptureSize)?,
                height: u32::try_from(monitor.rect[3].saturating_sub(monitor.rect[1]))
                    .map_err(|_| RecorderError::InvalidCaptureSize)?,
            };
            Ok(PreparedCaptureSource {
                item,
                capture_size,
                desktop_rect,
                kind: CaptureKind::Monitor,
                monitor_id: format!("primary-{}x{}", desktop_rect.width, desktop_rect.height),
                window_id: None,
                window_title: None,
            })
        }
        RecordingSource::Window(target) => {
            let item = window_capture_item(target)?;
            let capture_size = item.Size().map_err(RecorderError::Windows)?;
            Ok(PreparedCaptureSource {
                item,
                capture_size,
                desktop_rect: target.current_desktop_rect()?,
                kind: CaptureKind::Window,
                monitor_id: String::new(),
                window_id: Some(target.source_id()),
                window_title: Some(target.title.clone()),
            })
        }
    }
}

#[derive(Debug, Error)]
pub enum RecorderError {
    #[error("invalid saved edit file {path}: {source}")]
    InvalidEditFile {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("recording edit state is invalid: {0}")]
    InvalidEditState(String),
    #[error("could not read {path}: {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    ProjectLock(#[from] crate::platform::editor_transaction::EditorTransactionError),
    #[error("recording duration must be at least 250 ms")]
    InvalidDuration,
    #[error("recording duration overflows the Panzo timebase")]
    DurationOverflow,
    #[error("project already exists: {0}")]
    ProjectAlreadyExists(PathBuf),
    #[error("project parent directory does not exist: {0}")]
    MissingParent(PathBuf),
    #[error("recording requires at least 2 GiB free; only {available} bytes are available")]
    InsufficientDiskSpace { available: u64 },
    #[error("V0.1 preflight failed:\n{0}")]
    UnsupportedPreflight(String),
    #[error("capture item returned an invalid size")]
    InvalidCaptureSize,
    #[error("capture session did not disable the system cursor")]
    CursorCaptureStillEnabled,
    #[error("capture did not produce its first frame within five seconds")]
    FirstFrameTimeout,
    #[error("capture callback failed")]
    CaptureCallbackFailed,
    #[error("capture geometry changed during recording")]
    CaptureGeometryChanged,
    #[error("capture queue ownership invariant failed")]
    CaptureQueueInvariant,
    #[error("D3D11 returned a null capture texture")]
    NullCaptureTexture,
    #[error("shared capture texture synchronization failed during {stage}: {source}")]
    SharedTextureSync {
        stage: &'static str,
        source: windows::core::Error,
    },
    #[error("input queue overflowed; recording integrity is not guaranteed")]
    InputQueueOverflow,
    #[error("Media Foundation selected a non-hardware encoder")]
    HardwareEncoderRequired,
    #[error("could not atomically commit media {from} to {to}: {source}")]
    CommitMedia {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
    #[error("could not write {path}: {source}")]
    WriteFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Windows call failed: {0}")]
    Windows(windows::core::Error),
    #[error("could not serialize project data: {0}")]
    Serialize(serde_json::Error),
    #[error(transparent)]
    Preflight(#[from] PreflightError),
    #[error(transparent)]
    CaptureProbe(#[from] CaptureProbeError),
    #[error(transparent)]
    WindowCapture(#[from] WindowCaptureError),
    #[error(transparent)]
    Qpc(#[from] QpcSourceError),
    #[error(transparent)]
    Time(#[from] TimeError),
    #[error(transparent)]
    ProjectIo(#[from] ProjectIoError),
    #[error(transparent)]
    ProjectValidation(#[from] panzo_core::project::ProjectValidationError),
    #[error(transparent)]
    ProjectWriter(#[from] ProjectWriterError),
    #[error(transparent)]
    InputCapture(#[from] InputCaptureError),
    #[error(transparent)]
    InputNormalization(#[from] InputNormalizationError),
    #[error(transparent)]
    ManifestStore(#[from] ManifestStoreError),
    #[error(transparent)]
    VideoProcessor(#[from] VideoProcessorProbeError),
    #[error(transparent)]
    Media(#[from] Fmp4ProbeError),
    #[error(transparent)]
    Planner(#[from] panzo_core::planner::PlannerError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_control_shares_a_monotonic_stop_request() {
        let control = RecordingControl::default();
        let worker = control.clone();
        assert!(!control.stop_requested());
        assert!(!control.recording_started());
        worker.mark_recording_started();
        assert!(control.recording_started());
        worker.request_stop();
        assert!(control.stop_requested());
    }

    #[test]
    fn keyed_shared_texture_is_stable_across_isolated_capture_and_processing_devices() {
        const WIDTH: u32 = 2_560;
        const HEIGHT: u32 = 1_440;

        let capture_device = D3d11Device::create_hardware().unwrap();
        let processing_device = D3d11Device::create_hardware().unwrap();
        let shared =
            create_shared_capture_texture(&capture_device, &processing_device, WIDTH, HEIGHT)
                .unwrap();
        let source_description = D3D11_TEXTURE2D_DESC {
            Width: WIDTH,
            Height: HEIGHT,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut source = None;
        unsafe {
            capture_device.native_device().CreateTexture2D(
                &raw const source_description,
                None,
                Some(&raw mut source),
            )
        }
        .unwrap();
        let source = source.unwrap();
        let processor = Nv12VideoProcessor::create(&processing_device, WIDTH, HEIGHT).unwrap();

        for _ in 0..300 {
            unsafe {
                shared
                    .capture_mutex
                    .AcquireSync(CAPTURE_MUTEX_KEY, SHARED_TEXTURE_WAIT_MS)
                    .unwrap();
                capture_device
                    .immediate_context()
                    .CopyResource(&shared.capture_texture, &source);
                capture_device.immediate_context().Flush();
                shared
                    .capture_mutex
                    .ReleaseSync(PROCESSING_MUTEX_KEY)
                    .unwrap();
                shared
                    .processing_mutex
                    .AcquireSync(PROCESSING_MUTEX_KEY, SHARED_TEXTURE_WAIT_MS)
                    .unwrap();
            }
            let output = processor.convert(&shared.processing_texture).unwrap();
            assert_eq!(output.len(), 1_920 * 1_080 * 3 / 2);
            unsafe {
                shared
                    .processing_mutex
                    .ReleaseSync(CAPTURE_MUTEX_KEY)
                    .unwrap();
            }
        }
    }
}
