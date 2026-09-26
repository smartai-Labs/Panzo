#[path = "editor_layout.rs"]
mod editor_layout;
use editor_layout::ControlLayout;
#[path = "editor_chrome.rs"]
mod editor_chrome;
use editor_chrome::draw_controls;
#[cfg(test)]
#[path = "crop_review.rs"]
mod crop_review;
#[path = "editor_crop.rs"]
mod editor_crop;
#[path = "editor_navigation.rs"]
mod editor_navigation;
#[path = "editor_titlebar.rs"]
pub(crate) mod editor_titlebar;
#[path = "editor_widgets.rs"]
mod editor_widgets;
#[path = "editor_workflow.rs"]
mod editor_workflow;
use crate::platform::recorder_window::NavigationIntent;
#[path = "workbench_actions.rs"]
mod workbench_actions;
#[cfg(test)]
#[path = "workflow_review.rs"]
mod workflow_review;
use crate::platform::compositor::GpuPreviewPresenter;
use crate::platform::editor::{EditorSessionError, ProjectEditorSession};
use crate::platform::editor_timeline::TimelineViewport;
use crate::platform::editor_ui::{
    EditorTheme, IconKind, StatusFeedback, StatusMessage, StatusScope, StatusSeverity, TextStyle,
    button_interaction, complete_pointer_activation, draw_icon, enable_per_monitor_dpi_awareness,
    focus_ring, measure_text_width, next_keyboard_focus, rounded_surface, select_font, system_dpi,
    window_dpi,
};
use crate::platform::preview::{
    PreparedPreviewFrame, PreviewFrame, PreviewSessionError, ProjectPreviewSession,
};
use crate::platform::preview_scheduler::ScrubPresentationGate;
use crate::platform::scrub_preview::{ScrubPreviewWorker, ScrubPreviewWorkerError};
use panzo_core::{
    CameraEditKind, CameraSegment, CameraSegmentKind, CameraState, NormalizedPoint,
    OutputDescriptor, PlaybackState, TICKS_PER_SECOND, TimeMapping, TimeTick,
};
use std::ffi::c_void;
use std::mem;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use thiserror::Error;
use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT,
    RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
    DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, HALFTONE, HGDIOBJ, InvalidateRect,
    PAINTSTRUCT, SRCCOPY, ScreenToClient, SelectObject, SetBkMode, SetStretchBltMode, SetTextColor,
    SetViewportOrgEx, StretchDIBits, TRANSPARENT, UpdateWindow,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, CreateSolidBrush};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
    OPENFILENAMEW,
};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VK_CONTROL, VK_DELETE, VK_ESCAPE, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE,
    VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos,
    IDC_ARROW, IDC_HAND, IDC_IBEAM, IDC_SIZEALL, IDC_SIZENS, IDC_SIZEWE, LoadCursorW, MINMAXINFO,
    MoveWindow, PostMessageW, RegisterClassW, SWP_NOACTIVATE, SWP_NOZORDER, SetCursor,
    SetWindowPos, SetWindowTextW, WINDOW_EX_STYLE, WM_APP, WM_CANCELMODE, WM_CAPTURECHANGED,
    WM_CLOSE, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN, WM_KILLFOCUS,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCCREATE, WM_PAINT, WM_SETCURSOR, WM_SETFOCUS, WM_SIZE, WM_TIMER, WNDCLASSW,
    WS_CHILD, WS_DISABLED,
};
use windows::core::{Error as WindowsError, PCWSTR, PWSTR, w};

static NEXT_EDITOR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
const TIMER_ID: usize = 1;
const WM_INTERACTIVE_RENDER: u32 = WM_APP + 1;
const WM_SCRUB_PREVIEW: u32 = WM_APP + 2;
const WM_SCRUB_READY: u32 = WM_APP + 3;
const WM_FRAME_CLOCK: u32 = WM_APP + 4;
const INTERACTIVE_SEEK_FRAME_BUDGET: usize = 4;
const DRAG_REFRESH_INTERVAL: Duration = Duration::from_millis(16);
const SCRUB_PREVIEW_INTERVAL: Duration = Duration::from_millis(12);
const SCRUB_TIME_REFRESH_INTERVAL: Duration = Duration::from_millis(16);
const SEEK_STEP: TimeTick = TimeTick(5 * TICKS_PER_SECOND);
const EDIT_STEP: TimeTick = TimeTick(TICKS_PER_SECOND / 10);
const WHEEL_EDIT_SETTLE: Duration = Duration::from_millis(160);
const TOOLTIP_DELAY: Duration = Duration::from_millis(550);
const TIMELINE_RULER_STEPS: [i64; 18] = [
    10_000,
    50_000,
    100_000,
    500_000,
    1_000_000,
    5_000_000,
    10_000_000,
    20_000_000,
    50_000_000,
    100_000_000,
    300_000_000,
    600_000_000,
    1_200_000_000,
    3_000_000_000,
    6_000_000_000,
    12_000_000_000,
    30_000_000_000,
    60_000_000_000,
];

pub struct PreviewWindow;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SmokeAction {
    None,
    Play,
    Trim,
    QueuedPlayback,
    Scrub,
}

impl PreviewWindow {
    /// Opens the native Editor Workbench shell on top of the shared Preview/Export renderer.
    pub fn run(project_root: impl AsRef<Path>) -> Result<(), PreviewWindowError> {
        Self::run_internal(project_root.as_ref(), None, SmokeAction::None).map(|_| ())
    }

    pub fn smoke(
        project_root: impl AsRef<Path>,
        duration: Duration,
    ) -> Result<PreviewWindowSmokeReport, PreviewWindowError> {
        if duration.is_zero() {
            return Err(PreviewWindowError::InvalidSmokeDuration);
        }
        Self::run_internal(project_root.as_ref(), Some(duration), SmokeAction::Play)
    }

    pub fn smoke_paused(
        project_root: impl AsRef<Path>,
        duration: Duration,
    ) -> Result<PreviewWindowSmokeReport, PreviewWindowError> {
        if duration.is_zero() {
            return Err(PreviewWindowError::InvalidSmokeDuration);
        }
        Self::run_internal(project_root.as_ref(), Some(duration), SmokeAction::None)
    }

    pub fn smoke_trim(
        project_root: impl AsRef<Path>,
        duration: Duration,
    ) -> Result<PreviewWindowSmokeReport, PreviewWindowError> {
        if duration.is_zero() {
            return Err(PreviewWindowError::InvalidSmokeDuration);
        }
        Self::run_internal(project_root.as_ref(), Some(duration), SmokeAction::Trim)
    }

    pub fn smoke_queued_playback(
        project_root: impl AsRef<Path>,
        duration: Duration,
    ) -> Result<PreviewWindowSmokeReport, PreviewWindowError> {
        if duration.is_zero() {
            return Err(PreviewWindowError::InvalidSmokeDuration);
        }
        Self::run_internal(
            project_root.as_ref(),
            Some(duration),
            SmokeAction::QueuedPlayback,
        )
    }

    pub fn smoke_scrub(
        project_root: impl AsRef<Path>,
        duration: Duration,
    ) -> Result<PreviewWindowSmokeReport, PreviewWindowError> {
        if duration < Duration::from_secs(4) {
            return Err(PreviewWindowError::InvalidSmokeDuration);
        }
        Self::run_internal(project_root.as_ref(), Some(duration), SmokeAction::Scrub)
    }

    #[allow(clippy::too_many_lines)]
    fn run_internal(
        project_root: &Path,
        smoke_duration: Option<Duration>,
        action: SmokeAction,
    ) -> Result<PreviewWindowSmokeReport, PreviewWindowError> {
        Self::run_internal_notifying(project_root, smoke_duration, action, None)
    }

    pub fn run_notifying(
        project_root: &Path,
        ready: impl FnOnce(usize) + 'static,
    ) -> Result<(), PreviewWindowError> {
        Self::run_internal_notifying(project_root, None, SmokeAction::None, Some(Box::new(ready)))
            .map(|_| ())
    }

    #[allow(clippy::too_many_lines)]
    fn run_internal_notifying(
        project_root: &Path,
        smoke_duration: Option<Duration>,
        action: SmokeAction,
        ready: Option<Box<dyn FnOnce(usize)>>,
    ) -> Result<PreviewWindowSmokeReport, PreviewWindowError> {
        enable_per_monitor_dpi_awareness();
        let initial_theme = EditorTheme::current(system_dpi());
        let output = ProjectPreviewSession::native_output_descriptor(project_root)?;
        let telemetry = Arc::new(WindowTelemetry::default());
        let mut state = Box::new(WindowState::open(
            project_root,
            output,
            smoke_duration,
            action,
            Arc::clone(&telemetry),
        )?);
        let output = state.session.output();
        state.theme = initial_theme;
        let library = std::env::var_os("USERPROFILE").map_or_else(
            || {
                project_root
                    .parent()
                    .unwrap_or(Path::new("."))
                    .to_path_buf()
            },
            |profile| std::path::PathBuf::from(profile).join("Videos/Panzo"),
        );
        crate::platform::recorder_window::RecorderWindow::run_editor(&library, state, ready)
            .map_err(|error| PreviewWindowError::Host(error.to_string()))?;
        Ok(PreviewWindowSmokeReport {
            scrub_metrics: telemetry
                .scrub_metrics
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .report(),
            scripted_os_interruptions: telemetry.scripted_os_interruptions.load(Ordering::Relaxed),
            cadence: telemetry
                .cadence
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .report(),
            window_created: telemetry.window_created.load(Ordering::Relaxed),
            output_width: output.width,
            output_height: output.height,
            timer_ticks: telemetry.timer_ticks.load(Ordering::Relaxed),
            rendered_frames: telemetry.rendered_frames.load(Ordering::Relaxed),
            paint_calls: telemetry.paint_calls.load(Ordering::Relaxed),
            render_total_micros: telemetry.render_total_micros.load(Ordering::Relaxed),
            render_max_micros: telemetry.render_max_micros.load(Ordering::Relaxed),
            background_decodes: telemetry.background_decodes.load(Ordering::Relaxed),
            source_uploads: telemetry.source_uploads.load(Ordering::Relaxed),
            background_uploads: telemetry.background_uploads.load(Ordering::Relaxed),
            output_allocations: telemetry.output_allocations.load(Ordering::Relaxed),
            readbacks: telemetry.readbacks.load(Ordering::Relaxed),
            presentations: telemetry.presentations.load(Ordering::Relaxed),
            gpu_direct_preview: telemetry.gpu_direct_preview.load(Ordering::Relaxed),
            last_project_tick: TimeTick(telemetry.last_project_tick.load(Ordering::Relaxed)),
            graceful_close: telemetry.graceful_close.load(Ordering::Relaxed),
            trim_action_micros: telemetry.trim_action_micros.load(Ordering::Relaxed),
            trim_status: TrimSmokeStatus::from_telemetry(action == SmokeAction::Trim, &telemetry),
            queued_playback_completed: telemetry.queued_playback_completed.load(Ordering::Relaxed),
            queued_playback_micros: telemetry.queued_playback_micros.load(Ordering::Relaxed),
            scrub_presented_frames: telemetry.scrub_presented_frames.load(Ordering::Relaxed),
            scrub_distinct_frames: telemetry.scrub_distinct_frames.load(Ordering::Relaxed),
            scrub_reduced_frames: telemetry.scrub_reduced_frames.load(Ordering::Relaxed),
            scrub_max_time_error_tick: telemetry.scrub_max_time_error_tick.load(Ordering::Relaxed),
            scrub_max_gap_micros: telemetry.scrub_max_gap_micros.load(Ordering::Relaxed),
            scrub_settled: telemetry.scrub_settled.load(Ordering::Relaxed),
            media_errors: telemetry.media_errors.load(Ordering::Relaxed),
        })
    }
}

fn register_preview_surface_class(
    instance: HINSTANCE,
    class_name: PCWSTR,
) -> Result<(), PreviewWindowError> {
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(preview_surface_window_proc),
        hInstance: instance,
        lpszClassName: class_name,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&raw const window_class) } == 0 {
        let last_error = unsafe { GetLastError() };
        let source = WindowsError::from_thread();
        if last_error != ERROR_CLASS_ALREADY_EXISTS {
            return Err(PreviewWindowError::WindowsStage {
                stage: "RegisterClassW(preview surface)",
                source,
            });
        }
    }
    Ok(())
}

unsafe extern "system" fn preview_surface_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewWindowSmokeReport {
    pub scrub_metrics: crate::platform::presentation_metrics::ScrubMetricsReport,
    pub scripted_os_interruptions: u32,
    pub cadence: crate::platform::presentation_metrics::Cadence,
    pub window_created: bool,
    pub output_width: u32,
    pub output_height: u32,
    pub timer_ticks: u32,
    pub rendered_frames: u32,
    pub paint_calls: u32,
    pub render_total_micros: u64,
    pub render_max_micros: u64,
    pub background_decodes: u64,
    pub source_uploads: u64,
    pub background_uploads: u64,
    pub output_allocations: u64,
    pub readbacks: u64,
    pub presentations: u64,
    pub gpu_direct_preview: bool,
    pub last_project_tick: TimeTick,
    pub graceful_close: bool,
    pub trim_action_micros: u64,
    pub trim_status: TrimSmokeStatus,
    pub queued_playback_completed: bool,
    pub queued_playback_micros: u64,
    pub scrub_presented_frames: u32,
    pub scrub_distinct_frames: u32,
    pub scrub_reduced_frames: u32,
    pub scrub_max_time_error_tick: u64,
    pub scrub_max_gap_micros: u64,
    pub scrub_settled: bool,
    pub media_errors: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimSmokeStatus {
    NotRequested,
    ActionFailed,
    CompletedSynchronously,
    PreviewIncomplete,
    PassedIncrementally,
}

impl TrimSmokeStatus {
    fn from_telemetry(requested: bool, telemetry: &WindowTelemetry) -> Self {
        if !requested {
            return Self::NotRequested;
        }
        if !telemetry.trim_action_complete.load(Ordering::Relaxed) {
            return Self::ActionFailed;
        }
        if !telemetry.trim_preview_incremental.load(Ordering::Relaxed) {
            return Self::CompletedSynchronously;
        }
        if telemetry.trim_preview_complete.load(Ordering::Relaxed) {
            Self::PassedIncrementally
        } else {
            Self::PreviewIncomplete
        }
    }

    pub const fn passed_incrementally(self) -> bool {
        matches!(self, Self::PassedIncrementally)
    }

    const fn label(self) -> &'static str {
        match self {
            Self::NotRequested => "not-requested",
            Self::ActionFailed => "action-failed",
            Self::CompletedSynchronously => "completed-synchronously",
            Self::PreviewIncomplete => "preview-incomplete",
            Self::PassedIncrementally => "passed-incrementally",
        }
    }
}

impl std::fmt::Display for PreviewWindowSmokeReport {
    #[allow(clippy::too_many_lines)] // Explicit report fields preserve the acceptance probe format.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(formatter, "Panzo Editor Workbench smoke")?;
        writeln!(
            formatter,
            "  Scrub request-to-Present samples: {}",
            self.scrub_metrics.samples
        )?;
        writeln!(
            formatter,
            "  Scrub request-to-Present P50/P95/P99/max us: {}/{}/{}/{}",
            self.scrub_metrics.p50_us,
            self.scrub_metrics.p95_us,
            self.scrub_metrics.p99_us,
            self.scrub_metrics.max_us
        )?;
        writeln!(
            formatter,
            "  Scrub source time error P95 tick: {}",
            self.scrub_metrics.time_error_p95_tick
        )?;
        writeln!(
            formatter,
            "  Scrub minimum distinct updates per 2 seconds: {:?}",
            self.scrub_metrics.minimum_updates_per_two_seconds
        )?;
        writeln!(formatter, "  Window created: {}", self.window_created)?;
        writeln!(
            formatter,
            "  Output: {}x{}",
            self.output_width, self.output_height
        )?;
        writeln!(formatter, "  Timer ticks: {}", self.timer_ticks)?;
        writeln!(formatter, "  Rendered frames: {}", self.rendered_frames)?;
        writeln!(formatter, "  Paint calls: {}", self.paint_calls)?;
        writeln!(
            formatter,
            "  Average render: {} us",
            self.render_total_micros / u64::from(self.rendered_frames.max(1))
        )?;
        writeln!(formatter, "  Maximum render: {} us", self.render_max_micros)?;
        writeln!(
            formatter,
            "  Scripted OS input interruptions: {}",
            self.scripted_os_interruptions
        )?;
        writeln!(
            formatter,
            "  Playback cadence samples: {}",
            self.cadence.samples
        )?;
        writeln!(
            formatter,
            "  Playback Present P95: {} us",
            self.cadence.p95_us
        )?;
        writeln!(
            formatter,
            "  Playback Present P99: {} us",
            self.cadence.p99_us
        )?;
        writeln!(
            formatter,
            "  Playback Present maximum: {} us",
            self.cadence.max_us
        )?;
        writeln!(
            formatter,
            "  Background decodes: {}",
            self.background_decodes
        )?;
        writeln!(
            formatter,
            "  Source texture uploads: {}",
            self.source_uploads
        )?;
        writeln!(
            formatter,
            "  Background texture uploads: {}",
            self.background_uploads
        )?;
        writeln!(
            formatter,
            "  Output texture allocations: {}",
            self.output_allocations
        )?;
        writeln!(
            formatter,
            "  GPU direct preview: {}",
            self.gpu_direct_preview
        )?;
        writeln!(formatter, "  GPU presentations: {}", self.presentations)?;
        writeln!(formatter, "  CPU readbacks: {}", self.readbacks)?;
        writeln!(
            formatter,
            "  Last project tick: {}",
            self.last_project_tick.as_i64()
        )?;
        writeln!(formatter, "  Graceful close: {}", self.graceful_close)?;
        writeln!(
            formatter,
            "  Trim action elapsed: {} us",
            self.trim_action_micros
        )?;
        writeln!(
            formatter,
            "  Trim smoke status: {}",
            self.trim_status.label()
        )?;
        writeln!(
            formatter,
            "  Queued playback completed: {}",
            self.queued_playback_completed
        )?;
        writeln!(
            formatter,
            "  Queued playback latency: {} us",
            self.queued_playback_micros
        )?;
        writeln!(
            formatter,
            "  Scrub presented frames: {}",
            self.scrub_presented_frames
        )?;
        writeln!(
            formatter,
            "  Scrub maximum presentation gap: {} us",
            self.scrub_max_gap_micros
        )?;
        writeln!(
            formatter,
            "  Scrub release settled exactly: {}",
            self.scrub_settled
        )?;
        writeln!(
            formatter,
            "  Scrub distinct source frames: {}",
            self.scrub_distinct_frames
        )?;
        writeln!(
            formatter,
            "  Scrub reduced-resolution frames: {}",
            self.scrub_reduced_frames
        )?;
        writeln!(
            formatter,
            "  Scrub maximum source time error tick: {}",
            self.scrub_max_time_error_tick
        )?;
        write!(formatter, "  Media errors: {}", self.media_errors)
    }
}

#[derive(Default)]
struct WindowTelemetry {
    scrub_metrics: std::sync::Mutex<crate::platform::presentation_metrics::ScrubMetrics>,
    scripted_os_interruptions: AtomicU32,
    cadence: std::sync::Mutex<crate::platform::presentation_metrics::PresentationMetrics>,
    window_created: AtomicBool,
    timer_ticks: AtomicU32,
    rendered_frames: AtomicU32,
    paint_calls: AtomicU32,
    render_total_micros: AtomicU64,
    render_max_micros: AtomicU64,
    background_decodes: AtomicU64,
    source_uploads: AtomicU64,
    background_uploads: AtomicU64,
    output_allocations: AtomicU64,
    readbacks: AtomicU64,
    presentations: AtomicU64,
    gpu_direct_preview: AtomicBool,
    last_project_tick: AtomicI64,
    graceful_close: AtomicBool,
    trim_action_complete: AtomicBool,
    trim_action_micros: AtomicU64,
    trim_preview_incremental: AtomicBool,
    trim_preview_complete: AtomicBool,
    queued_playback_completed: AtomicBool,
    queued_playback_micros: AtomicU64,
    scrub_presented_frames: AtomicU32,
    scrub_distinct_frames: AtomicU32,
    scrub_reduced_frames: AtomicU32,
    scrub_max_time_error_tick: AtomicU64,
    scrub_max_gap_micros: AtomicU64,
    scrub_settled: AtomicBool,
    media_errors: AtomicU32,
}

#[allow(clippy::struct_excessive_bools)]
pub(crate) struct WindowState {
    accessibility: crate::platform::accessibility::AccessibilityHost,
    accessibility_focus: Option<u32>,
    scrub_preview: ScrubPreviewWorker,
    thumbnails: crate::platform::timeline_thumbnails::TimelineThumbnails,
    background_preview: std::cell::RefCell<Option<editor_chrome::BackgroundPreview>>,
    timer_resolution: Option<crate::platform::timer_resolution::TimerResolution>,
    frame_clock: Option<crate::platform::editor_clock::EditorClock>,
    session: ProjectPreviewSession,
    editor: ProjectEditorSession,
    frame: PreparedPreviewFrame,
    fallback_frame: Option<PreviewFrame>,
    preview_surface: Option<PreviewSurface>,
    host_hwnd: Option<HWND>,
    caption_bounds: Option<RECT>,
    displayed_title: String,
    project_name: String,
    selected_segment: Option<String>,
    selected_video: Option<String>,
    video_drag: Option<VideoDrag>,
    snap_guide: Option<TimeTick>,
    export_task: Option<crate::platform::editor_tasks::ExportTask>,
    export_output: Option<std::path::PathBuf>,
    export_panel: Option<crate::platform::export_window::ExportPanel>,
    save_task: Option<crate::platform::editor_tasks::SaveTask>,
    draft_manager: Option<crate::platform::editor::DraftManager>,
    recovery_notice: Option<String>,
    pending_navigation: Option<NavigationIntent>,
    navigation_authorized: bool,
    navigation_prompt_open: bool,
    #[cfg(test)]
    review_navigation_answer: Option<windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_RESULT>,
    generation: usize,
    background_task: Option<crate::platform::editor_tasks::BackgroundTask>,
    timeline_drag: Option<TimelineDrag>,
    timeline_scrub: Option<TimelineScrub>,
    canvas_drag: Option<CanvasDrag>,
    timeline: TimelineViewport,
    timeline_pan: Option<(i32, TimelineViewport)>,
    timeline_resize: Option<(i32, i32)>,
    inspector_resize: Option<(i32, i32)>,
    inspector_width_dip: i32,
    inspector_scroll: i32,
    inspector_scroll_drag: Option<(i32, i32)>,
    property_drag: Option<ControlId>,
    inline_input: Option<crate::platform::inline_input::InlineInput>,
    inline_control: Option<ControlId>,
    inline_validation: Option<String>,
    inspector_image_tab: bool,
    inspector_video_tab: bool,
    video_collapsed: [bool; 2],
    crop_editing: bool,
    crop_drag: Option<editor_crop::CropDrag>,
    crop_locked: bool,
    timeline_scroll: Option<(i32, TimelineViewport)>,
    follow_playhead: bool,
    wheel_edit_deadline: Option<Instant>,
    wheel_edit_pending: bool,
    trim_smoke: TrimSmokeState,
    queued_playback_smoke: QueuedPlaybackSmokeState,
    queued_playback_started: Option<Instant>,
    theme: EditorTheme,
    hot_control: Option<ControlId>,
    pressed_control: Option<ControlId>,
    focused_control: Option<ControlId>,
    keyboard_focus: bool,
    tooltip_deadline: Option<Instant>,
    tooltip_visible: bool,
    tracking_mouse_leave: bool,
    deferred_playback: DeferredPlayback,
    last_drag_refresh: Instant,
    last_scrub_submit: Instant,
    last_scrub_time_refresh: Instant,
    scrub_message_pending: bool,
    scrub_gate: ScrubPresentationGate,
    scrub_smoke: Option<ScrubSmoke>,
    last_clock: Instant,
    last_title_refresh: Instant,
    status: String,
    status_severity: crate::platform::editor_ui::StatusSeverity,
    feedback: StatusFeedback,
    status_until: Option<Instant>,
    close_at: Option<Instant>,
    telemetry: Arc<WindowTelemetry>,
}

#[derive(Clone)]
struct VideoDrag {
    original: panzo_core::VideoClip,
    start_x: i32,
    current_x: i32,
    view_start: TimeTick,
    playhead_source: TimeTick,
    trim_start: bool,
}

