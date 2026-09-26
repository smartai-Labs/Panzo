//! Theme only our thread's popup frame; Windows still owns menu input and sizing.
use super::{ACTIVE, EditorTheme};
use std::cell::RefCell;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, CreateSolidBrush, DeleteObject, ExcludeClipRect, GetWindowDC, HBRUSH, HDC,
    HGDIOBJ, ReleaseDC, RestoreDC, SaveDC,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetClassNameW, GetClientRect, GetWindowRect, HCBT_CREATEWND, HHOOK, HMENU,
    MENUINFO, MIM_BACKGROUND, PRF_NONCLIENT, SetMenuInfo, SetWindowsHookExW, UnhookWindowsHookEx,
    WH_CBT, WM_NCDESTROY, WM_NCPAINT, WM_PRINT,
};

thread_local! { static WINDOWS: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) }; }
const SUBCLASS_ID: usize = 0x504d;

pub(super) struct StyleGuard {
    hook: HHOOK,
    brush: HBRUSH,
}

impl StyleGuard {
    pub(super) fn install(menu: HMENU, theme: EditorTheme) -> windows::core::Result<Self> {
        let hook = unsafe { SetWindowsHookExW(WH_CBT, Some(created), None, GetCurrentThreadId())? };
        let brush = unsafe { CreateSolidBrush(theme.palette.panel) };
        let guard = Self { hook, brush };
        let info = MENUINFO {
            cbSize: std::mem::size_of::<MENUINFO>() as u32,
            fMask: MIM_BACKGROUND,
            hbrBack: brush,
            ..MENUINFO::default()
        };
        unsafe {
            SetMenuInfo(menu, &raw const info)?;
        }
        Ok(guard)
    }
}

impl Drop for StyleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.hook);
        }
        let windows = WINDOWS.with(|windows| std::mem::take(&mut *windows.borrow_mut()));
        for hwnd in windows {
            unsafe {
                let _ = RemoveWindowSubclass(hwnd, Some(frame_proc), SUBCLASS_ID);
            }
        }
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.brush.0));
        }
    }
}

unsafe extern "system" fn created(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HCBT_CREATEWND.cast_signed() {
        let hwnd = HWND(wp.0 as *mut std::ffi::c_void);
        let mut name = [0_u16; 32];
        let length = unsafe { GetClassNameW(hwnd, &mut name) };
        if length == 6
            && name[..6] == [35, 51, 50, 55, 54, 56]
            && let Some(color) =
                ACTIVE.with(|view| view.borrow().as_ref().map(|view| view.theme.palette.panel))
            && unsafe { SetWindowSubclass(hwnd, Some(frame_proc), SUBCLASS_ID, color.0 as usize) }
                .as_bool()
        {
            WINDOWS.with(|windows| windows.borrow_mut().push(hwnd));
        }
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

unsafe extern "system" fn frame_proc(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    let color = COLORREF(data as u32);
    if message == WM_NCPAINT {
        let dc = unsafe { GetWindowDC(Some(hwnd)) };
        paint_frame(hwnd, dc, color);
        unsafe {
            ReleaseDC(Some(hwnd), dc);
        }
        return LRESULT(0);
    }
    if message == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(frame_proc), id);
        }
        WINDOWS.with(|windows| windows.borrow_mut().retain(|window| *window != hwnd));
    }
    let result = unsafe { DefSubclassProc(hwnd, message, wp, lp) };
    if message == WM_PRINT && lp.0 & PRF_NONCLIENT as isize != 0 {
        paint_frame(hwnd, HDC(wp.0 as *mut std::ffi::c_void), color);
    }
    result
}

fn paint_frame(hwnd: HWND, dc: HDC, color: COLORREF) {
    if dc.is_invalid() {
        return;
    }
    let mut window = RECT::default();
    let mut client = RECT::default();
    let mut origin = POINT::default();
    unsafe {
        if GetWindowRect(hwnd, &raw mut window).is_err()
            || GetClientRect(hwnd, &raw mut client).is_err()
            || !ClientToScreen(hwnd, &raw mut origin).as_bool()
        {
            return;
        }
        let saved = SaveDC(dc);
        if saved == 0 {
            return;
        }
        let x = origin.x - window.left;
        let y = origin.y - window.top;
        ExcludeClipRect(dc, x, y, x + client.right, y + client.bottom);
        super::editor_ui::fill_rect(
            dc,
            &RECT {
                left: 0,
                top: 0,
                right: window.right - window.left,
                bottom: window.bottom - window.top,
            },
            color,
        );
        let _ = RestoreDC(dc, saved);
    }
}
