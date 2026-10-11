//! A themed native popup for existing application commands. Windows owns navigation/dismissal.
use super::editor_ui::{self, DrawingScale, EditorTheme, IconKind, TextStyle};
use std::cell::RefCell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, DrawTextW,
    SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::Accessibility::{MSAA_MENU_SIG, MSAAMENUINFO};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_SELECTED, ODT_MENU};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, HMENU, MF_CHECKED, MF_GRAYED, MF_OWNERDRAW,
    TPM_NONOTIFY, TPM_RETURNCMD, TrackPopupMenu, WM_DRAWITEM, WM_MEASUREITEM,
};
use windows::core::{PCWSTR, PWSTR};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Home,
    NewRecording,
    Open,
    ContinueRecent,
    Save,
    Export,
    OpenLocation,
    MediaEffects,
    Audio,
    Subtitles,
}

#[derive(Clone, Copy)]
#[allow(clippy::struct_excessive_bools)] // Independent command availability, not mutually exclusive states.
pub(crate) struct Availability {
    pub new_recording: bool,
    pub open: bool,
    pub recent: bool,
    pub save: bool,
    pub export: bool,
}

#[derive(Clone)]
pub(crate) struct Row {
    label: std::borrow::Cow<'static, str>,
    icon: Option<IconKind>,
    command: Option<i32>,
    enabled: bool,
    checked: bool,
    shortcut: String,
}

impl Row {
    const fn text_style(&self) -> TextStyle {
        if self.command.is_some() {
            TextStyle::Menu
        } else {
            TextStyle::Caption
        }
    }

    pub(crate) const fn command(
        command: i32,
        label: &'static str,
        icon: IconKind,
        enabled: bool,
    ) -> Self {
        Self {
            label: std::borrow::Cow::Borrowed(label),
            icon: Some(icon),
            command: Some(command),
            enabled,
            checked: false,
            shortcut: String::new(),
        }
    }

    pub(crate) fn labelled(
        command: i32,
        label: impl Into<String>,
        icon: IconKind,
        enabled: bool,
    ) -> Self {
        Self {
            label: std::borrow::Cow::Owned(label.into()),
            icon: Some(icon),
            command: Some(command),
            enabled,
            checked: false,
            shortcut: String::new(),
        }
    }
    pub(crate) fn shortcut_hint(self, id: &str) -> Self {
        self.with_shortcut(super::shortcuts::label(id))
    }
    pub(crate) fn with_shortcut(mut self, shortcut: String) -> Self {
        self.shortcut = shortcut;
        self
    }
    pub(crate) const fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
    pub(crate) const fn heading(label: &'static str) -> Self {
        Self {
            label: std::borrow::Cow::Borrowed(label),
            icon: None,
            command: None,
            enabled: false,
            checked: false,
            shortcut: String::new(),
        }
    }

    pub(crate) const fn separator() -> Self {
        Self::heading("")
    }
}

struct MenuView {
    rows: Vec<Row>,
    theme: EditorTheme,
    width: i32,
    shortcut_width: i32,
}

thread_local! { static ACTIVE: RefCell<Option<MenuView>> = const { RefCell::new(None) }; }
const FIRST_ID: u32 = 10_001;

pub(crate) fn is_open() -> bool {
    ACTIVE.with(|active| active.borrow().is_some())
}

struct Popup(HMENU);
impl Drop for Popup {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.0);
        }
        ACTIVE.with(|active| *active.borrow_mut() = None);
    }
}