struct PreviewSurface {
    hwnd: HWND,
    presenter: GpuPreviewPresenter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrimSmokeState {
    Disabled,
    Pending,
    WaitingForPreview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueuedPlaybackSmokeState {
    Disabled,
    Pending,
    WaitingForPlayback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeferredPlayback {
    None,
    PlayWhenReady,
}

impl DeferredPlayback {
    const fn is_requested(self) -> bool {
        matches!(self, Self::PlayWhenReady)
    }

    fn request(&mut self) {
        *self = Self::PlayWhenReady;
    }

    fn cancel(&mut self) {
        *self = Self::None;
    }

    fn toggle(&mut self) {
        *self = if self.is_requested() {
            Self::None
        } else {
            Self::PlayWhenReady
        };
    }

    fn take_requested(&mut self) -> bool {
        let requested = self.is_requested();
        self.cancel();
        requested
    }
}

use crate::platform::edit_gestures::RangeDrag as TimelineDragMode;

#[derive(Debug, Clone)]
struct TimelineDrag {
    mode: TimelineDragMode,
    start_x: i32,
    current_x: i32,
    original: CameraSegment,
    grab_source: TimeTick,
}

#[derive(Debug, Clone, Copy)]
struct TimelineScrub {
    original_tick: TimeTick,
    current_x: i32,
    desired_tick: TimeTick,
    submitted_tick: Option<TimeTick>,
}

impl TimelineScrub {
    fn update(&mut self, current_x: i32, desired_tick: TimeTick) {
        self.current_x = current_x;
        self.desired_tick = desired_tick;
    }

    fn needs_preview(self) -> bool {
        self.submitted_tick != Some(self.desired_tick)
    }

    fn take_latest_preview(&mut self) -> Option<TimeTick> {
        if !self.needs_preview() {
            return None;
        }
        self.submitted_tick = Some(self.desired_tick);
        Some(self.desired_tick)
    }
}

#[derive(Debug, Clone)]
struct CanvasDrag {
    current_x: i32,
    current_y: i32,
    original: CameraSegment,
    initial_transform: panzo_core::CameraTransform,
}

#[derive(Clone, Copy, Default)]
struct ScrubSmoke {
    started: Option<Instant>,
    released: bool,
    target: Option<TimeTick>,
    last_presented: Option<Instant>,
}

impl WindowState {
    fn set_status(&mut self, severity: crate::platform::editor_ui::StatusSeverity, text: String) {
        if self.status == text && self.status_severity == severity {
            return;
        }
        self.status_severity = severity;
        self.status = text;
        self.status_until = Some(Instant::now() + Duration::from_secs(8));
    }

    fn status_message(&self) -> (&str, StatusSeverity) {
        if let Some(message) = self.feedback.current() {
            (&message.summary, message.severity)
        } else {
            (&self.status, self.status_severity)
        }
    }

    fn note_problem(&mut self, summary: &str, detail: impl std::fmt::Display) {
        self.feedback.remember(summary, &detail.to_string());
        self.set_status(StatusSeverity::Warning, summary.into());
    }

    fn report_problem(
        &mut self,
        scope: StatusScope,
        summary: &str,
        detail: impl std::fmt::Display,
    ) {
        let severity = if matches!(scope, StatusScope::Save | StatusScope::Export) {
            StatusSeverity::Error
        } else {
            StatusSeverity::Warning
        };
        self.feedback.report(
            scope,
            StatusMessage::new(severity, summary).with_detail(detail.to_string()),
        );
        self.status = summary.into();
        self.status_severity = severity;
        self.status_until = Some(Instant::now() + Duration::from_secs(8));
    }

    fn resolve_problem(&mut self, scope: StatusScope) {
        if self
            .feedback
            .current()
            .is_some_and(|message| message.summary == self.status)
        {
            self.status = if self.editor.is_dirty() {
                "当前修改尚未保存"
            } else {
                "准备就绪"
            }
            .into();
            self.status_severity = StatusSeverity::Information;
        }
        self.feedback.resolve(scope);
    }

    fn edit_error(&mut self, error: &EditorSessionError) -> String {
        let message = crate::platform::editor::user_edit_error(error);
        self.feedback.remember(&message, &error.to_string());
        message
    }

    fn handle_window_error(&mut self, error: PreviewWindowError, edit_applied: bool) {
        match error {
            PreviewWindowError::Session(PreviewSessionError::PreparationCancelled) => {}
            PreviewWindowError::Editor(error)
            | PreviewWindowError::Session(PreviewSessionError::Editor(error)) => {
                let message = self.edit_error(&error);
                self.set_status(StatusSeverity::Warning, message);
            }
            PreviewWindowError::NoSelectedSegment => {
                self.set_status(StatusSeverity::Information, "请先选择要修改的镜头".into());
            }
            PreviewWindowError::Geometry(error) => {
                self.note_problem("焦点位置或倍率无效，请在画面内重新调整", error);
            }
            error => {
                self.log_media_error("preview", &error.to_string());
                self.deferred_playback.cancel();
                self.session.pause();
                self.report_problem(
                    StatusScope::Preview,
                    if edit_applied {
                        "修改已应用，预览尚未更新；请重新定位时间线，或先保存工程"
                    } else {
                        "画面暂时无法更新，请重新定位时间线；当前工程仍可保存"
                    },
                    error,
                );
            }
        }
    }

    fn control_layout(&self, client: RECT) -> ControlLayout {
        ControlLayout::new(client, self.theme)
            .with_caption_bounds(
                self.host_hwnd
                    .filter(|hwnd| {
                        client_rect(*hwnd).right == client.right
                            && window_dpi(*hwnd) == self.theme.dpi
                    })
                    .and(self.caption_bounds),
                self.theme,
            )
            .with_background_tab(self.inspector_image_tab)
            .with_inspector(
                self.selected_segment.is_some(),
                self.selected_video.is_some(),
                self.inspector_scroll,
                self.theme,
            )
            .with_video_groups(
                self.inspector_video_tab,
                self.inspector_scroll,
                self.theme,
                self.video_collapsed,
            )
    }
    #[allow(clippy::too_many_lines)]
    fn open(
        project_root: &Path,
        output: OutputDescriptor,
        smoke_duration: Option<Duration>,
        action: SmokeAction,
        telemetry: Arc<WindowTelemetry>,
    ) -> Result<Self, PreviewWindowError> {
        Self::open_with_recovery(
            project_root,
            output,
            smoke_duration,
            action,
            telemetry,
            true,
        )
    }

    fn open_with_recovery(
        project_root: &Path,
        output: OutputDescriptor,
        smoke_duration: Option<Duration>,
        action: SmokeAction,
        telemetry: Arc<WindowTelemetry>,
        restore_draft: bool,
    ) -> Result<Self, PreviewWindowError> {
        let loaded = crate::platform::editor_load::PreparedEditorLoad::prepare(
            project_root,
            restore_draft && smoke_duration.is_none(),
        )
        .map_err(|e| PreviewWindowError::Host(e.to_string()))?;
        Self::from_loaded(loaded, Some(output), smoke_duration, action, telemetry)
    }

    #[allow(clippy::too_many_lines)] // Session fields are initialized together before publication.
    fn from_loaded(
        loaded: crate::platform::editor_load::PreparedEditorLoad,
        output: Option<OutputDescriptor>,
        smoke_duration: Option<Duration>,
        action: SmokeAction,
        telemetry: Arc<WindowTelemetry>,
    ) -> Result<Self, PreviewWindowError> {
        let crate::platform::editor_load::PreparedEditorLoad {
            editor,
            preview,
            scrub_preview,
            project_name,
            recovery_notice,
            camera_planning_warning,
        } = loaded;
        let auto_play = action == SmokeAction::Play;
        let trim_smoke = action == SmokeAction::Trim;
        let queued_playback_smoke = action == SmokeAction::QueuedPlayback;
        let scrub_smoke = action == SmokeAction::Scrub;
        let mut session = ProjectPreviewSession::from_prepared(preview)?;
        if editor.settings().canvas.size.is_none()
            && let Some(output) = output
        {
            session.set_output(output);
        }
        // The worker prepared this exact frame; no media scan or decode wait on the UI thread.
        let frame = session.prepare_current()?;
        let selected_segment = if action == SmokeAction::None {
            None
        } else {
            editor
                .camera()
                .segments
                .first()
                .map(|segment| segment.id.clone())
        };
        let timeline = TimelineViewport::fit(editor.project_duration());
        if auto_play {
            session.play();
        }
        let now = Instant::now();
        Ok(Self {
            accessibility: crate::platform::accessibility::AccessibilityHost::default(),
            accessibility_focus: None,
            scrub_preview,
            thumbnails: crate::platform::timeline_thumbnails::TimelineThumbnails::default(),
            background_preview: std::cell::RefCell::default(),
            timer_resolution: None,
            frame_clock: None,
            session,
            inspector_image_tab: editor.settings().background.image.is_some(),
            inspector_video_tab: false,
            video_collapsed: [false; 2],
            crop_editing: false,
            crop_drag: None,
            crop_locked: false,
            editor,
            frame,
            fallback_frame: None,
            preview_surface: None,
            host_hwnd: None,
            caption_bounds: None,
            displayed_title: String::new(),
            project_name,
            selected_segment,
            selected_video: None,
            video_drag: None,
            snap_guide: None,
            export_task: None,
            export_output: None,
            export_panel: None,
            save_task: None,
            draft_manager: smoke_duration
                .is_none()
                .then(crate::platform::editor::DraftManager::default),
            recovery_notice,
            pending_navigation: None,
            navigation_authorized: false,
            navigation_prompt_open: false,
            #[cfg(test)]
            review_navigation_answer: None,
            generation: NEXT_EDITOR.fetch_add(1, Ordering::Relaxed),
            background_task: None,
            timeline_drag: None,
            timeline_scrub: None,
            canvas_drag: None,
            timeline,
            timeline_pan: None,
            timeline_resize: None,
            inspector_resize: None,
            inspector_width_dip: 288,
            inspector_scroll: 0,
            inspector_scroll_drag: None,
            property_drag: None,
            inline_input: None,
            inline_control: None,
            inline_validation: None,
            timeline_scroll: None,
            follow_playhead: true,
            wheel_edit_deadline: None,
            wheel_edit_pending: false,
            trim_smoke: if trim_smoke {
                TrimSmokeState::Pending
            } else {
                TrimSmokeState::Disabled
            },
            queued_playback_smoke: if queued_playback_smoke {
                QueuedPlaybackSmokeState::Pending
            } else {
                QueuedPlaybackSmokeState::Disabled
            },
            queued_playback_started: None,
            theme: EditorTheme::light(),
            hot_control: None,
            pressed_control: None,
            focused_control: None,
            keyboard_focus: false,
            tooltip_deadline: None,
            tooltip_visible: false,
            tracking_mouse_leave: false,
            deferred_playback: DeferredPlayback::None,
            last_drag_refresh: now,
            last_scrub_submit: now,
            last_scrub_time_refresh: now,
            scrub_message_pending: false,
            scrub_gate: ScrubPresentationGate::default(),
            scrub_smoke: scrub_smoke.then(ScrubSmoke::default),
            last_clock: now,
            last_title_refresh: now,
            status: if camera_planning_warning {
                "自动镜头生成失败，已保留原始视频 · 可继续剪辑并手动添加镜头".into()
            } else if auto_play {
                "正在运行自动播放测试".into()
            } else if smoke_duration.is_some() {
                "正在运行暂停状态测试".into()
            } else {
                "准备就绪".into()
            },
            status_severity: crate::platform::editor_ui::StatusSeverity::Information,
            feedback: StatusFeedback::default(),
            status_until: None,
            close_at: smoke_duration.map(|duration| now + duration),
            telemetry,
        })
    }

    fn initialize_preview_surface(
        &mut self,
        hwnd: HWND,
        instance: HINSTANCE,
    ) -> Result<(), PreviewWindowError> {
        self.host_hwnd = Some(hwnd);
        self.frame_clock =
            crate::platform::editor_clock::EditorClock::start(hwnd, WM_FRAME_CLOCK).ok();
        let window_handle = hwnd.0 as usize;
        let generation = self.generation;
        self.session.set_frame_ready_callback(move || {
            // Notification only; GPU composition and window state remain UI-owned.
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(window_handle as *mut c_void)),
                    WM_INTERACTIVE_RENDER,
                    WPARAM(generation),
                    LPARAM(0),
                )
            };
        });
        self.scrub_preview.set_frame_ready_callback(move || {
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(window_handle as *mut c_void)),
                    WM_SCRUB_READY,
                    WPARAM(generation),
                    LPARAM(0),
                )
            };
        });
        let started = Instant::now();
        match self.try_initialize_preview_surface(hwnd, instance) {
            Ok(surface) => {
                let overlay = self.focus_overlay();
                self.session
                    .present_prepared(&surface.presenter, &self.frame, overlay)?;
                self.preview_surface = Some(surface);
                self.fallback_frame = None;
                self.telemetry
                    .gpu_direct_preview
                    .store(true, Ordering::Relaxed);
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "GPU 直出预览已就绪".into(),
                );
            }
            Err(direct_error) => {
                let fallback = self.session.compose_prepared(&self.frame)?;
                self.fallback_frame = Some(fallback);
                self.preview_surface = None;
                self.feedback
                    .remember("已切换兼容预览", &direct_error.to_string());
                self.set_status(
                    StatusSeverity::Information,
                    "已切换兼容预览，可以继续编辑".into(),
                );
            }
        }
        self.record_displayed_frame(started.elapsed().as_micros() as u64);
        Ok(())
    }

    fn try_initialize_preview_surface(
        &self,
        parent: HWND,
        instance: HINSTANCE,
    ) -> Result<PreviewSurface, PreviewWindowError> {
        let class_name = w!("PanzoGpuPreviewSurface");
        register_preview_surface_class(instance, class_name)?;
        let rect = preview_video_rect(client_rect(parent), &self.frame, self.theme);
        let hwnd = win("CreateWindowExW(preview surface)", unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class_name,
                w!("Panzo GPU Preview"),
                WS_CHILD | WS_DISABLED,
                rect.left,
                rect.top,
                (rect.right - rect.left).max(1),
                (rect.bottom - rect.top).max(1),
                Some(parent),
                None,
                Some(instance),
                None,
            )
        })?;
        match self.session.create_presenter(hwnd) {
            Ok(presenter) => Ok(PreviewSurface { hwnd, presenter }),
            Err(error) => {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                Err(error.into())
            }
        }
    }

    fn focus_overlay(&self) -> Option<[f64; 2]> {
        if self.crop_editing {
            return None;
        }
        if let Some(drag) = &self.canvas_drag {
            let host = self.host_hwnd?;
            let video = preview_video_rect(client_rect(host), &self.frame, self.theme);
            let width = f64::from((video.right - video.left).max(1));
            let height = f64::from((video.bottom - video.top).max(1));
            return Some([
                (f64::from(drag.current_x - video.left) / width).clamp(0.0, 1.0),
                (f64::from(drag.current_y - video.top) / height).clamp(0.0, 1.0),
            ]);
        }
        if !self.editor.settings().camera_enabled
            || self.session.snapshot().playback == PlaybackState::Playing
        {
            return None;
        }
        self.selected_segment().map(|segment| {
            let focus = segment.focus_point();
            let point = self
                .frame
                .evaluation
                .camera_transform
                .source_to_output(focus.x, focus.y);
            let content = self.frame.content_rect(self.editor.settings());
            [
                content.left + point.x * (content.right - content.left),
                content.top + point.y * (content.bottom - content.top),
            ]
        })
    }

    fn present_focus_overlay(&self) -> Result<(), PreviewSessionError> {
        self.session
            .set_crop_overlay(self.crop_overlay(&self.frame));
        let Some(surface) = self.preview_surface.as_ref() else {
            return Ok(());
        };
        self.session
            .present_prepared(&surface.presenter, &self.frame, self.focus_overlay())?;
        update_cache_telemetry(&self.telemetry, &self.session);
        Ok(())
    }

    fn resize_preview_surface(&mut self, host: HWND) {
        if let Some(panel) = &mut self.export_panel {
            panel.relayout(host);
        }
        self.caption_bounds = editor_titlebar::caption_bounds(host);
        let rect = preview_video_rect(client_rect(host), &self.frame, self.theme);
        if let Some(surface) = self.preview_surface.as_ref() {
            if unsafe {
                MoveWindow(
                    surface.hwnd,
                    rect.left,
                    rect.top,
                    (rect.right - rect.left).max(1),
                    (rect.bottom - rect.top).max(1),
                    true,
                )
            }
            .is_err()
            {
                self.report_problem(
                    crate::platform::editor_ui::StatusScope::Preview,
                    "预览尺寸未更新，请再次调整窗口大小；当前工程仍可保存",
                    "错误：GPU 预览区域调整失败",
                );
            } else if let Err(error) = self.present_focus_overlay() {
                self.record_session_result(Err(error));
            }
        }
    }

    #[allow(clippy::too_many_lines)] // One ordered, nonblocking scheduling pass.
    fn on_timer(&mut self, hwnd: HWND) {
        if self
            .status_until
            .is_some_and(|until| Instant::now() >= until)
        {
            self.status_until = None;
            self.status = if self.editor.is_dirty() {
                "当前修改尚未保存"
            } else {
                "准备就绪"
            }
            .into();
            self.status_severity = StatusSeverity::Information;
            unsafe {
                let rect = self.control_layout(client_rect(hwnd)).status;
                let _ = InvalidateRect(Some(hwnd), Some(&raw const rect), false);
            }
        }
        crate::platform::editor_ui::sync_window_theme(&mut self.theme, hwnd);
        if self.session.snapshot().playback != PlaybackState::Playing {
            self.telemetry
                .cadence
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pause();
        }
        let active = self.timeline_scrub.is_some()
            || self.timeline_resize.is_some()
            || self.inspector_resize.is_some()
            || self.canvas_drag.is_some()
            || self.wheel_edit_pending
            || self.video_drag.is_some()
            || self.timeline_drag.is_some()
            || self.session.interactive_render_pending()
            || self.session.snapshot().playback == PlaybackState::Playing;
        if let Some(clock) = &self.frame_clock {
            clock.set_active(active);
        }
        if active && self.timer_resolution.is_none() {
            self.timer_resolution = crate::platform::timer_resolution::TimerResolution::acquire();
        } else if !active {
            self.timer_resolution = None;
        }
        self.poll_export();
        self.poll_background(hwnd);
        let controls = self.control_layout(client_rect(hwnd));
        let visible: Vec<_> = self
            .visible_thumbnails(controls.video_track)
            .iter()
            .map(|(_, tick)| *tick)
            .collect();
        self.scrub_preview
            .pause_preparation(active || self.export_task.is_some());
        if self.thumbnails.poll(
            self.scrub_preview.ready_proxy(),
            &visible,
            !active && self.export_task.is_none(),
        ) {
            unsafe {
                let _ = InvalidateRect(Some(hwnd), Some(&raw const controls.video_track.0), false);
            }
        }
        if self.poll_save(hwnd) {
            return;
        }
        self.advance_navigation(hwnd);
        if let Some(drafts) = &mut self.draft_manager {
            let can_write = self.save_task.is_none()
                && self.background_task.is_none()
                && !self.editor.has_edit_group();
            match drafts.poll(&self.editor, can_write) {
                Ok(true) => self.resolve_problem(StatusScope::RecoveryDraft),
                Ok(false) => {}
                Err(error) => self.report_problem(
                    crate::platform::editor_ui::StatusScope::RecoveryDraft,
                    "恢复副本暂未写入，请按 Ctrl+S 保存当前修改",
                    format!("错误：编辑恢复副本暂未保存 · {error}"),
                ),
            }
        }
        self.run_scrub_smoke(hwnd);
        let now = Instant::now();
        if !self.tooltip_visible
            && self
                .tooltip_deadline
                .is_some_and(|deadline| now >= deadline)
        {
            self.tooltip_visible = self.hot_control.is_some();
            self.tooltip_deadline = None;
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
        if self.trim_smoke == TrimSmokeState::Pending {
            self.trim_smoke = TrimSmokeState::Disabled;
            self.run_trim_smoke();
            self.refresh(hwnd, true);
        }
        if self.queued_playback_smoke == QueuedPlaybackSmokeState::Pending {
            self.queued_playback_smoke = QueuedPlaybackSmokeState::WaitingForPlayback;
            self.queued_playback_started = Some(Instant::now());
            let target = TimeTick(self.session.snapshot().duration_tick.as_i64() / 2);
            self.seek_absolute(target);
            self.toggle_playback_intent();
            if self.session.snapshot().playback == PlaybackState::Playing {
                self.complete_queued_playback_smoke();
            }
            self.refresh(hwnd, true);
        }
        if self
            .wheel_edit_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.finish_wheel_edit();
            self.refresh(hwnd, true);
        }
        self.refresh_drag_visual(hwnd);
        self.accept_scrub_preview(hwnd);
        self.auto_pan_gesture(hwnd);
        self.maybe_schedule_scrub_preview();
        self.telemetry.timer_ticks.fetch_add(1, Ordering::Relaxed);
        if self.session.interactive_render_pending() {
            self.schedule_interactive_poll();
        }
        let elapsed = now.saturating_duration_since(self.last_clock);
        self.last_clock = now;
        let was_playing = timer_requires_render(self.session.snapshot().playback);
        if was_playing {
            let result = duration_tick(elapsed)
                .map_err(PreviewWindowError::Clock)
                .and_then(|tick| self.session.advance(tick).map_err(Into::into))
                .and_then(|_| self.render().map_err(Into::into));
            self.record_result(result);
            if self.follow_playhead {
                self.timeline
                    .ensure_visible(self.session.snapshot().project_tick);
            }
            self.refresh(hwnd, false);
            if let (Some(clock), Some(delay)) =
                (&self.frame_clock, self.session.next_video_frame_delay())
            {
                clock.schedule_video_frame(delay.saturating_sub(now.elapsed()));
            }
        }
        if self.close_at.is_some_and(|deadline| now >= deadline) {
            self.request_navigation(hwnd, NavigationIntent::Quit);
        }
    }

    /// Test-only scripted input through the same handlers as the real timeline.
    fn run_scrub_smoke(&mut self, hwnd: HWND) {
        let Some(smoke) = self.scrub_smoke else {
            return;
        };
        if smoke.released {
            return;
        }
        let controls = self.control_layout(client_rect(hwnd));
        let track = controls.camera_track;
        let width = track.right - track.left;
        let start_x = track.left + width / 4;
        let y = i32::midpoint(track.top, track.bottom);
        let Some(started) = smoke.started else {
            self.begin_timeline_scrub(hwnd, track, start_x);
            self.scrub_smoke.as_mut().unwrap().started = Some(Instant::now());
            return;
        };
        let elapsed = started.elapsed().as_secs_f64();
        let drag_seconds = self
            .close_at
            .map_or(3.0, |end| {
                end.saturating_duration_since(started).as_secs_f64() - 2.0
            })
            .max(1.0);
        if elapsed >= drag_seconds {
            let smoke = self.scrub_smoke.as_mut().unwrap();
            smoke.released = true;
            smoke.target = Some(timeline_tick(track, start_x, self.timeline));
            // Set expectations before release, including a synchronous cache hit. Then
            // exercise the same playback intent as pressing Space immediately afterwards.
            self.queued_playback_smoke = QueuedPlaybackSmokeState::WaitingForPlayback;
            self.queued_playback_started = Some(Instant::now());
            self.on_mouse_up(hwnd, start_x, y);
            self.toggle_playback_intent();
            if self.session.snapshot().playback == PlaybackState::Playing {
                self.complete_queued_playback_smoke();
            }
            return;
        }
        let progress = scrub_probe_progress(elapsed, drag_seconds);
        let x = start_x + (f64::from(width) * 0.5 * progress) as i32;
        self.on_mouse_move(hwnd, x, y);
    }

    fn schedule_interactive_poll(&self) {
        if !self.session.interactive_render_pending() {
            return;
        }
        if let Some(hwnd) = self.host_hwnd {
            let _ = unsafe {
                PostMessageW(
                    Some(hwnd),
                    WM_INTERACTIVE_RENDER,
                    WPARAM(self.generation),
                    LPARAM(0),
                )
            };
        }
    }

    fn poll_interactive_render(&mut self, hwnd: HWND) {
        if !self.session.interactive_render_pending() {
            return;
        }
        let started = Instant::now();
        match self
            .session
            .poll_interactive_prepare(INTERACTIVE_SEEK_FRAME_BUDGET)
        {
            Ok(Some(frame)) => {
                if let Err(error) =
                    self.accept_prepared(frame, started.elapsed().as_micros() as u64)
                {
                    self.record_session_result(Err(error));
                    self.refresh(hwnd, true);
                    return;
                }
                if self.trim_smoke == TrimSmokeState::WaitingForPreview {
                    self.trim_smoke = TrimSmokeState::Disabled;
                    self.telemetry
                        .trim_preview_complete
                        .store(true, Ordering::Relaxed);
                }
                if self.timeline_scrub.is_none() && self.deferred_playback.take_requested() {
                    self.session.play();
                    self.last_clock = Instant::now();
                    self.set_status(
                        crate::platform::editor_ui::StatusSeverity::Information,
                        "正在播放".into(),
                    );
                    self.complete_queued_playback_smoke();
                } else if self.timeline_scrub.is_some() {
                    self.set_status(
                        crate::platform::editor_ui::StatusSeverity::Information,
                        "正在拖动播放头".into(),
                    );
                } else if self.session.snapshot().playback == PlaybackState::Playing {
                    self.set_status(
                        crate::platform::editor_ui::StatusSeverity::Information,
                        "正在播放".into(),
                    );
                } else {
                    self.set_status(
                        crate::platform::editor_ui::StatusSeverity::Information,
                        "准备就绪".into(),
                    );
                    // Pause/end can advance beyond the in-flight request. Settle exactly
                    // at the final controller position, including the last frame.
                    if self.frame.snapshot.project_tick != self.session.snapshot().project_tick {
                        let result = self.render();
                        self.record_session_result(result);
                    }
                }
                if self.timeline_scrub.is_some() {
                    if self.preview_surface.is_none() {
                        let video = preview_video_rect(client_rect(hwnd), &self.frame, self.theme);
                        unsafe {
                            let _ = InvalidateRect(Some(hwnd), Some(&raw const video), false);
                        }
                    }
                    self.maybe_schedule_scrub_preview();
                } else {
                    self.refresh(hwnd, true);
                }
            }
            // WM_TIMER polls again; self-reposting would starve paint and timer messages.
            Ok(None) => {}
            Err(error) => {
                self.record_session_result(Err(error));
                self.refresh(hwnd, true);
            }
        }
    }

    fn complete_queued_playback_smoke(&mut self) {
        if self.queued_playback_smoke != QueuedPlaybackSmokeState::WaitingForPlayback {
            return;
        }
        self.queued_playback_smoke = QueuedPlaybackSmokeState::Disabled;
        let elapsed = self
            .queued_playback_started
            .take()
            .map_or(0, |started| started.elapsed().as_micros() as u64);
        self.telemetry
            .queued_playback_micros
            .store(elapsed, Ordering::Relaxed);
        self.telemetry
            .queued_playback_completed
            .store(true, Ordering::Relaxed);
    }

    fn run_trim_smoke(&mut self) {
        let Some(segment) = self
            .editor
            .camera()
            .segments
            .iter()
            .max_by_key(|segment| segment.end_tick.as_i64() - segment.start_tick.as_i64())
            .cloned()
        else {
            self.note_problem(
                "此工程不满足修剪检查条件，请查看诊断详情",
                "错误：工程没有可修剪的镜头片段",
            );
            return;
        };
        let duration = segment.end_tick.as_i64() - segment.start_tick.as_i64();
        if duration < 4 {
            self.note_problem(
                "此工程不满足修剪检查条件，请查看诊断详情",
                "错误：镜头片段过短，无法运行修剪探针",
            );
            return;
        }
        let new_end_tick = TimeTick(segment.end_tick.as_i64() - duration / 4);
        let preview_tick = TimeTick(segment.start_tick.as_i64() + duration / 2);
        self.seek_absolute(preview_tick);

        let started = Instant::now();
        let result = self
            .editor
            .update_segment(&segment.id, segment.start_tick, new_end_tick, segment.to)
            .map_err(PreviewWindowError::from)
            .and_then(|()| self.apply_editor_state());
        let succeeded = result.is_ok();
        let incremental = succeeded && self.session.interactive_render_pending();
        self.telemetry
            .trim_action_micros
            .store(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        self.telemetry
            .trim_action_complete
            .store(succeeded, Ordering::Relaxed);
        self.telemetry
            .trim_preview_incremental
            .store(incremental, Ordering::Relaxed);
        self.trim_smoke = if incremental {
            TrimSmokeState::WaitingForPreview
        } else {
            TrimSmokeState::Disabled
        };
        if succeeded && !incremental {
            self.telemetry
                .trim_preview_complete
                .store(true, Ordering::Relaxed);
        }
        self.record_editor_result(result, "镜头长度修剪探针已提交");
    }

    #[allow(clippy::too_many_lines)] // Ordered keyboard routing: text, gestures, controls, then timeline.
    fn on_key(&mut self, hwnd: HWND, key: u16) {
        if key == VK_ESCAPE.0 && self.pending_navigation.is_some() {
            self.cancel_navigation();
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "已取消离开，当前任务完成后仍留在此工程".into(),
            );
            self.refresh(hwnd, true);
            return;
        }
        if self.navigation_authorized {
            return;
        }
        if key == VK_RETURN.0 && self.crop_editing && self.crop_drag.is_none() {
            self.toggle_crop_editing();
            self.refresh(hwnd, true);
            return;
        }
        if key == VK_ESCAPE.0
            && self.crop_editing
            && self.crop_drag.is_none()
            && self.property_drag.is_none()
        {
            self.toggle_crop_editing();
            self.refresh(hwnd, true);
            return;
        }
        if key == VK_ESCAPE.0 {
            self.cancel_gesture(hwnd);
            self.cancel_pressed_control(hwnd, true);
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "已取消当前操作".into(),
            );
            self.refresh(hwnd, true);
            return;
        }
        // A command must not mutate the model underneath an active pointer draft.
        if self.crop_drag.is_some()
            || self.property_drag.is_some()
            || self.video_drag.is_some()
            || self.timeline_drag.is_some()
            || self.canvas_drag.is_some()
        {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "请先松开鼠标完成调整，或按 Esc 取消".into(),
            );
            return;
        }
        self.finish_wheel_edit();
        let control_down = unsafe { GetKeyState(i32::from(VK_CONTROL.0)) } < 0;
        if key == VK_TAB.0 {
            let reverse = unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0;
            self.move_keyboard_focus(reverse);
            self.refresh(hwnd, true);
            return;
        }
        if self.keyboard_focus
            && matches!(key, value if value == VK_RETURN.0 || value == VK_SPACE.0)
            && let Some(control) = self
                .focused_control
                .filter(|control| self.control_enabled(*control))
        {
            self.activate_control(hwnd, control);
            self.refresh(hwnd, true);
            return;
        }
        if !control_down
            && self.keyboard_focus
            && matches!(key,k if k==VK_LEFT.0||k==VK_RIGHT.0)
            && let Some(control) = self.focused_control
            && let Some((value, maximum)) = self.widget_range(control)
        {
            let step = if control == ControlId::ZoomSlider {
                5.0
            } else if control == ControlId::VideoSpeed {
                0.05
            } else {
                1.0
            };
            self.apply_widget_range(
                hwnd,
                control,
                (value + if key == VK_RIGHT.0 { step } else { -step }).clamp(
                    if control == ControlId::VideoSpeed {
                        0.25
                    } else {
                        0.0
                    },
                    maximum,
                ),
            );
            return;
        }
        if control_down && key == u16::from(b'Z') {
            self.undo();
            self.refresh(hwnd, true);
            return;
        }
        if control_down && key == u16::from(b'Y') {
            self.redo();
            self.refresh(hwnd, true);
            return;
        }
        match key {
            value if value == VK_SPACE.0 => {
                self.toggle_playback_intent();
            }
            value if value == VK_HOME.0 => self.seek_absolute(TimeTick::ZERO),
            value if value == VK_LEFT.0 || value == VK_RIGHT.0 => {
                let forward = value == VK_RIGHT.0;
                if control_down {
                    self.seek_clip_boundary(forward);
                } else if unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0 {
                    self.seek_relative(if forward { SEEK_STEP.0 } else { -SEEK_STEP.0 });
                } else {
                    self.step_frame(forward);
                }
            }
            value if value == VK_DELETE.0 => self.delete_selected(),
            value if value == u16::from(b'B') => self.split_video(),
            value if value == u16::from(b'D') => {
                self.session.toggle_camera_diagnostics();
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "已切换镜头诊断".into(),
                );
            }
            value if value == u16::from(b'S') => self.save_editor(CameraEditKind::Update),
            value if value == u16::from(b'R') => self.reset_to_auto(),
            _ => {}
        }
        self.refresh(hwnd, true);
    }

    fn cancel_gesture(&mut self, hwnd: HWND) {
        self.snap_guide = None;
        if self.crop_drag.is_some() {
            self.finish_crop_drag(hwnd, true);
            return;
        }
        let changed = self.property_drag.take().is_some()
            || self.video_drag.take().is_some()
            || self.wheel_edit_pending
            || self.canvas_drag.is_some();
        self.inspector_scroll_drag = None;
        let scrub = self.timeline_scrub.take();
        self.timeline_drag = None;
        self.canvas_drag = None;
        if let Some((_, original)) = self.timeline_pan.take() {
            self.timeline = original;
        }
        if let Some((_, original)) = self.timeline_scroll.take() {
            self.timeline = original;
        }
        if let Some((_, original)) = self.timeline_resize.take() {
            self.theme.metrics.timeline_height = original;
            self.resize_preview_surface(hwnd);
        }
        if let Some((_, original)) = self.inspector_resize.take() {
            self.theme.metrics.inspector_width = original;
            self.resize_preview_surface(hwnd);
        }
        self.wheel_edit_deadline = None;
        self.wheel_edit_pending = false;
        self.editor.cancel_edit_group();
        if changed || scrub.is_some() {
            self.scrub_preview.cancel();
            self.scrub_gate.reset();
            self.deferred_playback.cancel();
            if let Some(scrub) = scrub {
                let result = self.session.seek(scrub.original_tick).map(|_| ());
                self.record_session_result(result);
            }
            let result = self.apply_editor_state();
            self.record_result(result);
        }
        // State is cleared before releasing capture: Windows sends a synchronous
        // WM_CAPTURECHANGED when capture is released.
        unsafe {
            let _ = ReleaseCapture();
        }
        self.refresh(hwnd, true);
    }

    fn move_keyboard_focus(&mut self, reverse: bool) {
        if let Some(hwnd) = self.host_hwnd {
            self.publish_accessibility(hwnd);
        }
        let layout = self.control_layout(self.host_hwnd.map_or(RECT::default(), client_rect));
        let mut order: Vec<_> = ControlId::ORDER
            .into_iter()
            .filter(|id| {
                let r = layout.raw_control_rect(*id);
                self.control_enabled(*id) && r.right > r.left && r.bottom > r.top
            })
            .map(|id| id as u32 + 1)
            .collect();
        for id in self.accessibility.focus_order() {
            if !order.contains(&id) {
                order.push(id);
            }
        }
        let current = self
            .accessibility_focus
            .or(self.focused_control.map(|id| id as u32 + 1));
        let next = next_keyboard_focus(&order, current, reverse, |_| true);
        self.focused_control = ControlId::ORDER
            .into_iter()
            .find(|control| Some(*control as u32 + 1) == next);
        self.accessibility_focus = self.focused_control.is_none().then_some(next).flatten();
        if let Some(id) = self.accessibility_focus {
            self.select_accessible_item(id, true);
        }
        self.keyboard_focus = true;
        if let Some(control) = self.focused_control
            && ControlLayout::is_inspector_control(control)
        {
            let r = layout.raw_control_rect(control);
            let shift = if r.top < layout.inspector_body_top {
                r.top - layout.inspector_body_top - self.theme.scale(8)
            } else if r.bottom > layout.panel.bottom {
                r.bottom - layout.panel.bottom + self.theme.scale(8)
            } else {
                0
            };
            self.inspector_scroll =
                (layout.inspector_scroll + shift).clamp(0, layout.inspector_scroll_max);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn on_mouse_down(&mut self, hwnd: HWND, x: i32, y: i32) {
        if self.navigation_authorized {
            return;
        }
        if !self.finish_inline_input(hwnd, true) {
            return;
        }
        self.finish_wheel_edit();
        unsafe {
            let _ = SetFocus(Some(hwnd));
        }
        self.keyboard_focus = false;
        self.accessibility_focus = None;
        self.cancel_pressed_control(hwnd, true);
        let client = client_rect(hwnd);
        let controls = self.control_layout(client);
        if controls.inspector_splitter.contains(x, y) {
            self.inspector_resize = Some((x, controls.panel.right - controls.panel.left));
            self.tooltip_visible = false;
            self.tooltip_deadline = None;
            unsafe {
                SetCapture(hwnd);
            }
            self.update_cursor(hwnd, x, y);
            return;
        }
        if controls.inspector_scroll_max > 0 && controls.inspector_scrollbar.contains(x, y) {
            let track = controls.inspector_scrollbar;
            let height = (track.bottom - track.top).max(1);
            let thumb = (height * height / (height + controls.inspector_scroll_max))
                .max(self.theme.scale(28));
            self.inspector_scroll = ((y - track.top - thumb / 2) * controls.inspector_scroll_max
                / (height - thumb).max(1))
            .clamp(0, controls.inspector_scroll_max);
            self.inspector_scroll_drag = Some((y, self.inspector_scroll));
            unsafe {
                SetCapture(hwnd);
            }
            self.refresh(hwnd, true);
            return;
        }
        if controls.splitter.contains(x, y) {
            self.timeline_resize = Some((
                y,
                controls.timeline_panel.bottom - controls.timeline_panel.top,
            ));
            unsafe {
                SetCapture(hwnd);
            }
            return;
        }
        if controls.scrollbar.contains(x, y) && !self.timeline.is_fit() {
            let thumb = self.scrollbar_thumb(controls.scrollbar);
            if !thumb.contains(x, y) {
                let fit = TimelineViewport::fit(self.editor.project_duration());
                let at = fit.offset_to_tick(
                    x - controls.scrollbar.left,
                    controls.scrollbar.right - controls.scrollbar.left,
                );
                self.timeline.pan_by(TimeTick(
                    at.0 - self.timeline.visible_start().0 - self.timeline.visible_duration().0 / 2,
                ));
            }
            self.timeline_scroll = Some((x, self.timeline));
            self.follow_playhead = false;
            unsafe {
                SetCapture(hwnd);
            }
            self.refresh(hwnd, true);
            return;
        }
        if let Some(control) = controls.hit_control(x, y)
            && self.begin_property_drag(hwnd, control, x, y)
        {
            return;
        }
        self.pressed_control = controls
            .hit_control(x, y)
            .filter(|control| self.control_enabled(*control));
        if let Some(control) = self.pressed_control {
            self.focused_control = Some(control);
            self.hot_control = Some(control);
            let rect = controls.raw_control_rect(control).0;
            unsafe {
                SetCapture(hwnd);
                let _ = InvalidateRect(Some(hwnd), Some(&raw const rect), false);
            }
            self.refresh(hwnd, true);
            return;
        }
        if self.playhead_hit(&controls, x, y) {
            self.begin_timeline_scrub(hwnd, controls.camera_track, x);
            return;
        }
        if self.begin_crop_drag(hwnd, x, y) {
            return;
        }
        let video = preview_video_rect(client, &self.frame, self.theme);
        if HitRect(video).contains(x, y) {
            if let Some(original) = self.selected_segment() {
                self.session.pause();
                self.deferred_playback.cancel();
                self.session.cancel_interactive_render();
                self.editor.begin_edit_group();
                self.canvas_drag = Some(CanvasDrag {
                    current_x: x,
                    current_y: y,
                    original,
                    initial_transform: self.frame.evaluation.camera_transform,
                });
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "拖动焦点调整位置，滚轮调整缩放".into(),
                );
                unsafe {
                    SetCapture(hwnd);
                }
            } else {
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "请先选择一个镜头片段".into(),
                );
            }
        } else if controls.video_track.contains(x, y) {
            if self.crop_editing {
                self.toggle_crop_editing();
            }
            let video = self.editor.video_edit();
            if let Some(span) = video.clip_at(timeline_tick(controls.video_track, x, self.timeline))
            {
                self.inspector_scroll = 0;
                self.selected_video = Some(span.clip.id.clone());
                self.inspector_video_tab = true;
                self.selected_segment = None;
                let left = timeline_x(controls.video_track, span.project_in, self.timeline);
                let right = timeline_x(controls.video_track, span.project_out, self.timeline);
                let edge_mode = timeline_edge_mode(x, left, right, self.theme.scale(8));
                if edge_mode != TimelineDragMode::Move {
                    self.session.pause();
                    self.deferred_playback.cancel();
                    self.editor.begin_edit_group();
                    self.video_drag = Some(VideoDrag {
                        original: span.clip.clone(),
                        start_x: x,
                        current_x: x,
                        view_start: self.timeline.visible_start(),
                        playhead_source: panzo_core::TimeMapping::source_time(
                            &video,
                            self.session.snapshot().project_tick,
                        )
                        .unwrap_or(TimeTick::ZERO),
                        trim_start: edge_mode == TimelineDragMode::TrimStart,
                    });
                    unsafe {
                        SetCapture(hwnd);
                    }
                }
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Success,
                    "已选择视频 · B 分割 · Delete 删除 · 拖动两端裁剪".into(),
                );
            }
            self.refresh(hwnd, true);
        } else if controls.ruler.contains(x, y) || controls.click_track.contains(x, y) {
            self.begin_timeline_scrub(hwnd, controls.camera_track, x);
        } else if controls.camera_track.contains(x, y) {
            if self.crop_editing {
                self.toggle_crop_editing();
            }
            let playhead_tick = self
                .timeline_scrub
                .map_or(self.session.snapshot().project_tick, |scrub| {
                    scrub.desired_tick
                });
            let playhead_x = timeline_x(controls.camera_track, playhead_tick, self.timeline);
            if (x - playhead_x).abs() <= self.theme.scale(7) {
                self.begin_timeline_scrub(hwnd, controls.camera_track, x);
            } else if let Some((segment, rect)) = self.segment_at_x(controls.camera_track, x) {
                self.session.pause();
                self.deferred_playback.cancel();
                self.last_clock = Instant::now();
                self.last_drag_refresh = self
                    .last_clock
                    .checked_sub(DRAG_REFRESH_INTERVAL)
                    .unwrap_or(self.last_clock);
                self.selected_video = None;
                self.inspector_scroll = 0;
                self.selected_segment = Some(segment.id.clone());
                self.inspector_video_tab = false;
                let mode = timeline_edge_mode(x, rect.left, rect.right, self.theme.scale(8));
                self.timeline_drag = Some(TimelineDrag {
                    mode,
                    start_x: x,
                    current_x: x,
                    original: segment,
                    grab_source: panzo_core::TimeMapping::source_time(
                        &self.editor.video_edit(),
                        timeline_tick(controls.camera_track, x, self.timeline),
                    )
                    .unwrap_or(TimeTick::ZERO),
                });
                unsafe {
                    SetCapture(hwnd);
                }
            } else {
                self.begin_timeline_scrub(hwnd, controls.camera_track, x);
            }
        }
        self.refresh(hwnd, true);
    }

    #[allow(clippy::too_many_lines)]
    fn activate_control(&mut self, hwnd: HWND, control: ControlId) {
        if !self.control_enabled(control) {
            return;
        }
        if control != ControlId::CameraEnabled {
            self.accessibility.invoked(control as u32 + 1);
        }
        match control {
            ControlId::SpeedSection | ControlId::CropSection => {
                let index = usize::from(control == ControlId::CropSection);
                self.video_collapsed[index] = !self.video_collapsed[index];
                self.inspector_scroll = 0;
                self.persist_layout();
            }
            ControlId::CanvasOriginal
            | ControlId::CanvasWide
            | ControlId::CanvasPortrait
            | ControlId::CanvasSquare => self.set_canvas_preset(control),
            ControlId::CanvasWidth | ControlId::CanvasHeight => {
                self.begin_inline_input(hwnd, control);
            }
            ControlId::PreviousFrame | ControlId::NextFrame => {
                self.step_frame(control == ControlId::NextFrame);
            }
            ControlId::CropEdit => self.toggle_crop_editing(),
            ControlId::CropLock => {
                self.crop_locked = !self.crop_locked;
            }
            ControlId::WindowMinimize | ControlId::WindowMaximize => unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{
                    IsZoomed, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, ShowWindow,
                };
                let mode = if control == ControlId::WindowMinimize {
                    SW_MINIMIZE
                } else if IsZoomed(hwnd).as_bool() {
                    SW_RESTORE
                } else {
                    SW_MAXIMIZE
                };
                let _ = ShowWindow(hwnd, mode);
            },
            ControlId::Menu => self.application_menu(hwnd),
            ControlId::Back | ControlId::NewRecording => {
                self.request_navigation(hwnd, NavigationIntent::NewRecording);
            }
            ControlId::OpenProject => {
                if let Some(project) = crate::platform::recorder_window::choose_project_path(hwnd) {
                    self.request_navigation(hwnd, NavigationIntent::OpenProject(project));
                }
            }
            ControlId::WindowClose => unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            ControlId::CropReset
            | ControlId::CropWide
            | ControlId::CropStandard
            | ControlId::CropSquare
            | ControlId::CropPortrait => self.apply_crop_preset(hwnd, control),
            ControlId::CanvasTab => {
                if self.crop_editing {
                    self.toggle_crop_editing();
                }
                self.inspector_video_tab = false;
                self.inspector_scroll = 0;
            }
            ControlId::VideoTab => self.select_video_properties(),
            ControlId::SpeedReset
            | ControlId::SpeedHalf
            | ControlId::SpeedNormal
            | ControlId::SpeedOneHalf
            | ControlId::SpeedDouble
            | ControlId::SpeedQuadruple => {
                let speed = editor_widgets::SPEED_PRESETS
                    .into_iter()
                    .find(|(id, _)| *id == control)
                    .map_or(1.0, |(_, speed)| speed);
                self.apply_widget_range(hwnd, ControlId::VideoSpeed, speed);
            }
            ControlId::InspectorToggle => {
                self.theme.metrics.inspector_width = if self.theme.metrics.inspector_width == 0 {
                    self.theme.scale(self.inspector_width_dip)
                } else {
                    self.inspector_width_dip =
                        self.theme.metrics.inspector_width * 96 / self.theme.dpi.cast_signed();
                    0
                };
                self.resize_preview_surface(hwnd);
            }
            ControlId::StartValue
            | ControlId::EndValue
            | ControlId::ScaleValue
            | ControlId::Inset
            | ControlId::Radius
            | ControlId::VideoSpeed
            | ControlId::CropLeft
            | ControlId::CropTop
            | ControlId::CropRight
            | ControlId::CropBottom
            | ControlId::Shadow => self.begin_inline_input(hwnd, control),
            ControlId::CenterFocus => {
                if let Some(segment) = self.selected_segment() {
                    let focus = segment.focus_point();
                    self.adjust_target(0.5 - focus.x, 0.5 - focus.y, 0.0);
                }
            }
            ControlId::SwatchMist
            | ControlId::SwatchPeach
            | ControlId::SwatchLilac
            | ControlId::SwatchCharcoal
            | ControlId::SwatchWhite
            | ControlId::SwatchSage => {
                let result = self
                    .editor
                    .set_solid_background(editor_widgets::swatch_color(control));
                self.record_result(result.map_err(Into::into));
                let result = self.apply_editor_state();
                self.record_result(result);
            }
            ControlId::Cursor => {
                let settings = self.editor.settings().cursor;
                let result = self
                    .editor
                    .set_cursor_style(!settings.visible, settings.scale);
                self.record_result(result.map_err(Into::into));
                let result = self.apply_editor_state();
                self.record_result(result);
            }
            ControlId::ZoomSlider => self
                .timeline
                .zoom_at(self.session.snapshot().project_tick, 2.0),
            ControlId::Split => self.split_video(),
            ControlId::Background => {
                self.inspector_image_tab = false;
            }
            ControlId::VideoTrim
            | ControlId::CameraProperties
            | ControlId::BackgroundColor
            | ControlId::CursorProperties => {
                self.edit_properties(hwnd, control);
            }
            ControlId::CameraEnabled => {
                self.editor
                    .set_camera_enabled(!self.editor.settings().camera_enabled);
                let result = self.apply_editor_state();
                self.record_editor_result(result, "镜头效果已切换，镜头数据保留");
            }
            ControlId::Export => self.begin_export(hwnd),
            ControlId::Location => self.show_export_location(),
            ControlId::Home => self.seek_absolute(TimeTick::ZERO),
            ControlId::Play => self.toggle_playback_intent(),
            ControlId::Save => self.save_editor(CameraEditKind::Update),
            ControlId::Undo => self.undo(),
            ControlId::Redo => self.redo(),
            ControlId::Add => self.add_manual_segment(),
            ControlId::Delete => self.delete_selected(),
            ControlId::Reset => self.reset_to_auto(),
            ControlId::Diagnostics => {
                self.appearance_menu(hwnd);
            }
            ControlId::TimelineZoomOut => {
                self.timeline
                    .zoom_at(self.session.snapshot().project_tick, 0.5);
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "时间线已缩小".into(),
                );
            }
            ControlId::TimelineFit => {
                self.follow_playhead = true;
                self.timeline.reset();
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "时间线已适配项目".into(),
                );
            }
            ControlId::TimelineZoomIn => {
                self.timeline
                    .zoom_at(self.session.snapshot().project_tick, 2.0);
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "时间线已放大".into(),
                );
            }
            ControlId::ScaleMinus => self.adjust_target(0.0, 0.0, -0.1),
            ControlId::ScalePlus => self.adjust_target(0.0, 0.0, 0.1),
            ControlId::FocusLeft => self.adjust_target(-0.02, 0.0, 0.0),
            ControlId::FocusRight => self.adjust_target(0.02, 0.0, 0.0),
            ControlId::FocusUp => self.adjust_target(0.0, -0.02, 0.0),
            ControlId::FocusDown => self.adjust_target(0.0, 0.02, 0.0),
            ControlId::StartMinus => {
                self.adjust_timing(TimeTick(-EDIT_STEP.as_i64()), TimeTick::ZERO);
            }
            ControlId::StartPlus => self.adjust_timing(EDIT_STEP, TimeTick::ZERO),
            ControlId::EndMinus => {
                self.adjust_timing(TimeTick::ZERO, TimeTick(-EDIT_STEP.as_i64()));
            }
            ControlId::EndPlus => self.adjust_timing(TimeTick::ZERO, EDIT_STEP),
            ControlId::BackgroundImage => self.inspector_image_tab = true,
            ControlId::BackgroundFile => self.import_background(hwnd),
        }
    }

    fn begin_timeline_scrub(&mut self, hwnd: HWND, track: HitRect, x: i32) {
        if self.crop_editing {
            self.toggle_crop_editing();
        }
        self.telemetry
            .scrub_metrics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .start(Instant::now());
        let original_tick = self.session.snapshot().project_tick;
        let resume_after_scrub = self.session.snapshot().playback == PlaybackState::Playing
            || self.deferred_playback.is_requested();
        self.session.pause();
        self.session.cancel_interactive_render();
        self.scrub_gate.reset();
        self.scrub_preview.cancel();
        if resume_after_scrub {
            self.deferred_playback.request();
        }
        let desired_tick = timeline_tick(track, x, self.timeline);
        self.scrub_gate.move_to(desired_tick);
        self.timeline_scrub = Some(TimelineScrub {
            original_tick,
            current_x: x,
            desired_tick,
            submitted_tick: None,
        });
        self.set_status(
            crate::platform::editor_ui::StatusSeverity::Information,
            "正在拖动播放头".into(),
        );
        self.last_scrub_submit = Instant::now()
            .checked_sub(SCRUB_PREVIEW_INTERVAL)
            .unwrap_or_else(Instant::now);
        self.last_scrub_time_refresh = Instant::now()
            .checked_sub(SCRUB_TIME_REFRESH_INTERVAL)
            .unwrap_or_else(Instant::now);
        unsafe {
            SetCapture(hwnd);
        }
        self.refresh_drag_visual(hwnd);
        self.maybe_schedule_scrub_preview();
    }

    fn maybe_schedule_scrub_preview(&mut self) {
        if self.scrub_message_pending {
            return;
        }
        let now = Instant::now();
        if now.saturating_duration_since(self.last_scrub_submit) < SCRUB_PREVIEW_INTERVAL {
            return;
        }
        if !self
            .timeline_scrub
            .is_some_and(TimelineScrub::needs_preview)
        {
            return;
        }
        let Some(hwnd) = self.host_hwnd else {
            return;
        };
        self.last_scrub_submit = now;
        self.scrub_message_pending = true;
        if unsafe {
            PostMessageW(
                Some(hwnd),
                WM_SCRUB_PREVIEW,
                WPARAM(self.generation),
                LPARAM(0),
            )
        }
        .is_err()
        {
            self.scrub_message_pending = false;
        }
    }

    fn submit_scrub_preview(&mut self, hwnd: HWND) {
        self.scrub_message_pending = false;
        let Some(target) = self
            .timeline_scrub
            .as_mut()
            .and_then(TimelineScrub::take_latest_preview)
        else {
            return;
        };
        let result = self
            .session
            .seek(target)
            .and_then(|_| self.session.scrub_decode_plan());
        self.last_clock = Instant::now();
        match result {
            Ok(plan) => match self.scrub_preview.submit(plan) {
                Ok(generation) => {
                    self.scrub_gate.submitted(generation);
                }
                Err(error) => {
                    self.report_problem(
                        crate::platform::editor_ui::StatusScope::Preview,
                        "预览暂未更新，请重新定位时间线；当前工程仍可保存",
                        format!("错误：{error}"),
                    );
                    self.refresh(hwnd, true);
                }
            },
            Err(error) => {
                self.record_session_result(Err(error));
                self.refresh(hwnd, true);
            }
        }
    }

    #[allow(clippy::too_many_lines)] // One ordered presentation pass with explicit failure status.
    fn accept_scrub_preview(&mut self, hwnd: HWND) {
        let Some(result) = self.scrub_preview.take_latest() else {
            return;
        };
        if self.timeline_scrub.is_none()
            || !self
                .scrub_gate
                .accept(result.generation, result.submitted_at.elapsed())
        {
            return;
        }
        let frame = match result.frame {
            Ok(frame) => frame,
            Err(error) => {
                self.log_media_error("scrub-decode", &error);
                self.session.pause();
                self.deferred_playback.cancel();
                self.report_problem(
                    crate::platform::editor_ui::StatusScope::Preview,
                    "此处画面暂时无法读取，请重新定位时间线；当前工程仍可保存",
                    format!("错误：{error}"),
                );
                self.refresh(hwnd, true);
                return;
            }
        };
        let video = self.editor.video_edit();
        if let Some(span) = video.clip_at(result.project_tick)
            && (frame.pts < span.clip.source_in_tick || frame.pts >= span.clip.source_out_tick)
        {
            // Never flash a proxy frame from a deleted source interval at a cut.
            return;
        }
        let changed = self.frame.evaluation.source_frame.presentation_tick != frame.pts;
        let requested_source = panzo_core::TimeMapping::source_time(&video, result.project_tick)
            .unwrap_or(result.project_tick);
        let time_error = requested_source.as_i64().abs_diff(frame.pts.as_i64());
        let started = Instant::now();
        if frame
            .native_size
            .is_some_and(|size| size != (frame.width, frame.height))
        {
            self.telemetry
                .scrub_reduced_frames
                .fetch_add(1, Ordering::Relaxed);
        }
        let prepared = self.session.prepare_external_scrub_frame(frame);
        match prepared
            .and_then(|frame| self.accept_prepared(frame, started.elapsed().as_micros() as u64))
        {
            Ok(()) => {
                if changed {
                    self.telemetry
                        .scrub_distinct_frames
                        .fetch_add(1, Ordering::Relaxed);
                }
                self.telemetry
                    .scrub_metrics
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .present(
                        Instant::now(),
                        result.submitted_at.elapsed().as_micros() as u64,
                        time_error,
                        changed,
                    );
                self.telemetry
                    .scrub_max_time_error_tick
                    .fetch_max(time_error, Ordering::Relaxed);
                self.telemetry
                    .scrub_presented_frames
                    .fetch_add(1, Ordering::Relaxed);
                if let Some(smoke) = &mut self.scrub_smoke {
                    let now = Instant::now();
                    if let Some(last) = smoke.last_presented.or(smoke.started) {
                        let gap = now.duration_since(last);
                        if gap > Duration::from_millis(150) {
                            eprintln!(
                                "Scrub slow presentation: gapUs={} first={} requestAgeUs={} presentUs={} projectTick={}",
                                gap.as_micros(),
                                smoke.last_presented.is_none(),
                                result.submitted_at.elapsed().as_micros(),
                                started.elapsed().as_micros(),
                                result.project_tick.as_i64()
                            );
                        }
                        self.telemetry.scrub_max_gap_micros.fetch_max(
                            now.duration_since(last).as_micros() as u64,
                            Ordering::Relaxed,
                        );
                    }
                    smoke.last_presented = Some(now);
                }
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "正在拖动播放头".into(),
                );
                if self.preview_surface.is_none() {
                    let video = preview_video_rect(client_rect(hwnd), &self.frame, self.theme);
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), Some(&raw const video), false);
                    }
                }
            }
            Err(error) => {
                self.record_session_result(Err(error));
                self.refresh(hwnd, true);
            }
        }
    }

    fn auto_pan_gesture(&mut self, hwnd: HWND) {
        let x = self
            .timeline_scrub
            .map(|s| s.current_x)
            .or_else(|| self.timeline_drag.as_ref().map(|d| d.current_x))
            .or_else(|| self.video_drag.as_ref().map(|d| d.current_x));
        let Some(x) = x else {
            return;
        };
        if self.timeline.is_fit() {
            return;
        }
        let controls = self.control_layout(client_rect(hwnd));
        let zone = self.theme.scale(18);
        let direction = if x < controls.camera_track.left + zone {
            -1
        } else {
            i64::from(x > controls.camera_track.right - zone)
        };
        if direction != 0 {
            let before = self.timeline.visible_start();
            // Bounded at 2/3 of a viewport per second at the 60 Hz frame clock.
            self.timeline.pan_by(TimeTick(
                self.timeline.visible_duration().0 / 90 * direction,
            ));
            if before != self.timeline.visible_start() {
                self.follow_playhead = false;
                self.on_mouse_move(hwnd, x, controls.camera_track.top);
                self.refresh(hwnd, true);
            }
        }
    }

    #[allow(clippy::too_many_lines)] // Mutually exclusive gestures; each returns before the next handler.
    fn on_mouse_move(&mut self, hwnd: HWND, x: i32, y: i32) {
        self.update_cursor(hwnd, x, y);
        if self.crop_drag.is_some() {
            self.update_crop_drag(hwnd, x, y);
            return;
        }
        if let Some((start_x, original)) = self.inspector_resize {
            self.theme.metrics.inspector_width = ControlLayout::clamp_inspector_width(
                client_rect(hwnd).right,
                (original + start_x - x).max(1),
                self.theme,
            );
            if self.last_drag_refresh.elapsed() >= DRAG_REFRESH_INTERVAL {
                self.last_drag_refresh = Instant::now();
                self.resize_preview_surface(hwnd);
                self.refresh(hwnd, true);
            }
            return;
        }
        if let Some((start_y, original)) = self.inspector_scroll_drag {
            let controls = self.control_layout(client_rect(hwnd));
            let height =
                (controls.inspector_scrollbar.bottom - controls.inspector_scrollbar.top).max(1);
            let thumb = (height * height / (height + controls.inspector_scroll_max))
                .max(self.theme.scale(28));
            self.inspector_scroll = (original
                + (y - start_y) * controls.inspector_scroll_max / (height - thumb).max(1))
            .clamp(0, controls.inspector_scroll_max);
            self.refresh(hwnd, true);
            return;
        }
        if let Some(control) = self.property_drag {
            self.update_property_drag(hwnd, control, x);
            return;
        }
        if let Some((start_y, height)) = self.timeline_resize {
            self.theme.metrics.timeline_height = ControlLayout::clamp_timeline_height(
                client_rect(hwnd).bottom,
                height + start_y - y,
                self.theme,
            );
            if self.last_drag_refresh.elapsed() >= DRAG_REFRESH_INTERVAL {
                self.last_drag_refresh = Instant::now();
                self.resize_preview_surface(hwnd);
                self.refresh(hwnd, true);
            }
            return;
        }
        if let Some((start_x, original)) = self.timeline_scroll {
            let controls = self.control_layout(client_rect(hwnd));
            let thumb = self.scrollbar_thumb(controls.scrollbar);
            let travel = ((controls.scrollbar.right - controls.scrollbar.left)
                - (thumb.right - thumb.left))
                .max(1);
            let delta =
                (i128::from(self.editor.project_duration().0 - original.visible_duration().0)
                    * i128::from(x - start_x)
                    / i128::from(travel)) as i64;
            self.timeline = original;
            self.timeline.pan_by(TimeTick(delta));
            self.refresh(hwnd, true);
            return;
        }
        if let Some((start_x, original)) = self.timeline_pan {
            let controls = self.control_layout(client_rect(hwnd));
            self.timeline = original;
            self.timeline.pan_by(original.pixel_delta_to_tick(
                start_x - x,
                controls.camera_track.right - controls.camera_track.left,
            ));
            self.follow_playhead = false;
            unsafe {
                let _ = InvalidateRect(Some(hwnd), Some(&raw const controls.timeline_panel), false);
            }
            return;
        }
        if let Some(drag) = self.video_drag.clone() {
            let controls = self.control_layout(client_rect(hwnd));
            if let Some(active) = &mut self.video_drag {
                active.current_x = x;
            }
            let project_delta =
                timeline_delta(controls.video_track, x - drag.start_x, self.timeline).0
                    + self.timeline.visible_start().0
                    - drag.view_start.0;
            let delta = drag.original.source_delta(TimeTick(project_delta)).0;
            let video = self.editor.video_edit();
            let Some(index) = video.clips.iter().position(|c| c.id == drag.original.id) else {
                self.cancel_gesture(hwnd);
                return;
            };
            let lower = if index == 0 {
                0
            } else {
                video.clips[index - 1].source_out_tick.0
            };
            let upper = video
                .clips
                .get(index + 1)
                .map_or(video.source_duration_tick.0, |c| c.source_in_tick.0);
            let alt = unsafe { GetKeyState(0x12) } < 0;
            let tolerance = if alt {
                0
            } else {
                drag.original
                    .source_delta(timeline_delta(
                        controls.video_track,
                        self.theme.scale(6),
                        self.timeline,
                    ))
                    .0
                    .unsigned_abs()
            };
            let (start, end) = crate::platform::edit_gestures::adjusted_range(
                (
                    drag.original.source_in_tick.0,
                    drag.original.source_out_tick.0,
                ),
                delta,
                (lower, upper),
                if drag.trim_start {
                    TimelineDragMode::TrimStart
                } else {
                    TimelineDragMode::TrimEnd
                },
                &[lower, upper, drag.playhead_source.0],
                tolerance,
            );
            let (start, end) = (TimeTick(start), TimeTick(end));
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                format!(
                    "裁剪时长 {} · Alt 取消吸附 · Esc 取消",
                    format_tick(TimeTick(end.0 - start.0))
                ),
            );
            if let Err(error) = self.editor.trim_video(&drag.original.id, start, end) {
                let message = self.edit_error(&error);
                self.set_status(StatusSeverity::Warning, message);
            }
            let edge = if drag.trim_start { start.0 } else { end.0 };
            self.snap_guide =
                if tolerance > 0 && [lower, upper, drag.playhead_source.0].contains(&edge) {
                    self.editor
                        .video_edit()
                        .spans()
                        .find(|span| span.clip.id == drag.original.id)
                        .map(|span| {
                            if drag.trim_start {
                                span.project_in
                            } else {
                                span.project_out
                            }
                        })
                } else {
                    None
                };

            unsafe {
                let _ = InvalidateRect(Some(hwnd), Some(&raw const controls.timeline_panel), false);
            }
            return;
        }
        self.update_pointer_state(hwnd, x, y);
        if let Some(drag) = &mut self.timeline_drag {
            drag.current_x = x;
        }
        if let Some(drag) = &mut self.canvas_drag {
            drag.current_x = x;
            drag.current_y = y;
        }
        if let Some(scrub) = &mut self.timeline_scrub {
            let controls = ControlLayout::new(client_rect(hwnd), self.theme);
            let target = timeline_tick(controls.camera_track, x, self.timeline);
            scrub.update(x, target);
            if self.scrub_gate.move_to(target) {
                self.scrub_preview.cancel();
                scrub.submitted_tick = None;
            }
        }
        self.refresh_drag_visual(hwnd);
        self.maybe_schedule_scrub_preview();
    }

    fn refresh_drag_visual(&mut self, hwnd: HWND) {
        let now = Instant::now();
        if (self.timeline_drag.is_some()
            || self.timeline_scrub.is_some()
            || self.canvas_drag.is_some())
            && now.saturating_duration_since(self.last_drag_refresh) >= DRAG_REFRESH_INTERVAL
        {
            self.last_drag_refresh = now;
            if let Some(drag) = self.canvas_drag.clone() {
                let result = self.canvas_focus(hwnd, &drag).and_then(|focus| {
                    self.editor.set_segment_focus(
                        &drag.original.id,
                        focus,
                        drag.original.to.scale,
                    )?;
                    self.apply_editor_state()
                });
                self.record_result(result);
            }
            let region = if self.timeline_drag.is_some() || self.timeline_scrub.is_some() {
                let controls = self.control_layout(client_rect(hwnd));
                RECT {
                    left: controls.camera_track.left - 3,
                    top: controls.ruler.top - 3,
                    right: controls.camera_track.right + 3,
                    bottom: controls.camera_track.bottom + 3,
                }
            } else {
                preview_video_rect(client_rect(hwnd), &self.frame, self.theme)
            };
            unsafe {
                let _ = InvalidateRect(Some(hwnd), Some(&raw const region), false);
                let _ = UpdateWindow(hwnd);
            }
            if self.timeline_scrub.is_some()
                && now.saturating_duration_since(self.last_scrub_time_refresh)
                    >= SCRUB_TIME_REFRESH_INTERVAL
            {
                self.last_scrub_time_refresh = now;
                if now.saturating_duration_since(self.last_title_refresh)
                    >= Duration::from_millis(100)
                {
                    self.update_window_title(hwnd);
                    self.last_title_refresh = now;
                }
                let controls = self.control_layout(client_rect(hwnd));
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), Some(&raw const controls.time), false);
                    let _ = UpdateWindow(hwnd);
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)] // Release each exclusive gesture before releasing capture.
    fn on_mouse_up(&mut self, hwnd: HWND, x: i32, y: i32) {
        self.snap_guide = None;
        if self.crop_drag.is_some() {
            self.update_crop_drag(hwnd, x, y);
            self.finish_crop_drag(hwnd, false);
            return;
        }
        if self.inspector_resize.take().is_some() {
            self.inspector_width_dip =
                self.theme.metrics.inspector_width * 96 / self.theme.dpi.cast_signed();
            unsafe {
                let _ = ReleaseCapture();
            }
            self.persist_layout();
            self.resize_preview_surface(hwnd);
            self.refresh(hwnd, true);
            return;
        }
        if self.inspector_scroll_drag.take().is_some() {
            unsafe {
                let _ = ReleaseCapture();
            }
            self.refresh(hwnd, true);
            return;
        }
        if let Some(control) = self.property_drag.take() {
            self.update_property_drag(hwnd, control, x);
            self.editor.finish_edit_group();
            if control != ControlId::ZoomSlider {
                let result = self.apply_editor_state();
                self.record_result(result);
            }
            unsafe {
                let _ = ReleaseCapture();
            }
            self.refresh(hwnd, true);
            return;
        }
        let resized = self.timeline_resize.take().is_some();
        let scrolled = self.timeline_scroll.take().is_some();
        if resized {
            self.persist_layout();
        }
        if resized || scrolled {
            unsafe {
                let _ = ReleaseCapture();
            }
            if resized {
                self.resize_preview_surface(hwnd);
            }
            self.refresh(hwnd, true);
            return;
        }
        if self.video_drag.take().is_some() {
            self.editor.finish_edit_group();
            unsafe {
                let _ = ReleaseCapture();
            }
            let result = self.apply_editor_state();
            self.record_editor_result(result, "视频裁剪已提交 · 一次撤销可恢复");
            self.refresh(hwnd, true);
            return;
        }
        if self.finish_pressed_control(hwnd, x, y) {
            return;
        }
        if let Some(drag) = self.canvas_drag.take() {
            self.finish_canvas_drag(hwnd, x, y, drag);
            return;
        }
        if self.timeline_scrub.take().is_some() {
            self.telemetry
                .scrub_metrics
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .finish(Instant::now());
            self.scrub_gate.reset();
            self.scrub_preview.cancel();
            unsafe {
                let _ = ReleaseCapture();
            }
            let controls = self.control_layout(client_rect(hwnd));
            let target = timeline_tick(controls.camera_track, x, self.timeline);
            self.seek_absolute(target);
            self.refresh(hwnd, true);
            return;
        }
        let Some(mut drag) = self.timeline_drag.take() else {
            return;
        };
        drag.current_x = x;
        unsafe {
            let _ = ReleaseCapture();
        }
        if (drag.current_x - drag.start_x).abs() <= 2 {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Success,
                "已选择镜头片段".into(),
            );
            self.refresh(hwnd, true);
            return;
        }
        let controls = self.control_layout(client_rect(hwnd));
        let (start_tick, end_tick) = self.dragged_camera_range(&drag, controls.camera_track);
        if self
            .editor
            .video_edit()
            .project_ranges(start_tick, end_tick)
            .is_empty()
        {
            self.note_problem(
                "镜头需位于保留的视频范围内，请调整到有效位置",
                "镜头必须位于保留的视频内容上；本次拖动未应用",
            );
            self.refresh(hwnd, true);
            return;
        }
        let result = self
            .editor
            .update_segment(&drag.original.id, start_tick, end_tick, drag.original.to)
            .map_err(PreviewWindowError::from)
            .and_then(|()| self.apply_editor_state());
        self.record_editor_result(result, "时间线片段已更新");
        self.refresh(hwnd, true);
    }

    fn finish_canvas_drag(&mut self, hwnd: HWND, x: i32, y: i32, mut drag: CanvasDrag) {
        drag.current_x = x;
        drag.current_y = y;
        unsafe {
            let _ = ReleaseCapture();
        }
        let result = self.canvas_focus(hwnd, &drag).and_then(|focus| {
            self.editor
                .set_segment_focus(&drag.original.id, focus, drag.original.to.scale)?;
            Ok(())
        });
        if result.is_ok() {
            self.editor.finish_edit_group();
        } else {
            self.editor.cancel_edit_group();
        }
        let result = result.and_then(|()| self.apply_editor_state());
        self.record_editor_result(result, "画面焦点已更新 · Ctrl+Z 撤销");
        self.refresh(hwnd, true);
    }

    fn canvas_focus(
        &self,
        hwnd: HWND,
        drag: &CanvasDrag,
    ) -> Result<NormalizedPoint, PreviewWindowError> {
        let video = preview_video_rect(client_rect(hwnd), &self.frame, self.theme);
        let width = f64::from((video.right - video.left).max(1));
        let height = f64::from((video.bottom - video.top).max(1));
        let content = self.frame.content_rect(self.editor.settings());
        let output_x = ((f64::from(drag.current_x - video.left) / width - content.left)
            / (content.right - content.left))
            .clamp(0.0, 1.0);
        let output_y = ((f64::from(drag.current_y - video.top) / height - content.top)
            / (content.bottom - content.top))
            .clamp(0.0, 1.0);
        let source = drag.initial_transform.output_to_source(output_x, output_y);
        NormalizedPoint::new(source.x.clamp(0.0, 1.0), source.y.clamp(0.0, 1.0))
            .map_err(PreviewWindowError::Geometry)
    }

    fn finish_pressed_control(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        if self.pressed_control.is_none() {
            return false;
        }
        let controls = self.control_layout(client_rect(hwnd));
        let released_over = controls
            .hit_control(x, y)
            .filter(|control| self.control_enabled(*control));
        let activated = complete_pointer_activation(&mut self.pressed_control, released_over);
        unsafe {
            let _ = ReleaseCapture();
        }
        if let Some(control) = released_over {
            let rect = controls.raw_control_rect(control).0;
            unsafe {
                let _ = InvalidateRect(Some(hwnd), Some(&raw const rect), false);
            }
        }
        if let Some(control) = activated {
            self.activate_control(hwnd, control);
        }
        self.refresh(hwnd, true);
        true
    }

    fn update_pointer_state(&mut self, hwnd: HWND, x: i32, y: i32) {
        let client = client_rect(hwnd);
        let controls = self.control_layout(client);
        let next = if self.timeline_drag.is_none()
            && self.timeline_scrub.is_none()
            && self.canvas_drag.is_none()
        {
            controls.hit_control(x, y)
        } else {
            None
        };
        if next != self.hot_control {
            let tooltip_was_visible = self.tooltip_visible;
            self.tooltip_visible = false;
            self.tooltip_deadline = next.map(|_| Instant::now() + TOOLTIP_DELAY);
            if let Some(previous) = self.hot_control {
                let rect = controls.control_rect(previous).0;
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), Some(&raw const rect), false);
                }
            }
            self.hot_control = next;
            if let Some(current) = self.hot_control {
                let rect = controls.control_rect(current).0;
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), Some(&raw const rect), false);
                }
            }
            if tooltip_was_visible {
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
        }
        if !self.tracking_mouse_leave {
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            self.tracking_mouse_leave = unsafe { TrackMouseEvent(&raw mut tracking) }.is_ok();
        }
        self.update_cursor(hwnd, x, y);
    }

    fn playhead_hit(&self, controls: &ControlLayout, x: i32, y: i32) -> bool {
        let tick = self
            .timeline_scrub
            .map_or(self.session.snapshot().project_tick, |s| s.desired_tick);
        self.timeline.contains(tick)
            && controls.timeline_area().contains(x, y)
            && (x - timeline_x(controls.camera_track, tick, self.timeline)).abs()
                <= self.theme.scale(4)
    }

    fn update_cursor(&self, hwnd: HWND, x: i32, y: i32) {
        if let Ok(cursor) = unsafe { LoadCursorW(None, self.cursor_at(client_rect(hwnd), x, y)) } {
            unsafe {
                let _ = SetCursor(Some(cursor));
            }
        }
    }

    #[allow(clippy::too_many_lines)] // One priority order for all mutually exclusive pointer targets.
    fn cursor_at(&self, client: RECT, x: i32, y: i32) -> PCWSTR {
        if let Some((dx, dy)) = self.crop_hit(client, x, y) {
            use windows::Win32::UI::WindowsAndMessaging::{IDC_SIZENESW, IDC_SIZENWSE};
            return match (dx, dy) {
                (0, 0) => IDC_SIZEALL,
                (0, _) => IDC_SIZENS,
                (_, 0) => IDC_SIZEWE,
                (a, b) if a == b => IDC_SIZENWSE,
                _ => IDC_SIZENESW,
            };
        }
        let c = self.control_layout(client);
        if self.inspector_resize.is_some() {
            IDC_SIZEWE
        } else if self.timeline_resize.is_some() {
            IDC_SIZENS
        } else if self.timeline_scrub.is_some()
            || self.property_drag.is_some()
            || self.timeline_scroll.is_some()
            || self.inspector_scroll_drag.is_some()
        {
            IDC_HAND
        } else if self.timeline_pan.is_some() || self.canvas_drag.is_some() {
            IDC_SIZEALL
        } else if let Some(drag) = &self.timeline_drag {
            if drag.mode == TimelineDragMode::Move {
                IDC_SIZEALL
            } else {
                IDC_SIZEWE
            }
        } else if self.video_drag.is_some() || c.inspector_splitter.contains(x, y) {
            IDC_SIZEWE
        } else if c.splitter.contains(x, y) {
            IDC_SIZENS
        } else if self.playhead_hit(&c, x, y) {
            IDC_HAND
        } else if let Some(id) = c.hit_control(x, y).filter(|id| self.control_enabled(*id)) {
            match id {
                ControlId::StartValue
                | ControlId::EndValue
                | ControlId::ScaleValue
                | ControlId::CanvasWidth
                | ControlId::CanvasHeight => IDC_IBEAM,
                ControlId::Inset
                | ControlId::Radius
                | ControlId::Shadow
                | ControlId::VideoSpeed
                | ControlId::CropLeft
                | ControlId::CropTop
                | ControlId::CropRight
                | ControlId::CropBottom => {
                    if HitRect(editor_widgets::value_rect(
                        c.raw_control_rect(id).0,
                        self.theme,
                        id,
                    ))
                    .contains(x, y)
                    {
                        IDC_IBEAM
                    } else {
                        IDC_HAND
                    }
                }
                ControlId::ZoomSlider => IDC_HAND,
                _ => IDC_ARROW,
            }
        } else if c.video_track.contains(x, y) {
            self.editor
                .video_edit()
                .clip_at(timeline_tick(c.video_track, x, self.timeline))
                .map_or(IDC_ARROW, |span| {
                    let left = timeline_x(c.video_track, span.project_in, self.timeline);
                    let right = timeline_x(c.video_track, span.project_out, self.timeline);
                    if timeline_edge_mode(x, left, right, self.theme.scale(8))
                        == TimelineDragMode::Move
                    {
                        IDC_ARROW
                    } else {
                        IDC_SIZEWE
                    }
                })
        } else if c.camera_track.contains(x, y) {
            let playhead = timeline_x(
                c.camera_track,
                self.session.snapshot().project_tick,
                self.timeline,
            );
            if (x - playhead).abs() <= self.theme.scale(7) {
                IDC_HAND
            } else {
                self.segment_at_x(c.camera_track, x)
                    .map_or(IDC_HAND, |(_, r)| {
                        if timeline_edge_mode(x, r.left, r.right, self.theme.scale(8))
                            == TimelineDragMode::Move
                        {
                            IDC_SIZEALL
                        } else {
                            IDC_SIZEWE
                        }
                    })
            }
        } else if c.ruler.contains(x, y)
            || c.click_track.contains(x, y)
            || (!self.timeline.is_fit() && c.scrollbar.contains(x, y))
            || (c.inspector_scroll_max > 0 && c.inspector_scrollbar.contains(x, y))
        {
            IDC_HAND
        } else if self.selected_segment.is_some()
            && HitRect(preview_video_rect(client, &self.frame, self.theme)).contains(x, y)
        {
            IDC_SIZEALL
        } else {
            IDC_ARROW
        }
    }

    fn clear_pressed_control(&mut self, hwnd: HWND) {
        let Some(control) = self.pressed_control.take() else {
            return;
        };
        let rect = self
            .control_layout(client_rect(hwnd))
            .control_rect(control)
            .0;
        unsafe {
            let _ = InvalidateRect(Some(hwnd), Some(&raw const rect), false);
        }
    }

    fn cancel_pressed_control(&mut self, hwnd: HWND, release_capture: bool) {
        self.clear_pressed_control(hwnd);
        if release_capture {
            unsafe {
                let _ = ReleaseCapture();
            }
        }
    }

    fn on_mouse_leave(&mut self, hwnd: HWND) {
        self.tracking_mouse_leave = false;
        let tooltip_was_visible = self.tooltip_visible;
        self.tooltip_visible = false;
        self.tooltip_deadline = None;
        if let Some(control) = self.hot_control.take() {
            let rect = self
                .control_layout(client_rect(hwnd))
                .control_rect(control)
                .0;
            unsafe {
                let _ = InvalidateRect(Some(hwnd), Some(&raw const rect), false);
            }
        }
        if tooltip_was_visible {
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
    }

    fn on_keyboard_focus(&mut self, hwnd: HWND, focused: bool) {
        if focused {
            self.publish_accessibility(hwnd);
        } else {
            self.keyboard_focus = false;
            if self.scrub_smoke.is_none() {
                self.cancel_pressed_control(hwnd, true);
            }
            self.accessibility.blur();
        }
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }

    #[allow(clippy::too_many_lines)] // Keep the shared UI/keyboard/accessibility availability table together.
    fn control_enabled(&self, control: ControlId) -> bool {
        if self.navigation_authorized
            && !matches!(
                control,
                ControlId::WindowClose | ControlId::WindowMinimize | ControlId::WindowMaximize
            )
        {
            return false;
        }
        match control {
            ControlId::Menu => self.pending_navigation.is_none(),
            ControlId::CropEdit
            | ControlId::CropLock
            | ControlId::CropLeft
            | ControlId::CropTop
            | ControlId::CropRight
            | ControlId::CropBottom
            | ControlId::CropReset
            | ControlId::CropWide
            | ControlId::CropStandard
            | ControlId::CropSquare
            | ControlId::CropPortrait => self.selected_crop().is_some(),
            ControlId::VideoSpeed
            | ControlId::SpeedReset
            | ControlId::SpeedHalf
            | ControlId::SpeedNormal
            | ControlId::SpeedOneHalf
            | ControlId::SpeedDouble
            | ControlId::SpeedQuadruple => self.widget_value(ControlId::VideoSpeed).is_some(),
            ControlId::Split => {
                let tick = self.session.snapshot().project_tick;
                let video = self.editor.video_edit();
                video.clip_at(tick).is_some_and(|span| {
                    tick > span.project_in
                        && video.source_time(tick).is_ok_and(|source| {
                            source > span.clip.source_in_tick && source < span.clip.source_out_tick
                        })
                })
            }
            ControlId::SpeedSection
            | ControlId::CropSection
            | ControlId::CanvasOriginal
            | ControlId::CanvasWide
            | ControlId::CanvasPortrait
            | ControlId::CanvasSquare
            | ControlId::CanvasWidth
            | ControlId::CanvasHeight
            | ControlId::PreviousFrame
            | ControlId::NextFrame
            | ControlId::WindowMinimize
            | ControlId::WindowMaximize
            | ControlId::WindowClose
            | ControlId::CanvasTab
            | ControlId::VideoTab
            | ControlId::Back
            | ControlId::NewRecording
            | ControlId::OpenProject
            | ControlId::InspectorToggle
            | ControlId::ZoomSlider
            | ControlId::SwatchMist
            | ControlId::SwatchPeach
            | ControlId::SwatchLilac
            | ControlId::SwatchCharcoal
            | ControlId::SwatchWhite
            | ControlId::SwatchSage
            | ControlId::CameraEnabled
            | ControlId::Cursor
            | ControlId::CursorProperties
            | ControlId::Export
            | ControlId::Location
            | ControlId::Play
            | ControlId::Home
            | ControlId::Add
            | ControlId::Reset
            | ControlId::Diagnostics
            | ControlId::TimelineZoomOut
            | ControlId::TimelineFit
            | ControlId::TimelineZoomIn
            | ControlId::Background
            | ControlId::Inset
            | ControlId::Radius
            | ControlId::Shadow => true,
            ControlId::VideoTrim => self.selected_video.is_some(),
            ControlId::Save => {
                self.editor.is_dirty() && self.save_task.is_none() && self.background_task.is_none()
            }
            ControlId::BackgroundImage | ControlId::BackgroundFile | ControlId::BackgroundColor => {
                self.background_task.is_none()
            }
            ControlId::Undo => self.editor.can_undo(),
            ControlId::Redo => self.editor.can_redo(),
            ControlId::Delete => {
                if self.selected_video.is_some() {
                    self.editor.video_edit().clips.len() > 1
                } else {
                    self.selected_segment.is_some()
                }
            }
            ControlId::StartValue | ControlId::EndValue => {
                self.selected_segment.is_some() || self.selected_video.is_some()
            }
            ControlId::ScaleValue
            | ControlId::CenterFocus
            | ControlId::CameraProperties
            | ControlId::ScaleMinus
            | ControlId::ScalePlus
            | ControlId::FocusLeft
            | ControlId::FocusRight
            | ControlId::FocusUp
            | ControlId::FocusDown
            | ControlId::StartMinus
            | ControlId::StartPlus
            | ControlId::EndMinus
            | ControlId::EndPlus => self.selected_segment.is_some(),
        }
    }

    fn on_mouse_wheel(&mut self, hwnd: HWND, delta: i16, x: i32, y: i32) {
        if self.navigation_authorized {
            return;
        }
        let layout = self.control_layout(client_rect(hwnd));
        if HitRect(layout.panel).contains(x, y) && layout.inspector_scroll_max > 0 {
            if !self.finish_inline_input(hwnd, true) {
                return;
            }
            self.inspector_scroll = (layout.inspector_scroll
                - i32::from(delta) * self.theme.scale(48) / 120)
                .clamp(0, layout.inspector_scroll_max);
            self.hot_control = None;
            self.tooltip_visible = false;
            self.tooltip_deadline = None;
            self.refresh(hwnd, true);
            return;
        }
        let steps = f64::from(delta) / 120.0;
        if steps.abs() < f64::EPSILON {
            return;
        }
        let controls = self.control_layout(client_rect(hwnd));
        if let Some(control @ (ControlId::Inset | ControlId::Radius | ControlId::Shadow)) =
            controls.hit_control(x, y)
        {
            if self.wheel_edit_deadline.is_none() {
                self.editor.begin_edit_group();
            }
            self.wheel_edit_deadline = Some(Instant::now() + WHEEL_EDIT_SETTLE);
            let canvas = self.editor.settings().canvas;
            let result = match control {
                ControlId::Inset => self
                    .editor
                    .set_canvas_inset((canvas.inset + steps * 0.01).clamp(0.0, 0.25)),
                ControlId::Radius => self
                    .editor
                    .set_corner_radius((canvas.corner_radius + steps * 0.005).clamp(0.0, 0.10)),
                ControlId::Shadow => self
                    .editor
                    .set_shadow((canvas.shadow + steps * 0.05).clamp(0.0, 1.0)),
                _ => unreachable!(),
            };
            self.wheel_edit_pending |= result.is_ok();
            let result = result
                .map_err(PreviewWindowError::from)
                .and_then(|()| self.apply_editor_state());
            self.record_editor_result(
                result,
                "外观已预览 · 滚轮连续调整 · 单击输入数值 · Esc 取消",
            );
            self.refresh(hwnd, true);
            return;
        }
        if controls.timeline_area().contains(x, y) {
            let control_down = unsafe { GetKeyState(i32::from(VK_CONTROL.0)) } < 0;
            if control_down {
                let anchor = timeline_tick(controls.camera_track, x, self.timeline);
                self.timeline.zoom_at(anchor, 1.25_f64.powf(steps));
            } else {
                let pan = -steps * self.timeline.visible_duration().as_i64() as f64 * 0.10;
                self.timeline.pan_by(TimeTick(pan.round() as i64));
                self.follow_playhead = false;
            }
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                format!(
                    "时间线视图 {} – {}",
                    format_tick(self.timeline.visible_start()),
                    format_tick(self.timeline.visible_end())
                ),
            );
            let region = controls.timeline_area().0;
            unsafe {
                let _ = InvalidateRect(Some(hwnd), Some(&raw const region), false);
            }
            return;
        }
        if HitRect(preview_video_rect(
            client_rect(hwnd),
            &self.frame,
            self.theme,
        ))
        .contains(x, y)
        {
            if self.wheel_edit_deadline.is_none() {
                self.editor.begin_edit_group();
            }
            self.wheel_edit_deadline = Some(Instant::now() + WHEEL_EDIT_SETTLE);
            let result = self
                .update_target(0.0, 0.0, steps * 0.1)
                .and_then(|()| self.apply_editor_state());
            self.wheel_edit_pending |= result.is_ok();
            self.record_editor_result(result, "镜头缩放预览已更新");
            unsafe {
                let _ = InvalidateRect(Some(hwnd), Some(&raw const controls.panel), false);
                let _ = InvalidateRect(Some(hwnd), Some(&raw const controls.status), false);
            }
        }
    }

    fn finish_wheel_edit(&mut self) {
        if self.wheel_edit_deadline.take().is_some() {
            self.editor.finish_edit_group();
            if self.wheel_edit_pending {
                self.wheel_edit_pending = false;
                let result = self.apply_editor_state();
                self.record_editor_result(result, "调整已提交 · Ctrl+Z 撤销");
            }
        }
    }

    fn seek_relative(&mut self, delta: i64) {
        let current = self.session.snapshot().project_tick.as_i64();
        let target = TimeTick(current.saturating_add(delta).max(0));
        self.seek_absolute(target);
    }

    fn seek_absolute(&mut self, target: TimeTick) {
        if self.crop_editing {
            self.toggle_crop_editing();
        }
        let was_playing = self.session.snapshot().playback == PlaybackState::Playing;
        if was_playing {
            self.deferred_playback.request();
        }
        self.session.pause();
        let started = Instant::now();
        let result = self
            .session
            .seek(target)
            .and_then(|_| self.session.begin_interactive_prepare());
        self.last_clock = Instant::now();
        match result {
            Ok(Some(frame)) => {
                if let Err(error) =
                    self.accept_prepared(frame, started.elapsed().as_micros() as u64)
                {
                    self.record_session_result(Err(error));
                    return;
                }
                if self.deferred_playback.take_requested() {
                    self.session.play();
                    self.last_clock = Instant::now();
                    self.set_status(
                        crate::platform::editor_ui::StatusSeverity::Information,
                        "正在播放".into(),
                    );
                } else {
                    self.set_status(
                        crate::platform::editor_ui::StatusSeverity::Information,
                        "准备就绪".into(),
                    );
                }
            }
            Ok(None) if self.deferred_playback.is_requested() => {
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "正在播放 · 正在同步画面…".into(),
                );
                self.schedule_interactive_poll();
            }
            Ok(None) => {
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "正在定位精确画面…".into(),
                );
                self.schedule_interactive_poll();
            }
            Err(error) => self.record_session_result(Err(error)),
        }
    }

    fn toggle_playback_intent(&mut self) {
        if self.crop_editing {
            self.toggle_crop_editing();
        }
        if !self.session.background_matches(self.editor.settings()) {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "正在应用外观，请稍候再播放".into(),
            );
            return;
        }
        if self.timeline_scrub.is_some() {
            self.deferred_playback.toggle();
            self.session.pause();
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                if self.deferred_playback.is_requested() {
                    "松开播放头后继续播放".into()
                } else {
                    "已暂停".into()
                },
            );
            return;
        }
        if self.session.snapshot().playback == PlaybackState::Playing {
            self.deferred_playback.cancel();
            self.session.pause();
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "已暂停".into(),
            );
            return;
        }
        if self.session.interactive_render_pending() {
            if self.deferred_playback.is_requested() {
                self.deferred_playback.cancel();
                self.session.pause();
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "已暂停".into(),
                );
                return;
            }
            self.deferred_playback.request();
            self.session.pause();
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "正在播放 · 正在同步画面…".into(),
            );
            self.schedule_interactive_poll();
            return;
        }
        self.deferred_playback.cancel();
        self.session.toggle_playback();
        self.last_clock = Instant::now();
        self.set_status(
            crate::platform::editor_ui::StatusSeverity::Information,
            if self.session.snapshot().playback == PlaybackState::Playing {
                "正在播放".into()
            } else {
                "已暂停".into()
            },
        );
    }

    fn selected_segment(&self) -> Option<CameraSegment> {
        let id = self.selected_segment.as_deref()?;
        self.editor
            .camera()
            .segments
            .iter()
            .find(|segment| segment.id == id)
            .cloned()
    }

    fn segment_at_x(&self, track: HitRect, x: i32) -> Option<(CameraSegment, RECT)> {
        self.display_segments().iter().find_map(|segment| {
            let rect = segment_rect(track, segment, self.timeline);
            (x >= rect.left && x < rect.right).then(|| {
                (
                    self.editor
                        .camera()
                        .segments
                        .iter()
                        .find(|source| source.id == segment.id)
                        .unwrap()
                        .clone(),
                    rect,
                )
            })
        })
    }

    fn adjust_target(&mut self, delta_x: f64, delta_y: f64, delta_scale: f64) {
        let result = self
            .update_target(delta_x, delta_y, delta_scale)
            .and_then(|()| self.apply_editor_state());
        self.record_editor_result(result, "镜头目标已更新");
    }

    fn update_target(
        &mut self,
        delta_x: f64,
        delta_y: f64,
        delta_scale: f64,
    ) -> Result<(), PreviewWindowError> {
        let Some(segment) = self.selected_segment() else {
            return Err(PreviewWindowError::NoSelectedSegment);
        };
        let scale = (segment.to.scale + delta_scale).max(1.0);
        let focus = segment.focus_point();
        let point = NormalizedPoint::new(
            (focus.x + delta_x).clamp(0.0, 1.0),
            (focus.y + delta_y).clamp(0.0, 1.0),
        );
        point
            .map_err(PreviewWindowError::Geometry)
            .and_then(|point| {
                self.editor
                    .set_segment_focus(&segment.id, point, scale)
                    .map_err(PreviewWindowError::from)
            })
    }

    fn adjust_timing(&mut self, start_delta: TimeTick, end_delta: TimeTick) {
        let Some(segment) = self.selected_segment() else {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "请先选择一个镜头片段".into(),
            );
            return;
        };
        let start_tick = TimeTick(
            segment
                .start_tick
                .as_i64()
                .saturating_add(start_delta.as_i64()),
        );
        let end_tick = TimeTick(segment.end_tick.as_i64().saturating_add(end_delta.as_i64()));
        let result = self
            .editor
            .update_segment(&segment.id, start_tick, end_tick, segment.to)
            .map_err(PreviewWindowError::from)
            .and_then(|()| self.apply_editor_state());
        self.record_editor_result(result, "镜头时间已更新");
    }

    fn add_manual_segment(&mut self) {
        if self.crop_editing {
            self.toggle_crop_editing();
        }
        let video = self.editor.video_edit();
        let project_tick = self.session.snapshot().project_tick;
        let Some(span) = video.clip_at(project_tick) else {
            return;
        };
        let Ok(start_tick) = panzo_core::TimeMapping::source_time(&video, project_tick) else {
            return;
        };
        let end_tick = TimeTick(
            start_tick
                .as_i64()
                .saturating_add(
                    span.clip
                        .source_delta(TimeTick(5 * TICKS_PER_SECOND / 10))
                        .0,
                )
                .min(span.clip.source_out_tick.as_i64()),
        );
        let current = if self.editor.settings().camera_enabled {
            self.editor.camera().evaluate(start_tick)
        } else {
            CameraState::BASE
        };
        let target = CameraState::focused(
            NormalizedPoint::new(current.center_x, current.center_y).expect("valid camera center"),
            current.scale.max(1.6),
        )
        .expect("valid manual camera target");
        let id = format!("manual-{}", uuid::Uuid::new_v4());
        let result = self
            .editor
            .add_segment(
                id.clone(),
                CameraSegmentKind::ZoomIn,
                start_tick,
                end_tick,
                target,
            )
            .map_err(PreviewWindowError::from)
            .and_then(|_| {
                self.selected_segment = Some(id);
                self.selected_video = None;
                self.inspector_video_tab = false;
                self.apply_editor_state()
            });
        self.record_editor_result(result, "已添加手动镜头片段");
    }

    fn delete_selected(&mut self) {
        if !self.control_enabled(ControlId::Delete) {
            self.set_status(
                StatusSeverity::Information,
                if self.selected_video.is_some() {
                    "需要保留至少一个视频片段，无法删除最后一段"
                } else {
                    "请先选择要删除的片段"
                }
                .into(),
            );
            return;
        }
        if self.crop_editing {
            self.toggle_crop_editing();
        }
        if let Some(id) = self.selected_video.clone() {
            let result = self
                .editor
                .delete_video(&id)
                .map_err(PreviewWindowError::from)
                .and_then(|()| {
                    self.selected_video = None;
                    self.apply_editor_state()
                });
            self.record_editor_result(result, "视频已删除，空隙已闭合 · Ctrl+Z 撤销");
            return;
        }
        let Some(id) = self.selected_segment.clone() else {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "请先选择一个镜头片段".into(),
            );
            return;
        };
        let result = self
            .editor
            .delete_segment(&id)
            .map_err(PreviewWindowError::from)
            .and_then(|()| {
                self.selected_segment = self
                    .editor
                    .camera()
                    .segments
                    .first()
                    .map(|segment| segment.id.clone());
                self.apply_editor_state()
            });
        self.record_editor_result(result, "镜头片段已删除");
    }

    fn undo(&mut self) {
        if self.editor.undo() {
            let result = self.apply_editor_state();
            self.record_editor_result(result, "已撤销");
        } else {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "没有可撤销的操作".into(),
            );
        }
    }

    fn redo(&mut self) {
        if self.editor.redo() {
            let result = self.apply_editor_state();
            self.record_editor_result(result, "已重做");
        } else {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "没有可重做的操作".into(),
            );
        }
    }

    fn reset_to_auto(&mut self) {
        let result = self
            .editor
            .reset_to_auto_draft()
            .map_err(PreviewWindowError::from)
            .and_then(|()| {
                self.selected_segment = self
                    .editor
                    .camera()
                    .segments
                    .first()
                    .map(|segment| segment.id.clone());
                self.apply_editor_state()
            });
        self.record_editor_result(result, "已恢复自动初稿");
    }

    fn import_background(&mut self, hwnd: HWND) {
        if self.background_task.is_some() {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "背景正在载入，请稍候…".into(),
            );
            return;
        }
        let Some(path) = choose_background_image(hwnd) else {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "已取消导入背景".into(),
            );
            return;
        };
        match crate::platform::editor_tasks::BackgroundTask::start(
            self.editor.background_snapshot(),
            Some(path),
        ) {
            Ok(task) => {
                self.background_task = Some(task);
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "正在导入背景… · 可继续编辑".into(),
                );
            }
            Err(error) => self.note_problem(
                "未能载入图片，请重新选择；原背景未改变",
                format!("错误：无法导入背景 · {error}"),
            ),
        }
    }

    fn poll_background(&mut self, hwnd: HWND) {
        let Some(task) = self.background_task.as_ref() else {
            return;
        };
        let importing = task.importing;
        let stale_import = importing && self.editor.settings().background != task.previous_style;
        let Some(result) = task.poll() else {
            return;
        };
        self.background_task = None;
        if stale_import {
            let result = self.apply_editor_state();
            self.record_editor_result(result, "已忽略过期的背景载入结果");
            return;
        }
        match result {
            Ok((style, image)) => {
                if importing {
                    if let Err(error) = self.editor.set_background_style(style.clone()) {
                        self.report_problem(
                            crate::platform::editor_ui::StatusScope::Background,
                            "背景暂未更新，请重新选择背景或切换为纯色",
                            error.to_string(),
                        );
                        return;
                    }
                } else if self.editor.settings().background != style {
                    let result = self.apply_editor_state();
                    self.record_editor_result(result, "正在载入最新背景");
                    return;
                }
                let result = self
                    .session
                    .apply_edit_state_with_background(
                        self.editor.camera().clone(),
                        self.editor.settings().clone(),
                        image,
                    )
                    .map_err(PreviewWindowError::from)
                    .and_then(|()| self.apply_editor_state());
                self.record_editor_result(result, "背景已更新 · Ctrl+Z 撤销");
            }
            Err(error) => self.report_problem(
                crate::platform::editor_ui::StatusScope::Background,
                "背景暂未更新，请重新选择背景或切换为纯色",
                format!("错误：背景载入失败，保留上一画面 · {error}"),
            ),
        }
        self.refresh(hwnd, true);
    }

    fn apply_editor_state(&mut self) -> Result<(), PreviewWindowError> {
        self.timeline.set_duration(self.editor.project_duration());
        // Undoing a split can remove the selected ID. Keep video properties usable
        // by selecting the surviving clip under the playhead instead of a stale ID.
        let video = self.editor.video_edit();
        if self
            .selected_video
            .as_ref()
            .is_some_and(|id| !video.clips.iter().any(|clip| &clip.id == id))
        {
            let tick = self
                .session
                .snapshot()
                .project_tick
                .min(TimeTick(video.duration().0 - 1));
            self.selected_video = video.clip_at(tick).map(|span| span.clip.id.clone());
        }
        if self.selected_segment.as_ref().is_some_and(|id| {
            !self
                .editor
                .camera()
                .segments
                .iter()
                .any(|segment| &segment.id == id)
        }) {
            self.selected_segment = None;
        }
        self.scrub_preview.cancel();
        self.scrub_gate.reset();
        self.session.cancel_interactive_render();
        self.session.pause();
        self.deferred_playback.cancel();
        if !self.session.background_matches(self.editor.settings()) {
            if self.background_task.is_none() {
                match crate::platform::editor_tasks::BackgroundTask::start(
                    self.editor.background_snapshot(),
                    None,
                ) {
                    Ok(task) => {
                        self.background_task = Some(task);
                        self.set_status(
                            crate::platform::editor_ui::StatusSeverity::Information,
                            "正在载入背景…".into(),
                        );
                    }
                    Err(error) => self.report_problem(
                        crate::platform::editor_ui::StatusScope::Background,
                        "背景暂未更新，请重新选择背景或切换为纯色",
                        format!("错误：背景任务无法启动 · {error}"),
                    ),
                }
            }
            return Ok(());
        }
        self.resolve_problem(StatusScope::Background);
        self.timeline.set_duration(self.editor.project_duration());
        self.scrub_preview.cancel();
        self.scrub_gate.reset();
        if let Some(scrub) = &mut self.timeline_scrub {
            scrub.submitted_tick = None;
        }
        self.deferred_playback.cancel();
        self.session.pause();
        self.update_canvas_output()?;
        self.session
            .apply_edit_state(self.editor.camera().clone(), self.editor.settings().clone())?;
        let started = Instant::now();
        if let Some(frame) = self.session.begin_interactive_prepare()? {
            self.accept_prepared(frame, started.elapsed().as_micros() as u64)?;
        }
        self.schedule_interactive_poll();
        Ok(())
    }

    fn save_editor(&mut self, kind: CameraEditKind) {
        if self.save_task.is_some() {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "正在保存，请稍候…".into(),
            );
            return;
        }
        self.editor.finish_edit_group();
        let segment_id = self.selected_segment.clone();
        match crate::platform::editor_tasks::SaveTask::start(
            self.editor.background_snapshot(),
            kind,
            segment_id,
        ) {
            Ok(task) => {
                self.save_task = Some(task);
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "正在安全保存… · 可继续编辑".into(),
                );
            }
            Err(error) => self.report_problem(
                crate::platform::editor_ui::StatusScope::Save,
                "修改尚未保存，请勿关闭窗口；请再次保存，仍失败时查看诊断详情",
                format!("错误：{error}"),
            ),
        }
    }

    fn poll_save(&mut self, hwnd: HWND) -> bool {
        let Some(result) = self
            .save_task
            .as_ref()
            .and_then(crate::platform::editor_tasks::SaveTask::poll)
        else {
            return false;
        };
        self.save_task = None;
        match result {
            Ok((saved, report)) => {
                self.editor.acknowledge_background_save(&saved);
                self.resolve_problem(StatusScope::Save);
                if !self.editor.is_dirty() {
                    self.resolve_problem(StatusScope::RecoveryDraft);
                }
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Success,
                    if self.editor.is_dirty() {
                        "快照已保存 · 期间的新修改尚未保存".into()
                    } else {
                        format!("已保存版本 {}", report.revision)
                    },
                );
                if self.pending_navigation.is_some() {
                    if self.editor.is_dirty() {
                        self.cancel_navigation();
                    } else {
                        self.navigation_authorized = true;
                    }
                }
            }
            Err(error) => {
                self.report_problem(
                    crate::platform::editor_ui::StatusScope::Save,
                    "修改尚未保存，请勿关闭窗口；检查工程目录后再次保存",
                    format!("错误：保存失败，修改仍保留 · {error}"),
                );
                self.cancel_navigation();
            }
        }
        self.advance_navigation(hwnd);
        self.refresh(hwnd, true);
        false
    }

    fn record_editor_result(&mut self, result: Result<(), PreviewWindowError>, success: &str) {
        match result {
            Ok(()) if self.session.interactive_render_pending() => {
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    format!("{success} · 正在更新预览…"),
                );
            }
            Ok(()) => self.set_status(
                crate::platform::editor_ui::StatusSeverity::Success,
                success.into(),
            ),
            Err(error) => self.handle_window_error(error, true),
        }
    }

    fn render(&mut self) -> Result<(), PreviewSessionError> {
        // Do not starve completed playback frames by cancelling on every clock tick.
        if self.session.interactive_render_pending() {
            return Ok(());
        }
        let started = Instant::now();
        if let Some(frame) = self.session.begin_interactive_prepare()? {
            self.accept_prepared(frame, started.elapsed().as_micros() as u64)?;
        } else {
            self.schedule_interactive_poll();
        }
        Ok(())
    }

    fn accept_prepared(
        &mut self,
        frame: PreparedPreviewFrame,
        elapsed_micros: u64,
    ) -> Result<(), PreviewSessionError> {
        let display_started = Instant::now();
        self.session.set_crop_overlay(self.crop_overlay(&frame));
        let resized = frame.evaluation.output != self.frame.evaluation.output;
        if resized && let Some(old) = self.preview_surface.take() {
            let _ = unsafe { DestroyWindow(old.hwnd) };
            drop(old);
            if let Some(host) = self.host_hwnd
                && let Ok(module) = unsafe { GetModuleHandleW(None) }
                && let Ok(surface) = self.try_initialize_preview_surface(host, module.into())
            {
                self.preview_surface = Some(surface);
            }
        }
        if let Some(surface) = self.preview_surface.as_ref() {
            let overlay = self.focus_overlay();
            self.session
                .present_prepared(&surface.presenter, &frame, overlay)?;
            self.fallback_frame = None;
        } else {
            self.fallback_frame = Some(self.session.compose_prepared(&frame)?);
        }
        self.frame = frame;
        self.resolve_problem(StatusScope::Preview);
        if resized && let Some(host) = self.host_hwnd {
            self.resize_preview_surface(host);
        }
        if self.session.decoded_frame_is_native()
            && self.scrub_smoke.is_some_and(|smoke| {
                smoke.released && smoke.target == Some(self.frame.snapshot.project_tick)
            })
        {
            self.telemetry.scrub_settled.store(true, Ordering::Relaxed);
        }
        self.record_displayed_frame(
            elapsed_micros.saturating_add(display_started.elapsed().as_micros() as u64),
        );
        Ok(())
    }

    fn record_displayed_frame(&self, elapsed_micros: u64) {
        self.telemetry
            .cadence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .present(
                Instant::now(),
                self.frame.evaluation.source_frame.presentation_tick,
                self.session.snapshot().playback == PlaybackState::Playing,
            );
        self.telemetry
            .rendered_frames
            .fetch_add(1, Ordering::Relaxed);
        self.telemetry
            .render_total_micros
            .fetch_add(elapsed_micros, Ordering::Relaxed);
        self.telemetry
            .render_max_micros
            .fetch_max(elapsed_micros, Ordering::Relaxed);
        update_cache_telemetry(&self.telemetry, &self.session);
        self.telemetry
            .last_project_tick
            .store(self.frame.snapshot.project_tick.as_i64(), Ordering::Relaxed);
    }

    fn log_media_error(&self, stage: &str, message: &str) {
        let index = self.telemetry.media_errors.fetch_add(1, Ordering::Relaxed);
        // Bounded diagnostics: retain the actual cause of a failed probe, not only a count.
        if index < 8 {
            eprintln!(
                "Panzo preview error: stage={stage} tick={} message={message}",
                self.session.snapshot().project_tick.as_i64()
            );
        }
    }

    fn record_session_result(&mut self, result: Result<(), PreviewSessionError>) {
        match result {
            Ok(()) => self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "准备就绪".into(),
            ),
            Err(error) => self.handle_window_error(error.into(), false),
        }
    }

    fn record_result(&mut self, result: Result<(), PreviewWindowError>) {
        if let Err(error) = result {
            self.handle_window_error(error, false);
        }
    }

    fn update_window_title(&mut self, hwnd: HWND) {
        let title = format!(
            "{}{} — Panzo",
            self.project_name,
            if self.editor.is_dirty() { " *" } else { "" }
        );
        if self.displayed_title == title {
            return;
        }
        self.displayed_title.clone_from(&title);
        let wide = wide(&title);
        let caption = self.control_layout(client_rect(hwnd)).command_bar;
        unsafe {
            let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
            let _ = InvalidateRect(Some(hwnd), Some(&raw const caption), false);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn publish_accessibility(&self, hwnd: HWND) {
        use crate::platform::accessibility::{Kind, Node, PLAYHEAD_ID, STATUS_ID};
        let client = client_rect(hwnd);
        let layout = self.control_layout(client);
        let mut nodes: Vec<_> = ControlId::ORDER
            .into_iter()
            .map(|id| {
                let mut node = Node::button(
                    id as u32 + 1,
                    id.accessibility_name(),
                    id.tooltip(),
                    layout.control_rect(id).0,
                    self.control_enabled(id),
                );
                if id == ControlId::CameraEnabled {
                    node.kind = Kind::Toggle(self.editor.settings().camera_enabled);
                }
                if id == ControlId::CropLock {
                    node.kind = Kind::Toggle(self.crop_locked);
                }
                if id == ControlId::Cursor {
                    node.kind = Kind::Toggle(self.editor.settings().cursor.visible);
                }
                if id == ControlId::CanvasTab || id == ControlId::VideoTab {
                    node.kind =
                        Kind::Toggle(self.inspector_video_tab == (id == ControlId::VideoTab));
                }
                if id == ControlId::Play {
                    node.name = if self.session.snapshot().playback == PlaybackState::Playing
                        || self.deferred_playback.is_requested()
                    {
                        "暂停视频"
                    } else {
                        "播放视频"
                    }
                    .into();
                }
                if let Some((value, maximum)) = self.widget_range(id) {
                    node.kind = Kind::Range {
                        value,
                        maximum,
                        minimum: if id == ControlId::VideoSpeed {
                            0.25
                        } else {
                            0.0
                        },
                    };
                }
                if self.selected_segment.is_some() {
                    match id {
                        ControlId::StartValue => node.name = "镜头开始时间（秒）".into(),
                        ControlId::EndValue => node.name = "镜头结束时间（秒）".into(),
                        _ => {}
                    }
                }
                if let Some(value) = self.widget_value(id) {
                    node.name = format!("{}，当前 {value:.3}", node.name);
                }
                node
            })
            .collect();
        let snapshot = self.session.snapshot();
        nodes.push(Node {
            id: PLAYHEAD_ID,
            name: "播放位置（秒）".into(),
            help: "左右方向键逐步定位；Home 回到开头；Space 播放或暂停".into(),
            rect: layout.ruler.0,
            enabled: true,
            kind: Kind::Range {
                value: snapshot.project_tick.as_i64() as f64 / 10_000_000.0,
                minimum: 0.0,
                maximum: snapshot.duration_tick.as_i64() as f64 / 10_000_000.0,
            },
        });
        nodes.push(Node {
            id: STATUS_ID,
            name: self.status_message().0.to_owned(),
            help: String::new(),
            rect: layout.status,
            enabled: true,
            kind: Kind::Text,
        });
        for (index, span) in self.editor.video_edit().spans().enumerate() {
            let rect = RECT {
                left: timeline_x(layout.video_track, span.project_in, self.timeline)
                    .max(layout.video_track.left),
                right: timeline_x(layout.video_track, span.project_out, self.timeline)
                    .min(layout.video_track.right),
                top: layout.video_track.top + 3,
                bottom: layout.video_track.bottom - 3,
            };
            nodes.push(Node {
                id: self
                    .accessibility
                    .stable_id(format!("video:{}", span.clip.id)),
                name: format!(
                    "视频片段 {}，{} 至 {}",
                    index + 1,
                    format_tick(span.project_in),
                    format_tick(span.project_out)
                ),
                help: "选择后可裁剪入出点；B 在播放位置分割；Delete 删除；Ctrl+Z 撤销".into(),
                rect,
                enabled: true,
                kind: Kind::Item(self.selected_video.as_deref() == Some(span.clip.id.as_str())),
            });
        }
        let video = self.editor.video_edit();
        for (index, segment) in self.editor.camera().segments.iter().enumerate() {
            let rects: Vec<_> = video
                .project_ranges(segment.start_tick, segment.end_tick)
                .into_iter()
                .map(|(_, start, end)| {
                    let left = timeline_x(layout.camera_track, start, self.timeline)
                        .max(layout.camera_track.left);
                    let right = timeline_x(layout.camera_track, end, self.timeline)
                        .min(layout.camera_track.right);
                    (left, right)
                })
                .collect();
            let Some(left) = rects.iter().map(|r| r.0).min() else {
                continue;
            };
            let right = rects.iter().map(|r| r.1).max().unwrap_or(left);
            nodes.push(Node {
                id: self
                    .accessibility
                    .stable_id(format!("camera:{}", segment.id)),
                name: format!("镜头片段 {}，倍率 {:.2}", index + 1, segment.to.scale),
                help: "选择后使用镜头属性调整时间、倍率和焦点；Delete 删除；Ctrl+Z 撤销".into(),
                rect: RECT {
                    left,
                    right,
                    top: layout.camera_track.top + 3,
                    bottom: layout.camera_track.bottom - 3,
                },
                enabled: true,
                kind: Kind::Item(self.selected_segment.as_deref() == Some(segment.id.as_str())),
            });
        }
        let focus = self
            .keyboard_focus
            .then_some(
                self.accessibility_focus
                    .or(self.focused_control.map(|id| id as u32 + 1)),
            )
            .flatten();
        self.accessibility.publish(
            hwnd,
            &format!("Panzo 编辑器 · {}", self.project_name),
            client,
            nodes,
            focus,
        );
    }

    fn select_accessible_item(&mut self, id: u32, selected: bool) {
        if self.accessibility.node_rect(id).is_none() {
            return;
        }
        let Some(key) = self.accessibility.key(id) else {
            return;
        };
        if self.crop_editing && (key.starts_with("video:") || key.starts_with("camera:")) {
            self.toggle_crop_editing();
        }
        if let Some(clip) = key.strip_prefix("video:")
            && self
                .editor
                .video_edit()
                .clips
                .iter()
                .any(|item| item.id == clip)
        {
            if selected {
                self.selected_video = Some(clip.into());
                self.selected_segment = None;
                self.inspector_video_tab = true;
                self.inspector_scroll = 0;
                if let Some(span) = self
                    .editor
                    .video_edit()
                    .spans()
                    .find(|span| span.clip.id == clip)
                {
                    self.timeline.ensure_visible(span.project_in);
                }
            } else if self.selected_video.as_deref() == Some(clip) {
                self.selected_video = None;
            }
        } else if let Some(segment) = key.strip_prefix("camera:")
            && self
                .editor
                .camera()
                .segments
                .iter()
                .any(|item| item.id == segment)
        {
            if selected {
                self.selected_segment = Some(segment.into());
                self.selected_video = None;
                self.inspector_video_tab = false;
                self.inspector_scroll = 0;
                if let Some(camera) = self
                    .editor
                    .camera()
                    .segments
                    .iter()
                    .find(|item| item.id == segment)
                    && let Some((_, start, _)) = self
                        .editor
                        .video_edit()
                        .project_ranges(camera.start_tick, camera.end_tick)
                        .first()
                {
                    self.timeline.ensure_visible(*start);
                }
            } else if self.selected_segment.as_deref() == Some(segment) {
                self.selected_segment = None;
            }
        }
    }

    fn accessibility_actions(&mut self, hwnd: HWND) {
        use crate::platform::accessibility::{Action, PLAYHEAD_ID};
        for action in self.accessibility.take_actions() {
            // Recheck at consumption time: an earlier action in this same batch
            // may already have authorized leaving this editor.
            if self.navigation_authorized {
                continue;
            }
            // Do not change the model while a real pointer owns an uncommitted draft.
            if self.crop_drag.is_some()
                || self.property_drag.is_some()
                || self.video_drag.is_some()
                || self.timeline_drag.is_some()
                || self.canvas_drag.is_some()
                || self.timeline_scrub.is_some()
            {
                continue;
            }
            self.finish_wheel_edit();
            match action {
                Action::Invoke(id) | Action::Focus(id) => {
                    if let Some(control) = ControlId::ORDER
                        .into_iter()
                        .find(|control| *control as u32 + 1 == id)
                    {
                        let rect = self.control_layout(client_rect(hwnd)).control_rect(control);
                        if self.control_enabled(control)
                            && rect.right > rect.left
                            && rect.bottom > rect.top
                        {
                            self.focused_control = Some(control);
                            self.accessibility_focus = None;
                            self.keyboard_focus = true;
                            if matches!(action, Action::Invoke(_)) {
                                self.activate_control(hwnd, control);
                            }
                        }
                    } else if id == PLAYHEAD_ID || id == 0 {
                        self.focused_control = None;
                        self.accessibility_focus = (id != 0).then_some(id);
                        self.keyboard_focus = id != 0;
                    } else if self.accessibility.key(id).is_some() {
                        self.select_accessible_item(id, true);
                        self.accessibility_focus = Some(id);
                        self.focused_control = None;
                        self.keyboard_focus = true;
                    }
                }
                Action::Select(id) => self.select_accessible_item(id, true),
                Action::ClearSelection(id) => self.select_accessible_item(id, false),
                Action::SetRange(PLAYHEAD_ID, value) => {
                    self.keyboard_focus = true;
                    self.accessibility_focus = Some(PLAYHEAD_ID);
                    self.focused_control = None;
                    if value.is_finite() && value >= 0.0 {
                        self.seek_absolute(TimeTick((value * 10_000_000.0).round() as i64));
                    }
                }
                Action::SetRange(id, value) => {
                    if let Some(control) =
                        ControlId::ORDER.into_iter().find(|c| *c as u32 + 1 == id)
                    {
                        self.apply_widget_range(hwnd, control, value);
                    }
                }
            }
        }
        self.refresh(hwnd, true);
    }

    fn refresh(&mut self, hwnd: HWND, force_title: bool) {
        let now = Instant::now();
        let refresh_chrome = force_title
            || now.saturating_duration_since(self.last_title_refresh) >= Duration::from_millis(100);
        unsafe {
            if refresh_chrome {
                self.publish_accessibility(hwnd);
                self.update_window_title(hwnd);
                self.last_title_refresh = now;
            }
            if force_title {
                let _ = InvalidateRect(Some(hwnd), None, false);
            } else {
                let client = client_rect(hwnd);
                if self.preview_surface.is_none() {
                    let video = preview_video_rect(client, &self.frame, self.theme);
                    let _ = InvalidateRect(Some(hwnd), Some(&raw const video), false);
                }
                if refresh_chrome {
                    let layout = self.control_layout(client);
                    let _ =
                        InvalidateRect(Some(hwnd), Some(&raw const layout.timeline_panel), false);
                    let _ = InvalidateRect(Some(hwnd), Some(&raw const layout.time), false);
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)] // Keep the paired GDI buffer lifetime and caption alpha update together.
    fn paint(&self, hwnd: HWND) {
        let _scale = crate::platform::editor_ui::DrawingScale::enter(self.theme.dpi);
        self.telemetry.paint_calls.fetch_add(1, Ordering::Relaxed);
        let mut paint = PAINTSTRUCT::default();
        let hdc = unsafe { BeginPaint(hwnd, &raw mut paint) };
        let client = client_rect(hwnd);
        let dirty = paint.rcPaint;
        let width = (dirty.right - dirty.left).max(1);
        let height = (dirty.bottom - dirty.top).max(1);
        let buffer_dc = unsafe { CreateCompatibleDC(Some(hdc)) };
        let mut caption_bits = std::ptr::null_mut();
        let buffer_bitmap = if dirty.top < self.theme.metrics.toolbar_height {
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            unsafe {
                windows::Win32::Graphics::Gdi::CreateDIBSection(
                    Some(hdc),
                    &raw const info,
                    DIB_RGB_COLORS,
                    &raw mut caption_bits,
                    None,
                    0,
                )
            }
            .unwrap_or_default()
        } else {
            unsafe { CreateCompatibleBitmap(hdc, width, height) }
        };
        if !buffer_dc.0.is_null() && !buffer_bitmap.0.is_null() {
            let previous = unsafe { SelectObject(buffer_dc, HGDIOBJ(buffer_bitmap.0)) };
            unsafe {
                let _ = SetViewportOrgEx(buffer_dc, -dirty.left, -dirty.top, None);
            }
            draw_scene_region(buffer_dc, client, dirty, self);
            editor_titlebar::paint_system_buttons(hwnd, buffer_dc, self.theme);
            if !caption_bits.is_null() && self.caption_bounds.is_none() {
                // Preserve the opaque header and antialiased icon alpha in live
                // frames; also normalize alpha for the non-native fallback.
                let rows = (self.theme.metrics.toolbar_height - dirty.top).clamp(0, height);
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::GdiFlush();
                    let bytes = std::slice::from_raw_parts_mut(
                        caption_bits.cast::<u8>(),
                        usize::try_from(width * rows * 4).unwrap_or(0),
                    );
                    for pixel in bytes.chunks_exact_mut(4) {
                        pixel[3] = 255;
                    }
                }
            }
            unsafe {
                let _ = SetViewportOrgEx(buffer_dc, 0, 0, None);
            }
            let _ = unsafe {
                BitBlt(
                    hdc,
                    dirty.left,
                    dirty.top,
                    width,
                    height,
                    Some(buffer_dc),
                    0,
                    0,
                    SRCCOPY,
                )
            };
            unsafe {
                SelectObject(buffer_dc, previous);
                let _ = DeleteObject(HGDIOBJ(buffer_bitmap.0));
                let _ = DeleteDC(buffer_dc);
            }
        } else {
            if !buffer_bitmap.0.is_null() {
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(buffer_bitmap.0));
                }
            }
            if !buffer_dc.0.is_null() {
                unsafe {
                    let _ = DeleteDC(buffer_dc);
                }
            }
            draw_scene_region(hdc, client, dirty, self);
            editor_titlebar::paint_system_buttons(hwnd, hdc, self.theme);
        }

        unsafe {
            let _ = EndPaint(hwnd, &raw const paint);
        }
    }
}

