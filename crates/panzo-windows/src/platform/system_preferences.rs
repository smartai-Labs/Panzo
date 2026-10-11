//! Cached Windows accessibility preferences, refreshed on system setting changes.
use std::sync::{
    Once,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use windows::{
    Win32::{
        System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            WindowsAndMessaging::*,
        },
    },
    core::w,
};
static INITIALIZED: Once = Once::new();
static TEXT: AtomicU32 = AtomicU32::new(100);
static CONTRAST: AtomicBool = AtomicBool::new(false);
static MOTION: AtomicBool = AtomicBool::new(true);
fn ensure() {
    INITIALIZED.call_once(|| {
        refresh();
    });
}
pub(crate) fn text_scale() -> u32 {
    ensure();
    TEXT.load(Ordering::Relaxed)
}
pub(crate) fn high_contrast() -> bool {
    ensure();
    CONTRAST.load(Ordering::Relaxed)
}
pub(crate) fn animations() -> bool {
    ensure();
    MOTION.load(Ordering::Relaxed) && !high_contrast() && super::ui_preferences::animations()
}
pub(crate) fn refresh() -> bool {
    let mut contrast = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    let mut animation = windows::core::BOOL(1);
    let mut text = 100u32;
    let mut size = 4u32;
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            Some((&mut contrast as *mut HIGHCONTRASTW).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&mut animation as *mut windows::core::BOOL).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let _ = RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Accessibility"),
            w!("TextScaleFactor"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut text as *mut u32).cast()),
            Some(&mut size),
        );
    }
    let contrast = contrast.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0;
    let previous = (
        TEXT.swap(text.clamp(100, 225), Ordering::Relaxed),
        CONTRAST.swap(contrast, Ordering::Relaxed),
        MOTION.swap(animation.as_bool(), Ordering::Relaxed),
    );
    previous != (text.clamp(100, 225), contrast, animation.as_bool())
}
