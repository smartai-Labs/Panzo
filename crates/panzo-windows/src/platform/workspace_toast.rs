//! Owned, non-activating notifications positioned inside the application client.
use crate::platform::{editor_ui as editor, workspace_ui as ui};
use std::time::{Duration, Instant};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi as gdi,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};
const TIMER: usize = 1;
const FADE: Duration = Duration::from_millis(160);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Info,
    Success,
    Error,
}
impl Kind {
    fn duration(self) -> Duration {
        Duration::from_millis(if self == Self::Error { 5000 } else { 2500 })
    }
}
struct State {
    owner: HWND,
    message: String,
    kind: Kind,
    started: Option<Instant>,
    alpha: u8,
}
pub(super) struct Toast(HWND);
impl Toast {
    pub fn new(owner: HWND) -> Result<Self, String> {
        ui::register(w!("PanzoToast"), Some(procedure))?;
        let module = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }
            .map_err(|e| e.to_string())?;
        let h = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("PanzoToast"),
                w!(""),
                WS_POPUP,
                0,
                0,
                1,
                1,
                Some(owner),
                None,
                Some(module.into()),
                None,
            )
        }
        .map_err(|e| format!("Notification window: {e}"))?;
        let state = Box::new(State {
            owner,
            message: String::new(),
            kind: Kind::Info,
            started: None,
            alpha: 0,
        });
        unsafe {
            SetWindowLongPtrW(h, GWLP_USERDATA, Box::into_raw(state) as isize);
        }
        Ok(Self(h))
    }
    pub fn show(&self, kind: Kind, text: &str) {
        let Some(s) = (unsafe { state(self.0) }) else {
            return;
        };
        if text.trim().is_empty() {
            s.dismiss(self.0);
            return;
        }
        // Refresh the single existing surface without flashing duplicate messages.
        let already_visible = unsafe { IsWindowVisible(self.0) }.as_bool();
        s.message = text.replace(['\r', '\n'], " ");
        s.kind = kind;
        s.started = Some(Instant::now());
        s.alpha = if already_visible { 255 } else { 0 };
        ui::text(self.0, &s.message);
        s.layout(self.0);
        unsafe {
            SetTimer(Some(self.0), TIMER, 16, None);
        }
        s.tick(self.0);
        crate::platform::desktop_ui::invalidate(self.0);
    }
    pub fn suspend(&self) {
        ui::show(self.0, false);
    }
    pub fn layout(&self) {
        if let Some(s) = unsafe { state(self.0) } {
            s.layout(self.0);
            s.tick(self.0);
            crate::platform::desktop_ui::invalidate(self.0);
        }
    }
}
impl State {
    fn dismiss(&mut self, h: HWND) {
        self.started = None;
        self.message.clear();
        unsafe {
            let _ = KillTimer(Some(h), TIMER);
        }
        ui::show(h, false);
        ui::text(h, "");
    }
    fn layout(&self, h: HWND) {
        let r = ui::rect(self.owner);
        let theme = editor::EditorTheme::current(editor::window_dpi(self.owner));
        let _scale = editor::DrawingScale::enter(theme.dpi);
        let d = |n| theme.scale(n);
        let dc = unsafe { gdi::GetDC(Some(h)) };
        let text_width = editor::measure_text_width(dc, &self.message, editor::TextStyle::Body);
        let previous = editor::select_font(dc, editor::TextStyle::Body);
        let mut metrics = gdi::TEXTMETRICW::default();
        unsafe {
            let _ = gdi::GetTextMetricsW(dc, &mut metrics);
            gdi::SelectObject(dc, previous);
            gdi::ReleaseDC(Some(h), dc);
        }
        let width = (text_width + d(64))
            .clamp(d(180), d(560))
            .min((r.right - d(32)).max(1));
        let height = (metrics.tmHeight + d(20)).max(d(40));
        let y = super::caption_height(self.owner)
            + d(crate::platform::workbench_layout::WORKSPACES + 16);
        let mut origin = windows::Win32::Foundation::POINT {
            x: (r.right - width) / 2,
            y: y.min((r.bottom - height).max(0)),
        };
        unsafe {
            let _ = gdi::ClientToScreen(self.owner, &mut origin);
            let _ = SetWindowPos(
                h,
                None,
                origin.x,
                origin.y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
            let region = gdi::CreateRoundRectRgn(0, 0, width + 1, height + 1, d(12), d(12));
            if !region.is_invalid() && gdi::SetWindowRgn(h, Some(region), false) == 0 {
                let _ = gdi::DeleteObject(region.into());
            }
        }
    }
    fn tick(&mut self, h: HWND) {
        let Some(start) = self.started else {
            return;
        };
        let elapsed = start.elapsed();
        let duration = self.kind.duration();
        if elapsed >= duration {
            self.dismiss(h);
            return;
        }
        let active = unsafe { GetForegroundWindow() } == self.owner
            && unsafe { IsWindowVisible(self.owner) }.as_bool()
            && !unsafe { IsIconic(self.owner) }.as_bool();
        if !active {
            ui::show(h, false);
            return;
        }
        let mut alpha = 255;
        if crate::platform::system_preferences::animations() {
            let remaining = duration.saturating_sub(elapsed);
            if remaining < FADE {
                alpha = (remaining.as_secs_f64() / FADE.as_secs_f64() * 255.).round() as u8;
            } else if self.alpha < 255 && elapsed < FADE {
                alpha = (elapsed.as_secs_f64() / FADE.as_secs_f64() * 255.).round() as u8;
            }
        }
        self.alpha = alpha;
        unsafe {
            let _ = SetLayeredWindowAttributes(
                h,
                windows::Win32::Foundation::COLORREF(0),
                alpha,
                LWA_ALPHA,
            );
        }
        ui::show(h, true);
    }
    fn paint(&self, h: HWND, dc: gdi::HDC) {
        let theme = editor::EditorTheme::current(editor::window_dpi(self.owner));
        let _scale = editor::DrawingScale::enter(theme.dpi);
        let r = ui::rect(h);
        editor::rounded_surface(
            dc,
            r,
            theme.palette.raised,
            theme.palette.raised,
            theme.scale(6),
        );
        let path = match self.kind {
            Kind::Success => r#"<circle cx="12" cy="12" r="9"/><path d="m8 12 3 3 5-6"/>"#,
            Kind::Info => r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7v1"/>"#,
            Kind::Error => r#"<circle cx="12" cy="12" r="9"/><path d="M12 7v6M12 16v1"/>"#,
        };
        let size = theme.scale(18);
        let color = if self.kind == Kind::Error {
            theme.palette.danger
        } else {
            theme.palette.accent
        };
        editor::draw_icon(
            dc,
            RECT {
                left: theme.scale(14),
                top: (r.bottom - size) / 2,
                right: theme.scale(14) + size,
                bottom: (r.bottom + size) / 2,
            },
            editor::IconKind::Prototype(path),
            color,
        );
        editor::draw_single_line(
            dc,
            RECT {
                left: theme.scale(42),
                right: r.right - theme.scale(14),
                ..r
            },
            &self.message,
            theme.palette.text,
            editor::TextStyle::Body,
        );
    }
}
unsafe fn state(h: HWND) -> Option<&'static mut State> {
    unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State).as_mut() }
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Some(s) = (unsafe { state(h) }) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    match m {
        WM_TIMER if w.0 == TIMER => s.tick(h),
        WM_NCHITTEST => return LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = gdi::PAINTSTRUCT::default();
            let dc = unsafe { gdi::BeginPaint(h, &mut ps) };
            s.paint(h, dc);
            unsafe {
                let _ = gdi::EndPaint(h, &ps);
            }
        }
        WM_NCDESTROY => unsafe {
            let _ = KillTimer(Some(h), TIMER);
            let ptr = GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State;
            SetWindowLongPtrW(h, GWLP_USERDATA, 0);
            drop(Box::from_raw(ptr));
            return DefWindowProcW(h, m, w, l);
        },
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
    // Desktop test processes do not own the user's foreground permission.
    // Attach only while activating our own fixture; production never does this.
    unsafe fn foreground(h: HWND) {
        use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
        unsafe {
            let current = GetCurrentThreadId();
            let other = GetWindowThreadProcessId(GetForegroundWindow(), None);
            let attached = current != other && AttachThreadInput(current, other, true).as_bool();
            let _ = SetForegroundWindow(h);
            if attached {
                let _ = AttachThreadInput(current, other, false);
            }
        }
    }
    fn pump(ms: u64) {
        let until = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < until {
            let mut message = MSG::default();
            unsafe {
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            std::thread::sleep(Duration::from_millis(4));
        }
    }
    #[test]
    #[ignore = "requires isolated Windows desktop/profile"]
    fn native_workspace_toast_lifecycle_and_job_routing() {
        use crate::platform::{
            image_tasks::{Output, Update},
            workspace,
        };
        use std::sync::{Arc, atomic::AtomicBool, mpsc};
        let library = std::env::current_dir()
            .unwrap()
            .join(".tmp")
            .join(format!("toast-review-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&library).unwrap();
        crate::platform::ui_preferences::set_theme(
            crate::platform::ui_preferences::ThemeMode::Dark,
        )
        .unwrap();
        let root = workspace::create().unwrap();
        struct Cleanup(HWND);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        let _cleanup = Cleanup(root);
        let video = ui::child(root, w!("STATIC"), "", 440, WINDOW_STYLE(0)).unwrap();
        workspace::attach(root, video, &library, false).unwrap();
        unsafe {
            SetWindowPos(root, Some(HWND_TOPMOST), 40, 40, 1100, 800, SWP_NOACTIVATE).unwrap();
            foreground(root);
        }
        let input = ui::edit(root, "Keep editing", 441, false).unwrap();
        ui::place(input, 50, 600, 200, 28);
        unsafe {
            let _ = SetFocus(Some(input));
        }
        let toast =
            unsafe { &*(GetWindowLongPtrW(root, GWLP_USERDATA) as *const super::super::State) }
                .toast
                .0;
        assert!(!unsafe { IsWindowVisible(toast) }.as_bool());
        unsafe {
            foreground(root);
        }
        workspace::notice("已复制");
        pump(240);
        assert!(unsafe { IsWindowVisible(toast) }.as_bool());
        assert_eq!(unsafe { GetFocus() }, input);
        assert_eq!(ui::value(toast), "已复制");
        assert_eq!(
            unsafe { SendMessageW(toast, WM_NCHITTEST, None, None) }.0,
            HTTRANSPARENT as isize
        );
        let mut r = RECT::default();
        unsafe {
            GetWindowRect(toast, &mut r).unwrap();
        }
        let mut p = windows::Win32::Foundation::POINT {
            x: r.left,
            y: r.top,
        };
        unsafe {
            let _ = gdi::ScreenToClient(root, &mut p);
        }
        assert!((p.x * 2 + r.right - r.left - ui::rect(root).right).abs() <= 1);
        assert_eq!(
            p.y,
            workspace::caption_height(root)
                + ui::scale(root, crate::platform::workbench_layout::WORKSPACES + 16)
        );
        // Refresh repeated content instead of queuing or starting another surface.
        unsafe { state(toast) }.unwrap().started =
            Some(Instant::now() - Duration::from_millis(2400));
        workspace::notice("已复制");
        pump(30);
        assert!(
            unsafe { state(toast) }.unwrap().started.unwrap().elapsed()
                < Duration::from_millis(200)
        );
        assert_eq!(unsafe { state(toast) }.unwrap().alpha, 255);
        unsafe {
            SetWindowPos(
                root,
                None,
                0,
                0,
                1400,
                850,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .unwrap();
        }
        unsafe {
            GetWindowRect(toast, &mut r).unwrap();
        }
        p.x = r.left;
        p.y = r.top;
        unsafe {
            let _ = gdi::ScreenToClient(root, &mut p);
        }
        assert!((p.x * 2 + r.right - r.left - ui::rect(root).right).abs() <= 1);
        unsafe { state(toast) }.unwrap().started =
            Some(Instant::now() - Duration::from_millis(2600));
        unsafe {
            SendMessageW(toast, WM_TIMER, Some(WPARAM(TIMER)), None);
        }
        assert!(!unsafe { IsWindowVisible(toast) }.as_bool());
        assert!(unsafe { state(toast) }.unwrap().started.is_none());
        assert!(ui::value(toast).is_empty());
        workspace::notice_error("无法完成识别，请重试");
        pump(180);
        unsafe { state(toast) }.unwrap().started =
            Some(Instant::now() - Duration::from_millis(3000));
        unsafe {
            SendMessageW(toast, WM_TIMER, Some(WPARAM(TIMER)), None);
        }
        assert!(unsafe { IsWindowVisible(toast) }.as_bool());
        assert_eq!(unsafe { state(toast) }.unwrap().kind, Kind::Error);
        unsafe { state(toast) }.unwrap().started =
            Some(Instant::now() - Duration::from_millis(5100));
        unsafe {
            SendMessageW(toast, WM_TIMER, Some(WPARAM(TIMER)), None);
        }
        assert!(!unsafe { IsWindowVisible(toast) }.as_bool());
        // Switching to an unrelated top-level window immediately hides the toast.
        workspace::notice("焦点切换");
        pump(180);
        let other = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!("Toast focus test"),
                WS_OVERLAPPEDWINDOW,
                50,
                50,
                250,
                150,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        unsafe {
            let _ = ShowWindow(other, SW_SHOW);
            foreground(other);
        }
        pump(40);
        assert_eq!(unsafe { GetForegroundWindow() }, other);
        assert!(!unsafe { IsWindowVisible(toast) }.as_bool());
        unsafe {
            DestroyWindow(other).unwrap();
            foreground(root);
        }
        unsafe { state(toast) }.unwrap().dismiss(toast);
        // Exercise the actual host's job receiver: progress stays in task UI.
        let host =
            unsafe { &mut *(GetWindowLongPtrW(root, GWLP_USERDATA) as *mut super::super::State) };
        let (tx, rx) = mpsc::channel();
        host.jobs.push(super::super::Job {
            target: workspace::Target::Image {
                id: u64::MAX,
                revision: 0,
            },
            name: "离线 OCR".into(),
            cancel: Arc::new(AtomicBool::new(false)),
            receiver: rx,
        });
        tx.send(Update::Progress("正在识别…".into())).unwrap();
        host.poll();
        assert!(!unsafe { IsWindowVisible(toast) }.as_bool());
        tx.send(Update::Done(Ok(Output::Text("识别结果".into()))))
            .unwrap();
        host.poll();
        pump(180);
        assert_eq!(ui::value(toast), "离线 OCR已完成");
        assert_eq!(unsafe { state(toast) }.unwrap().kind, Kind::Success);
        assert!(host.jobs.is_empty());
        // Verify theme rendering, hidden/minimized behavior and real timer expiry.
        if let Some(out) = std::env::var_os("PANZO_TOAST_REVIEW_DIR") {
            let out = std::path::PathBuf::from(out);
            std::fs::create_dir_all(&out).unwrap();
            crate::platform::capture_tools::review_window_png(root, &out.join("host-dark.png"));
            let r = ui::rect(toast);
            editor::render_review_png(&out.join("toast-dark.png"), r.right, r.bottom, |dc, _| {
                unsafe { state(toast) }.unwrap().paint(toast, dc)
            });
            crate::platform::ui_preferences::set_theme(
                crate::platform::ui_preferences::ThemeMode::Light,
            )
            .unwrap();
            host.poll();
            pump(80);
            crate::platform::capture_tools::review_window_png(root, &out.join("host-light.png"));
            editor::render_review_png(&out.join("toast-light.png"), r.right, r.bottom, |dc, _| {
                unsafe { state(toast) }.unwrap().paint(toast, dc)
            });
        }
        unsafe {
            let _ = ShowWindow(root, SW_MINIMIZE);
        }
        pump(30);
        assert!(!unsafe { IsWindowVisible(toast) }.as_bool());
        unsafe {
            let _ = ShowWindow(root, SW_RESTORE);
            foreground(root);
        }
        workspace::notice("短时提示");
        pump(180);
        assert!(unsafe { IsWindowVisible(toast) }.as_bool());
        pump(2500);
        assert!(!unsafe { IsWindowVisible(toast) }.as_bool());
        assert!(unsafe { state(toast) }.unwrap().started.is_none());
        unsafe {
            DestroyWindow(root).unwrap();
        }
        assert!(!unsafe { IsWindow(Some(toast)) }.as_bool());
        std::mem::forget(_cleanup);
        eprintln!(
            "Toast: centered, focus retained, click-through, repeat refresh, resize, 2.5s/5s expiry, progress/completion routing, themes, minimize and cleanup passed"
        );
    }
}
