use panzo_core::PhysicalRect;
use std::ffi::c_void;
use thiserror::Error;
use windows::Graphics::Capture::GraphicsCaptureItem;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetClassNameW, GetShellWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, factory};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowCaptureTarget {
    handle: usize,
    pub process_id: u32,
    pub title: String,
    pub desktop_rect: PhysicalRect,
}

impl WindowCaptureTarget {
    pub fn source_id(&self) -> String {
        format!("window-{}-{:X}", self.process_id, self.handle)
    }

    pub const fn raw_handle(&self) -> usize {
        self.handle
    }

    pub(crate) fn hwnd(&self) -> HWND {
        HWND(self.handle as *mut c_void)
    }

    pub fn current_desktop_rect(&self) -> Result<PhysicalRect, WindowCaptureError> {
        validate_identity(self)?;
        window_rect(self.hwnd()).ok_or(WindowCaptureError::InvalidWindowGeometry)
    }
}

pub fn enumerate_capturable_windows() -> Result<Vec<WindowCaptureTarget>, WindowCaptureError> {
    let mut targets: Vec<WindowCaptureTarget> = Vec::new();
    unsafe {
        EnumWindows(
            Some(enumerate_window),
            LPARAM(std::ptr::from_mut(&mut targets) as isize),
        )
    }
    .map_err(WindowCaptureError::EnumerateWindows)?;
    targets.sort_by_cached_key(|target| target.title.to_lowercase());
    Ok(targets)
}

pub fn find_capturable_window(
    title_substring: &str,
) -> Result<WindowCaptureTarget, WindowCaptureError> {
    let needle = title_substring.trim().to_lowercase();
    if needle.is_empty() {
        return Err(WindowCaptureError::EmptyTitleSearch);
    }
    let mut matches = enumerate_capturable_windows()?
        .into_iter()
        .filter(|target| target.title.to_lowercase().contains(&needle));
    let first = matches
        .next()
        .ok_or_else(|| WindowCaptureError::WindowNotFound(title_substring.into()))?;
    if matches.next().is_some() {
        return Err(WindowCaptureError::AmbiguousWindowTitle(
            title_substring.into(),
        ));
    }
    Ok(first)
}

pub(crate) fn window_capture_item(
    target: &WindowCaptureTarget,
) -> Result<GraphicsCaptureItem, WindowCaptureError> {
    validate_identity(target)?;
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .map_err(WindowCaptureError::CaptureInterop)?;
    unsafe { interop.CreateForWindow::<GraphicsCaptureItem>(target.hwnd()) }.map_err(|source| {
        WindowCaptureError::CreateCaptureItem {
            title: target.title.clone(),
            source,
        }
    })
}

unsafe extern "system" fn enumerate_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let targets = unsafe { &mut *(lparam.0 as *mut Vec<WindowCaptureTarget>) };
    if let Some(target) = inspect_window(hwnd) {
        targets.push(target);
    }
    true.into()
}

fn inspect_window(hwnd: HWND) -> Option<WindowCaptureTarget> {
    if hwnd == unsafe { GetShellWindow() }
        || window_class(hwnd).is_some_and(|class_name| {
            matches!(
                class_name.as_str(),
                "PanzoRecorderWindow" | "PanzoEditorWorkbenchWindow"
            )
        })
        || (unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) }.cast_unsigned()
            & WS_EX_TOOLWINDOW.0 as usize)
            != 0
    {
        return None;
    }
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() || unsafe { IsIconic(hwnd) }.as_bool() {
        return None;
    }
    let mut cloaked = 0_u32;
    if unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            std::ptr::from_mut(&mut cloaked).cast(),
            size_of::<u32>() as u32,
        )
    }
    .is_ok()
        && cloaked != 0
    {
        return None;
    }
    let title = window_title(hwnd)?;
    let desktop_rect = window_rect(hwnd)?;
    let mut process_id = 0_u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&raw mut process_id));
    }
    if process_id == 0 || process_id == std::process::id() {
        return None;
    }
    Some(WindowCaptureTarget {
        handle: hwnd.0 as usize,
        process_id,
        title,
        desktop_rect,
    })
}

fn window_class(hwnd: HWND) -> Option<String> {
    let mut buffer = [0_u16; 256];
    let copied = unsafe { GetClassNameW(hwnd, &mut buffer) };
    if copied <= 0 {
        return None;
    }
    Some(String::from_utf16_lossy(
        &buffer[..usize::try_from(copied).ok()?],
    ))
}

fn window_title(hwnd: HWND) -> Option<String> {
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return None;
    }
    let mut buffer = vec![0_u16; usize::try_from(length).ok()?.saturating_add(1)];
    let copied = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    if copied <= 0 {
        return None;
    }
    let title = String::from_utf16_lossy(&buffer[..usize::try_from(copied).ok()?]);
    (!title.trim().is_empty()).then_some(title)
}

fn window_rect(hwnd: HWND) -> Option<PhysicalRect> {
    let mut rect = RECT::default();
    if unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            std::ptr::from_mut(&mut rect).cast(),
            size_of::<RECT>() as u32,
        )
    }
    .is_err()
        && unsafe { GetWindowRect(hwnd, &raw mut rect) }.is_err()
    {
        return None;
    }
    let width = u32::try_from(rect.right.saturating_sub(rect.left)).ok()?;
    let height = u32::try_from(rect.bottom.saturating_sub(rect.top)).ok()?;
    (width > 1 && height > 1).then_some(PhysicalRect {
        x: rect.left,
        y: rect.top,
        width,
        height,
    })
}

fn validate_identity(target: &WindowCaptureTarget) -> Result<(), WindowCaptureError> {
    let hwnd = target.hwnd();
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err(WindowCaptureError::WindowClosed(target.title.clone()));
    }
    let mut process_id = 0_u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&raw mut process_id));
    }
    if process_id != target.process_id {
        return Err(WindowCaptureError::WindowClosed(target.title.clone()));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum WindowCaptureError {
    #[error("could not enumerate top-level windows: {0}")]
    EnumerateWindows(windows::core::Error),
    #[error("window title search must not be empty")]
    EmptyTitleSearch,
    #[error("no capturable window title contains '{0}'")]
    WindowNotFound(String),
    #[error("more than one capturable window title contains '{0}'")]
    AmbiguousWindowTitle(String),
    #[error("the selected window is no longer available: {0}")]
    WindowClosed(String),
    #[error("the selected window has invalid desktop geometry")]
    InvalidWindowGeometry,
    #[error("could not obtain the GraphicsCaptureItem interop factory: {0}")]
    CaptureInterop(windows::core::Error),
    #[error("could not create a capture item for window '{title}': {source}")]
    CreateCaptureItem {
        title: String,
        #[source]
        source: windows::core::Error,
    },
}
