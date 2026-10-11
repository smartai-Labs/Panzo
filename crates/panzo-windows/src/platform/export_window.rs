//! In-window export panel. Configuration and progress are UI state only;
//! encoding remains on the existing `ExportTask` worker owned by the editor.
use super::editor_ui::{self, ButtonRole, ButtonVisualState, EditorTheme, TextStyle};
use super::export::{ExportFormat, ExportOptions, ExportPreset};
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
    SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SendMessageW, SetWindowLongPtrW, SetWindowPos,
    SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_DESTROY,
    WM_DPICHANGED, WM_DRAWITEM, WM_ERASEBKGND, WM_PAINT, WM_SETFONT, WNDCLASSW, WS_CHILD,
    WS_CLIPCHILDREN, WS_EX_CONTROLPARENT, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};
use windows::core::{PCWSTR, w};

pub enum ExportAction {
    Start {
        output: PathBuf,
        preset: ExportPreset,
        options: ExportOptions,
    },
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SnapshotMetadata {
    description: String,
    resolutions: [String; 4],
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
            description: format!("{duration} · 当前编辑快照"),
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
                "分享尺寸 · 宽度不超过 1280".into(),
                "紧凑尺寸 · 宽度不超过 640".into(),
            ],
        }
    }
}

fn panel_rect(client: RECT, theme: EditorTheme, expanded:bool) -> RECT {
    let margin = theme.scale(8);
    let available_width = (client.right - client.left - margin * 2).max(0);
    let top = (client.top + theme.metrics.toolbar_height + margin).min(client.bottom);
    let available_height = (client.bottom - top - margin).max(0);
    let width = theme.scale(460).min(available_width);
    let height = theme.scale(if expanded{576}else{408}).min(available_height);
    let left = client.left+(client.right-client.left-width)/2;
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
    client:RECT,title:RECT,body:RECT,status:RECT,progress:RECT,primary:RECT,cancel:RECT,dismiss:RECT,
}
impl SheetLayout {
    fn new(client:RECT,theme:EditorTheme)->Self {
        let s=|n|theme.scale(n);let margin=s(20).min(client.right/4);
        let left=margin;let right=client.right-margin;let bottom=client.bottom-s(16);
        let footer=bottom-s(28);let status_top=footer-s(52);
        Self {client,
            title:RECT{left,right:right-s(32),top:s(14),bottom:s(38)},
            dismiss:RECT{left:right-s(24),right,top:s(14),bottom:s(38)},
            body:RECT{left:0,right:client.right,top:s(54),bottom:status_top-s(10)},
            status:RECT{left,right,top:status_top,bottom:status_top+s(28)},
            progress:RECT{left,right,top:status_top+s(34),bottom:status_top+s(37)},
            primary:RECT{left:right-s(104),right,top:footer,bottom},
            cancel:RECT{left:right-s(192),right:right-s(112),top:footer,bottom},
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
    format_combo: HWND,
    fps_combo: HWND,
    advanced: HWND,
    body: HWND,
    advanced_fields: [HWND;4],
    expanded: bool,
    scroll: i32,
    extra_options: ExportOptions,
    browse: HWND,
    directory: HWND,
    filename: HWND,
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
        let bounds = panel_rect(owner_rect, theme,false);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_CONTROLPARENT,
                class,
                w!("导出视频"),
                WS_CHILD | WS_CLIPCHILDREN | windows::Win32::UI::WindowsAndMessaging::WS_CLIPSIBLINGS,
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
                description: format!("{duration} · 当前编辑快照"),
                combo: HWND::default(),
                format_combo: HWND::default(),
                fps_combo: HWND::default(),
                advanced: HWND::default(),
                body: HWND::default(),
                advanced_fields:[HWND::default();4],
                expanded:false,scroll:0,
                extra_options: ExportOptions::default(),
                browse: HWND::default(),
                directory: HWND::default(),
                filename: HWND::default(),
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
            WS_VSCROLL
                | WINDOW_STYLE(
                    (CBS_DROPDOWNLIST
                        | CBS_HASSTRINGS
                        | windows::Win32::UI::WindowsAndMessaging::CBS_OWNERDRAWFIXED)
                        as u32,
                ),
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
        panel.state.format_combo = make(
            w!("COMBOBOX"),
            "格式与帧率",
            14,
            144,
            170,
            288,
            220,
            WS_VSCROLL
                | WINDOW_STYLE(
                    (CBS_DROPDOWNLIST
                        | CBS_HASSTRINGS
                        | windows::Win32::UI::WindowsAndMessaging::CBS_OWNERDRAWFIXED)
                        as u32,
                ),
        )?;
        for label in ["MP4", "GIF"] {
            let value = wide(label);
            unsafe {
                SendMessageW(
                    panel.state.format_combo,
                    CB_ADDSTRING,
                    None,
                    Some(LPARAM(value.as_ptr() as isize)),
                );
            }
        }
        unsafe {
            SendMessageW(
                panel.state.format_combo,
                CB_SETCURSEL,
                Some(WPARAM(0)),
                None,
            );
        }
        panel.state.fps_combo=make(w!("COMBOBOX"),"帧率",16,144,208,288,220,WS_VSCROLL|WINDOW_STYLE((CBS_DROPDOWNLIST|CBS_HASSTRINGS|windows::Win32::UI::WindowsAndMessaging::CBS_OWNERDRAWFIXED)as u32))?;
        panel.state.update_fps_choices();
        panel.state.advanced = make(
            w!("BUTTON"),
            "高级设置",
            15,
            144,
            144,
            288,
            32,
            WINDOW_STYLE(BS_OWNERDRAW as u32),
        )?;
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
        crate::platform::desktop_ui::install(hwnd);
        for combo in [panel.state.combo, panel.state.format_combo, panel.state.fps_combo] {
            crate::platform::desktop_ui::combo(combo);
        }
        for button in [
            panel.state.advanced,
            panel.state.cancel,
            panel.state.primary,
            panel.state.dismiss,
            panel.state.browse,
        ] {
            crate::platform::desktop_ui::button(button);
        }
        let folder = crate::platform::ui_preferences::output_directory().unwrap_or_else(crate::platform::workspace::library_path);
        let filename = format!(
            "{}-{}.mp4",
            name.chars()
                .map(
                    |c| if ['<', '>', ':', '"', '/', '\\', '|', '?', '*'].contains(&c) {
                        '_'
                    } else {
                        c
                    }
                )
                .collect::<String>(),
            &uuid::Uuid::new_v4().to_string()[..8]
        );
        panel.state.directory =
            crate::platform::path_field::create(hwnd, &folder.display().to_string(), 18)
                .map_err(|e| windows::core::Error::new(windows::Win32::Foundation::E_FAIL, e))?;
        panel.state.filename = crate::platform::path_field::filename(hwnd, &filename, 17)
            .map_err(|e| windows::core::Error::new(windows::Win32::Foundation::E_FAIL, e))?;
        panel.state.output = Some(folder.join(filename));
        use super::{workspace_ui as ui,desktop_ui as desk};
        let err=|e:String|windows::core::Error::new(windows::Win32::Foundation::E_FAIL,e);
        ui::register(w!("PanzoExportFields"),Some(fields_proc)).map_err(err)?;
        panel.state.body=ui::child(hwnd,w!("PanzoExportFields"),"",40,WS_CLIPCHILDREN).map_err(err)?;
        desk::install(panel.state.body);
        panel.state.advanced_fields=[
            ui::edit(panel.state.body,"0.000",21,false).map_err(err)?,
            ui::edit(panel.state.body,"",22,false).map_err(err)?,
            ui::combo(panel.state.body,&["开启","关闭"],0,23).map_err(err)?,
            ui::combo(panel.state.body,&["清晰","均衡","小体积"],1,24).map_err(err)?,
        ];
        super::desktop_ui::decimal_field(panel.state.advanced_fields[0],0.,86400.,0.1);
        super::desktop_ui::decimal_field(panel.state.advanced_fields[1],0.,86400.,0.1);
        unsafe {
            SendMessageW(panel.state.advanced_fields[1],0x1501,Some(WPARAM(0)),Some(LPARAM(w!("留空为完整视频").as_ptr() as isize)));
            SetWindowLongPtrW(panel.state.body,GWLP_USERDATA,std::ptr::from_mut(&mut *panel.state) as isize);
            for child in [panel.state.directory,panel.state.filename,panel.state.combo,panel.state.fps_combo,panel.state.format_combo,panel.state.advanced] {
                let _=windows::Win32::UI::WindowsAndMessaging::SetParent(child,Some(panel.state.body));
            }
        }
        super::path_field::compact(panel.state.directory);super::path_field::compact(panel.state.filename);
        desk::style(panel.state.advanced,Some(editor_ui::IconKind::ChevronRight),true,false,false);
        desk::style(panel.state.primary,None,false,true,false);
        desk::style(panel.state.dismiss,Some(editor_ui::IconKind::Close),true,false,true);

        crate::platform::workspace_ui::show(panel.state.browse, false);
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
    fn relayout(&mut self) {
        let mut client = RECT::default();
        if unsafe { GetClientRect(self.owner, &raw mut client) }.is_err() {
            return;
        }
        let rect = panel_rect(client, self.theme,self.expanded);
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
    fn resize_controls(&mut self) {
        let layout=self.layout();let d=|n|self.theme.scale(n);
        for (h,r) in [(self.body,layout.body),(self.primary,layout.primary),(self.cancel,layout.cancel),(self.dismiss,layout.dismiss)] {
            super::workspace_ui::place(h,r.left,r.top,(r.right-r.left).max(0),(r.bottom-r.top).max(0));
        }
        let page=(layout.body.bottom-layout.body.top).max(0);
        let total=d(if self.expanded{412}else{242});
        self.scroll=self.scroll.clamp(0,(total-page).max(0));
        super::desktop_ui::scroll_panel(self.body,total,page,self.scroll);
        let width=super::workspace_ui::rect(self.body).right;
        let left=d(20);let right=width-d(20);let input=(left+d(90)).min(right);
        let x=(right-input).max(0);
        for (i,h) in [self.filename,self.directory,self.combo,self.fps_combo,self.format_combo].into_iter().enumerate() {
            let y=d(i as i32*40)-self.scroll;
            let height=d(28);
            super::workspace_ui::place(h,input,y,x,height);
        }
        super::workspace_ui::place(self.advanced,left,d(200)-self.scroll,(right-left).max(0),d(30));
        for (i,h) in self.advanced_fields.iter().enumerate() {
            let y=d(244+i as i32*40)-self.scroll;
            super::workspace_ui::place(*h,input,y,x,d(28));
            super::workspace_ui::show(*h,self.expanded);
        }
        for h in [self.primary,self.cancel,self.dismiss,self.advanced,self.filename,self.directory] {
            unsafe{SendMessageW(h,WM_SETFONT,Some(WPARAM(editor_ui::font_handle(TextStyle::Body,self.theme.dpi).0 as usize)),Some(LPARAM(1)));}
        }
        super::desktop_ui::style(self.advanced,Some(if self.expanded{editor_ui::IconKind::ChevronDown}else{editor_ui::IconKind::ChevronRight}),true,false,false);
        super::desktop_ui::invalidate(self.body);
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
        for h in self.advanced_fields {unsafe{let _=EnableWindow(h,!busy&&!complete);}}
        unsafe {
            let _ = EnableWindow(self.cancel, !self.cancelling);
            let _ = EnableWindow(self.combo, !busy && !complete);
            let _ = EnableWindow(self.advanced, !busy && !complete);
            let _ = EnableWindow(self.format_combo, !busy && !complete);
            let _ = EnableWindow(self.fps_combo, !busy && !complete);
            let _ = EnableWindow(self.browse, !busy && !complete);
            let _ = EnableWindow(self.directory, !busy && !complete);
            let _ = EnableWindow(self.filename, !busy && !complete);
            let _ = EnableWindow(self.primary, !busy && (self.output.is_some() || complete));
            let _ = SetWindowTextW(
                self.primary,
                if complete {
                    w!("完成")
                } else {
                    w!("导出视频")
                },
            );
            let _ = SetWindowTextW(self.cancel, w!("取消"));
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
    fn update_fps_choices(&self) {
        let gif=unsafe{SendMessageW(self.format_combo,CB_GETCURSEL,None,None)}.0==1;
        let previous=crate::platform::workspace_ui::value(self.fps_combo);
        let values:&[&str]=if gif {&["30 fps","25 fps","20 fps","15 fps","10 fps"]}else{&["30 fps","60 fps"]};
        unsafe{SendMessageW(self.fps_combo,CB_RESETCONTENT,None,None);for value in values{let text=wide(value);SendMessageW(self.fps_combo,CB_ADDSTRING,None,Some(LPARAM(text.as_ptr()as isize)));}SendMessageW(self.fps_combo,CB_SETCURSEL,Some(WPARAM(values.iter().position(|v|*v==previous).unwrap_or(0))),None);}
    }
    fn options(&self) -> ExportOptions {
        let gif=unsafe{SendMessageW(self.format_combo,CB_GETCURSEL,None,None)}.0==1;
        let fps=crate::platform::workspace_ui::value(self.fps_combo).split_whitespace().next().and_then(|s|s.parse().ok()).unwrap_or(30);
        let mut options=ExportOptions{format:if gif{ExportFormat::Gif}else{ExportFormat::Mp4},fps,..Default::default()};
        options.range = self.extra_options.range;
        options.audio = self.extra_options.audio;
        options.quality = self.extra_options.quality;
        options
    }
    fn command(&mut self, id: usize) {
        match id {
            15 if !self.phase.is_busy() => {
                self.expanded=!self.expanded;self.scroll=0;self.relayout();
            },
            1 if self.phase == ExportPhase::Complete => self.hide(),
            1 if self.phase == ExportPhase::Idle => {
                if !self.advanced_fields[0].is_invalid() {
                    if let Err(e)=self.read_advanced(){self.expanded=true;self.status=e;self.relayout();return;}
                }
                if !self.directory.is_invalid() {
                    if let Err(e) = crate::platform::path_field::commit(self.directory, true)
                        .and_then(|()| crate::platform::path_field::commit(self.filename, true))
                    {
                        self.status = e;
                        self.refresh();
                        return;
                    }
                    let folder =
                        PathBuf::from(crate::platform::workspace_ui::value(self.directory));
                    if let Err(e) = std::fs::create_dir_all(&folder) {
                        self.status = format!("无法使用输出文件夹：{e}");
                        self.refresh();
                        return;
                    }
                    let mut output =
                        folder.join(crate::platform::workspace_ui::value(self.filename));
                    output.set_extension(self.options().extension());
                    self.output = Some(output);
                }
                if let Some(output) = self.output.clone() {
                    let selected = unsafe { SendMessageW(self.combo, CB_GETCURSEL, None, None) }.0;
                    self.actions.push_back(ExportAction::Start {
                        output,
                        preset: match selected {
                            1 => ExportPreset::Hd1080,
                            2 => ExportPreset::Share720,
                            3 => ExportPreset::Compact480,
                            _ => ExportPreset::Native,
                        },
                        options: self.options(),
                    });
                    self.phase = ExportPhase::Starting;
                    self.cancelling = false;
                    self.progress = 0;
                    self.status = "正在准备导出…".into();
                    self.update_controls();
                    self.refresh();
                }
            }
            14 if self.phase == ExportPhase::Idle => {
                self.update_fps_choices();
                let options = self.options();
                let mut output = if self.directory.is_invalid() {
                    self.output.clone()
                } else {
                    // Read the live fields before changing extensions: the saved output may
                    // still describe the filename and directory from before the user's edits.
                    let draft = crate::platform::path_field::commit(self.directory, true)
                        .and_then(|()| crate::platform::path_field::commit(self.filename, true))
                        .and_then(|()| {
                            crate::platform::path_field::validate(
                                &crate::platform::workspace_ui::value(self.directory),
                            )
                        });
                    match draft {
                        Ok(folder) => {
                            Some(folder.join(crate::platform::workspace_ui::value(self.filename)))
                        }
                        Err(error) => {
                            self.status = error;
                            self.refresh();
                            return;
                        }
                    }
                };
                if let Some(path) = &mut output {
                    path.set_extension(options.extension());
                    crate::platform::workspace_ui::text(
                        self.filename,
                        &path.file_name().unwrap_or_default().to_string_lossy(),
                    );
                }
                self.output = output;
                if options.format == ExportFormat::Gif {
                    unsafe {
                        SendMessageW(self.combo, CB_SETCURSEL, Some(WPARAM(3)), None);
                    }
                    self.status = "GIF 循环播放；可选择帧率与分享尺寸".into();
                } else {
                    self.status = "MP4 保留当前编辑效果".into();
                }
                self.refresh();
            }
            12 => {if self.phase.is_busy(){self.cancel_export();}else{self.hide();}},
            2 | 3 => self.hide(),
            11 if self.phase == ExportPhase::Idle => {
                if let Some(path) = choose_path(self.hwnd, &self.name, self.options()) {
                    if let Some(folder) = path.parent() {
                        crate::platform::workspace_ui::text(
                            self.directory,
                            &folder.display().to_string(),
                        );
                    }
                    crate::platform::workspace_ui::text(
                        self.filename,
                        &path.file_name().unwrap_or_default().to_string_lossy(),
                    );
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
        text(dc, layout.title, "导出", palette.text, TextStyle::Heading);
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

    }
}
unsafe extern "system" fn sheet_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Sheet;
    if let Some(sheet) = unsafe { pointer.as_mut() } {
        match message {
            windows::Win32::UI::WindowsAndMessaging::WM_SETCURSOR if wp.0==hwnd.0 as usize => {
                unsafe{if let Ok(c)=LoadCursorW(None,IDC_ARROW){let _=windows::Win32::UI::WindowsAndMessaging::SetCursor(Some(c));}}return LRESULT(1);
            }
            WM_DPICHANGED => {
                sheet.theme = EditorTheme::current(editor_ui::window_dpi(sheet.owner));
                sheet.relayout();
                return LRESULT(0);
            }
            WM_COMMAND => {
                let id = wp.0 & 0xffff;
                let notification = (wp.0 >> 16) & 0xffff;
                if (id == 14
                    && notification
                        == windows::Win32::UI::WindowsAndMessaging::CBN_SELCHANGE as usize)
                    || (id != 14 && notification == 0)
                {
                    sheet.command(id);
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
fn choose_path(owner: HWND, name: &str, options: ExportOptions) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::UI::Controls::Dialogs::{
        GetSaveFileNameW, OFN_EXPLORER, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    let mut buffer = vec![0u16; 32768];
    let initial = wide(&format!("{name}-edited.{}", options.extension()));
    let n = initial.len().min(buffer.len() - 1);
    buffer[..n].copy_from_slice(&initial[..n]);
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: if options.format == ExportFormat::Gif {
            w!("GIF 动图\0*.gif\0\0")
        } else {
            w!("MP4 视频\0*.mp4\0\0")
        },
        lpstrDefExt: if options.format == ExportFormat::Gif {
            w!("gif")
        } else {
            w!("mp4")
        },
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
pub(crate) fn review_format_controls(owner: HWND, directory: &std::path::Path) {
    crate::platform::capture_tools::review_present(owner);
    let mut panel = ExportPanel::open(
        owner,
        "Region capture",
        320,
        180,
        "00:00:01",
        &panzo_core::WorkbenchSettings::default(),
    )
    .unwrap();
    assert!(
        directory.is_absolute(),
        "export review requires an isolated absolute output directory"
    );
    panel.state.output = Some(directory.join("controls.mp4"));
    crate::platform::workspace_ui::text(panel.state.directory, &directory.display().to_string());
    crate::platform::workspace_ui::text(panel.state.filename, "controls.mp4");
    let edit_draft = |field: HWND, value: &str| {
        unsafe {
            SendMessageW(field, WM_COMMAND, Some(WPARAM(2)), None);
        }
        let edit =
            unsafe { windows::Win32::UI::WindowsAndMessaging::GetDlgItem(Some(field), 3) }.unwrap();
        crate::platform::workspace_ui::text(edit, value);
        edit
    };
    let select_gif = |panel: &mut ExportPanel| unsafe {
        SendMessageW(
            panel.state.format_combo,
            CB_SETCURSEL,
            Some(WPARAM(1)),
            None,
        );
        SendMessageW(
            panel.hwnd(),
            WM_COMMAND,
            Some(WPARAM(
                0x000e | ((windows::Win32::UI::WindowsAndMessaging::CBN_SELCHANGE as usize) << 16),
            )),
            None,
        );
        SendMessageW(panel.state.fps_combo,CB_SETCURSEL,Some(WPARAM(3)),None);
    };
    let directory_edit = edit_draft(panel.state.directory, "invalid-relative-directory");
    select_gif(&mut panel);
    assert_eq!(panel.state.output, Some(directory.join("controls.mp4")));
    assert_eq!(
        crate::platform::workspace_ui::value(directory_edit),
        "invalid-relative-directory",
        "format changes must keep an invalid directory draft available for correction"
    );
    assert!(panel.state.status.contains("绝对路径"));
    let edited_directory = directory.join("中文 手动目录");
    crate::platform::workspace_ui::text(directory_edit, &edited_directory.to_string_lossy());
    let filename_edit = edit_draft(panel.state.filename, "错误?名称.mp4");
    select_gif(&mut panel);
    assert_eq!(panel.state.output, Some(directory.join("controls.mp4")));
    assert_eq!(
        crate::platform::workspace_ui::value(filename_edit),
        "错误?名称.mp4",
        "format changes must keep an invalid filename draft available for correction"
    );
    assert!(panel.state.status.contains("有效文件名"));
    crate::platform::workspace_ui::text(filename_edit, "中文手动名称.mp4");
    select_gif(&mut panel);
    assert_eq!(
        panel.state.output,
        Some(edited_directory.join("中文手动名称.gif")),
        "switching format must preserve the live directory and filename drafts"
    );
    assert_eq!(
        crate::platform::workspace_ui::value(panel.state.directory),
        edited_directory.to_string_lossy()
    );
    assert_eq!(
        crate::platform::workspace_ui::value(panel.state.filename),
        "中文手动名称.gif"
    );
    assert_eq!(
        panel.state.options(),
        ExportOptions {
            format: ExportFormat::Gif,
            fps: 15,
            ..Default::default()
        }
    );
    assert_eq!(
        panel.state.output.as_ref().unwrap().extension().unwrap(),
        "gif"
    );
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::UpdateWindow(owner);
    }
    crate::platform::capture_tools::review_present(panel.hwnd());
    crate::platform::capture_tools::review_window_png(
        panel.hwnd(),
        &directory.join("export-gif-panel.png"),
    );
    panel.state.command(1);
    assert!(
        matches!(
            panel.action(),
            Some(ExportAction::Start {
                output,
                preset: ExportPreset::Compact480,
                options: ExportOptions {
                    format: ExportFormat::Gif,
                    fps: 15,
                    ..
                },
            }) if output == edited_directory.join("中文手动名称.gif")
        ),
        "export action must use the isolated directory and edited filename"
    );
}

impl Sheet {
    fn read_advanced(&mut self)->Result<(),String> {
        use super::{workspace_ui as ui,export::ExportQuality};
        let a=ui::value(self.advanced_fields[0]);let b=ui::value(self.advanced_fields[1]);
        let range=parse_export_range(&a,&b)?;
        self.extra_options.range=range;
        self.extra_options.audio=ui::selection(self.advanced_fields[2])==0;
        self.extra_options.quality=match ui::selection(self.advanced_fields[3]){0=>ExportQuality::Clear,2=>ExportQuality::Small,_=>ExportQuality::Balanced};
        Ok(())
    }
    fn scroll_fields(&mut self,delta:i32) {
        let page=(self.layout().body.bottom-self.layout().body.top).max(0);
        let max=(self.theme.scale(if self.expanded{412}else{242})-page).max(0);
        self.scroll=self.scroll.saturating_add(delta).clamp(0,max);self.resize_controls();
    }
    fn paint_fields(&self,dc:HDC) {
        let theme=self.theme;let _drawing=editor_ui::DrawingScale::enter(theme.dpi);let d=|n|theme.scale(n);let r=super::workspace_ui::rect(self.body);
        fill(dc,r,theme.palette.panel);
        for (i,label) in ["名称","输出位置","分辨率","帧率","格式"].into_iter().enumerate() {
            let top=d(i as i32*40)-self.scroll;
            text(dc,RECT{left:d(20),right:d(102),top,bottom:top+d(28)},label,theme.palette.text_secondary,TextStyle::Body);
        }
        if self.expanded {for (i,label) in ["开始 · s","结束 · s","输出声音","MP4 质量"].into_iter().enumerate() {
            let top=d(244+i as i32*40)-self.scroll;
            text(dc,RECT{left:d(20),right:d(102),top,bottom:top+d(28)},label,theme.palette.text_secondary,TextStyle::Body);
        }}
    }
}
fn parse_export_range(a:&str,b:&str)->Result<Option<(panzo_core::TimeTick,panzo_core::TimeTick)>,String> {
    let a=a.trim().parse::<f64>().unwrap_or(-1.);
    if b.trim().is_empty(){return if a==0.{Ok(None)}else{Err("指定范围时必须填写终点".into())};}
    let b=b.trim().parse::<f64>().unwrap_or(-1.);
    if !a.is_finite()||!b.is_finite()||a<0.||b<=a||b>86400.{return Err("时间范围无效：终点须晚于起点，最大 24 小时".into());}
    Ok(Some((panzo_core::TimeTick((a*1e7).round() as i64),panzo_core::TimeTick((b*1e7).round() as i64))))
}
unsafe extern "system" fn fields_proc(h:HWND,m:u32,w:WPARAM,l:LPARAM)->LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::*;
    let ptr=unsafe{GetWindowLongPtrW(h,GWLP_USERDATA)} as *mut Sheet;
    if let Some(s)=unsafe{ptr.as_mut()} {
        match m {
            WM_COMMAND=>{unsafe{SendMessageW(s.hwnd,m,Some(w),Some(l));}return LRESULT(0);}
            WM_MOUSEWHEEL=>{s.scroll_fields(-((w.0>>16) as i16 as i32)/120*s.theme.scale(40));return LRESULT(0);}
            WM_VSCROLL=>{
                let (_,_,pos)=super::desktop_ui::panel_scroll_info(h);
                let next=match w.0&0xffff {0=>s.scroll-s.theme.scale(24),1=>s.scroll+s.theme.scale(24),2=>s.scroll-s.theme.scale(160),3=>s.scroll+s.theme.scale(160),4|5=>pos,6=>0,7=>i32::MAX,_=>s.scroll};
                s.scroll_fields(next.saturating_sub(s.scroll));return LRESULT(0);
            }
            WM_PAINT=>{let mut p=PAINTSTRUCT::default();let dc=unsafe{BeginPaint(h,&mut p)};s.paint_fields(dc);unsafe{let _=EndPaint(h,&p);}return LRESULT(0);}
            WM_ERASEBKGND=>return LRESULT(1),
            WM_SETCURSOR if w.0==h.0 as usize=>{unsafe{if let Ok(c)=LoadCursorW(None,IDC_ARROW){let _=SetCursor(Some(c));}}return LRESULT(1);}
            _=>{}
        }
    }
    unsafe{DefWindowProcW(h,m,w,l)}
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
                description: "00:30.000 · 当前编辑快照".into(),
                combo: HWND::default(),
                format_combo: HWND::default(),
                fps_combo: HWND::default(),
                advanced: HWND::default(),
                body: HWND::default(),
                advanced_fields:[HWND::default();4],
                expanded:false,scroll:0,
                extra_options: ExportOptions::default(),
                browse: HWND::default(),
                directory: HWND::default(),
                filename: HWND::default(),
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
    fn export_range_validates_drafts_and_preserves_fractional_seconds() {
        assert_eq!(parse_export_range("0.000","").unwrap(),None);
        assert_eq!(parse_export_range("0.125","2.5").unwrap(),Some((panzo_core::TimeTick(1_250_000),panzo_core::TimeTick(25_000_000))));
        for (a,b) in [("1",""),("NaN","5"),("0","inf"),("-1","5"),("1","1"),("3","2"),("0","86401")] {
            assert!(parse_export_range(a,b).is_err(),"{a}..{b}");
        }
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
                for expanded in [false,true] {
                let panel = panel_rect(owner, theme,expanded);
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
                    layout.body,
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
                assert!(layout.body.bottom>layout.body.top);
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn review_layout_and_advanced(owner:HWND,out:&std::path::Path) {
    use super::{workspace_ui as ui,capture_tools,desktop_ui as desk};
    use windows::Win32::UI::WindowsAndMessaging::*;
    let mut p=ExportPanel::open(owner,"panzo-fixture",2560,1440,"00:03.000",&Default::default()).unwrap();
    assert!(!p.state.expanded);
    let body=p.state.body;
    let mut rows=Vec::new();
    for h in [p.state.filename,p.state.directory,p.state.combo,p.state.fps_combo,p.state.format_combo] {
        assert_eq!(unsafe{GetParent(h)}.unwrap(),body);
        let mut r=RECT::default();unsafe{GetWindowRect(h,&mut r).unwrap();}assert_eq!(r.bottom-r.top,p.state.theme.scale(28));rows.push(r);
    }
    for pair in rows.windows(2){assert!(pair[0].top<pair[1].top);assert_eq!(pair[0].left,pair[1].left);assert_eq!(pair[0].right,pair[1].right);}
    unsafe{SetCursor(Some(LoadCursorW(None,IDC_CROSS).unwrap()));SendMessageW(p.hwnd(),WM_SETCURSOR,Some(WPARAM(p.hwnd().0 as usize)),Some(LPARAM(1)));}
    assert_eq!(unsafe{GetCursor()},unsafe{LoadCursorW(None,IDC_ARROW)}.unwrap());
    capture_tools::review_present(p.hwnd());
    capture_tools::review_window_png(p.hwnd(),&out.join("export-collapsed.png"));
    p.state.command(15);assert!(p.state.expanded);
    for h in p.state.advanced_fields{assert!(unsafe{IsWindowVisible(h)}.as_bool());assert_eq!(unsafe{GetParent(h)}.unwrap(),body);}
    ui::text(p.state.advanced_fields[0],"0.500");ui::text(p.state.advanced_fields[1],"2.500");
    unsafe{SendMessageW(p.state.advanced_fields[2],CB_SETCURSEL,Some(WPARAM(1)),None);SendMessageW(p.state.advanced_fields[3],CB_SETCURSEL,Some(WPARAM(0)),None);}
    p.state.read_advanced().unwrap();assert!(!p.state.options().audio);assert_eq!(p.state.options().quality,super::export::ExportQuality::Clear);
    capture_tools::review_present(p.hwnd());
    capture_tools::review_window_png(p.hwnd(),&out.join("export-expanded.png"));
    unsafe{SetWindowPos(p.hwnd(),None,0,0,p.state.theme.scale(460),p.state.theme.scale(380),SWP_NOMOVE|SWP_NOZORDER|SWP_NOACTIVATE).unwrap();}
    p.state.resize_controls();p.state.scroll_fields(10000);assert!(p.state.scroll>0);
    capture_tools::review_present(p.hwnd());
    capture_tools::review_window_png(p.hwnd(),&out.join("export-short-scrolled.png"));
    p.state.relayout();assert_eq!(p.state.scroll,0);
    p.state.command(15);assert!(!p.state.expanded);p.state.command(15);
    assert_eq!(ui::value(p.state.advanced_fields[0]),"0.500");
    ui::text(p.state.directory,&out.display().to_string());ui::text(p.state.filename,"preview-quality-export.mp4");
    p.state.command(1);
    assert!(matches!(p.action(),Some(ExportAction::Start{options:ExportOptions{range:Some((panzo_core::TimeTick(5_000_000),panzo_core::TimeTick(25_000_000))),audio:false,quality:super::export::ExportQuality::Clear,..},..})));
    p.finish(false,"测试结束");
    ui::text(p.state.advanced_fields[1],"0.100");p.state.command(1);assert!(p.action().is_none());assert_eq!(p.state.phase,ExportPhase::Idle);
    p.state.scroll_fields(10000);let(_,_,pos)=desk::panel_scroll_info(body);assert!(pos>=0);
    drop(p);
    // Keep format changes and live filename/path draft preservation covered.
    review_format_controls(owner,out);
}
