//! Dedicated library popup. Other preview and editor windows retain their existing behavior.
use super::*;
use std::cell::RefCell;
thread_local! {static POPUPS:RefCell<Vec<HWND>>=const{RefCell::new(Vec::new())};}
struct Popup {
    owner: HWND,
    library: PathBuf,
    entry: Entry,
    preview: HWND,
    close: HWND,
    image: HWND,
    video: HWND,
    info: HWND,
}
pub(super) fn dialog_message(message: &MSG) -> bool {
    for h in POPUPS.with(|v| v.borrow().clone()) {
        if message.hwnd == h || unsafe { IsChild(h, message.hwnd) }.as_bool() {
            if message.message == WM_KEYDOWN && message.wParam.0 == 27 {
                unsafe {
                    let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                return true;
            }
            if message.message == WM_KEYDOWN && message.wParam.0 == 9 {
                return unsafe { IsDialogMessageW(h, message) }.as_bool();
            }
        }
    }
    false
}
pub(super) fn show(
    parent: HWND,
    owner: HWND,
    library: &Path,
    entry: Entry,
) -> Result<HWND, String> {
    controls::register(w!("PanzoLibraryPreviewDialog"), Some(procedure))?;
    let root = unsafe { GetAncestor(parent, GA_ROOT) };
    let mut bounds = RECT::default();
    unsafe {
        let _ = GetWindowRect(root, &raw mut bounds);
    }
    let dpi = editor_ui::window_dpi(parent);
    let d = |n| editor_ui::scale_dip(n, dpi);
    let width = (bounds.right - bounds.left - d(60)).clamp(d(480), d(1000));
    let height = (bounds.bottom - bounds.top - d(60)).clamp(d(360), d(740));
    let x = bounds.left + (bounds.right - bounds.left - width) / 2;
    let y = bounds.top + (bounds.bottom - bounds.top - height) / 2;
    let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    let h = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            w!("PanzoLibraryPreviewDialog"),
            w!("素材预览"),
            WS_POPUP | WS_THICKFRAME | WS_CLIPCHILDREN,
            x,
            y,
            width,
            height,
            Some(root),
            None,
            Some(module.into()),
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    ui::install(h);
    editor_ui::apply_window_chrome(h);
    let preview = asset_preview::create(h)?;
    asset_preview::library_view(preview, false);
    let close = controls::button(h, "关闭预览", 1)?;
    ui::style(
        close,
        Some(crate::platform::image_icons::CLOSE),
        true,
        false,
        true,
    );
    let image = controls::button(h, "在图片工具中打开", 2)?;
    ui::style(image, None, false, true, false);
    let video = controls::button(h, "添加到视频工作台", 3)?;
    ui::style(video, None, false, true, false);
    let info = controls::button(h, "文件信息", 4)?;
    controls::show(image, matches!(&entry,Entry::File(a)if a.kind.is_image()));
    controls::show(video, matches!(&entry, Entry::File(_)));
    let present = matches!(&entry,Entry::File(a) if !a.missing);
    unsafe {
        let _ = EnableWindow(image, present);
        let _ = EnableWindow(video, present);
    }
    match &entry {
        Entry::File(a) => asset_preview::load(preview, a.clone()),
        Entry::Pin(id, _) => {
            asset_preview::image(preview, "桌面贴图", pin_manager::preview_image(id))
        }
    }
    let mut state = Box::new(Popup {
        owner,
        library: library.into(),
        entry,
        preview,
        close,
        image,
        video,
        info,
    });
    state.layout(h);
    unsafe {
        SetWindowLongPtrW(h, GWLP_USERDATA, Box::into_raw(state) as isize);
        let _ = ShowWindow(h, SW_SHOW);
        let _ = SetForegroundWindow(h);
        let _ = SetFocus(Some(preview));
    }
    POPUPS.with(|v| v.borrow_mut().push(h));
    Ok(h)
}
impl Popup {
    fn layout(&mut self, h: HWND) {
        let r = controls::rect(h);
        let d = |n| controls::scale(h, n);
        controls::place(self.close, r.right - d(44), d(10), d(30), d(30));
        controls::place(
            self.preview,
            d(8),
            d(48),
            r.right - d(16),
            (r.bottom - d(112)).max(1),
        );
        controls::place(self.info, d(20), r.bottom - d(46), d(88), d(30));
        controls::place(
            self.video,
            r.right - d(184),
            r.bottom - d(46),
            d(164),
            d(30),
        );
        controls::place(
            self.image,
            r.right - d(358),
            r.bottom - d(46),
            d(164),
            d(30),
        );
        ui::invalidate(h);
    }
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(result) = library_popup_frame::message(h, m, l, 46) {
        return result;
    }
    let p = unsafe { GetWindowLongPtrW(h, GWLP_USERDATA) } as *mut Popup;
    if p.is_null() {
        return unsafe { DefWindowProcW(h, m, w, l) };
    }
    let s = unsafe { &mut *p };
    match m {
        WM_NCDESTROY => {
            POPUPS.with(|v| v.borrow_mut().retain(|x| *x != h));
            unsafe {
                SetWindowLongPtrW(h, GWLP_USERDATA, 0);
                drop(Box::from_raw(p));
            }
            return unsafe { DefWindowProcW(h, m, w, l) };
        }
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(h);
            }
            return LRESULT(0);
        }
        WM_COMMAND => match w.0 & 65535 {
            1 => unsafe {
                let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            2 | 3 => match open_entry(s.owner, &s.library, &s.entry, w.0 & 65535 == 3) {
                Ok(()) => unsafe {
                    let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
                },
                Err(e) => asset_preview::notice(s.preview, &e),
            },
            4 => show_information(h, &s.entry),
            _ => {}
        },
        WM_SIZE => s.layout(h),
        WM_KEYDOWN if w.0 == 27 => unsafe {
            let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
        },

        WM_GETMINMAXINFO => unsafe {
            (*(l.0 as *mut MINMAXINFO)).ptMinTrackSize = POINT {
                x: controls::scale(h, 500),
                y: controls::scale(h, 360),
            };
        },
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &raw mut ps) };
            let r = controls::rect(h);
            let frame = ui::BufferedFrame::new(dc, r);
            let t = editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
            let d = |n| t.scale(n);
            super::super::fill(frame.dc, r, t.palette.panel);
            ui::text_style(
                frame.dc,
                ui::rect(d(20), d(10), r.right - d(80), d(30)),
                &s.entry.name(),
                t.palette.text,
                t.dpi,
                false,
                editor_ui::TextStyle::Heading,
            );
            ui::line(frame.dc, ui::rect(0, r.bottom - d(60), r.right, 1), t);
            drop(frame);
            unsafe {
                let _ = EndPaint(h, &ps);
            }
        }
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}

#[cfg(test)]
pub(super) fn review_preview(h: HWND) -> HWND {
    unsafe { (*(GetWindowLongPtrW(h, GWLP_USERDATA) as *const Popup)).preview }
}