pub(crate) fn show(
    hwnd: HWND,
    anchor: RECT,
    theme: EditorTheme,
    available: Availability,
) -> windows::core::Result<Option<Action>> {
    let rows = vec![
        Row::command(7, "录制准备", IconKind::Home, available.new_recording),
        Row::command(1, "录制新视频", IconKind::Record, available.new_recording),
        Row::command(2, "打开工程…", IconKind::Folder, available.open),
        Row::command(3, "继续最近工程", IconKind::Restore, available.recent),
        Row::separator(),
        Row::command(4, "保存", IconKind::Save, available.save),
        Row::command(5, "导出…", IconKind::Export, available.export),
        Row::command(9, "声音与旁白", IconKind::Microphone, available.export),
        Row::command(
            10,
            "字幕与 AI 自动字幕",
            IconKind::Subtitles,
            available.export,
        ),
        Row::command(8, "镜头、标注与音量…", IconKind::Edit, available.export),
        Row::separator(),
        Row::command(6, "打开文件位置", IconKind::Folder, true),
    ];
    Ok(
        show_rows(hwnd, anchor, theme, rows, false)?.and_then(|command| match command {
            9 => Some(Action::Audio),
            10 => Some(Action::Subtitles),
            8 => Some(Action::MediaEffects),
            7 => Some(Action::Home),
            1 => Some(Action::NewRecording),
            2 => Some(Action::Open),
            3 => Some(Action::ContinueRecent),
            4 => Some(Action::Save),
            5 => Some(Action::Export),
            6 => Some(Action::OpenLocation),
            _ => None,
        }),
    )
}

pub(crate) fn show_rows(
    hwnd: HWND,
    anchor: RECT,
    theme: EditorTheme,
    rows: Vec<Row>,
    align_right: bool,
) -> windows::core::Result<Option<i32>> {
    if is_open() {
        return Ok(None);
    }
    // Windows reads these names through MSAA for owner-drawn menu items. Keep
    // both allocations alive until the native popup is destroyed.
    let mut labels: Vec<Vec<u16>> = rows
        .iter()
        .map(|row| {
            let label = if row.shortcut.is_empty() {
                row.label.to_string()
            } else {
                format!("{}\t{}", row.label, row.shortcut)
            };
            label.encode_utf16().chain([0]).collect()
        })
        .collect();
    let names: Vec<MSAAMENUINFO> = labels
        .iter_mut()
        .map(|label| MSAAMENUINFO {
            dwMSAASignature: MSAA_MENU_SIG.cast_unsigned(),
            cchWText: u32::try_from(label.len() - 1).unwrap_or(0),
            pszWText: PWSTR(label.as_mut_ptr()),
        })
        .collect();
    let menu = Popup(unsafe { CreatePopupMenu()? });
    for (index, row) in rows.iter().enumerate() {
        let mut flags = if row.enabled {
            MF_OWNERDRAW
        } else {
            MF_OWNERDRAW | MF_GRAYED
        };
        if row.checked {
            flags |= MF_CHECKED;
        }
        unsafe {
            AppendMenuW(
                menu.0,
                flags,
                FIRST_ID as usize + index,
                PCWSTR((&raw const names[index]).cast()),
            )?;
        }
    }
    let (width, shortcut_width) = menu_metrics(hwnd, &rows, theme);
    ACTIVE.with(|active| {
        *active.borrow_mut() = Some(MenuView {
            rows,
            theme,
            width,
            shortcut_width,
        })
    });
    let _frame = frame::StyleGuard::install(menu.0, theme)?;
    let mut point = POINT {
        x: if align_right {
            anchor.right
        } else {
            anchor.left
        },
        y: anchor.bottom + theme.scale(4),
    };
    unsafe {
        let _ = ClientToScreen(hwnd, &raw mut point);
    }
    let selected = unsafe {
        TrackPopupMenu(
            menu.0,
            TPM_RETURNCMD
                | TPM_NONOTIFY
                | if align_right {
                    windows::Win32::UI::WindowsAndMessaging::TPM_RIGHTALIGN
                } else {
                    windows::Win32::UI::WindowsAndMessaging::TPM_LEFTALIGN
                },
            point.x,
            point.y,
            None,
            hwnd,
            None,
        )
    }
    .0;
    let choice = u32::try_from(selected)
        .ok()
        .and_then(|id| id.checked_sub(FIRST_ID))
        .and_then(|index| row(FIRST_ID + index).map(|(row, _)| row))
        .filter(|row| row.enabled)
        .and_then(|row| row.command);
    Ok(choice)
}

