//! Small native controls shared by persistent workspace panels.
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CBS_DROPDOWN, CBS_DROPDOWNLIST,
            CBS_HASSTRINGS, CBS_OWNERDRAWFIXED, CreateWindowExW, ES_AUTOHSCROLL, ES_AUTOVSCROLL,
            ES_MULTILINE, ES_WANTRETURN, GetClassNameW, GetClientRect, GetWindowTextLengthW,
            GetWindowTextW, HMENU, IDC_ARROW, LoadCursorW, RegisterClassW, SW_HIDE, SW_SHOWNA,
            SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW, SetWindowPos, SetWindowTextW, ShowWindow,
            WINDOW_EX_STYLE, WINDOW_STYLE, WM_SETFONT, WNDCLASSW, WNDPROC, WS_CHILD,
            WS_CLIPCHILDREN, WS_EX_CONTROLPARENT, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
        },
    },
    core::{PCWSTR, w},
};
pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}
pub(crate) fn text(hwnd: HWND, s: &str) {
    if value(hwnd) == s {
        return;
    }
    let s = wide(s);
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(s.as_ptr()));
    }
}
pub(crate) fn value(hwnd: HWND) -> String {
    let n = unsafe { GetWindowTextLengthW(hwnd) };
    let mut s = vec![0; usize::try_from(n).unwrap_or(0) + 1];
    let n = unsafe { GetWindowTextW(hwnd, &mut s) };
    String::from_utf16_lossy(&s[..usize::try_from(n).unwrap_or(0)])
}
pub(crate) fn child(
    parent: HWND,
    class: PCWSTR,
    label: &str,
    id: usize,
    style: WINDOW_STYLE,
) -> Result<HWND, String> {
    super::desktop_ui::install(parent);
    let label = wide(label);
    let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    let h = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR(label.as_ptr()),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            1,
            1,
            Some(parent),
            Some(HMENU(id as *mut _)),
            Some(module.into()),
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    unsafe {
        SendMessageW(
            h,
            WM_SETFONT,
            Some(WPARAM(
                super::editor_ui::font_handle(
                    super::editor_ui::TextStyle::Body,
                    super::editor_ui::window_dpi(parent),
                )
                .0 as usize,
            )),
            Some(LPARAM(1)),
        );
    }
    Ok(h)
}
pub(crate) fn button(p: HWND, s: &str, id: usize) -> Result<HWND, String> {
    let h = child(p, w!("BUTTON"), s, id, WS_TABSTOP)?;
    super::desktop_ui::button(h);
    Ok(h)
}
pub(crate) fn label(p: HWND, s: &str, id: usize) -> Result<HWND, String> {
    child(p, w!("STATIC"), s, id, WINDOW_STYLE(0))
}
pub(crate) fn edit(p: HWND, s: &str, id: usize, multi: bool) -> Result<HWND, String> {
    let h = child(
        p,
        w!("EDIT"),
        s,
        id,
        WS_TABSTOP
            | if multi {
                WS_VSCROLL | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32)
            } else {
                WINDOW_STYLE(ES_AUTOHSCROLL as u32)
            },
    )?;
    super::desktop_ui::edit(h);
    Ok(h)
}
pub(crate) fn combo(p: HWND, values: &[&str], selected: usize, id: usize) -> Result<HWND, String> {
    let h = child(
        p,
        w!("COMBOBOX"),
        "",
        id,
        WS_TABSTOP
            | WS_VSCROLL
            | WINDOW_STYLE((CBS_DROPDOWNLIST | CBS_HASSTRINGS | CBS_OWNERDRAWFIXED) as u32),
    )?;
    for v in values {
        let v = wide(v);
        unsafe {
            SendMessageW(h, CB_ADDSTRING, None, Some(LPARAM(v.as_ptr() as isize)));
        }
    }
    unsafe {
        SendMessageW(h, CB_SETCURSEL, Some(WPARAM(selected)), None);
    }
    super::desktop_ui::combo(h);
    Ok(h)
}
pub(crate) fn selection(h: HWND) -> usize {
    usize::try_from(unsafe { SendMessageW(h, CB_GETCURSEL, None, None) }.0).unwrap_or(0)
}
thread_local! {static PLACEMENTS:std::cell::RefCell<std::collections::HashMap<isize,(i32,i32,i32,i32)>>=std::cell::RefCell::new(std::collections::HashMap::new());static BATCH:std::cell::RefCell<(usize,Vec<(HWND,i32,i32,i32,i32)>)>=const{std::cell::RefCell::new((0,Vec::new()))};}
// Commit sibling positions together, without exposing intermediate parent erases.
thread_local! { static MOVED_TREES: std::cell::RefCell<std::collections::HashSet<isize>> = std::cell::RefCell::new(std::collections::HashSet::new()); }
thread_local! { static PANEL_RESIZE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }
pub(crate) struct PanelResize;
pub(crate) fn panel_resize() -> PanelResize {
    PANEL_RESIZE.with(|v|v.set(v.get()+1));
    PanelResize
}
pub(crate) fn is_panel_resize() -> bool { PANEL_RESIZE.with(|v|v.get()>0) }
impl Drop for PanelResize { fn drop(&mut self) { PANEL_RESIZE.with(|v|v.set(v.get()-1)); } }
pub(crate) struct LayoutBatch;
pub(crate) fn layout_batch(_parent: HWND) -> LayoutBatch {
    BATCH.with(|v| v.borrow_mut().0 += 1);
    LayoutBatch
}
impl Drop for LayoutBatch {
    fn drop(&mut self) {
        let pending = BATCH.with(|v| {
            let mut v = v.borrow_mut();
            v.0 -= 1;
            if v.0 == 0 {
                std::mem::take(&mut v.1)
            } else {
                Vec::new()
            }
        });
        if pending.is_empty() {
            return;
        }
        use windows::Win32::UI::WindowsAndMessaging::*;
        let retaining = is_panel_resize();
        let position_flags = SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW
            | if retaining { SET_WINDOW_POS_FLAGS(0) } else { SWP_NOCOPYBITS };
        unsafe {
            let mut damage = Vec::new();
            let mut groups = std::collections::BTreeMap::<isize, Vec<_>>::new();
            // Nested layout passes may revise a placement before the batch is
            // committed. DeferWindowPos receives each HWND only once, at its final rect.
            let mut latest = std::collections::BTreeMap::new();
            for item in pending {
                latest.insert(item.0.0 as isize, item);
            }
            for item in latest.into_values() {
                let parent = GetParent(item.0).unwrap_or_default();
                let mut old = RECT::default();
                if GetWindowRect(item.0, &mut old).is_ok() {
                    let mut origin = windows::Win32::Foundation::POINT::default();
                    let _ = windows::Win32::Graphics::Gdi::ClientToScreen(parent, &mut origin);
                    old.left -= origin.x; old.right -= origin.x;
                    old.top -= origin.y; old.bottom -= origin.y;
                    damage.push((parent, old));
                }
                damage.push((parent, RECT { left:item.1, top:item.2, right:item.1+item.3, bottom:item.2+item.4 }));
                groups.entry(parent.0 as isize).or_default().push(item);
            }
            // Commit ancestors first. Resizing a parent synchronously lays out its children;
            // that newer layout must supersede the child's older queued rectangle.
            let mut ordered = groups.iter().collect::<Vec<_>>();
            ordered.sort_by_key(|(parent, _)| {
                let mut h = HWND(**parent as *mut _);
                let mut depth = 0;
                while let Ok(p) = GetParent(h) {
                    if p.0.is_null() {
                        break;
                    }
                    depth += 1;
                    h = p;
                    if depth > 64 {
                        break;
                    }
                }
                depth
            });
            for (_, items) in ordered {
                let pending = items
                    .iter()
                    .copied()
                    .filter(|(h, x, y, w, height)| {
                        PLACEMENTS.with(|v| {
                            v.borrow().get(&(h.0 as isize)).copied() == Some((*x, *y, *w, *height))
                        })
                    })
                    .collect::<Vec<_>>();
                if pending.is_empty() {
                    continue;
                }
                if let Ok(mut batch) = BeginDeferWindowPos(pending.len() as i32) {
                    let mut valid = true;
                    for &(h, x, y, w, height) in &pending {
                        match DeferWindowPos(
                            batch,
                            h,
                            None,
                            x,
                            y,
                            w,
                            height,
                            position_flags,
                        ) {
                            Ok(next) => batch = next,
                            Err(_) => {
                                valid = false;
                                break;
                            }
                        }
                    }
                    if valid {
                        let _ = EndDeferWindowPos(batch);
                    } else {
                        for &(h, x, y, w, height) in &pending {
                            let _ = SetWindowPos(
                                h,
                                None,
                                x,
                                y,
                                w,
                                height,
                                position_flags,
                            );
                        }
                    }
                } else {
                    for &(h, x, y, w, height) in &pending {
                        let _ = SetWindowPos(
                            h,
                            None,
                            x,
                            y,
                            w,
                            height,
                            position_flags,
                        );
                    }
                }
            }
            for pending in groups.values() {
                for &(h, _, _, _, _) in pending {
                    let moved = MOVED_TREES.with(|v| v.borrow_mut().remove(&(h.0 as isize)));
                    let flags = windows::Win32::Graphics::Gdi::RDW_INVALIDATE
                        | windows::Win32::Graphics::Gdi::RDW_NOERASE;
                    let _ = windows::Win32::Graphics::Gdi::RedrawWindow(
                        Some(h),
                        None,
                        None,
                        if moved && !retaining {
                            flags | windows::Win32::Graphics::Gdi::RDW_ALLCHILDREN
                        } else {
                            flags
                        },
                    );
                }
            }
            for (parent, rect) in damage {
                let _ = windows::Win32::Graphics::Gdi::RedrawWindow(Some(parent),Some(&rect),None,
                    windows::Win32::Graphics::Gdi::RDW_INVALIDATE
                    | windows::Win32::Graphics::Gdi::RDW_NOERASE
                    | windows::Win32::Graphics::Gdi::RDW_NOCHILDREN);
            }
        }
    }
}
pub(crate) fn forget(h: HWND) {
    MOVED_TREES.with(|v| {
        v.borrow_mut().remove(&(h.0 as isize));
    });
    PLACEMENTS.with(|v| {
        v.borrow_mut().remove(&(h.0 as isize));
    });
}
pub(crate) fn place(h: HWND, x: i32, y: i32, w: i32, height: i32) {
    let mut class = [0u16; 32];
    let len = unsafe { GetClassNameW(h, &mut class) };
    let combo =
        String::from_utf16_lossy(&class[..len.max(0) as usize]).eq_ignore_ascii_case("ComboBox");
    if combo {
        // A combo's window height is its dropdown extent; its closed face uses
        // CB_SETITEMHEIGHT(-1). Honor the requested field height separately.
        let face = rect(h).bottom;
        let item = unsafe {
            SendMessageW(
                h,
                windows::Win32::UI::WindowsAndMessaging::CB_GETITEMHEIGHT,
                Some(WPARAM(usize::MAX)),
                None,
            )
            .0 as i32
        };
        if item >= 0 && face != height {
            let trim = (face - item).max(0);
            unsafe {
                SendMessageW(
                    h,
                    windows::Win32::UI::WindowsAndMessaging::CB_SETITEMHEIGHT,
                    Some(WPARAM(usize::MAX)),
                    Some(LPARAM((height - trim).max(1) as isize)),
                );
            }
        }
    }
    let height = if combo {
        height.max(scale(h, 260))
    } else {
        height
    }
    .max(1);
    let w = w.max(1);
    unsafe {
        let _ =
            windows::Win32::UI::Shell::SetWindowSubclass(h, Some(layout_lifetime), 0x50415a55, 0);
    }
    let next = (x, y, w, height);
    let previous = PLACEMENTS.with(|v| v.borrow_mut().insert(h.0 as isize, next));
    if previous == Some(next) {
        return;
    }
    if previous.is_none_or(|old| old.0 != x || old.1 != y) {
        MOVED_TREES.with(|v| {
            v.borrow_mut().insert(h.0 as isize);
        });
    }
    let queued = BATCH.with(|v| {
        let mut v = v.borrow_mut();
        if v.0 > 0 {
            v.1.push((h, x, y, w, height));
            true
        } else {
            false
        }
    });
    if !queued {
        MOVED_TREES.with(|v| {
            v.borrow_mut().remove(&(h.0 as isize));
        });
        unsafe {
            let _ = SetWindowPos(
                h,
                None,
                x,
                y,
                w,
                height,
                SWP_NOZORDER
                    | SWP_NOACTIVATE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOCOPYBITS,
            );
        }
    }
}
pub(crate) fn show(h: HWND, visible: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{GWL_STYLE, GetWindowLongPtrW};
    let shown = unsafe { GetWindowLongPtrW(h, GWL_STYLE) } & WS_VISIBLE.0 as isize != 0;
    if shown != visible {
        unsafe {
            let _ = ShowWindow(h, if visible { SW_SHOWNA } else { SW_HIDE });
        }
    }
}
pub(crate) fn rect(h: HWND) -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(h, &raw mut r);
    }
    r
}
pub(crate) fn scale(h: HWND, v: i32) -> i32 {
    super::editor_ui::scale_dip(v, super::editor_ui::window_dpi(h))
}
pub(crate) fn register(name: PCWSTR, procedure: WNDPROC) -> Result<(), String> {
    let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    unsafe {
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: procedure,
            hInstance: module.into(),
            lpszClassName: name,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(
                (windows::Win32::Graphics::Gdi::COLOR_WINDOW.0 + 1) as *mut _,
            ),
            ..Default::default()
        });
    }
    Ok(())
}
pub(crate) fn panel(parent: HWND, class: PCWSTR) -> Result<HWND, String> {
    let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class,
            w!(""),
            WS_CHILD | WS_CLIPCHILDREN | windows::Win32::UI::WindowsAndMessaging::WS_CLIPSIBLINGS,
            0,
            0,
            1,
            1,
            Some(parent),
            None,
            Some(module.into()),
            None,
        )
    }
    .map_err(|e| e.to_string())
}

