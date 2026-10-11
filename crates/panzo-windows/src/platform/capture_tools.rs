//! Screenshot selection and lightweight image/desktop pin windows.
// Raster sizes are bounded by MAX_PIXELS; signed pixel coordinates are clipped before indexing.
// Win32 packs signed coordinates into unsigned message words.
#![allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
#[path = "image_text.rs"]
pub(crate) mod image_text;
use super::editor_ui::{self, TextStyle};
use panzo_core::PhysicalRect;
use std::io::Write;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};
use windows::Win32::Foundation::{
    COLORREF, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CAPTUREBLT, CreateCompatibleDC,
    CreateDIBSection, CreateSolidBrush, DIB_RGB_COLORS, DT_LEFT, DT_NOPREFIX, DT_WORDBREAK,
    DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect, GdiFlush, GetDC, HALFTONE,
    HBITMAP, HDC, HGDIOBJ, InvalidateRect, PAINTSTRUCT, ReleaseDC, SRCCOPY, SelectObject,
    SetBkMode, SetStretchBltMode, SetTextColor, StretchDIBits, TRANSPARENT,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_SHIFT,
};
#[cfg(test)]
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
    GetCursorPos, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, GetWindowTextW, HMENU,
    IDC_ARROW, IDC_CROSS, IsWindowVisible, LWA_ALPHA, LoadCursorW, MB_ICONERROR, MB_OK, MINMAXINFO,
    PostMessageW, RegisterClassW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW,
    SetWindowPos, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CANCELMODE,
    WM_CAPTURECHANGED, WM_COMMAND, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT,
    WM_RBUTTONUP, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_BORDER, WS_CHILD, WS_CLIPCHILDREN,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_TABSTOP,
    WS_VISIBLE,
};
use windows::core::{PCWSTR, w};
pub(crate) const WM_CAPTURE_COMPLETE: u32 = WM_APP + 72;
const MAX_PIXELS: u64 = 64_000_000;
thread_local! {
    static OUTCOME:RefCell<Option<CaptureOutcome>>=const{RefCell::new(None)};
    static PINS:RefCell<Vec<HWND>>=const{RefCell::new(Vec::new())};
    static PICTURES:RefCell<Vec<HWND>>=const{RefCell::new(Vec::new())};
    static SELECTING:RefCell<bool>=const{RefCell::new(false)};
}
#[derive(Clone, Copy)]
pub(crate) enum CaptureIntent {
    Ocr,
    Scroll,
    Screenshot,
    Region,
}
pub(crate) enum CaptureOutcome {
    Ocr(Raster),
    Scroll(PhysicalRect),
    Image(Raster),
    Pin(Raster),
    Project(PathBuf),
    Region(PhysicalRect),
    Cancelled,
}
pub(crate) fn take_outcome() -> Option<CaptureOutcome> {
    OUTCOME.with(|v| v.borrow_mut().take())
}
#[derive(Clone)]
pub(crate) struct Raster {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}
impl Raster {
    fn bytes(w: u32, h: u32) -> Result<usize, String> {
        let count = u64::from(w) * u64::from(h);
        if count == 0 || count > MAX_PIXELS {
            return Err("图片尺寸超出可处理范围".into());
        }
        usize::try_from(count * 4).map_err(|e| e.to_string())
    }
    pub(crate) fn load(path: &Path) -> Result<Self, String> {
        let mut reader = image::ImageReader::open(path).map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(MAX_PIXELS * 4);
        reader.limits(limits);
        let mut image = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
        Self::bytes(image.width(), image.height())?;
        for p in image.pixels_mut() {
            p.0.swap(0, 2);
        }
        Ok(Self {
            width: image.width(),
            height: image.height(),
            pixels: image.into_raw(),
        })
    }
    pub(crate) fn crop(&self, r: PhysicalRect) -> Result<Self, String> {
        if r.x < 0
            || r.y < 0
            || i64::from(r.x) + i64::from(r.width) > i64::from(self.width)
            || i64::from(r.y) + i64::from(r.height) > i64::from(self.height)
        {
            return Err("选区超出屏幕边界".into());
        }
        let mut pixels = vec![0; Self::bytes(r.width, r.height)?];
        let row = r.width as usize * 4;
        for y in 0..r.height as usize {
            let start = ((r.y as usize + y) * self.width as usize + r.x as usize) * 4;
            pixels[y * row..(y + 1) * row].copy_from_slice(&self.pixels[start..start + row]);
        }
        Ok(Self {
            width: r.width,
            height: r.height,
            pixels,
        })
    }
    pub(crate) fn save(&self, path: &Path) -> Result<(), String> {
        let mut pixels = self.pixels.clone();
        for p in pixels.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        let image =
            image::RgbaImage::from_raw(self.width, self.height, pixels).ok_or("图片数据无效")?;
        let mut bytes = std::io::Cursor::new(Vec::new());
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        if extension == "jpg" || extension == "jpeg" {
            let mut rgb = image::RgbImage::new(self.width, self.height);
            for (source, target) in image.pixels().zip(rgb.pixels_mut()) {
                let alpha = u32::from(source.0[3]);
                for channel in 0..3 {
                    target.0[channel] =
                        ((u32::from(source.0[channel]) * alpha + 255 * (255 - alpha)) / 255) as u8;
                }
            }
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
                .encode_image(&rgb)
                .map_err(|e| e.to_string())?;
        } else if extension == "png" {
            image
                .write_to(&mut bytes, image::ImageFormat::Png)
                .map_err(|e| e.to_string())?;
        } else {
            return Err("请选择 PNG 或 JPEG 文件扩展名".into());
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        if let Err(e) = file
            .write_all(bytes.get_ref())
            .and_then(|()| file.sync_all())
        {
            drop(file);
            let _ = std::fs::remove_file(path);
            return Err(e.to_string());
        }
        Ok(())
    }
    pub(crate) fn pixel(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if x >= 0 && y >= 0 && x < self.width as i32 && y < self.height as i32 {
            let i = (y as usize * self.width as usize + x as usize) * 4;
            self.pixels[i..i + 4].copy_from_slice(&color);
        }
    }
}
fn bitmap_info(w: u32, h: u32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
}
pub(crate) struct Surface {
    pub(crate) dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    bytes: usize,
}
impl Surface {
    pub(crate) fn new(w: u32, h: u32) -> Result<Self, String> {
        let bytes = Raster::bytes(w, h)?;
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.is_invalid() {
            return Err("无法创建图片画布".into());
        }
        let mut bits = std::ptr::null_mut();
        let info = bitmap_info(w, h);
        let bitmap = match unsafe {
            CreateDIBSection(
                Some(dc),
                &raw const info,
                DIB_RGB_COLORS,
                &raw mut bits,
                None,
                0,
            )
        } {
            Ok(b) => b,
            Err(e) => {
                unsafe {
                    let _ = DeleteDC(dc);
                }
                return Err(e.to_string());
            }
        };
        let old = unsafe { SelectObject(dc, bitmap.into()) };
        Ok(Self {
            dc,
            bitmap,
            old,
            bits: bits.cast(),
            bytes,
        })
    }
    pub(crate) fn write_pixels(&self, pixels: &[u8]) {
        assert_eq!(pixels.len(), self.bytes);
        unsafe {
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.bits, self.bytes);
        }
    }
    pub(crate) fn opaque_rows(&self, width: u32, rows: u32) {
        unsafe {
            let _ = GdiFlush();
        }
        let len = ((width as usize * rows as usize) * 4).min(self.bytes);
        let pixels = unsafe { std::slice::from_raw_parts_mut(self.bits, len) };
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
    }
    fn raster(&self, w: u32, h: u32) -> Raster {
        unsafe {
            let _ = GdiFlush();
        }
        let mut pixels = unsafe { std::slice::from_raw_parts(self.bits, self.bytes) }.to_vec();
        for p in pixels.chunks_exact_mut(4) {
            p[3] = 255;
        }
        Raster {
            width: w,
            height: h,
            pixels,
        }
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}
fn screen() -> Result<(PhysicalRect, Raster), String> {
    let r = unsafe {
        PhysicalRect {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN) as u32,
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN) as u32,
        }
    };
    let surface = Surface::new(r.width, r.height)?;
    let dc = unsafe { GetDC(None) };
    if dc.is_invalid() {
        return Err("无法读取桌面".into());
    }
    let result = unsafe {
        BitBlt(
            surface.dc,
            0,
            0,
            r.width as i32,
            r.height as i32,
            Some(dc),
            r.x,
            r.y,
            SRCCOPY | CAPTUREBLT,
        )
    };
    unsafe {
        ReleaseDC(None, dc);
    }
    result.map_err(|e| e.to_string())?;
    Ok((r, surface.raster(r.width, r.height)))
}
#[path = "image_document.rs"]
mod image_document;
use image_document::{Document, ImageState, Mark, Tool};
#[path = "capture_selection.rs"]
mod capture_selection;
#[path = "picture_window.rs"]
mod picture_window;
use picture_window::{create_picture, picture_mut};
#[path = "capture_history.rs"]
mod capture_history;
#[path = "scroll_capture.rs"]
mod scroll_capture;
pub(crate) fn start_scroll(
    owner: HWND,
    region: PhysicalRect,
    library: &Path,
) -> Result<(), String> {
    scroll_capture::start(owner, region, library)
}
#[path = "pin_manager.rs"]
mod pin_manager;
pub(crate) fn begin(owner: HWND, intent: CaptureIntent) -> Result<(), String> {
    capture_selection::begin(owner, intent)
}
pub(crate) fn configure(library: &Path, restore: bool) -> Vec<String> {
    pin_manager::configure(library, restore)
}
pub(crate) fn restore_closed_pin() -> Result<(), String> {
    pin_manager::restore_closed(None)
}
pub(crate) fn set_pin_recovery_available(enabled: bool) {
    pin_manager::set_recovery_available(enabled);
}
pub(crate) fn recover_pins() -> Result<(), String> {
    pin_manager::recover_interaction()
}
pub(crate) fn remember_capture(image: &Raster) -> Result<(), String> {
    pin_manager::remember_image(image).map(|_| ())
}
pub(crate) fn show_history(owner: HWND, library: &Path) -> Result<(), String> {
    capture_history::show(owner, library)
}
pub(crate) fn dialog_message(message: &windows::Win32::UI::WindowsAndMessaging::MSG) -> bool {
    picture_window::dialog_message(message)
        || capture_history::dialog_message(message)
        || crate::platform::image_tasks::dialog_message(message)
}
pub(crate) fn show_image(image: Raster, library: &Path, pinned: bool) -> Result<HWND, String> {
    create_picture(
        std::rc::Rc::new(RefCell::new(Document::new(image))),
        library,
        pinned,
    )
}
pub(crate) fn prepare_close_pictures(owner: HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        IDNO, IDYES, MB_DEFBUTTON3, MB_ICONQUESTION, MB_YESNOCANCEL,
    };
    // No picture is destroyed during preparation: cancel/save failure leaves every
    // document open. The final owner teardown closes them only after authorization.
    let unsaved = PICTURES
        .with(|pictures| pictures.borrow().clone())
        .into_iter()
        .filter(|hwnd| unsafe { picture_mut(*hwnd) }.is_some_and(|p| !p.pinned && p.unsaved()))
        .collect::<Vec<_>>();
    let owner = crate::platform::workspace::window(owner);
    if !unsaved.is_empty() {
        let prompt = wide(&format!(
            "有 {} 张图片尚未保存。\n\n保存后，图片会保存在截图历史中。选择“不保存”将丢弃这些图片的修改。",
            unsaved.len()
        ));
        unsafe {
            let _ = ShowWindow(owner, windows::Win32::UI::WindowsAndMessaging::SW_RESTORE);
        }
        let answer = unsafe {
            crate::platform::app_dialog::message(
                Some(owner),
                PCWSTR(prompt.as_ptr()),
                w!("退出 Panzo"),
                MB_YESNOCANCEL | MB_ICONQUESTION | MB_DEFBUTTON3,
            )
        };
        if answer != IDYES && answer != IDNO {
            return false;
        }
        if answer == IDYES {
            for hwnd in unsaved {
                if let Some(picture) = unsafe { picture_mut(hwnd) } {
                    if let Err(message) = picture.command(hwnd, 44) {
                        error(
                            owner,
                            &format!("图片保存失败，已取消退出。所有图片仍保持打开。\n{message}"),
                        );
                        return false;
                    }
                }
            }
        }
    }
    if let Err(message) = pin_manager::save_session() {
        error(owner, &message);
        return false;
    }
    true
}
pub(crate) fn close_all_pictures() {
    scroll_capture::close_all();
    crate::platform::image_tasks::close_all();
    pin_manager::shutdown();
    capture_history::close_all();
    let windows = PICTURES.with(|pictures| pictures.borrow().clone());
    for hwnd in windows {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}
pub(crate) fn toggle_pins() -> Result<(), String> {
    let pins = PINS.with(|v| v.borrow().clone());
    let show = pins
        .iter()
        .any(|h| !unsafe { IsWindowVisible(*h) }.as_bool());
    for hwnd in pins {
        unsafe {
            let _ = ShowWindow(hwnd, if show { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }
    pin_manager::save_session()
}
pub(crate) fn pin_clipboard(owner: HWND, library: &Path) -> Result<(), String> {
    show_image(read_clipboard(owner)?, library, true).map(|_| ())
}
struct Clipboard;
impl Clipboard {
    fn open(owner: HWND) -> Result<Self, String> {
        unsafe { OpenClipboard(Some(owner)) }.map_err(|e| e.to_string())?;
        Ok(Self)
    }
}
impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}
fn copy_image(owner: HWND, image: &Raster) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(40 + image.pixels.len());
    let info = bitmap_info(image.width, image.height).bmiHeader;
    let header = unsafe { std::slice::from_raw_parts((&raw const info).cast::<u8>(), 40) };
    bytes.extend_from_slice(header);
    bytes.extend_from_slice(&image.pixels);
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }.map_err(|e| e.to_string())?;
    let data = unsafe { GlobalLock(memory) };
    if data.is_null() {
        unsafe {
            let _ = windows::Win32::Foundation::GlobalFree(Some(memory));
        }
        return Err("剪贴板内存分配失败".into());
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data.cast(), bytes.len());
        let _ = GlobalUnlock(memory);
    }
    let result = (|| {
        let _clipboard = Clipboard::open(owner)?;
        unsafe { EmptyClipboard() }.map_err(|e| e.to_string())?;
        unsafe { SetClipboardData(8, Some(HANDLE(memory.0))) }.map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        unsafe {
            let _ = windows::Win32::Foundation::GlobalFree(Some(memory));
        }
    }
    result
}
pub(crate) fn copy_text(owner: HWND, value: &str) -> Result<(), String> {
    let words = wide(value);
    let memory =
        unsafe { GlobalAlloc(GMEM_MOVEABLE, words.len() * 2) }.map_err(|e| e.to_string())?;
    let pointer = unsafe { GlobalLock(memory) };
    if pointer.is_null() {
        unsafe {
            let _ = windows::Win32::Foundation::GlobalFree(Some(memory));
        }
        return Err("无法分配剪贴板内存".into());
    }
    unsafe {
        std::ptr::copy_nonoverlapping(words.as_ptr(), pointer.cast(), words.len());
        let _ = GlobalUnlock(memory);
    }
    let result = (|| {
        let _clipboard = Clipboard::open(owner)?;
        unsafe { EmptyClipboard() }.map_err(|e| e.to_string())?;
        unsafe { SetClipboardData(13, Some(HANDLE(memory.0))) }.map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        unsafe {
            let _ = windows::Win32::Foundation::GlobalFree(Some(memory));
        }
    }
    result
}
fn read_clipboard(owner: HWND) -> Result<Raster, String> {
    let _clipboard = Clipboard::open(owner)?;
    let handle = unsafe { GetClipboardData(8) }
        .map_err(|_| "剪贴板中没有可贴出的图片，请先复制一张图片".to_string())?;
    let memory = HGLOBAL(handle.0);
    let size = unsafe { GlobalSize(memory) };
    let p = unsafe { GlobalLock(memory) };
    if p.is_null() {
        return Err("无法读取剪贴板图片".into());
    }
    let result = decode_dib(unsafe { std::slice::from_raw_parts(p.cast::<u8>(), size) });
    unsafe {
        let _ = GlobalUnlock(memory);
    }
    result
}
fn decode_dib(bytes: &[u8]) -> Result<Raster, String> {
    if bytes.len() < 40 {
        return Err("剪贴板图片数据不完整".into());
    }
    let value = |n| u32::from_le_bytes(bytes[n..n + 4].try_into().unwrap());
    let header = value(0) as usize;
    let width = value(4) as i32;
    let height = value(8) as i32;
    let bits = u16::from_le_bytes([bytes[14], bytes[15]]);
    let compression = value(16);
    if !(40..=124).contains(&header)
        || width <= 0
        || height == 0
        || height == i32::MIN
        || !matches!(bits, 24 | 32)
        || compression != 0
    {
        return Err("暂不支持此剪贴板图片格式，请先在 Panzo 中截图或打开 PNG".into());
    }
    let h = height.unsigned_abs();
    let len = Raster::bytes(width as u32, h)?;
    let stride = (width as usize * usize::from(bits) / 8).div_ceil(4) * 4;
    if header
        .checked_add(stride * h as usize)
        .is_none_or(|n| n > bytes.len())
    {
        return Err("剪贴板图片像素不完整".into());
    }
    let mut pixels = vec![255; len];
    for y in 0..h as usize {
        let sy = if height < 0 { y } else { h as usize - 1 - y };
        for x in 0..width as usize {
            let source = header + sy * stride + x * usize::from(bits / 8);
            let target = (y * width as usize + x) * 4;
            pixels[target..target + 3].copy_from_slice(&bytes[source..source + 3]);
        }
    }
    Ok(Raster {
        width: width as u32,
        height: h,
        pixels,
    })
}
fn save_dialog(owner: HWND, library: &Path) -> Option<PathBuf> {
    use windows::Win32::UI::Controls::Dialogs::{
        GetSaveFileNameW, OFN_EXPLORER, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    let directory = crate::platform::ui_preferences::output_directory().unwrap_or_else(||library.join("Screenshots"));
    let _ = std::fs::create_dir_all(&directory);
    let dir = wide(&directory.to_string_lossy());
    let mut buffer = vec![0u16; 32768];
    let name = wide(&format!("截图-{}.png", uuid::Uuid::new_v4()));
    buffer[..name.len()].copy_from_slice(&name);
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: w!("PNG 图片\0*.png\0JPEG 图片\0*.jpg;*.jpeg\0\0"),
        lpstrDefExt: w!("png"),
        lpstrInitialDir: PCWSTR(dir.as_ptr()),
        lpstrFile: windows::core::PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        Flags: OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if unsafe { GetSaveFileNameW(&raw mut dialog) }.as_bool() {
        let mut path = PathBuf::from(String::from_utf16_lossy(
            &buffer[..buffer.iter().position(|v| *v == 0)?],
        ));
        if dialog.nFilterIndex == 2
            && path
                .extension()
                .is_none_or(|extension| extension.eq_ignore_ascii_case("png"))
        {
            path.set_extension("jpg");
        }
        Some(path)
    } else {
        None
    }
}
pub(crate) fn draw_raster(dc: HDC, image: &Raster, r: RECT) {
    let mut preview = std::borrow::Cow::Borrowed(image.pixels.as_slice());
    if image.pixels.chunks_exact(4).any(|pixel| pixel[3] != 255) {
        let pixels = preview.to_mut();
        for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
            let x = index % image.width as usize;
            let y = index / image.width as usize;
            let background = if ((x / 12) + (y / 12)).is_multiple_of(2) {
                235_u32
            } else {
                205_u32
            };
            let alpha = u32::from(pixel[3]);
            for channel in pixel.iter_mut().take(3) {
                *channel = ((u32::from(*channel) * alpha + background * (255 - alpha)) / 255) as u8;
            }
            pixel[3] = 255;
        }
    }
    let info = bitmap_info(image.width, image.height);
    unsafe {
        SetStretchBltMode(dc, HALFTONE);
        StretchDIBits(
            dc,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            0,
            0,
            image.width as i32,
            image.height as i32,
            Some(preview.as_ptr().cast()),
            &raw const info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
}
fn point(lp: LPARAM) -> POINT {
    POINT {
        x: i32::from(lp.0 as u16 as i16),
        y: i32::from((lp.0 >> 16) as u16 as i16),
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}
fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}
fn title(hwnd: HWND, s: &str) {
    let value = wide(s);
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(value.as_ptr()));
    }
}
fn error(hwnd: HWND, s: &str) {
    if crate::platform::workspace::active() {
        crate::platform::workspace::notice_error(s);
        return;
    }
    let value = wide(s);
    unsafe {
        let _ = crate::platform::app_dialog::message(
            Some(hwnd),
            PCWSTR(value.as_ptr()),
            w!("Panzo"),
            MB_OK | MB_ICONERROR,
        );
    }
}
fn fill(dc: HDC, r: RECT, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(dc, &raw const r, brush);
        let _ = DeleteObject(brush.into());
    }
}
fn frame(dc: HDC, r: RECT, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FrameRect(dc, &raw const r, brush);
        let _ = DeleteObject(brush.into());
    }
}
fn text(dc: HDC, r: RECT, s: &str, color: COLORREF) {
    text_dpi(dc, r, s, color, 96);
}
fn text_dpi(dc: HDC, mut r: RECT, s: &str, color: COLORREF, dpi: u32) {
    let old = unsafe { SelectObject(dc, editor_ui::font_handle(TextStyle::Body, dpi)) };
    let mut value: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, color);
        DrawTextW(
            dc,
            &mut value,
            &raw mut r,
            DT_LEFT | DT_NOPREFIX | DT_WORDBREAK,
        );
        SelectObject(dc, old);
    }
}
#[cfg(test)]
#[path = "capture_review.rs"]
mod capture_review;
#[cfg(test)]
pub(crate) fn review_stage_b(owner: HWND, library: &Path, directory: &Path, round: usize) {
    if round == 0 {
        capture_review::first_run(owner, library, directory);
    } else if round == 1 {
        capture_review::second_run(owner, library, directory);
    } else {
        capture_review::fresh_process(library, directory);
    }
}