fn update_cache_telemetry(telemetry: &WindowTelemetry, session: &ProjectPreviewSession) {
    let cache = session.compositor_cache_stats();
    telemetry
        .background_decodes
        .store(session.background_decodes(), Ordering::Relaxed);
    telemetry
        .source_uploads
        .store(cache.source_uploads, Ordering::Relaxed);
    telemetry
        .background_uploads
        .store(cache.background_uploads, Ordering::Relaxed);
    telemetry
        .output_allocations
        .store(cache.output_allocations, Ordering::Relaxed);
    telemetry
        .readbacks
        .store(cache.readbacks, Ordering::Relaxed);
    telemetry
        .presentations
        .store(cache.presentations, Ordering::Relaxed);
}

fn draw_scene_region(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    client: RECT,
    dirty: RECT,
    state: &WindowState,
) {
    let controls = state.control_layout(client);
    let palette = state.theme.palette;
    fill(hdc, &dirty, palette.window);
    let video = preview_video_rect(client, &state.frame, state.theme);
    for card in [
        controls.preview_card,
        controls.panel,
        controls.timeline_panel,
    ] {
        if card.right > card.left && rects_intersect(dirty, card) {
            rounded_surface(
                hdc,
                card,
                palette.panel,
                palette.panel,
                state.theme.metrics.radius_panel,
            );
        }
    }
    if rects_intersect(dirty, video)
        && let Some(frame) = state.fallback_frame.as_ref()
    {
        draw_frame(hdc, video, frame);
        draw_focus_target(hdc, video, state);
    }
    let full_controls_dirty = rects_intersect(dirty, controls.command_bar)
        || rects_intersect(dirty, controls.panel)
        || rects_intersect(dirty, controls.transport)
        || rects_intersect(dirty, controls.preview_meta);
    let timeline_tracks_dirty = dirty.top >= controls.ruler.top - 3
        && dirty.bottom <= controls.camera_track.bottom + 3
        && rects_intersect(dirty, controls.timeline_panel);
    let timeline_time_dirty = dirty.left >= controls.time.left
        && dirty.top >= controls.time.top
        && dirty.right <= controls.time.right
        && dirty.bottom <= controls.time.bottom;
    if timeline_time_dirty && !full_controls_dirty {
        draw_timeline_time_only(hdc, &controls, state);
    } else if timeline_tracks_dirty && !full_controls_dirty {
        draw_timeline_tracks_only(hdc, &controls, state);
    } else if full_controls_dirty || rects_intersect(dirty, controls.timeline_panel) {
        draw_controls(hdc, &controls, state);
    }
    if state.keyboard_focus
        && let Some(id) = state.accessibility_focus
        && let Some(rect) = state.accessibility.node_rect(id)
        && rects_intersect(dirty, rect)
    {
        focus_ring(hdc, rect, state.theme.palette.accent, state.theme.scale(4));
    }
}

