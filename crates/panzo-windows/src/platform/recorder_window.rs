use crate::platform::capture_probe::primary_monitor;
#[cfg(test)]
#[path = "unified_window_review.rs"]
pub(crate) mod unified_window_review;
use crate::platform::editor_ui::{
    ButtonRole, ButtonVisualState, EditorTheme, IconKind, TextStyle, apply_window_chrome,
    button_colors, button_interaction, complete_pointer_activation, draw_icon,
    enable_per_monitor_dpi_awareness, focus_ring, next_keyboard_focus, rounded_surface,
    select_font, system_dpi, window_dpi,
};
use crate::platform::export::{ExportReport, ExportRequest, ProjectExporter};
use crate::platform::preview_window;
use crate::platform::recorder::{
    CaptureRecorder, RecordingControl, RecordingReport, RecordingSource,
};
use crate::platform::window_capture::enumerate_capturable_windows;
use panzo_core::{ProjectLayout, ProjectState};
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC,
    CreateSolidBrush, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC,
    DeleteObject, DrawTextW, EndPaint, FillRect, HGDIOBJ, InvalidateRect, PAINTSTRUCT, SRCCOPY,
    SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VK_ESCAPE, VK_RETURN, VK_SHIFT, VK_SPACE, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW,
    DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetClientRect,
    GetMessageW, GetWindowLongPtrW, IDC_ARROW, KillTimer, LoadCursorW, MF_SEPARATOR, MF_STRING,
    MINMAXINFO, MSG, PostQuitMessage, RegisterClassW, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER,
    SetCursor, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
    ShowWindow, TPM_LEFTALIGN, TPM_RETURNCMD, TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE,
    WM_CANCELMODE, WM_CAPTURECHANGED, WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND,
    WM_GETMINMAXINFO, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_NCDESTROY, WM_PAINT, WM_SETFOCUS, WM_SIZE, WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    WS_VISIBLE,
};
use windows::core::{Error as WindowsError, PCWSTR, w};

const TIMER_ID: usize = 1;
const TIMER_INTERVAL_MS: u32 = 16;
pub(crate) const WM_NAVIGATE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 30;
const WM_QUIT_WINDOW: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 31;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NavigationIntent {
    OpenProject(PathBuf),
    NewRecording,
    Quit,
}
const MAXIMUM_RECORDING_DURATION: Duration = Duration::from_hours(24);
const TOOLTIP_DELAY: Duration = Duration::from_millis(550);
const MONITOR_SOURCE_COMMAND: usize = 1;
const FIRST_WINDOW_SOURCE_COMMAND: usize = 1_000;

#[derive(Debug, Clone, Copy)]
enum Automation {
    None,
    RecordAndExport(Duration),
    OpenLast(Duration),
}

pub struct RecorderWindow;

impl RecorderWindow {
    pub fn run(project_library: impl AsRef<Path>) -> Result<(), RecorderWindowError> {
        Self::run_internal(project_library.as_ref(), Automation::None, None, None).map(|_| ())
    }

    pub fn smoke(
        project_library: impl AsRef<Path>,
        recording_duration: Duration,
    ) -> Result<RecorderWindowSmokeReport, RecorderWindowError> {
        if recording_duration < Duration::from_millis(500) {
            return Err(RecorderWindowError::InvalidSmokeDuration);
        }
        Self::run_internal(
            project_library.as_ref(),
            Automation::RecordAndExport(recording_duration),
            None,
            None,
        )
    }

    pub fn smoke_open_last(
        project_library: impl AsRef<Path>,
        visible_duration: Duration,
    ) -> Result<RecorderWindowSmokeReport, RecorderWindowError> {
        if visible_duration < Duration::from_millis(250) {
            return Err(RecorderWindowError::InvalidOpenLastSmokeDuration);
        }
        Self::run_internal(
            project_library.as_ref(),
            Automation::OpenLast(visible_duration),
            None,
            None,
        )
    }

    pub fn run_project(project_library: &Path, project: &Path) -> Result<(), RecorderWindowError> {
        Self::run_internal_with_project(
            project_library,
            Automation::None,
            None,
            None,
            Some(project.to_path_buf()),
        )
        .map(|_| ())
    }

    pub(crate) fn run_editor(
        library: &Path,
        editor: Box<preview_window::WindowState>,
        ready: Option<Box<dyn FnOnce(usize)>>,
    ) -> Result<(), RecorderWindowError> {
        Self::run_internal(library, Automation::None, Some(editor), ready).map(|_| ())
    }

    #[allow(clippy::too_many_lines)]
    fn run_internal(
        project_library: &Path,
        automation: Automation,
        initial_editor: Option<Box<preview_window::WindowState>>,
        ready: Option<Box<dyn FnOnce(usize)>>,
    ) -> Result<RecorderWindowSmokeReport, RecorderWindowError> {
        Self::run_internal_with_project(project_library, automation, initial_editor, ready, None)
    }

    #[allow(clippy::too_many_lines)] // Owns the one native host and its one message loop.
    fn run_internal_with_project(
        project_library: &Path,
        automation: Automation,
        initial_editor: Option<Box<preview_window::WindowState>>,
        ready: Option<Box<dyn FnOnce(usize)>>,
        initial_project: Option<PathBuf>,
    ) -> Result<RecorderWindowSmokeReport, RecorderWindowError> {
        enable_per_monitor_dpi_awareness();
        let initial_theme = EditorTheme::current(system_dpi());
        fs::create_dir_all(project_library).map_err(|source| {
            RecorderWindowError::CreateLibrary {
                path: project_library.into(),
                source,
            }
        })?;
        let monitor = primary_monitor().map_err(RecorderWindowError::Windows)?;
        let monitor_width = u32::try_from(monitor.rect[2] - monitor.rect[0])
            .map_err(|_| RecorderWindowError::InvalidMonitorGeometry)?;
        let monitor_height = u32::try_from(monitor.rect[3] - monitor.rect[1])
            .map_err(|_| RecorderWindowError::InvalidMonitorGeometry)?;
        if monitor_width == 0 || monitor_height == 0 {
            return Err(RecorderWindowError::InvalidMonitorGeometry);
        }

        let telemetry = Arc::new(WindowTelemetry::default());
        let mut state = Box::new(WindowState::new(
            project_library.into(),
            monitor_width,
            monitor_height,
            automation,
            Arc::clone(&telemetry),
        ));
        state.theme = initial_theme;
        state.editor = initial_editor;
        state.requested_project = initial_project;
        let module = win("GetModuleHandleW", unsafe { GetModuleHandleW(None) })?;
        let instance = HINSTANCE(module.0);
        let cursor = win("LoadCursorW", unsafe { LoadCursorW(None, IDC_ARROW) })?;
        let class_name = w!("PanzoRecorderWindow");
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: cursor,
            lpszClassName: class_name,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&raw const window_class) } == 0
            && unsafe { windows::Win32::Foundation::GetLastError() }
                != windows::Win32::Foundation::ERROR_CLASS_ALREADY_EXISTS
        {
            return Err(RecorderWindowError::WindowsStage {
                stage: "RegisterClassW",
                source: WindowsError::from_thread(),
            });
        }

        let (initial_width, initial_height) = crate::platform::editor_ui::available_window_size(
            initial_theme.scale(1440),
            initial_theme.scale(900),
        );
        let hwnd = win("CreateWindowExW", unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class_name,
                w!("Panzo"),
                WS_OVERLAPPEDWINDOW
                    | WS_VISIBLE
                    | windows::Win32::UI::WindowsAndMessaging::WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                initial_width,
                initial_height,
                None,
                None,
                Some(instance),
                None,
            )
        })?;
        state.theme = EditorTheme::current(window_dpi(hwnd));
        let state_pointer = Box::into_raw(state);
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_pointer as isize);
        }
        apply_window_chrome(hwnd);
        preview_window::editor_titlebar::initialize(hwnd);
        let host = unsafe { &mut *state_pointer };
        host.hwnd = Some(hwnd);
        if let Some(editor) = host.editor.as_mut() {
            let result = editor
                .attach(hwnd)
                .map_err(|e| e.to_string())
                .and_then(|()| editor.publish(hwnd).map_err(|e| e.to_string()));
            if let Err(error) = result {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                return Err(RecorderWindowError::Editor(error));
            }
            host.last_project = Some(editor.project_root().to_path_buf());
        } else {
            host.refresh(hwnd);
        }
        telemetry.window_created.store(true, Ordering::Relaxed);
        if unsafe { SetTimer(Some(hwnd), TIMER_ID, TIMER_INTERVAL_MS, None) } == 0 {
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(state_pointer));
                let _ = DestroyWindow(hwnd);
            }
            return Err(RecorderWindowError::WindowsStage {
                stage: "SetTimer",
                source: WindowsError::from_thread(),
            });
        }
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = windows::Win32::Graphics::Gdi::UpdateWindow(hwnd);
        }

        if let Some(ready) = ready {
            ready(hwnd.0 as usize);
        }
        if unsafe { &*state_pointer }.requested_project.is_some() {
            unsafe { &*state_pointer }.post_navigation();
        }

        let mut message = MSG::default();
        loop {
            let status = unsafe { GetMessageW(&raw mut message, None, 0, 0) }.0;
            if status == -1 {
                return Err(RecorderWindowError::WindowsStage {
                    stage: "GetMessageW",
                    source: WindowsError::from_thread(),
                });
            }
            if status == 0 {
                break;
            }
            let panel = unsafe { &*state_pointer }
                .editor
                .as_ref()
                .and_then(|editor| editor.panel_hwnd());
            if panel.is_some_and(|panel| unsafe {
                windows::Win32::UI::WindowsAndMessaging::IsDialogMessageW(panel, &raw const message)
                    .as_bool()
            }) {
                continue;
            }
            unsafe {
                let _ = TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }

        let recording = telemetry
            .recording
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let export = telemetry
            .export
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let error = telemetry
            .error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let last_project_loaded = telemetry.last_project_loaded.load(Ordering::Relaxed);
        let loaded_project = last_project_loaded
            .then(|| find_last_ready_project(project_library))
            .flatten();
        Ok(RecorderWindowSmokeReport {
            window_created: telemetry.window_created.load(Ordering::Relaxed),
            recording_started: telemetry.recording_started.load(Ordering::Relaxed),
            stop_requested: telemetry.stop_requested.load(Ordering::Relaxed),
            last_project_loaded,
            project_ready: recording.is_some() || last_project_loaded,
            export_completed: export.is_some(),
            graceful_close: telemetry.graceful_close.load(Ordering::Relaxed),
            monitor_width,
            monitor_height,
            loaded_project,
            recording,
            export,
            error,
        })
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct RecorderWindowSmokeReport {
    pub window_created: bool,
    pub recording_started: bool,
    pub stop_requested: bool,
    pub last_project_loaded: bool,
    pub project_ready: bool,
    pub export_completed: bool,
    pub graceful_close: bool,
    pub monitor_width: u32,
    pub monitor_height: u32,
    pub loaded_project: Option<PathBuf>,
    pub recording: Option<RecordingReport>,
    pub export: Option<ExportReport>,
    pub error: Option<String>,
}

impl std::fmt::Display for RecorderWindowSmokeReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(formatter, "Panzo V0.1 Recorder Window smoke")?;
        writeln!(formatter, "  Window created: {}", self.window_created)?;
        writeln!(
            formatter,
            "  Selected monitor: {}x{} @ 60 FPS",
            self.monitor_width, self.monitor_height
        )?;
        writeln!(formatter, "  Recording started: {}", self.recording_started)?;
        writeln!(formatter, "  Stop requested: {}", self.stop_requested)?;
        writeln!(
            formatter,
            "  Last project loaded: {}",
            self.last_project_loaded
        )?;
        writeln!(formatter, "  Project ready: {}", self.project_ready)?;
        writeln!(formatter, "  Export completed: {}", self.export_completed)?;
        if let Some(project) = &self.loaded_project {
            writeln!(formatter, "  Loaded project: {}", project.display())?;
        }
        if let Some(recording) = &self.recording {
            writeln!(formatter, "  Project: {}", recording.project_root.display())?;
            writeln!(formatter, "  Duration tick: {}", recording.duration_tick)?;
            writeln!(formatter, "  Frames encoded: {}", recording.frames_encoded)?;
            writeln!(
                formatter,
                "  Backpressure drops: {}",
                recording.dropped_by_backpressure
            )?;
            writeln!(
                formatter,
                "  Rate-limited frames: {}",
                recording.dropped_by_frame_rate_limit
            )?;
        }
        if let Some(export) = &self.export {
            writeln!(
                formatter,
                "  Export output: {}",
                export.output_path.display()
            )?;
            writeln!(formatter, "  Export frames: {}", export.frames_written)?;
            writeln!(formatter, "  Export bytes: {}", export.output_bytes)?;
            writeln!(formatter, "  Export fragments: {}", export.fragment_count)?;
        }
        writeln!(
            formatter,
            "  Error: {}",
            self.error.as_deref().unwrap_or("none")
        )?;
        write!(formatter, "  Graceful close: {}", self.graceful_close)
    }
}

