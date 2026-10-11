//! Tray lifetime and global shortcuts. No capture or encoding is initialized here.
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, PostMessageW, SetForegroundWindow, WM_APP, WM_LBUTTONDBLCLK, WM_NULL,
    WM_RBUTTONUP,
};
// Native confirmation dialogs dispatch messages recursively. Coalesce posted exits and
// serialize application/image close confirmations across those nested message loops.
thread_local! {
    static EXIT_QUEUED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static CLOSE_CONFIRMING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
pub(crate) fn close_confirmation_open() -> bool {
    CLOSE_CONFIRMING.with(std::cell::Cell::get)
}
pub(crate) fn queue_exit() -> bool {
    !close_confirmation_open() && EXIT_QUEUED.with(|queued| !queued.replace(true))
}
pub(crate) fn clear_queued_exit() {
    EXIT_QUEUED.with(|queued| queued.set(false));
}
pub(crate) struct CloseConfirmation;
pub(crate) fn begin_close_confirmation() -> Option<CloseConfirmation> {
    clear_queued_exit();
    (!CLOSE_CONFIRMING.with(|active| active.replace(true))).then(|| CloseConfirmation)
}
impl Drop for CloseConfirmation {
    fn drop(&mut self) {
        CLOSE_CONFIRMING.with(|active| active.set(false));
        clear_queued_exit();
    }
}
pub(crate) const WM_TRAY: u32 = WM_APP + 73;
pub(crate) struct ShellIntegration {
    hwnd: HWND,
    tray: bool,
}
impl ShellIntegration {
    pub(crate) fn install(hwnd: HWND) -> (Self, Vec<String>) {
        let mut this = Self { hwnd, tray: false };
        let mut errors = super::shortcuts::install(hwnd);
        let mut icon = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            ..Default::default()
        };
        icon.hIcon = super::app_icon::small(super::editor_ui::system_dpi());
        let tip: Vec<u16> = "Panzo · 截图 / 贴图 / 录屏".encode_utf16().collect();
        icon.szTip[..tip.len()].copy_from_slice(&tip);
        this.tray = unsafe { Shell_NotifyIconW(NIM_ADD, &raw const icon) }.as_bool();
        if !this.tray {
            errors.push("系统托盘不可用，可保留主窗口操作".into());
        }
        (this, errors)
    }
    pub(crate) fn pin_recovery_available(&self) -> bool {
        self.tray || super::shortcuts::globally_active("recover_pins")
    }
    pub(crate) fn available(&self) -> bool {
        self.tray
    }
}
impl Drop for ShellIntegration {
    fn drop(&mut self) {
        unsafe {
            super::shortcuts::uninstall();
            if self.tray {
                let data = NOTIFYICONDATAW {
                    cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                    hWnd: self.hwnd,
                    uID: 1,
                    ..Default::default()
                };
                let _ = Shell_NotifyIconW(NIM_DELETE, &raw const data);
            }
        }
    }
}
pub(crate) fn tray_command(hwnd: HWND, message: u32) -> Option<u32> {
    if message == WM_LBUTTONDBLCLK {
        return Some(10);
    }
    if message != WM_RBUTTONUP {
        return None;
    }
    use super::{
        app_menu,
        editor_ui::{self, EditorTheme},
    };
    let rows = tray_rows(super::ui_preferences::close_to_tray());
    let mut point = windows::Win32::Foundation::POINT::default();
    unsafe {
        let _ = GetCursorPos(&raw mut point);
        let _ = SetForegroundWindow(hwnd);
        let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &raw mut point);
    }
    let anchor = windows::Win32::Foundation::RECT {
        left: point.x,
        right: point.x,
        top: point.y,
        bottom: point.y,
    };
    let choice = app_menu::show_rows(
        hwnd,
        anchor,
        EditorTheme::current(editor_ui::window_dpi(hwnd)),
        rows,
        false,
    )
    .ok()
    .flatten();
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    }
    choice.map(|id| id as u32)
}

/// One grouped model for the tray popup; labels never contain shortcut text.
pub(super) fn tray_rows(close_to_tray: bool) -> Vec<super::app_menu::Row> {
    use super::{app_menu::Row, editor_ui::IconKind as I};
    let command = |id, label, icon| {
        Row::command(id, label, icon, true)
            .with_shortcut(super::shortcuts::shell_shortcut(id as u32))
    };
    vec![
        command(10, "打开 Panzo", I::Home),
        Row::separator(),
        command(1, "区域截图", I::Capture),
        command(6, "延时 3 秒截图", I::Capture),
        Row::separator(),
        command(2, "区域录屏 / 停止", I::Record),
        command(13, "暂停 / 继续", I::Pause),
        command(14, "鼠标跟随缩放", I::Cursor),
        command(15, "录制设置…", I::Settings),
        command(16, "录音设备与电平…", I::Microphone),
        Row::separator(),
        command(3, "贴出剪贴板图片", I::Pin),
        command(4, "显示 / 隐藏贴图", I::Image),
        command(5, "恢复全部贴图交互", I::Cursor),
        command(7, "恢复最近关闭的贴图", I::Restore),
        command(8, "历史与贴图管理", I::Image),
        Row::separator(),
        command(9, "关闭到托盘", I::Minimize).checked(close_to_tray),
        command(11, "隐藏主窗口", I::Minimize),
        Row::separator(),
        command(12, "退出 Panzo", I::Close),
    ]
}

#[cfg(test)]
mod close_tests {
    use super::*;
    #[test]
    fn nested_close_attempts_keep_outer_confirmation_locked_and_cancel_allows_retry() {
        assert!(queue_exit());
        assert!(!queue_exit());
        let outer = begin_close_confirmation().unwrap();
        for _ in 0..80 {
            assert!(begin_close_confirmation().is_none());
            assert!(close_confirmation_open());
            assert!(!queue_exit());
        }
        drop(outer);
        assert!(!close_confirmation_open());
        assert!(queue_exit());
        assert!(!queue_exit());
        let retry = begin_close_confirmation().unwrap();
        assert!(!queue_exit());
        drop(retry);
        assert!(queue_exit());
        clear_queued_exit();
    }
}
