#![allow(clippy::many_single_char_names)] // Win32 callback parameters and local rectangle coordinates.
//! Shared native desktop surfaces. Standard HWND controls retain focus, keyboard and UIA behavior.
use super::{
    editor_ui::{self, EditorTheme, IconKind, TextStyle},
    ui_preferences::ThemeMode,
};
use std::{cell::RefCell, collections::HashMap};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{
        self, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE, DT_VCENTER, HBRUSH, HDC,
        TRANSPARENT,
    },
    UI::{
        Controls::{
            DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED, ODT_BUTTON, WM_MOUSELEAVE,
        },
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            BS_OWNERDRAW, CB_SETCURSEL, EnumChildWindows, GWL_STYLE, GetWindowLongPtrW,
            PostMessageW, SendMessageW, SetWindowLongPtrW, WM_CTLCOLORBTN, WM_CTLCOLOREDIT,
            WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_DRAWITEM, WM_ENABLE, WM_ERASEBKGND,
            WM_KILLFOCUS, WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT, WM_PRINTCLIENT, WM_SETFOCUS,
            WM_SETFONT, WM_SIZE,
        },
    },
};
const SURFACE: usize = 0x5041_5a10;
const BUTTON: usize = 0x5041_5a11;
thread_local! { static CANVASES: RefCell<std::collections::HashSet<isize>> = RefCell::new(std::collections::HashSet::new()); }
pub(crate) fn install_canvas(hwnd: HWND) {
    if CANVASES.with(|v| v.borrow_mut().insert(hwnd.0 as isize)) {
        install(hwnd);
    }
}
#[allow(clippy::struct_excessive_bools)] // Independent Win32 visual flags can coexist.
#[derive(Clone, Copy, Default)]
struct ButtonStyle {
    selected: bool,
    primary: bool,
    flat: bool,
    hover: bool,
    icon: Option<IconKind>,
    icon_only: bool,
    path: bool,
    tab: bool,
    choice: Option<ChoiceVisual>,
}
#[derive(Clone, Copy)]
pub(crate) enum ChoiceVisual {
    Layout(u8),
    Swatch([u8; 4]),
    Toggle(bool),
    ColorDot([u8; 4]),
    AddCell,
}
pub(crate) fn choice(hwnd: HWND, visual: ChoiceVisual) {
    BUTTONS.with(|b| b.borrow_mut().entry(hwnd.0 as isize).or_default().choice = Some(visual));
    invalidate(hwnd);
}
thread_local! { static BUTTONS:RefCell<HashMap<isize,ButtonStyle>>=RefCell::new(HashMap::new()); static THUMBS:RefCell<HashMap<isize,std::rc::Rc<super::capture_tools::Raster>>>=RefCell::new(HashMap::new()); }
pub(crate) fn install(hwnd: HWND) {
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(surface_proc), SURFACE, 0);
    }
}
pub(crate) fn button(hwnd: HWND) {
    let first = BUTTONS.with(|b| {
        let mut buttons = b.borrow_mut();
        if buttons.contains_key(&(hwnd.0 as isize)) {
            false
        } else {
            buttons.insert(hwnd.0 as isize, ButtonStyle::default());
            true
        }
    });
    if !first {
        return;
    }
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(hwnd, GWL_STYLE, (style & !0xf) | BS_OWNERDRAW as isize);
        let _ = SetWindowSubclass(hwnd, Some(button_proc), BUTTON, 0);
    }
    tooltip(hwnd);
}
pub(crate) fn style(
    hwnd: HWND,
    icon: Option<IconKind>,
    flat: bool,
    primary: bool,
    icon_only: bool,
) {
    let changed = BUTTONS.with(|b| {
        let mut b = b.borrow_mut();
        let s = b.entry(hwnd.0 as isize).or_default();
        let changed = (s.icon, s.flat, s.primary, s.icon_only) != (icon, flat, primary, icon_only);
        s.icon = icon;
        s.flat = flat;
        s.primary = primary;
        s.icon_only = icon_only;
        changed
    });
    if changed {
        tooltip(hwnd);
        invalidate(hwnd);
    }
}

