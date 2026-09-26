use crossbeam_queue::ArrayQueue;
use panzo_core::{MouseAction, MouseButton, PhysicalPoint, RawInputKind, StampedInputEvent};
use std::mem::size_of;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use thiserror::Error;
use windows::Win32::Foundation::{CloseHandle, LPARAM, LRESULT, POINT, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::Performance::QueryPerformanceCounter;
use windows::Win32::System::Threading::{
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, GetCurrentThreadId, INFINITE,
    SetWaitableTimerEx, TIMER_ALL_ACCESS, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSORINFO, CallNextHookEx, DispatchMessageW, GetCursorInfo, GetCursorPos,
    GetMessageW, HC_ACTION, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_MOUSE_LL, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP,
};
use windows::core::PCWSTR;

pub const INPUT_QUEUE_CAPACITY: usize = 65_536;
pub const CURSOR_SAMPLE_RATE_HZ: u32 = 120;
const CURSOR_SAMPLE_INTERVAL_100NS: i64 = 10_000_000 / 120_i64;

struct SharedInputState {
    queue: ArrayQueue<StampedInputEvent>,
    active: AtomicBool,
    overflowed: AtomicBool,
    next_sequence: AtomicU64,
    sample_attempts: AtomicU64,
    sample_read_failures: AtomicU64,
}

impl SharedInputState {
    fn new() -> Self {
        Self {
            queue: ArrayQueue::new(INPUT_QUEUE_CAPACITY),
            active: AtomicBool::new(false),
            overflowed: AtomicBool::new(false),
            next_sequence: AtomicU64::new(1),
            sample_attempts: AtomicU64::new(0),
            sample_read_failures: AtomicU64::new(0),
        }
    }

    fn push(&self, qpc: i64, point: PhysicalPoint, kind: RawInputKind) {
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        if self
            .queue
            .push(StampedInputEvent {
                sequence,
                qpc,
                desktop_point: point,
                kind,
            })
            .is_err()
        {
            self.overflowed.store(true, Ordering::Release);
        }
    }
}

fn shared_state() -> &'static SharedInputState {
    static STATE: OnceLock<SharedInputState> = OnceLock::new();
    STATE.get_or_init(SharedInputState::new)
}

pub struct InputCaptureService {
    sampler_thread: Option<JoinHandle<Result<(), InputCaptureError>>>,
    hook_thread: Option<JoinHandle<Result<(), InputCaptureError>>>,
    hook_thread_id: u32,
    stopped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputCaptureStats {
    pub queued_events: usize,
    pub overflowed: bool,
    pub sample_attempts: u64,
    pub sample_read_failures: u64,
}

impl InputCaptureService {
    pub fn start() -> Result<Self, InputCaptureError> {
        let state = shared_state();
        state
            .active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| InputCaptureError::AlreadyRunning)?;
        while state.queue.pop().is_some() {}
        state.overflowed.store(false, Ordering::Release);
        state.next_sequence.store(1, Ordering::Release);
        state.sample_attempts.store(0, Ordering::Release);
        state.sample_read_failures.store(0, Ordering::Release);

        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let hook_thread = thread::Builder::new()
            .name("panzo-mouse-hook".into())
            .spawn(move || run_hook_thread(&ready_sender))
            .map_err(InputCaptureError::SpawnThread)?;

        let hook_thread_id = match ready_receiver.recv() {
            Ok(Ok(thread_id)) => thread_id,
            Ok(Err(error)) => {
                state.active.store(false, Ordering::Release);
                let _ = hook_thread.join();
                return Err(error);
            }
            Err(error) => {
                state.active.store(false, Ordering::Release);
                let _ = hook_thread.join();
                return Err(InputCaptureError::HookStartupChannel(error));
            }
        };