fn menu_metrics(hwnd: HWND, rows: &[Row], theme: EditorTheme) -> (i32, i32) {
    use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
    let _scale = DrawingScale::enter(theme.dpi);
    let dc = unsafe { GetDC(Some(hwnd)) };
    if dc.is_invalid() {
        return (theme.scale(280), 0);
    }
    // Reserve whole columns: the longest label and longest shortcut may be on
    // different rows. Leave a real gutter before the right-aligned shortcut.
    let label = rows
        .iter()
        .map(|row| editor_ui::measure_text_width(dc, &row.label, row.text_style()))
        .max()
        .unwrap_or(0);
    let shortcut = rows
        .iter()
        .map(|row| editor_ui::measure_text_width(dc, &row.shortcut, TextStyle::Caption))
        .max()
        .unwrap_or(0);
    unsafe {
        ReleaseDC(Some(hwnd), dc);
    }
    let gutter = if shortcut > 0 { theme.scale(24) } else { 0 };
    (
        (theme.scale(42 + 36) + label + gutter + shortcut).max(theme.scale(224)),
        shortcut,
    )
}

fn text_columns(rect: RECT, shortcut_width: i32, theme: EditorTheme) -> (RECT, RECT) {
    let hint = RECT {
        left: rect.right - theme.scale(36) - shortcut_width,
        right: rect.right - theme.scale(36),
        ..rect
    };
    let label = RECT {
        left: rect.left + theme.scale(42),
        right: hint.left
            - if shortcut_width > 0 {
                theme.scale(24)
            } else {
                0
            },
        ..rect
    };
    (label, hint)
}

fn row(id: u32) -> Option<(Row, EditorTheme)> {
    let index = id.checked_sub(FIRST_ID)? as usize;
    ACTIVE.with(|active| {
        active
            .borrow()
            .as_ref()
            .and_then(|view| view.rows.get(index).map(|row| (row.clone(), view.theme)))
    })
}

pub(crate) fn window_message(message: u32, lparam: LPARAM) -> Option<LRESULT> {
    #[cfg(test)]
    if review::capture_popup(message, lparam) {
        return Some(LRESULT(0));
    }
    if lparam.0 == 0 {
        return None;
    }
    match message {
        WM_MEASUREITEM => {
            let item = unsafe { &mut *(lparam.0 as *mut MEASUREITEMSTRUCT) };
            if item.CtlType != ODT_MENU {
                return None;
            }
            let (row, theme) = row(item.itemID)?;
            item.itemWidth = ACTIVE
                .with(|active| {
                    active
                        .borrow()
                        .as_ref()
                        .map_or(theme.scale(224), |view| view.width)
                })
                .cast_unsigned();
            item.itemHeight = theme
                .scale(if row.command.is_some() {
                    36
                } else if row.label.is_empty() {
                    11
                } else {
                    24
                })
                .cast_unsigned();
            Some(LRESULT(1))
        }
        WM_DRAWITEM => {
            let item = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
            if item.CtlType != ODT_MENU {
                return None;
            }
            let (row, theme) = row(item.itemID)?;
            draw_row(item, row, theme);
            Some(LRESULT(1))
        }
        _ => None,
    }
}

fn draw_row(item: &DRAWITEMSTRUCT, row: Row, theme: EditorTheme) {
    let _scale = DrawingScale::enter(theme.dpi);
    let s = |n| theme.scale(n);
    let rect = item.rcItem;
    let dc = item.hDC;
    editor_ui::fill_rect(dc, &rect, theme.palette.raised);
    if row.command.is_none() && row.label.is_empty() {
        let y = i32::midpoint(rect.top, rect.bottom);
        editor_ui::fill_rect(
            dc,
            &RECT {
                left: rect.left + s(12),
                right: rect.right - s(12),
                top: y,
                bottom: y + s(1),
            },
            theme.palette.divider,
        );
        return;
    }
    if row.enabled && item.itemState.0 & ODS_SELECTED.0 != 0 {
        editor_ui::rounded_surface(
            dc,
            RECT {
                left: rect.left + s(5),
                right: rect.right - s(5),
                top: rect.top + s(2),
                bottom: rect.bottom - s(2),
            },
            theme.palette.hover,
            theme.palette.hover,
            s(6),
        );
    }
    let color = if row.enabled {
        theme.palette.text
    } else {
        theme.palette.text_muted
    };
    let y = i32::midpoint(rect.top, rect.bottom) - s(8);
    if let Some(icon) = row.icon {
        editor_ui::draw_icon(
            dc,
            RECT {
                left: rect.left + s(14),
                right: rect.left + s(30),
                top: y,
                bottom: y + s(16),
            },
            icon,
            color,
        );
    }
    if row.checked {
        editor_ui::draw_icon(
            dc,
            RECT {
                left: rect.right - s(28),
                right: rect.right - s(12),
                top: y,
                bottom: y + s(16),
            },
            IconKind::Check,
            theme.palette.accent,
        );
    }
    let shortcut_width = ACTIVE.with(|active| {
        active
            .borrow()
            .as_ref()
            .map_or(0, |view| view.shortcut_width)
    });
    let (mut text_rect, hint_rect) = text_columns(rect, shortcut_width, theme);
    if !row.shortcut.is_empty() {
        let mut hint_rect = hint_rect;
        let mut hint: Vec<u16> = row.shortcut.encode_utf16().collect();
        let previous = editor_ui::select_font(dc, TextStyle::Caption);
        unsafe {
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, theme.palette.text_muted);
            DrawTextW(
                dc,
                &mut hint,
                &mut hint_rect,
                DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            SelectObject(dc, previous);
        }
    }
    if row.command.is_none() {
        text_rect.left = rect.left + s(14);
    }
    let mut text: Vec<u16> = row.label.encode_utf16().collect();
    let previous = editor_ui::select_font(dc, row.text_style());
    unsafe {
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, color);
        DrawTextW(
            dc,
            &mut text,
            &raw mut text_rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        SelectObject(dc, previous);
    }
}