fn draw_timeline_time_only(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    controls: &ControlLayout,
    state: &WindowState,
) {
    let snapshot = state.session.snapshot();
    let display_tick = state
        .timeline_scrub
        .map_or(snapshot.project_tick, |scrub| scrub.desired_tick);
    fill(hdc, &controls.time, state.theme.palette.panel);
    draw_text_styled(
        hdc,
        controls.time,
        &format!(
            "{}  /  {}",
            format_tick(display_tick),
            format_tick(snapshot.duration_tick)
        ),
        state.theme.palette.text_secondary,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        TextStyle::Body,
    );
}

fn rects_intersect(left: RECT, right: RECT) -> bool {
    left.left < right.right
        && left.right > right.left
        && left.top < right.bottom
        && left.bottom > right.top
}

#[allow(clippy::too_many_lines)]
fn draw_timeline_tracks_only(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    controls: &ControlLayout,
    state: &WindowState,
) {
    editor_chrome::draw_tracks(hdc, controls, state);
}

fn timeline_edge_mode(x: i32, left: i32, right: i32, tolerance: i32) -> TimelineDragMode {
    let start = (x - left).abs();
    let end = (x - right).abs();
    if start.min(end) > tolerance {
        TimelineDragMode::Move
    } else if start <= end {
        TimelineDragMode::TrimStart
    } else {
        TimelineDragMode::TrimEnd
    }
}