        let sampler_thread = match thread::Builder::new()
            .name("panzo-cursor-sampler".into())
            .spawn(run_sampler_thread)
        {
            Ok(handle) => handle,
            Err(error) => {
                state.active.store(false, Ordering::Release);
                unsafe { PostThreadMessageW(hook_thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) }.ok();
                let _ = hook_thread.join();
                return Err(InputCaptureError::SpawnThread(error));
            }
        };

        Ok(Self {
            sampler_thread: Some(sampler_thread),
            hook_thread: Some(hook_thread),
            hook_thread_id,
            stopped: false,
        })
    }

    pub fn drain(&self) -> Vec<StampedInputEvent> {
        let state = shared_state();
        let mut events = Vec::with_capacity(state.queue.len());
        while let Some(event) = state.queue.pop() {
            events.push(event);
        }
        events
    }

    pub fn queued_len(&self) -> usize {
        shared_state().queue.len()
    }

    pub fn overflowed(&self) -> bool {
        shared_state().overflowed.load(Ordering::Acquire)
    }

    pub fn stats(&self) -> InputCaptureStats {
        let state = shared_state();
        InputCaptureStats {
            queued_events: state.queue.len(),
            overflowed: state.overflowed.load(Ordering::Acquire),
            sample_attempts: state.sample_attempts.load(Ordering::Acquire),
            sample_read_failures: state.sample_read_failures.load(Ordering::Acquire),
        }
    }

    pub fn stop(&mut self) -> Result<(), InputCaptureError> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        shared_state().active.store(false, Ordering::Release);
        unsafe { PostThreadMessageW(self.hook_thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) }
            .map_err(InputCaptureError::Windows)?;

        join_capture_thread(self.sampler_thread.take(), "cursor sampler")?;
        join_capture_thread(self.hook_thread.take(), "mouse hook")?;
        Ok(())
    }
}