#[derive(Default)]
struct WindowTelemetry {
    window_created: AtomicBool,
    recording_started: AtomicBool,
    stop_requested: AtomicBool,
    last_project_loaded: AtomicBool,
    graceful_close: AtomicBool,
    recording: Mutex<Option<RecordingReport>>,
    export: Mutex<Option<ExportReport>>,
    error: Mutex<Option<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecorderPhase {
    Ready,
    Starting,
    Recording,
    Finalizing,
    Exporting,
    Complete,
    Failed,
}

impl RecorderPhase {
    const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Recording | Self::Finalizing | Self::Exporting
        )
    }
}

enum BackgroundResult {
    Recording(Result<RecordingReport, String>),
    Export(Result<ExportReport, String>),
}

#[derive(Default)]
struct RecorderPointerState {
    hot: Option<RecorderControlId>,
    pressed: Option<RecorderControlId>,
    focused: Option<RecorderControlId>,
    keyboard_focus: bool,
    tooltip_deadline: Option<Instant>,
    tooltip_visible: bool,
    tracking_leave: bool,
}

struct ContentTransition {
    intent: NavigationIntent,
    load: Option<crate::platform::editor_tasks::LoadTask>,
    candidate: Option<Box<preview_window::WindowState>>,
}

#[allow(clippy::struct_excessive_bools)]
struct WindowState {
    transition: Option<ContentTransition>,
    accessibility: crate::platform::accessibility::AccessibilityHost,
    library_root: PathBuf,
    primary_monitor_width: u32,
    primary_monitor_height: u32,
    capture_width: u32,
    capture_height: u32,
    capture_source: RecordingSource,
    phase: RecorderPhase,
    status: String,
    status_severity: crate::platform::editor_ui::StatusSeverity,
    started_at: Option<Instant>,
    worker: Option<Receiver<BackgroundResult>>,
    editor: Option<Box<preview_window::WindowState>>,
    hwnd: Option<HWND>,
    requested_project: Option<PathBuf>,
    control: Option<RecordingControl>,
    last_project: Option<PathBuf>,
    last_report: Option<RecordingReport>,
    last_export: Option<ExportReport>,
    auto_start: bool,
    auto_stop_after: Option<Duration>,
    auto_export: bool,
    open_last_close_at: Option<Instant>,
    auto_deadline: Option<Instant>,
    close_when_finished: bool,
    telemetry: Arc<WindowTelemetry>,
    pointer: RecorderPointerState,
    theme: EditorTheme,
}

impl WindowState {
    fn set_status(&mut self, severity: crate::platform::editor_ui::StatusSeverity, text: String) {
        self.status_severity = severity;
        self.status = text;
    }

    fn new(
        library_root: PathBuf,
        monitor_width: u32,
        monitor_height: u32,
        automation: Automation,
        telemetry: Arc<WindowTelemetry>,
    ) -> Self {
        let last_project = find_last_ready_project(&library_root);
        if last_project.is_some() {
            telemetry.last_project_loaded.store(true, Ordering::Relaxed);
        }
        let (auto_start, auto_stop_after, auto_export, open_last_close_at) = match automation {
            Automation::None => (false, None, false, None),
            Automation::RecordAndExport(duration) => (true, Some(duration), true, None),
            Automation::OpenLast(duration) => (false, None, false, Some(Instant::now() + duration)),
        };
        let phase = if last_project.is_some() {
            RecorderPhase::Complete
        } else {
            RecorderPhase::Ready
        };
        let status = last_project.as_ref().map_or_else(
            || "可以开始录制主显示器".into(),
            |project| format!("上一个项目已就绪：{}", project.display()),
        );
        Self {
            library_root,
            transition: None,
            accessibility: crate::platform::accessibility::AccessibilityHost::default(),
            primary_monitor_width: monitor_width,
            primary_monitor_height: monitor_height,
            capture_width: monitor_width,
            capture_height: monitor_height,
            capture_source: RecordingSource::PrimaryMonitor,
            phase,
            status,
            status_severity: crate::platform::editor_ui::StatusSeverity::Information,
            started_at: None,
            worker: None,
            editor: None,
            hwnd: None,
            requested_project: None,
            control: None,
            last_project,
            last_report: None,
            last_export: None,
            auto_start,
            auto_stop_after,
            auto_export,
            open_last_close_at,
            auto_deadline: None,
            close_when_finished: false,
            telemetry,
            pointer: RecorderPointerState::default(),
            theme: EditorTheme::light(),
        }
    }

