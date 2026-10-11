#![allow(clippy::cast_possible_wrap)] // Control indexes are bounded by small static field lists.
//! Persistent application shell. Tabs change visibility, never document lifetime.
use super::{
    image_tasks::{Output, Update},
    workspace_ui as ui,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, GWLP_USERDATA, GetWindowLongPtrW, MSG,
            PostMessageW, SendMessageW, SetTimer, SetWindowLongPtrW, WM_APP, WM_CLOSE, WM_COMMAND,
            WM_HOTKEY, WM_NCDESTROY, WM_SIZE, WM_TIMER, WS_CLIPCHILDREN, WS_EX_CONTROLPARENT,
            WS_OVERLAPPEDWINDOW,
        },
    },
    core::w,
};
const NAMES: [&str; 3] = ["图片工具", "录制与视频", "素材"];
const DISPATCH: u32 = WM_APP + 180;
#[path = "workspace_toast.rs"]
mod workspace_toast;
use workspace_toast::{Kind as NoticeKind, Toast};
#[derive(Clone, Copy)]
struct Handles {
    root: HWND,
    video: HWND,
    image: HWND,
}
thread_local! {static CUSTOM_CAPTION:Cell<bool>=const{Cell::new(false)};static HANDLES:Cell<Option<Handles>>=const{Cell::new(None)};static EVENTS:RefCell<VecDeque<Event>>=const{RefCell::new(VecDeque::new())};static BACKGROUND:Cell<bool>=const{Cell::new(false)};}
enum Event {
    Switch(usize),
    Picture(HWND, bool),
    ImageView(Option<HWND>),
    Form(usize, HWND),
    HideForm(HWND),
    Notice(NoticeKind, String),
    Job(Job),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Image { id: u64, revision: u64 },
    Form(HWND),
}
struct Job {
    target: Target,
    name: String,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<Update>,
}
struct State {
    video: HWND,
    image: HWND,
    library: PathBuf,
    library_view: Option<HWND>,
    image_view: Option<HWND>,
    tabs: Vec<HWND>,
    forms: [Option<HWND>; 5],
    menus: Vec<HWND>,
    theme_button: HWND,
    export_button: HWND,
    task_button: HWND,
    task_stop: HWND,
    theme: super::ui_preferences::ThemeMode,
    active: usize,
    toast: Toast,
    jobs: Vec<Job>,
}
pub(crate) fn root() -> Option<HWND> {
    HANDLES.with(|h| h.get().map(|h| h.root))
}
pub(crate) fn custom_caption(hwnd: HWND) -> bool {
    (root() == Some(hwnd) && CUSTOM_CAPTION.with(Cell::get))
        || super::management_window::is_custom(hwnd)
}
pub(crate) fn caption_height(hwnd: HWND) -> i32 {
    if custom_caption(hwnd) {
        ui::scale(hwnd, 32)
    } else {
        0
    }
}
pub(crate) fn image_parent() -> Option<HWND> {
    HANDLES
        .with(|h| h.get().map(|h| h.image))
        .filter(|h| !h.is_invalid())
}
pub(crate) fn window(hwnd: HWND) -> HWND {
    HANDLES.with(|h| {
        h.get()
            .map_or(hwnd, |h| if hwnd == h.video { h.root } else { hwnd })
    })
}
pub(crate) fn active() -> bool {
    root().is_some()
}
pub(crate) fn switch(tab: usize) {
    post(Event::Switch(tab));
}
pub(crate) fn notice(s: &str) {
    post(Event::Notice(NoticeKind::Info, s.into()));
}
pub(crate) fn notice_error(s: &str) {
    post(Event::Notice(NoticeKind::Error, s.into()));
}
pub(crate) fn set_form(tab: usize, h: HWND) {
    post(Event::Form(tab, h));
}
pub(crate) fn hide_form(h: HWND) {
    post(Event::HideForm(h));
}
pub(crate) fn image_view(h: Option<HWND>) {
    post(Event::ImageView(h));
}
pub(crate) fn picture(h: HWND) {
    post(Event::Picture(h, !BACKGROUND.with(Cell::get)));
}
pub(crate) fn video_command(command: u32) {
    HANDLES.with(|h| {
        if let Some(h) = h.get() {
            unsafe {
                let _ = PostMessageW(
                    Some(h.video),
                    WM_HOTKEY,
                    WPARAM(command as usize),
                    LPARAM(0),
                );
            }
        }
    });
}
fn post(e: Event) {
    if let Some(h) = root() {
        EVENTS.with(|events| events.borrow_mut().push_back(e));
        unsafe {
            let _ = PostMessageW(Some(h), DISPATCH, WPARAM(0), LPARAM(0));
        }
    }
}
pub(crate) fn start_job(
    target: Target,
    name: &str,
    work: impl FnOnce(Arc<AtomicBool>, Sender<Update>) -> Result<Output, String> + Send + 'static,
) -> Arc<AtomicBool> {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    post(Event::Job(Job {
        target,
        name: name.into(),
        cancel: cancel.clone(),
        receiver: rx,
    }));
    let token = cancel.clone();
    std::thread::spawn(move || {
        let result = work(token.clone(), tx.clone());
        let result = if token.load(Ordering::Acquire) {
            Err("任务已取消".into())
        } else {
            result
        };
        let _ = tx.send(Update::Done(result));
    });
    cancel
}
pub(crate) fn create() -> windows::core::Result<HWND> {
    ui::register(w!("PanzoWorkspace"), Some(procedure)).map_err(|e| {
        windows::core::Error::new(
            windows::core::HRESULT(windows::Win32::Foundation::E_FAIL.0),
            e,
        )
    })?;
    let (wid, hei) = super::editor_ui::available_window_size(
        ui::scale(HWND::default(), 1440),
        ui::scale(HWND::default(), 940),
    );
    let module = unsafe { GetModuleHandleW(None)? };
    let h = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            w!("PanzoWorkspace"),
            w!("Panzo · 图片工具"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            wid,
            hei,
            None,
            None,
            Some(module.into()),
            None,
        )?
    };
    HANDLES.with(|v| {
        v.set(Some(Handles {
            root: h,
            video: HWND::default(),
            image: HWND::default(),
        }));
    });
    let color = super::editor_ui::EditorTheme::current(super::editor_ui::window_dpi(h))
        .palette
        .window;
    let _ = color;
    CUSTOM_CAPTION.with(|value| value.set(true));
    super::editor_ui::apply_window_chrome(h);
    super::preview_window::editor_titlebar::initialize(h);
    Ok(h)
}
pub(crate) fn attach(
    root: HWND,
    video: HWND,
    library: &Path,
    has_project: bool,
) -> Result<(), String> {
    let image = super::image_workspace::create(root, library)?;
    let tabs = NAMES
        .iter()
        .enumerate()
        .map(|(i, t)| ui::button(root, t, 100 + i))
        .collect::<Result<Vec<_>, _>>()?;
    let menus = ["文件", "编辑", "视图", "帮助", "插件", "设置"]
        .iter()
        .enumerate()
        .map(|(i, t)| ui::button(root, t, 111 + i))
        .collect::<Result<Vec<_>, _>>()?;
    let theme_button = ui::button(root, "切换主题", 110)?;
    let export_button = ui::button(root, "导出", 119)?;
    super::desktop_ui::style(export_button, None, false, true, false);
    for tab in &tabs {
        super::desktop_ui::workspace_tab(*tab);
    }
    let task_button = ui::button(root, "后台任务", 117)?;
    let task_stop = ui::button(root, "结束旁白录制", 118)?;
    super::desktop_ui::style(task_button, None, true, false, false);
    super::desktop_ui::style(
        task_stop,
        Some(super::editor_ui::IconKind::Stop),
        true,
        false,
        true,
    );
    ui::show(task_button, false);
    ui::show(task_stop, false);
    for b in tabs
        .iter()
        .chain(&menus)
        .chain(std::iter::once(&theme_button))
    {
        super::desktop_ui::style(*b, None, true, false, false);
    }
    let toast = Toast::new(root)?;
    let state = State {
        video,
        image,
        library: library.into(),
        library_view: None,
        image_view: None,
        tabs,
        forms: [None; 5],
        menus,
        theme_button,
        export_button,
        task_button,
        task_stop,
        theme: super::ui_preferences::theme(),
        active: usize::from(has_project),
        toast,
        jobs: Vec::new(),
    };
    let ptr = Box::into_raw(Box::new(state));
    unsafe {
        SetWindowLongPtrW(root, GWLP_USERDATA, ptr as isize);
        SetTimer(Some(root), 1, 100, None);
    }
    HANDLES.with(|h| h.set(Some(Handles { root, video, image })));
    unsafe { &mut *ptr }.select(root, usize::from(has_project));
    ui::show(root, true);
    Ok(())
}
thread_local! {static LAST_TEXT:Cell<Option<HWND>>=const{Cell::new(None)};}
pub(crate) fn remember_text_focus(h: HWND) {
    LAST_TEXT.with(|v| v.set(Some(h)));
}
fn text_target() -> Option<HWND> {
    LAST_TEXT.with(Cell::get).filter(|h| {
        let mut class = [0u16; 32];
        let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassNameW(*h, &mut class) };
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(*h) }.as_bool()
            && String::from_utf16_lossy(&class[..n.max(0) as usize])
                .to_ascii_lowercase()
                .contains("edit")
    })
}
pub(crate) fn dialog_message(msg: &MSG) -> bool {
    if super::recording_setup::dialog_message(msg){return true;}
    use windows::Win32::UI::WindowsAndMessaging::*;
    if msg.message == WM_LBUTTONDOWN {
        let menu = unsafe { GetDlgCtrlID(msg.hwnd) };
        let mut class = [0u16; 32];
        let n = unsafe { GetClassNameW(msg.hwnd, &mut class) };
        if String::from_utf16_lossy(&class[..n.max(0) as usize])
            .to_ascii_lowercase()
            .contains("edit")
        {
            remember_text_focus(msg.hwnd);
        } else if !(111..=114).contains(&menu) {
            LAST_TEXT.with(|v| v.set(None));
        }
    }
    if msg.message != WM_KEYDOWN {
        return false;
    }
    if super::management_window::dialog_message(msg) {
        return true;
    }
    let Some(root) = root() else { return false };
    if msg.hwnd == root || unsafe { IsChild(root, msg.hwnd) }.as_bool() {
        if msg.wParam.0 == 9 {
            return unsafe { IsDialogMessageW(root, msg) }.as_bool();
        }
    }
    false
}
impl State {
    fn menu(&mut self, h: HWND, id: usize) {
        use super::{
            app_menu::{self, Row},
            desktop_ui as desk,
            editor_ui::{EditorTheme, IconKind as I},
        };
        let text = text_target();
        let image = self.active == 0
            && self.image_view.is_none()
            && super::image_workspace::current_document().is_ok();
        let video = if self.active == 1 {
            super::preview_window::workspace_available(self.video)
        } else {
            [false; 4]
        };
        let rows = match id {
            111 if self.active == 1 => vec![
                Row::command(230, "导入视频 / 图片", I::Folder, true).shortcut_hint("import_video"),
                Row::command(231, "打开视频工程", I::Folder, true).shortcut_hint("open_project"),
                Row::command(
                    260,
                    "声音与旁白",
                    I::Record,
                    super::preview_window::media_snapshot(self.video).is_ok(),
                )
                .shortcut_hint("audio_tools"),
                Row::command(
                    261,
                    "字幕与 AI 自动字幕",
                    I::Text,
                    super::preview_window::media_snapshot(self.video).is_ok(),
                )
                .shortcut_hint("subtitle_tools"),
                Row::command(266, "打开工程所在位置", I::Folder, video[3]),
                Row::command(202, "保存当前工程", I::Save, video[0]).shortcut_hint("save"),
                Row::command(211, "导出视频 / GIF", I::Export, video[3])
                    .shortcut_hint("export_video"),
            ],
            111 => vec![
                Row::command(201, "打开图片 / GIF", I::Folder, true).shortcut_hint("open_image"),
                Row::command(200, "区域截图", I::Capture, true).shortcut_hint("capture"),
                Row::command(202, "保存当前图片", I::Save, image).shortcut_hint("save"),
                Row::command(203, "关闭当前图片", I::Close, image).shortcut_hint("close_image"),
            ],
            112 if self.active == 1 && text.is_none() => vec![
                Row::command(207, "撤销", I::Undo, video[1]).shortcut_hint("undo"),
                Row::command(208, "重做", I::Redo, video[2]).shortcut_hint("redo"),
                Row::separator(),
                Row::command(262, "标注、跟随与混音…", I::Edit, video[3]),
            ],
            112 => vec![
                Row::command(
                    207,
                    if text.is_some() {
                        "撤销文字输入"
                    } else {
                        "撤销"
                    },
                    I::Undo,
                    text.is_some() || image || video[1],
                ),
                Row::command(208, "重做", I::Redo, text.is_none() && (image || video[2]))
                    .shortcut_hint("redo"),
                Row::command(
                    209,
                    if text.is_some() {
                        "复制选中文字"
                    } else {
                        "复制图片"
                    },
                    I::Image,
                    text.is_some() || image,
                ),
            ],
            113 if self.active == 1 => vec![
                Row::command(263, "预览居中并适应窗口", I::Fit, video[3]),
                Row::command(265, "展开 / 收起项目素材", I::Panel, video[3]),
                Row::command(264, "展开 / 收起属性面板", I::Panel, video[3]),
            ],
            113 => vec![
                Row::command(212, "显示 / 隐藏识别面板", I::Window, self.active == 0)
                    .shortcut_hint("recognition_panel"),
                Row::command(213, "适应窗口", I::Maximize, image).shortcut_hint("fit_image"),
                Row::command(214, "实际大小 · 100%", I::OneToOne, image)
                    .shortcut_hint("actual_image"),
                Row::command(210, "切换浅色 / 深色主题", I::Sun, true).shortcut_hint("theme"),
            ],
            _ => vec![
                Row::command(220, "查看快捷键", I::Log, true).shortcut_hint("shortcuts"),
                Row::command(221, "配置多模态识图服务", I::Settings, true),
            ],
        };
        let t = EditorTheme::current(super::editor_ui::window_dpi(h));
        if let Ok(Some(c)) = app_menu::show_rows(
            h,
            desk::rect(
                t.scale(40 + 52 * (id as i32 - 111)),
                t.scale(2),
                t.scale(50),
                t.scale(28),
            ),
            t,
            rows,
            false,
        ) {
            self.dispatch(h, c, text);
        }
    }
    fn dispatch(&mut self, h: HWND, c: i32, text: Option<HWND>) {
        if matches!(c, 207 | 209)
            && let Some(edit) = text
        {
            unsafe {
                SendMessageW(
                    edit,
                    if c == 209 {
                        windows::Win32::UI::WindowsAndMessaging::WM_COPY
                    } else {
                        windows::Win32::UI::Controls::EM_UNDO
                    },
                    None,
                    None,
                );
            }
            return;
        }
        if self.active == 1 && matches!(c, 202 | 207 | 208 | 211 | 240..=251 | 260..=266) {
            super::preview_window::workspace_command(self.video, c);
            return;
        }
        let image = self.active == 0
            && self.image_view.is_none()
            && super::image_workspace::current_document().is_ok();
        match c {
            200 | 201 => unsafe {
                SendMessageW(
                    self.image,
                    WM_COMMAND,
                    Some(WPARAM(if c == 200 { 1 } else { 2 })),
                    None,
                );
            },
            203 if image => unsafe {
                SendMessageW(self.image, WM_COMMAND, Some(WPARAM(6)), None);
            },
            202 | 207 | 208 | 209 if image => super::image_workspace::picture_command(match c {
                202 => 10,
                207 => 7,
                208 => 8,
                _ => 9,
            }),
            240 if image => super::image_workspace::picture_command(37),
            212 if self.active == 0 => unsafe {
                SendMessageW(self.image, WM_COMMAND, Some(WPARAM(12)), None);
            },
            213 | 214 if image => {
                super::image_workspace::picture_command(if c == 213 { 63 } else { 64 })
            }
            210 => unsafe {
                SendMessageW(h, WM_COMMAND, Some(WPARAM(110)), None);
            },
            220 => {
                if let Err(e) = super::shortcut_settings::open() {
                    self.toast.show(NoticeKind::Error, &e);
                }
            }
            221 => {
                if let Err(e) = super::workspace_settings::open() {
                    self.toast.show(NoticeKind::Error, &e);
                }
            }
            230 | 231 => {
                super::recorder_window::workspace_open(self.video, c == 231);
            }
            _ => {}
        }
    }
    fn select(&mut self, hwnd: HWND, tab: usize) {
        if tab == 4 {
            // Reusing the owned window must still reload its installed plugin list.
            if let Err(e) = super::plugin_ui::manager() {
                self.toast.show(NoticeKind::Error, &e);
            }
            return;
        }
        if tab >= 3 {
            if !super::management_window::show(tab) {
                let result = if tab == 3 {
                    super::workspace_settings::general()
                } else {
                    super::plugin_ui::manager()
                };
                if let Err(e) = result {
                    self.toast.show(NoticeKind::Error, &e);
                }
            }
            return;
        }
        LAST_TEXT.with(|v| v.set(None));
        self.active = tab.min(2);
        if self.active == 2 && self.library_view.is_none() {
            match super::capture_tools::embedded_history(hwnd, self.video, &self.library) {
                Ok(h) => self.library_view = Some(h),
                Err(e) => self.toast.show(NoticeKind::Error, &e),
            }
        }
        for (i, b) in self.tabs.iter().enumerate() {
            super::desktop_ui::selected(*b, i == self.active);
        }
        ui::text(hwnd, &format!("Panzo · {}", NAMES[self.active]));
        self.layout(hwnd);
    }
    fn layout(&self, hwnd: HWND) {
        let r = ui::rect(hwnd);
        let d = |v| ui::scale(hwnd, v);
        let w = r.right;
        let _batch = ui::layout_batch(hwnd);
        let caption = caption_height(hwnd);
        let caption_top = (caption - d(28)) / 2;
        for (i, b) in self.menus.iter().enumerate() {
            ui::place(*b, d(40 + 52 * i as i32), caption_top, d(50), d(28));
        }
        ui::place(self.task_button, d(12), caption + d(8), d(208), d(28));
        ui::place(self.task_stop, d(224), caption + d(8), d(32), d(28));
        let tab_gap = if w <= d(1150) { 9 } else { 22 };
        let tab_padding = if w <= d(1000) { 12 } else { 17 };
        let widths = [
            52 + tab_padding * 2,
            65 + tab_padding * 2,
            26 + tab_padding * 2,
        ];
        let mut left = (w - d(widths.iter().sum::<i32>() + tab_gap * 2)) / 2;
        for (i, b) in self.tabs.iter().enumerate() {
            let width = d(widths[i]);
            ui::place(
                *b,
                left,
                caption,
                width,
                d(super::workbench_layout::WORKSPACES) - 1,
            );
            left += width + d(tab_gap);
        }
        let system_left = super::preview_window::editor_titlebar::caption_bounds(hwnd)
            .map_or(w - d(138), |r| r.left);
        ui::place(
            self.theme_button,
            system_left - d(48),
            caption_top,
            d(34),
            d(28),
        );
        let dark = self.theme == super::ui_preferences::ThemeMode::Dark;
        ui::text(
            self.theme_button,
            if dark {
                "切换为浅色主题"
            } else {
                "切换为深色主题"
            },
        );
        super::desktop_ui::style(
            self.theme_button,
            Some(if dark {
                super::editor_ui::IconKind::Sun
            } else {
                super::editor_ui::IconKind::Moon
            }),
            true,
            false,
            true,
        );
        ui::place(self.export_button, w - d(90), caption + d(8), d(74), d(28));
        ui::show(self.export_button, self.active < 2);
        let top = caption + d(super::workbench_layout::WORKSPACES);
        let height = (r.bottom - top).max(1);
        let side = if self.active != 1 && self.active < 3 && self.forms[self.active].is_some() {
            d(440).min(r.right / 2)
        } else {
            0
        };
        super::image_workspace::set_inspector(self.image, self.forms[0].is_some());
        if self.active == 0 {
            ui::place(self.image, 0, top, r.right - side, height);
        }
        if self.active == 1 {
            ui::place(self.video, 0, top, r.right, height);
        }
        ui::show(self.image, self.active == 0 && self.image_view.is_none());
        if let Some(h) = self.image_view {
            ui::place(h, 0, top, r.right, height);
            ui::show(h, self.active == 0);
        }
        ui::show(self.video, self.active == 1);
        if let Some(h) = self.library_view {
            if self.active == 2 {
                ui::place(h, 0, top, r.right - side, height);
            }
            ui::show(h, self.active == 2);
        }
        for (i, h) in self.forms[..3].iter().enumerate().filter(|(i, _)| *i != 1) {
            if let Some(h) = h {
                ui::place(*h, r.right - side, top, side, height);
                ui::show(*h, i == self.active);
            }
        }
        self.toast.layout();
        // Finish every nested size update before presenting one complete child tree.
        // Otherwise a stream of WM_SIZE messages starves child WM_PAINT messages.
        drop(_batch);
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::RedrawWindow(
                Some(hwnd),
                None,
                None,
                windows::Win32::Graphics::Gdi::RDW_INVALIDATE
                    | windows::Win32::Graphics::Gdi::RDW_NOERASE
                    | windows::Win32::Graphics::Gdi::RDW_ALLCHILDREN,
            );
        }
    }
    fn drain(&mut self, hwnd: HWND) {
        loop {
            let event = EVENTS.with(|q| q.borrow_mut().pop_front());
            let Some(event) = event else { break };
            match event {
                Event::Switch(t) => self.select(hwnd, t),
                Event::ImageView(view) => {
                    if let Some(old) = self.image_view.take() {
                        ui::show(old, false);
                    }
                    self.image_view = view;
                    if view.is_some() {
                        if let Some(old) = self.forms[0].take() {
                            ui::show(old, false);
                        }
                    }
                    self.select(hwnd, 0);
                }
                Event::Picture(h, focus) => {
                    super::image_workspace::add(self.image, h, focus);
                    if focus {
                        if let Some(old) = self.image_view.take() {
                            ui::show(old, false);
                        }
                        self.select(hwnd, 0);
                    }
                }
                Event::Form(1, h) => {
                    match super::preview_window::attach_inspector_panel(self.video, h) {
                        Ok(()) => self.select(hwnd, 1),
                        Err(error) => self.toast.show(NoticeKind::Error, &error),
                    }
                }
                Event::Form(t, h) => {
                    if t == 0 {
                        if let Some(old) = self.image_view.take() {
                            ui::show(old, false);
                        }
                    }
                    if let Some(old) = self.forms[t].replace(h)
                        && old != h
                    {
                        ui::show(old, false);
                    }
                    if t >= 3 {
                        super::management_window::present(t, h);
                    } else {
                        self.select(hwnd, t);
                    }
                }
                Event::HideForm(h) => {
                    super::recording_setup::advanced_closed(h);
                    if !super::preview_window::hide_inspector_panel(self.video, h)
                        && !super::management_window::hide_panel(h)
                    {
                        for f in &mut self.forms[..3] {
                            if *f == Some(h) {
                                *f = None;
                            }
                        }
                        ui::show(h, false);
                        self.layout(hwnd);
                    }
                }
                Event::Notice(kind, s) => self.toast.show(kind, &s),
                Event::Job(job) => {
                    if let Target::Form(h) = job.target {
                        super::workspace_form::set_running(h, Some(job.cancel.clone()));
                    }
                    self.jobs.push(job);
                }
            }
        }
    }
    fn poll(&mut self) {
        let task = super::media_panel::active_task();
        ui::show(self.task_button, task.is_some() || !self.jobs.is_empty());
        ui::show(
            self.task_stop,
            task.as_ref().is_some_and(|(_, recording, _)| *recording),
        );
        if let Some((_, _, label)) = &task {
            ui::text(self.task_button, &format!("{label} · 返回"));
        } else if let Some(job) = self.jobs.first() {
            ui::text(self.task_button, &format!("{} · 返回", job.name));
        }

        let theme = super::ui_preferences::theme();
        if self.theme != theme {
            self.theme = theme;
            if let Some(h) = root() {
                super::desktop_ui::refresh(h);
                if let Some(form) = self.forms[3] {
                    super::workspace_settings::sync_theme(form);
                }
                super::management_window::refresh();
                self.layout(h);
            }
        }
        super::image_workspace::prune(self.image);
        let mut done = Vec::new();
        for (i, j) in self.jobs.iter_mut().enumerate() {
            loop {
                match j.receiver.try_recv() {
                    Ok(Update::Progress(s)) => match j.target {
                        Target::Image { id, .. } => {
                            super::image_workspace::progress(self.image, id, &s);
                        }
                        Target::Form(h) => super::workspace_form::feedback(h, &s),
                    },
                    Ok(Update::Done(result)) => {
                        match &result {
                            Ok(_) => self
                                .toast
                                .show(NoticeKind::Success, &format!("{}已完成", j.name)),
                            Err(e) => self
                                .toast
                                .show(NoticeKind::Error, &format!("{}：{e}", j.name)),
                        }
                        match j.target {
                            Target::Image { id, revision } => {
                                super::image_workspace::complete(self.image, id, revision, result);
                            }
                            Target::Form(h) => {
                                super::workspace_form::set_running(h, None);
                                super::workspace_form::feedback(
                                    h,
                                    &match result {
                                        Ok(Output::Text(s)) => s,
                                        Ok(Output::Image(image)) => {
                                            match result_image(image, &self.library) {
                                                Ok(_) => "处理完成，结果已加入图片工具，原图保留。"
                                                    .into(),
                                                Err(e) => e,
                                            }
                                        }
                                        Err(e) => e,
                                    },
                                );
                            }
                        }
                        done.push(i);
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let notice = format!("{}意外中断，请重试", j.name);
                        self.toast.show(NoticeKind::Error, &notice);
                        if let Target::Image { id, revision } = j.target {
                            super::image_workspace::complete(
                                self.image,
                                id,
                                revision,
                                Err(notice.clone()),
                            );
                        }
                        if let Target::Form(h) = j.target {
                            super::workspace_form::set_running(h, None);
                            super::workspace_form::feedback(h, &notice);
                        }
                        done.push(i);
                        break;
                    }
                }
            }
        }
        for i in done.into_iter().rev() {
            self.jobs.remove(i);
        }
    }
}
#[allow(clippy::too_many_lines)] // Main host message dispatch.
unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if custom_caption(hwnd)
        && let Some(result) =
            super::preview_window::editor_titlebar::window_message(hwnd, msg, wp, lp)
    {
        return result;
    }
    if let Some(r) = super::app_menu::window_message(msg, lp) {
        return r;
    }
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut State;
    let Some(s) = (unsafe { ptr.as_mut() }) else {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    };
    match msg {
        DISPATCH => s.drain(hwnd),
        windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE
        | windows::Win32::UI::WindowsAndMessaging::WM_THEMECHANGED
        | windows::Win32::UI::WindowsAndMessaging::WM_SYSCOLORCHANGE => {
            super::system_preferences::refresh();
            super::desktop_ui::refresh(hwnd);
            super::management_window::refresh();
            s.layout(hwnd);
        }
        WM_COMMAND if (100..105).contains(&(wp.0 & 0xffff)) => {
            s.select(hwnd, (wp.0 & 0xffff) - 100);
        }
        windows::Win32::UI::WindowsAndMessaging::WM_PAINT => {
            use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(hwnd, &raw mut ps) };
            let frame = super::desktop_ui::BufferedFrame::new(dc, ui::rect(hwnd));
            let dc = frame.dc;
            let t = super::editor_ui::EditorTheme::current(super::editor_ui::window_dpi(hwnd));
            let r = ui::rect(hwnd);
            let d = |v| t.scale(v);
            let caption = caption_height(hwnd);
            super::editor_ui::fill_rect(dc, &r, t.palette.panel);
            for y in [caption + d(super::workbench_layout::WORKSPACES) - 1] {
                super::desktop_ui::line(dc, super::desktop_ui::rect(0, y, r.right, 1), t);
            }
            if caption > 0 {
                super::editor_ui::fill_opaque_rect(
                    dc,
                    super::desktop_ui::rect(0, 0, r.right, caption),
                    t.palette.window,
                );
                super::editor_ui::draw_icon(
                    dc,
                    super::desktop_ui::rect(d(12), d(8), d(16), d(16)),
                    super::editor_ui::IconKind::Panzo,
                    t.palette.text,
                );
                super::preview_window::editor_titlebar::paint_system_buttons(hwnd, dc, t);
            }
            drop(frame);
            unsafe {
                let _ = EndPaint(hwnd, &raw const ps);
            }
        }
        WM_COMMAND if (6000..6000 + super::shortcuts::ACTIONS.len()).contains(&wp.0) => {
            use super::shortcuts::Target;
            let a = &super::shortcuts::ACTIONS[wp.0 - 6000];
            match a.target {
                Target::Shell(c) => video_command(c),
                Target::Workspace(c) => unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        Some(hwnd),
                        WM_COMMAND,
                        WPARAM(c),
                        LPARAM(0),
                    );
                },
                Target::Image(c) if s.active == 0 => unsafe {
                    SendMessageW(s.image, WM_COMMAND, Some(WPARAM(c)), None);
                },
                Target::Picture(c) if s.active == 0 && s.image_view.is_none() => {
                    super::image_workspace::picture_command(c)
                }
                Target::Video(c) if s.active == 1 => {
                    super::preview_window::workspace_command(s.video, c)
                }
                _ => {}
            }
        }
        WM_COMMAND if (200..=261).contains(&wp.0) => s.dispatch(hwnd, wp.0 as i32, text_target()),
        WM_COMMAND if wp.0 == 110 => {
            let mode = if s.theme == super::ui_preferences::ThemeMode::Dark {
                super::ui_preferences::ThemeMode::Light
            } else {
                super::ui_preferences::ThemeMode::Dark
            };
            if let Err(e) = super::ui_preferences::set_theme(mode) {
                s.toast.show(NoticeKind::Error, &e);
            }
            s.poll();
        }
        WM_COMMAND if (111..=114).contains(&wp.0) => s.menu(hwnd, wp.0),
        WM_COMMAND if wp.0 == 115 => s.select(hwnd, 4),
        WM_COMMAND if wp.0 == 116 => s.select(hwnd, 3),
        WM_COMMAND if wp.0 == 117 => {
            if let Some((panel, _, _)) = super::media_panel::active_task() {
                s.select(hwnd, 1);
                set_form(1, panel);
            } else if let Some(job) = s.jobs.first() {
                match job.target {
                    Target::Form(h) => {
                        let _ = super::workspace_form::reveal(h);
                    }
                    Target::Image { .. } => s.select(hwnd, 0),
                }
            }
        }
        WM_COMMAND if wp.0 == 119 => {
            if s.active == 0 && super::collage_workspace::export_active() {
                return LRESULT(0);
            }
            s.dispatch(hwnd, if s.active == 1 { 211 } else { 202 }, None);
        }
        WM_COMMAND if wp.0 == 118 => {
            if let Some((panel, true, _)) = super::media_panel::active_task() {
                unsafe {
                    SendMessageW(panel, WM_COMMAND, Some(WPARAM(32)), None);
                }
            }
        }
        WM_COMMAND if (120..=122).contains(&wp.0) => {
            let result = match wp.0 {
                120 => super::workspace_settings::general(),
                121 => super::workspace_settings::open(),
                _ => super::image_workflows::manage_ocr_async(),
            };
            if let Err(e) = result {
                s.toast.show(NoticeKind::Error, &e);
            }
        }
        windows::Win32::UI::WindowsAndMessaging::WM_GETMINMAXINFO => {
            let info =
                unsafe { &mut *(lp.0 as *mut windows::Win32::UI::WindowsAndMessaging::MINMAXINFO) };
            let minimum =
                super::editor_ui::available_window_size(ui::scale(hwnd, 960), ui::scale(hwnd, 720));
            info.ptMinTrackSize.x = minimum.0;
            info.ptMinTrackSize.y = minimum.1;
        }
        windows::Win32::UI::WindowsAndMessaging::WM_DPICHANGED => {
            let r = unsafe { &*(lp.0 as *const windows::Win32::Foundation::RECT) };
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
                );
            }
            super::desktop_ui::refresh(hwnd);
            s.layout(hwnd);
        }
        windows::Win32::UI::WindowsAndMessaging::WM_MOVE => s.toast.layout(),
        windows::Win32::UI::WindowsAndMessaging::WM_ACTIVATE => {
            if wp.0 & 0xffff == windows::Win32::UI::WindowsAndMessaging::WA_INACTIVE as usize {
                s.toast.suspend();
            } else {
                s.toast.layout();
            }
            return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
        }
        WM_SIZE => s.layout(hwnd),
        WM_TIMER => s.poll(),
        WM_CLOSE => unsafe {
            let _ = PostMessageW(Some(s.video), WM_CLOSE, WPARAM(0), LPARAM(0));
        },
        WM_NCDESTROY => {
            HANDLES.with(|h| h.set(None));
            CUSTOM_CAPTION.with(|v| v.set(false));
            EVENTS.with(|e| e.borrow_mut().clear());
            for j in &s.jobs {
                j.cancel.store(true, Ordering::Release);
            }
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(ptr));
            }
        }
        _ => return unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
    LRESULT(0)
}

