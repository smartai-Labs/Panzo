//! Installed fonts and shared native font controls. No font files are bundled.
use super::{desktop_ui as desk, workspace_ui as ui};
use panzo_core::text_style::{FontStyle, TextAlignment};
use std::sync::OnceLock;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT, WPARAM},
        Graphics::{DirectWrite::*, Gdi::*},
        UI::WindowsAndMessaging::*,
    },
    core::{PCWSTR, w},
};
#[derive(Clone)]
pub struct Family {
    pub name: String,
}
pub fn families() -> &'static [Family] {
    static FONTS: OnceLock<Vec<Family>> = OnceLock::new();
    FONTS.get_or_init(|| {
        enumerate().unwrap_or_else(|_| {
            vec![Family {
                name: "Microsoft YaHei UI".into(),
            }]
        })
    })
}
fn enumerate() -> windows::core::Result<Vec<Family>> {
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let mut collection = None;
        factory.GetSystemFontCollection(&mut collection, true)?;
        let collection = collection.unwrap();
        let mut result = Vec::new();
        for i in 0..collection.GetFontFamilyCount() {
            let family = collection.GetFontFamily(i)?;
            let names = family.GetFamilyNames()?;
            let mut value = vec![0; names.GetStringLength(0)? as usize + 1];
            names.GetString(0, &mut value)?;
            let name = String::from_utf16_lossy(&value[..value.len() - 1]);
            if name.starts_with('@') {
                continue;
            }
            result.push(Family { name });
        }
        result.sort_by_key(|f| f.name.to_lowercase());
        result.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
        Ok(result)
    }
}
pub fn exists(name: &str) -> bool {
    families().iter().any(|f| f.name.eq_ignore_ascii_case(name))
}
pub fn create_font(style: &FontStyle, size: i32) -> HFONT {
    let name = ui::wide(if exists(&style.family) {
        &style.family
    } else {
        "Microsoft YaHei UI"
    });
    unsafe {
        CreateFontW(
            -size.max(1),
            0,
            0,
            0,
            i32::from(style.weight),
            u32::from(style.italic),
            u32::from(style.underline),
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            u32::from(DEFAULT_PITCH.0 | FF_DONTCARE.0),
            PCWSTR(name.as_ptr()),
        )
    }
}
pub fn alignment(align: TextAlignment) -> DRAW_TEXT_FORMAT {
    match align {
        TextAlignment::Left => DT_LEFT,
        TextAlignment::Center => DT_CENTER,
        TextAlignment::Right => DT_RIGHT,
    }
}
pub fn measure(text: &str, style: &FontStyle, size: i32) -> (i32, i32) {
    unsafe {
        let dc = CreateCompatibleDC(None);
        let font = create_font(style, size);
        let old = SelectObject(dc, font.into());
        let mut value = text.encode_utf16().collect::<Vec<_>>();
        if value.is_empty() {
            value.push(32);
        }
        let mut rect = RECT::default();
        DrawTextW(
            dc,
            &mut value,
            &mut rect,
            DT_CALCRECT | DT_NOPREFIX | DT_LEFT,
        );
        SelectObject(dc, old);
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(dc);
        ((rect.right + 2).max(1), (rect.bottom + 2).max(size))
    }
}
pub struct FontControls {
    pub base: usize,
    pub family: HWND,
    pub weight: HWND,
    pub italic: HWND,
    pub underline: HWND,
    pub align: HWND,
    pub center: HWND,
    pub right: HWND,
    pub preview: HWND,
    pub title: HWND,
    preview_font: HFONT,
    pub loaded: FontStyle,
}
impl FontControls {
    pub fn new(parent: HWND, base: usize) -> Result<Self, String> {
        let family = ui::child(
            parent,
            w!("COMBOBOX"),
            "",
            base,
            WS_TABSTOP
                | WS_VSCROLL
                | WINDOW_STYLE((CBS_DROPDOWN | CBS_AUTOHSCROLL | CBS_HASSTRINGS) as u32),
        )?;
        for f in families() {
            let s = ui::wide(&f.name);
            unsafe {
                SendMessageW(
                    family,
                    CB_ADDSTRING,
                    None,
                    Some(LPARAM(s.as_ptr() as isize)),
                );
            }
        }
        desk::combo(family);
        let result = Self {
            base,
            family,
            weight: ui::button(parent, "粗体", base + 1)?,
            italic: ui::button(parent, "斜体", base + 2)?,
            underline: ui::button(parent, "下划线", base + 3)?,
            align: ui::button(parent, "左对齐", base + 4)?,
            center: ui::button(parent, "居中对齐", base + 10)?,
            right: ui::button(parent, "右对齐", base + 11)?,
            preview: ui::label(parent, "字体预览 Aa 中文", base + 5)?,
            title: ui::label(parent, "字体", base + 6)?,
            preview_font: HFONT::default(),
            loaded: FontStyle::default(),
        };
        desk::style(
            result.weight,
            Some(super::editor_ui::IconKind::Prototype(
                "<path d='M6 4h7a4 4 0 0 1 0 8H6V4Zm0 8h8a4 4 0 0 1 0 8H6v-8Z'/>",
            )),
            false,
            false,
            true,
        );
        desk::style(
            result.italic,
            Some(super::editor_ui::IconKind::Prototype(
                "<path d='M19 4h-9M14 20H5M15 4 9 20'/>",
            )),
            false,
            false,
            true,
        );
        desk::style(
            result.underline,
            Some(super::editor_ui::IconKind::Prototype(
                "<path d='M6 3v7a6 6 0 0 0 12 0V3M4 21h16'/>",
            )),
            false,
            false,
            true,
        );
        for (h, svg) in [
            (result.align, "<path d='M4 5h16M4 10h10M4 15h16M4 20h10'/>"),
            (result.center, "<path d='M4 5h16M7 10h10M4 15h16M7 20h10'/>"),
            (
                result.right,
                "<path d='M4 5h16M10 10h10M4 15h16M10 20h10'/>",
            ),
        ] {
            desk::style(
                h,
                Some(super::editor_ui::IconKind::Prototype(svg)),
                false,
                false,
                true,
            );
        }
        Ok(result)
    }
    pub fn handles(&self) -> [HWND; 9] {
        [
            self.family,
            self.weight,
            self.italic,
            self.underline,
            self.align,
            self.preview,
            self.title,
            self.center,
            self.right,
        ]
    }
    pub fn show(&self, shown: bool) {
        for h in self.handles() {
            ui::show(h, shown && h != self.preview);
        }
    }
    pub fn place(&self, parent: HWND, x: i32, y: i32, width: i32) {
        let d = |n| ui::scale(parent, n);
        ui::place(self.title, x, y, width, d(20));
        ui::place(self.family, x, y + d(28), width, d(28));
        for (i, h) in [self.weight, self.italic, self.underline]
            .iter()
            .enumerate()
        {
            ui::place(*h, x + d(i as i32 * 36), y + d(64), d(28), d(28));
        }
        for (i, h) in [self.align, self.center, self.right].iter().enumerate() {
            ui::place(
                *h,
                x + width - d(100) + d(i as i32 * 36),
                y + d(64),
                d(28),
                d(28),
            );
        }
    }