    fn start_recording(&mut self) {
        if self.editor.is_some() {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "请先保存并关闭当前编辑器，再开始新录制".into(),
            );
            return;
        }
        if self.phase.is_active() {
            return;
        }
        let project_root = match next_project_path(&self.library_root) {
            Ok(path) => path,
            Err(error) => {
                self.fail(error.to_string());
                return;
            }
        };
        let control = RecordingControl::default();
        let worker_control = control.clone();
        let worker_project = project_root.clone();
        let worker_source = self.capture_source.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = CaptureRecorder::record_with_control(
                &worker_project,
                MAXIMUM_RECORDING_DURATION,
                &worker_control,
                &worker_source,
            )
            .map_err(|error| error.to_string());
            let _ = sender.send(BackgroundResult::Recording(result));
        });
        let now = Instant::now();
        self.phase = RecorderPhase::Starting;
        self.set_status(
            crate::platform::editor_ui::StatusSeverity::Information,
            format!(
                "正在启动 {}×{} 原生分辨率录制…",
                self.capture_width, self.capture_height
            ),
        );
        self.started_at = None;
        self.auto_deadline = self
            .auto_stop_after
            .map(|duration| now + duration + Duration::from_secs(30));
        self.worker = Some(receiver);
        self.control = Some(control);
        self.last_project = Some(project_root);
    }

    fn request_stop(&mut self) {
        if !matches!(
            self.phase,
            RecorderPhase::Starting | RecorderPhase::Recording
        ) {
            return;
        }
        if let Some(control) = &self.control {
            control.request_stop();
        }
        self.phase = RecorderPhase::Finalizing;
        self.set_status(
            crate::platform::editor_ui::StatusSeverity::Information,
            "正在完成录制并生成镜头轨道…".into(),
        );
        self.telemetry.stop_requested.store(true, Ordering::Relaxed);
    }

    #[allow(clippy::too_many_lines)] // One ordered, nonblocking scheduling pass.
    fn on_timer(&mut self, hwnd: HWND) {
        crate::platform::editor_ui::sync_window_theme(&mut self.theme, hwnd);
        if self.close_when_finished && self.editor.is_none() && !self.phase.is_active() {
            self.telemetry.graceful_close.store(true, Ordering::Relaxed);
            post_quit_window(hwnd);
            return;
        }
        let now = Instant::now();
        if !self.pointer.tooltip_visible
            && self
                .pointer
                .tooltip_deadline
                .is_some_and(|deadline| now >= deadline)
        {
            self.pointer.tooltip_visible = self.pointer.hot.is_some();
            self.pointer.tooltip_deadline = None;
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
        if self.auto_start {
            self.auto_start = false;
            self.start_recording();
        }
        if self.phase == RecorderPhase::Starting
            && self
                .control
                .as_ref()
                .is_some_and(RecordingControl::recording_started)
        {
            self.phase = RecorderPhase::Recording;
            self.started_at = Some(Instant::now());
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                format!("正在录制{}", self.capture_source.label()),
            );
            self.telemetry
                .recording_started
                .store(true, Ordering::Relaxed);
        }
        if self.phase == RecorderPhase::Recording
            && self.auto_stop_after.is_some_and(|duration| {
                self.started_at
                    .is_some_and(|started| started.elapsed() >= duration)
            })
        {
            self.request_stop();
        }

        let worker_result = self
            .worker
            .as_ref()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(result) => Some(Ok(result)),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err(
                    "background worker disconnected unexpectedly".to_string(),
                )),
            });
        if let Some(result) = worker_result {
            self.worker = None;
            match result {
                Ok(BackgroundResult::Recording(Ok(report))) => {
                    self.control = None;
                    self.complete_recording(report);
                    if self.auto_export {
                        self.start_export();
                    } else if self.auto_stop_after.is_none() && !self.close_when_finished {
                        self.open_preview();
                    }
                }
                Ok(BackgroundResult::Export(Ok(report))) => self.complete_export(report),
                Ok(
                    BackgroundResult::Recording(Err(error)) | BackgroundResult::Export(Err(error)),
                )
                | Err(error) => self.fail(error),
            }
            let automation_finished = self.auto_export
                && matches!(self.phase, RecorderPhase::Complete | RecorderPhase::Failed);
            if automation_finished
                || (self.close_when_finished && !self.phase.is_active() && self.editor.is_none())
            {
                self.telemetry.graceful_close.store(true, Ordering::Relaxed);
                post_quit_window(hwnd);
                return;
            }
        }

        if self
            .open_last_close_at
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.telemetry.graceful_close.store(true, Ordering::Relaxed);
            post_quit_window(hwnd);
            return;
        }

        if self.phase.is_active()
            && self
                .auto_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.fail("recorder window smoke exceeded its watchdog deadline".into());
            post_quit_window(hwnd);
            return;
        }
        self.refresh(hwnd);
    }

    fn complete_recording(&mut self, report: RecordingReport) {
        self.phase = RecorderPhase::Complete;
        self.started_at = None;
        self.set_status(
            crate::platform::editor_ui::StatusSeverity::Information,
            format!(
                "项目已就绪：已编码 {} 帧，背压丢弃 {} 帧",
                report.frames_encoded, report.dropped_by_backpressure
            ),
        );
        self.last_project = Some(report.project_root.clone());
        self.last_report = Some(report.clone());
        *self
            .telemetry
            .recording
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(report);
    }

    fn start_export(&mut self) {
        if self.phase.is_active() {
            return;
        }
        let Some(project_root) = self.last_project.clone() else {
            return;
        };
        let output_path = match next_export_path(&project_root) {
            Ok(path) => path,
            Err(error) => {
                self.fail(error.to_string());
                return;
            }
        };
        let request = ExportRequest {
            project_root,
            output_path: output_path.clone(),
            maximum_duration: None,
        };
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = ProjectExporter::export(&request).map_err(|error| error.to_string());
            let _ = sender.send(BackgroundResult::Export(result));
        });
        self.phase = RecorderPhase::Exporting;
        self.started_at = Some(Instant::now());
        self.worker = Some(receiver);
        self.set_status(
            crate::platform::editor_ui::StatusSeverity::Information,
            format!("正在导出 1080p60 MP4：{}", output_path.display()),
        );
    }

    fn complete_export(&mut self, report: ExportReport) {
        self.phase = RecorderPhase::Complete;
        self.started_at = None;
        self.set_status(
            crate::platform::editor_ui::StatusSeverity::Success,
            format!(
                "导出完成：{} 帧，{} 字节",
                report.frames_written, report.output_bytes
            ),
        );
        self.last_export = Some(report.clone());
        *self
            .telemetry
            .export
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(report);
    }

    fn fail(&mut self, error: String) {
        self.phase = RecorderPhase::Failed;
        self.started_at = None;
        self.set_status(
            crate::platform::editor_ui::StatusSeverity::Error,
            format!("错误：{error}"),
        );
        *self
            .telemetry
            .error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error);
    }

    fn on_mouse_down(&mut self, hwnd: HWND, x: i32, y: i32) {
        unsafe {
            let _ = SetFocus(Some(hwnd));
        }
        let keyboard_focus_was_visible = self.pointer.keyboard_focus;
        self.pointer.keyboard_focus = false;
        self.cancel_pressed_control(hwnd, true);
        let controls = ControlLayout::for_window(hwnd, self.theme);
        let Some(control) = controls
            .hit_control(x, y)
            .filter(|control| self.control_enabled(*control))
        else {
            if keyboard_focus_was_visible {
                self.refresh(hwnd);
            }
            return;
        };
        self.pointer.pressed = Some(control);
        self.pointer.focused = Some(control);
        self.pointer.hot = Some(control);
        unsafe {
            SetCapture(hwnd);
        }
        self.refresh(hwnd);
    }

    fn activate_control(&mut self, hwnd: HWND, control: RecorderControlId) {
        if !self.control_enabled(control) {
            return;
        }
        self.accessibility.invoked(control as u32 + 1);
        match control {
            RecorderControlId::Menu => self.show_application_menu(hwnd),
            RecorderControlId::NewRecording => {
                self.phase = RecorderPhase::Ready;
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "选择录制来源后开始新录制".into(),
                );
            }
            RecorderControlId::WindowMinimize | RecorderControlId::WindowMaximize => unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{
                    IsZoomed, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
                };
                let command = if control == RecorderControlId::WindowMinimize {
                    SW_MINIMIZE
                } else if IsZoomed(hwnd).as_bool() {
                    SW_RESTORE
                } else {
                    SW_MAXIMIZE
                };
                let _ = ShowWindow(hwnd, command);
            },
            RecorderControlId::WindowClose => self.on_close(hwnd),
            RecorderControlId::MonitorSource => {
                self.capture_source = RecordingSource::PrimaryMonitor;
                self.capture_width = self.primary_monitor_width;
                self.capture_height = self.primary_monitor_height;
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Success,
                    "已选择整个主显示器".into(),
                );
            }
            RecorderControlId::WindowSource => self.select_capture_source(hwnd, true),
            RecorderControlId::Source => self.select_capture_source(
                hwnd,
                matches!(self.capture_source, RecordingSource::Window(_)),
            ),
            RecorderControlId::Record => {
                if matches!(
                    self.phase,
                    RecorderPhase::Starting | RecorderPhase::Recording
                ) {
                    self.request_stop();
                } else {
                    self.start_recording();
                }
            }
            RecorderControlId::OpenProject => self.choose_project(hwnd),
            RecorderControlId::Preview => self.open_preview(),
            RecorderControlId::Folder => self.open_project_folder(),
            RecorderControlId::Diagnostics => self.show_more_menu(hwnd),
        }
    }

    fn on_key(&mut self, hwnd: HWND, key: u16) {
        if key == VK_ESCAPE.0 {
            self.on_close(hwnd);
            return;
        }
        if key == VK_TAB.0 {
            let reverse = unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0;
            self.move_keyboard_focus(reverse);
            self.refresh(hwnd);
            return;
        }
        if self.pointer.keyboard_focus
            && matches!(key, value if value == VK_RETURN.0 || value == VK_SPACE.0)
            && let Some(control) = self
                .pointer
                .focused
                .filter(|control| self.control_enabled(*control))
        {
            self.activate_control(hwnd, control);
            self.refresh(hwnd);
        }
    }

    fn move_keyboard_focus(&mut self, reverse: bool) {
        let Some(hwnd) = self.hwnd else {
            return;
        };
        let layout = ControlLayout::for_window(hwnd, self.theme);
        self.pointer.focused = next_keyboard_focus(
            &RecorderControlId::ORDER,
            self.pointer.focused,
            reverse,
            |control| {
                self.control_enabled(control) && {
                    let rect = layout.control_rect(control).0;
                    rect.right > rect.left && rect.bottom > rect.top
                }
            },
        );
        self.pointer.keyboard_focus = true;
        if let Some(control) = self.pointer.focused {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                format!("键盘焦点：{}", control.accessibility_name()),
            );
        }
    }

    fn on_mouse_move(&mut self, hwnd: HWND, x: i32, y: i32) {
        let controls = ControlLayout::for_window(hwnd, self.theme);
        let next = controls
            .hit_control(x, y)
            .filter(|control| self.control_enabled(*control));
        if next != self.pointer.hot {
            let tooltip_was_visible = self.pointer.tooltip_visible;
            self.pointer.tooltip_visible = false;
            self.pointer.tooltip_deadline = next.map(|_| Instant::now() + TOOLTIP_DELAY);
            if let Some(previous) = self.pointer.hot {
                invalidate_control(hwnd, &controls, previous);
            }
            self.pointer.hot = next;
            if let Some(current) = self.pointer.hot {
                invalidate_control(hwnd, &controls, current);
            }
            if tooltip_was_visible {
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
        }
        if !self.pointer.tracking_leave {
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            self.pointer.tracking_leave = unsafe { TrackMouseEvent(&raw mut tracking) }.is_ok();
        }
        if let Ok(cursor) = unsafe { LoadCursorW(None, IDC_ARROW) } {
            unsafe {
                let _ = SetCursor(Some(cursor));
            }
        }
    }

    fn on_mouse_up(&mut self, hwnd: HWND, x: i32, y: i32) {
        if self.pointer.pressed.is_none() {
            return;
        }
        let controls = ControlLayout::for_window(hwnd, self.theme);
        let released_over = controls
            .hit_control(x, y)
            .filter(|control| self.control_enabled(*control));
        let activated = complete_pointer_activation(&mut self.pointer.pressed, released_over);
        unsafe {
            let _ = ReleaseCapture();
        }
        if let Some(control) = released_over {
            invalidate_control(hwnd, &controls, control);
        }
        if let Some(control) = activated {
            self.activate_control(hwnd, control);
        }
        self.refresh(hwnd);
    }

    fn on_mouse_leave(&mut self, hwnd: HWND) {
        self.pointer.tracking_leave = false;
        let tooltip_was_visible = self.pointer.tooltip_visible;
        self.pointer.tooltip_visible = false;
        self.pointer.tooltip_deadline = None;
        if let Some(control) = self.pointer.hot.take() {
            invalidate_control(hwnd, &ControlLayout::for_window(hwnd, self.theme), control);
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
            self.pointer.keyboard_focus = false;
            self.cancel_pressed_control(hwnd, true);
            self.accessibility.blur();
        }
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }

    fn clear_pressed_control(&mut self, hwnd: HWND) {
        let Some(control) = self.pointer.pressed.take() else {
            return;
        };
        invalidate_control(hwnd, &ControlLayout::for_window(hwnd, self.theme), control);
    }

    fn cancel_pressed_control(&mut self, hwnd: HWND, release_capture: bool) {
        self.clear_pressed_control(hwnd);
        if release_capture {
            unsafe {
                let _ = ReleaseCapture();
            }
        }
    }

    fn control_enabled(&self, control: RecorderControlId) -> bool {
        if self.transition.is_some()
            && !matches!(
                control,
                RecorderControlId::WindowClose
                    | RecorderControlId::WindowMinimize
                    | RecorderControlId::WindowMaximize
            )
        {
            return false;
        }
        match control {
            RecorderControlId::NewRecording => !self.phase.is_active(),
            RecorderControlId::WindowMinimize
            | RecorderControlId::WindowMaximize
            | RecorderControlId::WindowClose
            | RecorderControlId::Diagnostics
            | RecorderControlId::Menu => true,
            RecorderControlId::Source
            | RecorderControlId::MonitorSource
            | RecorderControlId::WindowSource
            | RecorderControlId::OpenProject => !self.phase.is_active() && self.editor.is_none(),
            RecorderControlId::Record => {
                self.editor.is_none()
                    && !matches!(
                        self.phase,
                        RecorderPhase::Finalizing | RecorderPhase::Exporting
                    )
            }
            RecorderControlId::Folder => false,
            RecorderControlId::Preview => self.last_project.is_some() && !self.phase.is_active(),
        }
    }

    fn show_application_menu(&mut self, hwnd: HWND) {
        use crate::platform::app_menu::{self, Action, Availability};
        let anchor = ControlLayout::for_window(hwnd, self.theme)
            .control_rect(RecorderControlId::Menu)
            .0;
        let available = Availability {
            new_recording: self.control_enabled(RecorderControlId::NewRecording),
            open: self.control_enabled(RecorderControlId::OpenProject),
            recent: self.control_enabled(RecorderControlId::Preview),
            save: false,
            export: false,
        };
        match app_menu::show(hwnd, anchor, self.theme, available) {
            Ok(Some(Action::NewRecording)) => {
                self.activate_control(hwnd, RecorderControlId::NewRecording);
            }
            Ok(Some(Action::Open)) => self.activate_control(hwnd, RecorderControlId::OpenProject),
            Ok(Some(Action::OpenLocation)) => self.open_project_folder(),
            Ok(Some(Action::ContinueRecent)) => {
                self.activate_control(hwnd, RecorderControlId::Preview);
            }
            Ok(_) => (),
            Err(error) => self.set_status(
                crate::platform::editor_ui::StatusSeverity::Error,
                format!("无法打开主菜单：{error}"),
            ),
        }
        self.pointer.hot = None;
        self.refresh(hwnd);
    }

    fn show_more_menu(&mut self, hwnd: HWND) {
        use crate::platform::app_menu::{self, Row};
        let mut rows = app_menu::theme_rows(self.theme);
        rows.push(Row::heading("诊断"));
        rows.push(Row::command(2, "查看诊断日志", IconKind::Log, true));
        let c = ControlLayout::for_window(hwnd, self.theme);
        let action = match app_menu::show_rows(hwnd, c.diagnostics.0, self.theme, rows, true) {
            Ok(Some(action)) => action,
            Ok(None) => return,
            Err(error) => {
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Error,
                    format!("无法打开设置：{error}"),
                );
                self.refresh(hwnd);
                return;
            }
        };
        if let Some(result) = crate::platform::editor_ui::theme_command(action) {
            if let Err(error) = result {
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Error,
                    format!("错误：设置未保存：{error}"),
                );
            }
            crate::platform::editor_ui::sync_window_theme(&mut self.theme, hwnd);
            self.refresh(hwnd);
            return;
        }
        if action == 2 {
            self.open_diagnostics();
        }
    }

    #[allow(clippy::too_many_lines)] // Native popup lifetime and selection dispatch stay together.
    fn select_capture_source(&mut self, hwnd: HWND, windows_only: bool) {
        let targets = match enumerate_capturable_windows() {
            Ok(targets) => targets,
            Err(error) => {
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Error,
                    format!("错误：无法枚举窗口：{error}"),
                );
                return;
            }
        };
        if windows_only && targets.is_empty() {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "没有可录制的窗口，请先打开需要录制的应用".into(),
            );
            return;
        }
        let menu = match unsafe { CreatePopupMenu() } {
            Ok(menu) => menu,
            Err(error) => {
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Error,
                    format!("错误：无法创建捕获来源菜单：{error}"),
                );
                return;
            }
        };
        let menu_result = (|| -> windows::core::Result<usize> {
            let monitor_label = wide("整个主显示器");
            if !windows_only {
                unsafe {
                    AppendMenuW(
                        menu,
                        MF_STRING,
                        MONITOR_SOURCE_COMMAND,
                        PCWSTR(monitor_label.as_ptr()),
                    )?;
                    AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null())?;
                }
            }
            for (index, target) in targets.iter().take(30).enumerate() {
                let label = menu_label(&target.title);
                let label = wide(&label);
                unsafe {
                    AppendMenuW(
                        menu,
                        MF_STRING,
                        FIRST_WINDOW_SOURCE_COMMAND + index,
                        PCWSTR(label.as_ptr()),
                    )?;
                }
            }
            let source_rect = ControlLayout::for_window(hwnd, self.theme).source.0;
            let mut point = POINT {
                x: source_rect.left,
                y: source_rect.bottom,
            };
            unsafe {
                ClientToScreen(hwnd, &raw mut point).ok()?;
                let _ = SetForegroundWindow(hwnd);
            }
            let command = unsafe {
                TrackPopupMenu(
                    menu,
                    TPM_LEFTALIGN | TPM_RETURNCMD,
                    point.x,
                    point.y,
                    None,
                    hwnd,
                    None,
                )
            };
            Ok(usize::try_from(command.0).unwrap_or_default())
        })();
        let _ = unsafe { DestroyMenu(menu) };
        let command = match menu_result {
            Ok(command) => command,
            Err(error) => {
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Error,
                    format!("错误：无法显示捕获来源菜单：{error}"),
                );
                return;
            }
        };
        if command == MONITOR_SOURCE_COMMAND {
            self.capture_source = RecordingSource::PrimaryMonitor;
            self.capture_width = self.primary_monitor_width;
            self.capture_height = self.primary_monitor_height;
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Success,
                "已选择主显示器".into(),
            );
        } else if command >= FIRST_WINDOW_SOURCE_COMMAND
            && let Some(target) = targets.get(command - FIRST_WINDOW_SOURCE_COMMAND)
        {
            self.capture_width = target.desktop_rect.width & !1;
            self.capture_height = target.desktop_rect.height & !1;
            self.capture_source = RecordingSource::Window(target.clone());
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Success,
                format!("已选择窗口：{}", target.title),
            );
        }
    }

    fn choose_project(&mut self, hwnd: HWND) {
        if let Some(project) = choose_project_path(hwnd) {
            self.requested_project = Some(project);
            self.post_navigation();
        }
    }

    fn open_preview(&mut self) {
        self.requested_project = self.last_project.clone();
        if self.requested_project.is_some() {
            self.post_navigation();
        }
    }

    fn post_navigation(&self) {
        if let Some(hwnd) = self.hwnd {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(hwnd),
                    WM_NAVIGATE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }

    fn navigate(&mut self, hwnd: HWND) {
        if self.transition.is_some() {
            return;
        }
        let intent = if let Some(editor) = &mut self.editor {
            editor.take_navigation()
        } else {
            self.requested_project
                .take()
                .map(NavigationIntent::OpenProject)
        };
        let Some(intent) = intent else {
            return;
        };
        let load = if let NavigationIntent::OpenProject(project) = &intent {
            let same_project = self.editor.as_ref().is_some_and(|editor| {
                fs::canonicalize(editor.project_root())
                    .ok()
                    .zip(fs::canonicalize(project).ok())
                    .is_some_and(|(current, requested)| current == requested)
            });
            match crate::platform::editor_tasks::LoadTask::start(project.clone(), !same_project) {
                Ok(load) => Some(load),
                Err(error) => {
                    self.transition_failed(hwnd, &error.to_string());
                    return;
                }
            }
        } else {
            None
        };
        if let Some(editor) = &mut self.editor {
            editor.loading(hwnd);
        } else {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "正在打开工程… · Esc 取消".into(),
            );
        }
        self.transition = Some(ContentTransition {
            intent,
            load,
            candidate: None,
        });
        self.poll_transition(hwnd);
    }

    fn transition_failed(&mut self, hwnd: HWND, error: &str) {
        self.transition = None;
        if let Some(editor) = &mut self.editor {
            editor.navigation_failed(hwnd, error);
        } else {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Error,
                format!("无法打开工程 · {error}"),
            );
            self.refresh(hwnd);
        }
    }

    fn cancel_transition(&mut self, hwnd: HWND) {
        self.transition = None;
        if let Some(editor) = &mut self.editor {
            editor.cancel_loading(hwnd);
        } else {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "已取消打开工程".into(),
            );
            self.refresh(hwnd);
        }
    }

    fn poll_transition(&mut self, hwnd: HWND) {
        // A native popup runs a nested message loop. Keep the current page alive
        // until its menu call returns, even if background loading finishes.
        if crate::platform::app_menu::is_open() {
            return;
        }
        let Some(mut transition) = self.transition.take() else {
            return;
        };
        if let Some(load) = &transition.load {
            let Some(result) = load.poll() else {
                self.transition = Some(transition);
                return;
            };
            transition.load = None;
            let prepared = result.and_then(|loaded| {
                let mut candidate = preview_window::WindowState::accept_loaded(loaded)
                    .map_err(|error| error.to_string())?;
                candidate.attach(hwnd).map_err(|error| error.to_string())?;
                Ok(candidate)
            });
            match prepared {
                Ok(candidate) => transition.candidate = Some(candidate),
                Err(error) => {
                    self.transition_failed(hwnd, &error);
                    return;
                }
            }
        }
        // Keep routing to the owned old editor while cleanup is pending or fails.
        // Publishing a temporary candidate here would require a fallible SetPropW
        // rollback before the candidate could safely be cancelled or dropped.
        if let Some(editor) = &mut self.editor {
            match editor.confirm_leave() {
                Some(true) => (),
                state => {
                    if state.is_none() {
                        self.transition = Some(transition);
                    } else {
                        editor.cleanup_failed(hwnd);
                    }
                    return;
                }
            }
        }
        // The old editor still owns its state and binding. A failed replacement
        // leaves that binding untouched; navigation_failed resumes its draft writer.
        // After a successful bind there is no fallible step before ownership moves.
        if let Some(candidate) = &mut transition.candidate
            && let Err(error) = candidate.bind(hwnd)
        {
            self.transition_failed(hwnd, &error.to_string());
            return;
        }
        if let Some(editor) = &mut self.editor {
            editor.detach(hwnd);
        }
        self.editor = None;
        self.accessibility.detach();
        self.pointer = RecorderPointerState::default();
        unsafe {
            let _ = ReleaseCapture();
        }
        self.close_when_finished = false;
        if transition.intent == NavigationIntent::Quit {
            self.telemetry.graceful_close.store(true, Ordering::Relaxed);
            post_quit_window(hwnd);
            return;
        }
        if let Some(editor) = transition.candidate {
            self.last_project = Some(editor.project_root().to_path_buf());
            self.editor = Some(editor);
            if let Some(editor) = &mut self.editor {
                editor.show_content(hwnd);
            }
        } else {
            self.phase = RecorderPhase::Ready;
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "选择录制来源后开始新录制".into(),
            );
            self.theme = EditorTheme::current(window_dpi(hwnd));
            self.refresh(hwnd);
        }
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }

    fn open_project_folder(&mut self) {
        let Some(project_root) = &self.last_project else {
            return;
        };
        match Command::new("explorer.exe").arg(project_root).spawn() {
            Ok(_) => self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "项目文件夹已打开".into(),
            ),
            Err(error) => self.set_status(
                crate::platform::editor_ui::StatusSeverity::Error,
                format!("错误：无法打开项目文件夹：{error}"),
            ),
        }
    }

    fn open_diagnostics(&mut self) {
        let Some(project_root) = &self.last_project else {
            return;
        };
        let log_path = project_root.join("diagnostics/session.log");
        if !log_path.is_file() {
            return;
        }
        match Command::new("notepad.exe").arg(log_path).spawn() {
            Ok(_) => self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "诊断日志已打开".into(),
            ),
            Err(error) => self.set_status(
                crate::platform::editor_ui::StatusSeverity::Error,
                format!("错误：无法打开诊断日志：{error}"),
            ),
        }
    }

    fn on_close(&mut self, hwnd: HWND) {
        if self.phase.is_active() {
            self.close_when_finished = true;
            self.request_stop();
            self.refresh(hwnd);
        } else {
            self.telemetry.graceful_close.store(true, Ordering::Relaxed);
            post_quit_window(hwnd);
        }
    }

    fn publish_accessibility(&self, hwnd: HWND) {
        use crate::platform::accessibility::{Kind, Node, STATUS_ID};
        let client = client_rect(hwnd);
        let layout = ControlLayout::for_window(hwnd, self.theme);
        let mut nodes: Vec<_> = RecorderControlId::ORDER
            .into_iter()
            .map(|id| {
                let mut node = Node::button(
                    id as u32 + 1,
                    id.accessibility_name(),
                    id.tooltip(),
                    layout.control_rect(id).0,
                    self.control_enabled(id),
                );
                if id == RecorderControlId::Record {
                    node.name = match self.phase {
                        RecorderPhase::Starting | RecorderPhase::Recording => "停止录制",
                        RecorderPhase::Finalizing => "正在完成录制",
                        _ => "开始录制（无音频）",
                    }
                    .into();
                }
                if id == RecorderControlId::Source {
                    node.help = format!(
                        "当前录制尺寸 {}×{}；本版本不录制音频",
                        self.capture_width, self.capture_height
                    );
                }
                node
            })
            .collect();
        nodes.push(Node {
            id: STATUS_ID,
            name: format!("{}。{}", self.phase_label(), self.status),
            help: String::new(),
            rect: layout.status,
            enabled: true,
            kind: Kind::Text,
        });
        let focus = self
            .pointer
            .keyboard_focus
            .then_some(self.pointer.focused)
            .flatten()
            .map(|id| id as u32 + 1);
        self.accessibility
            .publish(hwnd, "Panzo 录制器", client, nodes, focus);
    }

    fn accessibility_actions(&mut self, hwnd: HWND) {
        use crate::platform::accessibility::Action;
        for action in self.accessibility.take_actions() {
            if let Action::Invoke(id) | Action::Focus(id) = action
                && let Some(control) = RecorderControlId::ORDER
                    .into_iter()
                    .find(|control| *control as u32 + 1 == id)
                && self.control_enabled(control)
            {
                self.pointer.focused = Some(control);
                self.pointer.keyboard_focus = true;
                if matches!(action, Action::Invoke(_)) {
                    self.activate_control(hwnd, control);
                }
            }
        }
        self.refresh(hwnd);
    }

    fn refresh(&self, hwnd: HWND) {
        self.publish_accessibility(hwnd);
        let title = format!("Panzo 录制器 — {}", self.phase_label());
        let title = wide(&title);
        unsafe {
            let _ = SetWindowTextW(hwnd, PCWSTR(title.as_ptr()));
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }

    fn phase_label(&self) -> &'static str {
        match self.phase {
            RecorderPhase::Ready => "准备就绪",
            RecorderPhase::Starting => "正在启动",
            RecorderPhase::Recording => "正在录制",
            RecorderPhase::Finalizing => "正在完成",
            RecorderPhase::Exporting => "正在导出 MP4",
            RecorderPhase::Complete => "项目已就绪",
            RecorderPhase::Failed => "发生错误",
        }
    }

    fn elapsed(&self) -> Duration {
        self.started_at
            .map_or(Duration::ZERO, |started| started.elapsed())
    }

    fn paint(&self, hwnd: HWND) {
        let _scale = crate::platform::editor_ui::DrawingScale::enter(self.theme.dpi);
        let mut paint = PAINTSTRUCT::default();
        let hdc = unsafe { BeginPaint(hwnd, &raw mut paint) };
        let client = client_rect(hwnd);
        let width = (client.right - client.left).max(1);
        let height = (client.bottom - client.top).max(1);
        let buffer_dc = unsafe { CreateCompatibleDC(Some(hdc)) };
        let buffer_bitmap = unsafe { CreateCompatibleBitmap(hdc, width, height) };
        if !buffer_dc.0.is_null() && !buffer_bitmap.0.is_null() {
            let previous = unsafe { SelectObject(buffer_dc, HGDIOBJ(buffer_bitmap.0)) };
            draw_scene(buffer_dc, client, self);
            preview_window::editor_titlebar::paint_system_buttons(hwnd, buffer_dc, self.theme);
            let _ = unsafe { BitBlt(hdc, 0, 0, width, height, Some(buffer_dc), 0, 0, SRCCOPY) };
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
            draw_scene(hdc, client, self);
            preview_window::editor_titlebar::paint_system_buttons(hwnd, hdc, self.theme);
        }
        unsafe {
            let _ = EndPaint(hwnd, &raw const paint);
        }
    }
}

