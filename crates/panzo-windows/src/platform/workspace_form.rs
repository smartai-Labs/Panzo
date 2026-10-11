#![allow(clippy::cast_possible_wrap)] // Control indexes are bounded by small static field lists.
//! Nonblocking property inspector. Draft values survive workspace switches.
use super::workspace_ui as ui;
#[path = "form_schema.rs"]
mod schema;
pub(crate) use schema::{FieldKind, FieldSpec, FormId};
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
thread_local! {static FORMS:RefCell<Vec<(usize,FormId,String,HWND)>>=const{RefCell::new(Vec::new())};}
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::WindowsAndMessaging::{
            DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, SendMessageW,
            SetWindowLongPtrW, WM_CLOSE, WM_COMMAND, WM_NCDESTROY, WM_SIZE,
        },
    },
    core::w,
};
pub(crate) type Apply = Box<dyn FnMut(usize, Vec<String>) -> Result<String, String>>;
struct State {
    id: FormId,
    body: Option<HWND>,
    groups: Vec<HWND>,
    specs: Vec<FieldSpec>,
    heading: HWND,
    description: HWND,
    fields: Vec<(HWND, HWND)>,
    buttons: Vec<HWND>,
    status: HWND,
    apply: Option<Apply>,
    close: HWND,
    cancel: HWND,
    running: Option<Arc<AtomicBool>>,
    scroll: std::cell::Cell<i32>,
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn open(
    tab: usize,
    id: FormId,
    title: &str,
    description: &str,
    fields: &[(&str, String)],
    options: &[(usize, &[&str])],
    passwords: &[usize],
    buttons: &[&str],
    apply: Apply,
) -> Result<HWND, String> {
    open_scoped(
        tab,
        id,
        title,
        "",
        description,
        fields,
        options,
        passwords,
        buttons,
        apply,
    )
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn open_scoped(
    tab: usize,
    id: FormId,
    title: &str,
    context: &str,
    description: &str,
    fields: &[(&str, String)],
    options: &[(usize, &[&str])],
    passwords: &[usize],
    buttons: &[&str],
    apply: Apply,
) -> Result<HWND, String> {
    let mut scope = if id.project_scoped() {
        super::workspace::video_handle()
            .map(super::preview_window::workspace_project_key)
            .unwrap_or_default()
    } else {
        String::new()
    };
    scope.push_str("::");
    scope.push_str(context);
    let reusable = tab != 4;
    let cached = FORMS.with(|v| {
        v.borrow()
            .iter()
            .find(|(t, n, key, h)| {
                *t == tab
                    && *n == id
                    && key == &scope
                    && unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(*h)) }
                        .as_bool()
            })
            .map(|(_, _, _, h)| *h)
    });
    if reusable && let Some(h) = cached {
        if let Some(s) = unsafe { state(h) }
            && s.running.is_none()
            && id != FormId::Batch
        {
            s.apply = Some(apply);
            ui::text(s.description, description);
            for (index, choices) in options {
                if let Some((_, field)) = s.fields.get(*index) {
                    let previous = ui::value(*field);
                    unsafe {
                        SendMessageW(
                            *field,
                            windows::Win32::UI::WindowsAndMessaging::CB_RESETCONTENT,
                            None,
                            None,
                        );
                    }
                    for choice in *choices {
                        let value = ui::wide(choice);
                        unsafe {
                            SendMessageW(
                                *field,
                                windows::Win32::UI::WindowsAndMessaging::CB_ADDSTRING,
                                None,
                                Some(LPARAM(value.as_ptr() as isize)),
                            );
                        }
                    }
                    let selection = choices
                        .iter()
                        .position(|v| *v == previous)
                        .or_else(|| {
                            fields
                                .get(*index)
                                .and_then(|(_, v)| choices.iter().position(|c| *c == v))
                        })
                        .unwrap_or(0);
                    unsafe {
                        SendMessageW(
                            *field,
                            windows::Win32::UI::WindowsAndMessaging::CB_SETCURSEL,
                            Some(WPARAM(selection)),
                            None,
                        );
                    }
                }
            }
        }
        super::workspace::set_form(tab, h);
        return Ok(h);
    }
    let parent = if tab >= 3 {
        super::management_window::parent(tab)?
    } else {
        super::workspace::root().ok_or("工作区尚未就绪")?
    };
    ui::register(w!("PanzoWorkspaceForm"), Some(procedure))?;
    let hwnd = ui::panel(parent, w!("PanzoWorkspaceForm"))?;
    let specs = id.fields();
    debug_assert!(passwords.iter().all(|i| {
        specs
            .get(*i)
            .is_some_and(|f| matches!(f.kind, FieldKind::Secret))
    }));
    debug_assert!(specs.iter().all(|spec| !spec.id.is_empty()));
    if specs.len() != fields.len() {
        return Err("表单字段与定义不匹配".into());
    }
    let result = (|| {
        let mut controls = Vec::new();
        for (i, (label, value)) in fields.iter().enumerate() {
            let l = ui::label(hwnd, label, 200 + i)?;
            let h = if let Some((_, choices)) = options.iter().find(|(index, _)| *index == i) {
                ui::combo(
                    hwnd,
                    choices,
                    choices.iter().position(|s| *s == value).unwrap_or(0),
                    100 + i,
                )?
            } else if matches!(specs[i].kind, FieldKind::Directory) {
                super::path_field::create(hwnd, value, 100 + i)?
            } else if let FieldKind::Number { presets, .. } = specs[i].kind {
                ui::number(hwnd, value, presets, 100 + i, None)?
            } else if matches!(specs[i].kind, FieldKind::Color) {
                ui::button(hwnd, value, 100 + i)?
            } else {
                ui::edit(hwnd, value, 100 + i, false)?
            };
            if matches!(specs[i].kind, FieldKind::Secret) {
                unsafe {
                    SendMessageW(
                        h,
                        windows::Win32::UI::Controls::EM_SETPASSWORDCHAR,
                        Some(WPARAM(0x25cf)),
                        None,
                    );
                }
            }
            controls.push((l, h));
        }
        Ok::<_, String>(State {
            id,
            body: None,
            groups: Vec::new(),
            specs,
            scroll: std::cell::Cell::new(0),
            heading: ui::label(hwnd, title, 10)?,
            description: {
                let h = ui::label(hwnd, description, 11)?;
                unsafe {
                    SendMessageW(
                        h,
                        windows::Win32::UI::Controls::EM_SETREADONLY,
                        Some(WPARAM(1)),
                        None,
                    );
                }
                h
            },
            fields: controls,
            buttons: buttons
                .iter()
                .enumerate()
                .map(|(i, s)| ui::button(hwnd, s, 1 + i))
                .collect::<Result<_, _>>()?,
            status: ui::edit(hwnd, "", 12, true)?,
            apply: Some(apply),
            close: ui::button(hwnd, if tab >= 3 { "关闭" } else { "收起" }, 50)?,
            cancel: ui::button(hwnd, "取消任务", 51)?,
            running: None,
        })
    })();
    let mut state = match result {
        Ok(s) => s,
        Err(e) => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return Err(e);
        }
    };
    if matches!(
        id,
        FormId::General | FormId::Vision | FormId::Speech | FormId::Ocr
    ) {
        ui::register(w!("PanzoSettingsBody"), Some(settings_body_proc))?;
        let body = ui::panel(hwnd, w!("PanzoSettingsBody"))?;
        super::desktop_ui::install(body);
        ui::show(body, true);
        for child in state
            .fields
            .iter()
            .flat_map(|(l, f)| [*l, *f])
            .chain([state.heading, state.description])
            .chain(state.buttons.iter().skip(1).copied())
        {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetParent(child, Some(body));
            }
        }
        state.groups = if id == FormId::General {
            ["界面", "文件与窗口"]
        } else if id == FormId::Vision {
            ["多模态模型", "识别偏好"]
        } else if id == FormId::Speech {
            ["语音模型", "识别偏好"]
        } else {
            ["识别语言", "系统语言"]
        }
        .iter()
        .enumerate()
        .map(|(i, text)| ui::label(body, text, 300 + i))
        .collect::<Result<Vec<_>, _>>()?;
        state.body = Some(body);
    }
    if let Some(first) = state.buttons.first() {
        super::desktop_ui::style(*first, None, false, true, false);
    }
    unsafe {
        SendMessageW(
            state.status,
            windows::Win32::UI::Controls::EM_SETREADONLY,
            Some(WPARAM(1)),
            None,
        );
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(Box::new(state)) as isize);
    }
    if let Some(s) = unsafe { self::state(hwnd) } {
        ui::show(s.status, false);
        ui::show(s.cancel, false);
    }
    if reusable {
        FORMS.with(|v| v.borrow_mut().push((tab, id, scope, hwnd)));
    }
    super::workspace::set_form(tab, hwnd);
    Ok(hwnd)
}
pub(crate) fn feedback(hwnd: HWND, message: &str) {
    if let Some(s) = unsafe { state(hwnd) } {
        ui::text(s.status, message);
        ui::show(s.status, !message.is_empty());
    }
}
#[allow(clippy::many_single_char_names)] // Rectangle coordinates and control handles.
fn layout(hwnd: HWND, s: &State) {
    if s.body.is_some() {
        settings_layout(hwnd, s);
        return;
    }
    let _batch = ui::layout_batch(hwnd);
    let r = ui::rect(hwnd);
    let d = |v| ui::scale(hwnd, v);
    let font = super::editor_ui::font_handle(
        super::editor_ui::TextStyle::Heading,
        super::editor_ui::window_dpi(hwnd),
    );
    unsafe {
        if SendMessageW(
            s.heading,
            windows::Win32::UI::WindowsAndMessaging::WM_GETFONT,
            None,
            None,
        )
        .0 != font.0 as isize
        {
            SendMessageW(
                s.heading,
                windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
        }
    }
    let width = r.right.min(d(720));
    let narrow = width < d(520);
    let row = d(if narrow { 68 } else { 44 });
    let field_top = d(if narrow { 133 } else { 116 });
    let columns = if narrow { 2 } else { 3 };
    let button_width = ((width - d(40)) / columns).min(d(190));
    let button_top = field_top + row * s.fields.len() as i32 + d(12);
    let status_top =
        button_top + d(36) * ((s.buttons.len() as i32 + columns - 1) / columns) + d(12);
    let total = status_top + d(116);
    let offset = s.scroll.get().clamp(0, (total - r.bottom).max(0));
    s.scroll.set(offset);
    super::desktop_ui::scroll_panel(hwnd, total, r.bottom, offset);
    let place = |h, x, y, w, height| ui::place(h, x, y - offset, w, height);
    place(s.heading, d(20), d(16), width - d(112), d(28));
    place(s.close, width - d(80), d(14), d(62), d(27));
    place(s.cancel, width - d(182), d(14), d(96), d(27));
    place(
        s.description,
        d(20),
        d(58),
        width - d(40),
        d(if narrow { 66 } else { 48 }),
    );
    for (i, (label, field)) in s.fields.iter().enumerate() {
        let y = field_top + row * i as i32;
        if narrow {
            place(*label, d(20), y, width - d(40), d(22));
            place(*field, d(20), y + d(25), width - d(40), d(29));
        } else {
            place(*label, d(20), y + d(3), d(154), d(24));
            place(*field, d(182), y, width - d(202), d(29));
        }
    }
    for (i, b) in s.buttons.iter().enumerate() {
        place(
            *b,
            d(20) + button_width * (i as i32 % columns),
            button_top + d(36) * (i as i32 / columns),
            button_width - d(7),
            d(28),
        );
    }
    place(s.status, d(20), status_top, width - d(40), d(88));
    super::desktop_ui::invalidate(hwnd);
}
unsafe fn state(hwnd: HWND) -> Option<&'static mut State> {
    unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State).as_mut() }
}
unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(s) = (unsafe { state(hwnd) }) else {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    };
    match msg {
        windows::Win32::UI::WindowsAndMessaging::WM_VSCROLL
        | windows::Win32::UI::WindowsAndMessaging::WM_MOUSEWHEEL => {
            use windows::Win32::UI::WindowsAndMessaging::WM_MOUSEWHEEL;
            let (total, page, position) = super::desktop_ui::panel_scroll_info(hwnd);
            let next = if msg == WM_MOUSEWHEEL {
                s.scroll.get() - (i32::from((wp.0 >> 16) as u16 as i16) / 120) * ui::scale(hwnd, 60)
            } else {
                match (wp.0 & 0xffff) as i32 {
                    0 => s.scroll.get() - ui::scale(hwnd, 30),
                    1 => s.scroll.get() + ui::scale(hwnd, 30),
                    2 => s.scroll.get() - page,
                    3 => s.scroll.get() + page,
                    4 | 5 => position,
                    6 => 0,
                    7 => total,
                    _ => s.scroll.get(),
                }
            };
            s.scroll.set(next);
            layout(hwnd, s);
        }
        WM_SIZE => layout(hwnd, s),
        super::desktop_ui::COLOR_PREVIEW => {
            let field = HWND(lp.0 as *mut _);
            if s.fields
                .iter()
                .enumerate()
                .any(|(i, f)| f.1 == field && matches!(s.specs[i].kind, FieldKind::Color))
            {
                let c = (wp.0 as u32).to_le_bytes();
                ui::text(field, &format!("#{:02X}{:02X}{:02X}", c[2], c[1], c[0]));
            }
        }
        WM_COMMAND if wp.0 >> 16 == 0 && wp.0 >= 100 && wp.0 < 100 + s.specs.len() => {
            let index = wp.0 - 100;
            if matches!(s.specs[index].kind, FieldKind::Color) {
                let field = s.fields[index].1;
                let value = ui::value(field);
                let hex =
                    u32::from_str_radix(value.trim_start_matches('#'), 16).unwrap_or(0xffffff);
                if let Err(e) = super::desktop_ui::open_color(
                    hwnd,
                    field,
                    [hex as u8, (hex >> 8) as u8, (hex >> 16) as u8, 255],
                    true,
                ) {
                    feedback(hwnd, &e);
                }
            }
        }
        WM_COMMAND if wp.0 == 51 => {
            if let Some(token) = &s.running {
                token.store(true, Ordering::Release);
                feedback(hwnd, "已请求取消任务…");
            }
        }
        WM_COMMAND if wp.0 >> 16 == 0 && (1..=s.buttons.len()).contains(&wp.0) => {
            if s.running.is_some() {
                feedback(hwnd, "任务正在运行，可点击取消任务或关闭窗口后稍后返回。");
                return LRESULT(0);
            }
            for (_, field) in &s.fields {
                if let Err(e) = super::path_field::commit(*field, true) {
                    feedback(hwnd, &e);
                    return LRESULT(0);
                }
            }
            let values: Vec<String> = s.fields.iter().map(|(_, h)| ui::value(*h)).collect();
            for (index, (spec, value)) in s.specs.iter().zip(&values).enumerate() {
                if wp.0 == if s.id == FormId::Batch { 2 } else { 1 }
                    && let Err(error) = spec.validate(value)
                {
                    feedback(hwnd, &format!("{}：{error}", ui::value(s.fields[index].0)));
                    unsafe {
                        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(
                            s.fields[index].1,
                        ));
                    }
                    return LRESULT(0);
                }
            }
            let Some(mut apply) = s.apply.take() else {
                return LRESULT(0);
            };
            let result = apply(wp.0, values);
            if let Some(current) = unsafe { state(hwnd) }
                && current.apply.is_none()
            {
                current.apply = Some(apply);
            }
            feedback(
                hwnd,
                &result.unwrap_or_else(|e| format!("{e}\r\n请调整参数后重试。")),
            );
        }
        WM_CLOSE => super::workspace::hide_form(hwnd),
        WM_COMMAND if wp.0 == 50 => super::workspace::hide_form(hwnd),
        WM_NCDESTROY => unsafe {
            FORMS.with(|v| v.borrow_mut().retain(|(_, _, _, h)| *h != hwnd));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(std::ptr::from_mut(s)));
        },
        _ => return unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
    LRESULT(0)
}