fn preview_video_rect(client: RECT, frame: &PreparedPreviewFrame, theme: EditorTheme) -> RECT {
    let controls = ControlLayout::new(client, theme);
    let padding = ((controls.preview_well.bottom - controls.preview_well.top) / 16)
        .clamp(theme.scale(8), theme.scale(24));
    video_rect(
        RECT {
            left: controls.preview_well.left + padding,
            top: controls.preview_well.top + padding,
            right: controls.preview_well.right - padding,
            bottom: controls.preview_well.bottom - padding,
        },
        frame.evaluation.output.width,
        frame.evaluation.output.height,
    )
}

fn draw_focus_target(hdc: windows::Win32::Graphics::Gdi::HDC, video: RECT, state: &WindowState) {
    let Some(point) = state.focus_overlay() else {
        return;
    };
    let x = video.left + (point[0] * f64::from(video.right - video.left)).round() as i32;
    let y = video.top + (point[1] * f64::from(video.bottom - video.top)).round() as i32;
    let x = x.clamp(video.left, video.right - 1);
    let y = y.clamp(video.top, video.bottom - 1);
    fill(
        hdc,
        &RECT {
            left: x - 14,
            top: y - 1,
            right: x + 15,
            bottom: y + 2,
        },
        rgb(255, 194, 92),
    );
    fill(
        hdc,
        &RECT {
            left: x - 1,
            top: y - 14,
            right: x + 2,
            bottom: y + 15,
        },
        rgb(255, 194, 92),
    );
}

