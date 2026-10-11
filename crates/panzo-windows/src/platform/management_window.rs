//! Owned management windows; hiding a window preserves drafts and in-flight tasks.
use super::{
    desktop_ui as desk,
    editor_ui::{self, EditorTheme},
    workspace_ui as ui,
};
use std::cell::RefCell;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::{PCWSTR, w},
};
thread_local! {static WINDOWS:RefCell<Vec<(usize,HWND,bool)>>=const{RefCell::new(Vec::new())};}
struct State {
    tab: usize,
    fitted: bool,
    panel: Option<HWND>,
    navigation: Vec<HWND>,
}
pub(crate) fn is_custom(h: HWND) -> bool {
    WINDOWS.with(|v| v.borrow().iter().any(|(_, w, c)| *w == h && *c))
}
pub(crate) fn get(tab: usize) -> Option<HWND> {
    WINDOWS.with(|v| {
        v.borrow()
            .iter()
            .find(|(t, h, _)| *t == tab && unsafe { IsWindow(Some(*h)) }.as_bool())
            .map(|(_, h, _)| *h)
    })
}
pub(crate) fn parent(tab: usize) -> Result<HWND, String> {
    if let Some(h) = get(tab) {
        return Ok(h);
    }
    let owner = super::workspace::root().ok_or("工作区尚未就绪")?;
    ui::register(w!("PanzoManagementWindow"), Some(procedure))?;
    let title = ui::wide(if tab == 3 {
        "设置"
    } else {
        "插件"
    });
    let mut r = windows::Win32::Foundation::RECT::default();
    unsafe {
        let _ = GetWindowRect(owner, &raw mut r);
    }
    let (width, height) = editor_ui::available_window_size(
        ui::scale(owner, 740),
        ui::scale(owner, if tab == 3 { 640 } else { 600 }),
    );
    let h = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            w!("PanzoManagementWindow"),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            r.left + ((r.right - r.left - width) / 2).max(0),
            r.top + ((r.bottom - r.top - height) / 2).max(0),
            width,
            height,
            Some(owner),
            None,
            Some(GetModuleHandleW(None).map_err(|e| e.to_string())?.into()),
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    let color = EditorTheme::current(editor_ui::window_dpi(h))
        .palette
        .window;
    let custom = unsafe {
        windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            h,
            windows::Win32::Graphics::Dwm::DWMWA_CAPTION_COLOR,
            (&raw const color).cast(),
            size_of_val(&color) as u32,
        )
    }
    .is_err();
    WINDOWS.with(|v| v.borrow_mut().push((tab, h, custom)));
    let navigation = if tab == 3 {
        ["外观与行为", "识图服务", "OCR 语言", "快捷键", "AI 字幕"]
            .iter()
            .enumerate()
            .map(|(i, t)| ui::button(h, t, 120 + i))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    let state = Box::new(State {
        tab,
        fitted: false,
        panel: None,
        navigation,
    });
    unsafe {
        SetWindowLongPtrW(h, GWLP_USERDATA, Box::into_raw(state) as isize);
    }
    desk::install(h);
    editor_ui::apply_window_chrome(h);
    if custom {
        super::preview_window::editor_titlebar::initialize(h);
    }
    Ok(h)
}
pub(crate) fn present(tab: usize, panel: HWND) {
    if let Ok(h) = parent(tab) {
        if let Some(s) = unsafe { state(h) } {
            if !s.fitted {
                s.fitted = true;
                if tab == 3
                    && let Some(height) = super::workspace_form::preferred_height(panel)
                {
                    let size = editor_ui::available_window_size(
                        ui::scale(h, 740),
                        ui::scale(h, (height + 32).clamp(520, 640)),
                    );
                    unsafe {
                        let _ = SetWindowPos(
                            h,
                            None,
                            0,
                            0,
                            size.0,
                            size.1,
                            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                    }
                }
            }
            if let Some(old) = s.panel.replace(panel) {
                if old != panel {
                    super::shortcut_settings::cancel_capture();
                    ui::show(old, false);
                }
            }
            layout(h, s);
            ui::show(panel, true);
        }
        unsafe {
            let _ = ShowWindow(h, SW_SHOWNORMAL);
            let _ = SetForegroundWindow(h);
        }
    }
}
pub(crate) fn show(tab: usize) -> bool {
    let Some(h) = get(tab) else { return false };
    if unsafe { state(h) }.is_none_or(|s| s.panel.is_none()) {
        return false;
    }
    unsafe {
        let _ = ShowWindow(h, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(h);
    }
    true
}
pub(crate) fn hide_panel(panel: HWND) -> bool {
    let handles = WINDOWS.with(|v| v.borrow().iter().map(|(_, h, _)| *h).collect::<Vec<_>>());
    for h in handles {
        if unsafe { state(h) }.is_some_and(|s| s.panel == Some(panel)) {
            ui::show(h, false);
            return true;
        }
    }
    false
}
pub(crate) fn refresh() {
    let handles = WINDOWS.with(|v| v.borrow().iter().map(|(_, h, _)| *h).collect::<Vec<_>>());
    for h in handles {
        desk::refresh(h);
        if let Some(s) = unsafe { state(h) } {
            layout(h, s);
        }
    }
}
pub(crate) fn dialog_message(msg: &MSG) -> bool {
    let handles = WINDOWS.with(|v| v.borrow().iter().map(|(_, h, _)| *h).collect::<Vec<_>>());
    handles.into_iter().any(|h| {
        unsafe { IsWindowVisible(h) }.as_bool()
            && (msg.hwnd == h || unsafe { IsChild(h, msg.hwnd) }.as_bool())
            && unsafe { IsDialogMessageW(h, msg) }.as_bool()
    })
}
unsafe fn state(h: HWND) -> Option<&'static mut State> {
    unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State).as_mut() }
}
fn layout(h: HWND, s: &State) {
    let r = ui::rect(h);
    let d = |n| ui::scale(h, n);
    let cap = super::workspace::caption_height(h);
    let side = if s.tab == 3 { d(158) } else { 0 };
    for (i, b) in s.navigation.iter().enumerate() {
        ui::place(*b, d(12), cap + d(20 + 38 * match i {0=>0,1=>1,4=>2,3=>3,_=>4}), d(134), d(32));
        desk::style(*b, None, true, false, false);
    }
    if let Some(panel) = s.panel {
        ui::place(
            panel,
            side,
            cap,
            (r.right - side).max(1),
            (r.bottom - cap).max(1),
        );
        let page = super::workspace_form::identity(panel)
            .and_then(|id| id.navigation())
            .or(Some(3));
        for (i, button) in s.navigation.iter().enumerate() {
            desk::selected(*button, page == Some(i));
        }
    }
    desk::invalidate(h);
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if is_custom(h)
        && let Some(r) = super::preview_window::editor_titlebar::window_message(h, m, w, l)
    {
        return r;
    }
    if let Some(r) = super::app_menu::window_message(m, l) {
        return r;
    }
    let Some(s) = (unsafe { state(h) }) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    match m {
        WM_CLOSE => {
            super::shortcut_settings::cancel_capture();
            ui::show(h, false);
            if let Some(root) = super::workspace::root() {
                unsafe {
                    let _ = SetForegroundWindow(root);
                }
            }
        }
        WM_SIZE => layout(h, s),
        WM_COMMAND if (120..=124).contains(&w.0) => {
            let result = match w.0 {
                120 => super::workspace_settings::general(),
                121 => super::workspace_settings::open(),
                122 => super::image_workflows::manage_ocr_async(),
                123 => super::shortcut_settings::open(),
                _ => super::speech_settings::open(),
            };
            if let Err(e) = result {
                super::workspace::notice_error(&e);
            }
        }
        WM_GETMINMAXINFO => {
            let r = unsafe { &mut *(l.0 as *mut MINMAXINFO) };
            let minimum =
                super::editor_ui::available_window_size(ui::scale(h, 620), ui::scale(h, 500));
            r.ptMinTrackSize.x = minimum.0;
            r.ptMinTrackSize.y = minimum.1;
        }
        WM_DPICHANGED => {
            let r = unsafe { &*(l.0 as *const windows::Win32::Foundation::RECT) };
            unsafe {
                let _ = SetWindowPos(
                    h,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            desk::refresh(h);
            layout(h, s);
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &raw mut ps) };
            let frame = super::desktop_ui::BufferedFrame::new(dc, ui::rect(h));
            let dc = frame.dc;
            let t = EditorTheme::current(editor_ui::window_dpi(h));
            editor_ui::fill_rect(dc, &ui::rect(h), t.palette.window);
            let cap = super::workspace::caption_height(h);
            if cap > 0 {
                editor_ui::fill_opaque_rect(
                    dc,
                    desk::rect(0, 0, ui::rect(h).right, cap),
                    t.palette.window,
                );
                desk::text(
                    dc,
                    desk::rect(t.scale(16), 0, ui::rect(h).right - t.scale(170), cap),
                    &ui::value(h),
                    t.palette.text,
                    t.dpi,
                    false,
                );
                super::preview_window::editor_titlebar::paint_system_buttons(h, dc, t);
            }
            drop(frame);
            unsafe {
                let _ = EndPaint(h, &raw const ps);
            }
        }
        WM_NCDESTROY => {
            WINDOWS.with(|v| v.borrow_mut().retain(|(_, w, _)| *w != h));
            unsafe {
                SetWindowLongPtrW(h, GWLP_USERDATA, 0);
                drop(Box::from_raw(std::ptr::from_mut(s)));
            }
        }
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}
