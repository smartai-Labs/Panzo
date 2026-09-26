use crate::platform::capture_probe::primary_monitor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use thiserror::Error;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush, DT_CENTER,
    DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, HBITMAP,
    HBRUSH, HDC, HGDIOBJ, InvalidateRect, PAINTSTRUCT, SRCCOPY, SelectObject, SetBkMode,
    SetTextColor, TRANSPARENT, UpdateWindow,
};
use windows::Win32::Media::{TIMERR_NOERROR, timeBeginPeriod, timeEndPeriod};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GWLP_USERDATA, GetClientRect, GetMessageW, GetWindowLongPtrW, IDC_ARROW, KillTimer,
    LoadCursorW, MSG, PostQuitMessage, RegisterClassW, SW_SHOW, SetTimer, SetWindowLongPtrW,
    ShowWindow, TranslateMessage, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_NCDESTROY,
    WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Error as WindowsError, w};

const TIMER_ID: usize = 1;
const TIMER_POLL_INTERVAL_MS: u32 = 10;
const TIMER_RESOLUTION_MS: u32 = 1;
const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);
const TILE_SIZE: i32 = 128;
const BAND_WIDTH: i32 = 48;

pub struct StabilityStimulus;

impl StabilityStimulus {
    pub fn run(duration: Duration) -> Result<StabilityStimulusReport, StabilityStimulusError> {
        Self::run_sized(duration, None)
    }

    pub fn run_windowed(
        duration: Duration,
        width: i32,
        height: i32,
    ) -> Result<StabilityStimulusReport, StabilityStimulusError> {
        Self::run_sized(duration, Some((width, height)))
    }