#[cfg(test)]
thread_local! {static REVIEW:Cell<bool>=const{Cell::new(false)};}
pub(crate) fn enabled(normal: bool) -> bool {
    #[cfg(test)]
    if REVIEW.with(Cell::get) {
        return true;
    }
    normal
}
#[cfg(test)]
pub(crate) fn enable_review() {
    REVIEW.with(|v| v.set(true));
}

pub(crate) fn result_image(
    image: super::capture_tools::Raster,
    library: &Path,
) -> Result<HWND, String> {
    super::capture_tools::remember_capture(&image)?;
    BACKGROUND.with(|v| v.set(true));
    let result = super::capture_tools::show_image(image, library, false);
    BACKGROUND.with(|v| v.set(false));
    result
}

#[cfg(test)]
pub(crate) fn review_form(tab: usize) -> HWND {
    if tab == 1 {
        return super::preview_window::inspector_panel(video_handle().unwrap())
            .expect("video inspector");
    }
    let root = root().unwrap();
    let s = unsafe { &*(GetWindowLongPtrW(root, GWLP_USERDATA) as *const State) };
    s.forms[tab].expect("review inspector")
}

pub(crate) fn video_handle() -> Option<HWND> {
    HANDLES.with(|v| v.get().map(|h| h.video))
}