#[cfg(test)]
pub(crate) fn review_present(hwnd: HWND) {
    use windows::Win32::Graphics::Gdi::{
        RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, HWND_TOPMOST, MSG, PM_REMOVE, PeekMessageW,
    };
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE,
        );
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN,
        );
    }
    let until = std::time::Instant::now() + std::time::Duration::from_millis(250);
    while std::time::Instant::now() < until {
        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(&raw mut message, Some(hwnd), WM_PAINT, WM_PAINT, PM_REMOVE)
                .as_bool()
            {
                DispatchMessageW(&raw const message);
            }
            let _ = windows::Win32::Graphics::Dwm::DwmFlush();
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(test)]
pub(crate) fn review_window_png(hwnd: HWND, path: &Path) {
    // Capture the application's own surface: desktop occlusion must not contaminate evidence.
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &raw mut rect).unwrap(); }
    assert!(rect.right>0 && rect.bottom>0,"review window has no client area");
    editor_ui::render_review_png(path,rect.right,rect.bottom,|dc,_| {
        assert!(unsafe {windows::Win32::Storage::Xps::PrintWindow(hwnd,dc,windows::Win32::Storage::Xps::PRINT_WINDOW_FLAGS(3))}.as_bool());
    });
}

#[cfg(test)]
pub(crate) fn review_picture_flow(owner: HWND, library: &Path, directory: &Path) -> PhysicalRect {
    use windows::Win32::Graphics::Gdi::{ClientToScreen, UpdateWindow};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, HWND_TOPMOST};
    let mut pixels = vec![255; 640 * 360 * 4];
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        pixel.copy_from_slice(if index % 640 < 400 {
            &[20, 30, 230, 255]
        } else {
            &[230, 60, 20, 255]
        });
    }
    let source = show_image(
        Raster {
            width: 640,
            height: 360,
            pixels,
        },
        library,
        true,
    )
    .unwrap();
    unsafe {
        let _ = UpdateWindow(source);
    }
    let mut origin = POINT { x: 0, y: 0 };
    unsafe {
        let _ = ClientToScreen(source, &raw mut origin);
    }
    let region = PhysicalRect {
        x: origin.x + 20,
        y: origin.y + 20,
        width: 320,
        height: 180,
    };
    // Exercise the real desktop selector, then the annotation window's native commands.
    begin(owner, CaptureIntent::Screenshot).unwrap();
    let selector = unsafe { FindWindowW(w!("PanzoCaptureSelection"), None) }.unwrap();
    let x = region.x - unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let y = region.y - unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let pack = |x: i32, y: i32| LPARAM(((y as u32 & 0xffff) << 16 | (x as u32 & 0xffff)) as isize);
    unsafe {
        SendMessageW(selector, WM_LBUTTONDOWN, Some(WPARAM(0)), Some(pack(x, y)));
        SendMessageW(
            selector,
            WM_MOUSEMOVE,
            Some(WPARAM(0)),
            Some(pack(x + 320, y + 180)),
        );
        SendMessageW(
            selector,
            WM_LBUTTONUP,
            Some(WPARAM(0)),
            Some(pack(x + 320, y + 180)),
        );
        SendMessageW(selector, WM_KEYDOWN, Some(WPARAM(13)), None);
    }
    let Some(CaptureOutcome::Image(image)) = take_outcome() else {
        panic!("selection result")
    };
    assert_eq!((image.width, image.height), (320, 180));
    let picture = show_image(image, library, false).unwrap();
    let state = unsafe {
        &mut *(GetWindowLongPtrW(picture, GWLP_USERDATA) as *mut picture_window::Picture)
    };
    state.document.borrow_mut().commit(Mark {
        tool: Tool::Cover,
        points: vec![POINT { x: 10, y: 10 }, POINT { x: 60, y: 50 }],
        color: [0, 0, 0, 255],
        text: String::new(),
        ..Mark::default()
    });
    state.document.borrow_mut().commit(Mark {
        tool: Tool::Arrow,
        points: vec![POINT { x: 90, y: 80 }, POINT { x: 210, y: 115 }],
        color: [20, 30, 220, 255],
        text: String::new(),
        ..Mark::default()
    });
    state.command(picture, 7).unwrap();
    assert_eq!(state.document.borrow().state.marks.len(), 1);
    state.command(picture, 8).unwrap();
    assert_eq!(state.document.borrow().state.marks.len(), 2);
    state
        .output()
        .unwrap()
        .save(&directory.join("annotated.png"))
        .unwrap();
    let mut rect = RECT::default();
    unsafe {
        let _ = GetClientRect(picture, &raw mut rect);
        let _ = UpdateWindow(picture);
    }
    editor_ui::render_review_png(
        &directory.join("screenshot-editor.png"),
        rect.right,
        rect.bottom,
        |dc, _| {
            assert!(
                unsafe {
                    windows::Win32::Storage::Xps::PrintWindow(
                        picture,
                        dc,
                        windows::Win32::Storage::Xps::PRINT_WINDOW_FLAGS(3),
                    )
                }
                .as_bool()
            );
        },
    );
    unsafe {
        let _ = DestroyWindow(picture);
        let _ = SetWindowPos(
            source,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE,
        );
        let _ = UpdateWindow(source);
    }
    region
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cropped_images_and_redactions_do_not_export_unselected_pixels() {
        let original = Raster {
            width: 30,
            height: 20,
            pixels: vec![255; 30 * 20 * 4],
        };
        assert!(
            original
                .crop(PhysicalRect {
                    x: 29,
                    y: 0,
                    width: 2,
                    height: 2
                })
                .is_err()
        );
        let cropped = original
            .crop(PhysicalRect {
                x: 3,
                y: 4,
                width: 10,
                height: 8,
            })
            .unwrap();
        assert_eq!(cropped.pixels.len(), 320);
        let mut doc = Document::new(cropped);
        doc.commit(Mark {
            tool: Tool::Cover,
            points: vec![POINT { x: 0, y: 0 }, POINT { x: 5, y: 8 }],
            color: [0, 0, 0, 255],
            text: String::new(),
            ..Mark::default()
        });
        let image = doc.render(None).unwrap();
        assert_eq!(&image.pixels[..4], &[0, 0, 0, 255]);
        assert_eq!(&image.pixels[24..28], &[255; 4]);
        doc.undo();
        assert_eq!(doc.render(None).unwrap().pixels, doc.original.pixels);
        doc.redo();
        assert_eq!(doc.render(None).unwrap().pixels, image.pixels);
    }
    #[test]
    fn rejects_incomplete_or_oversized_clipboard_images() {
        assert!(decode_dib(&[0; 39]).is_err());
        let mut dib = vec![0u8; 40];
        dib[..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&100_000_u32.to_le_bytes());
        dib[8..12].copy_from_slice(&100_000_u32.to_le_bytes());
        dib[14..16].copy_from_slice(&32u16.to_le_bytes());
        assert!(decode_dib(&dib).is_err());
    }
}
#[cfg(test)]
pub(crate) fn review_ocr_image() -> Raster {
    let mut doc = Document::new(Raster {
        width: 800,
        height: 250,
        pixels: vec![255; 800 * 250 * 4],
    });
    doc.commit(Mark {
        tool: Tool::Text,
        points: vec![POINT { x: 24, y: 24 }, POINT { x: 780, y: 160 }],
        color: [0, 0, 0, 255],
        text: "Panzo OCR 2026\nScreenshot to text\n截图识别文字".into(),
        font_size: 42,
        ..Mark::default()
    });
    doc.render(None).unwrap()
}