pub(crate) fn workspace_tab(hwnd: HWND) {
    BUTTONS.with(|v| v.borrow_mut().entry(hwnd.0 as isize).or_default().tab = true);
    style(hwnd, None, true, false, false);
}
pub(crate) fn path_label(hwnd: HWND) {
    BUTTONS.with(|v| {
        let mut v = v.borrow_mut();
        let style = v.entry(hwnd.0 as isize).or_default();
        style.path = true;
        style.flat = true;
    });
    tooltip(hwnd);
    invalidate(hwnd);
}
pub(crate) fn selected(hwnd: HWND, value: bool) {
    let changed = BUTTONS.with(|b| {
        let mut b = b.borrow_mut();
        let s = b.entry(hwnd.0 as isize).or_default();
        let changed = s.selected != value;
        s.selected = value;
        changed
    });
    if changed {
        invalidate(hwnd);
    }
}
pub(crate) fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = Gdi::InvalidateRect(Some(hwnd), None, false);
    }
}
pub(crate) fn canvas(theme: EditorTheme) -> windows::Win32::Foundation::COLORREF {
    if theme.mode == ThemeMode::Dark {
        editor_ui::rgb(30, 32, 35)
    } else {
        editor_ui::rgb(232, 234, 237)
    }
}
pub(crate) fn rect(x: i32, y: i32, w: i32, h: i32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}
pub(crate) fn text(
    dc: HDC,
    rect: RECT,
    label: &str,
    ink: windows::Win32::Foundation::COLORREF,
    dpi: u32,
    center: bool,
) {
    text_style(dc, rect, label, ink, dpi, center, TextStyle::Body);
}

pub(crate) fn text_style(
    dc: HDC,
    mut rect: RECT,
    label: &str,
    ink: windows::Win32::Foundation::COLORREF,
    dpi: u32,
    center: bool,
    style: TextStyle,
) {
    let mut value = label.encode_utf16().collect::<Vec<_>>();
    unsafe {
        let old = Gdi::SelectObject(dc, editor_ui::font_handle(style, dpi));
        let mode = Gdi::SetBkMode(dc, TRANSPARENT);
        let color = Gdi::SetTextColor(dc, ink);
        Gdi::DrawTextW(
            dc,
            &mut value,
            &raw mut rect,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | if center { DT_CENTER } else { DT_LEFT },
        );
        Gdi::SetTextColor(dc, color);
        Gdi::SetBkMode(
            dc,
            Gdi::BACKGROUND_MODE(u32::try_from(mode).unwrap_or_default()),
        );
        Gdi::SelectObject(dc, old);
    }
}
pub(crate) fn line(dc: HDC, r: RECT, theme: EditorTheme) {
    editor_ui::fill_rect(dc, &r, theme.palette.border);
}
fn brush(field: bool) -> HBRUSH {
    static BRUSHES: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<u32, usize>>> =
        std::sync::OnceLock::new();
    let t = EditorTheme::current(96);
    let color = if field {
        t.palette.raised
    } else {
        t.palette.panel
    };
    let mut cache = BRUSHES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    HBRUSH(
        *cache
            .entry(color.0)
            .or_insert_with(|| unsafe { Gdi::CreateSolidBrush(color) }.0 as usize)
            as *mut _,
    )
}