pub(crate) fn number(
    p: HWND,
    value: &str,
    presets: &[&str],
    id: usize,
    icon: Option<super::editor_ui::IconKind>,
) -> Result<HWND, String> {
    let h = child(
        p,
        w!("COMBOBOX"),
        "",
        id,
        WS_TABSTOP
            | WS_VSCROLL
            | WINDOW_STYLE((CBS_DROPDOWN | CBS_HASSTRINGS | CBS_OWNERDRAWFIXED) as u32),
    )?;
    for v in presets {
        let label = wide(v);
        unsafe {
            SendMessageW(h, CB_ADDSTRING, None, Some(LPARAM(label.as_ptr() as isize)));
        }
    }
    text(h, value);
    super::desktop_ui::combo(h);
    super::desktop_ui::combo_icon(h, icon);
    unsafe {
        let mut info = windows::Win32::UI::Controls::COMBOBOXINFO {
            cbSize: std::mem::size_of::<windows::Win32::UI::Controls::COMBOBOXINFO>() as u32,
            ..Default::default()
        };
        if windows::Win32::UI::Controls::GetComboBoxInfo(h, &mut info).is_ok() {
            super::desktop_ui::decimal_field(info.hwndItem, 0.0, 86400.0, 0.1);
            // Numeric fields accept ASCII numbers, including decimal points;
            // composition belongs to text editors, not numeric property fields.
            let _ = windows::Win32::UI::Input::Ime::ImmAssociateContextEx(
                info.hwndItem,
                windows::Win32::UI::Input::Ime::HIMC::default(),
                0,
            );
            SendMessageW(
                info.hwndItem,
                windows::Win32::UI::Controls::EM_SETSEL,
                Some(WPARAM(usize::MAX)),
                Some(LPARAM(-1)),
            );
        }
    }
    Ok(h)
}

unsafe extern "system" fn layout_lifetime(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> windows::Win32::Foundation::LRESULT {
    if m == windows::Win32::UI::WindowsAndMessaging::WM_NCDESTROY {
        forget(h);
        unsafe {
            let _ = windows::Win32::UI::Shell::RemoveWindowSubclass(h, Some(layout_lifetime), id);
        }
    }
    unsafe { windows::Win32::UI::Shell::DefSubclassProc(h, m, w, l) }
}
