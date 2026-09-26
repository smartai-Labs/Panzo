//! A themed native popup for existing application commands. Windows owns navigation/dismissal.
use super::editor_ui::{self, DrawingScale, EditorTheme, IconKind, TextStyle};
use std::cell::RefCell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DrawTextW, SelectObject,
    SetBkMode, SetTextColor, TRANSPARENT,
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
    NewRecording,
    Open,
    ContinueRecent,
    Save,
    Export,
    OpenLocation,
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

#[derive(Clone, Copy)]
pub(crate) struct Row {
    label: &'static str,
    icon: Option<IconKind>,
    command: Option<i32>,
    enabled: bool,
    checked: bool,
}

impl Row {
    const fn text_style(self) -> TextStyle {
        if self.command.is_some() {
            TextStyle::Button
        } else {
            TextStyle::Heading
        }
    }

    pub(crate) const fn command(
        command: i32,
        label: &'static str,
        icon: IconKind,
        enabled: bool,
    ) -> Self {
        Self {
            label,
            icon: Some(icon),
            command: Some(command),
            enabled,
            checked: false,
        }
    }

    pub(crate) const fn heading(label: &'static str) -> Self {
        Self {
            label,
            icon: None,
            command: None,
            enabled: false,
            checked: false,
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
        Row::command(1, "录制新视频", IconKind::Record, available.new_recording),
        Row::command(2, "打开工程…", IconKind::Folder, available.open),
        Row::command(3, "继续最近工程", IconKind::Restore, available.recent),
        Row::separator(),
        Row::command(4, "保存", IconKind::Save, available.save),
        Row::command(5, "导出…", IconKind::Export, available.export),
        Row::separator(),
        Row::command(6, "打开文件位置", IconKind::Folder, true),
    ];
    Ok(
        show_rows(hwnd, anchor, theme, rows, false)?.and_then(|command| match command {
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

pub(crate) fn theme_rows(theme: EditorTheme) -> Vec<Row> {
    use super::ui_preferences::ThemeMode;
    let mut light = Row::command(2501, "浅色主题", IconKind::Background, true);
    light.checked = theme.mode == ThemeMode::Light;
    let mut dark = Row::command(2502, "深色主题", IconKind::Background, true);
    dark.checked = theme.mode == ThemeMode::Dark;
    vec![Row::heading("主题"), light, dark, Row::separator()]
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
        .map(|row| row.label.encode_utf16().chain([0]).collect())
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
    let width = menu_width(hwnd, &rows, theme);
    ACTIVE.with(|active| *active.borrow_mut() = Some(MenuView { rows, theme, width }));
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

fn menu_width(hwnd: HWND, rows: &[Row], theme: EditorTheme) -> i32 {
    use windows::Win32::Graphics::Gdi::{GetDC, GetTextExtentPoint32W, ReleaseDC};
    let _scale = DrawingScale::enter(theme.dpi);
    let dc = unsafe { GetDC(Some(hwnd)) };
    if dc.is_invalid() {
        return theme.scale(280);
    }
    let old = editor_ui::select_font(dc, TextStyle::Button);
    let width = rows
        .iter()
        .map(|row| {
            editor_ui::select_font(dc, row.text_style());
            let text: Vec<u16> = row.label.encode_utf16().collect();
            let mut size = windows::Win32::Foundation::SIZE::default();
            unsafe {
                let _ = GetTextExtentPoint32W(dc, &text, &raw mut size);
            }
            size.cx + theme.scale(72)
        })
        .max()
        .unwrap_or(0)
        .max(theme.scale(224));
    unsafe {
        SelectObject(dc, old);
        ReleaseDC(Some(hwnd), dc);
    }
    width
}

fn row(id: u32) -> Option<(Row, EditorTheme)> {
    let index = id.checked_sub(FIRST_ID)? as usize;
    ACTIVE.with(|active| {
        active
            .borrow()
            .as_ref()
            .and_then(|view| view.rows.get(index).map(|row| (*row, view.theme)))
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
                    34
                } else if row.label.is_empty() {
                    9
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
    editor_ui::fill_rect(dc, &rect, theme.palette.panel);
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
                left: rect.left + s(4),
                right: rect.right - s(4),
                ..rect
            },
            theme.palette.hover,
            theme.palette.hover,
            s(4),
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
            IconKind::Record,
            color,
        );
    }
    let mut text_rect = RECT {
        left: rect.left + s(if row.command.is_some() { 42 } else { 14 }),
        right: rect.right - s(36),
        ..rect
    };
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