fn paint_button_content(item: &DRAWITEMSTRUCT) -> bool {
    let Some(s) = BUTTONS.with(|b| b.borrow().get(&(item.hwndItem.0 as isize)).copied()) else {
        return false;
    };
    let dpi = editor_ui::window_dpi(item.hwndItem);
    let t = EditorTheme::current(dpi);
    let d = |v| t.scale(v);
    let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
    let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
    let fill = if matches!(s.choice, Some(ChoiceVisual::ColorDot(_))) {
        t.palette.panel
    } else if s.primary && !disabled {
        if pressed {
            t.palette.accent_pressed
        } else {
            super::ui_motion::blend(
                t.palette.accent,
                t.palette.accent_hover,
                super::ui_motion::amount(item.hwndItem, s.hover),
            )
        }
    } else if pressed {
        t.palette.pressed
    } else if s.selected && !s.tab {
        if matches!(s.icon, Some(IconKind::Prototype(_))) {
            t.palette.hover
        } else {
            t.palette.divider
        }
    } else {
        super::ui_motion::blend(
            if s.flat {
                t.palette.panel
            } else {
                t.palette.raised
            },
            t.palette.hover,
            if disabled {
                0.
            } else {
                super::ui_motion::amount(item.hwndItem, s.hover)
            },
        )
    };
    let prototype_icon = matches!(s.icon, Some(IconKind::Prototype(_)));
    let ink = if disabled {
        t.palette.text_muted
    } else if s.primary {
        t.palette.window
    } else if prototype_icon && s.icon_only {
        if s.selected {
            t.palette.accent
        } else {
            t.palette.text_secondary
        }
    } else {
        t.palette.text
    };
    let edge = if s.selected && super::system_preferences::high_contrast() {
        t.palette.text
    } else {
        fill
    };
    editor_ui::rounded_surface(item.hDC, item.rcItem, fill, edge, d(3));
    if s.tab && s.selected {
        editor_ui::fill_rect(
            item.hDC,
            &rect(
                item.rcItem.left + d(17),
                item.rcItem.bottom - d(2),
                item.rcItem.right - item.rcItem.left - d(34),
                d(2),
            ),
            t.palette.accent,
        );
    }
    if let Some(visual) = s.choice {
        let r = item.rcItem;
        let center = (r.left + r.right) / 2;
        match visual {
            ChoiceVisual::Layout(count) => {
                let (cols, rows) = match count {
                    2 => (2, 1),
                    3 => (3, 1),
                    4 => (2, 2),
                    _ => (3, 3),
                };
                let size = d(22);
                let gap = d(3);
                let cw = (size - (cols - 1) * gap) / cols;
                let ch = (size - (rows - 1) * gap) / rows;
                for row in 0..rows {
                    for col in 0..cols {
                        editor_ui::fill_rect(
                            item.hDC,
                            &rect(
                                center - size / 2 + col * (cw + gap),
                                d(9) + row * (ch + gap),
                                cw,
                                ch,
                            ),
                            if s.selected {
                                t.palette.accent
                            } else {
                                t.palette.text_muted
                            },
                        );
                    }
                }
                text(
                    item.hDC,
                    rect(r.left, d(34), r.right - r.left, d(22)),
                    &super::workspace_ui::value(item.hwndItem),
                    ink,
                    dpi,
                    true,
                );
                if s.selected {
                    editor_ui::draw_icon(
                        item.hDC,
                        rect(r.right - d(15), d(4), d(11), d(11)),
                        IconKind::Check,
                        t.palette.accent,
                    );
                }
            }
            ChoiceVisual::Toggle(on) => {
                let left = (r.right - d(32)) / 2;
                let top = (r.bottom - d(18)) / 2;
                editor_ui::rounded_surface(
                    item.hDC,
                    rect(left, top, d(32), d(18)),
                    if on {
                        t.palette.accent
                    } else {
                        t.palette.hover
                    },
                    fill,
                    d(9),
                );
                editor_ui::rounded_surface(
                    item.hDC,
                    rect(left + d(if on { 16 } else { 3 }), top + d(3), d(12), d(12)),
                    if on {
                        t.palette.window
                    } else {
                        t.palette.text_secondary
                    },
                    if on {
                        t.palette.window
                    } else {
                        t.palette.text_secondary
                    },
                    d(6),
                );
            }
            ChoiceVisual::ColorDot(color) => {
                let size = d(21);
                let circle = rect((r.right - size) / 2, (r.bottom - size) / 2, size, size);
                let ink = editor_ui::rgb(color[2], color[1], color[0]);
                let ring = s.selected || item.itemState.0 & ODS_FOCUS.0 != 0;
                let edge = if ring { t.palette.text } else { ink };
                editor_ui::rounded_surface(item.hDC, circle, edge, edge, size / 2);
                if ring {
                    let inset = d(2);
                    let inner = rect(
                        circle.left + inset,
                        circle.top + inset,
                        size - 2 * inset,
                        size - 2 * inset,
                    );
                    editor_ui::rounded_surface(item.hDC, inner, ink, ink, (size - 2 * inset) / 2);
                }
            }
            ChoiceVisual::AddCell => {
                for x in (r.left..r.right).step_by(d(6).max(1) as usize) {
                    for y in [r.top, r.bottom - 1] {
                        editor_ui::fill_rect(
                            item.hDC,
                            &rect(x, y, d(3).min(r.right - x), 1),
                            t.palette.border,
                        );
                    }
                }
                for y in (r.top..r.bottom).step_by(d(6).max(1) as usize) {
                    for x in [r.left, r.right - 1] {
                        editor_ui::fill_rect(
                            item.hDC,
                            &rect(x, y, 1, d(3).min(r.bottom - y)),
                            t.palette.border,
                        );
                    }
                }
                editor_ui::draw_icon(
                    item.hDC,
                    rect((r.right - d(16)) / 2, (r.bottom - d(16)) / 2, d(16), d(16)),
                    IconKind::Add,
                    t.palette.text_secondary,
                );
            }
            ChoiceVisual::Swatch(color) => {
                if r.right <= d(40) {
                    editor_ui::rounded_surface(
                        item.hDC,
                        rect(
                            d(3),
                            d(3),
                            (r.right - d(6)).max(1),
                            (r.bottom - d(6)).max(1),
                        ),
                        editor_ui::rgb(color[2], color[1], color[0]),
                        t.palette.border,
                        d(2),
                    );
                    return true;
                }
                editor_ui::rounded_surface(
                    item.hDC,
                    rect(d(8), d(7), d(20), (r.bottom - d(14)).max(1)),
                    editor_ui::rgb(color[2], color[1], color[0]),
                    t.palette.border,
                    d(3),
                );
                text(
                    item.hDC,
                    rect(d(38), 0, (r.right - d(46)).max(1), r.bottom),
                    &format!("#{:02X}{:02X}{:02X}", color[2], color[1], color[0]),
                    ink,
                    dpi,
                    false,
                );
            }
        }
        if (s.selected && matches!(visual, ChoiceVisual::Layout(_)))
            || (item.itemState.0 & ODS_FOCUS.0 != 0 && !matches!(visual, ChoiceVisual::ColorDot(_)))
        {
            editor_ui::selection_outline(item.hDC, r, t.palette.accent);
        }
        return true;
    }
    let mut r = item.rcItem;
    r.left += d(9);
    r.right -= d(9);
    if let Some(icon) = s.icon {
        let icon_size = d(if prototype_icon { 17 } else { 16 });
        let x = if s.icon_only {
            (item.rcItem.right - icon_size) / 2
        } else {
            r.left
        };
        editor_ui::draw_icon(
            item.hDC,
            rect(
                x,
                (item.rcItem.bottom - icon_size) / 2,
                icon_size,
                icon_size,
            ),
            icon,
            ink,
        );
        r.left += d(23);
    }
    if let Some(image) = THUMBS.with(|v| v.borrow().get(&(item.hwndItem.0 as isize)).cloned()) {
        let max_height = (item.rcItem.bottom - d(10)).max(1);
        let max_width = d(64);
        let scale = (f64::from(max_width) / f64::from(image.width))
            .min(f64::from(max_height) / f64::from(image.height));
        let iw = (f64::from(image.width) * scale) as i32;
        let ih = (f64::from(image.height) * scale) as i32;
        super::capture_tools::draw_raster(
            item.hDC,
            &image,
            rect(
                d(5) + (max_width - iw) / 2,
                (item.rcItem.bottom - ih) / 2,
                iw,
                ih,
            ),
        );
        r.left = d(77);
    }
    if !s.icon_only && THUMBS.with(|v| v.borrow().contains_key(&(item.hwndItem.0 as isize))) {
        let value = super::workspace_ui::value(item.hwndItem);
        let lines = value.lines().collect::<Vec<_>>();
        text(
            item.hDC,
            rect(r.left, d(5), r.right - r.left, d(23)),
            lines.first().copied().unwrap_or(""),
            ink,
            dpi,
            false,
        );
        if let Some(sub) = lines.get(1) {
            text(
                item.hDC,
                rect(r.left, d(27), r.right - r.left, d(19)),
                sub,
                t.palette.text_muted,
                dpi,
                false,
            );
        }
    } else if s.path {
        let mut value = super::workspace_ui::value(item.hwndItem)
            .encode_utf16()
            .collect::<Vec<_>>();
        unsafe {
            let old = Gdi::SelectObject(item.hDC, editor_ui::font_handle(TextStyle::Body, dpi));
            let color = Gdi::SetTextColor(item.hDC, ink);
            let mode = Gdi::SetBkMode(item.hDC, TRANSPARENT);
            Gdi::DrawTextW(
                item.hDC,
                &mut value,
                &raw mut r,
                DT_SINGLELINE | DT_VCENTER | Gdi::DT_PATH_ELLIPSIS | Gdi::DT_NOPREFIX,
            );
            Gdi::SetTextColor(item.hDC, color);
            Gdi::SetBkMode(item.hDC, Gdi::BACKGROUND_MODE(mode as u32));
            Gdi::SelectObject(item.hDC, old);
        }
    } else if !s.icon_only {
        text(
            item.hDC,
            r,
            &super::workspace_ui::value(item.hwndItem),
            ink,
            dpi,
            s.icon.is_none(),
        );
    }
    // Native keyboard focus is retained; buttons never draw a focus underline.
    true
}
unsafe extern "system" fn surface_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    if let Some(result) = super::app_menu::window_message(m, l) {
        return result;
    }
    match m {
        WM_DRAWITEM if l.0 != 0 => {
            let item = unsafe { &*(l.0 as *const DRAWITEMSTRUCT) };
            if item.CtlType == windows::Win32::UI::Controls::ODT_COMBOBOX {
                paint_combo_item(item);
                return LRESULT(1);
            }
            if item.CtlType == ODT_BUTTON && paint_button(item) {
                return LRESULT(1);
            }
        }
        windows::Win32::UI::WindowsAndMessaging::WM_MEASUREITEM if l.0 != 0 => {
            let item =
                unsafe { &mut *(l.0 as *mut windows::Win32::UI::Controls::MEASUREITEMSTRUCT) };
            if item.CtlType == windows::Win32::UI::Controls::ODT_COMBOBOX {
                item.itemHeight = EditorTheme::current(editor_ui::window_dpi(h)).scale(30) as u32;
                return LRESULT(1);
            }
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_CTLCOLORBTN => {
            let t = EditorTheme::current(editor_ui::window_dpi(h));
            let dc = HDC(w.0 as *mut _);
            let field = (matches!(m, WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX)
                || field_controls::is_edit(HWND(l.0 as *mut _)))
                && (m != WM_CTLCOLORLISTBOX
                    || !["PanzoProjectBin", "PanzoPluginManager"].contains(&class(h).as_str()));
            unsafe {
                Gdi::SetTextColor(dc, t.palette.text);
                Gdi::SetBkColor(
                    dc,
                    if field {
                        t.palette.raised
                    } else {
                        t.palette.panel
                    },
                );
            }
            return LRESULT(brush(field).0 as isize);
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT if flat_container(h) => {
            let mut ps = Gdi::PAINTSTRUCT::default();
            let target = unsafe { Gdi::BeginPaint(h, &mut ps) };
            let frame = BufferedFrame::new(target, super::workspace_ui::rect(h));
            editor_ui::fill_rect(
                frame.dc,
                &super::workspace_ui::rect(h),
                EditorTheme::current(editor_ui::window_dpi(h)).palette.panel,
            );
            if super::workspace::root().is_some_and(|root| {
                unsafe { windows::Win32::UI::WindowsAndMessaging::GetParent(h) }.ok() == Some(root)
            }) {
                let r = super::workspace_ui::rect(h);
                editor_ui::fill_rect(
                    frame.dc,
                    &windows::Win32::Foundation::RECT { right: 1, ..r },
                    EditorTheme::current(editor_ui::window_dpi(h))
                        .palette
                        .divider,
                );
            }
            drop(frame);
            unsafe {
                let _ = Gdi::EndPaint(h, &ps);
            }
            return LRESULT(0);
        }
        WM_NCDESTROY => unsafe {
            CANVASES.with(|v| {
                v.borrow_mut().remove(&(h.0 as isize));
            });
            super::workspace_ui::forget(h);
            let _ = RemoveWindowSubclass(h, Some(surface_proc), id);
        },
        _ => {}
    }
    unsafe { DefSubclassProc(h, m, w, l) }
}
unsafe extern "system" fn button_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    };
    match m {
        windows::Win32::UI::WindowsAndMessaging::WM_SETTEXT => {
            let result = unsafe { DefSubclassProc(h, m, w, l) };
            if BUTTONS.with(|v| {
                v.borrow()
                    .get(&(h.0 as isize))
                    .is_some_and(|v| v.path || v.icon_only)
            }) {
                tooltip(h);
            }
            invalidate(h);
            return result;
        }
        WM_MOUSEMOVE => {
            let changed = BUTTONS.with(|b| {
                let mut b = b.borrow_mut();
                if let Some(s) = b.get_mut(&(h.0 as isize)) {
                    let c = !s.hover;
                    s.hover = true;
                    c
                } else {
                    false
                }
            });
            if changed {
                super::ui_motion::hover(h, true);
                tooltip(h);
                unsafe { windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(h), 0x50415a81, 450, None); }
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: h,
                    ..Default::default()
                };
                unsafe {
                    let _ = TrackMouseEvent(&raw mut track);
                }
                invalidate(h);
            }
        }
        windows::Win32::UI::WindowsAndMessaging::WM_TIMER if w.0 == 0x50415a81 => {
            unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(h), 0x50415a81); }
            if BUTTONS.with(|b| b.borrow().get(&(h.0 as isize)).is_some_and(|s| s.hover)) {
                super::hover_tip::show(h, super::workspace_ui::rect(h), &super::workspace_ui::value(h));
            }
            return LRESULT(0);
        }
        windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDOWN | windows::Win32::UI::WindowsAndMessaging::WM_KEYDOWN => {
            unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(h), 0x50415a81); }
            super::hover_tip::hide(h);
        }
        WM_MOUSELEAVE => {
            unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(h), 0x50415a81); }
            super::hover_tip::hide(h);
            super::ui_motion::hover(h, false);
            BUTTONS.with(|b| {
                if let Some(s) = b.borrow_mut().get_mut(&(h.0 as isize)) {
                    s.hover = false;
                }
            });
            invalidate(h);
        }
        WM_NCDESTROY => {
            super::ui_motion::forget(h);
            THUMBS.with(|v| {
                v.borrow_mut().remove(&(h.0 as isize));
            });
            BUTTONS.with(|b| {
                b.borrow_mut().remove(&(h.0 as isize));
            });
            unsafe {
                let _ = RemoveWindowSubclass(h, Some(button_proc), id);
            }
        }
        _ => {}
    }
    unsafe { DefSubclassProc(h, m, w, l) }
}
pub(crate) fn refresh(root: HWND) {
    editor_ui::apply_window_chrome(root);
    unsafe {
        let _ = EnumChildWindows(Some(root), Some(refresh_child), LPARAM(0));
        let _ = Gdi::RedrawWindow(
            Some(root),
            None,
            None,
            Gdi::RDW_INVALIDATE | Gdi::RDW_ERASE | Gdi::RDW_ALLCHILDREN | Gdi::RDW_FRAME,
        );
    }
}
unsafe extern "system" fn refresh_child(h: HWND, _: LPARAM) -> windows::core::BOOL {
    unsafe {
        SendMessageW(
            h,
            WM_SETFONT,
            Some(WPARAM(
                editor_ui::font_handle(TextStyle::Body, editor_ui::window_dpi(h)).0 as usize,
            )),
            Some(LPARAM(1)),
        );
        let _ = PostMessageW(Some(h), WM_SIZE, WPARAM(0), LPARAM(0));
    }
    windows::core::BOOL(1)
}

