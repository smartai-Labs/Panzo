//! Shortcut settings live inside the owned Settings window; edits remain a draft until saved.
use super::{
    desktop_ui as desk,
    editor_ui::{self, IconKind},
    shortcuts::{self, ACTIONS, Chord, Config, Scope},
    workspace_ui as ui,
};
use std::cell::Cell;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT},
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, GetKeyState, SetFocus},
            WindowsAndMessaging::*,
        },
    },
    core::w,
};
thread_local! {static PANEL:Cell<HWND>=const{Cell::new(HWND(std::ptr::null_mut()))};}
struct Row {
    label: HWND,
    key: HWND,
    scope: HWND,
    clear: HWND,
    reset: HWND,
}
struct State {
    draft: Config,
    heading: HWND,
    description: HWND,
    filter: HWND,
    list: HWND,
    rows: Vec<Row>,
    save: HWND,
    restore: HWND,
    revert: HWND,
    status: HWND,
    scroll: i32,
    listening: Option<usize>,
}
fn state(h: HWND) -> Option<&'static mut State> {
    unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State).as_mut() }
}
pub(crate) fn open() -> Result<(), String> {
    let old = PANEL.with(Cell::get);
    if unsafe { IsWindow(Some(old)) }.as_bool() {
        super::workspace::set_form(3, old);
        return Ok(());
    }
    let parent = super::management_window::parent(3)?;
    ui::register(w!("PanzoShortcutSettings"), Some(proc))?;
    ui::register(w!("PanzoShortcutList"), Some(list_proc))?;
    let h = ui::panel(parent, w!("PanzoShortcutSettings"))?;
    let heading = ui::label(h, "设置 · 快捷键", 10)?;
    let description = ui::label(
        h,
        "点击按键框后按下组合键。应用内快捷键仅在 Panzo 工作区生效；全局快捷键可在后台使用。Esc 取消录入。",
        11,
    )?;
    let filter = ui::combo(
        h,
        &["全部功能", "截图与录制", "通用", "图片工具", "视频工具"],
        0,
        12,
    )?;
    let list = ui::panel(h, w!("PanzoShortcutList"))?;
    desk::install(list);
    ui::show(list, true);
    let mut rows = Vec::new();
    for (i, a) in ACTIONS.iter().enumerate() {
        let id = 1000 + i * 4;
        let label = ui::label(list, a.label, 2000 + i)?;
        let key = ui::button(list, "", id)?;
        let scope = ui::combo(list, &["应用内", "全局"], 0, id + 1)?;
        unsafe {
            let _ = EnableWindow(scope, a.global);
        }
        let clear = ui::button(list, "清除快捷键", id + 2)?;
        desk::style(clear, Some(IconKind::Close), true, false, true);
        let reset = ui::button(list, "恢复此项默认值", id + 3)?;
        desk::style(reset, Some(IconKind::Reset), true, false, true);
        rows.push(Row {
            label,
            key,
            scope,
            clear,
            reset,
        });
    }
    let save = ui::button(h, "保存并生效", 20)?;
    desk::style(save, None, false, true, false);
    let restore = ui::button(h, "恢复全部默认", 21)?;
    let revert = ui::button(h, "撤销未保存修改", 22)?;
    let status = ui::label(h, &shortcuts::issues(), 23)?;
    let s = Box::new(State {
        draft: shortcuts::config(),
        heading,
        description,
        filter,
        list,
        rows,
        save,
        restore,
        revert,
        status,
        scroll: 0,
        listening: None,
    });
    unsafe {
        SetWindowLongPtrW(h, GWLP_USERDATA, Box::into_raw(s) as isize);
    }
    PANEL.with(|v| v.set(h));
    desk::install(h);
    if let Some(s) = state(h) {
        s.sync();
        s.layout(h);
    }
    super::workspace::set_form(3, h);
    Ok(())
}
impl State {
    fn sync(&mut self) {
        for (i, (a, row)) in ACTIONS.iter().zip(&self.rows).enumerate() {
            let b = self.draft.binding(a);
            ui::text(
                row.key,
                &if self.listening == Some(i) {
                    "按下组合键…".into()
                } else {
                    b.chord.map(Chord::label).unwrap_or("未设置".into())
                },
            );
            unsafe {
                SendMessageW(
                    row.scope,
                    CB_SETCURSEL,
                    Some(WPARAM(usize::from(b.scope == Scope::Global))),
                    None,
                );
            }
        }
    }
    fn layout(&mut self, h: HWND) {
        let _batch = ui::layout_batch(h);
        let r = ui::rect(h);
        let d = |n| ui::scale(h, n);
        let width = r.right - d(40);
        ui::place(self.heading, d(20), d(16), width, d(28));
        ui::place(self.description, d(20), d(53), width, d(48));
        ui::place(self.filter, d(20), d(105), d(180), d(29));
        let height = (r.bottom - d(262)).max(d(80));
        ui::place(self.list, d(16), d(145), r.right - d(32), height);
        let visible: Vec<_> = ACTIONS
            .iter()
            .enumerate()
            .filter(|(_, a)| {
                ui::selection(self.filter) == 0 || a.category == ui::selection(self.filter)
            })
            .map(|(i, _)| i)
            .collect();
        self.scroll = self
            .scroll
            .clamp(0, (visible.len() as i32 * d(56) - height).max(0));
        desk::scroll_panel(self.list, visible.len() as i32 * d(56), height, self.scroll);
        // Child positions are batched; use the planned width, not the previous client rect.
        let w = r.right - d(32) - d(18);
        let narrow = w < d(650);
        let row_height = d(56);
        for (i, row) in self.rows.iter().enumerate() {
            let position = visible.iter().position(|p| *p == i);
            for child in [row.label, row.key, row.scope, row.clear, row.reset] {
                ui::show(child, position.is_some());
            }
            if let Some(pos) = position {
                let y = pos as i32 * row_height - self.scroll;
                let keys = if narrow { d(170) } else { d(220) };
                let scopes = d(88);
                let right = w - d(62);
                let key_x = right - scopes - d(10) - keys;
                ui::place(row.label, d(4), y + d(4), (key_x - d(12)).max(d(70)), d(42));
                ui::place(row.key, key_x, y + d(5), keys, d(36));
                ui::place(row.scope, right - scopes, y + d(8), scopes, d(30));
                ui::place(row.clear, right + d(4), y + d(9), d(26), d(28));
                ui::place(row.reset, right + d(34), y + d(9), d(26), d(28));
            }
        }
        let bottom = r.bottom - d(108);
        ui::place(self.save, d(20), bottom, d(118), d(32));
        ui::place(self.restore, d(148), bottom, d(124), d(32));
        ui::place(self.revert, d(282), bottom, d(144), d(32));
        ui::place(self.status, d(20), bottom + d(42), width, d(62));
        desk::invalidate(h);
    }
    fn changed(&mut self) {
        self.sync();
        ui::text(
            self.status,
            "有未保存的修改。保存时检查重复绑定与全局占用；失败会保留原设置。",
        );
    }
    fn command(&mut self, h: HWND, id: usize, notification: usize) {
        if id == 12 {
            self.listening = None;
            self.scroll = 0;
            self.sync();
            self.layout(h);
            return;
        }
        match id {
            20 => {
                self.listening = None;
                self.sync();
                let result = shortcuts::apply(self.draft.clone());
                ui::text(
                    self.status,
                    &match result {
                        Ok(()) => "已保存并即时生效，重启后继续保留。".into(),
                        Err(e) => e,
                    },
                );
            }
            21 => {
                self.listening = None;
                self.draft = Config::default();
                self.changed();
            }
            22 => {
                self.listening = None;
                self.draft = shortcuts::config();
                self.sync();
                ui::text(self.status, "已撤销未保存的修改。");
            }
            1000..=1999 => {
                let i = (id - 1000) / 4;
                if i >= ACTIONS.len() {
                    return;
                }
                let slot = (id - 1000) % 4;
                let a = &ACTIONS[i];
                let mut b = self.draft.binding(a);
                match slot {
                    0 => {
                        self.listening = Some(i);
                        self.sync();
                        unsafe {
                            let _ = SetFocus(Some(self.rows[i].key));
                        }
                        ui::text(
                            self.status,
                            "请按下新的快捷键；Esc 取消。清除绑定请点击右侧 ×。",
                        );
                        return;
                    }
                    1 if notification == CBN_SELCHANGE as usize => {
                        b.scope = if ui::selection(self.rows[i].scope) == 1 {
                            Scope::Global
                        } else {
                            Scope::App
                        };
                    }
                    2 => b.chord = None,
                    3 => b = Config::default().binding(a),
                    _ => return,
                }
                self.listening = None;
                self.draft.bindings.insert(a.id.into(), b);
                self.changed();
            }
            _ => {}
        }
    }
}
fn capture(c: Chord) -> bool {
    let h = PANEL.with(Cell::get);
    if !unsafe { IsWindowVisible(h) }.as_bool() {
        return false;
    }
    let Some(s) = state(h) else { return false };
    let Some(i) = s.listening else { return false };
    if c.key == 27 {
        s.listening = None;
        s.sync();
        ui::text(s.status, "已取消录入。");
        return true;
    }
    if matches!(c.key,0x10..=0x12|0xa0..=0xa5) {
        return true;
    }
    if shortcuts::key_name(c.key).is_empty() || c.modifiers & !7 != 0 {
        ui::text(
            s.status,
            "此按键保留给系统或界面操作，请使用字母、数字、功能键或组合键。",
        );
        return true;
    }
    let a = &ACTIONS[i];
    let mut b = s.draft.binding(a);
    b.chord = Some(c);
    s.draft.bindings.insert(a.id.into(), b);
    s.listening = None;
    s.changed();
    true
}
pub(crate) fn capture_global(c: Chord) -> bool {
    capture(c)
}
pub(crate) fn dialog_message(msg: &MSG) -> bool {
    let h = PANEL.with(Cell::get);
    if !unsafe { IsWindowVisible(h) }.as_bool() || state(h).is_none_or(|s| s.listening.is_none()) {
        return false;
    }
    if !matches!(msg.message, WM_KEYDOWN | WM_SYSKEYDOWN) {
        return false;
    }
    if unsafe { GetAncestor(msg.hwnd, GA_ROOT) } != unsafe { GetAncestor(h, GA_ROOT) } {
        return false;
    }
    let mut c = shortcuts::current_chord(msg.wParam.0 as u32);
    if unsafe { GetKeyState(0x5b) } < 0 || unsafe { GetKeyState(0x5c) } < 0 {
        c.modifiers |= 8;
    }
    capture(c)
}
fn paint(h: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let dc = unsafe { BeginPaint(h, &raw mut ps) };
    let frame = desk::BufferedFrame::new(dc, ui::rect(h));
    let t = editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
    editor_ui::fill_rect(frame.dc, &ui::rect(h), t.palette.window);
    drop(frame);
    unsafe {
        let _ = EndPaint(h, &raw const ps);
    }
}
unsafe extern "system" fn list_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let parent = unsafe { GetParent(h) }.unwrap_or_default();
    let Some(s) = state(parent) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    match m {
        WM_COMMAND => s.command(parent, w.0 & 0xffff, w.0 >> 16),
        WM_MOUSEWHEEL | WM_VSCROLL => {
            let (total, page, position) = desk::panel_scroll_info(h);
            s.scroll = if m == WM_MOUSEWHEEL {
                s.scroll - ((w.0 >> 16) as u16 as i16 as i32 / 120) * ui::scale(h, 56)
            } else {
                match w.0 & 0xffff {
                    0 => s.scroll - ui::scale(h, 40),
                    1 => s.scroll + ui::scale(h, 40),
                    2 => s.scroll - page,
                    3 => s.scroll + page,
                    4 | 5 => position,
                    6 => 0,
                    7 => total,
                    _ => s.scroll,
                }
            };
            s.layout(parent);
        }
        WM_PAINT => paint(h),
        WM_ERASEBKGND => return LRESULT(1),
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}
unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(r) = super::app_menu::window_message(m, l) {
        return r;
    }
    let Some(s) = state(h) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    match m {
        WM_SIZE => s.layout(h),
        WM_COMMAND => s.command(h, w.0 & 0xffff, w.0 >> 16),
        WM_PAINT => paint(h),
        WM_ERASEBKGND => return LRESULT(1),
        WM_NCDESTROY => {
            PANEL.with(|v| v.set(HWND::default()));
            unsafe {
                SetWindowLongPtrW(h, GWLP_USERDATA, 0);
                drop(Box::from_raw(std::ptr::from_mut(s)));
            }
        }
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}

pub(crate) fn cancel_capture() {
    let h = PANEL.with(Cell::get);
    if let Some(s) = state(h)
        && s.listening.take().is_some()
    {
        s.sync();
        ui::text(s.status, "已取消录入，未保存的修改仍保留。");
    }
}
