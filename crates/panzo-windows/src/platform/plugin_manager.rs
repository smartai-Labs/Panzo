#![allow(clippy::many_single_char_names)] // Local native painting coordinates and message parameters.
//! Native plugin list. Entries and operations use the existing verified package store.
use super::{
    desktop_ui as v,
    editor_ui::{self, EditorTheme, IconKind},
    plugins, workspace, workspace_ui as ui,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT},
        UI::{
            Controls::{DRAWITEMSTRUCT, ODS_SELECTED},
            WindowsAndMessaging::{
                DefWindowProcW, GWLP_USERDATA, GetWindowLongPtrW, IDOK, LB_ADDSTRING, LB_GETCURSEL,
                LB_RESETCONTENT, LB_SETCURSEL, LB_SETITEMHEIGHT, LB_SETTOPINDEX, LBS_HASSTRINGS,
                LBS_NOINTEGRALHEIGHT, LBS_NOTIFY, LBS_OWNERDRAWFIXED, MB_ICONQUESTION, MB_OKCANCEL,
                SendMessageW, SetWindowLongPtrW, WINDOW_STYLE, WM_COMMAND, WM_DRAWITEM,
                WM_NCDESTROY, WM_PAINT, WM_SIZE, WS_TABSTOP, WS_VSCROLL,
            },
        },
    },
    core::w,
};
struct State {
    entries: Vec<plugins::Entry>,
    list: HWND,
    buttons: Vec<HWND>,
    status: HWND,
    toggles: Vec<HWND>,
}
thread_local! {static MANAGER:std::cell::Cell<Option<HWND>>=const{std::cell::Cell::new(None)};}
pub(crate) fn open() -> Result<(), String> {
    if let Some(h) = MANAGER.with(std::cell::Cell::get).filter(|h| {
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(*h)) }.as_bool()
    }) {
        unsafe {
            SendMessageW(h, WM_COMMAND, Some(WPARAM(6)), None);
        }
        workspace::set_form(4, h);
        return Ok(());
    }
    let parent = super::management_window::parent(4)?;
    ui::register(w!("PanzoPluginManager"), Some(procedure))?;
    let h = ui::panel(parent, w!("PanzoPluginManager"))?;
    let list = ui::child(
        h,
        w!("LISTBOX"),
        "",
        100,
        WS_TABSTOP
            | WS_VSCROLL
            | WINDOW_STYLE(
                (LBS_OWNERDRAWFIXED | LBS_HASSTRINGS | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT) as u32,
            ),
    )?;
    let buttons = [
        "安装本地插件",
        "启用",
        "停用",
        "回退上一版本",
        "卸载插件",
        "刷新",
    ]
    .iter()
    .enumerate()
    .map(|(i, n)| ui::button(h, n, i + 1))
    .collect::<Result<Vec<_>, _>>()?;
    v::style(buttons[0], None, false, false, false);
    v::install(h);
    v::list(list);
    unsafe {let _=windows::Win32::UI::Shell::SetWindowSubclass(list,Some(plugin_list_proc),0x505a141,0);}
    let mut s = State {
        entries: Vec::new(),
        toggles:Vec::new(),
        list,
        buttons,
        status: ui::label(
            h,
            "停用后保留素材和识别结果；仅从信任的来源安装原生插件。",
            20,
        )?,
    };
    s.reload()?;
    s.layout(h);
    unsafe {
        SetWindowLongPtrW(h, GWLP_USERDATA, Box::into_raw(Box::new(s)) as isize);
        windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(h), 1, 800, None);
    }
    MANAGER.with(|v| v.set(Some(h)));
    workspace::set_form(4, h);
    Ok(())
}
/// Return to the real manager and reveal the package that was just installed.
pub(crate) fn show_installed(id: &str) -> Result<(), String> {
    open()?;
    if let Some(h) = MANAGER.with(std::cell::Cell::get) {
        let s = unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State).as_mut() };
        if let Some(s) = s {
            if let Some(index) = s.entries.iter().position(|entry| entry.id == id) {
                unsafe {
                    SendMessageW(s.list, LB_SETCURSEL, Some(WPARAM(index)), None);
                    SendMessageW(
                        s.list,
                        LB_SETTOPINDEX,
                        Some(WPARAM(index.saturating_sub(2))),
                        None,
                    );
                }
                let entry = &s.entries[index];
                ui::text(
                    s.status,
                    &format!(
                        "已安装：{} · {}",
                        entry.name,
                        if entry.enabled {
                            "已启用"
                        } else {
                            "已停用，可点击启用"
                        }
                    ),
                );
                v::invalidate(s.list);
            }
        }
    }
    Ok(())
}