// Theme both the closed face and the native dropdown while preserving keyboard selection.
thread_local! { static COMBOS: RefCell<std::collections::HashSet<isize>> = RefCell::new(std::collections::HashSet::new()); }
pub(crate) fn combo(hwnd: HWND) {
    if !COMBOS.with(|v| v.borrow_mut().insert(hwnd.0 as isize)) {
        return;
    }
    install(hwnd);
    field_controls::dropdown(hwnd);
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(combo_proc), 0x5041_5a12, 0);
    }
    invalidate(hwnd);
}
unsafe extern "system" fn combo_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    if m == WM_ERASEBKGND {
        return LRESULT(1);
    }
    if m == WM_PAINT || m == WM_PRINTCLIENT {
        let mut ps = Gdi::PAINTSTRUCT::default();
        let target = if m == WM_PAINT {
            unsafe { Gdi::BeginPaint(h, &mut ps) }
        } else {
            HDC(w.0 as *mut _)
        };
        let r = super::workspace_ui::rect(h);
        let frame = BufferedFrame::new(target, r);
        let dc = frame.dc;
        let t = EditorTheme::current(editor_ui::window_dpi(h));
        let d = |n| t.scale(n);
        editor_ui::fill_rect(dc, &r, t.palette.raised);
        let icon = combo_glyph(h);
        let offset = if icon.is_some() { d(30) } else { d(8) };
        if let Some(icon) = icon {
            editor_ui::draw_icon(
                dc,
                rect(d(8), (r.bottom - d(14)) / 2, d(14), d(14)),
                icon,
                t.palette.text_secondary,
            );
        }
        if !editable_combo(h) {
            text(
                dc,
                rect(offset, 0, r.right - offset - d(27), r.bottom),
                &super::workspace_ui::value(h),
                t.palette.text,
                t.dpi,
                false,
            );
        }
        editor_ui::draw_icon(
            dc,
            rect(r.right - d(22), (r.bottom - d(14)) / 2, d(14), d(14)),
            IconKind::ChevronDown,
            t.palette.text_secondary,
        );
        drop(frame);
        if m == WM_PAINT {
            unsafe {
                let _ = Gdi::EndPaint(h, &ps);
            }
        }
        return LRESULT(0);
    }
    if m == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(h, Some(combo_proc), id);
        }
    }
    let result = unsafe { DefSubclassProc(h, m, w, l) };
    if m == WM_SIZE || m == WM_SETFONT {
        layout_combo_edit(h);
    }
    if m == windows::Win32::UI::WindowsAndMessaging::CB_SHOWDROPDOWN {
        field_controls::dropdown(h);
    }
    if m == WM_NCDESTROY {
        COMBOS.with(|v| {
            v.borrow_mut().remove(&(h.0 as isize));
        });
        COMBO_ICONS.with(|v| {
            v.borrow_mut().remove(&(h.0 as isize));
        });
        super::workspace_ui::forget(h);
    }
    if matches!(m, WM_SETFOCUS | WM_KILLFOCUS | CB_SETCURSEL | WM_ENABLE) {
        invalidate(h);
    }
    result
}

