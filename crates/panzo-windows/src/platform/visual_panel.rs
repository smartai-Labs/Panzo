//! Properties of the selected imported visual clip, with immediate, undoable edits.
use super::{desktop_ui as desk, editor_ui, preview_window, workspace_ui as ui};
use panzo_core::TimeTick;
use std::cell::RefCell;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::*,
        UI::{Input::KeyboardAndMouse::GetFocus, WindowsAndMessaging::*},
    },
    core::w,
};
thread_local! {static PANELS:RefCell<Vec<(HWND,HWND)>>=const{RefCell::new(Vec::new())};}
struct State {
    owner: HWND,
    id: String,
    audio: bool,
    title: HWND,
    fields: Vec<(HWND, HWND)>,
    loading: bool,
    scroll: i32,
    revision: Option<(usize, u64)>,
}
unsafe fn state(h: HWND) -> Option<&'static mut State> {
    unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State).as_mut() }
}
pub(crate) fn page(h: HWND) -> Option<bool> {
    let owned = PANELS.with(|p| p.borrow().iter().any(|(_, v)| *v == h));
    if owned {
        unsafe { state(h) }.map(|s| s.audio)
    } else {
        None
    }
}
pub(crate) fn open(owner: HWND, id: &str, audio: bool) -> Result<(), String> {
    let existing = PANELS.with(|p| {
        p.borrow()
            .iter()
            .find(|(o, h)| *o == owner && unsafe { IsWindow(Some(*h)) }.as_bool())
            .map(|(_, h)| *h)
    });
    let h = if let Some(h) = existing {
        h
    } else {
        ui::register(w!("PanzoVisualProperties"), Some(proc))?;
        let h = ui::panel(owner, w!("PanzoVisualProperties"))?;
        desk::install_canvas(h);
        let title = ui::label(h, "", 1)?;
        let mut fields = Vec::new();
        for i in 0..11 {
            let label = ui::child(h, w!("STATIC"), "", 100 + i, WINDOW_STYLE(0x200))?;
            let field = ui::number(h, "", &[], 200 + i, None)?;
            fields.push((label, field));
        }
        let s = Box::new(State {
            owner,
            id: id.into(),
            audio,
            title,
            fields,
            loading: false,
            scroll: 0,
            revision: None,
        });
        unsafe {
            SetWindowLongPtrW(h, GWLP_USERDATA, Box::into_raw(s) as isize);
            SetTimer(Some(h), 1, 250, None);
        }
        PANELS.with(|p| p.borrow_mut().push((owner, h)));
        h
    };
    if let Some(s) = unsafe { state(h) } {
        s.id = id.into();
        s.audio = audio;
        s.scroll = 0;
        refresh(h, s)?;
        layout(h, s);
    }
    preview_window::attach_inspector_panel(owner, h)
}
fn refresh(h: HWND, s: &mut State) -> Result<(), String> {
    let (_, settings, _, _) = preview_window::media_snapshot(s.owner)?;
    s.revision = preview_window::media_revision(s.owner);
    let Some(c) = settings.visual_clips.iter().find(|c| c.id == s.id) else {
        ui::text(s.title, "请选择视频或图片片段");
        for (_, f) in &s.fields {
            unsafe {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(*f, false);
            }
        }
        return Ok(());
    };
    s.loading = true;
    ui::text(
        s.title,
        settings
            .visual_assets
            .iter()
            .find(|a| a.id == c.asset_id)
            .map_or("视频", |a| a.name.as_str()),
    );
    let values = if s.audio {
        vec![
            format!("{:.1}", c.gain * 100.0),
            secs(c.fade_in),
            secs(c.fade_out),
        ]
    } else {
        vec![
            secs(c.start),
            secs(c.source_in),
            secs(c.source_out),
            c.speed_percent.to_string(),
            format!("{:.1}", c.x * 100.0),
            format!("{:.1}", c.y * 100.0),
            format!("{:.1}", c.scale * 100.0),
            format!("{:.1}", f64::from(c.crop.left) / 10.0),
            format!("{:.1}", f64::from(c.crop.top) / 10.0),
            format!("{:.1}", f64::from(c.crop.right) / 10.0),
            format!("{:.1}", f64::from(c.crop.bottom) / 10.0),
        ]
    };
    for ((_, f), v) in s.fields.iter().zip(values) {
        ui::text(*f, &v);
        unsafe {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(*f, true);
        }
    }
    s.loading = false;
    desk::invalidate(h);
    Ok(())
}
fn secs(t: TimeTick) -> String {
    format!("{:.3}", t.0 as f64 / 1e7)
}
fn layout(h: HWND, s: &State) {
    let _batch = ui::layout_batch(h);
    let r = ui::rect(h);
    let d = |n| ui::scale(h, n);
    ui::place(s.title, d(16), d(14) - s.scroll, r.right - d(32), d(32));
    let labels: &[&str] = if s.audio {
        &["音量 %", "淡入 · 秒", "淡出 · 秒"]
    } else {
        &[
            "位置 · 秒",
            "入点 · 秒",
            "出点 · 秒",
            "速度 %",
            "水平位置 %",
            "垂直位置 %",
            "缩放 %",
            "裁剪左 %",
            "裁剪上 %",
            "裁剪右 %",
            "裁剪下 %",
        ]
    };
    for (i, (l, f)) in s.fields.iter().enumerate() {
        let show = i < labels.len();
        ui::show(*l, show);
        ui::show(*f, show);
        if show {
            ui::text(*l, labels[i]);
            let y = d(66 + i as i32 * 44) - s.scroll;
            ui::place(*l, d(16), y, (r.right - d(152)).max(d(60)), d(28));
            ui::place(*f, (r.right - d(120)).max(d(80)), y, d(104), d(28));
        }
    }
    desk::scroll_panel(h, d(80 + labels.len() as i32 * 44), r.bottom, s.scroll);
}
fn apply(s: &mut State) -> Result<(), String> {
    if s.loading {
        return Ok(());
    }
    let (root, expected, _, _) = preview_window::media_snapshot(s.owner)?;
    let mut next = expected.clone();
    let c = next
        .visual_clips
        .iter_mut()
        .find(|c| c.id == s.id)
        .ok_or("片段已删除")?;
    let v = s
        .fields
        .iter()
        .map(|(_, f)| ui::value(*f))
        .collect::<Vec<_>>();
    let num = |i: usize| {
        v[i].parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or("请输入有效数字".to_owned())
    };
    if s.audio {
        c.gain = num(0)? / 100.0;
        c.fade_in = panzo_core::media_edit::parse_time(&v[1])?;
        c.fade_out = panzo_core::media_edit::parse_time(&v[2])?;
    } else {
        c.start = panzo_core::media_edit::parse_time(&v[0])?;
        c.source_in = panzo_core::media_edit::parse_time(&v[1])?;
        c.source_out = panzo_core::media_edit::parse_time(&v[2])?;
        let speed = num(3)?;
        if !(25.0..=400.0).contains(&speed) {
            return Err("速度范围为 25–400%".into());
        }
        c.speed_percent = speed.round() as u16;
        c.x = num(4)? / 100.0;
        c.y = num(5)? / 100.0;
        c.scale = num(6)? / 100.0;
        for (i, margin) in [
            &mut c.crop.left,
            &mut c.crop.top,
            &mut c.crop.right,
            &mut c.crop.bottom,
        ]
        .into_iter()
        .enumerate()
        {
            let n = num(i + 7)?;
            if !(0.0..=95.0).contains(&n) {
                return Err("裁剪范围为 0–95%".into());
            }
            *margin = (n * 10.0).round() as u16;
        }
        c.fade_in = c.fade_in.min(c.duration());
        c.fade_out = c.fade_out.min(c.duration());
    }
    next.validate().map_err(|e| e.to_string())?;
    if next != expected {
        preview_window::apply_workspace_media(s.owner, &root, &expected, next)?;
    }
    Ok(())
}
unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Some(s) = (unsafe { state(h) }) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    match m {
        WM_SIZE => layout(h, s),
        WM_ERASEBKGND => return LRESULT(1),
        WM_COMMAND if !s.loading && (w.0 & 65535) >= 200 && (w.0 & 65535) < 211 => {
            let code = w.0 >> 16;
            if code == CBN_KILLFOCUS as usize || code == CBN_SELENDOK as usize {
                unsafe {
                    let _ = PostMessageW(Some(h), WM_APP + 1, WPARAM(0), LPARAM(0));
                }
            }
        }
        m if m == WM_APP + 1 => {
            if let Err(e) = apply(s) {
                super::workspace::notice_error(&e);
            }
        }
        WM_TIMER => {
            if unsafe { IsWindowVisible(h) }.as_bool() {
                let focus = unsafe { GetFocus() };
                if !unsafe { IsChild(h, focus) }.as_bool() {
                    if preview_window::media_snapshot(s.owner).is_ok() {
                        if s.revision != preview_window::media_revision(s.owner) {
                            let _ = refresh(h, s);
                        }
                    }
                }
            }
        }
        WM_MOUSEWHEEL => {
            let max = (ui::scale(h, 600) - ui::rect(h).bottom).max(0);
            s.scroll =
                (s.scroll - (w.0 >> 16) as i16 as i32 / 120 * ui::scale(h, 44)).clamp(0, max);
            layout(h, s);
            desk::invalidate(h);
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &mut ps) };
            editor_ui::fill_rect(
                dc,
                &ui::rect(h),
                editor_ui::EditorTheme::current(editor_ui::window_dpi(h))
                    .palette
                    .panel,
            );
            unsafe {
                let _ = EndPaint(h, &ps);
            }
        }
        WM_NCDESTROY => {
            PANELS.with(|p| p.borrow_mut().retain(|(_, v)| *v != h));
            unsafe {
                SetWindowLongPtrW(h, GWLP_USERDATA, 0);
                drop(Box::from_raw(std::ptr::from_mut(s)));
            }
        }
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}
