//! Read-only file information, used only by the asset library.
use super::*;
use std::cell::RefCell;
thread_local! {static INFOS:RefCell<Vec<HWND>>=const{RefCell::new(Vec::new())};}
struct Information {
    entry: Entry,
    name: HWND,
    path: HWND,
    copy: HWND,
    done: HWND,
    close: HWND,
}
pub(super) fn dialog_message(message: &MSG) -> bool {
    for h in INFOS.with(|v| v.borrow().clone()) {
        if message.hwnd == h || unsafe { IsChild(h, message.hwnd) }.as_bool() {
            if message.message == WM_KEYDOWN && matches!(message.wParam.0, 13 | 27) {
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
pub(super) fn show(parent: HWND, entry: Entry) -> Result<HWND, String> {
    controls::register(w!("PanzoLibraryFileInformation"), Some(procedure))?;
    let owner = unsafe { GetAncestor(parent, GA_ROOT) };
    let mut bounds = RECT::default();
    unsafe {
        let _ = GetWindowRect(owner, &mut bounds);
    }
    let d = |n| controls::scale(parent, n);
    let (width, height) = editor_ui::available_window_size(d(640), d(520));
    let h = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            w!("PanzoLibraryFileInformation"),
            w!("文件信息"),
            WS_POPUP | WS_THICKFRAME | WS_CLIPCHILDREN,
            (bounds.left + bounds.right - width) / 2,
            (bounds.top + bounds.bottom - height) / 2,
            width,
            height,
            Some(owner),
            None,
            None,
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    ui::install(h);
    editor_ui::apply_window_chrome(h);
    let name = information_text(h, &entry.name(), 10)?;
    let path_text = match &entry {
        Entry::File(a) => a.path.to_string_lossy().into_owned(),
        _ => "桌面贴图没有源文件路径".into(),
    };
    let path = information_text(h, &path_text, 11)?;
    let copy = controls::button(h, "复制完整路径", 2)?;
    ui::style(copy, Some(IconKind::Copy), false, false, false);
    unsafe {
        let _ = EnableWindow(copy, matches!(entry, Entry::File(_)));
    }
    let done = controls::button(h, "确定", 1)?;
    ui::style(done, None, false, true, false);
    let close = controls::button(h, "关闭", 3)?;
    ui::style(
        close,
        Some(crate::platform::image_icons::CLOSE),
        true,
        false,
        true,
    );
    let mut state = Box::new(Information {
        entry,
        name,
        path,
        copy,
        done,
        close,
    });
    state.layout(h);
    unsafe {
        SetWindowLongPtrW(h, GWLP_USERDATA, Box::into_raw(state) as isize);
        let _ = ShowWindow(h, SW_SHOW);
        let _ = SetFocus(Some(done));
    }
    INFOS.with(|v| v.borrow_mut().push(h));
    Ok(h)
}
impl Information {
    fn layout(&mut self, h: HWND) {
        let r = controls::rect(h);
        let d = |n| controls::scale(h, n);
        let batch = controls::layout_batch(h);
        controls::place(self.close, r.right - d(48), d(10), d(30), d(30));
        controls::place(self.name, d(24), d(90), r.right - d(48), d(66));
        controls::place(
            self.path,
            d(24),
            d(332),
            r.right - d(48),
            (r.bottom - d(404)).max(d(44)),
        );
        controls::place(self.copy, d(24), r.bottom - d(48), d(152), d(32));
        controls::place(self.done, r.right - d(116), r.bottom - d(48), d(92), d(32));
        drop(batch);
        ui::invalidate(h);
    }
    fn paint(&self, h: HWND, dc: HDC) {
        let r = controls::rect(h);
        let t = editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
        let d = |n| t.scale(n);
        super::super::fill(dc, r, t.palette.panel);
        ui::text_style(
            dc,
            ui::rect(d(24), d(12), r.right - d(100), d(28)),
            "文件信息",
            t.palette.text,
            t.dpi,
            false,
            editor_ui::TextStyle::Heading,
        );
        ui::line(dc, ui::rect(0, d(52), r.right, 1), t);
        for (label, y) in [("文件名", 64), ("完整路径", 306)] {
            ui::text_style(
                dc,
                ui::rect(d(24), d(y), r.right - d(48), d(22)),
                label,
                t.palette.text_muted,
                t.dpi,
                false,
                editor_ui::TextStyle::Body,
            );
        }
        let values = match &self.entry {
            Entry::File(a) => vec![
                (
                    "文件类型",
                    format!(
                        "{} · {}",
                        a.path
                            .extension()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_uppercase(),
                        self.entry.label()
                    ),
                ),
                (
                    "文件大小",
                    format!("{}  /  {} 字节", file_size(a.bytes), a.bytes),
                ),
                ("修改时间", modified_label(a.modified)),
                (
                    "文件状态",
                    if a.missing {
                        "源文件不存在"
                    } else {
                        "可用"
                    }
                    .into(),
                ),
            ],
            _ => vec![
                ("素材类型", self.entry.label().into()),
                ("文件大小", "—".into()),
                ("修改时间", "—".into()),
                ("文件状态", "桌面贴图".into()),
            ],
        };
        let col = (r.right - d(48)) / 2;
        for (i, (label, value)) in values.iter().enumerate() {
            let x = d(24) + (i as i32 % 2) * col;
            let y = d(178 + (i as i32 / 2) * 62);
            ui::text_style(
                dc,
                ui::rect(x, y, col - d(16), d(22)),
                label,
                t.palette.text_muted,
                t.dpi,
                false,
                editor_ui::TextStyle::Body,
            );
            ui::text_style(
                dc,
                ui::rect(x, y + d(24), col - d(16), d(24)),
                value,
                t.palette.text,
                t.dpi,
                false,
                editor_ui::TextStyle::Body,
            );
        }
        ui::line(dc, ui::rect(0, r.bottom - d(64), r.right, 1), t);
    }
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(result) = library_popup_frame::message(h, m, l, 48) {
        return result;
    }
    let p = unsafe { GetWindowLongPtrW(h, GWLP_USERDATA) } as *mut Information;
    if p.is_null() {
        return unsafe { DefWindowProcW(h, m, w, l) };
    }
    let s = unsafe { &mut *p };
    match m {
        WM_NCDESTROY => {
            INFOS.with(|v| v.borrow_mut().retain(|v| *v != h));
            unsafe {
                SetWindowLongPtrW(h, GWLP_USERDATA, 0);
                drop(Box::from_raw(p));
            }
            return unsafe { DefWindowProcW(h, m, w, l) };
        }
        WM_CLOSE => unsafe {
            let _ = DestroyWindow(h);
        },
        WM_COMMAND => match w.0 & 65535 {
            1 | 3 => unsafe {
                let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            2 => {
                if let Entry::File(a) = &s.entry {
                    if super::super::copy_text(h, &a.path.to_string_lossy()).is_ok() {
                        controls::text(s.copy, "路径已复制");
                    }
                }
            }
            _ => {}
        },
        WM_SIZE => s.layout(h),
        WM_GETMINMAXINFO => unsafe {
            (*(l.0 as *mut MINMAXINFO)).ptMinTrackSize = POINT {
                x: controls::scale(h, 560),
                y: controls::scale(h, 490),
            };
        },

        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let target = unsafe { BeginPaint(h, &mut ps) };
            let frame = ui::BufferedFrame::new(target, controls::rect(h));
            s.paint(h, frame.dc);
            drop(frame);
            unsafe {
                let _ = EndPaint(h, &ps);
            }
        }
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}

// Static text cannot accept edits or show an insertion caret. Long names and
// paths wrap, and ampersands in real filenames are displayed literally.
fn information_text(parent: HWND, value: &str, id: usize) -> Result<HWND, String> {
    const SS_NOPREFIX: u32 = 0x0080;
    const SS_EDITCONTROL: u32 = 0x2000;
    controls::child(
        parent,
        w!("STATIC"),
        value,
        id,
        WINDOW_STYLE(SS_NOPREFIX | SS_EDITCONTROL),
    )
}

#[cfg(test)]
pub(super) fn review_read_only(h: HWND) {
    let s = unsafe { &*(GetWindowLongPtrW(h, GWLP_USERDATA) as *const Information) };
    for field in [s.name, s.path] {
        let mut class = [0u16; 32];
        let length = unsafe { GetClassNameW(field, &mut class) };
        assert_eq!(
            String::from_utf16_lossy(&class[..length as usize]),
            "Static"
        );
        assert_eq!(
            unsafe { GetWindowLongPtrW(field, GWL_STYLE) } & WS_TABSTOP.0 as isize,
            0
        );
        let value = controls::value(field);
        unsafe {
            SendMessageW(field, WM_CHAR, Some(WPARAM(b'x' as usize)), None);
            SendMessageW(field, WM_CHAR, Some(WPARAM(8)), None);
            SendMessageW(field, WM_CLEAR, None, None);
            SendMessageW(field, WM_PASTE, None, None);
        }
        assert_eq!(
            controls::value(field),
            value,
            "information must never accept editing"
        );
    }
}