// All controls use the same owned popup as the painted workbench commands.
pub(crate) fn tooltip(button: HWND) {
    super::hover_tip::refresh_label(button);
}

// Composited native frames require opaque alpha. Ordinary GDI text leaves alpha
// unset; painting it directly can make flat button labels disappear on activation.
// Do not call GDI/UxTheme from a TLS destructor: process teardown may already
// have dismantled their thread state. The host owns a session across its message
// loop; standalone controls use shorter nested sessions that end with each frame.
thread_local! { static PAINT_USERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
pub(crate) fn has_paint_session() -> bool {
    PAINT_USERS.with(|users| users.get() != 0)
}
pub(crate) struct PaintSession {
    active: bool,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl PaintSession {
    pub(crate) fn new() -> Self {
        let active = PAINT_USERS.with(|users| {
            if users.get() == 0
                && unsafe { windows::Win32::UI::Controls::BufferedPaintInit() }.is_err()
            {
                return false;
            }
            users.set(users.get() + 1);
            true
        });
        Self {
            active,
            _thread: std::marker::PhantomData,
        }
    }
}
impl Drop for PaintSession {
    fn drop(&mut self) {
        if self.active {
            PAINT_USERS.with(|users| {
                users.set(users.get() - 1);
                if users.get() == 0 {
                    super::editor_ui::clear_alpha_cache();
                    unsafe {
                        let _ = windows::Win32::UI::Controls::BufferedPaintUnInit();
                    }
                }
            });
        }
    }
}
pub(crate) fn paint_button(item: &DRAWITEMSTRUCT) -> bool {
    // Each owner-drawn control gets a fully initialised, opaque local frame.
    // Avoid inheriting native theme-buffer contents or coordinates on partial
    // redraw/activation. Publish only after all background, icon and text paint.
    let _runtime = PaintSession::new();
    let width = (item.rcItem.right - item.rcItem.left).max(1) as u32;
    let height = (item.rcItem.bottom - item.rcItem.top).max(1) as u32;
    let Ok(frame) = super::capture_tools::Surface::new(width, height) else {
        return paint_button_content(item);
    };
    let mut buffered = *item;
    buffered.hDC = frame.dc;
    buffered.rcItem = rect(0, 0, width as i32, height as i32);
    editor_ui::fill_rect(
        frame.dc,
        &buffered.rcItem,
        EditorTheme::current(editor_ui::window_dpi(item.hwndItem))
            .palette
            .panel,
    );
    let painted = paint_button_content(&buffered);
    if painted {
        frame.opaque_rows(width, height);
        unsafe {
            let _ = Gdi::BitBlt(
                item.hDC,
                item.rcItem.left,
                item.rcItem.top,
                width as i32,
                height as i32,
                Some(frame.dc),
                0,
                0,
                Gdi::SRCCOPY,
            );
        }
    }
    painted
}

#[path = "field_controls.rs"]
mod field_controls;
pub(crate) use field_controls::{decimal_field, edit, list, number_field, panel_scroll_info, scroll_panel};
#[path = "color_popup.rs"]
mod color_popup;
pub(crate) use color_popup::{COLOR_PREVIEW, COLOR_CLOSED, open_color, open_color_at};

fn class(h: HWND) -> String {
    let mut name = [0u16; 64];
    let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassNameW(h, &mut name) };
    String::from_utf16_lossy(&name[..n.max(0) as usize])
}
fn flat_container(h: HWND) -> bool {
    let mut name = [0u16; 64];
    let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassNameW(h, &mut name) };
    matches!(
        String::from_utf16_lossy(&name[..n.max(0) as usize]).as_str(),
        "PanzoWorkspaceForm"
            | "PanzoSettingsBody"
            | "PanzoPathField"
            | "PanzoProperties"
            | "PanzoCollageProperties"
    )
}
/// A complete paint transaction; alpha is opaque for client-painted DWM captions.
pub(crate) struct BufferedFrame {
    // Dropped after EndBufferedPaint in this frame's Drop implementation.
    _runtime: PaintSession,
    buffer: isize,
    pub dc: HDC,
}
impl BufferedFrame {
    pub fn new(target: HDC, r: RECT) -> Self {
        let mut dc = HDC::default();
        let runtime = PaintSession::new();
        let buffer = if runtime.active {
            unsafe {
                windows::Win32::UI::Controls::BeginBufferedPaint(
                    target,
                    &r,
                    windows::Win32::UI::Controls::BPBF_TOPDOWNDIB,
                    None,
                    &mut dc,
                )
            }
        } else {
            0
        };
        Self {
            _runtime: runtime,
            buffer,
            dc: if buffer == 0 { target } else { dc },
        }
    }
}
impl Drop for BufferedFrame {
    fn drop(&mut self) {
        if self.buffer != 0 {
            unsafe {
                let _ = windows::Win32::UI::Controls::BufferedPaintSetAlpha(self.buffer, None, 255);
                let _ = windows::Win32::UI::Controls::EndBufferedPaint(self.buffer, true);
            }
        }
    }
}
thread_local! {static COMBO_ICONS:RefCell<HashMap<isize,IconKind>>=RefCell::new(HashMap::new());}
pub(crate) fn combo_icon(h: HWND, icon: Option<IconKind>) {
    COMBO_ICONS.with(|v| {
        let mut v = v.borrow_mut();
        if let Some(icon) = icon {
            v.insert(h.0 as isize, icon);
        } else {
            v.remove(&(h.0 as isize));
        }
    });
    layout_combo_edit(h);
    invalidate(h);
}
fn combo_glyph(h: HWND) -> Option<IconKind> {
    COMBO_ICONS.with(|v| v.borrow().get(&(h.0 as isize)).copied())
}
fn editable_combo(h: HWND) -> bool {
    unsafe {
        GetWindowLongPtrW(h, GWL_STYLE) & 3
            == windows::Win32::UI::WindowsAndMessaging::CBS_DROPDOWN as isize
    }
}
fn layout_combo_edit(h: HWND) {
    use windows::Win32::UI::Controls::{COMBOBOXINFO, GetComboBoxInfo};
    if !editable_combo(h) {
        return;
    }
    let mut info = COMBOBOXINFO {
        cbSize: size_of::<COMBOBOXINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetComboBoxInfo(h, &mut info) }.is_err() || info.hwndItem.is_invalid() {
        return;
    }
    field_controls::edit(info.hwndItem);
    let t = EditorTheme::current(editor_ui::window_dpi(h));
    let r = super::workspace_ui::rect(h);
    let x = if combo_glyph(h).is_some() {
        t.scale(27)
    } else {
        t.scale(3)
    };
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
            info.hwndItem,
            None,
            x,
            t.scale(3),
            (r.right - x - t.scale(26)).max(1),
            (r.bottom - t.scale(6)).max(1),
            windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE
                | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                | windows::Win32::UI::WindowsAndMessaging::SWP_NOCOPYBITS,
        );
        if windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() != info.hwndItem {
            SendMessageW(
                info.hwndItem,
                windows::Win32::UI::Controls::EM_SETSEL,
                Some(WPARAM(usize::MAX)),
                Some(LPARAM(-1)),
            );
        }
    }
}
fn paint_combo_item(item: &DRAWITEMSTRUCT) {
    // The list subclass paints a complete buffered frame. Let native selection
    // update its state, then repaint once; native partial row paints would flash.
    use windows::Win32::UI::Controls::{COMBOBOXINFO, GetComboBoxInfo, ODS_COMBOBOXEDIT};
    if item.itemState.0 & ODS_COMBOBOXEDIT.0 != 0 {
        invalidate(item.hwndItem);
        return;
    }
    let mut info = COMBOBOXINFO {
        cbSize: std::mem::size_of::<COMBOBOXINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetComboBoxInfo(item.hwndItem, &mut info) }.is_ok() {
        invalidate(info.hwndList);
    }
}

