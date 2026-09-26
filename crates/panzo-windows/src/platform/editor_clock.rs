//! Coalesced source-PTS-aware clock. No waits or decoder calls on the window thread.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::Threading::{
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateEventW, CreateWaitableTimerExW, SetEvent,
    SetWaitableTimer, TIMER_ALL_ACCESS, WaitForMultipleObjects, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

static NEXT_CLOCK: AtomicUsize = AtomicUsize::new(1);
// Numeric storage is private. The event is OS-thread-safe and remains owned by
// the shared State until the waiter and window both release their Arc.
struct WakeEvent(usize);
impl WakeEvent {
    fn handle(&self) -> HANDLE {
        HANDLE(self.0 as *mut _)
    }
    fn signal(&self) {
        let _ = unsafe { SetEvent(self.handle()) };
    }
}
impl Drop for WakeEvent {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.handle()) };
    }
}
struct State {
    stopped: AtomicBool,
    queued: AtomicBool,
    active: AtomicBool,
    next_frame: Mutex<Option<Instant>>,
    wake: WakeEvent,
    id: usize,
}
pub struct EditorClock(Arc<State>);
struct Timer(Option<HANDLE>);
impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(handle) = self.0 {
            let _ = unsafe { CloseHandle(handle) };
        }
    }
}
impl EditorClock {
    pub fn start(hwnd: HWND, message: u32) -> std::io::Result<Self> {
        let event = unsafe { CreateEventW(None, false, false, None) }
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let state = Arc::new(State {
            stopped: AtomicBool::new(false),
            queued: AtomicBool::new(false),
            active: AtomicBool::new(true),
            next_frame: Mutex::new(None),
            wake: WakeEvent(event.0 as usize),
            id: NEXT_CLOCK.fetch_add(1, Ordering::Relaxed),
        });
        let shared = Arc::clone(&state);
        let raw = hwnd.0 as usize;
        std::thread::Builder::new()
            .name("panzo-frame-clock".into())
            .spawn(move || {
                let timer = Timer(
                    unsafe {
                        CreateWaitableTimerExW(
                            None,
                            None,
                            CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                            TIMER_ALL_ACCESS.0,
                        )
                    }
                    .ok(),
                );
                while !shared.stopped.load(Ordering::Acquire) {
                    let period = if shared.active.load(Ordering::Acquire) {
                        Duration::from_nanos(16_666_667)
                    } else {
                        Duration::from_millis(50)
                    };
                    let requested = *shared
                        .next_frame
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let deadline = requested.unwrap_or_else(|| Instant::now() + period);
                    let delay = deadline.saturating_duration_since(Instant::now());
                    let interrupted = if let Some(handle) = timer.0 {
                        let due =
                            -i64::try_from((delay.as_nanos() / 100).max(1)).unwrap_or(166_667);
                        if unsafe { SetWaitableTimer(handle, &raw const due, 0, None, None, false) }
                            .is_ok()
                        {
                            (unsafe {
                                WaitForMultipleObjects(&[shared.wake.handle(), handle], false, 100)
                            }) == WAIT_OBJECT_0
                        } else {
                            (unsafe {
                                WaitForSingleObject(
                                    shared.wake.handle(),
                                    delay.as_millis().clamp(1, 50) as u32,
                                )
                            }) == WAIT_OBJECT_0
                        }
                    } else {
                        (unsafe {
                            WaitForSingleObject(
                                shared.wake.handle(),
                                delay.as_millis().clamp(1, 50) as u32,
                            )
                        }) == WAIT_OBJECT_0
                    };
                    if interrupted {
                        continue;
                    }
                    if shared.stopped.load(Ordering::Acquire) {
                        break;
                    }
                    {
                        let mut next = shared
                            .next_frame
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if *next == requested {
                            *next = None;
                        }
                    }
                    if !shared.queued.swap(true, Ordering::AcqRel)
                        && unsafe {
                            PostMessageW(
                                Some(HWND(raw as *mut _)),
                                message,
                                WPARAM(shared.id),
                                LPARAM(0),
                            )
                        }
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self(state))
    }
    pub fn accept(&self, id: usize) -> bool {
        if id != self.0.id {
            return false;
        }
        self.0.queued.store(false, Ordering::Release);
        true
    }
    pub fn set_active(&self, active: bool) {
        if self.0.active.swap(active, Ordering::AcqRel) != active {
            self.0.wake.signal();
        }
    }
    pub fn schedule_video_frame(&self, delay: Duration) {
        // A decoded VFR recording has 10/20 ms intervals on a 100 Hz source.
        // Re-sampling it on a fixed 60 Hz timer creates 33 ms duplicate-frame holds.
        // Wake at its next actual PTS instead; at most one pending window message.
        *self
            .0
            .next_frame
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(Instant::now() + delay.clamp(Duration::from_millis(1), Duration::from_millis(50)));
        self.0.wake.signal();
    }
}
impl Drop for EditorClock {
    fn drop(&mut self) {
        self.0.stopped.store(true, Ordering::Release);
        self.0.wake.signal();
    }
}