#[derive(Clone, Copy)]
struct HitRect(RECT);

impl HitRect {
    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.0.left && x < self.0.right && y >= self.0.top && y < self.0.bottom
    }
}

impl std::ops::Deref for HitRect {
    type Target = RECT;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlId {
    Split,
    VideoTrim,
    CameraProperties,
    CameraEnabled,
    Cursor,
    Export,
    Location,
    Home,
    Play,
    Save,
    Undo,
    Redo,
    Add,
    Delete,
    Reset,
    Diagnostics,
    TimelineZoomOut,
    TimelineFit,
    TimelineZoomIn,
    ScaleMinus,
    ScalePlus,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    StartMinus,
    StartPlus,
    EndMinus,
    EndPlus,
    Background,
    BackgroundImage,
    Inset,
    Radius,
    Shadow,
    Back,
    InspectorToggle,
    StartValue,
    EndValue,
    ScaleValue,
    CenterFocus,
    ZoomSlider,
    SwatchMist,
    SwatchPeach,
    SwatchLilac,
    SwatchCharcoal,
    SwatchWhite,
    SwatchSage,
    BackgroundFile,
    CanvasTab,
    VideoTab,
    VideoSpeed,
    SpeedReset,
    SpeedHalf,
    SpeedNormal,
    SpeedOneHalf,
    SpeedDouble,
    SpeedQuadruple,
    CropLeft,
    CropTop,
    CropRight,
    CropBottom,
    CropReset,
    CropWide,
    CropStandard,
    CropSquare,
    CropPortrait,
    WindowMinimize,
    WindowMaximize,
    WindowClose,
    SpeedSection,
    CropSection,
    CropEdit,
    CropLock,
    CanvasOriginal,
    CanvasWide,
    CanvasPortrait,
    CanvasSquare,
    CanvasWidth,
    CanvasHeight,
    PreviousFrame,
    NextFrame,
    NewRecording,
    OpenProject,
    Menu,
    BackgroundColor,
    CursorProperties,
}

impl ControlId {
    const ORDER: [Self; 86] = [
        Self::Menu,
        Self::Diagnostics,
        Self::NewRecording,
        Self::OpenProject,
        Self::Save,
        Self::Export,
        Self::Split,
        Self::VideoTrim,
        Self::CameraProperties,
        Self::CameraEnabled,
        Self::Cursor,
        Self::Location,
        Self::Home,
        Self::Play,
        Self::Undo,
        Self::Redo,
        Self::Add,
        Self::Delete,
        Self::Reset,
        Self::TimelineZoomOut,
        Self::TimelineFit,
        Self::TimelineZoomIn,
        Self::ScaleMinus,
        Self::ScalePlus,
        Self::FocusLeft,
        Self::FocusRight,
        Self::FocusUp,
        Self::FocusDown,
        Self::StartMinus,
        Self::StartPlus,
        Self::EndMinus,
        Self::EndPlus,
        Self::Background,
        Self::BackgroundColor,
        Self::BackgroundImage,
        Self::Inset,
        Self::Radius,
        Self::Shadow,
        Self::Back,
        Self::InspectorToggle,
        Self::StartValue,
        Self::EndValue,
        Self::ScaleValue,
        Self::CenterFocus,
        Self::ZoomSlider,
        Self::SwatchMist,
        Self::SwatchPeach,
        Self::SwatchLilac,
        Self::SwatchCharcoal,
        Self::SwatchWhite,
        Self::SwatchSage,
        Self::BackgroundFile,
        Self::CursorProperties,
        Self::CanvasTab,
        Self::VideoTab,
        Self::VideoSpeed,
        Self::SpeedReset,
        Self::SpeedHalf,
        Self::SpeedNormal,
        Self::SpeedOneHalf,
        Self::SpeedDouble,
        Self::SpeedQuadruple,
        Self::CropLeft,
        Self::CropTop,
        Self::CropRight,
        Self::CropBottom,
        Self::CropReset,
        Self::CropWide,
        Self::CropStandard,
        Self::CropSquare,
        Self::CropPortrait,
        Self::WindowMinimize,
        Self::WindowMaximize,
        Self::WindowClose,
        Self::SpeedSection,
        Self::CropSection,
        Self::CropEdit,
        Self::CropLock,
        Self::CanvasOriginal,
        Self::CanvasWide,
        Self::CanvasPortrait,
        Self::CanvasSquare,
        Self::CanvasWidth,
        Self::CanvasHeight,
        Self::PreviousFrame,
        Self::NextFrame,
    ];

    const fn accessibility_name(self) -> &'static str {
        match self {
            Self::Menu => "主菜单",
            Self::NewRecording => "新录制",
            Self::OpenProject => "打开工程",
            Self::SpeedSection => "展开或收起播放速度",
            Self::CropSection => "展开或收起画面裁剪",
            Self::CropEdit => "在预览中调整裁剪（Enter 完成）",
            Self::CropLock => "锁定裁剪比例",
            Self::CanvasOriginal => "原始画布",
            Self::CanvasWide => "16:9 画布",
            Self::CanvasPortrait => "9:16 画布",
            Self::CanvasSquare => "1:1 画布",
            Self::CanvasWidth => "画布宽度（像素）",
            Self::CanvasHeight => "画布高度（像素）",
            Self::PreviousFrame => "上一帧（←）",
            Self::NextFrame => "下一帧（→）",

            Self::CropLeft => "裁剪左侧（%）",
            Self::CropTop => "裁剪顶部（%）",
            Self::CropRight => "裁剪右侧（%）",
            Self::CropBottom => "裁剪底部（%）",
            Self::CropReset => "恢复完整画面",
            Self::CropWide => "居中裁剪为 16:9",
            Self::CropStandard => "居中裁剪为 4:3",
            Self::CropSquare => "居中裁剪为 1:1",
            Self::CropPortrait => "居中裁剪为 9:16",
            Self::WindowMinimize => "最小化",
            Self::WindowMaximize => "最大化或还原",
            Self::WindowClose => "关闭窗口",
            Self::CanvasTab => "画布属性标签",
            Self::VideoTab => "视频属性标签",
            Self::VideoSpeed => "视频速度（倍）：单击数值输入，拖动滑杆调整",
            Self::SpeedReset => "恢复视频原速",
            Self::SpeedHalf => "视频速度 0.5 倍",
            Self::SpeedNormal => "视频速度 1 倍",
            Self::SpeedOneHalf => "视频速度 1.5 倍",
            Self::SpeedDouble => "视频速度 2 倍",
            Self::SpeedQuadruple => "视频速度 4 倍",
            Self::Back => "返回录制器（未保存时询问）",
            Self::InspectorToggle => "展开或收起属性面板",
            Self::StartValue => "选中片段源开始时间（秒）",
            Self::EndValue => "选中片段源结束时间（秒）",
            Self::ScaleValue => "镜头倍率（倍）",
            Self::CenterFocus => "将镜头焦点居中",
            Self::ZoomSlider => "时间线缩放滑杆",
            Self::SwatchMist => "雾蓝背景",
            Self::SwatchPeach => "暖杏背景",
            Self::SwatchLilac => "淡紫背景",
            Self::SwatchCharcoal => "石墨背景",
            Self::SwatchWhite => "纯白背景",
            Self::SwatchSage => "鼠尾草背景",
            Self::BackgroundFile => "选择或更换背景图片",
            Self::Split => "在播放头分割视频（B）",
            Self::VideoTrim => "视频入点与出点",
            Self::CameraProperties => "镜头时间、倍率与焦点",
            Self::CameraEnabled => "应用或旁路镜头效果",
            Self::Cursor => "显示或隐藏光标",
            Self::CursorProperties => "光标大小…",
            Self::BackgroundColor => "自定义背景颜色…",
            Self::Export => "导出当前编辑结果",
            Self::Location => "打开文件位置",
            Self::Home => "回到时间线起点",
            Self::Play => "播放或暂停视频",
            Self::Save => "保存编辑更改",
            Self::Undo => "撤销上一次编辑",
            Self::Redo => "重做上一次编辑",
            Self::Add => "新增镜头片段",
            Self::Delete => "删除选中的视频或镜头片段",
            Self::Reset => "恢复自动镜头初稿",
            Self::Diagnostics => "设置",
            Self::TimelineZoomOut => "缩小时间线",
            Self::TimelineFit => "让时间线适配整个项目",
            Self::TimelineZoomIn => "放大时间线",
            Self::ScaleMinus => "减小镜头缩放",
            Self::ScalePlus => "增大镜头缩放",
            Self::FocusLeft => "向左移动镜头焦点",
            Self::FocusRight => "向右移动镜头焦点",
            Self::FocusUp => "向上移动镜头焦点",
            Self::FocusDown => "向下移动镜头焦点",
            Self::StartMinus => "镜头起点提前零点一秒",
            Self::StartPlus => "镜头起点延后零点一秒",
            Self::EndMinus => "镜头终点提前零点一秒",
            Self::EndPlus => "镜头终点延后零点一秒",
            Self::Background => "切换纯色背景",
            Self::BackgroundImage => "切换图片背景",
            Self::Inset => "画布边距：单击输入，滚轮连续调整",
            Self::Radius => "画布圆角：单击输入，滚轮连续调整",
            Self::Shadow => "画布阴影：单击输入，滚轮连续调整",
        }
    }

    const fn tooltip(self) -> &'static str {
        match self {
            Self::Play => "播放/暂停（Space）",
            Self::Save => "保存更改（S）",
            Self::Undo => "撤销（Ctrl+Z）",
            Self::Redo => "重做（Ctrl+Y）",
            Self::Home => "回到项目起点（Home）",
            Self::Delete => "删除选中对象（Delete）",
            Self::Reset => "恢复自动镜头初稿（R）",
            Self::Diagnostics => "设置：主题、属性与更多选项",
            other => other.accessibility_name(),
        }
    }

    fn icon(self, state: &WindowState) -> IconKind {
        match self {
            Self::Menu => IconKind::Menu,
            Self::NewRecording => IconKind::Record,
            Self::OpenProject | Self::Location => IconKind::Folder,
            Self::VideoTab => IconKind::Monitor,
            Self::Back => IconKind::Back,
            Self::Split => IconKind::Split,
            Self::CameraEnabled => IconKind::Camera,
            Self::Cursor | Self::CursorProperties => IconKind::Cursor,
            Self::Export => IconKind::Export,
            Self::Home => IconKind::Home,
            Self::Play
                if state.session.snapshot().playback == PlaybackState::Playing
                    || state.deferred_playback.is_requested() =>
            {
                IconKind::Pause
            }
            Self::Play
            | Self::VideoSpeed
            | Self::SpeedHalf
            | Self::SpeedNormal
            | Self::SpeedOneHalf
            | Self::SpeedDouble
            | Self::SpeedQuadruple => IconKind::Play,

            Self::Save => IconKind::Save,
            Self::Undo => IconKind::Undo,
            Self::Redo => IconKind::Redo,
            Self::Add => IconKind::Add,
            Self::Delete => IconKind::Delete,
            Self::WindowMinimize => IconKind::Minimize,
            Self::WindowMaximize => {
                if state.host_hwnd.is_some_and(|hwnd| {
                    unsafe { windows::Win32::UI::WindowsAndMessaging::IsZoomed(hwnd) }.as_bool()
                }) {
                    IconKind::WindowRestore
                } else {
                    IconKind::Maximize
                }
            }
            Self::WindowClose => IconKind::Close,
            Self::SpeedSection | Self::CropSection => IconKind::ChevronDown,

            Self::CropLock => IconKind::Lock,
            Self::PreviousFrame => IconKind::PreviousFrame,
            Self::NextFrame => IconKind::NextFrame,
            Self::CropEdit
            | Self::CanvasOriginal
            | Self::CanvasWide
            | Self::CanvasPortrait
            | Self::CanvasSquare
            | Self::CanvasWidth
            | Self::CanvasHeight
            | Self::CropLeft
            | Self::CropTop
            | Self::CropRight
            | Self::CropBottom
            | Self::CropWide
            | Self::CropStandard
            | Self::CropSquare
            | Self::CropPortrait => IconKind::Crop,
            Self::SpeedReset | Self::CropReset => IconKind::Restore,
            Self::Reset => IconKind::Reset,
            Self::Diagnostics => IconKind::Settings,
            Self::InspectorToggle | Self::CameraProperties => IconKind::Diagnostics,
            Self::TimelineZoomOut | Self::ScaleMinus => IconKind::ZoomOut,
            Self::CenterFocus | Self::TimelineFit => IconKind::Fit,
            Self::ZoomSlider | Self::TimelineZoomIn | Self::ScalePlus => IconKind::ZoomIn,
            Self::FocusLeft => IconKind::ArrowLeft,
            Self::FocusRight => IconKind::ArrowRight,
            Self::FocusUp => IconKind::ArrowUp,
            Self::FocusDown => IconKind::ArrowDown,
            Self::StartMinus => IconKind::TrimStartEarlier,
            Self::StartPlus | Self::VideoTrim => IconKind::TrimStartLater,
            Self::EndMinus => IconKind::TrimEndEarlier,
            Self::EndPlus => IconKind::TrimEndLater,
            Self::CanvasTab
            | Self::BackgroundColor
            | Self::SwatchMist
            | Self::SwatchPeach
            | Self::SwatchLilac
            | Self::SwatchCharcoal
            | Self::SwatchWhite
            | Self::SwatchSage
            | Self::Background => IconKind::Background,
            Self::StartValue | Self::EndValue | Self::ScaleValue => IconKind::Edit,
            Self::BackgroundImage | Self::BackgroundFile => IconKind::Image,
            Self::Inset => IconKind::Inset,
            Self::Radius => IconKind::Radius,
            Self::Shadow => IconKind::Shadow,
        }
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if matches!(
        message,
        WM_INTERACTIVE_RENDER | WM_SCRUB_PREVIEW | WM_SCRUB_READY
    ) && unsafe { state_mut(hwnd) }.is_none_or(|state| state.generation != wparam.0)
    {
        return LRESULT(0);
    }
    if let Some(result) = editor_titlebar::window_message(hwnd, message, wparam, lparam) {
        return result;
    }
    if let Some(result) = crate::platform::editor_ui::native_control_color(message, wparam) {
        return result;
    }
    if let Some(result) =
        crate::platform::accessibility::window_message(hwnd, message, wparam, lparam)
    {
        return result;
    }
    if message == WM_NCCREATE {
        let _ = lparam.0 as *const CREATESTRUCTW;
    }
    match message {
        crate::platform::inline_input::WM_INLINE_DONE => {
            let mut restore_focus = false;
            if let Some(state) = unsafe { state_mut(hwnd) }
                && state
                    .inline_input
                    .as_ref()
                    .is_some_and(|input| input.hwnd.0 as usize == wparam.0)
            {
                let completed = state.finish_inline_input(hwnd, lparam.0 != 0);
                restore_focus = completed && lparam.0 != 1;
                if completed && lparam.0 >= 3 {
                    state.move_keyboard_focus(lparam.0 == 4);
                }
            }
            if restore_focus {
                unsafe {
                    let _ = SetFocus(Some(hwnd));
                }
            }
            LRESULT(0)
        }
        crate::platform::accessibility::WM_ACCESS_ACTION => {
            // Focus may synchronously dispatch WM_SETFOCUS: do it before borrowing state.
            unsafe {
                let _ = SetFocus(Some(hwnd));
            }
            if let Some(state) = unsafe { state_mut(hwnd) }
                && state.finish_inline_input(hwnd, true)
            {
                state.accessibility_actions(hwnd);
            }
            LRESULT(0)
        }
        WM_SCRUB_READY => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.accept_scrub_preview(hwnd);
            }
            LRESULT(0)
        }
        WM_SCRUB_PREVIEW => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.submit_scrub_preview(hwnd);
            }
            LRESULT(0)
        }
        WM_INTERACTIVE_RENDER => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.poll_interactive_render(hwnd);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_ID => {
            if let Some(state) = unsafe { state_mut(hwnd) }
                && state.frame_clock.is_none()
            {
                state.on_timer(hwnd);
            }
            LRESULT(0)
        }
        WM_FRAME_CLOCK => {
            if let Some(state) = unsafe { state_mut(hwnd) }
                && state
                    .frame_clock
                    .as_ref()
                    .is_some_and(|clock| clock.accept(wparam.0))
            {
                state.on_timer(hwnd);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if let Some(state) = unsafe { state_mut(hwnd) }
                && state.scrub_smoke.is_none()
            {
                state.on_key(hwnd, wparam.0 as u16);
            }
            LRESULT(0)
        }
        WM_SETFOCUS => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.on_keyboard_focus(hwnd, true);
            }
            LRESULT(0)
        }
        WM_KILLFOCUS => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.on_keyboard_focus(hwnd, false);
            }
            LRESULT(0)
        }
        WM_CANCELMODE => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                if state.scrub_smoke.is_some() {
                    state
                        .telemetry
                        .scripted_os_interruptions
                        .fetch_add(1, Ordering::Relaxed);
                } else {
                    state.cancel_gesture(hwnd);
                    state.cancel_pressed_control(hwnd, true);
                }
            }
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                // The CLI probe owns a synthetic gesture, not the physical mouse.
                // Keep real-user capture loss cancellable, while recording external
                // OS interruptions separately from deterministic media feedback.
                if state.scrub_smoke.is_some() {
                    state
                        .telemetry
                        .scripted_os_interruptions
                        .fetch_add(1, Ordering::Relaxed);
                    return LRESULT(0);
                }
                state.clear_pressed_control(hwnd);
                if state.video_drag.is_some()
                    || state.crop_drag.is_some()
                    || state.canvas_drag.is_some()
                    || state.timeline_drag.is_some()
                    || state.timeline_scrub.is_some()
                    || state.timeline_pan.is_some()
                    || state.inspector_scroll_drag.is_some()
                    || state.inspector_resize.is_some()
                    || state.timeline_resize.is_some()
                    || state.timeline_scroll.is_some()
                    || state.property_drag.is_some()
                {
                    state.cancel_gesture(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if let Some(state) = unsafe { state_mut(hwnd) }
                && state.scrub_smoke.is_none()
            {
                state.on_mouse_down(hwnd, signed_low(lparam.0), signed_high(lparam.0));
            }
            LRESULT(0)
        }
        WM_SETCURSOR if signed_low(lparam.0) == 1 => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                let mut point = POINT::default();
                if unsafe { GetCursorPos(&raw mut point) }.is_ok() {
                    unsafe {
                        let _ = ScreenToClient(hwnd, &raw mut point);
                    }
                    state.update_cursor(hwnd, point.x, point.y);
                    return LRESULT(1);
                }
            }
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        WM_MOUSEMOVE => {
            // The scripted probe calls these same handlers itself. Do not interleave
            // physical / synthesized OS pointer messages with its deterministic path.
            if let Some(state) = unsafe { state_mut(hwnd) }
                && state.scrub_smoke.is_none()
            {
                state.on_mouse_move(hwnd, signed_low(lparam.0), signed_high(lparam.0));
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.on_mouse_leave(hwnd);
            }
            LRESULT(0)
        }
        WM_MBUTTONDOWN => {
            if let Some(state) = unsafe { state_mut(hwnd) }
                && !state.navigation_authorized
            {
                let x = signed_low(lparam.0);
                let y = signed_high(lparam.0);
                if state
                    .control_layout(client_rect(hwnd))
                    .timeline_area()
                    .contains(x, y)
                {
                    state.timeline_pan = Some((x, state.timeline));
                    unsafe {
                        SetCapture(hwnd);
                    }
                }
            }
            LRESULT(0)
        }
        WM_MBUTTONUP => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.timeline_pan = None;
            }
            unsafe {
                let _ = ReleaseCapture();
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(state) = unsafe { state_mut(hwnd) }
                && state.scrub_smoke.is_none()
            {
                state.on_mouse_up(hwnd, signed_low(lparam.0), signed_high(lparam.0));
                state.update_cursor(hwnd, signed_low(lparam.0), signed_high(lparam.0));
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            if let Some(state) = unsafe { state_mut(hwnd) }
                && state.scrub_smoke.is_none()
            {
                let mut point = POINT {
                    x: signed_low(lparam.0),
                    y: signed_high(lparam.0),
                };
                unsafe {
                    let _ = ScreenToClient(hwnd, &raw mut point);
                }
                state.on_mouse_wheel(
                    hwnd,
                    (signed_high(wparam.0.cast_signed())
                        * if message == WM_MOUSEHWHEEL { -1 } else { 1 })
                        as i16,
                    point.x,
                    point.y,
                );
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                if state.finish_inline_input(hwnd, true) {
                    state.on_close(hwnd);
                }
            } else {
                let _ = unsafe { DestroyWindow(hwnd) };
            }
            LRESULT(0)
        }
        WM_PAINT => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.paint(hwnd);
            } else {
                let mut paint = PAINTSTRUCT::default();
                unsafe {
                    BeginPaint(hwnd, &raw mut paint);
                    let _ = EndPaint(hwnd, &raw const paint);
                }
            }
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let limits = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            let theme =
                unsafe { state_mut(hwnd) }.map_or_else(EditorTheme::light, |state| state.theme);
            let (x, y) = crate::platform::editor_ui::available_window_size(
                theme.scale(1_040),
                theme.scale(760),
            );
            limits.ptMinTrackSize = POINT { x, y };
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let dpi = u32::try_from((wparam.0 >> 16) & 0xffff)
                .unwrap_or(96)
                .max(96);
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.finish_inline_input(hwnd, false);
                state.cancel_gesture(hwnd);
                let old = state.theme;
                state.theme = EditorTheme::current(dpi);
                state.theme.metrics.inspector_width =
                    (i64::from(old.metrics.inspector_width) * i64::from(dpi) / i64::from(old.dpi))
                        as i32;
                state.theme.metrics.timeline_height =
                    (i64::from(old.metrics.timeline_height) * i64::from(dpi) / i64::from(old.dpi))
                        as i32;
                state.inspector_scroll = (i64::from(state.inspector_scroll) * i64::from(dpi)
                    / i64::from(old.dpi)) as i32;
            }
            let suggested = unsafe { &*(lparam.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.resize_preview_surface(hwnd);
            }
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_SIZE => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.finish_inline_input(hwnd, false);
                state.resize_preview_surface(hwnd);
            }
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut WindowState> {
    let pointer = unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetPropW(hwnd, w!("PanzoEditorState")).0
    }
    .cast::<WindowState>();
    unsafe { pointer.as_mut() }
}