impl State {
    fn reload(&mut self) -> Result<(), String> {
        let h=unsafe {windows::Win32::UI::WindowsAndMessaging::GetParent(self.list)}.map_err(|e|e.to_string())?;
        let previous = unsafe { SendMessageW(self.list, LB_GETCURSEL, None, None) }.0;
        let id = usize::try_from(previous)
            .ok()
            .and_then(|i| self.entries.get(i))
            .map(|e| e.id.clone());
        let entries = plugins::with_store(plugins::Store::refreshed_entries)?;
        if self.entries == entries {
            self.update_commands();
            self.layout_toggles(h);
            return Ok(());
        }
        self.entries = entries;
        while self.toggles.len()<self.entries.len() {
            let i=self.toggles.len();let toggle=ui::button(h,"启用或停用插件",200+i)?;
            v::style(toggle,None,true,false,true);self.toggles.push(toggle);
        }

        unsafe {
            SendMessageW(self.list, LB_RESETCONTENT, None, None);
        }
        for e in &self.entries {
            let n = ui::wide(&e.name);
            unsafe {
                SendMessageW(
                    self.list,
                    LB_ADDSTRING,
                    None,
                    Some(LPARAM(n.as_ptr() as isize)),
                );
            }
        }
        let i = self
            .entries
            .iter()
            .position(|e| Some(&e.id) == id.as_ref())
            .unwrap_or(0);
        unsafe {
            SendMessageW(self.list, LB_SETCURSEL, Some(WPARAM(i)), None);
        }
        let external = self.entries.iter().filter(|e| e.external).count();
        ui::text(
            self.status,
            &format!(
                "已安装 {external} 个独立插件 · 共 {} 项功能",
                self.entries.len()
            ),
        );
        super::image_workspace::refresh_plugins();
        self.update_commands();
        v::invalidate(self.list);
        self.layout_toggles(h);
        Ok(())
    }
    fn update_commands(&self) {
        let index = unsafe { SendMessageW(self.list, LB_GETCURSEL, None, None) }.0;
        let selected = usize::try_from(index)
            .ok()
            .and_then(|i| self.entries.get(i));
        for (i, button) in self.buttons.iter().enumerate() {
            let enabled = match i {
                0 | 5 => true,
                1 => selected.is_some_and(|e| !e.enabled),
                2 => selected.is_some_and(|e| e.enabled),
                3 => selected.is_some_and(|e| e.external && e.rollback && !e.busy),
                4 => selected.is_some_and(|e| e.external && !e.busy),
                _ => false,
            };
            unsafe {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(*button, enabled);
            }
        }
    }
    fn layout(&self, h: HWND) {
        let r = ui::rect(h);
        let d = |n| ui::scale(h, n);
        for (i,button) in self.buttons.iter().enumerate() {ui::show(*button,[0,3,4,5].contains(&i));}
        ui::place(self.buttons[0],r.right-d(154),d(20),d(134),d(30));
        ui::place(self.buttons[5],r.right-d(190),d(20),d(28),d(30));
        v::style(self.buttons[5],Some(IconKind::Restore),true,false,true);
        ui::place(self.list,d(20),d(66),r.right-d(40),(r.bottom-d(124)).max(1));
        unsafe {SendMessageW(self.list,LB_SETITEMHEIGHT,Some(WPARAM(0)),Some(LPARAM(d(96) as isize)));}
        for (i,index) in [3,4].into_iter().enumerate() {ui::place(self.buttons[index],d(20+120*i as i32),r.bottom-d(44),d(110),d(28));v::style(self.buttons[index],None,true,false,false);}
        ui::place(self.status,d(270),r.bottom-d(46),r.right-d(290),d(32));
        self.layout_toggles(h);v::invalidate(h);
    }
    fn layout_toggles(&self,h:HWND) {
        let r=ui::rect(h);let d=|n|ui::scale(h,n);
        let first=unsafe {SendMessageW(self.list,windows::Win32::UI::WindowsAndMessaging::LB_GETTOPINDEX,None,None)}.0.max(0) as i32;
        for (i,toggle) in self.toggles.iter().enumerate() {
            let y=d(66+(i as i32-first)*96+31);
            let entry=self.entries.get(i);
            ui::show(*toggle,entry.is_some() && y>=d(66) && y+d(28)<=r.bottom-d(58));
            if let Some(e)=entry {
                ui::place(*toggle,r.right-d(72),y,d(40),d(28));
                v::choice(*toggle,v::ChoiceVisual::Toggle(e.enabled));
                ui::text(*toggle,&format!("{}{}",if e.enabled {"停用"}else{"启用"},e.name));
                unsafe {let _=windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(*toggle,!e.busy);}
            }
        }
    }
    fn action(&mut self, h: HWND, id: usize) -> Result<(), String> {
        if id == 1 {
            super::plugin_ui::choose_package()?;
            return Ok(());
        }
        if id == 6 {
            self.reload()?;
            return Ok(());
        }
        let index = usize::try_from(unsafe { SendMessageW(self.list, LB_GETCURSEL, None, None) }.0)
            .map_err(|_| "请选择插件")?;
        let e = self.entries.get(index).ok_or("请选择插件")?.clone();
        match id {
            2 => plugins::with_store(|s| s.set_enabled(&e.id, true))?,
            3 => plugins::with_store(|s| s.set_enabled(&e.id, false))?,
            4 => plugins::with_store(|s| s.rollback(&e.id))?,
            5 => {
                if !e.external {
                    return Err("内置功能可停用，随主程序更新".into());
                }
                let message = ui::wide(&format!("卸载“{}”？素材和识别结果会保留。", e.name));
                if unsafe {
                    crate::platform::app_dialog::message(
                        Some(h),
                        windows::core::PCWSTR(message.as_ptr()),
                        w!("卸载插件"),
                        MB_OKCANCEL | MB_ICONQUESTION,
                    )
                } != IDOK
                {
                    return Ok(());
                }
                plugins::with_store(|s| s.uninstall(&e.id))?;
            }
            _ => return Ok(()),
        }
        self.reload()?;
        ui::text(self.status, "操作完成，用户素材与结果已保留。");
        Ok(())
    }
    fn draw(&self, item: &DRAWITEMSTRUCT) {
        let Some(e) = self.entries.get(item.itemID as usize) else {
            return;
        };
        let t = EditorTheme::current(editor_ui::window_dpi(item.hwndItem));
        let d = |n| t.scale(n);
        let r = item.rcItem;
        let w = r.right - r.left;
        let selected=item.itemState.0 & ODS_SELECTED.0 != 0;
        editor_ui::fill_rect(item.hDC,&r,if selected {t.palette.raised}else{t.palette.panel});
        let icon=if e.name.contains("声音") {IconKind::Microphone}else if e.name.contains("识图") {IconKind::Vision}else if e.name.contains("拼图") {IconKind::Collage}else{IconKind::Image};
        editor_ui::draw_icon(item.hDC,v::rect(d(4),r.top+d(25),d(24),d(24)),icon,t.palette.text_secondary);
        v::text_style(item.hDC,v::rect(d(44),r.top+d(13),w-d(110),d(22)),&e.name,t.palette.text,t.dpi,false,editor_ui::TextStyle::Heading);
        v::text_style(item.hDC,v::rect(d(44),r.top+d(39),w-d(110),d(20)),&format!("{} · {}",if e.external {"独立插件"}else{"内置功能"},e.version),t.palette.text_secondary,t.dpi,false,editor_ui::TextStyle::Caption);
        v::text_style(item.hDC,v::rect(d(44),r.top+d(62),w-d(110),d(20)),if e.busy {"正在使用 · 任务结束后可停用"}else if e.enabled {"已启用"}else{"已停用 · 已有作品保留"},t.palette.text_muted,t.dpi,false,editor_ui::TextStyle::Caption);
        v::line(item.hDC,v::rect(0,r.bottom-1,w,1),t);

    }
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let p = unsafe { GetWindowLongPtrW(h, GWLP_USERDATA) } as *mut State;
    let Some(s) = (unsafe { p.as_mut() }) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    match m {
        WM_SIZE => s.layout(h),
        windows::Win32::UI::WindowsAndMessaging::WM_TIMER => {
            if !super::app_dialog::is_open()
                && unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(h) }.as_bool()
            {
                let _ = s.reload();
            }
        }
        WM_COMMAND if w.0 >> 16 == 0 && w.0>=200 => {
            let i=w.0-200;
            if let Some(e)=s.entries.get(i).cloned().filter(|e|!e.busy) {
                unsafe {SendMessageW(s.list,LB_SETCURSEL,Some(WPARAM(i)),None);}
                if let Err(e)=s.action(h,if e.enabled {3}else{2}) {ui::text(s.status,&e);}
            }
        }
        WM_COMMAND if w.0 & 65535 == 100 => {
            s.update_commands();
            let index = unsafe { SendMessageW(s.list, LB_GETCURSEL, None, None) }.0;
            if let Some(e) = usize::try_from(index).ok().and_then(|i| s.entries.get(i)) {
                ui::text(
                    s.status,
                    if !e.external {
                        "内置功能可启用或停用，随主程序更新。"
                    } else if e.busy {
                        "任务正在运行；可停用以阻止新任务，完成后可卸载或回退。"
                    } else if !e.rollback {
                        "没有可回退的版本；停用或卸载会保留用户素材与结果。"
                    } else {
                        "可回退上一版本；停用或卸载会保留用户素材与结果。"
                    },
                );
            }
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &raw mut ps) };
            let r = ui::rect(h);
            let t = EditorTheme::current(editor_ui::window_dpi(h));
            let d = |v| t.scale(v);
            editor_ui::fill_rect(dc,&r,t.palette.panel);
            v::text_style(dc,v::rect(d(20),d(20),d(250),d(30)),"按需启用功能",t.palette.text_muted,t.dpi,false,editor_ui::TextStyle::Caption);
            v::line(dc,v::rect(0,r.bottom-d(58),r.right,1),t);
            unsafe {
                let _ = EndPaint(h, &raw const ps);
            }
        }
        WM_DRAWITEM if w.0 == 100 => {
            s.draw(unsafe { &*(l.0 as *const DRAWITEMSTRUCT) });
            return LRESULT(1);
        }
        WM_COMMAND if w.0 >> 16 == 0 && (1..=6).contains(&w.0) => {
            if let Err(e) = s.action(h, w.0) {
                ui::text(s.status, &e);
            }
        }
        WM_NCDESTROY => unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(h), 1);
            MANAGER.with(|v| v.set(None));
            SetWindowLongPtrW(h, GWLP_USERDATA, 0);
            drop(Box::from_raw(p));
        },
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}

unsafe extern "system" fn plugin_list_proc(h:HWND,m:u32,w:WPARAM,l:LPARAM,id:usize,_:usize)->LRESULT {
    use windows::Win32::{UI::{Shell::{DefSubclassProc,RemoveWindowSubclass},WindowsAndMessaging::*}};
    let result=unsafe{DefSubclassProc(h,m,w,l)};
    if matches!(m,WM_VSCROLL|WM_MOUSEWHEEL|WM_KEYDOWN|LB_SETTOPINDEX) {
        if let Ok(parent)=unsafe{GetParent(h)} {if let Some(s)=unsafe{(GetWindowLongPtrW(parent,GWLP_USERDATA) as *mut State).as_ref()} {s.layout_toggles(parent);}}
    }
    if m==WM_NCDESTROY {unsafe{let _=RemoveWindowSubclass(h,Some(plugin_list_proc),id);}}
    result
}