    #[allow(clippy::too_many_lines)]
    fn run_sized(
        duration: Duration,
        size: Option<(i32, i32)>,
    ) -> Result<StabilityStimulusReport, StabilityStimulusError> {
        if duration < Duration::from_secs(1) {
            return Err(StabilityStimulusError::InvalidDuration);
        }
        let monitor = primary_monitor().map_err(StabilityStimulusError::Windows)?;
        let (width, height) = size.unwrap_or((
            monitor.rect[2].saturating_sub(monitor.rect[0]),
            monitor.rect[3].saturating_sub(monitor.rect[1]),
        ));
        if width <= 0 || height <= 0 {
            return Err(StabilityStimulusError::InvalidMonitorGeometry);
        }
        let _timer_resolution = TimerResolution::acquire()?;

        let telemetry = Arc::new(WindowTelemetry::default());
        let started = Instant::now();
        let state = Box::new(WindowState {
            started,
            next_frame_at: started + FRAME_INTERVAL,
            duration,
            frame_index: 0,
            back_buffer: None,
            telemetry: Arc::clone(&telemetry),
        });
        let module = win("GetModuleHandleW", unsafe { GetModuleHandleW(None) })?;
        let instance = HINSTANCE(module.0);
        let cursor = win("LoadCursorW", unsafe { LoadCursorW(None, IDC_ARROW) })?;
        let class_name = w!("PanzoM4StabilityStimulus");
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: cursor,
            lpszClassName: class_name,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&raw const window_class) } == 0 {
            return Err(StabilityStimulusError::WindowsStage {
                stage: "RegisterClassW",
                source: WindowsError::from_thread(),
            });
        }

        let hwnd = win("CreateWindowExW", unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST,
                class_name,
                w!("Panzo M4 60 Hz Stability Stimulus"),
                WS_POPUP,
                monitor.rect[0],
                monitor.rect[1],
                width,
                height,
                None,
                None,
                Some(instance),
                None,
            )
        })?;
        let state_pointer = Box::into_raw(state);
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_pointer as isize);
        }
        if unsafe { SetTimer(Some(hwnd), TIMER_ID, TIMER_POLL_INTERVAL_MS, None) } == 0 {
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(state_pointer));
                let _ = DestroyWindow(hwnd);
            }
            return Err(StabilityStimulusError::WindowsStage {
                stage: "SetTimer",
                source: WindowsError::from_thread(),
            });
        }
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = UpdateWindow(hwnd);
        }

        let mut message = MSG::default();
        loop {
            let status = unsafe { GetMessageW(&raw mut message, None, 0, 0) }.0;
            if status == -1 {
                return Err(StabilityStimulusError::WindowsStage {
                    stage: "GetMessageW",
                    source: WindowsError::from_thread(),
                });
            }
            if status == 0 {
                break;
            }
            unsafe {
                let _ = TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }

        let timer_ticks = telemetry.timer_ticks.load(Ordering::Relaxed);
        let active_duration_ms = telemetry.active_duration_ms.load(Ordering::Relaxed);
        let effective_timer_hz_milli = if active_duration_ms == 0 {
            0
        } else {
            timer_ticks
                .saturating_mul(1_000_000)
                .saturating_div(active_duration_ms)
        };
        Ok(StabilityStimulusReport {
            width: u32::try_from(width).unwrap_or_default(),
            height: u32::try_from(height).unwrap_or_default(),
            requested_duration_ms: duration.as_millis(),
            active_duration_ms,
            timer_wakeups: telemetry.timer_wakeups.load(Ordering::Relaxed),
            timer_ticks,
            paint_calls: telemetry.paint_calls.load(Ordering::Relaxed),
            effective_timer_hz_milli,
            graceful_close: telemetry.graceful_close.load(Ordering::Relaxed),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StabilityStimulusReport {
    pub width: u32,
    pub height: u32,
    pub requested_duration_ms: u128,
    pub active_duration_ms: u64,
    pub timer_wakeups: u64,
    pub timer_ticks: u64,
    pub paint_calls: u64,
    pub effective_timer_hz_milli: u64,
    pub graceful_close: bool,
}

impl std::fmt::Display for StabilityStimulusReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(formatter, "Panzo M4 60 Hz stability stimulus")?;
        writeln!(formatter, "  Surface: {}x{}", self.width, self.height)?;
        writeln!(
            formatter,
            "  Requested duration: {} ms",
            self.requested_duration_ms
        )?;
        writeln!(
            formatter,
            "  Active duration: {} ms",
            self.active_duration_ms
        )?;
        writeln!(formatter, "  Timer wakeups: {}", self.timer_wakeups)?;
        writeln!(formatter, "  Timer ticks: {}", self.timer_ticks)?;
        writeln!(formatter, "  Paint calls: {}", self.paint_calls)?;
        writeln!(
            formatter,
            "  Effective timer rate: {}.{:03} Hz",
            self.effective_timer_hz_milli / 1_000,
            self.effective_timer_hz_milli % 1_000
        )?;
        write!(formatter, "  Graceful close: {}", self.graceful_close)
    }
}

#[derive(Default)]
struct WindowTelemetry {
    timer_wakeups: AtomicU64,
    timer_ticks: AtomicU64,
    paint_calls: AtomicU64,
    active_duration_ms: AtomicU64,
    graceful_close: AtomicBool,
}

struct WindowState {
    started: Instant,
    next_frame_at: Instant,
    duration: Duration,
    frame_index: u64,
    back_buffer: Option<BackBuffer>,
    telemetry: Arc<WindowTelemetry>,
}

impl WindowState {
    fn on_timer(&mut self, hwnd: HWND) {
        self.telemetry.timer_wakeups.fetch_add(1, Ordering::Relaxed);
        let now = Instant::now();
        if now.saturating_duration_since(self.started) >= self.duration {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return;
        }
        if now < self.next_frame_at {
            return;
        }
        self.telemetry.timer_ticks.fetch_add(1, Ordering::Relaxed);
        let client = client_rect(hwnd);
        let old_band = band_rect(client, self.frame_index);
        while self.next_frame_at <= now {
            self.next_frame_at += FRAME_INTERVAL;
            self.frame_index = self.frame_index.wrapping_add(1);
        }
        let new_band = band_rect(client, self.frame_index);
        unsafe {
            let _ = InvalidateRect(Some(hwnd), Some(&raw const old_band), false);
            let _ = InvalidateRect(Some(hwnd), Some(&raw const new_band), false);
        }
    }

    fn paint(&mut self, hwnd: HWND) {
        self.telemetry.paint_calls.fetch_add(1, Ordering::Relaxed);
        let mut paint = PAINTSTRUCT::default();
        let target = unsafe { BeginPaint(hwnd, &raw mut paint) };
        let client = client_rect(hwnd);
        let width = (client.right - client.left).max(1);
        let height = (client.bottom - client.top).max(1);
        if self
            .back_buffer
            .as_ref()
            .is_none_or(|buffer| buffer.width != width || buffer.height != height)
        {
            self.back_buffer = BackBuffer::create(target, width, height);
        }
        if let Some(buffer) = &self.back_buffer {
            buffer.render(target, paint.rcPaint, self.frame_index);
        } else {
            draw_fallback(target, client, self.frame_index);
        }
        unsafe {
            let _ = EndPaint(hwnd, &raw const paint);
        }
    }
}

struct BackBuffer {
    pattern_dc: HDC,
    pattern_bitmap: HBITMAP,
    previous_pattern_bitmap: HGDIOBJ,
    accent_brush: HBRUSH,
    width: i32,
    height: i32,
}

impl BackBuffer {
    fn create(target: HDC, width: i32, height: i32) -> Option<Self> {
        let pattern_dc = unsafe { CreateCompatibleDC(Some(target)) };
        let pattern_bitmap = unsafe { CreateCompatibleBitmap(target, width, height) };
        if pattern_dc.0.is_null() || pattern_bitmap.0.is_null() {
            unsafe {
                if !pattern_bitmap.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(pattern_bitmap.0));
                }
                if !pattern_dc.0.is_null() {
                    let _ = DeleteDC(pattern_dc);
                }
            }
            return None;
        }
        let previous_pattern_bitmap =
            unsafe { SelectObject(pattern_dc, HGDIOBJ(pattern_bitmap.0)) };
        draw_static_checkerboard(pattern_dc, width, height);
        let accent_brush = unsafe { CreateSolidBrush(rgb(255, 145, 40)) };
        if accent_brush.0.is_null() {
            unsafe {
                SelectObject(pattern_dc, previous_pattern_bitmap);
                let _ = DeleteObject(HGDIOBJ(pattern_bitmap.0));
                let _ = DeleteDC(pattern_dc);
            }
            return None;
        }
        Some(Self {
            pattern_dc,
            pattern_bitmap,
            previous_pattern_bitmap,
            accent_brush,
            width,
            height,
        })
    }

    fn render(&self, target: HDC, update: RECT, frame_index: u64) {
        let update_width = (update.right - update.left).max(0);
        let update_height = (update.bottom - update.top).max(0);
        unsafe {
            let _ = BitBlt(
                target,
                update.left,
                update.top,
                update_width,
                update_height,
                Some(self.pattern_dc),
                update.left,
                update.top,
                SRCCOPY,
            );
        }
        draw_accent_and_text(
            target,
            self.width,
            self.height,
            frame_index,
            self.accent_brush,
        );
    }
}