pub(crate) fn manage_pin_groups(owner: HWND) -> Result<(), String> {
    pin_manager::manage_groups(owner)
}
pub(crate) fn show_gif(path: &Path, library: &Path) -> Result<HWND, String> {
    let clip = crate::platform::gif_player::Clip::load(path)?;
    let doc = std::rc::Rc::new(RefCell::new(Document::new(clip.frames[0].clone())));
    picture_window::create_picture_with_clip(
        doc,
        library,
        !crate::platform::workspace::active(),
        Some((clip, path.into())),
    )
}

#[path = "annotation_raster.rs"]
mod annotation_raster;
pub(crate) fn render_annotations(
    items: &[&panzo_core::VideoAnnotation],
    evaluation: &panzo_core::FrameEvaluation,
    settings: &panzo_core::WorkbenchSettings,
    source_size: (u32, u32),
) -> Result<Raster, String> {
    annotation_raster::render(items, evaluation, settings, source_size)
}

#[cfg(test)]
#[path = "image_tools_review.rs"]
mod image_tools_review;
#[cfg(test)]
pub(crate) fn review_stage_d(owner: HWND, library: &Path, root: &Path, round: usize) {
    image_tools_review::run(owner, library, root, round);
}

pub(crate) fn workspace_snapshot(hwnd: HWND) -> Result<(Raster, u64), String> {
    let state = unsafe { picture_mut(hwnd) }.ok_or("图片已关闭")?;
    Ok((state.output()?, state.document.borrow().revision))
}
pub(crate) fn embedded_history(parent: HWND, owner: HWND, library: &Path) -> Result<HWND, String> {
    capture_history::create_embedded(parent, owner, library)
}

