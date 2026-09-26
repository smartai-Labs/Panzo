//! Scoped timer accuracy while media is active; release when paused or closed.
//! <https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timebeginperiod>
pub struct TimerResolution;
impl TimerResolution {
    pub fn acquire() -> Option<Self> {
        (unsafe { windows::Win32::Media::timeBeginPeriod(1) } == 0).then_some(Self)
    }
}
impl Drop for TimerResolution {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::Media::timeEndPeriod(1);
        }
    }
}