fn client_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &raw mut rect) };
    rect
}

fn video_rect(bounds: RECT, width: u32, height: u32) -> RECT {
    let available_width = (bounds.right - bounds.left).max(1);
    let available_height = (bounds.bottom - bounds.top).max(1);
    let mut destination_width = available_width;
    let mut destination_height =
        i32::try_from(i64::from(destination_width) * i64::from(height) / i64::from(width))
            .unwrap_or(available_height);
    if destination_height > available_height {
        destination_height = available_height;
        destination_width =
            i32::try_from(i64::from(destination_height) * i64::from(width) / i64::from(height))
                .unwrap_or(available_width);
    }
    let left = bounds.left + (available_width - destination_width) / 2;
    let top = bounds.top + (available_height - destination_height) / 2;
    RECT {
        left,
        top,
        right: left + destination_width,
        bottom: top + destination_height,
    }
}

fn draw_frame(hdc: windows::Win32::Graphics::Gdi::HDC, destination: RECT, frame: &PreviewFrame) {
    let source_width = i32::try_from(frame.composited.width).unwrap_or(i32::MAX);
    let source_height = i32::try_from(frame.composited.height).unwrap_or(i32::MAX);
    let bitmap = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: source_width,
            biHeight: -source_height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        SetStretchBltMode(hdc, HALFTONE);
        StretchDIBits(
            hdc,
            destination.left,
            destination.top,
            destination.right - destination.left,
            destination.bottom - destination.top,
            0,
            0,
            source_width,
            source_height,
            Some(frame.composited.pixels.as_ptr().cast::<c_void>()),
            &raw const bitmap,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
}

fn scrub_probe_progress(elapsed: f64, duration: f64) -> f64 {
    // A five-minute resource run repeats the ten-second gesture. Stretching
    // one sweep over five minutes moves only a few integer pixels per second
    // on a fit-to-project timeline; unchanged pointer coordinates are not a
    // decoder stall and cannot exercise cache eviction or continuous feedback.
    let period = duration.clamp(1.0, 10.0);
    let phase = elapsed % period;
    let half = period / 2.0;
    if phase < half {
        phase / half
    } else {
        (period - phase) / half
    }
}

fn draw_control_tooltip(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    controls: &ControlLayout,
    control: ControlId,
    theme: EditorTheme,
) {
    crate::platform::editor_ui::draw_tooltip(
        hdc,
        RECT {
            left: controls.command_bar.left,
            top: controls.command_bar.top,
            right: controls.command_bar.right,
            bottom: controls.timeline_panel.bottom + theme.metrics.outer_margin,
        },
        controls.control_rect(control).0,
        control.tooltip(),
        theme,
    );
}

fn draw_timeline_ruler(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    ruler: HitRect,
    viewport: TimelineViewport,
    theme: EditorTheme,
) {
    fill(hdc, &ruler, theme.palette.panel);
    let width = (ruler.right - ruler.left).max(1);
    let target = i64::try_from(
        i128::from(viewport.visible_duration().as_i64()) * i128::from(theme.scale(104))
            / i128::from(width),
    )
    .unwrap_or(TICKS_PER_SECOND);
    let step = TIMELINE_RULER_STEPS
        .iter()
        .copied()
        .find(|step| *step >= target)
        .unwrap_or(60_000_000_000);
    let mut tick = viewport.visible_start().as_i64().div_euclid(step) * step;
    if tick < viewport.visible_start().as_i64() {
        tick = tick.saturating_add(step);
    }
    let mut last_label_right = ruler.left - theme.scale(12);
    while tick <= viewport.visible_end().as_i64() {
        let x = timeline_x(ruler, TimeTick(tick), viewport);
        fill(
            hdc,
            &RECT {
                left: x,
                top: ruler.bottom - theme.scale(7),
                right: x + 1,
                bottom: ruler.bottom,
            },
            theme.palette.border,
        );
        let label = format_tick(TimeTick(tick));
        let label_left = x + theme.scale(5);
        let label_width = measure_text_width(hdc, &label, TextStyle::Caption);
        if label_left >= last_label_right + theme.scale(12)
            && label_left + label_width <= ruler.right
        {
            draw_text_styled(
                hdc,
                RECT {
                    left: label_left,
                    top: ruler.top,
                    right: label_left + label_width,
                    bottom: ruler.bottom - theme.scale(3),
                },
                &label,
                theme.palette.text_muted,
                DT_LEFT | DT_SINGLELINE,
                TextStyle::Caption,
            );
            last_label_right = label_left + label_width;
        }
        tick = tick.saturating_add(step);
    }
}

fn timeline_x(track: HitRect, tick: TimeTick, viewport: TimelineViewport) -> i32 {
    track.left + viewport.tick_to_offset(tick, (track.right - track.left).max(1))
}

fn timeline_tick(track: HitRect, x: i32, viewport: TimelineViewport) -> TimeTick {
    viewport.offset_to_tick(x - track.left, (track.right - track.left).max(1))
}

fn timeline_delta(track: HitRect, pixels: i32, viewport: TimelineViewport) -> TimeTick {
    viewport.pixel_delta_to_tick(pixels, (track.right - track.left).max(1))
}

fn segment_rect(track: HitRect, segment: &CameraSegment, viewport: TimelineViewport) -> RECT {
    let left = timeline_x(track, segment.start_tick, viewport);
    let right = timeline_x(track, segment.end_tick, viewport).max(left + 3);
    RECT {
        left,
        top: track.top + 4,
        right,
        bottom: track.bottom - 4,
    }
}

fn border(hdc: windows::Win32::Graphics::Gdi::HDC, rect: RECT, color: COLORREF, width: i32) {
    fill(
        hdc,
        &RECT {
            bottom: rect.top + width,
            ..rect
        },
        color,
    );
    fill(
        hdc,
        &RECT {
            top: rect.bottom - width,
            ..rect
        },
        color,
    );
    fill(
        hdc,
        &RECT {
            right: rect.left + width,
            ..rect
        },
        color,
    );
    fill(
        hdc,
        &RECT {
            left: rect.right - width,
            ..rect
        },
        color,
    );
}

fn button(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    state: &WindowState,
    id: ControlId,
    selected: bool,
    enabled: bool,
) {
    use crate::platform::editor_ui::{ButtonRole, ButtonVisualState, draw_button};
    draw_button(
        hdc,
        rect,
        "",
        id.icon(state),
        state.theme,
        ButtonRole::Toolbar,
        ButtonVisualState {
            interaction: button_interaction(
                enabled,
                state.pressed_control == Some(id),
                state.hot_control == Some(id),
            ),
            selected,
        },
        state.keyboard_focus && state.focused_control == Some(id),
    );
}

fn fill(hdc: windows::Win32::Graphics::Gdi::HDC, rect: &RECT, color: COLORREF) {
    let brush = unsafe { CreateSolidBrush(color) };
    unsafe {
        FillRect(hdc, rect, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}

fn draw_text_styled(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    mut rect: RECT,
    value: &str,
    color: COLORREF,
    format: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT,
    style: TextStyle,
) {
    let mut text: Vec<u16> = value.encode_utf16().collect();
    if text.is_empty() || rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let previous_font = select_font(hdc, style);
    let format = if format.0 & DT_SINGLELINE.0 != 0 {
        format | DT_END_ELLIPSIS | DT_NOPREFIX
    } else {
        format | DT_NOPREFIX
    };
    unsafe {
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, color);
        DrawTextW(hdc, &mut text, &raw mut rect, format);
        SelectObject(hdc, previous_font);
    }
}

const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(red as u32 | (green as u32) << 8 | (blue as u32) << 16)
}

fn duration_tick(duration: Duration) -> Result<TimeTick, ClockError> {
    let tick = duration.as_nanos() / 100;
    i64::try_from(tick)
        .map(TimeTick)
        .map_err(|_| ClockError::Overflow)
}

const fn timer_requires_render(playback: PlaybackState) -> bool {
    matches!(playback, PlaybackState::Playing)
}

fn format_tick(tick: TimeTick) -> String {
    let total_millis = tick.as_i64().max(0) / 10_000;
    let minutes = total_millis / 60_000;
    let seconds = (total_millis / 1_000) % 60;
    let millis = total_millis % 1_000;
    format!("{minutes:02}:{seconds:02}.{millis:03}")
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn choose_background_image(hwnd: HWND) -> Option<std::path::PathBuf> {
    let filter = wide("图片文件\0*.png;*.jpg;*.jpeg;*.bmp\0所有文件\0*.*\0");
    let title = wide("将背景图片导入 Panzo 项目");
    let mut file = vec![0_u16; 32_768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&raw mut dialog) }.as_bool() {
        return None;
    }
    let length = file.iter().position(|value| *value == 0)?;
    Some(std::ffi::OsString::from_wide(&file[..length]).into())
}

fn signed_low(value: isize) -> i32 {
    let raw = value.cast_unsigned();
    let low = u16::try_from(raw & 0xffff).unwrap_or_default();
    i32::from(low.cast_signed())
}

fn signed_high(value: isize) -> i32 {
    let raw = value.cast_unsigned();
    let high = u16::try_from((raw >> 16) & 0xffff).unwrap_or_default();
    i32::from(high.cast_signed())
}

fn win<T>(stage: &'static str, result: windows::core::Result<T>) -> Result<T, PreviewWindowError> {
    result.map_err(|source| PreviewWindowError::WindowsStage { stage, source })
}

#[derive(Debug, Error)]
pub enum PreviewWindowError {
    #[error("main window failed: {0}")]
    Host(String),
    #[error(transparent)]
    Session(#[from] PreviewSessionError),
    #[error(transparent)]
    ScrubPreview(#[from] ScrubPreviewWorkerError),
    #[error(transparent)]
    Editor(#[from] EditorSessionError),
    #[error(transparent)]
    Geometry(#[from] panzo_core::geometry::GeometryError),
    #[error("preview clock failed: {0}")]
    Clock(#[from] ClockError),
    #[error("preview smoke duration must be positive")]
    InvalidSmokeDuration,
    #[error("select a Camera Segment first")]
    NoSelectedSegment,
    #[error("Windows stage '{stage}' failed: {source}")]
    WindowsStage {
        stage: &'static str,
        #[source]
        source: WindowsError,
    },
}

#[derive(Debug, Error)]
pub enum ClockError {
    #[error("elapsed duration exceeds the Panzo timebase")]
    Overflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "explicit disposable fixture and UI review directory required"]
    #[allow(clippy::too_many_lines)]
    fn editor_feedback_ui_review() {
        use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
        let project =
            std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
        let output = std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
        let report = PreviewWindow::run_internal_notifying(&project, Some(Duration::from_millis(500)), SmokeAction::None,
            Some(Box::new(move |raw| {
                let hwnd = HWND(raw as *mut c_void);
                let state = unsafe { state_mut(hwnd) }.unwrap();
                let capture = |label: &str| {
                    use windows::Win32::Graphics::Gdi::{RedrawWindow, RDW_INVALIDATE, RDW_UPDATENOW, RDW_ALLCHILDREN};
                    unsafe { let _ = RedrawWindow(Some(hwnd), None, None, RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN); }
                    let rect = client_rect(hwnd);
                    crate::platform::editor_ui::render_review_png(&output.join(label), rect.right, rect.bottom, |dc, _| {
                        assert!(unsafe { PrintWindow(hwnd, dc, PRINT_WINDOW_FLAGS(2)) }.as_bool());
                    });
                };
                let original = state.editor.settings().canvas.inset;
                state.begin_inline_input(hwnd, ControlId::Inset);
                let field = state.inline_input.as_ref().unwrap().hwnd;
                unsafe { SetWindowTextW(field, w!("999")).unwrap(); }
                assert!(!state.finish_inline_input(hwnd, true));
                assert_eq!(state.editor.settings().canvas.inset, original);
                assert!(state.inline_validation.as_ref().unwrap().contains("0–25%"));
                assert_eq!(state.status_message().1, StatusSeverity::Warning);
                capture("input-validation.png");
                assert!(state.finish_inline_input(hwnd, false));
                assert!(state.inline_validation.is_none());
                assert_eq!(state.editor.settings().canvas.inset, original);

                let errors = state.telemetry.media_errors.load(Ordering::Relaxed);
                state.record_session_result(Err(PreviewSessionError::PreparationCancelled));
                assert_eq!(state.telemetry.media_errors.load(Ordering::Relaxed), errors);
                assert!(state.feedback.current().is_none());

                state.select_video_properties();
                if state.editor.video_edit().clips.len() == 1 {
                    assert!(!state.control_enabled(ControlId::Delete));
                    let before = state.editor.video_edit();
                    state.delete_selected();
                    assert_eq!(state.editor.video_edit(), before);
                    assert_eq!(state.status_message().1, StatusSeverity::Information);
                }
                state.session.seek(TimeTick::ZERO).unwrap();
                assert!(!state.control_enabled(ControlId::Split));
                state.split_video();
                assert_eq!(state.status_message().1, StatusSeverity::Information);
                let midpoint = TimeTick(state.editor.project_duration().0 / 2);
                state.session.seek(midpoint).unwrap();
                assert!(state.control_enabled(ControlId::Split));
                state.split_video();
                assert!(!state.control_enabled(ControlId::Split));
                state.undo();

                let (save_task, save_sender) = crate::platform::editor_tasks::SaveTask::review_pending();
                state.save_task = Some(save_task);
                save_sender.send(Err("injected access denied (os error 5)".into())).unwrap();
                state.poll_save(hwnd);
                state.record_editor_result(Ok(()), "参数已更新");
                state.report_problem(StatusScope::Preview, "预览尚未更新，请重新定位时间线", "injected decoder detail");
                state.resolve_problem(StatusScope::Preview);
                assert!(state.status_message().0.starts_with("修改尚未保存"));
                assert!(!state.status_message().0.contains("os error"));
                assert!(state.feedback.details().contains("os error 5"));
                capture("save-failure-retained.png");

                state.editor.set_canvas_inset(if original == 0.08 { 0.09 } else { 0.08 }).unwrap();
                state.save_editor(CameraEditKind::Update);
                let started = Instant::now();
                while state.save_task.is_some() {
                    state.poll_save(hwnd);
                    assert!(started.elapsed() < Duration::from_secs(10));
                    std::thread::sleep(Duration::from_millis(5));
                }
                assert!(state.feedback.current().is_none());
                assert!(!state.editor.is_dirty());
                capture("save-recovered.png");
                std::fs::write(output.join("feedback-review.txt"), "Invalid input retained with local guidance; cancel preserves value; stale preparation not an error; split/delete boundaries blocked; save issue survives unrelated updates; real successful save resolves it. Failure injected, no physical mouse input.\n").unwrap();
            }))).unwrap();
        assert!(report.graceful_close);
        assert_eq!(report.media_errors, 0);
    }

    #[test]
    #[ignore = "explicit disposable project and native window required"]
    fn focus_drag_reaches_frame_edges_ui_review() {
        let project =
            std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
        for (dpi, inset) in [(96, 0.0), (144, 0.09), (192, 0.25)] {
            let report = PreviewWindow::run_internal_notifying(
                &project,
                Some(Duration::from_millis(300)),
                SmokeAction::None,
                Some(Box::new(move |raw| {
                    let hwnd = HWND(raw as *mut std::ffi::c_void);
                    let state = unsafe { state_mut(hwnd) }.unwrap();
                    state.theme = EditorTheme::for_dpi(dpi);
                    state.editor.set_camera_enabled(false);
                    state.editor.set_canvas_inset(inset).unwrap();
                    state.session.seek(TimeTick::ZERO).unwrap();
                    state.apply_editor_state().unwrap();
                    state.add_manual_segment();
                    state.frame = state.session.prepare_current().unwrap();
                    let id = state.selected_segment.clone().unwrap();
                    let video = preview_video_rect(client_rect(hwnd), &state.frame, state.theme);
                    let width = f64::from(video.right - video.left);
                    let height = f64::from(video.bottom - video.top);
                    let middle = (
                        i32::midpoint(video.left, video.right),
                        i32::midpoint(video.top, video.bottom),
                    );
                    for (x, y) in [
                        (0.0, 0.0),
                        (1.0, 0.0),
                        (1.0, 1.0),
                        (0.0, 1.0),
                        (0.0, 0.5),
                        (1.0, 0.5),
                        (0.5, 0.0),
                        (0.5, 1.0),
                    ] {
                        let pointer_x =
                            video.left + ((inset + x * (1.0 - 2.0 * inset)) * width).round() as i32;
                        let pointer_y =
                            video.top + ((inset + y * (1.0 - 2.0 * inset)) * height).round() as i32;
                        let before = state.editor.camera().clone();
                        state.on_mouse_down(hwnd, middle.0, middle.1);
                        assert!(state.canvas_drag.is_some());
                        state.last_drag_refresh =
                            Instant::now().checked_sub(DRAG_REFRESH_INTERVAL).unwrap();
                        state.on_mouse_move(hwnd, pointer_x, pointer_y);
                        state.on_mouse_up(hwnd, pointer_x, pointer_y);
                        assert!(state.canvas_drag.is_none());
                        let focus = state.selected_segment().unwrap().focus_point();
                        assert!((focus.x - x).abs() < 3.0 / width, "dpi {dpi}: {focus:?}");
                        assert!((focus.y - y).abs() < 3.0 / height, "dpi {dpi}: {focus:?}");
                        let overlay = state.focus_overlay().unwrap();
                        assert!(
                            (f64::from(video.left) + overlay[0] * width - f64::from(pointer_x))
                                .abs()
                                <= 1.0
                        );
                        assert!(
                            (f64::from(video.top) + overlay[1] * height - f64::from(pointer_y))
                                .abs()
                                <= 1.0
                        );
                        state.editor.camera().validate().unwrap();
                        assert!(state.editor.undo());
                        assert_eq!(*state.editor.camera(), before, "one drag must be one undo");
                        assert!(state.editor.redo());
                        state.apply_editor_state().unwrap();
                    }
                    let before_cancel = state.editor.camera().clone();
                    state.on_mouse_down(hwnd, middle.0, middle.1);
                    state.last_drag_refresh =
                        Instant::now().checked_sub(DRAG_REFRESH_INTERVAL).unwrap();
                    state.on_mouse_move(hwnd, video.left - 100, video.bottom + 100);
                    state.cancel_gesture(hwnd);
                    assert_eq!(*state.editor.camera(), before_cancel);
                    // A zoomed preview maps the selected visible point back to its source point.
                    let transform = panzo_core::CameraTransform::from(
                        CameraState::focused(NormalizedPoint::new(0.6, 0.4).unwrap(), 2.0).unwrap(),
                    );
                    let drag = CanvasDrag {
                        current_x: middle.0,
                        current_y: middle.1,
                        original: state.selected_segment().unwrap(),
                        initial_transform: transform,
                    };
                    let focus = state.canvas_focus(hwnd, &drag).unwrap();
                    assert!((focus.x - 0.6).abs() < 3.0 / width);
                    assert!((focus.y - 0.4).abs() < 3.0 / height);
                    state.editor.set_segment_focus(&id, focus, 2.0).unwrap();
                    state.frame.evaluation.camera_transform = transform;
                    let overlay = state.focus_overlay().unwrap();
                    assert!((overlay[0] - 0.5).abs() < 3.0 / width);
                    assert!((overlay[1] - 0.5).abs() < 3.0 / height);
                })),
            )
            .unwrap();
            assert!(report.graceful_close);
            assert_eq!(report.media_errors, 0);
        }
    }

    #[test]
    #[ignore = "explicit local fixture and native window required"]
    #[allow(clippy::too_many_lines)] // Exercise both splitters and cancellation in one native window.
    fn exercise_layout_ui_review() {
        let project =
            std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
        let report = PreviewWindow::run_internal_notifying(
            &project,
            Some(Duration::from_millis(300)),
            SmokeAction::None,
            Some(Box::new(|raw| {
                let hwnd = HWND(raw as *mut std::ffi::c_void);
                let state = unsafe { state_mut(hwnd) }.unwrap();
                let client = client_rect(hwnd);
                let before = state.control_layout(client);
                let y = before.panel.top + state.theme.scale(60);
                let splitter_x = i32::midpoint(
                    before.inspector_splitter.left,
                    before.inspector_splitter.right,
                );
                state.on_mouse_down(hwnd, splitter_x, y);
                assert!(state.inspector_resize.is_some());
                let x = splitter_x - state.theme.scale(96);
                state.on_mouse_move(hwnd, x, y);
                state.on_mouse_up(hwnd, x, y);
                let width = state.theme.metrics.inspector_width;
                assert!(width > before.panel.right - before.panel.left);
                assert!(state.inspector_resize.is_none());
                assert!(!state.editor.is_dirty(), "layout must not edit the project");
                state.activate_control(hwnd, ControlId::InspectorToggle);
                assert_eq!(state.theme.metrics.inspector_width, 0);
                state.activate_control(hwnd, ControlId::InspectorToggle);
                assert!(
                    (state.theme.metrics.inspector_width - width).abs() <= state.theme.scale(1)
                );
                let c = state.control_layout(client);
                let original = state.theme.metrics.inspector_width;
                let splitter_x =
                    i32::midpoint(c.inspector_splitter.left, c.inspector_splitter.right);
                state.on_mouse_down(hwnd, splitter_x, y);
                state.on_mouse_move(hwnd, splitter_x + state.theme.scale(40), y);
                state.cancel_gesture(hwnd);
                assert_eq!(state.theme.metrics.inspector_width, original);
                let splitter_y = i32::midpoint(c.splitter.top, c.splitter.bottom);
                state.on_mouse_down(hwnd, state.theme.scale(20), splitter_y);
                assert!(state.timeline_resize.is_some());
                state.on_mouse_move(
                    hwnd,
                    state.theme.scale(10),
                    splitter_y - state.theme.scale(2000),
                );
                state.on_mouse_up(
                    hwnd,
                    state.theme.scale(10),
                    splitter_y - state.theme.scale(2000),
                );
                assert_eq!(
                    state.theme.metrics.timeline_height,
                    ControlLayout::clamp_timeline_height(client.bottom, i32::MAX, state.theme)
                );
                let taller = state.control_layout(client);
                assert!(
                    taller.video_track.bottom - taller.video_track.top
                        >= c.video_track.bottom - c.video_track.top
                );
                assert!(
                    taller.camera_track.bottom - taller.camera_track.top
                        >= c.camera_track.bottom - c.camera_track.top
                );
                assert_eq!(
                    taller.scrollbar.top - taller.camera_track.bottom,
                    state.theme.scale(8)
                );
                let expanded_height = state.theme.metrics.timeline_height;
                let y = i32::midpoint(taller.splitter.top, taller.splitter.bottom);
                state.on_mouse_down(hwnd, state.theme.scale(20), y);
                state.on_mouse_move(hwnd, state.theme.scale(20), y + state.theme.scale(100));
                state.on_mouse_up(hwnd, state.theme.scale(20), y + state.theme.scale(100));
                assert!(state.theme.metrics.timeline_height < expanded_height);
                let compact = state.control_layout(client);
                assert!(compact.preview_card.bottom > taller.preview_card.bottom);
                assert_eq!(compact.preview_card.bottom, compact.panel.bottom);
                let y = i32::midpoint(compact.splitter.top, compact.splitter.bottom);
                let compact_height = state.theme.metrics.timeline_height;
                state.on_mouse_down(hwnd, state.theme.scale(20), y);
                state.on_mouse_move(hwnd, state.theme.scale(20), y - state.theme.scale(50));
                state.cancel_gesture(hwnd);
                assert_eq!(state.theme.metrics.timeline_height, compact_height);
                assert!(!state.editor.is_dirty());
                let taller = compact;
                state.on_mouse_move(hwnd, taller.add.left + 8, taller.add.top + 8);
                assert_eq!(state.hot_control, Some(ControlId::Add));
                assert!(!state.tooltip_visible);
                assert!(state.tooltip_deadline.is_some());
                state.on_mouse_leave(hwnd);
                assert!(!state.tooltip_visible);
                assert!(state.hot_control.is_none());
                state.selected_video = state
                    .editor
                    .video_edit()
                    .spans()
                    .next()
                    .map(|s| s.clip.id.clone());
                state.activate_control(hwnd, ControlId::InspectorToggle);
                let hidden = state.control_layout(client);
                assert_eq!(hidden.panel.left, hidden.panel.right);
                let field = hidden.start_value;
                assert!(field.top >= hidden.context_bar.top && field.bottom <= hidden.ruler.top);
                let x = i32::midpoint(field.left, field.right);
                let y = i32::midpoint(field.top, field.bottom);
                state.on_mouse_down(hwnd, x, y);
                state.on_mouse_up(hwnd, x, y);
                assert_eq!(state.inline_control, Some(ControlId::StartValue));
                assert!(state.inline_input.is_some());
                assert!(state.finish_inline_input(hwnd, false));
                assert!(!state.editor.is_dirty());
            })),
        )
        .unwrap();
        assert!(report.graceful_close);
        assert_eq!(report.media_errors, 0);
    }

    #[test]
    #[ignore = "explicit local fixture and UI review output directory required"]
    #[allow(clippy::too_many_lines)] // Exercise native field, slider, tabs and trim as one user workflow.
    fn video_speed_ui_review() {
        use panzo_core::TimeMapping;
        use std::path::PathBuf;
        let project = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
        let output = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
        let report = PreviewWindow::run_internal_notifying(
            &project,
            Some(Duration::from_millis(300)),
            SmokeAction::None,
            Some(Box::new(move |raw| {
                let hwnd = HWND(raw as *mut std::ffi::c_void);
                let state = unsafe { state_mut(hwnd) }.unwrap();
                let original = state.editor.video_edit();
                state.session.seek(TimeTick::from_millis(3000)).unwrap();
                state.activate_control(hwnd, ControlId::VideoTab);
                assert!(state.inspector_video_tab);
                assert!(state.selected_video.is_some());
                state.activate_control(hwnd, ControlId::SpeedDouble);
                assert_eq!(state.widget_value(ControlId::VideoSpeed), Some(2.0));
                assert_eq!(
                    state
                        .editor
                        .video_edit()
                        .source_time(state.session.snapshot().project_tick)
                        .unwrap(),
                    TimeTick::from_millis(3000)
                );
                state.undo();
                assert_eq!(state.editor.video_edit(), original);
                assert!(!state.editor.is_dirty());
                state.redo();
                state.activate_control(hwnd, ControlId::SpeedReset);
                let controls = state.control_layout(client_rect(hwnd));
                let r = controls.video_speed;
                let y = r.bottom - state.theme.scale(10);
                let left = r.left + state.theme.scale(5);
                let right = r.right - state.theme.scale(5);
                let middle = i32::midpoint(left, right);
                state.on_mouse_down(hwnd, middle, y);
                state.on_mouse_move(hwnd, left, y);
                assert_eq!(state.widget_value(ControlId::VideoSpeed), Some(0.25));
                state.on_mouse_move(hwnd, right, y);
                state.on_mouse_up(hwnd, right, y);
                assert_eq!(state.widget_value(ControlId::VideoSpeed), Some(4.0));
                state.undo();
                assert_eq!(state.widget_value(ControlId::VideoSpeed), Some(1.0));
                state.on_mouse_down(hwnd, right, y);
                state.cancel_gesture(hwnd);
                assert_eq!(state.widget_value(ControlId::VideoSpeed), Some(1.0));
                state.begin_inline_input(hwnd, ControlId::VideoSpeed);
                let field = state.inline_input.as_ref().unwrap().hwnd;
                unsafe {
                    SetWindowTextW(field, w!("0.1")).unwrap();
                }
                assert!(!state.finish_inline_input(hwnd, true));
                assert_eq!(state.widget_value(ControlId::VideoSpeed), Some(1.0));
                unsafe {
                    SetWindowTextW(field, w!("1.25")).unwrap();
                }
                assert!(state.finish_inline_input(hwnd, true));
                assert_eq!(state.widget_value(ControlId::VideoSpeed), Some(1.25));
                state.activate_control(hwnd, ControlId::CanvasTab);
                state.seek_absolute(TimeTick::from_millis(1000));
                assert!(
                    !state.inspector_video_tab,
                    "playhead changes must not switch tabs"
                );
                let c = state.control_layout(client_rect(hwnd));
                let x = i32::midpoint(c.video_track.left, c.video_track.right);
                let y = i32::midpoint(c.video_track.top, c.video_track.bottom);
                state.on_mouse_down(hwnd, x, y);
                state.on_mouse_up(hwnd, x, y);
                assert!(
                    state.inspector_video_tab,
                    "explicit video selection opens video properties"
                );
                let before = state.editor.video_edit().clips[0].clone();
                let c = state.control_layout(client_rect(hwnd));
                let x = c.video_track.right - 2;
                state.on_mouse_down(hwnd, x, y);
                assert!(state.video_drag.is_some());
                state.on_mouse_move(hwnd, x - state.theme.scale(30), y);
                state.on_mouse_up(hwnd, x - state.theme.scale(30), y);
                let after = state.editor.video_edit().clips[0].clone();
                assert!(after.source_out_tick < before.source_out_tick);
                assert_eq!(
                    after.speed_percent, before.speed_percent,
                    "edge drag trims without changing speed"
                );
                state.undo();
                state.frame = state.session.prepare_current().unwrap();
                state.fallback_frame = Some(state.session.compose_prepared(&state.frame).unwrap());
                for mode in [
                    crate::platform::ui_preferences::ThemeMode::Dark,
                    crate::platform::ui_preferences::ThemeMode::Light,
                ] {
                    for (dpi, width, height) in
                        [(96, 1440, 900), (144, 1920, 1200), (192, 1600, 1200)]
                    {
                        state.theme = EditorTheme::for_mode(dpi, mode);
                        state.inspector_scroll = 0;
                        let _drawing = crate::platform::editor_ui::DrawingScale::enter(dpi);
                        crate::platform::editor_ui::render_review_png(
                            &output.join(format!("video-speed-{mode:?}-{dpi}.png")),
                            width,
                            height,
                            |dc, rect| draw_scene_region(dc, rect, rect, state),
                        );
                    }
                }
                while state.editor.can_undo() {
                    state.undo();
                }
                assert_eq!(state.editor.video_edit(), original);
                assert!(!state.editor.is_dirty());
                state.session.seek(TimeTick::from_millis(2000)).unwrap();
                state.split_video();
                state.activate_control(hwnd, ControlId::SpeedDouble);
                assert_eq!(state.widget_value(ControlId::VideoSpeed), Some(2.0));
                state.undo();
                state.undo();
                assert_eq!(state.editor.video_edit(), original);
                assert_eq!(
                    state.widget_value(ControlId::VideoSpeed),
                    Some(1.0),
                    "undoing a split must not leave video properties bound to a deleted clip"
                );
                assert!(state.control_enabled(ControlId::SpeedDouble));
                state.theme =
                    EditorTheme::for_mode(96, crate::platform::ui_preferences::ThemeMode::Dark);
                for speed in [25, 200, 400] {
                    state
                        .editor
                        .set_video_speed(&original.clips[0].id, speed)
                        .unwrap();
                    let id = format!("speed-camera-{speed}");
                    state
                        .editor
                        .add_segment(
                            id.clone(),
                            CameraSegmentKind::ZoomIn,
                            TimeTick::from_millis(8000),
                            TimeTick::from_millis(10000),
                            CameraState::focused(NormalizedPoint::new(0.7, 0.3).unwrap(), 2.0)
                                .unwrap(),
                        )
                        .unwrap();
                    state.apply_editor_state().unwrap();
                    state.timeline = TimelineViewport::fit(state.editor.project_duration());
                    state.session.seek(TimeTick::ZERO).unwrap();
                    let c = state.control_layout(client_rect(hwnd));
                    let video = state.editor.video_edit();
                    let center = video.project_time(TimeTick::from_millis(9000)).unwrap();
                    let x = timeline_x(c.camera_track, center, state.timeline);
                    let y = i32::midpoint(c.camera_track.top, c.camera_track.bottom);
                    state.on_mouse_down(hwnd, x, y);
                    assert!(matches!(
                        state.timeline_drag.as_ref().map(|d| d.mode),
                        Some(TimelineDragMode::Move)
                    ));
                    let expected_delta = video.clips[0].source_delta(
                        state
                            .timeline
                            .pixel_delta_to_tick(40, c.camera_track.right - c.camera_track.left),
                    );
                    state.on_mouse_move(hwnd, x + 40, y);
                    state.on_mouse_up(hwnd, x + 40, y);
                    let moved = state
                        .editor
                        .camera()
                        .segments
                        .iter()
                        .find(|s| s.id == id)
                        .unwrap()
                        .clone();
                    assert!(
                        (moved.start_tick.0 - TimeTick::from_millis(8000).0 - expected_delta.0)
                            .abs()
                            <= 4
                    );
                    assert_eq!(
                        moved.end_tick.0 - moved.start_tick.0,
                        TimeTick::from_millis(2000).0
                    );
                    let at = video
                        .project_time(TimeTick(moved.start_tick.0 + 500_000))
                        .unwrap();
                    state.session.seek(at).unwrap();
                    state.session.prepare_current().unwrap();
                    state.undo();
                    assert_eq!(
                        state
                            .editor
                            .camera()
                            .segments
                            .iter()
                            .find(|s| s.id == id)
                            .unwrap()
                            .start_tick,
                        TimeTick::from_millis(8000)
                    );
                    state.redo();
                    assert_eq!(
                        *state
                            .editor
                            .camera()
                            .segments
                            .iter()
                            .find(|s| s.id == id)
                            .unwrap(),
                        moved
                    );
                    state.editor.delete_segment(&id).unwrap();
                }
                while state.editor.can_undo() {
                    state.undo();
                }
                assert_eq!(state.editor.video_edit(), original);
                assert!(!state.editor.is_dirty());
            })),
        )
        .unwrap();
        assert!(report.graceful_close);
        assert_eq!(report.media_errors, 0);
    }

    #[test]
    #[ignore = "explicit local fixture and UI review output directory required"]
    #[allow(clippy::too_many_lines)] // Review fixtures also check the actual cursor policy without OS input.
    fn render_ui_review() {
        let output = std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
        let project =
            std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
        let descriptor = ProjectPreviewSession::native_output_descriptor(&project).unwrap();
        let mut state = WindowState::open(
            &project,
            descriptor,
            None,
            SmokeAction::None,
            Arc::new(WindowTelemetry::default()),
        )
        .unwrap();
        state.project_name = "产品演示".into();
        // Relocated property entry points preserve validation, visibility and undo.
        let original_cursor = state.editor.settings().cursor;
        state
            .apply_property_values(ControlId::CursorProperties, &["2.0".into()])
            .unwrap();
        assert_eq!(state.editor.settings().cursor.scale, 2.0);
        assert_eq!(
            state.editor.settings().cursor.visible,
            original_cursor.visible
        );
        assert!(
            state
                .apply_property_values(ControlId::CursorProperties, &["0".into()])
                .is_err()
        );
        assert_eq!(state.editor.settings().cursor.scale, 2.0);
        assert!(state.editor.undo());
        assert_eq!(state.editor.settings().cursor.scale, original_cursor.scale);
        let original_color = state.editor.settings().background.color.clone();
        state
            .apply_property_values(ControlId::BackgroundColor, &["#abcdef".into()])
            .unwrap();
        assert_eq!(state.editor.settings().background.color, "#abcdef");
        assert!(
            state
                .apply_property_values(ControlId::BackgroundColor, &["invalid".into()])
                .is_err()
        );
        assert_eq!(state.editor.settings().background.color, "#abcdef");
        assert!(state.editor.undo());
        assert_eq!(state.editor.settings().background.color, original_color);
        state.add_manual_segment();
        state.selected_video = None;
        state.fallback_frame = Some(state.session.compose_prepared(&state.frame).unwrap());
        for (dpi, width, height, camera, scroll) in [
            (96, 1440, 900, false, 0),
            (96, 1024, 680, true, 0),
            (144, 1280, 900, true, 2000),
            (192, 1280, 900, false, 2000),
        ] {
            state.theme = EditorTheme::for_dpi(dpi);
            state.inspector_scroll = scroll;
            state.selected_segment = if camera {
                state.editor.camera().segments.first().map(|s| s.id.clone())
            } else {
                None
            };
            let _drawing = crate::platform::editor_ui::DrawingScale::enter(dpi);
            crate::platform::editor_ui::render_review_png(
                &output.join(format!("editor-{dpi}-{width}.png")),
                width,
                height,
                |dc, rect| draw_scene_region(dc, rect, rect, &state),
            );
        }
        state.theme = EditorTheme::for_dpi(96);
        state.inspector_scroll = 0;
        let client = RECT {
            left: 0,
            top: 0,
            right: 1440,
            bottom: 900,
        };
        let c = state.control_layout(client);
        let mid = |r: HitRect| {
            (
                i32::midpoint(r.left, r.right),
                i32::midpoint(r.top, r.bottom),
            )
        };
        for r in [c.zoom_slider, c.ruler] {
            let (x, y) = mid(r);
            assert_eq!(state.cursor_at(client, x, y), IDC_HAND);
        }
        assert_eq!(
            state.cursor_at(client, c.video_track.left + 1, c.video_track.top + 10),
            IDC_HAND
        );
        assert_eq!(
            state.cursor_at(client, c.video_track.right - 2, c.video_track.top + 10),
            IDC_SIZEWE
        );
        assert_eq!(
            state.cursor_at(client, c.panel.left - 1, c.panel.top + 10),
            IDC_SIZEWE
        );
        state.timeline.zoom_at(TimeTick::ZERO, 2.0);
        let (x, y) = mid(c.scrollbar);
        assert_eq!(state.cursor_at(client, x, y), IDC_HAND);
        state.property_drag = Some(ControlId::ZoomSlider);
        assert_eq!(
            state.cursor_at(client, c.panel.left, c.panel.top + 10),
            IDC_HAND
        );
        state.property_drag = None;
        state.timeline = TimelineViewport::fit(state.editor.project_duration());
        state.editor.set_canvas_inset(0.08).unwrap();
        state.editor.set_corner_radius(0.1).unwrap();
        state
            .session
            .apply_edit_state(
                state.editor.camera().clone(),
                state.editor.settings().clone(),
            )
            .unwrap();
        state.frame = state.session.prepare_current().unwrap();
        state.fallback_frame = Some(state.session.compose_prepared(&state.frame).unwrap());
        state.selected_segment = state.editor.camera().segments.first().map(|s| s.id.clone());
        let selected = state.editor.camera().segments[0].clone();
        let review_span = selected.end_tick.0 - selected.start_tick.0;
        state
            .editor
            .update_segment(
                &selected.id,
                TimeTick(selected.start_tick.0 + review_span / 4),
                TimeTick(selected.end_tick.0 - review_span / 4),
                selected.to,
            )
            .unwrap();
        for width in [264, 360, 480] {
            state.theme.metrics.inspector_width = width;
            state.theme.metrics.timeline_height = 440;
            let _drawing = crate::platform::editor_ui::DrawingScale::enter(96);
            crate::platform::editor_ui::render_review_png(
                &output.join(format!("editor-expanded-inspector-{width}.png")),
                1600,
                1000,
                |dc, rect| draw_scene_region(dc, rect, rect, &state),
            );
        }
        state.hot_control = Some(ControlId::Add);
        state.tooltip_visible = true;
        crate::platform::editor_ui::render_review_png(
            &output.join("editor-toolbar-hover.png"),
            1600,
            1000,
            |dc, rect| draw_scene_region(dc, rect, rect, &state),
        );
        state.hot_control = None;
        state.tooltip_visible = false;
        for mode in [
            crate::platform::ui_preferences::ThemeMode::Light,
            crate::platform::ui_preferences::ThemeMode::Dark,
        ] {
            for (dpi, width, height) in [(96, 1440, 900), (144, 1280, 900), (192, 1280, 900)] {
                state.theme = EditorTheme::for_mode(dpi, mode);
                state.inspector_scroll = 0;
                let _drawing = crate::platform::editor_ui::DrawingScale::enter(dpi);
                crate::platform::editor_ui::render_review_png(
                    &output.join(format!("editor-{mode:?}-{dpi}.png")),
                    width,
                    height,
                    |dc, rect| draw_scene_region(dc, rect, rect, &state),
                );
            }
        }
        state.theme = EditorTheme::for_mode(96, crate::platform::ui_preferences::ThemeMode::Dark);
        state.selected_segment = None;
        state.selected_video = state
            .editor
            .video_edit()
            .spans()
            .next()
            .map(|span| span.clip.id.clone());
        state.inspector_image_tab = true;
        crate::platform::editor_ui::render_review_png(
            &output.join("editor-dark-video-image.png"),
            1440,
            900,
            |dc, rect| draw_scene_region(dc, rect, rect, &state),
        );
    }

    #[test]
    #[ignore = "uses an explicit disposable project copy and UI review output directory"]
    #[allow(clippy::too_many_lines)]
    fn background_and_low_resolution_filmstrip_ui_review() {
        use crate::platform::{
            media_decoder::MfBgraDecoder, timeline_thumbnails::TimelineThumbnails,
        };
        let output = std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
        let project =
            std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
        let descriptor = ProjectPreviewSession::native_output_descriptor(&project).unwrap();
        let mut state = WindowState::open(
            &project,
            descriptor,
            None,
            SmokeAction::None,
            Arc::new(WindowTelemetry::default()),
        )
        .unwrap();
        state.theme = EditorTheme::for_mode(96, crate::platform::ui_preferences::ThemeMode::Dark);
        state.theme.metrics.inspector_width = 416;
        state.theme.metrics.timeline_height = 2000;
        state.inspector_image_tab = true;
        state.editor.set_canvas_inset(0.09).unwrap();
        let first = output.join("background-fixture.png");
        let second = output.join("background-replacement.png");
        image::RgbaImage::from_fn(1000, 600, |x, y| {
            image::Rgba([
                (40 + x * 80 / 1000) as u8,
                (60 + y * 110 / 600) as u8,
                170,
                255,
            ])
        })
        .save(&first)
        .unwrap();
        image::RgbaImage::from_pixel(240, 600, image::Rgba([180, 80, 40, 128]))
            .save(&second)
            .unwrap();
        state.editor.import_background_image(&first).unwrap();
        state
            .session
            .apply_edit_state(
                state.editor.camera().clone(),
                state.editor.settings().clone(),
            )
            .unwrap();
        let original = Arc::clone(state.session.background_preview().unwrap());
        state.frame = state.session.prepare_current().unwrap();
        state.fallback_frame = Some(state.session.compose_prepared(&state.frame).unwrap());
        let client = RECT {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1417,
        };
        let track = state.control_layout(client).video_track;
        let visible: Vec<_> = state
            .visible_thumbnails(track)
            .iter()
            .map(|(_, tick)| *tick)
            .collect();
        let start = Instant::now();
        loop {
            state
                .thumbnails
                .poll(state.scrub_preview.ready_proxy(), &visible, true);
            if visible
                .iter()
                .all(|tick| state.thumbnails.get(*tick).is_some())
            {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_mins(2),
                "filmstrip did not finish loading"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        for tick in &visible {
            let frame = state.thumbnails.get(*tick).unwrap();
            assert_eq!(frame.width.max(frame.height), 160);
        }
        for (mode, name) in [
            (crate::platform::ui_preferences::ThemeMode::Dark, "dark"),
            (crate::platform::ui_preferences::ThemeMode::Light, "light"),
        ] {
            state.theme.set_mode(mode);
            crate::platform::editor_ui::render_review_png(
                &output.join(format!("background-filmstrip-{name}.png")),
                client.right,
                client.bottom,
                |dc, rect| draw_scene_region(dc, rect, rect, &state),
            );
        }
        state.editor.import_background_image(&second).unwrap();
        state
            .session
            .apply_edit_state(
                state.editor.camera().clone(),
                state.editor.settings().clone(),
            )
            .unwrap();
        let replacement = Arc::clone(state.session.background_preview().unwrap());
        assert!(!Arc::ptr_eq(&original, &replacement));
        assert_eq!(replacement.get_pixel(0, 0).0, [40, 80, 180, 128]);
        state
            .theme
            .set_mode(crate::platform::ui_preferences::ThemeMode::Dark);
        state.frame = state.session.prepare_current().unwrap();
        state.fallback_frame = Some(state.session.compose_prepared(&state.frame).unwrap());
        crate::platform::editor_ui::render_review_png(
            &output.join("background-replaced-portrait-alpha.png"),
            client.right,
            client.bottom,
            |dc, rect| draw_scene_region(dc, rect, rect, &state),
        );
        assert!(state.editor.undo());
        state
            .session
            .apply_edit_state(
                state.editor.camera().clone(),
                state.editor.settings().clone(),
            )
            .unwrap();
        assert_eq!(
            state.session.background_preview().unwrap().as_ref(),
            original.as_ref()
        );
        assert!(state.editor.redo());
        state
            .session
            .apply_edit_state(
                state.editor.camera().clone(),
                state.editor.settings().clone(),
            )
            .unwrap();
        assert_eq!(
            state.session.background_preview().unwrap().as_ref(),
            replacement.as_ref()
        );

        // The original low-resolution proxy path remains fixed at 160 pixels across DPI changes.
        let proxy = state.scrub_preview.ready_proxy().unwrap();
        let mut decoder = MfBgraDecoder::open_with_preview_limit(&proxy.path, Some(160)).unwrap();
        let reference = decoder.read_next().unwrap().unwrap();
        let mut thumbnails = TimelineThumbnails::default();
        let ticks = [TimeTick::ZERO];
        thumbnails.poll(None, &ticks, true);
        assert!(thumbnails.get(TimeTick::ZERO).is_none());
        let started = Instant::now();
        loop {
            thumbnails.poll(Some(Arc::clone(&proxy)), &ticks, true);
            if let Some(frame) = thumbnails.get(TimeTick::ZERO) {
                assert_eq!(
                    (frame.width, frame.height),
                    (reference.width, reference.height)
                );
                assert_eq!(frame.width.max(frame.height), 160);
                assert_eq!(frame.pixels, reference.pixels);
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(20));
            std::thread::sleep(Duration::from_millis(5));
        }
        for dpi in [96, 144, 192] {
            state.theme = EditorTheme::for_dpi(dpi);
            thumbnails.poll(Some(Arc::clone(&proxy)), &ticks, true);
            let frame = thumbnails.get(TimeTick::ZERO).unwrap();
            assert_eq!(frame.width.max(frame.height), 160);
        }
        std::fs::write(output.join("low-resolution-filmstrip-review.txt"), format!(
            "Thumbnail: {}x{}\nVisible filmstrip frames: {}\nIndependent 160 px proxy decode: identical pixels\nDPI 96/144/192: thumbnail resolution remains 160 px\nBackground: import, replacement, alpha, undo/redo, light/dark rendered\n",
            reference.width, reference.height, visible.len())).unwrap();
    }

    #[test]
    fn narrow_clips_choose_the_nearest_trim_edge_at_every_dpi() {
        for dpi in [96, 120, 144, 192] {
            let theme = EditorTheme::for_dpi(dpi);
            let left = theme.scale(100);
            let right = theme.scale(110);
            assert_eq!(
                timeline_edge_mode(left + 1, left, right, theme.scale(8)),
                TimelineDragMode::TrimStart
            );
            assert_eq!(
                timeline_edge_mode(right - 1, left, right, theme.scale(8)),
                TimelineDragMode::TrimEnd
            );
        }
    }

    #[test]
    fn long_scrub_probe_repeats_full_gestures_instead_of_slowing_the_pointer() {
        for cycle in 0..30 {
            let start = f64::from(cycle) * 10.0;
            assert_eq!(scrub_probe_progress(start, 300.0), 0.0);
            assert_eq!(scrub_probe_progress(start + 2.5, 300.0), 0.5);
            assert_eq!(scrub_probe_progress(start + 5.0, 300.0), 1.0);
            assert_eq!(scrub_probe_progress(start + 7.5, 300.0), 0.5);
        }
        assert_eq!(scrub_probe_progress(2.0, 4.0), 1.0);
    }

    #[test]
    fn paused_timer_does_not_request_a_frame_render() {
        assert!(!timer_requires_render(PlaybackState::Paused));
        assert!(timer_requires_render(PlaybackState::Playing));
    }

    #[test]
    fn deferred_playback_can_be_requested_cancelled_and_consumed() {
        let mut intent = DeferredPlayback::None;
        intent.toggle();
        assert!(intent.is_requested());
        intent.toggle();
        assert!(!intent.is_requested());
        intent.request();
        assert!(intent.take_requested());
        assert!(!intent.is_requested());
        assert!(!intent.take_requested());
    }

    #[test]
    fn scrub_requests_coalesce_to_the_latest_pointer_position() {
        let mut scrub = TimelineScrub {
            original_tick: TimeTick(500),
            current_x: 100,
            desired_tick: TimeTick(1_000),
            submitted_tick: None,
        };
        assert_eq!(scrub.take_latest_preview(), Some(TimeTick(1_000)));
        assert!(!scrub.needs_preview());

        scrub.update(140, TimeTick(1_400));
        scrub.update(180, TimeTick(1_800));
        scrub.update(220, TimeTick(2_200));
        assert_eq!(scrub.take_latest_preview(), Some(TimeTick(2_200)));
        assert_eq!(scrub.current_x, 220);
        assert_eq!(scrub.take_latest_preview(), None);
        assert_eq!(scrub.original_tick, TimeTick(500));
    }

    #[test]
    fn macos_workbench_regions_do_not_overlap_at_default_size() {
        let layout = ControlLayout::new(
            RECT {
                left: 0,
                top: 0,
                right: 1_440,
                bottom: 900,
            },
            EditorTheme::light(),
        );
        assert!(layout.command_bar.bottom <= layout.preview_well.top);
        assert!(layout.preview_well.right <= layout.panel.left);
        assert!(layout.panel.bottom <= layout.timeline_panel.top);
        assert!(layout.timeline_panel.left <= layout.camera_track.left);
        assert!(layout.timeline_panel.right >= layout.camera_track.right);
        assert!(layout.camera_track.bottom < layout.status.top);
        assert_eq!(layout.start_value.right, layout.start_value.left);
        assert!(layout.start_value.bottom <= layout.status.top);
        assert!(layout.end_plus.right <= layout.timeline_panel.right);
        assert!(layout.background.left >= layout.panel.left);
        assert!(!rects_intersect(layout.ruler.0, layout.time));
        assert!(layout.status.right <= layout.timeline_panel.right);
        assert!(layout.background.right <= layout.background_image.left);
        assert!(layout.background_image.bottom <= layout.inset.top);
        assert!(layout.inset.bottom <= layout.radius.top);
        assert!(layout.radius.bottom <= layout.shadow.top);
    }

    #[test]
    fn editor_layout_stays_separated_across_supported_dpi_and_window_sizes() {
        for dpi in [96, 120, 144, 192] {
            let theme = EditorTheme::for_dpi(dpi);
            // 1024x720 approximates the client area of a 1040x760 outer window at 96 DPI.
            for (width, height) in [(1_024, 720), (1_040, 760), (1_440, 900), (2_560, 1_440)] {
                let layout = ControlLayout::new(
                    RECT {
                        left: 0,
                        top: 0,
                        right: theme.scale(width),
                        bottom: theme.scale(height),
                    },
                    theme,
                );
                assert_eq!(layout.preview_card.left, theme.metrics.outer_margin);
                assert_eq!(layout.timeline_panel.left, layout.preview_card.left);
                assert_eq!(layout.timeline_panel.right, layout.panel.right);
                assert_eq!(layout.preview_card.bottom, layout.panel.bottom);
                assert_eq!(
                    layout.panel.left - layout.preview_card.right,
                    theme.metrics.gap
                );
                assert_eq!(
                    layout.timeline_panel.top - layout.preview_card.bottom,
                    theme.metrics.gap
                );
                assert_eq!(layout.inspector_splitter.left, layout.preview_card.right);
                assert_eq!(layout.inspector_splitter.right, layout.panel.left);
                assert_eq!(layout.splitter.top, layout.preview_card.bottom);
                assert_eq!(layout.splitter.bottom, layout.timeline_panel.top);
                assert!(!rects_intersect(layout.ruler.0, layout.time));
                assert!(layout.status.right <= layout.timeline_panel.right);
                assert!(layout.reset.right < layout.time.left);
                assert!(layout.time.right < layout.timeline_zoom_out.left);
                assert!(layout.end_plus.right <= layout.timeline_panel.right);
                assert_eq!(layout.start_value.right, layout.start_value.left);
                assert!(layout.start_value.bottom <= layout.status.top);
                assert!(layout.status.bottom <= layout.timeline_panel.bottom);
                let scrolled = layout.with_inspector(false, false, i32::MAX, theme);
                assert!(scrolled.control_rect(ControlId::Shadow).bottom <= layout.panel.bottom);
            }
        }
    }

    #[test]
    fn every_visual_button_maps_back_to_its_control_id() {
        let layout = ControlLayout::new(
            RECT {
                left: 0,
                top: 0,
                right: 1_440,
                bottom: 900,
            },
            EditorTheme::light(),
        );
        for id in ControlId::ORDER {
            let rect = layout.control_rect(id);
            if rect.right <= rect.left || rect.bottom <= rect.top {
                continue;
            }
            assert_eq!(
                layout.hit_control(
                    i32::midpoint(rect.left, rect.right),
                    i32::midpoint(rect.top, rect.bottom),
                ),
                Some(id)
            );
        }
    }

    #[test]
    fn every_editor_control_has_a_unique_accessible_name_and_tooltip() {
        let mut names = std::collections::HashSet::new();
        for id in ControlId::ORDER {
            assert!(!id.accessibility_name().trim().is_empty());
            assert!(!id.tooltip().trim().is_empty());
            assert!(names.insert(id.accessibility_name()));
        }
    }
}
