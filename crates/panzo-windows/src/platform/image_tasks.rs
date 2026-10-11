//! Modeless progress/result window. Workers own input snapshots and never hold UI pointers.
use super::capture_tools::Raster;
use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, ES_AUTOVSCROLL,
            ES_MULTILINE, ES_WANTRETURN, GWLP_USERDATA, GetClientRect, GetWindowLongPtrW,
            GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IsChild, IsDialogMessageW,
            KillTimer, LoadCursorW, MSG, PostMessageW, RegisterClassW, SW_SHOW, SWP_NOZORDER,
            SendMessageW, SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
            WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_DPICHANGED, WM_KEYDOWN,
            WM_NCDESTROY, WM_SETFONT, WM_SIZE, WM_TIMER, WNDCLASSW, WS_BORDER, WS_CHILD,
            WS_CLIPCHILDREN, WS_EX_CONTROLPARENT, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
            WS_VSCROLL,
        },
    },
    core::{PCWSTR, w},
};
thread_local! {static WINDOWS:RefCell<Vec<HWND>>=const{RefCell::new(Vec::new())};}
pub(crate) enum Output {
    Text(String),
    Image(Raster),
}
pub(crate) enum Update {
    Progress(String),
    Done(Result<Output, String>),
}
struct State {
    edit: HWND,
    status: HWND,
    cancel_button: HWND,
    copy: HWND,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<Update>,
    library: PathBuf,
    running: bool,
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}
fn text(hwnd: HWND, s: &str) {
    let text = wide(s);
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(text.as_ptr()));
    }
}
#[allow(clippy::too_many_lines)] // Keep native window creation and partial-failure cleanup together.
pub(crate) fn start(
    title: &str,
    library: &std::path::Path,
    work: impl FnOnce(Arc<AtomicBool>, Sender<Update>) -> Result<Output, String> + Send + 'static,
) -> Result<(), String> {
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    unsafe {
        RegisterClassW(&WNDCLASSW {
            hInstance: module.into(),
            lpfnWndProc: Some(procedure),
            lpszClassName: w!("PanzoImageTask"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(
                (windows::Win32::Graphics::Gdi::COLOR_WINDOW.0 + 1) as *mut _,
            ),
            ..Default::default()
        });
    }
    let title = wide(title);
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            w!("PanzoImageTask"),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            760,
            540,
            None,
            None,
            Some(module.into()),
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    let result = (|| -> Result<State, String> {
        let child = |class: PCWSTR, label: &str, id, style| {
            let label = wide(label);
            let c = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    class,
                    PCWSTR(label.as_ptr()),
                    WS_CHILD | WS_VISIBLE | style,
                    12,
                    12,
                    200,
                    32,
                    Some(hwnd),
                    Some(HMENU(id as *mut _)),
                    Some(module.into()),
                    None,
                )
            }
            .map_err(|e| e.to_string())?;
            unsafe {
                SendMessageW(
                    c,
                    WM_SETFONT,
                    Some(WPARAM(
                        super::editor_ui::font_handle(
                            super::editor_ui::TextStyle::Body,
                            super::editor_ui::window_dpi(hwnd),
                        )
                        .0 as usize,
                    )),
                    Some(LPARAM(1)),
                );
            }
            Ok::<_, String>(c)
        };
        Ok(State {
            edit: child(
                w!("EDIT"),
                "",
                1,
                WS_TABSTOP
                    | WS_BORDER
                    | WS_VSCROLL
                    | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32),
            )?,
            status: child(w!("STATIC"), "正在处理…", 2, WINDOW_STYLE(0))?,
            cancel_button: child(w!("BUTTON"), "取消任务", 3, WS_TABSTOP)?,
            copy: child(w!("BUTTON"), "复制文字", 4, WS_TABSTOP)?,
            cancel: cancel.clone(),
            receiver,
            library: library.into(),
            running: true,
        })
    })();
    let state = match result {
        Ok(s) => s,
        Err(e) => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return Err(e);
        }
    };
    let ptr = Box::into_raw(Box::new(state));
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
        SendMessageW(
            (*ptr).edit,
            windows::Win32::UI::Controls::EM_SETLIMITTEXT,
            Some(WPARAM(1_000_000)),
            None,
        );
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow((*ptr).copy, false);
        SetTimer(Some(hwnd), 1, 100, None);
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    WINDOWS.with(|windows| windows.borrow_mut().push(hwnd));
    layout(hwnd, unsafe { &*ptr });
    thread::spawn(move || {
        let result = work(cancel, sender.clone());
        let _ = sender.send(Update::Done(result));
    });
    Ok(())
}