#[path = "popup_frame.rs"]
mod frame;

#[cfg(test)]
pub(crate) mod review {
    use super::*;
    use std::path::PathBuf;
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    use windows::Win32::UI::WindowsAndMessaging::{EndMenu, GetWindowRect, WM_ENTERIDLE};
    thread_local! { static OUTPUT: RefCell<Option<PathBuf>> = const { RefCell::new(None) }; }

    pub(crate) fn next_popup(output: PathBuf) {
        OUTPUT.with(|path| *path.borrow_mut() = Some(output));
    }

    pub(super) fn capture_popup(message: u32, lparam: LPARAM) -> bool {
        if message != WM_ENTERIDLE || lparam.0 == 0 || !is_open() {
            return false;
        }
        let Some(output) = OUTPUT.with(|path| path.borrow_mut().take()) else {
            return false;
        };
        let popup = HWND(lparam.0 as *mut std::ffi::c_void);
        let mut rect = RECT::default();
        unsafe {
            GetWindowRect(popup, &raw mut rect).unwrap();
        }
        editor_ui::render_review_png(
            &output,
            rect.right - rect.left,
            rect.bottom - rect.top,
            |dc, _| {
                assert!(unsafe { PrintWindow(popup, dc, PRINT_WINDOW_FLAGS(0)) }.as_bool());
            },
        );
        unsafe {
            EndMenu().unwrap();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcut_column_reserves_space_for_long_labels_at_all_dpi() {
        use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
        for dpi in [96, 120, 144, 192] {
            let theme = EditorTheme::for_dpi(dpi);
            let _scale = DrawingScale::enter(dpi);
            let rows = vec![
                Row::command(1, "恢复最近关闭的贴图", IconKind::Restore, true),
                Row::command(2, "区域截图", IconKind::Capture, true)
                    .with_shortcut("Ctrl+Alt+Shift+F9".into()),
                Row::command(3, "显示 / 隐藏贴图", IconKind::Pin, true)
                    .with_shortcut("Ctrl+P".into())
                    .checked(true),
            ];
            let (width, shortcut) = menu_metrics(HWND::default(), &rows, theme);
            let (label, hint) = text_columns(
                RECT {
                    right: width,
                    bottom: theme.scale(36),
                    ..RECT::default()
                },
                shortcut,
                theme,
            );
            let dc = unsafe { GetDC(None) };
            for row in rows {
                assert!(
                    editor_ui::measure_text_width(dc, &row.label, row.text_style())
                        <= label.right - label.left
                );
                assert!(
                    editor_ui::measure_text_width(dc, &row.shortcut, TextStyle::Caption)
                        <= hint.right - hint.left
                );
            }
            assert_eq!(hint.left - label.right, theme.scale(24));
            assert!(hint.right < width - theme.scale(28)); // checkmark has its own slot
            unsafe {
                ReleaseDC(None, dc);
            }
        }
    }
}