#[derive(Clone, Copy)]
struct HitRect(RECT);

impl HitRect {
    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.0.left && x < self.0.right && y >= self.0.top && y < self.0.bottom
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecorderControlId {
    Source,
    Record,
    OpenProject,
    Preview,
    Folder,
    Diagnostics,
    MonitorSource,
    WindowSource,
    NewRecording,
    WindowMinimize,
    WindowMaximize,
    WindowClose,
    Menu,
}

impl RecorderControlId {
    const ORDER: [Self; 13] = [
        Self::Menu,
        Self::Diagnostics,
        Self::NewRecording,
        Self::OpenProject,
        Self::MonitorSource,
        Self::WindowSource,
        Self::Source,
        Self::Record,
        Self::Preview,
        Self::Folder,
        Self::WindowMinimize,
        Self::WindowMaximize,
        Self::WindowClose,
    ];

    const fn accessibility_name(self) -> &'static str {
        match self {
            Self::MonitorSource => "整个屏幕",
            Self::WindowSource => "应用窗口",
            Self::Source => "选择显示器或应用窗口",
            Self::Record => "开始或停止屏幕录制",
            Self::OpenProject => "打开已有工程",
            Self::Preview => "打开视频编辑器",
            Self::Folder => "打开项目文件夹",
            Self::Diagnostics => "设置",
            Self::Menu => "主菜单",
            Self::NewRecording => "新录制",
            Self::WindowMinimize => "最小化",
            Self::WindowMaximize => "最大化或还原",
            Self::WindowClose => "关闭窗口",
        }
    }