#[cfg(test)]
mod paint_lifetime_tests {
    use super::*;
    #[test]
    fn nested_paint_sessions_release_before_thread_teardown_and_can_restart() {
        std::thread::spawn(|| {
            let first = PaintSession::new();
            let second = PaintSession::new();
            assert!(first.active && second.active);
            assert_eq!(PAINT_USERS.with(std::cell::Cell::get), 2);
            drop(first);
            assert_eq!(PAINT_USERS.with(std::cell::Cell::get), 1);
            drop(second);
            assert_eq!(PAINT_USERS.with(std::cell::Cell::get), 0);
            let restarted = PaintSession::new();
            assert!(restarted.active);
            drop(restarted);
            assert_eq!(PAINT_USERS.with(std::cell::Cell::get), 0);
        })
        .join()
        .unwrap();
    }
}

pub(crate) fn separator(hwnd: HWND) {
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(separator_proc), 0x50415a79, 0);
    }
}
unsafe extern "system" fn separator_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    if m == WM_PAINT || m == WM_PRINTCLIENT {
        let mut ps = Gdi::PAINTSTRUCT::default();
        let dc = if m == WM_PAINT {
            unsafe { Gdi::BeginPaint(h, &mut ps) }
        } else {
            HDC(w.0 as *mut _)
        };
        editor_ui::fill_rect(
            dc,
            &super::workspace_ui::rect(h),
            EditorTheme::current(editor_ui::window_dpi(h))
                .palette
                .divider,
        );
        if m == WM_PAINT {
            unsafe {
                let _ = Gdi::EndPaint(h, &ps);
            }
        }
        return LRESULT(0);
    }
    if m == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(h, Some(separator_proc), id);
        }
    }
    unsafe { DefSubclassProc(h, m, w, l) }
}

/// Focus/cancel notifications must not release capture owned by another control.
pub(crate) fn release_capture_owned_by(hwnd: HWND) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture};
    unsafe {
        if GetCapture() == hwnd {
            let _ = ReleaseCapture();
        }
    }
}

/// Native section labels share the same heading style as the painted inspector.
/// Keep it when the common theme/DPI refresh sends the default body font.
pub(crate) fn heading(hwnd: HWND) {
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(heading_proc), 0x5041_5a13, 0);
        SendMessageW(hwnd, WM_SETFONT, Some(WPARAM(0)), Some(LPARAM(1)));
    }
}

unsafe extern "system" fn heading_proc(
    hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, id: usize, _: usize,
) -> LRESULT {
    if message == WM_SETFONT {
        let font = editor_ui::font_handle(TextStyle::Heading, editor_ui::window_dpi(hwnd));
        return unsafe { DefSubclassProc(hwnd, message, WPARAM(font.0 as usize), lparam) };
    }
    if message == WM_NCDESTROY {
        unsafe { let _ = RemoveWindowSubclass(hwnd, Some(heading_proc), id); }
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}
