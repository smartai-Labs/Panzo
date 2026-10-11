//! Recording setup is an owned workspace child. Commands are posted to the recorder,
//! never dispatched synchronously while either window has a mutable state borrow.
use super::{
    desktop_ui as desk,
    editor_ui::{self, IconKind as I, TextStyle},
    recorder::RecordingOptions,
    workspace_ui as ui,
};
use std::cell::Cell;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::*,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};
pub(crate) const MESSAGE: u32 = WM_APP + 0x560;
thread_local! {static OPEN:Cell<HWND>=const{Cell::new(HWND(std::ptr::null_mut()))};static ADVANCED:Cell<HWND>=const{Cell::new(HWND(std::ptr::null_mut()))};}
struct State {
    owner: HWND,
    mode: usize,
    modes: [HWND; 3],
    source: HWND,
    reselect: HWND,
    fps: HWND,
    audio: HWND,
    follow: HWND,
    advanced: HWND,
    cancel: HWND,
    start: HWND,
    labels: [HWND; 3],
    enabled: bool,
}
unsafe fn state(h: HWND) -> Option<&'static mut State> {
    unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State).as_mut() }
}
pub(crate) fn is_open() -> bool {
    OPEN.with(|h| unsafe { IsWindow(Some(h.get())) }.as_bool())
}
pub(crate) fn close() {
    ADVANCED.with(|v| v.set(HWND::default()));
    let h = OPEN.with(|v| v.replace(HWND::default()));
    if unsafe { IsWindow(Some(h)) }.as_bool() {
        unsafe {
            let _ = DestroyWindow(h);
        }
    }
}
pub(crate) fn source(label: &str) {
    let h = OPEN.with(Cell::get);
    if let Some(s) = unsafe { state(h) } {
        ui::text(s.source, label);
        ui::show(h, true);
    }
}
pub(crate) fn open(
    owner: HWND,
    label: &str,
    mode: usize,
    options: &RecordingOptions,
) -> Result<(), String> {
    close();
    ui::register(w!("PanzoRecordingSetup"), Some(proc))?;
    let h = ui::panel(super::workspace::window(owner), w!("PanzoRecordingSetup"))?;
    desk::install_canvas(h);
    let modes = [
        ui::button(h, "区域", 1)?,
        ui::button(h, "窗口", 2)?,
        ui::button(h, "屏幕", 3)?,
    ];
    for (b, icon) in modes.iter().zip([I::Capture, I::Window, I::Monitor]) {
        desk::style(*b, Some(icon), false, false, false);
    }
    let fps = ui::combo(h, &["30 fps", "60 fps"], usize::from(options.fps == 60), 5)?;
    let audio = ui::combo(
        h,
        &["系统声音", "麦克风", "系统声音 + 麦克风", "无声音"],
        match (options.audio.system, options.audio.microphone) {
            (true, false) => 0,
            (false, true) => 1,
            (true, true) => 2,
            _ => 3,
        },
        6,
    )?;
    let follow = ui::button(h, "鼠标跟踪缩放", 7)?;
    desk::choice(follow, desk::ChoiceVisual::Toggle(options.camera.enabled));
    let s = Box::new(State {
        owner,
        mode,
        modes,
        source: ui::label(h, label, 4)?,
        reselect: ui::button(h, "重新选择来源", 8)?,
        fps,
        audio,
        follow,
        advanced: ui::button(h, "高级设置", 11)?,
        cancel: ui::button(h, "取消", 12)?,
        start: ui::button(h, "开始录制", 10)?,
        labels: [
            ui::label(h, "帧率", 21)?,
            ui::label(h, "声音", 22)?,
            ui::label(h, "鼠标跟踪缩放", 23)?,
        ],
        enabled: options.camera.enabled,
    });
    desk::style(s.start, None, false, true, false);
    desk::style(s.advanced, Some(I::Settings), true, false, false);
    unsafe {
        SetWindowLongPtrW(h, GWLP_USERDATA, Box::into_raw(s) as isize);
        SetTimer(Some(h), 1, 250, None);
    }
    OPEN.with(|v| v.set(h));
    layout(h);
    ui::show(h, true);
    unsafe {
        let _ = SetWindowPos(
            h,
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
    Ok(())
}
fn layout(h: HWND) {
    let Some(s) = (unsafe { state(h) }) else {
        return;
    };
    let root = super::workspace::window(s.owner);
    let r = ui::rect(root);
    let d = |v| ui::scale(h, v);
    let width = d(460).min(r.right - d(24)).max(1);
    let height = d(486).min(r.bottom - d(40)).max(1);
    let x = (r.right - width) / 2;
    let y = (r.bottom - height) / 2;
    ui::place(h, x, y, width, height);
    let inner = width - d(40);
    let cw = (inner - d(16)) / 3;
    for (i, b) in s.modes.iter().enumerate() {
        ui::place(*b, d(20) + (cw + d(8)) * i as i32, d(56), cw, d(62));
        desk::selected(*b, s.mode == i);
    }
    ui::place(s.source, d(36), d(150), inner - d(32), d(42));
    ui::place(s.reselect, (width - d(152)) / 2, d(210), d(152), d(28));
    for (i, label) in s.labels.iter().enumerate() {
        ui::place(*label, d(20), d(280 + i as i32 * 38), d(140), d(28));
    }
    ui::place(s.fps, d(174), d(280), width - d(194), d(28));
    ui::place(s.audio, d(174), d(318), width - d(194), d(28));
    ui::place(s.follow, width - d(60), d(356), d(40), d(26));
    ui::place(s.advanced, d(20), height - d(48), d(108), d(28));
    ui::place(s.cancel, width - d(212), height - d(48), d(80), d(28));
    ui::place(s.start, width - d(120), height - d(48), d(100), d(28));
}
unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Some(s) = (unsafe { state(h) }) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    match m {
        WM_TIMER => {
            if unsafe { IsWindowVisible(h) }.as_bool() {
                layout(h);
            }
        }
        WM_CLOSE => {
            close();
            return LRESULT(0);
        }
        WM_COMMAND if w.0 >> 16 == 0 => {
            let id = w.0 & 65535;
            if id == 12 {
                close();
                return LRESULT(0);
            }
            if id == 7 {
                s.enabled = !s.enabled;
                desk::choice(s.follow, desk::ChoiceVisual::Toggle(s.enabled));
                return LRESULT(0);
            }
            if (1..=3).contains(&id) {
                s.mode = id - 1;
                layout(h);
            }
            let command = if id == 8 { s.mode + 1 } else { id };
            if [1, 2, 3, 10, 11].contains(&command) {
                let packed = (if ui::selection(s.fps) == 1 { 60 } else { 30 })
                    | (ui::selection(s.audio) << 8)
                    | (usize::from(s.enabled) << 12);
                let owner = s.owner;
                if command == 1 || command == 2 || command == 11 {
                    ui::show(h, false);
                }
                if command == 10 {
                    close();
                }
                unsafe {
                    let _ = PostMessageW(
                        Some(owner),
                        MESSAGE,
                        WPARAM(command),
                        LPARAM(packed as isize),
                    );
                }
                return LRESULT(0);
            }
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &mut ps) };
            let r = ui::rect(h);
            let t = editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
            let d = |v| t.scale(v);
            let f = desk::BufferedFrame::new(dc, r);
            editor_ui::fill_rect(f.dc, &r, t.palette.panel);
            desk::text_style(
                f.dc,
                desk::rect(d(20), d(12), r.right - d(40), d(28)),
                "新录制",
                t.palette.text,
                t.dpi,
                false,
                TextStyle::Heading,
            );
            desk::line(f.dc, desk::rect(0, d(44), r.right, 1), t);
            let boxr = desk::rect(d(20), d(132), r.right - d(40), d(126));
            for x in (boxr.left..boxr.right).step_by(d(8).max(1) as usize) {
                for y in [boxr.top, boxr.bottom] {
                    editor_ui::fill_rect(f.dc, &desk::rect(x, y, d(4), 1), t.palette.border);
                }
            }
            for y in (boxr.top..boxr.bottom).step_by(d(8).max(1) as usize) {
                for x in [boxr.left, boxr.right] {
                    editor_ui::fill_rect(f.dc, &desk::rect(x, y, 1, d(4)), t.palette.border);
                }
            }
            desk::line(f.dc, desk::rect(0, r.bottom - d(64), r.right, 1), t);
            drop(f);
            unsafe {
                let _ = EndPaint(h, &ps);
            }
        }
        WM_NCDESTROY => {
            OPEN.with(|v| {
                if v.get() == h {
                    v.set(HWND::default());
                }
            });
            unsafe {
                SetWindowLongPtrW(h, GWLP_USERDATA, 0);
                drop(Box::from_raw(std::ptr::from_mut(s)));
            }
        }
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}

pub(crate) fn dialog_message(msg: &MSG) -> bool {
    let h = OPEN.with(Cell::get);
    if !unsafe { IsWindowVisible(h) }.as_bool() {
        return false;
    }
    if msg.message == WM_KEYDOWN && msg.wParam.0 == 27 {
        close();
        return true;
    }
    unsafe { IsDialogMessageW(h, msg) }.as_bool()
}

pub(crate) fn advanced_opened(form: HWND) {
    if is_open() {
        ADVANCED.with(|v| v.set(form));
    }
}
pub(crate) fn advanced_closed(form: HWND) {
    if !ADVANCED.with(|v| v.get() == form) {
        return;
    }
    ADVANCED.with(|v| v.set(HWND::default()));
    let h = OPEN.with(Cell::get);
    if let Some(s) = unsafe { state(h) } {
        unsafe {
            let _ = PostMessageW(Some(s.owner), MESSAGE, WPARAM(20), LPARAM(0));
        }
    }
}
#[cfg(test)]
pub(crate) fn review_handle() -> HWND {
    OPEN.with(Cell::get)
}