pub(crate) fn library_path() -> std::path::PathBuf {
    if let Some(h) = root()
        && let Some(s) = unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *const State).as_ref() }
    {
        return s.library.clone();
    }
    super::ui_preferences::library_directory().unwrap_or_else(|| {
        std::env::var_os("USERPROFILE").map_or_else(
            || PathBuf::from(".tmp/recordings"),
            |p| PathBuf::from(p).join("Videos/Panzo"),
        )
    })
}

pub(crate) fn caption_control_at(hwnd: HWND, point: windows::Win32::Foundation::POINT) -> bool {
    if root() != Some(hwnd) {
        return false;
    }
    let Some(s) = (unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State).as_ref() })
    else {
        return false;
    };
    s.menus
        .iter()
        .chain(std::iter::once(&s.theme_button))
        .any(|h| {
            let mut rect = windows::Win32::Foundation::RECT::default();
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(*h, &mut rect);
            }
            let mut screen = point;
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut screen);
            }
            screen.x >= rect.left
                && screen.x < rect.right
                && screen.y >= rect.top
                && screen.y < rect.bottom
        })
}

pub(crate) fn shortcut_context(index: usize) -> bool {
    use super::shortcuts::{ACTIONS, Target};
    let Some(root) = root() else { return false };
    let Some(s) = (unsafe { (GetWindowLongPtrW(root, GWLP_USERDATA) as *const State).as_ref() })
    else {
        return false;
    };
    match ACTIONS[index].target {
        Target::Shell(_) => true,
        Target::Picture(_) => {
            s.active == 0
                && s.image_view.is_none()
                && super::image_workspace::current_document().is_ok()
        }
        Target::Image(_) => s.active == 0,
        Target::Video(_) => s.active == 1,
        Target::Workspace(202 | 207 | 208 | 240) => {
            s.active == 1 || (s.active == 0 && s.image_view.is_none())
        }
        Target::Workspace(209 | 203 | 212) => s.active == 0 && s.image_view.is_none(),
        Target::Workspace(211) => s.active == 1,
        Target::Workspace(_) => true,
    }
}