    const fn tooltip(self) -> &'static str {
        match self {
            Self::MonitorSource => "录制整个主显示器，保持原始分辨率",
            Self::WindowSource => "选择一个可见应用窗口录制",
            Self::Source => "选择整个主显示器或一个可见应用窗口",
            Self::Record => "开始/停止录制（Enter 或 Space）",
            Self::OpenProject => "选择 .panzo 工程文件夹中的 project.json",
            Self::Preview => "在编辑器中调整镜头和画布",
            Self::Folder => "在资源管理器中打开项目",
            Self::Diagnostics => "设置：主题与诊断",
            Self::Menu => "主菜单",
            Self::NewRecording => "新录制",
            Self::WindowMinimize => "最小化",
            Self::WindowMaximize => "最大化或还原",
            Self::WindowClose => "关闭窗口",
        }
    }

    const fn icon(self, phase: RecorderPhase) -> IconKind {
        match self {
            Self::Menu => IconKind::Menu,
            Self::Source | Self::MonitorSource => IconKind::Monitor,
            Self::WindowSource => IconKind::Window,
            Self::Record if matches!(phase, RecorderPhase::Starting | RecorderPhase::Recording) => {
                IconKind::Stop
            }
            Self::Record | Self::NewRecording => IconKind::Record,
            Self::OpenProject | Self::Folder => IconKind::Folder,
            Self::Preview => IconKind::Edit,
            Self::Diagnostics => IconKind::Settings,
            Self::WindowMinimize => IconKind::Minimize,
            Self::WindowMaximize => IconKind::Maximize,
            Self::WindowClose => IconKind::Close,
        }
    }
}