fn layout(hwnd: HWND, state: &State) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &raw mut r);
    }
    let scale = |v| super::editor_ui::scale_dip(v, super::editor_ui::window_dpi(hwnd));
    for (child, x, y, w, h) in [
        (
            state.status,
            scale(12),
            scale(12),
            r.right - scale(24),
            scale(40),
        ),
        (
            state.edit,
            scale(12),
            scale(58),
            r.right - scale(24),
            (r.bottom - scale(118)).max(scale(80)),
        ),
        (
            state.copy,
            scale(12),
            r.bottom - scale(48),
            scale(140),
            scale(30),
        ),
        (
            state.cancel_button,
            r.right - scale(152),
            r.bottom - scale(48),
            scale(140),
            scale(30),
        ),
    ] {
        unsafe {
            let _ = SetWindowPos(child, None, x, y, w, h, SWP_NOZORDER);
        }
    }
}
pub(crate) fn close_all() {
    for hwnd in WINDOWS.with(|v| v.borrow().clone()) {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}
#[allow(clippy::too_many_lines)] // Native message dispatch shares the worker/result window lifetime.
unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut State;
    if let Some(state) = unsafe { ptr.as_mut() } {
        match message {
            WM_SIZE | WM_DPICHANGED => {
                layout(hwnd, state);
                return LRESULT(0);
            }
            WM_TIMER => {
                while let Ok(update) = state.receiver.try_recv() {
                    match update {
                        Update::Progress(v) => text(state.status, &v),
                        Update::Done(result) => {
                            state.running = false;
                            text(state.cancel_button, "关闭");
                            unsafe {
                                let _ = KillTimer(Some(hwnd), 1);
                            }
                            match result {
                                Ok(Output::Text(value)) => {
                                    text(state.edit, &value);
                                    text(state.status, "处理完成。文字可修改后复制。");
                                    unsafe {
                                        let _=windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(state.copy,true);
                                    }
                                }
                                Ok(Output::Image(image)) => {
                                    match super::capture_tools::show_image(
                                        image,
                                        &state.library,
                                        false,
                                    ) {
                                        Ok(_) => {
                                            unsafe {
                                                let _ = DestroyWindow(hwnd);
                                            }
                                            return LRESULT(0);
                                        }
                                        Err(e) => {
                                            text(state.edit, &e);
                                            text(state.status, "结果未打开");
                                        }
                                    }
                                }
                                Err(e) => {
                                    text(state.edit, &e);
                                    text(state.status, "任务结束");
                                }
                            }
                        }
                    }
                }
                return LRESULT(0);
            }
            WM_COMMAND => {
                match wp.0 & 0xffff {
                    3 => {
                        if state.running {
                            state.cancel.store(true, Ordering::Release);
                            text(state.status, "正在取消任务…");
                        } else {
                            unsafe {
                                let _ = DestroyWindow(hwnd);
                            }
                        }
                    }
                    4 => {
                        let len = unsafe { GetWindowTextLengthW(state.edit) }
                            .max(0)
                            .cast_unsigned() as usize;
                        let mut value = vec![0u16; len + 1];
                        let n = unsafe { GetWindowTextW(state.edit, &mut value) }
                            .max(0)
                            .cast_unsigned() as usize;
                        match super::capture_tools::copy_text(
                            hwnd,
                            &String::from_utf16_lossy(&value[..n]),
                        ) {
                            Ok(()) => text(state.status, "文字已复制"),
                            Err(e) => text(state.status, &e),
                        }
                    }
                    _ => {}
                }
                return LRESULT(0);
            }
            WM_CLOSE => {
                state.cancel.store(true, Ordering::Release);
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                return LRESULT(0);
            }
            WM_NCDESTROY => {
                state.cancel.store(true, Ordering::Release);
                WINDOWS.with(|v| v.borrow_mut().retain(|h| *h != hwnd));
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    drop(Box::from_raw(ptr));
                }
            }
            _ => {}
        }
    }
    unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}

pub(crate) fn dialog_message(message: &MSG) -> bool {
    if message.message != WM_KEYDOWN {
        return false;
    }
    for hwnd in WINDOWS.with(|v| v.borrow().clone()) {
        if message.hwnd == hwnd || unsafe { IsChild(hwnd, message.hwnd) }.as_bool() {
            if message.wParam.0 == 27 {
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                return true;
            }
            if message.wParam.0 == 9 {
                return unsafe { IsDialogMessageW(hwnd, message) }.as_bool();
            }
        }
    }
    false
}

#[cfg(test)]
pub(crate) fn review(library: &std::path::Path, root: &std::path::Path) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetDlgItem, PM_REMOVE, PeekMessageW, TranslateMessage,
    };
    let source = super::capture_tools::review_ocr_image();
    let language = super::ocr::languages().unwrap()[0].tag.clone();
    start("Panzo · 识别文字", library, move |_, _| {
        super::ocr::recognize(&source, Some(&language)).map(Output::Text)
    })
    .unwrap();
    let hwnd = WINDOWS.with(|v| v.borrow()[0]);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < until {
        let mut msg = MSG::default();
        unsafe {
            while PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }
        }
        if !unsafe { &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State) }.running {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let state = unsafe { &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State) };
    assert!(!state.running);
    assert!(
        unsafe { windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(state.copy) }
            .as_bool()
    );
    let edit = unsafe { GetDlgItem(Some(hwnd), 1) }.unwrap();
    let mut text = [0u16; 4096];
    let count = unsafe { GetWindowTextW(edit, &mut text) }
        .max(0)
        .cast_unsigned() as usize;
    assert!(String::from_utf16_lossy(&text[..count]).contains("Panzo"));
    super::capture_tools::review_window_png(hwnd, &root.join("ocr-result-window.png"));
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}