    pub fn load(&mut self, style: &FontStyle) {
        self.loaded = style.clone();
        ui::text(self.family, &style.family);
        desk::selected(self.weight, style.weight >= 600);
        desk::selected(self.align, style.alignment == TextAlignment::Left);
        desk::selected(self.center, style.alignment == TextAlignment::Center);
        desk::selected(self.right, style.alignment == TextAlignment::Right);
        desk::selected(self.italic, style.italic);
        desk::selected(self.underline, style.underline);
        ui::text(
            self.title,
            if exists(&style.family) {
                "字体"
            } else {
                "字体缺失 · 暂用微软雅黑"
            },
        );
    }
    pub fn read(&self, id: usize) -> FontStyle {
        let mut style = self.loaded.clone();
        style.family = if id == self.base {
            combo_value(self.family)
        } else {
            ui::value(self.family)
        }
        .trim()
        .to_owned();
        if id == self.base + 1 {
            style.weight = if style.weight >= 600 { 400 } else { 700 };
        }
        if id == self.base + 2 {
            style.italic = !style.italic;
        }
        if id == self.base + 3 {
            style.underline = !style.underline;
        }
        if id == self.base + 4 {
            style.alignment = TextAlignment::Left;
        }
        if id == self.base + 10 {
            style.alignment = TextAlignment::Center;
        }
        if id == self.base + 11 {
            style.alignment = TextAlignment::Right;
        }
        style
    }
}
impl Drop for FontControls {
    fn drop(&mut self) {
        if !self.preview_font.is_invalid() {
            unsafe {
                let _ = DeleteObject(self.preview_font.into());
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fonts_measure_real_glyphs() {
        assert!(families().len() > 1);
        let a = FontStyle {
            family: "Arial".into(),
            ..Default::default()
        };
        assert!(measure("WWW", &a, 32).0 > measure("iii", &a, 32).0);
        assert!(measure("中文\n中文", &a, 32).1 > measure("中文", &a, 32).1);
    }
}

/// Selection notifications can arrive before the combo updates its edit child.
pub(crate) fn combo_value(h: HWND) -> String {
    unsafe {
        let index = SendMessageW(h, CB_GETCURSEL, None, None).0;
        if index >= 0 {
            let len = SendMessageW(h, CB_GETLBTEXTLEN, Some(WPARAM(index as usize)), None).0;
            if (0..4096).contains(&len) {
                let mut text = vec![0u16; len as usize + 1];
                SendMessageW(
                    h,
                    CB_GETLBTEXT,
                    Some(WPARAM(index as usize)),
                    Some(LPARAM(text.as_mut_ptr() as isize)),
                );
                return String::from_utf16_lossy(&text[..len as usize]);
            }
        }
    }
    ui::value(h)
}