#[derive(Clone, Copy)]
struct ControlLayout {
    header: crate::platform::editor_ui::HeaderLayout,
    source: HitRect,
    record: HitRect,
    export: HitRect,
    preview: HitRect,
    folder: HitRect,
    diagnostics: HitRect,
    monitor: HitRect,
    window: HitRect,
    hero: RECT,
    status: RECT,
}
impl ControlLayout {
    fn new(client: RECT, theme: EditorTheme) -> Self {
        let scale = |n| theme.scale(n);
        let width = client.right;
        let height = client.bottom;
        let cw = scale(440).min(width - scale(48));
        let x = (width - cw) / 2;
        let content_top = theme.metrics.toolbar_height + theme.metrics.outer_margin;
        let y = content_top + ((height - content_top - scale(28) - scale(344)) / 2).max(0);
        let header = crate::platform::editor_ui::HeaderLayout::new(client, theme, None);
        let r = |left, top, width, height| {
            HitRect(RECT {
                left,
                top,
                right: left + width,
                bottom: top + height,
            })
        };
        let row = |top, height| r(x, y + scale(top), cw, scale(height));
        Self {
            header,
            hero: r(x, y, cw, scale(344)).0,
            source: row(120, 80),
            monitor: r(x, y + scale(72), cw / 2, scale(34)),
            window: r(x + cw / 2, y + scale(72), cw - cw / 2, scale(34)),
            record: row(244, 46),
            export: HitRect(header.rect(crate::platform::editor_ui::HeaderAction::OpenProject)),
            preview: row(312, 32),
            folder: r(0, 0, 0, 0),
            diagnostics: HitRect(header.rect(crate::platform::editor_ui::HeaderAction::Settings)),
            status: r(scale(20), height - scale(28), width - scale(40), scale(24)).0,
        }
    }

    fn for_window(hwnd: HWND, theme: EditorTheme) -> Self {
        let client = client_rect(hwnd);
        let mut layout = Self::new(client, theme);
        layout.header = crate::platform::editor_ui::HeaderLayout::new(
            client,
            theme,
            crate::platform::preview_window::editor_titlebar::caption_bounds(hwnd),
        );
        layout.export = HitRect(
            layout
                .header
                .rect(crate::platform::editor_ui::HeaderAction::OpenProject),
        );
        layout.diagnostics = HitRect(
            layout
                .header
                .rect(crate::platform::editor_ui::HeaderAction::Settings),
        );
        layout
    }

    fn hit_control(self, x: i32, y: i32) -> Option<RecorderControlId> {
        RecorderControlId::ORDER
            .into_iter()
            .find(|id| self.control_rect(*id).contains(x, y))
    }
    fn control_rect(self, id: RecorderControlId) -> HitRect {
        match id {
            RecorderControlId::Menu => HitRect(
                self.header
                    .rect(crate::platform::editor_ui::HeaderAction::Menu),
            ),
            RecorderControlId::Source => self.source,
            RecorderControlId::Record => self.record,
            RecorderControlId::OpenProject => self.export,
            RecorderControlId::Preview => self.preview,
            RecorderControlId::Folder => self.folder,
            RecorderControlId::Diagnostics => self.diagnostics,
            RecorderControlId::MonitorSource => self.monitor,
            RecorderControlId::WindowSource => self.window,
            RecorderControlId::NewRecording => HitRect(
                self.header
                    .rect(crate::platform::editor_ui::HeaderAction::NewRecording),
            ),
            RecorderControlId::WindowMinimize => HitRect(
                self.header
                    .rect(crate::platform::editor_ui::HeaderAction::Minimize),
            ),
            RecorderControlId::WindowMaximize => HitRect(
                self.header
                    .rect(crate::platform::editor_ui::HeaderAction::Maximize),
            ),
            RecorderControlId::WindowClose => HitRect(
                self.header
                    .rect(crate::platform::editor_ui::HeaderAction::Close),
            ),
        }
    }
}

fn invalidate_control(hwnd: HWND, controls: &ControlLayout, id: RecorderControlId) {
    let rect = controls.control_rect(id).0;
    unsafe {
        let _ = InvalidateRect(Some(hwnd), Some(&raw const rect), false);
    }
}

