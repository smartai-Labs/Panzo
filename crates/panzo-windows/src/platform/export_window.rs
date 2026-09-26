//! In-window export panel. Configuration and progress are UI state only;
//! encoding remains on the existing `ExportTask` worker owned by the editor.
use super::editor_ui::{self, ButtonRole, ButtonVisualState, EditorTheme, TextStyle};
use super::export::ExportPreset;
use std::{collections::VecDeque, path::PathBuf};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DRAW_TEXT_FORMAT, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT,
    DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FillRect, HDC,
    InvalidateRect, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    BS_OWNERDRAW, CB_ADDSTRING, CB_GETCURSEL, CB_RESETCONTENT, CB_SETCURSEL, CBS_DROPDOWNLIST,
    CBS_HASSTRINGS, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClientRect,
    GetWindowLongPtrW, GetWindowTextW, HMENU, HWND_TOP, IDC_ARROW, LoadCursorW, RegisterClassW,
    SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW, SetWindowLongPtrW, SetWindowPos,
    SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_DESTROY,
    WM_DPICHANGED, WM_DRAWITEM, WM_ERASEBKGND, WM_PAINT, WM_SETFONT, WNDCLASSW, WS_CHILD,
    WS_CLIPCHILDREN, WS_EX_CONTROLPARENT, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};
use windows::core::{PCWSTR, w};