impl Drop for InputCaptureService {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn join_capture_thread(
    handle: Option<JoinHandle<Result<(), InputCaptureError>>>,
    name: &'static str,
) -> Result<(), InputCaptureError> {
    let Some(handle) = handle else {
        return Ok(());
    };
    handle
        .join()
        .map_err(|_| InputCaptureError::ThreadPanicked(name))??;
    Ok(())
}

fn run_sampler_thread() -> Result<(), InputCaptureError> {
    let timer = unsafe {
        CreateWaitableTimerExW(
            None,
            PCWSTR::null(),
            CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
            TIMER_ALL_ACCESS.0,
        )
    }
    .map_err(InputCaptureError::Windows)?;
    let _timer_guard = HandleGuard(timer);
    let state = shared_state();

    while state.active.load(Ordering::Acquire) {
        let mut point = POINT::default();
        let mut cursor_info = CURSORINFO {
            cbSize: u32::try_from(size_of::<CURSORINFO>()).expect("CURSORINFO size fits in u32"),
            ..Default::default()
        };
        state.sample_attempts.fetch_add(1, Ordering::Relaxed);
        let position_result = unsafe { GetCursorPos(&raw mut point) };
        let info_result = unsafe { GetCursorInfo(&raw mut cursor_info) };
        let sample = if position_result.is_ok() {
            Some((
                point,
                info_result.is_ok() && cursor_info.flags.0 & CURSOR_SHOWING.0 != 0,
            ))
        } else if info_result.is_ok() {
            Some((
                cursor_info.ptScreenPos,
                cursor_info.flags.0 & CURSOR_SHOWING.0 != 0,
            ))
        } else {
            None
        };
        if let Some((sample_point, visible)) = sample
            && let Some(qpc) = query_qpc()
        {
            state.push(
                qpc,
                PhysicalPoint {
                    x: sample_point.x,
                    y: sample_point.y,
                },
                RawInputKind::Cursor { visible },
            );
        } else if sample.is_none() {
            state.sample_read_failures.fetch_add(1, Ordering::Relaxed);
        }

        let due_time = -CURSOR_SAMPLE_INTERVAL_100NS;
        unsafe { SetWaitableTimerEx(timer, &raw const due_time, 0, None, None, None, 0) }
            .map_err(InputCaptureError::Windows)?;
        if unsafe { WaitForSingleObject(timer, INFINITE) } != WAIT_OBJECT_0 {
            return Err(InputCaptureError::WaitTimer);
        }
    }
    Ok(())
}

fn run_hook_thread(
    ready: &mpsc::SyncSender<Result<u32, InputCaptureError>>,
) -> Result<(), InputCaptureError> {
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut message = MSG::default();
    let _ = unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_NOREMOVE) };
    let hook = match unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0) } {
        Ok(hook) => hook,
        Err(error) => {
            let startup_error = InputCaptureError::Windows(error);
            ready
                .send(Err(startup_error))
                .map_err(|_| InputCaptureError::HookReadyReceiverGone)?;
            return Ok(());
        }
    };
    ready
        .send(Ok(thread_id))
        .map_err(|_| InputCaptureError::HookReadyReceiverGone)?;

    loop {
        let result = unsafe { GetMessageW(&raw mut message, None, 0, 0) };
        if result.0 == -1 {
            unsafe { UnhookWindowsHookEx(hook) }.ok();
            return Err(InputCaptureError::GetMessage);
        }
        if result.0 == 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    unsafe { UnhookWindowsHookEx(hook) }.map_err(InputCaptureError::Windows)
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == i32::try_from(HC_ACTION).expect("HC_ACTION fits in i32") {
        let kind = match u32::try_from(wparam.0).ok() {
            Some(WM_LBUTTONDOWN) => Some((MouseButton::Left, MouseAction::Down)),
            Some(WM_LBUTTONUP) => Some((MouseButton::Left, MouseAction::Up)),
            Some(WM_RBUTTONDOWN) => Some((MouseButton::Right, MouseAction::Down)),
            Some(WM_RBUTTONUP) => Some((MouseButton::Right, MouseAction::Up)),
            Some(WM_MBUTTONDOWN) => Some((MouseButton::Middle, MouseAction::Down)),
            Some(WM_MBUTTONUP) => Some((MouseButton::Middle, MouseAction::Up)),
            _ => None,
        };
        if let Some((button, action)) = kind
            && let Some(qpc) = query_qpc()
        {
            let hook_data = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
            shared_state().push(
                qpc,
                PhysicalPoint {
                    x: hook_data.pt.x,
                    y: hook_data.pt.y,
                },
                RawInputKind::Button { button, action },
            );
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn query_qpc() -> Option<i64> {
    let mut qpc = 0_i64;
    unsafe { QueryPerformanceCounter(&raw mut qpc) }
        .ok()
        .map(|()| qpc)
}

struct HandleGuard(windows::Win32::Foundation::HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) }.ok();
    }
}

#[derive(Debug, Error)]
pub enum InputCaptureError {
    #[error("input capture is already running")]
    AlreadyRunning,
    #[error("could not spawn input capture thread: {0}")]
    SpawnThread(std::io::Error),
    #[error("input capture Windows call failed: {0}")]
    Windows(windows::core::Error),
    #[error("mouse hook startup channel failed: {0}")]
    HookStartupChannel(mpsc::RecvError),
    #[error("mouse hook startup receiver closed")]
    HookReadyReceiverGone,
    #[error("GetMessageW failed")]
    GetMessage,
    #[error("high-resolution timer wait failed")]
    WaitTimer,
    #[error("{0} thread panicked")]
    ThreadPanicked(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn samples_cursor_and_stops_cleanly() {
        let mut service = InputCaptureService::start().unwrap();
        thread::sleep(Duration::from_millis(100));
        service.stop().unwrap();
        let events = service.drain();
        let stats = service.stats();
        assert!(stats.sample_attempts >= 5);
        assert!(!service.overflowed());
        assert!(
            events
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );
    }
}
