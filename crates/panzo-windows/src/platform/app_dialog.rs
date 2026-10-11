//! Theme-aware application messages. Native file/directory choosers remain system dialogs.
use super::{desktop_ui as desk, editor_ui, workspace_ui as ui};
use std::cell::Cell;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT},
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, SetFocus},
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, w},
};
thread_local! { static ACTIVE: Cell<Option<HWND>> = const { Cell::new(None) }; }
pub(crate) fn is_open() -> bool {
    ACTIVE.with(|v| v.get().is_some())
}
struct State {
    answer: Option<MESSAGEBOX_RESULT>,
    cancel: MESSAGEBOX_RESULT,
    default: HWND,
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Some(s) = (unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State).as_mut() }) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    match m {
        WM_COMMAND if matches!(w.0 & 65535, 1 | 2 | 6 | 7) => {
            s.answer = Some(MESSAGEBOX_RESULT((w.0 & 65535) as i32))
        }
        WM_CLOSE => s.answer = Some(s.cancel),
        WM_KEYDOWN if w.0 == 27 => s.answer = Some(s.cancel),
        WM_KEYDOWN if w.0 == 13 => unsafe {
            SendMessageW(s.default, BM_CLICK, None, None);
        },
        WM_NCHITTEST => {
            let mut p = windows::Win32::Foundation::POINT {
                x: l.0 as u16 as i16 as i32,
                y: (l.0 >> 16) as u16 as i16 as i32,
            };
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(h, &mut p);
            }
            if p.y < ui::scale(h, 44) {
                return LRESULT(HTCAPTION as isize);
            }
            return LRESULT(HTCLIENT as isize);
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &mut ps) };
            let r = ui::rect(h);
            let frame = desk::BufferedFrame::new(dc, r);
            let t = editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
            editor_ui::fill_rect(frame.dc, &r, t.palette.panel);
            for edge in [
                desk::rect(0, 0, r.right, 1),
                desk::rect(0, r.bottom - 1, r.right, 1),
                desk::rect(0, 0, 1, r.bottom),
                desk::rect(r.right - 1, 0, 1, r.bottom),
            ] {
                editor_ui::fill_rect(frame.dc, &edge, t.palette.divider);
            }
            desk::line(
                frame.dc,
                desk::rect(
                    ui::scale(h, 24),
                    r.bottom - ui::scale(h, 66),
                    r.right - ui::scale(h, 48),
                    1,
                ),
                t,
            );
            drop(frame);
            unsafe {
                let _ = EndPaint(h, &ps);
            }
        }
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}
/// Signature intentionally matches the platform message call to preserve existing decision semantics.
pub(crate) unsafe fn message(
    owner: Option<HWND>,
    text: PCWSTR,
    title: PCWSTR,
    flags: MESSAGEBOX_STYLE,
) -> MESSAGEBOX_RESULT {
    let kind = flags.0 & 15;
    let cancel = if kind == MB_YESNO.0 {
        IDNO
    } else if kind == MB_OK.0 {
        IDOK
    } else {
        IDCANCEL
    };
    // Reentrant close/timer requests never create a second modal stack.
    if is_open() {
        if kind == MB_OK.0
            && let Ok(message) = unsafe { text.to_string() }
        {
            super::workspace::notice(&message);
        }
        return cancel;
    }
    let root = owner
        .map(|h| unsafe { GetAncestor(h, GA_ROOT) })
        .filter(|h| !h.is_invalid());
    let title = unsafe { title.to_string() }.unwrap_or_default();
    let body = unsafe { text.to_string() }
        .unwrap_or_default()
        .replace("\r\n", "\n")
        .replace('\n', "\r\n");
    if ui::register(w!("PanzoMessageDialog"), Some(procedure)).is_err() {
        return cancel;
    }
    let scale_owner = root.unwrap_or_default();
    let d = |n| ui::scale(scale_owner, n);
    let width = d(520);
    let measured_lines = body
        .lines()
        .map(|line| line.chars().count().div_ceil(32).max(1))
        .sum::<usize>();
    let lines = measured_lines.clamp(3, 12) as i32;
    let height = d(144 + lines * 21);
    let (width, height) = editor_ui::available_window_size(width, height);
    let mut bounds = windows::Win32::Foundation::RECT::default();
    if let Some(root) = root {
        unsafe {
            let _ = GetWindowRect(root, &mut bounds);
        }
    }
    let h = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            w!("PanzoMessageDialog"),
            PCWSTR(ui::wide(&title).as_ptr()),
            WS_POPUP | WS_CLIPCHILDREN,
            (bounds.left + bounds.right - width) / 2,
            (bounds.top + bounds.bottom - height) / 2,
            width,
            height,
            root,
            None,
            None,
            None,
        )
    };
    let Ok(h) = h else {
        return cancel;
    };
    desk::install(h);
    editor_ui::apply_window_chrome(h);
    let result = (|| -> Result<(Box<State>, Vec<HWND>), String> {
        let heading = ui::label(h, &title, 10)?;
        ui::place(heading, d(24), d(16), width - d(48), d(28));
        unsafe {
            SendMessageW(
                heading,
                WM_SETFONT,
                Some(WPARAM(
                    editor_ui::font_handle(editor_ui::TextStyle::Heading, editor_ui::window_dpi(h))
                        .0 as usize,
                )),
                Some(LPARAM(1)),
            );
        }
        let content = if measured_lines <= 12 && height >= d(144 + lines * 21) {
            ui::label(h, &body, 11)?
        } else {
            let content = ui::edit(h, &body, 11, true)?;
            unsafe {
                SendMessageW(
                    content,
                    windows::Win32::UI::Controls::EM_SETREADONLY,
                    Some(WPARAM(1)),
                    None,
                );
            }
            content
        };
        ui::place(content, d(24), d(58), width - d(48), height - d(140));
        let choices: Vec<(MESSAGEBOX_RESULT, &str)> = match kind {
            1 => vec![(IDOK, "确定"), (IDCANCEL, "取消")],
            3 => vec![(IDYES, "保存"), (IDNO, "不保存"), (IDCANCEL, "取消")],
            4 => vec![(IDYES, "继续"), (IDNO, "取消")],
            _ => vec![(IDOK, "确定")],
        };
        let mut controls = Vec::new();
        for (index, (answer, label)) in choices.iter().enumerate() {
            let b = ui::button(h, label, answer.0 as usize)?;
            ui::place(
                b,
                width - d(24 + (choices.len() - index) as i32 * 100),
                height - d(50),
                d(92),
                d(32),
            );
            desk::style(b, None, false, index == 0, false);
            controls.push(b);
        }
        let requested = ((flags.0 >> 8) & 3) as usize;
        let default = controls[if kind == 3 && requested == 0 {
            2
        } else {
            requested.min(controls.len() - 1)
        }];
        Ok((
            Box::new(State {
                answer: None,
                cancel,
                default,
            }),
            controls,
        ))
    })();
    let Ok((mut state, _buttons)) = result else {
        unsafe {
            let _ = DestroyWindow(h);
        }
        return cancel;
    };
    let was_enabled = root.is_some_and(|r| {
        unsafe { windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(r) }.as_bool()
    });
    ACTIVE.with(|v| v.set(Some(h)));
    unsafe {
        SetWindowLongPtrW(h, GWLP_USERDATA, (&mut *state as *mut State) as isize);
        if let Some(root) = root {
            let _ = EnableWindow(root, false);
        }
        let _ = ShowWindow(h, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(h);
        let _ = SetFocus(Some(state.default));
    }
    let mut msg = MSG::default();
    while state.answer.is_none() && unsafe { IsWindow(Some(h)) }.as_bool() {
        let result = unsafe { GetMessageW(&mut msg, None, 0, 0) }.0;
        if result <= 0 {
            if result == 0 {
                unsafe {
                    PostQuitMessage(msg.wParam.0 as i32);
                }
            }
            break;
        }
        if msg.message == WM_KEYDOWN
            && msg.wParam.0 == 27
            && (msg.hwnd == h || unsafe { IsChild(h, msg.hwnd) }.as_bool())
        {
            state.answer = Some(cancel);
            continue;
        }
        if !unsafe { IsDialogMessageW(h, &msg) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    let answer = state.answer.unwrap_or(cancel);
    unsafe {
        SetWindowLongPtrW(h, GWLP_USERDATA, 0);
        let _ = DestroyWindow(h);
        if let Some(root) = root.filter(|r| IsWindow(Some(*r)).as_bool()) {
            if was_enabled {
                let _ = EnableWindow(root, true);
            }
            let _ = SetForegroundWindow(root);
        }
    }
    ACTIVE.with(|v| v.set(None));
    answer
}
