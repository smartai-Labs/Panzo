//! Compact capture-excluded recording strip. The owning recorder retains lifecycle control.
use super::{
    desktop_ui as ui,
    editor_ui::{self, EditorTheme, IconKind},
    recorder::RecordingControl,
    workspace_ui as controls,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};
struct State {
    control: RecordingControl,
    label: HWND,
    pause: HWND,
    camera: HWND,
    stop: HWND,
    created: std::time::Instant,
    theme: std::cell::Cell<super::ui_preferences::ThemeMode>,
}
pub struct RecordingHud {
    hwnd: HWND,
    state: Box<State>,
}
impl RecordingHud {
    pub fn new(_owner: HWND, control: RecordingControl) -> Result<Self, String> {
        controls::register(w!("PanzoRecordingHud"), Some(procedure))?;
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("PanzoRecordingHud"),
                w!("Panzo · 录制控制"),
                WS_POPUP | WS_CLIPCHILDREN,
                40,
                40,
                450,
                68,
                None,
                None,
                None,
                None,
            )
        }
        .map_err(|e| e.to_string())?;
        if let Err(e) = unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) } {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return Err(e.to_string());
        }
        ui::install(hwnd);
        editor_ui::apply_window_chrome(hwnd);
        let setup = (|| -> Result<State, String> {
            let label = controls::label(hwnd, "准备录制…", 10)?;
            let pause = controls::button(hwnd, "暂停录制", 1)?;
            ui::style(pause, Some(IconKind::Pause), true, false, true);
            let camera = controls::button(hwnd, "开启 / 关闭鼠标跟随缩放", 2)?;
            ui::style(camera, Some(IconKind::Cursor), true, false, true);
            let stop = controls::button(hwnd, "停止录制", 3)?;
            ui::style(stop, Some(IconKind::Stop), false, false, true);
            Ok(State {
                control,
                label,
                pause,
                camera,
                stop,
                created: std::time::Instant::now(),
                theme: std::cell::Cell::new(super::ui_preferences::theme()),
            })
        })();
        let mut state = match setup {
            Ok(s) => Box::new(s),
            Err(e) => {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                return Err(e);
            }
        };
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&raw mut *state) as isize);
        }
        layout(hwnd, &state);
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        let hud = Self { hwnd, state };
        hud.update();
        Ok(hud)
    }
    pub fn update(&self) {
        let mode = super::ui_preferences::theme();
        if self.state.theme.replace(mode) != mode {
            ui::refresh(self.hwnd);
        }
        let c = &self.state.control;
        let seconds = c.elapsed().0.max(0) / 10_000_000;
        let status = if c.recording_started() {
            format!(
                "{}  {:02}:{:02}\n系统 {}%  ·  麦克风 {}%",
                if c.paused() { "已暂停" } else { "录制中" },
                seconds / 60,
                seconds % 60,
                c.levels[0].load(std::sync::atomic::Ordering::Relaxed) * 100 / 32768,
                c.levels[1].load(std::sync::atomic::Ordering::Relaxed) * 100 / 32768
            )
        } else {
            format!(
                "准备录制\n倒计时 {}",
                u64::from(c.options.countdown_seconds)
                    .saturating_sub(self.state.created.elapsed().as_secs())
            )
        };
        controls::text(self.state.label, &status);
        controls::text(
            self.state.pause,
            if c.paused() {
                "继续录制"
            } else {
                "暂停录制"
            },
        );
        ui::style(
            self.state.pause,
            Some(if c.paused() {
                IconKind::Play
            } else {
                IconKind::Pause
            }),
            true,
            false,
            true,
        );
        ui::selected(self.state.camera, c.camera_active());
        unsafe {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                self.state.pause,
                c.recording_started() && !c.stop_requested(),
            );
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                self.state.camera,
                c.recording_started() && !c.stop_requested(),
            );
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                self.state.stop,
                !c.stop_requested(),
            );
        }
        ui::invalidate(self.hwnd);
    }
}
fn layout(h: HWND, s: &State) {
    let t = EditorTheme::current(editor_ui::window_dpi(h));
    let d = |n| t.scale(n);
    unsafe {
        let _ = SetWindowPos(
            h,
            None,
            0,
            0,
            d(450),
            d(68),
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    controls::place(s.label, d(36), d(12), d(260), d(46));
    for (button, x) in [(s.pause, 310), (s.camera, 352), (s.stop, 394)] {
        controls::place(button, d(x), d(17), d(34), d(34));
    }
}
impl Drop for RecordingHud {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(h, GWLP_USERDATA) } as *mut State;
    if let Some(s) = unsafe { ptr.as_ref() } {
        match m {
            WM_COMMAND => {
                match w.0 & 0xffff {
                    1 => {
                        let _ = s.control.toggle_pause();
                    }
                    2 => {
                        let _ = s.control.set_camera_enabled(!s.control.camera_active());
                    }
                    3 => s.control.request_stop(),
                    _ => {}
                }
                return LRESULT(0);
            }
            WM_CLOSE => return LRESULT(0),
            WM_NCHITTEST => return LRESULT(HTCAPTION as isize),
            WM_DPICHANGED => {
                if l.0 != 0 {
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
                }
                layout(h, s);
                ui::refresh(h);
                return LRESULT(0);
            }
            WM_PAINT => {
                let mut ps = Gdi::PAINTSTRUCT::default();
                let dc = unsafe { Gdi::BeginPaint(h, &raw mut ps) };
                let t = EditorTheme::current(editor_ui::window_dpi(h));
                editor_ui::fill_rect(dc, &controls::rect(h), t.palette.panel);
                editor_ui::draw_icon(
                    dc,
                    ui::rect(t.scale(15), t.scale(19), t.scale(10), t.scale(10)),
                    IconKind::Record,
                    if s.control.paused() {
                        t.palette.warning
                    } else {
                        t.palette.danger
                    },
                );
                unsafe {
                    let _ = Gdi::EndPaint(h, &raw const ps);
                }
                return LRESULT(0);
            }
            _ => {}
        }
    }
    unsafe { DefWindowProcW(h, m, w, l) }
}
#[cfg(test)]
mod review {
    use super::*;
    #[test]
    #[ignore = "native HUD review; isolated LOCALAPPDATA and PANZO_UI_REVIEW_DIR"]
    fn recording_strip_native_theme_and_capture_exclusion() {
        use crate::platform::{
            capture_tools,
            ui_preferences::{self, ThemeMode},
        };
        let out =
            std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").expect("output"));
        std::fs::create_dir_all(&out).unwrap();
        let original = ui_preferences::theme();
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            ui_preferences::set_theme(mode).unwrap();
            let hud = RecordingHud::new(HWND::default(), RecordingControl::default()).unwrap();
            controls::text(hud.state.label, "录制中  01:23\n系统 37%  ·  麦克风 18%");
            unsafe {
                for h in [hud.state.pause, hud.state.camera] {
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(h, true);
                }
                let _ = Gdi::UpdateWindow(hud.hwnd);
            }
            let mut affinity = 0;
            unsafe {
                GetWindowDisplayAffinity(hud.hwnd, &raw mut affinity).unwrap();
            }
            assert_eq!(affinity, WDA_EXCLUDEFROMCAPTURE.0);
            assert_eq!(
                unsafe { SendMessageW(hud.hwnd, WM_NCHITTEST, None, Some(LPARAM(0))) }.0,
                HTCAPTION as isize
            );
            // This synthetic fixture never starts capture. Verify exclusion first,
            // then allow PrintWindow solely to inspect the strip's pixels.
            unsafe {
                SetWindowDisplayAffinity(hud.hwnd, WDA_NONE).unwrap();
                let _ = windows::Win32::Graphics::Dwm::DwmFlush();
            }
            capture_tools::review_window_png(
                hud.hwnd,
                &out.join(if mode == ThemeMode::Dark {
                    "recording-strip-dark.png"
                } else {
                    "recording-strip-light.png"
                }),
            );
        }
        ui_preferences::set_theme(original).unwrap();
    }
}
