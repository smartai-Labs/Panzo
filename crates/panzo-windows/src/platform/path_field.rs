//! Compact directory display with explicit browsing and transient text editing.
use super::{desktop_ui as desk, editor_ui::IconKind, workspace_ui as ui};
use std::path::{Path, PathBuf};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
        UI::{
            Input::KeyboardAndMouse::SetFocus,
            Shell::{
                DefSubclassProc, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
                FileOpenDialog, IFileOpenDialog, IShellItem, RemoveWindowSubclass,
                SHCreateItemFromParsingName, SIGDN_FILESYSPATH, SetWindowSubclass,
            },
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, w},
};
const DONE: u32 = WM_APP + 491;
struct State {
    folder: HWND,
    label: HWND,
    edit: HWND,
    editing: bool,
    directory: bool,
    trailing_browse: bool,
}
pub(crate) fn choose(owner: HWND, initial: &Path) -> Result<Option<PathBuf>, String> {
    let dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| e.to_string())?;
    unsafe {
        dialog
            .SetOptions(
                dialog.GetOptions().map_err(|e| e.to_string())?
                    | FOS_PICKFOLDERS
                    | FOS_FORCEFILESYSTEM
                    | FOS_PATHMUSTEXIST,
            )
            .map_err(|e| e.to_string())?;
        dialog
            .SetTitle(w!("选择文件夹"))
            .map_err(|e| e.to_string())?;
        let start = initial.ancestors().find(|p| p.is_dir());
        if let Some(start) = start {
            let name = ui::wide(&start.to_string_lossy());
            if let Ok(item) =
                SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(name.as_ptr()), None)
            {
                let _ = dialog.SetFolder(&item);
            }
        }
        if let Err(e) = dialog.Show(Some(owner)) {
            if e.code().0 as u32 == 0x800704c7 {
                return Ok(None);
            }
            return Err(e.to_string());
        }
        let result = dialog.GetResult().map_err(|e| e.to_string())?;
        let name = result
            .GetDisplayName(SIGDN_FILESYSPATH)
            .map_err(|e| e.to_string())?;
        let value = name.to_string().map_err(|e| e.to_string());
        CoTaskMemFree(Some(name.0.cast()));
        value.map(|s| Some(PathBuf::from(s)))
    }
}
pub(crate) fn validate(value: &str) -> Result<PathBuf, String> {
    let value = value.trim();
    let p = PathBuf::from(value);
    if !p.is_absolute() {
        return Err("请选择文件夹或输入完整的绝对路径".into());
    }
    let syntax = value.strip_prefix(r"\\?\").unwrap_or(value);
    if syntax
        .chars()
        .any(|c| c < ' ' || ['<', '>', '|', '"', '*', '?'].contains(&c))
    {
        return Err("文件夹路径包含不可用字符".into());
    }
    if p.exists() && !p.is_dir() {
        return Err("该位置是文件，请选择文件夹".into());
    }
    Ok(p)
}
pub(crate) fn create(parent: HWND, value: &str, id: usize) -> Result<HWND, String> {
    create_value(parent, value, id, true)
}
pub(crate) fn filename(parent: HWND, value: &str, id: usize) -> Result<HWND, String> {
    create_value(parent, value, id, false)
}
fn create_value(parent: HWND, value: &str, id: usize, directory: bool) -> Result<HWND, String> {
    ui::register(w!("PanzoPathField"), Some(procedure))?;
    let h = ui::child(parent, w!("PanzoPathField"), value, id, WS_CLIPCHILDREN)?;
    let folder = ui::button(h, "选择文件夹", 1)?;
    desk::style(folder, Some(IconKind::Folder), true, false, true);
    let label = ui::button(h, value, 2)?;
    desk::style(label, None, true, false, false);
    desk::path_label(label);
    let edit = ui::edit(h, value, 3, false)?;
    ui::show(edit, false);
    ui::show(folder, directory);
    unsafe {
        let _ = SetWindowSubclass(edit, Some(edit_proc), 491, h.0 as usize);
        SetWindowLongPtrW(
            h,
            GWLP_USERDATA,
            Box::into_raw(Box::new(State {
                folder,
                label,
                edit,
                editing: false,
                directory,
                trailing_browse:false,
            })) as isize,
        );
    }
    Ok(h)
}
pub(crate) fn is_field(h: HWND) -> bool {
    let mut b = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut b) };
    String::from_utf16_lossy(&b[..n.max(0) as usize]) == "PanzoPathField"
}
pub(crate) fn commit(h: HWND, accept: bool) -> Result<(), String> {
    if !is_field(h) {
        return Ok(());
    }
    let Some(s) = (unsafe { state(h) }) else {
        return Ok(());
    };
    if !s.editing {
        return Ok(());
    }
    let next = if accept {
        if s.directory {
            validate(&ui::value(s.edit))?.to_string_lossy().to_string()
        } else {
            validate_filename(&ui::value(s.edit))?
        }
    } else {
        ui::value(h)
    };
    s.editing = false;
    ui::show(s.edit, false);
    ui::show(s.label, true);
    ui::text(h, &next);
    ui::text(s.label, &next);
    if accept {
        if let Ok(parent) = unsafe { GetParent(h) } {
            unsafe {
                let _ = PostMessageW(
                    Some(parent),
                    WM_COMMAND,
                    WPARAM((GetDlgCtrlID(h) as usize) | ((EN_CHANGE as usize) << 16)),
                    LPARAM(h.0 as isize),
                );
            }
        }
    }
    Ok(())
}
unsafe fn state(h: HWND) -> Option<&'static mut State> {
    unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State).as_mut() }
}
unsafe extern "system" fn edit_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    if m == WM_GETDLGCODE && (w.0 == 13 || w.0 == 27) {
        return LRESULT(windows::Win32::UI::WindowsAndMessaging::DLGC_WANTALLKEYS as isize);
    }
    if m == WM_KEYDOWN && (w.0 == 13 || w.0 == 27) {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(data as *mut _)),
                DONE,
                WPARAM(usize::from(w.0 == 13)),
                LPARAM(0),
            );
        }
        return LRESULT(0);
    }
    if m == WM_CHAR && (w.0 == 13 || w.0 == 27) {
        return LRESULT(0);
    }
    if m == WM_KILLFOCUS {
        unsafe {
            let _ = PostMessageW(Some(HWND(data as *mut _)), DONE, WPARAM(1), LPARAM(0));
        }
    }
    if m == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(h, Some(edit_proc), id);
        }
    }
    unsafe { DefSubclassProc(h, m, w, l) }
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Some(s) = (unsafe { state(h) }) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    match m {
        WM_SIZE => {
            let r = ui::rect(h);
            let n = if s.directory { ui::scale(h, 30) } else { 0 };
            if s.trailing_browse {
                let gap=if s.directory{ui::scale(h,4)}else{0};
                ui::place(s.folder,r.right-n,0,n,r.bottom);
                ui::place(s.label,0,0,(r.right-n-gap).max(0),r.bottom);
                ui::place(s.edit,0,0,(r.right-n-gap).max(0),r.bottom);
            } else {
                ui::place(s.folder, 0, 0, n, r.bottom);
                ui::place(s.label, n + 4, 0, r.right - n - 4, r.bottom);
                ui::place(s.edit, n + 4, 0, r.right - n - 4, r.bottom);
            }
        }
        WM_COMMAND if w.0 == 1 => match choose(h, &PathBuf::from(ui::value(h))) {
            Ok(Some(p)) => {
                s.editing = false;
                ui::show(s.edit, false);
                ui::show(s.label, true);
                ui::text(h, &p.to_string_lossy());
            }
            Ok(None) => {}
            Err(e) => super::workspace::notice_error(&e),
        },
        WM_COMMAND if w.0 == 2 => {
            s.editing = true;
            ui::text(s.edit, &ui::value(h));
            ui::show(s.label, false);
            ui::show(s.edit, true);
            unsafe {
                SendMessageW(
                    s.edit,
                    windows::Win32::UI::Controls::EM_SETSEL,
                    Some(WPARAM(0)),
                    Some(LPARAM(-1)),
                );
                let _ = SetFocus(Some(s.edit));
            }
        }
        DONE => {
            if let Err(e) = commit(h, w.0 != 0) {
                super::workspace::notice_error(&e);
                if let Ok(parent) = unsafe { GetParent(h) } {
                    let mut name = [0u16; 64];
                    let n = unsafe { GetClassNameW(parent, &mut name) };
                    if String::from_utf16_lossy(&name[..n.max(0) as usize]) == "PanzoWorkspaceForm"
                    {
                        super::workspace_form::feedback(parent, &e);
                    }
                }
                if unsafe { IsWindowVisible(s.edit) }.as_bool()
                    && unsafe { GetForegroundWindow() } == unsafe { GetAncestor(h, GA_ROOT) }
                {
                    unsafe {
                        let _ = SetFocus(Some(s.edit));
                    }
                }
            }
        }
        WM_SETTEXT => {
            let result = unsafe { DefWindowProcW(h, m, w, l) };
            ui::text(s.label, &ui::value(h));
            return result;
        }
        WM_NCDESTROY => unsafe {
            SetWindowLongPtrW(h, GWLP_USERDATA, 0);
            drop(Box::from_raw(std::ptr::from_mut(s)));
        },
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_validation_distinguishes_relative_and_invalid_paths() {
        assert!(validate("relative/path").is_err());
        assert!(validate(r"C:\Users\a\输出 文件夹").is_ok());
        assert!(validate(r"\\server\share\folder").is_ok());
        assert!(validate(r"C:\bad*name").is_err());
        assert!(validate(r"\\?\C:\long folder").is_ok());
    }
}

fn validate_filename(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.ends_with(['.', ' '])
        || value
            .chars()
            .any(|c| c < ' ' || ['<', '>', ':', '"', '/', '\\', '|', '?', '*'].contains(&c))
    {
        return Err("请输入有效文件名，不要包含路径分隔符".into());
    }
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err("此文件名是系统保留名称，请修改".into());
    }
    Ok(value.into())
}

pub(crate) fn compact(h:HWND) {
    if let Some(s)=unsafe{state(h)} {
        s.trailing_browse=true;
        desk::style(s.label,None,false,false,false);
        desk::style(s.folder,Some(IconKind::Folder),false,false,true);
        desk::install_canvas(h);
    }
}
