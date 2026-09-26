use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};

pub struct WinRtApartment {
    initialized: bool,
}

impl WinRtApartment {
    pub fn multi_threaded() -> windows::core::Result<Self> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
        Ok(Self { initialized: true })
    }
}

impl Drop for WinRtApartment {
    fn drop(&mut self) {
        if self.initialized {
            unsafe { RoUninitialize() };
        }
    }
}
