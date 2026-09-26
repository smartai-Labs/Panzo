//! Transient native EDIT: IME, selection and text shortcuts stay inside the field.
//! Completion is posted, never called synchronously into the borrowed editor model.
use super::editor_ui::{TextStyle, font_handle, window_dpi};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, ES_AUTOHSCROLL, GetWindowTextW, PostMessageW, SendMessageW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CHAR, WM_KEYDOWN, WM_KILLFOCUS, WM_NCDESTROY,
    WM_SETFONT, WS_BORDER, WS_CHILD, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

pub const WM_INLINE_DONE: u32 = WM_APP + 0x4b0;
struct FieldState {
    owner: HWND,
    composing: std::cell::Cell<bool>,
}
pub struct InlineInput {
    pub hwnd: HWND,
    initial_value: String,
    _state: Box<FieldState>,
}
impl InlineInput {
    pub fn open(owner: HWND, rect: RECT, value: &str) -> windows::core::Result<Self> {
        let text: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                PCWSTR(text.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                Some(owner),
                None,
                None,
                None,
            )?
        };
        let state = Box::new(FieldState {
            owner,
            composing: std::cell::Cell::new(false),
        });
        let data = std::ptr::from_ref(&*state) as usize;
        let input = Self {
            hwnd,
            initial_value: value.to_owned(),
            _state: state,
        };
        if !unsafe { SetWindowSubclass(hwnd, Some(field_proc), 1, data) }.as_bool() {
            return Err(windows::core::Error::from_thread());
        }
        unsafe {
            SendMessageW(
                hwnd,
                WM_SETFONT,
                Some(WPARAM(
                    font_handle(TextStyle::Body, window_dpi(owner)).0 as usize,
                )),
                Some(LPARAM(1)),
            );
            SendMessageW(hwnd, 0x00C5, Some(WPARAM(64)), None); // EM_LIMITTEXT
            SendMessageW(hwnd, 0x00B1, Some(WPARAM(0)), Some(LPARAM(-1))); // EM_SETSEL
            let _ = SetFocus(Some(hwnd));
        }
        Ok(input)
    }
    pub fn value(&self) -> String {
        let mut text = [0u16; 65];
        let length = usize::try_from(unsafe { GetWindowTextW(self.hwnd, &mut text) }).unwrap_or(0);
        String::from_utf16_lossy(&text[..length])
    }
    pub fn is_changed(&self) -> bool {
        self.value() != self.initial_value
    }
}
impl Drop for InlineInput {
    fn drop(&mut self) {
        unsafe {
            let _ = RemoveWindowSubclass(self.hwnd, Some(field_proc), 1);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
unsafe extern "system" fn field_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    let field = unsafe { &*(data as *const FieldState) };
    if msg == 0x010D {
        field.composing.set(true);
    } // WM_IME_STARTCOMPOSITION
    if msg == 0x010E {
        field.composing.set(false);
    } // WM_IME_ENDCOMPOSITION
    if msg == WM_KEYDOWN && !field.composing.get() && matches!(wp.0, 13 | 27 | 9) {
        let completion = if wp.0 == 27 {
            0
        } else if wp.0 == 9 {
            if unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) } < 0 {
                4
            } else {
                3
            }
        } else {
            2
        };
        unsafe {
            let _ = PostMessageW(
                Some(field.owner),
                WM_INLINE_DONE,
                WPARAM(hwnd.0 as usize),
                LPARAM(completion),
            );
        }
        return LRESULT(0);
    }
    if msg == WM_CHAR && !field.composing.get() && matches!(wp.0, 13 | 27 | 9) {
        return LRESULT(0);
    }
    if msg == WM_KILLFOCUS {
        unsafe {
            let _ = PostMessageW(
                Some(field.owner),
                WM_INLINE_DONE,
                WPARAM(hwnd.0 as usize),
                LPARAM(1),
            );
        }
    }
    if msg == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(field_proc), id);
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
}
