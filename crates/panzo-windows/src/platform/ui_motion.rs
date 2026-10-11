//! One UI-thread scheduler; transitions only invalidate active controls and leave no idle timer.
use std::{
    cell::RefCell,
    collections::HashMap,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{IsWindow, IsWindowVisible, KillTimer, SetTimer},
};
const HOVER: Duration = Duration::from_millis(100);
struct Transition {
    from: f64,
    to: f64,
    start: Instant,
}
impl Transition {
    fn value(&self, now: Instant) -> f64 {
        let t = (now.duration_since(self.start).as_secs_f64() / HOVER.as_secs_f64()).clamp(0., 1.);
        let eased = 1. - (1. - t).powi(3);
        self.from + (self.to - self.from) * eased
    }
}
#[derive(Default)]
struct Scheduler {
    timer: usize,
    active: HashMap<isize, Transition>,
}
thread_local! {static STATE:RefCell<Scheduler>=RefCell::new(Scheduler::default());}
pub(crate) fn hover(h: HWND, on: bool) {
    if !super::system_preferences::animations() {
        forget(h);
        return;
    }
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let now = Instant::now();
        let to = f64::from(on);
        let from = s
            .active
            .get(&(h.0 as isize))
            .map_or(1. - to, |v| v.value(now));
        s.active.insert(
            h.0 as isize,
            Transition {
                from,
                to,
                start: now,
            },
        );
        if s.timer == 0 {
            s.timer = unsafe { SetTimer(None, 0, 16, Some(tick)) };
        }
    });
}
pub(crate) fn amount(h: HWND, on: bool) -> f64 {
    if !super::system_preferences::animations() {
        return f64::from(on);
    }
    STATE.with(|s| {
        s.borrow()
            .active
            .get(&(h.0 as isize))
            .map_or(f64::from(on), |v| v.value(Instant::now()))
    })
}
pub(crate) fn forget(h: HWND) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.active.remove(&(h.0 as isize));
        if s.active.is_empty() && s.timer != 0 {
            unsafe {
                let _ = KillTimer(None, s.timer);
            }
            s.timer = 0;
        }
    });
}
pub(crate) fn blend(
    a: windows::Win32::Foundation::COLORREF,
    b: windows::Win32::Foundation::COLORREF,
    t: f64,
) -> windows::Win32::Foundation::COLORREF {
    let channel = |shift| {
        let a = ((a.0 >> shift) & 255u32) as f64;
        let b = ((b.0 >> shift) & 255u32) as f64;
        (a + (b - a) * t).round() as u8
    };
    super::editor_ui::rgb(channel(0), channel(8), channel(16))
}
unsafe extern "system" fn tick(_: HWND, _: u32, _: usize, _: u32) {
    let handles = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let now = Instant::now();
        let handles = s.active.keys().copied().collect::<Vec<_>>();
        s.active.retain(|id, v| {
            let h = HWND(*id as *mut _);
            unsafe { IsWindow(Some(h)) }.as_bool()
                && unsafe { IsWindowVisible(h) }.as_bool()
                && now.duration_since(v.start) < HOVER
                && super::system_preferences::animations()
        });
        if s.active.is_empty() {
            unsafe {
                let _ = KillTimer(None, s.timer);
            }
            s.timer = 0;
        }
        handles
    });
    for id in handles {
        let h = HWND(id as *mut _);
        if unsafe { IsWindow(Some(h)) }.as_bool() {
            super::desktop_ui::invalidate(h);
        }
    }
}