pub enum ExportAction {
    Start {
        output: PathBuf,
        preset: ExportPreset,
    },
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SnapshotMetadata {
    description: String,
    resolutions: [String; 2],
}

impl SnapshotMetadata {
    fn new(
        width: u32,
        height: u32,
        duration: &str,
        settings: &panzo_core::WorkbenchSettings,
    ) -> Self {
        let (hd_width, hd_height) = ExportPreset::Hd1080.dimensions((width, height), settings);
        Self {
            description: format!("{duration} · 当前编辑快照 · 无音频"),
            resolutions: [
                format!(
                    "{} · {width} × {height}",
                    if settings.canvas.size.is_some() {
                        "画布尺寸"
                    } else {
                        "原始尺寸"
                    }
                ),
                format!("1080p · {hd_width} × {hd_height}（等比容纳）"),
            ],
        }
    }
}

fn panel_rect(client: RECT, theme: EditorTheme) -> RECT {
    let margin = theme.scale(8);
    let available_width = (client.right - client.left - margin * 2).max(0);
    let top = (client.top + theme.metrics.toolbar_height + margin).min(client.bottom);
    let available_height = (client.bottom - top - margin).max(0);
    let width = theme.scale(460).min(available_width);
    let height = theme.scale(430).min(available_height);
    let left = (client.right - margin - width).max(client.left);
    let top = top + (available_height - height) / 2;
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

#[derive(Clone, Copy)]
struct SheetLayout {
    client: RECT,
    title: RECT,
    summary: RECT,
    name: RECT,
    description: RECT,
    resolution_label: RECT,
    combo: RECT,
    frame_label: RECT,
    frame_value: RECT,
    path_label: RECT,
    path: RECT,
    browse: RECT,
    status: RECT,
    progress: RECT,
    note: RECT,
    primary: RECT,
    cancel: RECT,
    dismiss: RECT,
}

impl SheetLayout {
    // Keep all dependent rows together so compact layouts share the same anchors.
    #[allow(clippy::too_many_lines)]
    fn new(client: RECT, theme: EditorTheme) -> Self {
        let s = |n| theme.scale(n);
        let compact = client.bottom - client.top < s(380);
        let dense = client.bottom - client.top < s(280);
        let hide_frame = client.bottom - client.top < s(220);
        let width = (client.right - client.left).max(0);
        let margin = s(if compact { 12 } else { 24 }).min(width / 4);
        let left = client.left + margin;
        let right = (client.right - margin).max(left);
        let r = |x, y, w, h| RECT {
            left: x,
            top: client.top + s(y),
            right: x + w,
            bottom: client.top + s(y + h),
        };
        let row = |y, h| r(left, y, right - left, h);
        let dismiss = r(
            (right - s(56)).max(left),
            if compact { 8 } else { 12 },
            s(56).min(right - left),
            28,
        );
        let input_left = (left + s(if compact { 92 } else { 116 })).min(right);
        let combo = r(
            input_left,
            if dense {
                54
            } else if compact {
                84
            } else {
                128
            },
            right - input_left,
            if dense { 28 } else { 32 },
        );
        let browse = r(
            (right - s(72)).max(left),
            if hide_frame {
                106
            } else if dense {
                126
            } else if compact {
                162
            } else {
                226
            },
            s(72).min(right - left),
            if compact { 28 } else { 32 },
        );
        let footer_bottom = (client.bottom - s(if compact { 8 } else { 16 })).max(client.top);
        let footer_top = (footer_bottom - s(if compact { 28 } else { 32 })).max(client.top);
        let primary_left = (right - s(116)).max(left);
        let cancel_right = (primary_left - s(8)).max(left);
        Self {
            client,
            title: RECT {
                right: (dismiss.left - s(8)).max(left),
                ..row(
                    if dense {
                        4
                    } else if compact {
                        8
                    } else {
                        12
                    },
                    if dense { 24 } else { 32 },
                )
            },
            summary: if dense {
                RECT::default()
            } else {
                row(if compact { 38 } else { 58 }, if compact { 36 } else { 52 })
            },
            name: if dense {
                RECT::default()
            } else {
                r(
                    left + s(10),
                    if compact { 39 } else { 62 },
                    (right - left - s(20)).max(0),
                    if compact { 17 } else { 22 },
                )
            },
            description: if dense {
                row(34, 17)
            } else {
                r(
                    left + s(10),
                    if compact { 56 } else { 86 },
                    (right - left - s(20)).max(0),
                    if compact { 17 } else { 20 },
                )
            },
            resolution_label: RECT {
                left,
                right: input_left - s(8),
                ..combo
            },
            combo,
            frame_label: if hide_frame {
                RECT::default()
            } else {
                RECT {
                    right: input_left - s(8),
                    ..row(
                        if dense {
                            86
                        } else if compact {
                            118
                        } else {
                            170
                        },
                        if dense { 18 } else { 24 },
                    )
                }
            },
            frame_value: if hide_frame {
                RECT::default()
            } else {
                r(
                    input_left,
                    if dense {
                        86
                    } else if compact {
                        118
                    } else {
                        170
                    },
                    right - input_left,
                    if dense { 18 } else { 24 },
                )
            },
            path_label: row(
                if hide_frame {
                    86
                } else if dense {
                    106
                } else if compact {
                    142
                } else {
                    206
                },
                18,
            ),
            path: RECT {
                left,
                right: (browse.left - s(8)).max(left),
                ..browse
            },
            browse,
            status: row(
                if hide_frame {
                    138
                } else if dense {
                    160
                } else if compact {
                    198
                } else {
                    274
                },
                if dense { 22 } else { 28 },
            ),
            progress: if hide_frame {
                RECT::default()
            } else {
                row(
                    if dense {
                        190
                    } else if compact {
                        232
                    } else {
                        314
                    },
                    5,
                )
            },
            note: if compact {
                RECT::default()
            } else {
                row(330, 24)
            },
            primary: RECT {
                left: primary_left,
                right,
                top: footer_top,
                bottom: footer_bottom,
            },
            cancel: RECT {
                left: (cancel_right - s(88)).max(left),
                right: cancel_right,
                top: footer_top,
                bottom: footer_bottom,
            },
            dismiss,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExportPhase {
    Idle,
    Starting,
    Running,
    Complete,
}
impl ExportPhase {
    fn is_busy(self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }
}
struct Sheet {
    hwnd: HWND,
    owner: HWND,
    theme: EditorTheme,
    name: String,
    description: String,
    combo: HWND,
    browse: HWND,
    primary: HWND,
    cancel: HWND,
    dismiss: HWND,
    output: Option<PathBuf>,
    phase: ExportPhase,
    cancelling: bool,
    closed: bool,
    visible: bool,
    metadata: SnapshotMetadata,
    progress: u32,
    status: String,
    actions: VecDeque<ExportAction>,
}
pub struct ExportPanel {
    state: Box<Sheet>,
}
impl ExportPanel {
    #[allow(clippy::too_many_lines)]
    pub fn open(
        owner: HWND,
        name: &str,
        width: u32,
        height: u32,
        duration: &str,
        settings: &panzo_core::WorkbenchSettings,
    ) -> windows::core::Result<Self> {
        let theme = EditorTheme::current(editor_ui::window_dpi(owner));
        let scale = |n| theme.scale(n);
        let module = unsafe { GetModuleHandleW(None)? };
        let class = w!("PanzoExportSheet");
        unsafe {
            RegisterClassW(&WNDCLASSW {
                hInstance: module.into(),
                lpszClassName: class,
                lpfnWndProc: Some(sheet_proc),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                ..Default::default()
            });
        }
        let mut owner_rect = RECT::default();
        unsafe {
            GetClientRect(owner, &raw mut owner_rect)?;
        }
        let bounds = panel_rect(owner_rect, theme);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_CONTROLPARENT,
                class,
                w!("导出视频"),
                WS_CHILD | WS_CLIPCHILDREN,
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                Some(owner),
                None,
                Some(module.into()),
                None,
            )?
        };
        let mut panel = Self {
            state: Box::new(Sheet {
                hwnd,
                owner,
                theme,
                name: name.into(),
                description: format!("{duration} · 当前编辑快照 · 无音频"),
                combo: HWND::default(),
                browse: HWND::default(),
                primary: HWND::default(),
                cancel: HWND::default(),
                dismiss: HWND::default(),
                output: None,
                phase: ExportPhase::Idle,
                cancelling: false,
                closed: false,
                visible: false,
                metadata: SnapshotMetadata::new(width, height, duration, settings),
                progress: 0,
                status: "选择导出位置后即可开始。原始素材不会被修改。".into(),
                actions: VecDeque::new(),
            }),
        };
        let make = |class: PCWSTR,
                    label: &str,
                    id: usize,
                    x,
                    y,
                    width,
                    height,
                    style|
         -> windows::core::Result<HWND> {
            let value = wide(label);
            let child = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    class,
                    PCWSTR(value.as_ptr()),
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | style,
                    scale(x),
                    scale(y),
                    scale(width),
                    scale(height),
                    Some(hwnd),
                    Some(HMENU(id as *mut _)),
                    Some(module.into()),
                    None,
                )?
            };
            unsafe {
                SendMessageW(
                    child,
                    WM_SETFONT,
                    Some(WPARAM(
                        editor_ui::font_handle(TextStyle::Body, theme.dpi).0 as usize,
                    )),
                    Some(LPARAM(1)),
                );
            }
            Ok(child)
        };
        panel.state.combo = make(
            w!("COMBOBOX"),
            "分辨率",
            10,
            144,
            144,
            288,
            180,
            WS_VSCROLL | WINDOW_STYLE((CBS_DROPDOWNLIST | CBS_HASSTRINGS) as u32),
        )?;
        for label in &panel.state.metadata.resolutions {
            let value = wide(label);
            unsafe {
                SendMessageW(
                    panel.state.combo,
                    CB_ADDSTRING,
                    None,
                    Some(LPARAM(value.as_ptr() as isize)),
                );
            }
        }
        unsafe {
            SendMessageW(panel.state.combo, CB_SETCURSEL, Some(WPARAM(0)), None);
        }
        panel.state.browse = make(
            w!("BUTTON"),
            "选择…",
            11,
            352,
            238,
            80,
            32,
            WINDOW_STYLE(BS_OWNERDRAW as u32),
        )?;
        panel.state.cancel = make(
            w!("BUTTON"),
            "取消导出",
            12,
            218,
            414,
            88,
            32,
            WINDOW_STYLE(BS_OWNERDRAW as u32),
        )?;
        panel.state.primary = make(
            w!("BUTTON"),
            "导出视频",
            1,
            316,
            414,
            116,
            32,
            WINDOW_STYLE(BS_OWNERDRAW as u32),
        )?;
        panel.state.dismiss = make(
            w!("BUTTON"),
            "收起",
            3,
            376,
            12,
            56,
            28,
            WINDOW_STYLE(BS_OWNERDRAW as u32),
        )?;
        unsafe {
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                std::ptr::from_mut(&mut *panel.state) as isize,
            );
            let _ = EnableWindow(panel.state.primary, false);
        }
        panel.relayout(owner);
        panel.state.update_controls();
        panel.show();
        Ok(panel)
    }
    pub fn hwnd(&self) -> HWND {
        self.state.hwnd
    }
    pub fn is_closed(&self) -> bool {
        self.state.closed
    }
    pub fn is_visible(&self) -> bool {
        self.state.visible && !self.state.closed
    }
    pub fn show(&mut self) {
        if !self.state.closed {
            if self.state.phase == ExportPhase::Complete {
                self.state.phase = ExportPhase::Idle;
                self.state.progress = 0;
                self.state.status = "选择导出位置后即可开始。原始素材不会被修改。".into();
                self.state.update_controls();
            }
            self.state.visible = true;
            unsafe {
                let _ = ShowWindow(self.state.hwnd, SW_SHOW);
                let _ = SetWindowPos(
                    self.state.hwnd,
                    Some(HWND_TOP),
                    0,
                    0,
                    0,
                    0,
                    windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                        | SWP_NOACTIVATE,
                );
                let _ = SetFocus(Some(if self.state.phase.is_busy() {
                    self.state.dismiss
                } else {
                    self.state.combo
                }));
            }
        }
    }
    pub fn relayout(&mut self, owner: HWND) {
        self.state.owner = owner;
        self.state.theme = EditorTheme::current(editor_ui::window_dpi(owner));
        self.state.relayout();
    }
    pub fn refresh_snapshot(
        &mut self,
        width: u32,
        height: u32,
        duration: &str,
        settings: &panzo_core::WorkbenchSettings,
    ) {
        let metadata = SnapshotMetadata::new(width, height, duration, settings);
        if matches!(
            self.state.phase,
            ExportPhase::Running | ExportPhase::Complete
        ) || self.state.metadata == metadata
        {
            return;
        }
        self.state.description.clone_from(&metadata.description);
        self.state.metadata = metadata;
        let selection =
            usize::try_from(unsafe { SendMessageW(self.state.combo, CB_GETCURSEL, None, None) }.0)
                .unwrap_or(0);
        unsafe {
            SendMessageW(self.state.combo, CB_RESETCONTENT, None, None);
        }
        for label in &self.state.metadata.resolutions {
            let value = wide(label);
            unsafe {
                SendMessageW(
                    self.state.combo,
                    CB_ADDSTRING,
                    None,
                    Some(LPARAM(value.as_ptr() as isize)),
                );
            }
        }
        unsafe {
            SendMessageW(
                self.state.combo,
                CB_SETCURSEL,
                Some(WPARAM(selection)),
                None,
            );
        }
        self.state.refresh();
    }
    pub fn mark_started(&mut self) {
        self.state.phase = ExportPhase::Running;
        self.state.update_controls();
        self.state.refresh();
    }
    pub fn action(&mut self) -> Option<ExportAction> {
        editor_ui::sync_window_theme(&mut self.state.theme, self.state.hwnd);
        self.state.actions.pop_front()
    }
    pub fn progress(&mut self, progress: u32) {
        if progress != self.state.progress {
            self.state.progress = progress;
            if !self.state.cancelling {
                self.state.status = format!("正在导出 {progress}% · 可以取消");
            }
            self.state.refresh();
        }
    }
    pub fn finish(&mut self, success: bool, message: &str) {
        self.state.cancelling = false;
        self.state.phase = if success {
            ExportPhase::Complete
        } else {
            ExportPhase::Idle
        };
        self.state.status = message.into();
        if success {
            self.state.progress = 100;
        }
        self.state.update_controls();
        self.state.refresh();
    }
}
impl Drop for ExportPanel {
    fn drop(&mut self) {
        if !self.state.closed {
            unsafe {
                SetWindowLongPtrW(self.state.hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(self.state.hwnd);
            }
        }
    }
}
impl Sheet {
    fn layout(&self) -> SheetLayout {
        let mut client = RECT::default();
        let _ = unsafe { GetClientRect(self.hwnd, &raw mut client) };
        SheetLayout::new(client, self.theme)
    }
    fn relayout(&self) {
        let mut client = RECT::default();
        if unsafe { GetClientRect(self.owner, &raw mut client) }.is_err() {
            return;
        }
        let rect = panel_rect(client, self.theme);
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOP),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOACTIVATE,
            );
        }
        self.resize_controls();
        self.refresh();
    }
    fn resize_controls(&self) {
        let layout = self.layout();
        for (child, rect) in [
            (
                self.combo,
                RECT {
                    bottom: layout.combo.top + self.theme.scale(180),
                    ..layout.combo
                },
            ),
            (self.browse, layout.browse),
            (self.cancel, layout.cancel),
            (self.primary, layout.primary),
            (self.dismiss, layout.dismiss),
        ] {
            unsafe {
                let _ = SetWindowPos(
                    child,
                    None,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
                SendMessageW(
                    child,
                    WM_SETFONT,
                    Some(WPARAM(
                        editor_ui::font_handle(TextStyle::Body, self.theme.dpi).0 as usize,
                    )),
                    Some(LPARAM(1)),
                );
            }
        }
    }
    fn refresh(&self) {
        if !self.closed {
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
            }
        }
    }
    fn update_controls(&self) {
        let busy = self.phase.is_busy();
        let complete = self.phase == ExportPhase::Complete;
        unsafe {
            let _ = EnableWindow(self.cancel, busy && !self.cancelling);
            let _ = EnableWindow(self.combo, !busy && !complete);
            let _ = EnableWindow(self.browse, !busy && !complete);
            let _ = EnableWindow(self.primary, !busy && (self.output.is_some() || complete));
            let _ = SetWindowTextW(
                self.primary,
                if complete {
                    w!("完成")
                } else {
                    w!("导出视频")
                },
            );
            let _ = SetWindowTextW(self.cancel, w!("取消导出"));
        }
    }
    fn hide(&mut self) {
        self.visible = false;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let _ = SetFocus(Some(self.owner));
        }
    }
    fn cancel_export(&mut self) {
        if self.phase.is_busy() && !self.cancelling {
            if !matches!(self.actions.back(), Some(ExportAction::Cancel)) {
                self.actions.push_back(ExportAction::Cancel);
            }
            self.cancelling = true;
            self.update_controls();
            self.status = "正在取消导出…".into();
            self.refresh();
        }
    }
    fn command(&mut self, id: usize) {
        match id {
            1 if self.phase == ExportPhase::Complete => self.hide(),
            1 if self.phase == ExportPhase::Idle => {
                if let Some(output) = self.output.clone() {
                    let selected = unsafe { SendMessageW(self.combo, CB_GETCURSEL, None, None) }.0;
                    self.actions.push_back(ExportAction::Start {
                        output,
                        preset: if selected == 1 {
                            ExportPreset::Hd1080
                        } else {
                            ExportPreset::Native
                        },
                    });
                    self.phase = ExportPhase::Starting;
                    self.cancelling = false;
                    self.progress = 0;
                    self.status = "正在准备导出…".into();
                    self.update_controls();
                    self.refresh();
                }
            }
            12 => self.cancel_export(),
            2 | 3 => self.hide(),
            11 if self.phase == ExportPhase::Idle => {
                if let Some(path) = choose_path(self.hwnd, &self.name) {
                    self.output = Some(path);
                    self.update_controls();
                    self.refresh();
                }
            }
            _ => {}
        }
    }
    #[allow(clippy::too_many_lines)]
    fn paint(&self, dc: HDC) {
        let theme = self.theme;
        let palette = theme.palette;
        let scale = |n| theme.scale(n);
        let _drawing = editor_ui::DrawingScale::enter(theme.dpi);
        let layout = self.layout();
        fill(dc, layout.client, palette.panel);
        text(dc, layout.title, "导出视频", palette.text, TextStyle::Title);
        editor_ui::rounded_surface(dc, layout.summary, palette.window, palette.window, scale(8));
        text(
            dc,
            layout.name,
            &self.name,
            palette.text,
            TextStyle::Heading,
        );
        text(
            dc,
            layout.description,
            &self.description,
            palette.text_muted,
            TextStyle::Caption,
        );
        text(
            dc,
            layout.resolution_label,
            "分辨率",
            palette.text_secondary,
            TextStyle::Body,
        );
        text(
            dc,
            layout.frame_label,
            "帧率",
            palette.text_secondary,
            TextStyle::Body,
        );
        text(
            dc,
            layout.frame_value,
            "60 FPS · H.264 / MP4",
            palette.text,
            TextStyle::Body,
        );
        text(
            dc,
            layout.path_label,
            "导出位置",
            palette.text_muted,
            TextStyle::Caption,
        );
        editor_ui::rounded_surface(dc, layout.path, palette.window, palette.divider, scale(6));
        text(
            dc,
            RECT {
                left: layout.path.left + scale(10),
                right: layout.path.right - scale(10),
                ..layout.path
            },
            &self.output.as_ref().map_or("尚未选择文件位置".into(), |v| {
                v.display().to_string()
            }),
            palette.text_secondary,
            TextStyle::Body,
        );
        text(
            dc,
            layout.status,
            &self.status,
            if self.phase == ExportPhase::Complete {
                palette.accent
            } else {
                palette.text_secondary
            },
            TextStyle::Body,
        );
        editor_ui::rounded_surface(
            dc,
            layout.progress,
            palette.divider,
            palette.divider,
            scale(3),
        );
        if self.progress > 0 {
            let rail = RECT {
                right: layout.progress.left
                    + (layout.progress.right - layout.progress.left)
                        * self.progress.min(100).cast_signed()
                        / 100,
                ..layout.progress
            };
            editor_ui::rounded_surface(dc, rail, palette.accent, palette.accent, scale(3));
        }
        text(
            dc,
            layout.note,
            if self.phase == ExportPhase::Complete {
                "导出完成。文件夹菜单可查看视频。"
            } else {
                "仅导出视频画面，不包含音频。"
            },
            palette.text_muted,
            TextStyle::Caption,
        );
    }
}
unsafe extern "system" fn sheet_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Sheet;
    if let Some(sheet) = unsafe { pointer.as_mut() } {
        match message {
            WM_DPICHANGED => {
                sheet.theme = EditorTheme::current(editor_ui::window_dpi(sheet.owner));
                sheet.relayout();
                return LRESULT(0);
            }
            WM_COMMAND => {
                if wp.0 >> 16 == 0 {
                    sheet.command(wp.0 & 0xffff);
                }
                return LRESULT(0);
            }
            WM_CLOSE => {
                sheet.hide();
                return LRESULT(0);
            }
            WM_DESTROY => {
                sheet.closed = true;
                sheet.visible = false;
                return LRESULT(0);
            }
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = unsafe { BeginPaint(hwnd, &raw mut paint) };
                sheet.paint(dc);
                unsafe {
                    let _ = EndPaint(hwnd, &raw const paint);
                }
                return LRESULT(0);
            }
            WM_DRAWITEM => {
                let item = unsafe { &*(lp.0 as *const DRAWITEMSTRUCT) };
                let _drawing = editor_ui::DrawingScale::enter(sheet.theme.dpi);
                let (bg, stroke, fg) = editor_ui::button_colors(
                    sheet.theme,
                    if item.CtlID == 1 {
                        ButtonRole::Primary
                    } else {
                        ButtonRole::Standard
                    },
                    ButtonVisualState {
                        interaction: editor_ui::button_interaction(
                            item.itemState.0 & ODS_DISABLED.0 == 0,
                            item.itemState.0 & ODS_SELECTED.0 != 0,
                            false,
                        ),
                        selected: false,
                    },
                );
                editor_ui::rounded_surface(item.hDC, item.rcItem, bg, stroke, sheet.theme.scale(7));
                let mut label = [0u16; 96];
                let length = usize::try_from(unsafe { GetWindowTextW(item.hwndItem, &mut label) })
                    .unwrap_or(0);
                text_center(
                    item.hDC,
                    item.rcItem,
                    &String::from_utf16_lossy(&label[..length]),
                    fg,
                    TextStyle::Button,
                );
                if item.itemState.0 & ODS_FOCUS.0 != 0 {
                    editor_ui::focus_ring(
                        item.hDC,
                        item.rcItem,
                        sheet.theme.palette.accent,
                        sheet.theme.scale(7),
                    );
                }
                return LRESULT(1);
            }
            WM_ERASEBKGND => return LRESULT(1),
            _ => {}
        }
    }
    unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn fill(dc: HDC, r: RECT, color: windows::Win32::Foundation::COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(dc, &raw const r, brush);
        let _ = DeleteObject(brush.into());
    }
}
fn text(
    dc: HDC,
    r: RECT,
    value: &str,
    color: windows::Win32::Foundation::COLORREF,
    style: TextStyle,
) {
    draw_text(dc, r, value, color, style, DT_LEFT);
}
fn text_center(
    dc: HDC,
    r: RECT,
    value: &str,
    color: windows::Win32::Foundation::COLORREF,
    style: TextStyle,
) {
    draw_text(dc, r, value, color, style, DT_CENTER);
}
fn draw_text(
    dc: HDC,
    mut r: RECT,
    value: &str,
    color: windows::Win32::Foundation::COLORREF,
    style: TextStyle,
    align: DRAW_TEXT_FORMAT,
) {
    if r.right <= r.left || r.bottom <= r.top || value.is_empty() {
        return;
    }
    let old = editor_ui::select_font(dc, style);
    let mut value: Vec<u16> = value.encode_utf16().collect();
    unsafe {
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, color);
        DrawTextW(
            dc,
            &mut value,
            &raw mut r,
            align | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(dc, old);
    }
}
fn choose_path(owner: HWND, name: &str) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::UI::Controls::Dialogs::{
        GetSaveFileNameW, OFN_EXPLORER, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    let mut buffer = vec![0u16; 32768];
    let initial = wide(&format!("{name}-edited.mp4"));
    let n = initial.len().min(buffer.len() - 1);
    buffer[..n].copy_from_slice(&initial[..n]);
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: w!("MP4 视频\0*.mp4\0\0"),
        lpstrDefExt: w!("mp4"),
        lpstrFile: windows::core::PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        Flags: OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if unsafe { GetSaveFileNameW(&raw mut dialog) }.as_bool() {
        let n = buffer.iter().position(|x| *x == 0).unwrap_or(buffer.len());
        Some(std::ffi::OsString::from_wide(&buffer[..n]).into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> ExportPanel {
        ExportPanel {
            state: Box::new(Sheet {
                hwnd: HWND::default(),
                owner: HWND::default(),
                theme: EditorTheme::light(),
                name: "测试工程".into(),
                description: "00:30.000 · 当前编辑快照 · 无音频".into(),
                combo: HWND::default(),
                browse: HWND::default(),
                primary: HWND::default(),
                cancel: HWND::default(),
                dismiss: HWND::default(),
                output: Some("test-edited.mp4".into()),
                phase: ExportPhase::Idle,
                cancelling: false,
                closed: false,
                visible: true,
                metadata: SnapshotMetadata::new(
                    1920,
                    1080,
                    "00:30.000",
                    &panzo_core::WorkbenchSettings::default(),
                ),
                progress: 0,
                status: String::new(),
                actions: VecDeque::new(),
            }),
        }
    }

    #[test]
    fn hiding_a_running_export_keeps_it_running_and_only_explicit_cancel_queues_cancellation() {
        let mut panel = fixture();
        panel.mark_started();
        panel.state.command(2); // Escape from IsDialogMessage is IDCANCEL, not the cancel-export button.
        assert!(!panel.is_visible());
        assert!(!panel.is_closed());
        assert_eq!(panel.state.phase, ExportPhase::Running);
        assert!(panel.action().is_none());
        panel.state.command(12);
        panel.state.command(12);
        assert!(matches!(panel.action(), Some(ExportAction::Cancel)));
        assert!(panel.action().is_none());
        panel.progress(38);
        assert_eq!(panel.state.status, "正在取消导出…");
    }

    #[test]
    fn pending_start_accepts_committed_metadata_then_freezes_it_until_export_finishes() {
        let mut panel = fixture();
        panel.state.command(1);
        panel.state.command(1);
        assert_eq!(panel.state.phase, ExportPhase::Starting);
        assert!(matches!(panel.action(), Some(ExportAction::Start { .. })));
        assert!(panel.action().is_none());
        let settings = panzo_core::WorkbenchSettings::default();
        panel.refresh_snapshot(1080, 1920, "00:12.000", &settings);
        let displayed = panel.state.metadata.clone();
        assert!(displayed.resolutions[0].contains("1080 × 1920"));
        assert!(displayed.description.starts_with("00:12.000"));
        panel.mark_started();
        panel.refresh_snapshot(640, 640, "00:05.000", &settings);
        assert_eq!(panel.state.metadata, displayed);
        panel.finish(true, "导出完成");
        panel.refresh_snapshot(640, 640, "00:05.000", &settings);
        assert_eq!(panel.state.metadata, displayed);
    }

    #[test]
    fn embedded_export_fits_short_physical_clients_and_stays_below_the_shared_header() {
        for dpi in [96, 120, 144, 192] {
            let theme = EditorTheme::for_dpi(dpi);
            for (width, height) in [(800, 600), (1280, 720), (1280, 900), (1920, 1080)] {
                let owner = RECT {
                    right: width,
                    bottom: height,
                    ..RECT::default()
                };
                let panel = panel_rect(owner, theme);
                assert!(panel.top >= theme.metrics.toolbar_height);
                assert!(
                    panel.left >= 0 && panel.right <= owner.right && panel.bottom <= owner.bottom
                );
                let layout = SheetLayout::new(
                    RECT {
                        right: panel.right - panel.left,
                        bottom: panel.bottom - panel.top,
                        ..RECT::default()
                    },
                    theme,
                );
                let controls = [
                    layout.dismiss,
                    layout.combo,
                    layout.browse,
                    layout.cancel,
                    layout.primary,
                ];
                for (index, rect) in controls.iter().enumerate() {
                    assert!(
                        rect.left >= 0
                            && rect.top >= 0
                            && rect.right <= layout.client.right
                            && rect.bottom <= layout.client.bottom,
                        "{width}x{height} dpi={dpi}"
                    );
                    for other in &controls[index + 1..] {
                        assert!(
                            rect.right <= other.left
                                || other.right <= rect.left
                                || rect.bottom <= other.top
                                || other.bottom <= rect.top
                        );
                    }
                }
                assert!(layout.status.bottom <= layout.primary.top);
                assert!(layout.progress.bottom <= layout.primary.top);
            }
        }
    }
}