#[allow(clippy::too_many_lines)]
fn draw_scene(hdc: windows::Win32::Graphics::Gdi::HDC, client: RECT, state: &WindowState) {
    use crate::platform::editor_ui::{HeaderAction, draw_header};
    let theme = state.theme;
    let p = theme.palette;
    let s = |n| theme.scale(n);
    let c = state.hwnd.map_or_else(
        || ControlLayout::new(client, theme),
        |hwnd| ControlLayout::for_window(hwnd, theme),
    );
    fill(hdc, &client, p.window);
    let header_controls = [RecorderControlId::Menu, RecorderControlId::Diagnostics];
    let header_states = header_controls.map(|id| ButtonVisualState {
        interaction: button_interaction(
            state.control_enabled(id),
            state.pointer.pressed == Some(id),
            state.pointer.hot == Some(id),
        ),
        selected: false,
    });
    let focused = [HeaderAction::Menu, HeaderAction::Settings]
        .into_iter()
        .zip(header_controls)
        .find_map(|(action, id)| {
            (state.pointer.keyboard_focus && state.pointer.focused == Some(id)).then_some(action)
        });
    draw_header(hdc, c.header, theme, header_states, focused);
    let row = |top, height| RECT {
        top: c.hero.top + s(top),
        bottom: c.hero.top + s(top + height),
        ..c.hero
    };
    let title = match state.phase {
        RecorderPhase::Starting => "正在准备录制",
        RecorderPhase::Recording => "正在录制",
        RecorderPhase::Finalizing => "正在完成录制",
        _ => "新录制",
    };
    draw_text_styled(
        hdc,
        row(0, 32),
        title,
        p.text,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        TextStyle::Title,
    );
    draw_text_styled(
        hdc,
        row(36, 24),
        if state.phase.is_active() {
            "完成后进入编辑器"
        } else {
            "选择要录制的内容"
        },
        p.text_secondary,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        TextStyle::Body,
    );
    for (id, label) in [
        (RecorderControlId::MonitorSource, "整个屏幕"),
        (RecorderControlId::WindowSource, "应用窗口"),
    ] {
        button(
            hdc,
            c.control_rect(id).0,
            label,
            state,
            id,
            state.control_enabled(id),
            false,
        );
    }
    let source = c.source.0;
    let (bg, _, fg) = button_colors(
        theme,
        ButtonRole::Standard,
        ButtonVisualState {
            interaction: button_interaction(
                !state.phase.is_active(),
                state.pointer.pressed == Some(RecorderControlId::Source),
                state.pointer.hot == Some(RecorderControlId::Source),
            ),
            selected: false,
        },
    );
    rounded_surface(hdc, source, bg, bg, s(10));
    draw_icon(
        hdc,
        RECT {
            left: source.left + s(18),
            top: source.top + s(24),
            right: source.left + s(50),
            bottom: source.top + s(56),
        },
        if matches!(state.capture_source, RecordingSource::PrimaryMonitor) {
            IconKind::Monitor
        } else {
            IconKind::Window
        },
        p.text_secondary,
    );
    draw_text_styled(
        hdc,
        RECT {
            left: source.left + s(68),
            right: source.right - s(38),
            top: source.top + s(14),
            bottom: source.top + s(40),
        },
        &compact_source_label(&state.capture_source.label()),
        fg,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        TextStyle::Body,
    );
    draw_text_styled(
        hdc,
        RECT {
            left: source.left + s(68),
            right: source.right - s(38),
            top: source.top + s(41),
            bottom: source.bottom - s(12),
        },
        &format!(
            "{} × {} · 60 FPS",
            state.capture_width, state.capture_height
        ),
        p.text_secondary,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        TextStyle::Caption,
    );
    draw_icon(
        hdc,
        RECT {
            left: source.right - s(30),
            right: source.right - s(14),
            top: source.top + s(32),
            bottom: source.top + s(48),
        },
        IconKind::ChevronDown,
        fg,
    );
    if state.pointer.keyboard_focus && state.pointer.focused == Some(RecorderControlId::Source) {
        focus_ring(hdc, source, p.accent, s(10));
    }
    draw_text_styled(
        hdc,
        row(208, 24),
        "不录制声音",
        p.text_secondary,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        TextStyle::Caption,
    );
    let label = match state.phase {
        RecorderPhase::Recording => format!("停止录制   {}", format_duration(state.elapsed())),
        RecorderPhase::Starting => "停止录制 · 正在准备…".into(),
        RecorderPhase::Finalizing => "正在完成录制…".into(),
        _ => "开始录制".into(),
    };
    button(
        hdc,
        c.record.0,
        &label,
        state,
        RecorderControlId::Record,
        state.control_enabled(RecorderControlId::Record),
        matches!(
            state.phase,
            RecorderPhase::Starting | RecorderPhase::Recording
        ),
    );
    if state.last_project.is_some() {
        button(
            hdc,
            c.preview.0,
            "继续编辑最近的工程",
            state,
            RecorderControlId::Preview,
            state.control_enabled(RecorderControlId::Preview),
            false,
        );
    }
    draw_text_styled(
        hdc,
        c.status,
        &state.status,
        state.status_severity.color(theme),
        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        TextStyle::Caption,
    );
    if state.pointer.tooltip_visible
        && let Some(id) = state.pointer.hot
    {
        draw_control_tooltip(hdc, client, &c, id, theme);
    }
}

fn draw_control_tooltip(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    client: RECT,
    controls: &ControlLayout,
    control: RecorderControlId,
    theme: EditorTheme,
) {
    crate::platform::editor_ui::draw_tooltip(
        hdc,
        client,
        controls.control_rect(control).0,
        control.tooltip(),
        theme,
    );
}

fn button(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    label: &str,
    state: &WindowState,
    id: RecorderControlId,
    enabled: bool,
    destructive: bool,
) {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let theme = state.theme;
    let role = if destructive {
        ButtonRole::Destructive
    } else if id == RecorderControlId::Record {
        ButtonRole::Primary
    } else if matches!(
        id,
        RecorderControlId::Diagnostics
            | RecorderControlId::Preview
            | RecorderControlId::MonitorSource
            | RecorderControlId::WindowSource
    ) {
        ButtonRole::Toolbar
    } else {
        ButtonRole::Standard
    };
    let visual = ButtonVisualState {
        interaction: button_interaction(
            enabled,
            state.pointer.pressed == Some(id),
            state.pointer.hot == Some(id),
        ),
        selected: destructive
            || matches!(
                (id, &state.capture_source),
                (
                    RecorderControlId::MonitorSource,
                    RecordingSource::PrimaryMonitor
                ) | (RecorderControlId::WindowSource, RecordingSource::Window(_))
            ),
    };
    crate::platform::editor_ui::draw_button(
        hdc,
        rect,
        label,
        id.icon(state.phase),
        theme,
        role,
        visual,
        state.pointer.keyboard_focus && state.pointer.focused == Some(id),
    );
}

#[allow(clippy::too_many_lines)]
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    #[cfg(test)]
    if let Some(result) = unified_window_review::handle_message(hwnd, message, wparam, lparam) {
        return result;
    }
    if message == WM_QUIT_WINDOW {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return LRESULT(0);
    }
    if (message == WM_CLOSE || (message == WM_KEYDOWN && wparam.0 == usize::from(VK_ESCAPE.0)))
        && let Some(state) = unsafe { state_mut(hwnd) }
        && state.transition.is_some()
    {
        state.cancel_transition(hwnd);
        if message == WM_KEYDOWN {
            return LRESULT(0);
        }
    }
    if message == WM_TIMER
        && wparam.0 == TIMER_ID
        && let Some(state) = unsafe { state_mut(hwnd) }
    {
        state.poll_transition(hwnd);
    }
    if message == WM_NAVIGATE {
        if let Some(state) = unsafe { state_mut(hwnd) } {
            state.navigate(hwnd);
        }
        return LRESULT(0);
    }
    if !matches!(message, WM_DESTROY | WM_NCDESTROY)
        && !unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetPropW(hwnd, w!("PanzoEditorState"))
        }
        .is_invalid()
    {
        return unsafe { preview_window::window_proc(hwnd, message, wparam, lparam) };
    }
    if let Some(result) =
        preview_window::editor_titlebar::window_message(hwnd, message, wparam, lparam)
    {
        return result;
    }
    if let Some(result) =
        crate::platform::accessibility::window_message(hwnd, message, wparam, lparam)
    {
        return result;
    }
    match message {
        crate::platform::accessibility::WM_ACCESS_ACTION => {
            unsafe {
                let _ = SetFocus(Some(hwnd));
            }
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.accessibility_actions(hwnd);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_ID => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.on_timer(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.on_mouse_down(hwnd, signed_low(lparam.0), signed_high(lparam.0));
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.on_mouse_up(hwnd, signed_low(lparam.0), signed_high(lparam.0));
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
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
        WM_KEYDOWN => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
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
                state.cancel_pressed_control(hwnd, true);
            }
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.clear_pressed_control(hwnd);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.on_close(hwnd);
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
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
                theme.scale(900),
                theme.scale(470),
            );
            limits.ptMinTrackSize = POINT { x, y };
            LRESULT(0)
        }
        WM_SIZE => {
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let dpi = u32::try_from((wparam.0 >> 16) & 0xffff)
                .unwrap_or(96)
                .max(96);
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.theme = EditorTheme::current(dpi);
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
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_DESTROY => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_ID);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
            if pointer != 0 {
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    let mut state = Box::from_raw(pointer as *mut WindowState);
                    if let Some(editor) = &mut state.editor {
                        editor.detach(hwnd);
                    }
                    drop(state);
                }
            }
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut WindowState> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut WindowState;
    unsafe { pointer.as_mut() }
}

pub(crate) fn choose_project_path(hwnd: HWND) -> Option<PathBuf> {
    use windows::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
        OPENFILENAMEW,
    };
    let mut path = vec![0_u16; 32_768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: w!("Panzo 工程（project.json）\0project.json\0\0"),
        lpstrFile: windows::core::PWSTR(path.as_mut_ptr()),
        nMaxFile: path.len() as u32,
        lpstrTitle: w!("选择 .panzo 文件夹中的 project.json"),
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&raw mut dialog) }.as_bool() {
        return None;
    }
    let len = path.iter().position(|c| *c == 0).unwrap_or(path.len());
    PathBuf::from(std::ffi::OsString::from_wide(&path[..len]))
        .parent()
        .map(Path::to_path_buf)
}

