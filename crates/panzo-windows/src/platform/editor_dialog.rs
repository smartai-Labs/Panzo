//! Small native property form. Text entry has its own focus/shortcut routing.
use crate::platform::editor_ui::{TextStyle, font_handle, scale_dip, window_dpi};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::COLOR_WINDOW;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    BS_DEFPUSHBUTTON, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    ES_AUTOHSCROLL, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IsDialogMessageW, LoadCursorW, MSG,
    RegisterClassW, SW_SHOW, SendMessageW, SetWindowLongPtrW, ShowWindow, TranslateMessage,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_SETFONT, WNDCLASSW, WS_BORDER,
    WS_CAPTION, WS_CHILD, WS_EX_DLGMODALFRAME, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[allow(clippy::too_many_lines)]
pub fn form_with_options(
    owner: HWND,
    title: &str,
    fields: &[(&str, String)],
    options: &[(usize, &[&str])],
    accept: &str,
) -> windows::core::Result<Option<Vec<String>>> {
    form_with_description(owner, title, "", fields, options, accept)
}

#[allow(clippy::too_many_lines)]
pub fn form_with_description(
    owner: HWND,
    title: &str,
    description: &str,
    fields: &[(&str, String)],
    options: &[(usize, &[&str])],
    accept: &str,
) -> windows::core::Result<Option<Vec<String>>> {
    use windows::Win32::UI::WindowsAndMessaging::{
        CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CBS_DROPDOWNLIST, CBS_HASSTRINGS, WS_VSCROLL,
    };
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = w!("PanzoProperties");
    unsafe {
        RegisterClassW(&WNDCLASSW {
            hInstance: instance.into(),
            lpszClassName: class,
            lpfnWndProc: Some(dialog_proc),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH((COLOR_WINDOW.0 + 1) as *mut _),
            ..Default::default()
        });
    }
    let scale = |n| scale_dip(n, window_dpi(owner));
    let mut owner_rect = RECT::default();
    unsafe {
        GetWindowRect(owner, &raw mut owner_rect)?;
    }
    let title = wide(title);
    let width = scale(456);
    let description_height = if description.is_empty() { 0 } else { 76 };
    let height = scale(124 + description_height + i32::try_from(fields.len()).unwrap_or(8) * 44);
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            class,
            PCWSTR(title.as_ptr()),
            WS_CAPTION | WS_SYSMENU,
            owner_rect.left + (owner_rect.right - owner_rect.left - width) / 2,
            owner_rect.top + (owner_rect.bottom - owner_rect.top - height) / 2,
            width,
            height,
            Some(owner),
            None,
            Some(instance.into()),
            None,
        )?
    };
    crate::platform::editor_ui::apply_window_chrome(hwnd);
    let controls = (|| -> windows::core::Result<Vec<HWND>> {
        if !description.is_empty() {
            let description = wide(description);
            let label = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("STATIC"),
                    PCWSTR(description.as_ptr()),
                    WS_CHILD | WS_VISIBLE,
                    scale(20),
                    scale(16),
                    scale(402),
                    scale(68),
                    Some(hwnd),
                    None,
                    Some(instance.into()),
                    None,
                )?
            };
            unsafe {
                SendMessageW(
                    label,
                    WM_SETFONT,
                    Some(WPARAM(
                        font_handle(TextStyle::Body, window_dpi(owner)).0 as usize,
                    )),
                    Some(LPARAM(1)),
                );
            }
        }
        let mut inputs = Vec::new();
        for (index, (label, value)) in fields.iter().enumerate() {
            let y = 20 + description_height + i32::try_from(index).unwrap_or(0) * 44;
            let choice = options
                .iter()
                .find(|(field, _)| *field == index)
                .map(|(_, values)| *values);
            for (class, text, x, w, id, style) in [
                (w!("STATIC"), label.to_string(), 20, 152, 0, WINDOW_STYLE(0)),
                (
                    if choice.is_some() {
                        w!("COMBOBOX")
                    } else {
                        w!("EDIT")
                    },
                    value.clone(),
                    180,
                    242,
                    100 + index,
                    if choice.is_some() {
                        WS_TABSTOP
                            | WS_VSCROLL
                            | WINDOW_STYLE((CBS_DROPDOWNLIST | CBS_HASSTRINGS) as u32)
                    } else {
                        WS_BORDER | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32)
                    },
                ),
            ] {
                let text = wide(&text);
                let child = unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        class,
                        PCWSTR(text.as_ptr()),
                        WS_CHILD | WS_VISIBLE | style,
                        scale(x),
                        scale(y),
                        scale(w),
                        scale(if id >= 100 && choice.is_some() {
                            180
                        } else {
                            30
                        }),
                        Some(hwnd),
                        Some(HMENU(id as *mut _)),
                        Some(instance.into()),
                        None,
                    )?
                };
                unsafe {
                    SendMessageW(
                        child,
                        WM_SETFONT,
                        Some(WPARAM(
                            font_handle(TextStyle::Body, window_dpi(owner)).0 as usize,
                        )),
                        Some(LPARAM(1)),
                    );
                }
                if id >= 100 {
                    if let Some(choices) = choice {
                        for text in choices {
                            let text = wide(text);
                            unsafe {
                                SendMessageW(
                                    child,
                                    CB_ADDSTRING,
                                    None,
                                    Some(LPARAM(text.as_ptr() as isize)),
                                );
                            }
                        }
                        let selected = choices.iter().position(|text| *text == value).unwrap_or(0);
                        unsafe {
                            SendMessageW(child, CB_SETCURSEL, Some(WPARAM(selected)), None);
                        }
                    }
                    inputs.push(child);
                }
            }
        }
        for (id, label, x) in [(1, accept, 254), (2, "取消", 340)] {
            let text = wide(label);
            let child = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("BUTTON"),
                    PCWSTR(text.as_ptr()),
                    WS_CHILD
                        | WS_VISIBLE
                        | WS_TABSTOP
                        | WINDOW_STYLE(if id == 1 { BS_DEFPUSHBUTTON as u32 } else { 0 }),
                    scale(x),
                    height - scale(80),
                    scale(78),
                    scale(32),
                    Some(hwnd),
                    Some(HMENU(id as *mut _)),
                    Some(instance.into()),
                    None,
                )?
            };
            unsafe {
                SendMessageW(
                    child,
                    WM_SETFONT,
                    Some(WPARAM(
                        font_handle(TextStyle::Button, window_dpi(owner)).0 as usize,
                    )),
                    Some(LPARAM(1)),
                );
            }
        }
        Ok(inputs)
    })();
    let inputs = match controls {
        Ok(inputs) => inputs,
        Err(error) => {
            let _ = unsafe { DestroyWindow(hwnd) };
            return Err(error);
        }
    };
    unsafe {
        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetFocus(inputs.first().copied());
    }
    // Only dispatch this dialog and its children, avoiding reentrant mutable editor handlers.
    let mut message = MSG::default();
    while unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 0 {
        let result = unsafe { GetMessageW(&raw mut message, Some(hwnd), 0, 0) };
        if result.0 <= 0 {
            break;
        }
        if !unsafe { IsDialogMessageW(hwnd, &raw const message) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
    }
    let accepted = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 1;
    let values = accepted.then(|| {
        inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                if let Some((_, choices)) = options.iter().find(|(field, _)| *field == index) {
                    let selected = unsafe { SendMessageW(*input, CB_GETCURSEL, None, None) }.0;
                    return choices
                        .get(usize::try_from(selected).unwrap_or(0))
                        .unwrap_or(&"")
                        .to_string();
                }
                let length = usize::try_from(unsafe { GetWindowTextLengthW(*input) }).unwrap_or(0);
                let mut buffer = vec![0_u16; length + 1];
                let read =
                    usize::try_from(unsafe { GetWindowTextW(*input, &mut buffer) }).unwrap_or(0);
                String::from_utf16_lossy(&buffer[..read])
            })
            .collect()
    });
    unsafe {
        let _ = DestroyWindow(hwnd);
        let _ = EnableWindow(owner, true);
        let _ = SetFocus(Some(owner));
    }
    Ok(values)
}

unsafe extern "system" fn dialog_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if let Some(result) = crate::platform::editor_ui::native_control_color(message, wparam) {
        return result;
    }
    match message {
        WM_COMMAND if matches!(wparam.0 & 0xffff, 1 | 2) => {
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (wparam.0 & 0xffff).cast_signed());
            }
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_ERASEBKGND => {
            let mut rect = RECT::default();
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &raw mut rect);
            }
            let color = crate::platform::editor_ui::EditorTheme::current(96)
                .palette
                .window;
            crate::platform::editor_ui::rounded_surface(
                windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut _),
                rect,
                color,
                color,
                0,
            );
            LRESULT(1)
        }
        WM_CLOSE => {
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 2);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