pub(crate) fn manage_pin_groups_async() -> Result<(), String> {
    pin_manager::manage_groups_async()
}

pub(crate) fn workspace_metadata(hwnd: HWND) -> Option<(u32, u32, u64)> {
    let state = unsafe { picture_mut(hwnd) }?;
    let d = state.document.borrow();
    let (w, h) = d.size();
    Some((w, h, d.revision))
}
pub(crate) fn workspace_dirty(hwnd: HWND) -> bool {
    unsafe { picture_mut(hwnd) }.is_some_and(|state| state.unsaved())
}

#[cfg(test)]
pub(crate) fn review_chrome_png(hwnd: HWND, path: &Path) {
    review_present(hwnd);
    let mut r = RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &raw mut r);
        let _ = windows::Win32::Graphics::Dwm::DwmFlush();
    }
    let (desktop, image) = screen().unwrap();
    image
        .crop(PhysicalRect {
            x: r.left - desktop.x,
            y: r.top - desktop.y,
            width: (r.right - r.left) as u32,
            height: (r.bottom - r.top) as u32,
        })
        .unwrap()
        .save(path)
        .unwrap();
}

#[path = "subtitle_raster.rs"]
mod subtitle_raster;
pub(crate) fn render_subtitles(
    image: &mut Raster,
    cues: &[&panzo_core::media_edit::SubtitleCue],
    style: &panzo_core::media_edit::SubtitleStyle,
) -> Result<(), String> {
    subtitle_raster::render(image, cues, style)
}

pub(crate) fn workspace_inspector(hwnd: HWND, width: i32) {
    if let Some(p) = unsafe { picture_mut(hwnd) } {
        if p.inspector_width != width {
            p.inspector_width = width;
            p.relayout(hwnd);
        }
    }
}
pub(crate) fn workspace_zoom(hwnd: HWND) -> f64 {
    unsafe { picture_mut(hwnd) }.map_or(100.0, |p| p.workspace_zoom(hwnd))
}

pub(crate) fn asset_thumbnail(
    asset: &crate::platform::asset_index::Asset,
) -> Result<Raster, String> {
    capture_history::thumbnail(asset)
}

pub(crate) fn workspace_recognition(hwnd: HWND, online: bool) {
    if let Some(s) = unsafe { picture_window::picture_mut(hwnd) } {
        s.recognition = Some(online);
        s.command(hwnd, 62).ok();
        s.recognition = Some(online);
        s.relayout(hwnd);
    }
}
