//! An anchored, themed colour popover. Values are BGRA, matching image marks.
use super::super::workspace_ui as ui;
use super::{
    editor_ui::{self, EditorTheme},
    rect, text,
};
use std::cell::Cell;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi,
        UI::{
            Input::KeyboardAndMouse::{
                EnableWindow, GetFocus, ReleaseCapture, SetCapture, SetFocus,
            },
            WindowsAndMessaging::*,
        },
    },
    core::w,
};
thread_local! {static ACTIVE:Cell<HWND>=const{Cell::new(HWND(std::ptr::null_mut()))};}
#[cfg(test)]
static REVIEW_HWND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(crate) const COLOR_PREVIEW: u32 = WM_APP + 391;
pub(crate) const COLOR_CLOSED: u32 = WM_APP + 394;
struct State {
    preview_owner: Option<HWND>,
    anchor: HWND,
    previous_focus: HWND,
    syncing: bool,
    drag: u8,
    gradient_hue: f64,
    gradient: Vec<u8>,
    color: [u8; 4],
    hue: f64,
    used: bool,
    closing: bool,
    hex: HWND,
    alpha: HWND,
    error: HWND,
    recent: Vec<[u8; 4]>,
}
fn hsv(h: f64, s: f64, v: f64, alpha: u8) -> [u8; 4] {
    let c = v * s;
    let h = h / 60.;
    let x = c * (1. - (h % 2. - 1.).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    [
        ((b + m) * 255.).round() as u8,
        ((g + m) * 255.).round() as u8,
        ((r + m) * 255.).round() as u8,
        alpha,
    ]
}
fn hue(c: [u8; 4]) -> f64 {
    let r = f64::from(c[2]) / 255.;
    let g = f64::from(c[1]) / 255.;
    let b = f64::from(c[0]) / 255.;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if d == 0. {
        0.
    } else if max == r {
        (60. * ((g - b) / d)).rem_euclid(360.)
    } else if max == g {
        60. * ((b - r) / d + 2.)
    } else {
        60. * ((r - g) / d + 4.)
    }
}
fn sync(s: &mut State) {
    s.syncing = true;
    ui::text(
        s.hex,
        &format!("#{:02X}{:02X}{:02X}", s.color[2], s.color[1], s.color[0]),
    );
    ui::text(s.alpha, &format!("{}", u32::from(s.color[3]) * 100 / 255));
    ui::text(s.error, "");
    s.syncing = false;
    publish(s);
}
fn publish(s: &State) {
    if let Some(owner) = s.preview_owner {
        unsafe {
            SendMessageW(
                owner,
                COLOR_PREVIEW,
                Some(WPARAM(u32::from_le_bytes(s.color) as usize)),
                Some(LPARAM(s.anchor.0 as isize)),
            );
        }
    }
}
fn paint_gradient(dc: Gdi::HDC, r: RECT, pixels: &[u8], width: i32, height: i32) {
    let info = Gdi::BITMAPINFO {
        bmiHeader: Gdi::BITMAPINFOHEADER {
            biSize: std::mem::size_of::<Gdi::BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: Gdi::BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        Gdi::StretchDIBits(
            dc,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            0,
            0,
            width,
            height,
            Some(pixels.as_ptr().cast()),
            &info,
            Gdi::DIB_RGB_COLORS,
            Gdi::SRCCOPY,
        );
    }
}
fn apply(s: &mut State) -> bool {
    let value = ui::value(s.hex);
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 {
        return false;
    }
    let Ok(rgb) = u32::from_str_radix(value, 16) else {
        return false;
    };
    let Ok(alpha) = ui::value(s.alpha).trim().parse::<u16>() else {
        return false;
    };
    if alpha > 100 {
        return false;
    }
    s.color = [
        rgb as u8,
        (rgb >> 8) as u8,
        (rgb >> 16) as u8,
        if alpha == u16::from(s.color[3]) * 100 / 255 {
            s.color[3]
        } else {
            (alpha * 255 / 100) as u8
        },
    ];
    true
}
// All rectangles are in screen coordinates at the current per-monitor DPI.
// Prefer the workspace interior. If a window is too small, stay on its monitor.
fn popup_rect(anchor: RECT, client: RECT, work: RECT, width: i32, height: i32, gap: i32) -> RECT {
    let visible = RECT {
        left: client.left.max(work.left),
        top: client.top.max(work.top),
        right: client.right.min(work.right),
        bottom: client.bottom.min(work.bottom),
    };
    let bounds = if visible.right - visible.left >= width && visible.bottom - visible.top >= height
    {
        visible
    } else {
        work
    };
    let x = (anchor.right - width).clamp(bounds.left, (bounds.right - width).max(bounds.left));
    let below = anchor.bottom + gap;
    let above = anchor.top - gap - height;
    let y = if below + height <= bounds.bottom {
        below
    } else {
        above
    }
    .clamp(bounds.top, (bounds.bottom - height).max(bounds.top));
    RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    }
}

pub(crate) fn open_color(
    owner: HWND,
    anchor: HWND,
    color: [u8; 4],
    opaque: bool,
) -> Result<(), String> {
    let mut rect=RECT::default();unsafe{let _=GetWindowRect(anchor,&mut rect);}
    open_color_at(owner,anchor,rect,color,opaque)
}
pub(crate) fn open_color_at(owner:HWND,anchor:HWND,a:RECT,mut color:[u8;4],opaque:bool)->Result<(),String>{
    if opaque {
        color[3] = 255;
    }
    let active = ACTIVE.with(Cell::get);
    if unsafe { IsWindow(Some(active)) }.as_bool() {
        unsafe {
            SendMessageW(active, WM_CLOSE, None, None);
        }
    }
    // Anchor owned popups to the top-level workspace; keep every surface enabled.
    let root = unsafe { GetAncestor(owner, GA_ROOT) };
    let root_owner = if root.0.is_null() { owner } else { root };
    let previous_focus = unsafe { GetFocus() };
    ui::register(w!("PanzoColorPopover"), Some(procedure))?;
    let dpi = editor_ui::window_dpi(owner);
    let t = EditorTheme::current(dpi);
    let d = |n| t.scale(n);
    let mut client = ui::rect(root_owner);
    let mut origin = windows::Win32::Foundation::POINT::default();
    unsafe {
        let _ = Gdi::ClientToScreen(root_owner, &mut origin);
    }
    client.left += origin.x;
    client.right += origin.x;
    client.top += origin.y;
    client.bottom += origin.y;
    let mut info = Gdi::MONITORINFO {
        cbSize: std::mem::size_of::<Gdi::MONITORINFO>() as u32,
        ..Default::default()
    };
    let monitor = unsafe { Gdi::MonitorFromWindow(anchor, Gdi::MONITOR_DEFAULTTONEAREST) };
    let work = if unsafe { Gdi::GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        info.rcWork
    } else {
        client
    };
    let bounds = popup_rect(a, client, work, d(292), d(478), d(4));
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("PanzoColorPopover"),
            w!("颜色"),
            WS_POPUP | WS_CLIPCHILDREN,
            bounds.left,
            bounds.top,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            Some(root_owner),
            None,
            None,
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    super::install(hwnd);
    let mut state = Box::new(State {
        preview_owner: Some(owner),
        anchor,
        previous_focus,
        syncing: false,
        drag: 0,
        gradient_hue: f64::NAN,
        gradient: Vec::new(),
        color,
        hue: hue(color),
        used: false,
        closing: false,
        hex: HWND::default(),
        alpha: HWND::default(),
        error: HWND::default(),
        recent: super::super::ui_preferences::recent_colors(),
    });
    let result = (|| -> Result<(), String> {
        state.hex = ui::edit(hwnd, "", 10, false)?;
        ui::place(state.hex, d(16), d(410), d(148), d(28));
        state.alpha = ui::edit(hwnd, "", 11, false)?;
        super::number_field(state.alpha, 0, 100, 1);
        if opaque {
            unsafe {
                let _ = EnableWindow(state.alpha, false);
            }
        }
        ui::place(state.alpha, d(174), d(410), d(102), d(28));
        state.error = ui::label(hwnd, "", 12)?;
        ui::place(state.error, d(16), d(442), d(260), d(24));
        Ok(())
    })();
    if let Err(e) = result {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return Err(e);
    }
    sync(&mut state);
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
        ACTIVE.with(|v| v.set(hwnd));
        #[cfg(test)]
        REVIEW_HWND.store(hwnd.0 as usize, std::sync::atomic::Ordering::Release);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        for child in [(*ptr).hex, (*ptr).alpha] {
            let _ = windows::Win32::UI::Shell::SetWindowSubclass(
                child,
                Some(field_proc),
                0x5041_39,
                hwnd.0 as usize,
            );
        }
        let _ = SetFocus(Some((*ptr).hex));
        SetTimer(Some(hwnd), 1, 100, None);
    }
    Ok(())
}
fn close_popup(h: HWND, restore_focus: bool) {
    let ptr = unsafe { GetWindowLongPtrW(h, GWLP_USERDATA) } as *mut State;
    let Some(s) = (unsafe { ptr.as_mut() }) else {
        return;
    };
    if s.closing {
        return;
    }
    s.closing = true;
    // Incomplete HEX input never replaces the last valid colour.
    if s.used {
        let _ = super::super::ui_preferences::remember_color(s.color);
    }
    let owner = s.preview_owner;
    let anchor = s.anchor;
    let color = s.color;
    let focus = s.previous_focus;
    let restore = restore_focus && unsafe { GetForegroundWindow() } == h;
    if let Some(owner) = owner.filter(|o| unsafe { IsWindow(Some(*o)) }.as_bool()) {
        unsafe {
            SendMessageW(
                owner,
                COLOR_CLOSED,
                Some(WPARAM(u32::from_le_bytes(color) as usize)),
                Some(LPARAM(anchor.0 as isize)),
            );
        }
    }
    // Finish the activating mouse event before destroying its previous active window.
    // The colour/undo/history boundary above still occurs before the target click.
    if !restore_focus {
        unsafe { let _=PostMessageW(Some(h),WM_APP+395,WPARAM(0),LPARAM(0)); }
        return;
    }
    unsafe {
        let _ = DestroyWindow(h);
        if restore && IsWindow(Some(focus)).as_bool() {
            let _ = SetFocus(Some(focus));
        }
    }
}
unsafe extern "system" fn field_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    owner: usize,
) -> LRESULT {
    if m == WM_KEYDOWN && matches!(w.0, 13 | 27) {
        unsafe {
            let _ = PostMessageW(Some(HWND(owner as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        return LRESULT(0);
    }
    if m == WM_CHAR && matches!(w.0, 13 | 27) {
        return LRESULT(0);
    }
    unsafe { windows::Win32::UI::Shell::DefSubclassProc(h, m, w, l) }
}

fn palette() -> [[u8; 4]; 8] {
    [
        [48, 64, 236, 255],
        [52, 157, 246, 255],
        [83, 206, 245, 255],
        [115, 184, 92, 255],
        [222, 152, 62, 255],
        [231, 110, 138, 255],
        [243, 243, 245, 255],
        [32, 34, 38, 255],
    ]
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(h, GWLP_USERDATA) } as *mut State;
    if let Some(s) = unsafe { ptr.as_mut() } {
        match m {
            m if m==WM_APP+395 => {unsafe{let _=DestroyWindow(h);}return LRESULT(0);}
            WM_ACTIVATE if w.0 & 0xffff == WA_INACTIVE as usize => {
                close_popup(h, false);
                return LRESULT(0);
            }
            WM_CLOSE => {
                close_popup(h, true);
                return LRESULT(0);
            }
            WM_KEYDOWN if matches!(w.0, 13 | 27) => {
                close_popup(h, true);
                return LRESULT(0);
            }
            WM_TIMER => {
                if s.preview_owner.is_none_or(|o| {
                    !unsafe { IsWindow(Some(o)) }.as_bool()
                        || !unsafe { IsWindowVisible(o) }.as_bool()
                }) {
                    close_popup(h, false);
                }
                return LRESULT(0);
            }
            WM_NCDESTROY => {
                if !s.closing && s.used {
                    let _ = super::super::ui_preferences::remember_color(s.color);
                }
                unsafe {
                    SetWindowLongPtrW(h, GWLP_USERDATA, 0);
                    let _ = KillTimer(Some(h), 1);
                    drop(Box::from_raw(ptr));
                }
                ACTIVE.with(|v| {
                    if v.get() == h {
                        v.set(HWND(std::ptr::null_mut()));
                    }
                });
                #[cfg(test)]
                REVIEW_HWND.store(0, std::sync::atomic::Ordering::Release);
                return LRESULT(0);
            }
            WM_COMMAND => {
                if matches!(w.0 & 0xffff, 10 | 11) && w.0 >> 16 == EN_CHANGE as usize && !s.syncing
                {
                    if apply(s) {
                        s.used = true;
                        s.hue = hue(s.color);
                        publish(s);
                        super::invalidate(h);
                    }
                    return LRESULT(0);
                }
                return LRESULT(0);
            }
            WM_LBUTTONDOWN | WM_MOUSEMOVE => {
                let dpi = editor_ui::window_dpi(h);
                let x = (l.0 as i16 as i32) * 96 / dpi as i32;
                let y = ((l.0 >> 16) as i16 as i32) * 96 / dpi as i32;
                if m == WM_LBUTTONDOWN {
                    s.drag = if (16..276).contains(&x) && (216..342).contains(&y) {
                        1
                    } else if (16..276).contains(&x) && (350..366).contains(&y) {
                        2
                    } else {
                        0
                    };
                    if s.drag != 0 {
                        unsafe {
                            SetCapture(h);
                        }
                    }
                }
                if s.drag == 1 {
                    s.color = hsv(
                        s.hue,
                        f64::from((x - 16).clamp(0, 259)) / 259.,
                        1. - f64::from((y - 216).clamp(0, 125)) / 125.,
                        s.color[3],
                    );
                } else if s.drag == 2 {
                    s.hue = f64::from((x - 16).clamp(0, 259)) / 259. * 359.;
                    let max = *s.color[..3].iter().max().unwrap();
                    let min = *s.color[..3].iter().min().unwrap();
                    let sat = if max == 0 {
                        1.
                    } else {
                        f64::from(max - min) / f64::from(max)
                    };
                    s.color = hsv(s.hue, sat, f64::from(max) / 255., s.color[3]);
                } else if m == WM_LBUTTONDOWN && (16..276).contains(&x) {
                    let index = ((x - 16) / 33) as usize;
                    let selected = if (168..192).contains(&y) {
                        palette().get(index).copied()
                    } else if (64..88).contains(&y) || (96..120).contains(&y) {
                        s.recent.get(index + if y >= 96 { 8 } else { 0 }).copied()
                    } else {
                        None
                    };
                    let Some(color) = selected else {
                        return LRESULT(0);
                    };
                    s.color = [color[0], color[1], color[2], s.color[3]];
                    s.hue = hue(color);
                } else {
                    return LRESULT(0);
                }
                s.used = true;
                sync(s);
                super::invalidate(h);
                return LRESULT(0);
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED => {
                s.drag = 0;
                if m == WM_LBUTTONUP {
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                }
                return LRESULT(0);
            }
            WM_PAINT => {
                let mut ps = Gdi::PAINTSTRUCT::default();
                let dc = unsafe { Gdi::BeginPaint(h, &raw mut ps) };
                let t = EditorTheme::current(editor_ui::window_dpi(h));
                let d = |n| t.scale(n);
                let r = ui::rect(h);
                let frame = super::BufferedFrame::new(dc, r);
                let dc = frame.dc;
                editor_ui::fill_rect(dc, &r, t.palette.panel);
                text(
                    dc,
                    rect(d(16), d(4), d(260), d(28)),
                    "颜色",
                    t.palette.text,
                    t.dpi,
                    false,
                );
                if s.gradient_hue != s.hue {
                    s.gradient.clear();
                    for y in 0..126 {
                        for x in 0..260 {
                            s.gradient.extend_from_slice(&hsv(
                                s.hue,
                                f64::from(x) / 259.,
                                1. - f64::from(y) / 125.,
                                255,
                            ));
                        }
                    }
                    s.gradient_hue = s.hue;
                }
                paint_gradient(
                    dc,
                    rect(d(16), d(216), d(260), d(126)),
                    &s.gradient,
                    260,
                    126,
                );
                static HUES: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
                let hues = HUES.get_or_init(|| {
                    (0..260)
                        .flat_map(|x| hsv(f64::from(x) / 259. * 359., 1., 1., 255))
                        .collect()
                });
                paint_gradient(dc, rect(d(16), d(350), d(260), d(16)), hues, 260, 1);
                let selected = editor_ui::rgb(s.color[2], s.color[1], s.color[0]);
                editor_ui::rounded_surface(
                    dc,
                    rect(d(248), d(8), d(28), d(20)),
                    selected,
                    selected,
                    d(3),
                );
                let max = s.color[..3].iter().copied().max().unwrap_or(0);
                let min = s.color[..3].iter().copied().min().unwrap_or(0);
                let sat = if max == 0 {
                    0.
                } else {
                    f64::from(max - min) / f64::from(max)
                };
                let cx = d(16 + (sat * 259.) as i32);
                let cy = d(216 + ((1. - f64::from(max) / 255.) * 125.) as i32);
                unsafe {
                    let pen = Gdi::CreatePen(Gdi::PS_SOLID, d(2), editor_ui::rgb(255, 255, 255));
                    let old = Gdi::SelectObject(dc, Gdi::HGDIOBJ(pen.0));
                    let brush = Gdi::SelectObject(dc, Gdi::GetStockObject(Gdi::NULL_BRUSH));
                    let _ = Gdi::Ellipse(dc, cx - d(4), cy - d(4), cx + d(4), cy + d(4));
                    Gdi::SelectObject(dc, brush);
                    Gdi::SelectObject(dc, old);
                    let _ = Gdi::DeleteObject(Gdi::HGDIOBJ(pen.0));
                }
                editor_ui::fill_rect(
                    dc,
                    &rect(d(16 + (s.hue / 359. * 258.) as i32), d(348), d(2), d(20)),
                    editor_ui::rgb(255, 255, 255),
                );
                for (y, label) in [(36, "最近使用"), (140, "标准色")] {
                    text(
                        dc,
                        rect(d(16), d(y), d(260), d(20)),
                        label,
                        t.palette.text_secondary,
                        t.dpi,
                        false,
                    );
                }
                if s.recent.is_empty() {
                    text(
                        dc,
                        rect(d(16), d(64), d(260), d(24)),
                        "暂无最近颜色",
                        t.palette.text_muted,
                        t.dpi,
                        false,
                    );
                }
                for (i, c) in palette().iter().enumerate() {
                    let x = 16 + i as i32 * 33;
                    editor_ui::rounded_surface(
                        dc,
                        rect(d(x), d(168), d(24), d(24)),
                        editor_ui::rgb(c[2], c[1], c[0]),
                        editor_ui::rgb(c[2], c[1], c[0]),
                        d(3),
                    );
                }
                text(
                    dc,
                    rect(d(16), d(382), d(148), d(24)),
                    "HEX",
                    t.palette.text_secondary,
                    t.dpi,
                    false,
                );
                text(
                    dc,
                    rect(d(174), d(382), d(102), d(24)),
                    "不透明度 %",
                    t.palette.text_secondary,
                    t.dpi,
                    false,
                );
                for (i, c) in s.recent.iter().take(16).enumerate() {
                    let x = 16 + (i % 8) as i32 * 33;
                    editor_ui::fill_rect(
                        dc,
                        &rect(d(x), d(64 + (i / 8) as i32 * 32), d(24), d(24)),
                        editor_ui::rgb(c[2], c[1], c[0]),
                    );
                }
                drop(frame);
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
mod tests {
    use super::*;
    #[test]
    fn popup_position_stays_in_workspace_on_primary_secondary_and_negative_monitors() {
        for (monitor_x, monitor_y, dpi) in [
            (0, 0, 96),
            (2560, 0, 96),
            (-2560, -200, 144),
            (3840, 300, 192),
        ] {
            let scale = |v| v * dpi / 96;
            let work = rect(monitor_x, monitor_y, scale(2560), scale(1440));
            let client = rect(
                monitor_x + scale(100),
                monitor_y + scale(100),
                scale(1100),
                scale(800),
            );
            for anchor_y in [150, 730] {
                let anchor = rect(
                    client.right - scale(140),
                    client.top + scale(anchor_y),
                    scale(34),
                    scale(26),
                );
                let popup = popup_rect(anchor, client, work, scale(292), scale(478), scale(4));
                assert!(popup.left >= client.left && popup.right <= client.right);
                assert!(popup.top >= client.top && popup.bottom <= client.bottom);
                assert_eq!(popup.right, anchor.right);
                assert_eq!(popup.bottom - popup.top, scale(478));
            }
        }
        let small = rect(-1100, 100, 200, 200);
        let work = rect(-1920, 0, 1920, 1080);
        let popup = popup_rect(rect(-950, 200, 34, 26), small, work, 292, 478, 4);
        assert!(popup.left >= work.left && popup.right <= work.right);
        assert!(popup.top >= work.top && popup.bottom <= work.bottom);
    }

    #[test]
    fn hue_roundtrip_preserves_bgra_channels_and_alpha() {
        for (degrees, bgra) in [
            (0., [0, 0, 255, 123]),
            (60., [0, 255, 255, 123]),
            (120., [0, 255, 0, 123]),
            (180., [255, 255, 0, 123]),
            (240., [255, 0, 0, 123]),
            (300., [255, 0, 255, 123]),
        ] {
            assert_eq!(hsv(degrees, 1., 1., 123), bgra);
            assert!((hue(bgra) - degrees).abs() < 0.001);
        }
        assert_eq!(hsv(180., 0., 1., 0), [255, 255, 255, 0]);
        assert_eq!(hsv(180., 1., 0., 255), [0, 0, 0, 255]);
    }
}
