//! Application branding shared by top-level windows and the notification area.
use std::sync::OnceLock;
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::{
        HiDpi::GetSystemMetricsForDpi,
        WindowsAndMessaging::{
            CreateIconFromResourceEx, GWL_STYLE, GetWindowLongW, HICON, ICON_BIG, ICON_SMALL,
            IDI_APPLICATION, LR_DEFAULTCOLOR, LoadIconW, SM_CXICON, SM_CXSMICON, SendMessageW,
            WM_SETICON, WS_CHILD,
        },
    },
};

const IMAGES: &[(i32, &[u8])] = &[
    (16, include_bytes!("../../assets/panzo-16.png")),
    (20, include_bytes!("../../assets/panzo-20.png")),
    (24, include_bytes!("../../assets/panzo-24.png")),
    (32, include_bytes!("../../assets/panzo-32.png")),
    (40, include_bytes!("../../assets/panzo-40.png")),
    (48, include_bytes!("../../assets/panzo-48.png")),
    (64, include_bytes!("../../assets/panzo-64.png")),
    (96, include_bytes!("../../assets/panzo-96.png")),
    (128, include_bytes!("../../assets/panzo-128.png")),
    (256, include_bytes!("../../assets/panzo-256.png")),
];
// These ten shared handles live until process exit. Never destroy a handle still
// referenced by another window or by Explorer's notification area.
static ICONS: [OnceLock<usize>; IMAGES.len()] = [const { OnceLock::new() }; IMAGES.len()];

fn icon(pixels: i32) -> HICON {
    let index = IMAGES
        .iter()
        .position(|(size, _)| *size >= pixels)
        .unwrap_or(IMAGES.len() - 1);
    let (size, png) = IMAGES[index];
    let handle = *ICONS[index].get_or_init(|| unsafe {
        CreateIconFromResourceEx(png, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR)
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default()
            .0 as usize
    });
    HICON(handle as *mut std::ffi::c_void)
}

pub(crate) fn small(dpi: u32) -> HICON {
    icon(unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) }.max(16))
}

pub(crate) fn apply(hwnd: HWND) {
    // Embedded workspaces inherit the root's taskbar entry.
    if unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32 & WS_CHILD.0 != 0 {
        return;
    }
    let dpi = super::editor_ui::window_dpi(hwnd);
    let large = icon(unsafe { GetSystemMetricsForDpi(SM_CXICON, dpi) }.max(32));
    unsafe {
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(LPARAM(small(dpi).0 as isize)),
        );
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(LPARAM(large.0 as isize)),
        );
    }
}
