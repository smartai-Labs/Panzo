//! One native text layout for image text, inline editing and exported pixels.
use super::{Raster, Surface};
use crate::platform::{editor_ui, workspace_ui as ui};
use panzo_core::text_style::{FontStyle, TextAlignment};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT, WPARAM},
        UI::{Controls::RichEdit::*, WindowsAndMessaging::*},
    },
    core::w,
};
pub(crate) fn format(h: HWND, style: &FontStyle, size: i32, color: [u8; 4], dpi: u32) {
    let mut f = CHARFORMAT2W::default();
    f.Base.cbSize = std::mem::size_of::<CHARFORMAT2W>() as u32;
    f.Base.dwMask = CFM_FACE | CFM_SIZE | CFM_WEIGHT | CFM_ITALIC | CFM_UNDERLINE | CFM_COLOR;
    f.Base.yHeight = size * 1440 / dpi as i32;
    f.wWeight = style.weight;
    f.Base.crTextColor = editor_ui::rgb(color[2], color[1], color[0]);
    f.Base.dwEffects = if style.italic {
        CFE_ITALIC
    } else {
        CFE_EFFECTS(0)
    } | if style.underline {
        CFE_UNDERLINE
    } else {
        CFE_EFFECTS(0)
    };
    let family = if crate::platform::font_catalog::exists(&style.family) {
        &style.family
    } else {
        "Microsoft YaHei UI"
    };
    for (to, from) in f
        .Base
        .szFaceName
        .iter_mut()
        .take(31)
        .zip(family.encode_utf16())
    {
        *to = from;
    }
    let mut p = PARAFORMAT2::default();
    p.Base.cbSize = std::mem::size_of::<PARAFORMAT2>() as u32;
    p.Base.dwMask = PFM_ALIGNMENT;
    p.Base.wAlignment = match style.alignment {
        TextAlignment::Left => PFA_LEFT,
        TextAlignment::Center => PFA_CENTER,
        TextAlignment::Right => PFA_RIGHT,
    };
    unsafe {
        SendMessageW(
            h,
            EM_SETCHARFORMAT,
            Some(WPARAM(SCF_ALL as usize)),
            Some(LPARAM((&f as *const _) as isize)),
        );
        SendMessageW(
            h,
            EM_SETPARAFORMAT,
            None,
            Some(LPARAM((&p as *const _) as isize)),
        );
        if ui::value(h).is_empty() {
            SendMessageW(
                h,
                EM_SETCHARFORMAT,
                Some(WPARAM(SCF_SELECTION as usize)),
                Some(LPARAM((&f as *const _) as isize)),
            );
        }
    }
}
fn rasterize_uncached(
    text: &str,
    style: &FontStyle,
    size: i32,
    width: u32,
    height: u32,
) -> Result<Raster, String> {
    static DLL: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    DLL.get_or_init(|| {
        unsafe { windows::Win32::System::LibraryLoader::LoadLibraryW(w!("Msftedit.dll")) }
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .clone()?;
    let surface = Surface::new(width, height)?;
    let h = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("RICHEDIT50W"),
            w!(""),
            WS_POPUP | WINDOW_STYLE((ES_MULTILINE | ES_WANTRETURN) as u32),
            0,
            0,
            width as i32,
            height as i32,
            None,
            None,
            None,
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    struct Cleanup(HWND);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
    let _cleanup = Cleanup(h);
    ui::text(h, text);
    format(h, style, size, [255; 4], editor_ui::window_dpi(h));
    let rect = RECT {
        left: 0,
        top: 0,
        right: width as i32,
        bottom: height as i32,
    };
    unsafe {
        SendMessageW(h, EM_SETBKGNDCOLOR, None, Some(LPARAM(0)));
        SendMessageW(
            h,
            windows::Win32::UI::Controls::EM_SETMARGINS,
            Some(WPARAM(3)),
            Some(LPARAM(0)),
        );
        SendMessageW(
            h,
            windows::Win32::UI::Controls::EM_SETRECTNP,
            None,
            Some(LPARAM((&rect as *const _) as isize)),
        );
        let origin = windows::Win32::Foundation::POINT::default();
        SendMessageW(
            h,
            EM_SETSCROLLPOS,
            None,
            Some(LPARAM((&origin as *const _) as isize)),
        );
        std::ptr::write_bytes(surface.bits, 0, surface.bytes);
        SendMessageW(
            h,
            WM_PRINTCLIENT,
            Some(WPARAM(surface.dc.0 as usize)),
            Some(LPARAM(PRF_CLIENT as isize)),
        );
    }
    Ok(surface.raster(width, height))
}

// Moving text changes its origin, not its glyph coverage. Keep a bounded,
// thread-local cache so repeated drag paints never create RichEdit windows.
struct Coverage {
    text: String,
    style: FontStyle,
    size: i32,
    raster: std::rc::Rc<Raster>,
}
thread_local! {
    static COVERAGE: std::cell::RefCell<Vec<Coverage>> = const { std::cell::RefCell::new(Vec::new()) };
}
pub(crate) fn rasterize(
    text: &str,
    style: &FontStyle,
    size: i32,
    width: u32,
    height: u32,
) -> Result<std::rc::Rc<Raster>, String> {
    if let Some(hit) = COVERAGE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let index = cache.iter().position(|c| {
            c.text == text
                && c.style == *style
                && c.size == size
                && c.raster.width == width
                && c.raster.height == height
        })?;
        let entry = cache.remove(index);
        let raster = entry.raster.clone();
        cache.push(entry);
        Some(raster)
    }) {
        return Ok(hit);
    }
    let raster = std::rc::Rc::new(rasterize_uncached(text, style, size, width, height)?);
    const LIMIT: usize = 32 * 1024 * 1024;
    if raster.pixels.len() <= LIMIT {
        COVERAGE.with(|cache| {
            let mut cache = cache.borrow_mut();
            while !cache.is_empty()
                && (cache.len() >= 8
                    || cache.iter().map(|c| c.raster.pixels.len()).sum::<usize>()
                        + raster.pixels.len()
                        > LIMIT)
            {
                cache.remove(0);
            }
            cache.push(Coverage {
                text: text.into(),
                style: style.clone(),
                size,
                raster: raster.clone(),
            });
        });
    }
    Ok(raster)
}