impl Drop for BackBuffer {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.pattern_dc, self.previous_pattern_bitmap);
            let _ = DeleteObject(HGDIOBJ(self.pattern_bitmap.0));
            let _ = DeleteObject(HGDIOBJ(self.accent_brush.0));
            let _ = DeleteDC(self.pattern_dc);
        }
    }
}

fn draw_static_checkerboard(hdc: HDC, width: i32, height: i32) {
    let dark = unsafe { CreateSolidBrush(rgb(16, 21, 35)) };
    let light = unsafe { CreateSolidBrush(rgb(45, 105, 175)) };
    let mut row = 0_i32;
    let mut top = 0_i32;
    while top < height {
        let mut column = 0_i32;
        let mut left = 0_i32;
        while left < width {
            let rect = RECT {
                left,
                top,
                right: (left + TILE_SIZE).min(width),
                bottom: (top + TILE_SIZE).min(height),
            };
            let brush = if (row + column) & 1 == 0 { dark } else { light };
            unsafe {
                FillRect(hdc, &raw const rect, brush);
            }
            left += TILE_SIZE;
            column += 1;
        }
        top += TILE_SIZE;
        row += 1;
    }
    unsafe {
        let _ = DeleteObject(HGDIOBJ(dark.0));
        let _ = DeleteObject(HGDIOBJ(light.0));
    }
}