pub(crate) fn recent_project(hwnd: HWND) -> Option<PathBuf> {
    unsafe { state_mut(hwnd) }.and_then(|state| state.last_project.clone())
}

fn find_last_ready_project(library_root: &Path) -> Option<PathBuf> {
    fs::read_dir(library_root)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir()
                || !path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("panzo"))
            {
                return None;
            }
            let layout = ProjectLayout::open(&path).ok()?;
            let manifest = layout.load_manifest().ok()?;
            if !matches!(
                manifest.state,
                ProjectState::Ready | ProjectState::Recovered
            ) || !layout.root().join(&manifest.media.screen).is_file()
            {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

fn next_project_path(library_root: &Path) -> Result<PathBuf, RecorderWindowError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(RecorderWindowError::SystemClock)?
        .as_millis();
    for suffix in 0..1_000_u16 {
        let path = library_root.join(format!("panzo-{timestamp}-{suffix:03}.panzo"));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(RecorderWindowError::ProjectNameExhausted)
}

fn next_export_path(project_root: &Path) -> Result<PathBuf, RecorderWindowError> {
    let parent = project_root
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| RecorderWindowError::InvalidProjectPath(project_root.into()))?;
    let stem = project_root
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| RecorderWindowError::InvalidProjectPath(project_root.into()))?;
    for suffix in 0..1_000_u16 {
        let file_name = if suffix == 0 {
            format!("{stem}-1080p60.mp4")
        } else {
            format!("{stem}-1080p60-{suffix:03}.mp4")
        };
        let output = parent.join(&file_name);
        let temporary = output.with_file_name(format!("{file_name}.panzo-part.mp4"));
        if !output.exists() && !temporary.exists() {
            return Ok(output);
        }
    }
    Err(RecorderWindowError::ExportNameExhausted)
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3_600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

fn compact_source_label(label: &str) -> String {
    const LIMIT: usize = 72;
    if label.chars().count() <= LIMIT {
        return label.to_string();
    }
    let compact: String = label.chars().take(LIMIT.saturating_sub(1)).collect();
    format!("{compact}…")
}

fn menu_label(title: &str) -> String {
    const LIMIT: usize = 80;
    let escaped = title.replace('&', "&&");
    if escaped.chars().count() <= LIMIT {
        return escaped;
    }
    let mut compact: String = escaped.chars().take(LIMIT.saturating_sub(1)).collect();
    compact.push('…');
    compact
}

fn client_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &raw mut rect) };
    rect
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
    if value.is_empty() || rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let mut text: Vec<u16> = value.encode_utf16().collect();
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

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
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

fn win<T>(stage: &'static str, result: windows::core::Result<T>) -> Result<T, RecorderWindowError> {
    result.map_err(|source| RecorderWindowError::WindowsStage { stage, source })
}

#[derive(Debug, Error)]
pub enum RecorderWindowError {
    #[error("editor failed: {0}")]
    Editor(String),
    #[error("recorder smoke duration must be at least 500 ms")]
    InvalidSmokeDuration,
    #[error("open-last smoke duration must be at least 250 ms")]
    InvalidOpenLastSmokeDuration,
    #[error("primary monitor returned invalid geometry")]
    InvalidMonitorGeometry,
    #[error("could not create project library {path}: {source}")]
    CreateLibrary {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("system clock is earlier than the Unix epoch: {0}")]
    SystemClock(std::time::SystemTimeError),
    #[error("could not allocate a unique Panzo project name")]
    ProjectNameExhausted,
    #[error("invalid Panzo project path: {0}")]
    InvalidProjectPath(PathBuf),
    #[error("could not allocate a unique MP4 export name")]
    ExportNameExhausted,
    #[error("Windows call failed: {0}")]
    Windows(windows::core::Error),
    #[error("Windows stage '{stage}' failed: {source}")]
    WindowsStage {
        stage: &'static str,
        source: WindowsError,
    },
}

fn post_quit_window(hwnd: HWND) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
            Some(hwnd),
            WM_QUIT_WINDOW,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "explicit local UI review output directory required"]
    fn render_ui_review() {
        let output = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
        let project = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
        let mut state = WindowState::new(
            project.parent().unwrap().into(),
            2560,
            1440,
            Automation::None,
            Arc::new(WindowTelemetry::default()),
        );
        for (dpi, width, height) in [(96, 940, 640), (144, 960, 720)] {
            state.theme = EditorTheme::for_dpi(dpi);
            let _drawing = crate::platform::editor_ui::DrawingScale::enter(dpi);
            crate::platform::editor_ui::render_review_png(
                &output.join(format!("recorder-{dpi}.png")),
                width,
                height,
                |dc, rect| draw_scene(dc, rect, &state),
            );
        }
        state.theme = EditorTheme::for_dpi(96);
        state.phase = RecorderPhase::Recording;
        state.started_at = Instant::now().checked_sub(Duration::from_secs(12));
        let _drawing = crate::platform::editor_ui::DrawingScale::enter(96);
        crate::platform::editor_ui::render_review_png(
            &output.join("recorder-recording.png"),
            940,
            640,
            |dc, rect| draw_scene(dc, rect, &state),
        );
        state
            .theme
            .set_mode(crate::platform::ui_preferences::ThemeMode::Dark);
        crate::platform::editor_ui::render_review_png(
            &output.join("recorder-dark-recording.png"),
            940,
            640,
            |dc, rect| draw_scene(dc, rect, &state),
        );
        state.phase = RecorderPhase::Ready;
        crate::platform::editor_ui::render_review_png(
            &output.join("recorder-dark-ready.png"),
            940,
            640,
            |dc, rect| draw_scene(dc, rect, &state),
        );
        let library = project.parent().unwrap().to_path_buf();
        RecorderWindow::run_internal(
            &library,
            Automation::None,
            None,
            Some(Box::new(move |raw| {
                use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
                let hwnd = HWND(raw as *mut std::ffi::c_void);
                let theme = EditorTheme::current(window_dpi(hwnd));
                let client = client_rect(hwnd);
                let native = preview_window::editor_titlebar::caption_bounds(hwnd).unwrap();
                preview_window::editor_titlebar::review_activation_colors(
                    hwnd, &output, "recorder",
                );
                let path = output.join("native-recorder-titlebar.png");
                crate::platform::editor_ui::render_review_png(
                    &path,
                    client.right,
                    theme.scale(40),
                    |dc, _| {
                        assert!(unsafe { PrintWindow(hwnd, dc, PRINT_WINDOW_FLAGS(2)) }.as_bool());
                    },
                );
                let pixels = image::open(path).unwrap().to_rgb8();
                let background = pixels
                    .get_pixel(
                        theme.scale(300).cast_unsigned(),
                        (native.top + theme.scale(3)).cast_unsigned(),
                    )
                    .0;
                let width = (native.right - native.left) / 3;
                for i in 0..3 {
                    assert_eq!(
                        background,
                        pixels
                            .get_pixel(
                                (native.left + i * width + theme.scale(3)).cast_unsigned(),
                                (native.top + theme.scale(3)).cast_unsigned()
                            )
                            .0
                    );
                }
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        Some(hwnd),
                        WM_CLOSE,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            })),
        )
        .unwrap();
    }

    #[test]
    fn formats_recorder_duration_without_fractional_drift() {
        assert_eq!(format_duration(Duration::from_secs(0)), "00:00:00");
        assert_eq!(format_duration(Duration::from_secs(3_661)), "01:01:01");
    }

    #[test]
    fn recorder_controls_have_distinct_hit_targets() {
        let layout = ControlLayout::new(
            RECT {
                left: 0,
                top: 0,
                right: 940,
                bottom: 470,
            },
            EditorTheme::light(),
        );
        for id in RecorderControlId::ORDER {
            let rect = layout.control_rect(id).0;
            if rect.right <= rect.left {
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
    fn recorder_controls_scale_without_overlap_across_supported_dpi() {
        for dpi in [96, 120, 144, 192] {
            for (width, height) in [(640, 470), (940, 520), (940, 720), (1440, 900)] {
                let theme = EditorTheme::for_dpi(dpi);
                let client = RECT {
                    left: 0,
                    top: 0,
                    right: theme.scale(width),
                    bottom: theme.scale(height),
                };
                let layout = ControlLayout::new(client, theme);
                for (i, id) in RecorderControlId::ORDER.iter().enumerate() {
                    let a = layout.control_rect(*id).0;
                    if a.right <= a.left {
                        continue;
                    }
                    assert!(
                        a.left >= 0
                            && a.top >= 0
                            && a.right <= client.right
                            && a.bottom <= layout.status.top,
                        "{dpi} {width}x{height} {id:?}"
                    );
                    for other in &RecorderControlId::ORDER[i + 1..] {
                        let b = layout.control_rect(*other).0;
                        assert!(
                            a.right <= b.left
                                || a.left >= b.right
                                || a.bottom <= b.top
                                || a.top >= b.bottom,
                            "{id:?} {other:?}"
                        );
                    }
                }
                assert_eq!(
                    layout.record.0.right - layout.record.0.left,
                    theme.scale(440)
                );
                assert!(layout.source.0.bottom < layout.record.0.top);
            }
        }
    }

    #[test]
    fn every_recorder_control_has_a_unique_accessible_name_and_tooltip() {
        let mut names = std::collections::HashSet::new();
        for id in RecorderControlId::ORDER {
            assert!(!id.accessibility_name().trim().is_empty());
            assert!(!id.tooltip().trim().is_empty());
            assert!(names.insert(id.accessibility_name()));
        }
    }
}