pub(crate) fn set_running(hwnd: HWND, token: Option<Arc<AtomicBool>>) {
    if let Some(s) = unsafe { state(hwnd) } {
        s.running = token;
        ui::show(s.cancel, s.running.is_some());
        for button in &s.buttons {
            unsafe {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                    *button,
                    s.running.is_none(),
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "workspace_form_review.rs"]
mod review;

pub(crate) fn identity(hwnd: HWND) -> Option<FormId> {
    FORMS.with(|forms| {
        forms
            .borrow()
            .iter()
            .find(|(_, _, _, h)| *h == hwnd)
            .map(|(_, id, _, _)| *id)
    })
}

pub(crate) fn reveal(hwnd: HWND) -> bool {
    let tab = FORMS.with(|forms| {
        forms
            .borrow()
            .iter()
            .find(|(_, _, _, h)| *h == hwnd)
            .map(|(tab, _, _, _)| *tab)
    });
    if let Some(tab) = tab {
        if tab < 3 {
            super::workspace::switch(tab);
        }
        super::workspace::set_form(tab, hwnd);
        true
    } else {
        false
    }
}

pub(crate) fn field(hwnd: HWND, id: &str) -> Option<HWND> {
    let s = unsafe { state(hwnd) }?;
    s.specs
        .iter()
        .position(|spec| spec.id == id)
        .map(|index| s.fields[index].1)
}

pub(crate) fn preferred_height(hwnd: HWND) -> Option<i32> {
    let s = unsafe { state(hwnd) }?;
    Some(116 + s.fields.len() as i32 * 44 + ((s.buttons.len() as i32 + 2) / 3) * 36 + 100)
}

fn settings_layout(hwnd: HWND, s: &State) {
    use super::desktop_ui as desk;
    let _batch = ui::layout_batch(hwnd);
    let r = ui::rect(hwnd);
    let d = |n| ui::scale(hwnd, n);
    let body = s.body.unwrap();
    let footer = if ui::value(s.status).is_empty() {
        58
    } else {
        98
    };
    let height = (r.bottom - d(footer)).max(1);
    ui::place(body, 0, 0, r.right, height);
    ui::show(s.heading, false);
    ui::show(s.description, false);
    let order: Vec<usize> = if s.id == FormId::General {
        vec![1, 3, 2, 0]
    } else {
        (0..s.fields.len()).collect()
    };
    let cut = if s.id == FormId::General {
        2
    } else if s.id == FormId::Ocr {
        1
    } else {
        4
    };
    let content = d(108 + 44 * order.len() as i32 + if s.buttons.len() > 1 { 48 } else { 0 });
    let offset = s.scroll.get().clamp(0, (content - height).max(0));
    s.scroll.set(offset);
    desk::scroll_panel(hwnd, content, height, offset);
    let width = r.right - d(40);
    let field_x = d(142);
    let label_width = d(114);
    let place = |h, x, y, w, ht| ui::place(h, x, d(y) - offset, w, d(ht));
    for (index, title) in s.groups.iter().enumerate() {
        place(
            *title,
            d(20),
            if index == 0 { 16 } else { 68 + 44 * cut as i32 },
            width,
            24,
        );
    }
    for (row, index) in order.iter().enumerate() {
        let y = 52 + row as i32 * 44 + if row >= cut { 54 } else { 0 };
        let (label, field) = s.fields[*index];
        if s.specs[*index].id == "api-key" {
            ui::text(label, "API Key");
        }
        place(label, d(20), y, label_width, 28);
        place(field, field_x, y, r.right - field_x - d(20), 28);
    }
    // Provider tests and language helpers remain secondary; Save and Cancel stay fixed.
    for (i, button) in s.buttons.iter().enumerate().skip(1) {
        let show = if s.id == FormId::Vision {
            i == 1
        } else {
            !matches!(s.id, FormId::General)
        };
        ui::show(*button, show);
        if show {
            place(
                *button,
                d(20) + (i as i32 - 1) * d(144),
                102 + 44 * order.len() as i32,
                d(136),
                28,
            );
            desk::style(*button, None, true, false, false);
        }
    }
    ui::place(
        s.buttons[0],
        r.right - d(100),
        r.bottom - d(42),
        d(80),
        d(28),
    );
    ui::text(s.buttons[0], "保存");
    ui::place(s.close, r.right - d(188), r.bottom - d(42), d(80), d(28));
    ui::text(s.close, "取消");
    ui::place(s.cancel, d(20), r.bottom - d(42), d(110), d(28));
    ui::show(s.cancel, s.running.is_some());
    ui::place(s.status, d(20), height + d(6), r.right - d(40), d(30));
    ui::show(s.status, !ui::value(s.status).is_empty());
    desk::invalidate(hwnd);
}
unsafe extern "system" fn settings_body_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::*;
    if matches!(m, WM_COMMAND | WM_MOUSEWHEEL | WM_VSCROLL) {
        if let Ok(parent) = unsafe { GetParent(h) } {
            return unsafe { SendMessageW(parent, m, Some(w), Some(l)) };
        }
    }
    unsafe { DefWindowProcW(h, m, w, l) }
}