fn draw_accent_and_text(hdc: HDC, width: i32, height: i32, frame_index: u64, accent_brush: HBRUSH) {
    let width_u64 = u64::try_from(width).unwrap_or(1);
    let band_left = i32::try_from(frame_index.wrapping_mul(19) % width_u64).unwrap_or_default();
    let band = RECT {
        left: band_left,
        top: 0,
        right: (band_left + BAND_WIDTH).min(width),
        bottom: height,
    };
    unsafe {
        FillRect(hdc, &raw const band, accent_brush);
    }

    let mut text_rect = RECT {
        left: 0,
        top: height.saturating_sub(100),
        right: width,
        bottom: height.saturating_sub(20),
    };
    let mut text: Vec<u16> = "Panzo M4 60 Hz moving-band stimulus"
        .encode_utf16()
        .collect();
    unsafe {
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, rgb(245, 247, 250));
        DrawTextW(
            hdc,
            &mut text,
            &raw mut text_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
    }
}

fn band_rect(client: RECT, frame_index: u64) -> RECT {
    let width = (client.right - client.left).max(1);
    let width_u64 = u64::try_from(width).unwrap_or(1);
    let left = i32::try_from(frame_index.wrapping_mul(19) % width_u64).unwrap_or_default();
    RECT {
        left,
        top: client.top,
        right: (left + BAND_WIDTH).min(client.right),
        bottom: client.bottom,
    }
}

fn draw_fallback(hdc: HDC, client: RECT, frame_index: u64) {
    let background = unsafe { CreateSolidBrush(rgb(16, 21, 35)) };
    let accent = unsafe { CreateSolidBrush(rgb(255, 145, 40)) };
    unsafe {
        FillRect(hdc, &raw const client, background);
    }
    draw_accent_and_text(
        hdc,
        (client.right - client.left).max(1),
        (client.bottom - client.top).max(1),
        frame_index,
        accent,
    );
    unsafe {
        let _ = DeleteObject(HGDIOBJ(background.0));
        let _ = DeleteObject(HGDIOBJ(accent.0));
    }
}

struct TimerResolution;

impl TimerResolution {
    fn acquire() -> Result<Self, StabilityStimulusError> {
        let result = unsafe { timeBeginPeriod(TIMER_RESOLUTION_MS) };
        if result != TIMERR_NOERROR {
            return Err(StabilityStimulusError::TimerResolution(result));
        }
        Ok(Self)
    }
}

impl Drop for TimerResolution {
    fn drop(&mut self) {
        unsafe {
            timeEndPeriod(TIMER_RESOLUTION_MS);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TIMER if wparam.0 == TIMER_ID => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.on_timer(hwnd);
            }
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
            unsafe {
                let _ = DestroyWindow(hwnd);
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
        WM_ERASEBKGND => LRESULT(1),
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some(state) = unsafe { state_mut(hwnd) } {
                state.telemetry.active_duration_ms.store(
                    u64::try_from(state.started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    Ordering::Relaxed,
                );
                state
                    .telemetry
                    .graceful_close
                    .store(true, Ordering::Relaxed);
            }
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
                    drop(Box::from_raw(pointer as *mut WindowState));
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

fn client_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &raw mut rect) };
    rect
}

const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(red as u32 | (green as u32) << 8 | (blue as u32) << 16)
}

fn win<T>(
    stage: &'static str,
    result: windows::core::Result<T>,
) -> Result<T, StabilityStimulusError> {
    result.map_err(|source| StabilityStimulusError::WindowsStage { stage, source })
}

#[derive(Debug, Error)]
pub enum StabilityStimulusError {
    #[error("stability stimulus duration must be at least one second")]
    InvalidDuration,
    #[error("primary monitor geometry is invalid")]
    InvalidMonitorGeometry,
    #[error("timeBeginPeriod(1) failed with result {0}")]
    TimerResolution(u32),
    #[error(transparent)]
    Windows(#[from] windows::core::Error),
    #[error("{stage} failed: {source}")]
    WindowsStage {
        stage: &'static str,
        source: WindowsError,
    },
}
