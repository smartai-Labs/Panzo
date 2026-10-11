//! Native message-sequence acceptance; uses an isolated library and synthetic image.
use super::*;
fn message(hwnd: HWND, m: u32, wp: usize, p: POINT) {
    unsafe {
        SendMessageW(
            hwnd,
            m,
            Some(WPARAM(wp)),
            Some(LPARAM(
                ((p.y as u32 & 0xffff) << 16 | (p.x as u32 & 0xffff)) as isize,
            )),
        );
    }
}
fn source_client(hwnd: HWND, p: POINT) -> POINT {
    let s = unsafe { picture_mut(hwnd) }.unwrap();
    let r = s.view(hwnd);
    let (w, h) = s.document.borrow().size();
    POINT {
        x: r.left + p.x * (r.right - r.left) / w as i32,
        y: r.top + p.y * (r.bottom - r.top) / h as i32,
    }
}
fn command(hwnd: HWND, id: usize) {
    unsafe { picture_mut(hwnd) }
        .unwrap()
        .command(hwnd, id)
        .unwrap();
}
#[test]
#[ignore = "Opens an isolated native image editor; run explicitly on a Windows desktop"]
fn native_picture_gestures_crop_text_and_buffered_preview() {
    let library = std::env::current_dir()
        .unwrap()
        .join(".tmp")
        .join(format!("image-gesture-review-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&library).unwrap();
    pin_manager::configure(&library, false);
    let document = Rc::new(RefCell::new(Document::new(Raster {
        width: 640,
        height: 360,
        pixels: vec![240; 640 * 360 * 4],
    })));
    for p in document.borrow_mut().original.pixels.chunks_exact_mut(4) {
        p[3] = 255;
    }
    let hwnd = create_picture(document.clone(), &library, false).unwrap();
    struct Cleanup(HWND);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
    let _cleanup = Cleanup(hwnd);
    unsafe {
        let _ = SetWindowPos(hwnd, None, 60, 60, 1000, 760, SWP_NOZORDER | SWP_NOACTIVATE);
    }
    let p = source_client(hwnd, POINT { x: 40, y: 40 });
    let q = source_client(hwnd, POINT { x: 220, y: 140 });
    command(hwnd, 3);
    message(hwnd, WM_LBUTTONDOWN, 0, p);
    message(hwnd, WM_MOUSEMOVE, 0, q);
    message(hwnd, WM_KEYDOWN, 27, POINT::default());
    message(hwnd, WM_LBUTTONUP, 0, q);
    assert!(document.borrow().state.marks.is_empty());
    assert_eq!(document.borrow().revision, 0);
    command(hwnd, 3);
    message(hwnd, WM_LBUTTONDOWN, 0, p);
    message(hwnd, WM_MOUSEMOVE, 0, q);
    message(hwnd, WM_LBUTTONUP, 0, q);
    assert_eq!(document.borrow().state.marks.len(), 1);
    document.borrow_mut().commit(Mark {
        tool: Tool::Rectangle,
        points: vec![POINT { x: 400, y: 100 }, POINT { x: 500, y: 200 }],
        ..Mark::default()
    });
    command(hwnd, 7);
    let revision = document.borrow().revision;
    command(hwnd, 30);
    message(hwnd, WM_LBUTTONDOWN, 0, p);
    message(hwnd, WM_LBUTTONUP, 0, p);
    assert_eq!(document.borrow().revision, revision);
    command(hwnd, 8);
    assert_eq!(document.borrow().state.marks.len(), 2);
    document.borrow_mut().commit(Mark {
        tool: Tool::Text,
        points: vec![POINT { x: 260, y: 60 }],
        text: "original".into(),
        ..Mark::default()
    });
    command(hwnd, 30);
    let text_point = source_client(hwnd, POINT { x: 265, y: 65 });
    message(hwnd, WM_LBUTTONDOWN, 0, text_point);
    message(hwnd, WM_LBUTTONUP, 0, text_point);
    let input = unsafe { picture_mut(hwnd) }.unwrap().text_input;
    assert!(unsafe { super::super::IsWindowVisible(input) }.as_bool());
    let value = wide("edited");
    unsafe {
        SetWindowTextW(input, PCWSTR(value.as_ptr())).unwrap();
    }
    command(hwnd, 45);
    assert_eq!(document.borrow().state.marks[2].text, "edited");
    command(hwnd, 7);
    assert_eq!(document.borrow().state.marks[2].text, "original");
    let revision = document.borrow().revision;
    command(hwnd, 36);
    let crop = unsafe { picture_mut(hwnd) }.unwrap().crop.unwrap();
    assert_eq!(crop.pixels(), [0, 0, 640, 360]);
    let r = unsafe { picture_mut(hwnd) }
        .unwrap()
        .crop_screen(hwnd, crop);
    message(
        hwnd,
        WM_LBUTTONDOWN,
        0,
        POINT {
            x: r.right - 1,
            y: r.bottom - 1,
        },
    );
    message(
        hwnd,
        WM_MOUSEMOVE,
        0,
        POINT {
            x: r.left + (r.right - r.left) / 2,
            y: r.top + (r.bottom - r.top) / 2,
        },
    );
    message(hwnd, WM_LBUTTONUP, 0, POINT::default());
    assert_eq!(
        document.borrow().revision,
        revision,
        "mouse-up only finishes crop adjustment"
    );
    assert_ne!(
        unsafe { picture_mut(hwnd) }.unwrap().crop.unwrap().pixels(),
        [0, 0, 640, 360]
    );
    command(hwnd, 62);
    assert_eq!(document.borrow().size(), (640, 360));
    command(hwnd, 36);
    let state = unsafe { picture_mut(hwnd) }.unwrap();
    state.crop.as_mut().unwrap().set_ratio(Some(1.));
    state.crop_fields();
    command(hwnd, 61);
    assert_eq!(document.borrow().size(), (360, 360));
    assert_eq!(document.borrow().revision, revision + 1);
    command(hwnd, 7);
    assert_eq!(document.borrow().size(), (640, 360));
    let revision = document.borrow().revision;
    let before = unsafe { picture_mut(hwnd) }.unwrap().view(hwnd);
    command(hwnd, 66);
    let after = unsafe { picture_mut(hwnd) }.unwrap().view(hwnd);
    assert!(after.right - after.left > before.right - before.left);
    assert_eq!(document.borrow().revision, revision);
    command(hwnd, 63);
    // A complete off-screen frame is always presented. While a stroke is pending,
    // unchanged pixels retain the original image and the preview cache is viewport-sized.
    command(hwnd, 3);
    let p = source_client(hwnd, POINT { x: 30, y: 300 });
    let q = source_client(hwnd, POINT { x: 300, y: 320 });
    message(hwnd, WM_LBUTTONDOWN, 0, p);
    message(hwnd, WM_MOUSEMOVE, 0, q);
    let state = unsafe { picture_mut(hwnd) }.unwrap();
    let r = crate::platform::workspace_ui::rect(hwnd);
    let target = Surface::new(r.right as u32, r.bottom as u32).unwrap();
    state.buffered_paint(hwnd, target.dc);
    let cache = state.display_cache.as_ref().unwrap();
    assert!(
        cache.area.right - cache.area.left <= r.right
            && cache.area.bottom - cache.area.top <= r.bottom
    );
    let frame = target.raster(r.right as u32, r.bottom as u32);
    let unchanged = source_client(hwnd, POINT { x: 600, y: 330 });
    let i = (unchanged.y as usize * frame.width as usize + unchanged.x as usize) * 4;
    assert_eq!(&frame.pixels[i..i + 3], &[240, 240, 240]);
    frame
        .save(&library.join("image-tools-buffered.png"))
        .unwrap();
    message(hwnd, WM_KEYDOWN, 27, POINT::default());
    let saved = document.borrow().state.clone();
    let mark = saved.marks[2].clone();
    for rotation in 0..4 {
        for flip_x in [false, true] {
            for flip_y in [false, true] {
                {
                    let mut d = document.borrow_mut();
                    d.state.rotation = rotation;
                    d.state.flip_x = flip_x;
                    d.state.flip_y = flip_y;
                }
                let expected = document.borrow().render(None).unwrap();
                let mut preview = document.borrow().render_without(Some(2)).unwrap();
                let view = RECT {
                    left: 0,
                    top: 0,
                    right: expected.width as i32,
                    bottom: expected.height as i32,
                };
                unsafe { picture_mut(hwnd) }
                    .unwrap()
                    .preview_mark(&mut preview, &mark, view, RECT::default())
                    .unwrap();
                assert_eq!(
                    preview.pixels, expected.pixels,
                    "100% text preview must match committed pixels for every rotation/flip"
                );
            }
        }
    }
    document.borrow_mut().state = saved;
    println!(
        "isolated native image review evidence: {}",
        library.display()
    );
}

#[test]
#[ignore = "isolated canvas text and owner-drawn button regression"]
fn native_canvas_text_entry_sync_move_cancel_and_button_repaint() {
    use crate::platform::{desktop_ui, workspace_ui as ui};
    use windows::Win32::UI::{
        Input::KeyboardAndMouse::GetFocus,
        WindowsAndMessaging::{GetDlgItem, IsWindowVisible},
    };
    crate::platform::ui_preferences::set_theme(crate::platform::ui_preferences::ThemeMode::Dark)
        .unwrap();
    fn type_text(edit: HWND, text: &str) {
        let value: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        unsafe {
            SendMessageW(
                edit,
                windows::Win32::UI::Controls::EM_SETSEL,
                Some(WPARAM(0)),
                Some(LPARAM(-1)),
            );
            SendMessageW(
                edit,
                windows::Win32::UI::Controls::EM_REPLACESEL,
                Some(WPARAM(1)),
                Some(LPARAM(value.as_ptr() as isize)),
            );
        }
    }
    let library = std::env::current_dir()
        .unwrap()
        .join(".tmp")
        .join(format!("inline-text-review-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&library).unwrap();
    pin_manager::configure(&library, false);
    let document = Rc::new(RefCell::new(Document::new(Raster {
        width: 640,
        height: 360,
        pixels: vec![255; 640 * 360 * 4],
    })));
    let hwnd = create_picture(document.clone(), &library, false).unwrap();
    unsafe {
        SetWindowPos(hwnd, None, 60, 60, 1000, 760, SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
    }
    command(hwnd, 4);
    let p = source_client(hwnd, POINT { x: 100, y: 90 });
    message(hwnd, WM_LBUTTONDOWN, 0, p);
    message(hwnd, WM_LBUTTONUP, 0, p);
    let edit = unsafe { picture_mut(hwnd) }.unwrap().inline_input;
    let side = unsafe { picture_mut(hwnd) }.unwrap().text_input;
    assert!(unsafe { IsWindowVisible(edit) }.as_bool());
    assert_eq!(unsafe { GetFocus() }, edit);
    assert!(
        document.borrow().state.marks.is_empty(),
        "click creates a draft, not placeholder text"
    );
    assert!(!unsafe { picture_mut(hwnd) }.unwrap().inline_text_changed());
    type_text(edit, "画布输入\r\n第二行");
    assert!(unsafe { picture_mut(hwnd) }.unwrap().inline_text_changed());
    assert!(unsafe { picture_mut(hwnd) }.unwrap().unsaved());
    assert_eq!(ui::value(side), "画布输入\r\n第二行");
    type_text(side, "右侧修改");
    assert_eq!(ui::value(edit), "右侧修改");
    unsafe {
        SendMessageW(hwnd, picture_text::FINISH, Some(WPARAM(1)), None);
    }
    assert_eq!(document.borrow().state.marks.len(), 1);
    assert_eq!(document.borrow().state.marks[0].text, "右侧修改");
    let original = document.borrow().state.marks[0].clone();
    let at = source_client(hwnd, original.points[0]);
    message(hwnd, WM_LBUTTONDBLCLK, 0, at);
    type_text(edit, "取消的文字");
    unsafe {
        SendMessageW(hwnd, picture_text::FINISH, Some(WPARAM(0)), None);
    }
    assert_eq!(document.borrow().state.marks[0].text, "右侧修改");
    message(hwnd, WM_LBUTTONDBLCLK, 0, at);
    type_text(edit, "再次编辑");
    unsafe {
        SendMessageW(hwnd, picture_text::FINISH, Some(WPARAM(1)), None);
    }
    assert_eq!(document.borrow().state.marks[0].text, "再次编辑");
    command(hwnd, 7);
    assert_eq!(document.borrow().state.marks[0].text, "右侧修改");
    let q = POINT {
        x: at.x + 75,
        y: at.y + 30,
    };
    message(hwnd, WM_LBUTTONDOWN, 0, at);
    message(hwnd, WM_MOUSEMOVE, 1, q);
    message(hwnd, WM_LBUTTONUP, 0, q);
    assert!(document.borrow().state.marks[0].points[0].x > original.points[0].x);
    command(hwnd, 7);
    assert_eq!(
        document.borrow().state.marks[0].points[0].x,
        original.points[0].x
    );
    let blank = source_client(hwnd, POINT { x: 400, y: 250 });
    message(hwnd, WM_LBUTTONDOWN, 0, blank);
    message(hwnd, WM_LBUTTONUP, 0, blank);
    type_text(edit, "不保留");
    unsafe {
        SendMessageW(hwnd, picture_text::FINISH, Some(WPARAM(0)), None);
    }
    assert_eq!(document.borrow().state.marks.len(), 1);
    command(hwnd, 3);
    let field = unsafe { GetDlgItem(Some(hwnd), 50) }.unwrap();
    let mut dots = Vec::new();
    for width in [4, 64] {
        ui::text(field, &width.to_string());
        let state = unsafe { picture_mut(hwnd) }.unwrap();
        let r = ui::rect(hwnd);
        let target = Surface::new(r.right as u32, r.bottom as u32).unwrap();
        state.buffered_paint(hwnd, target.dc);
        let raster = target.raster(r.right as u32, r.bottom as u32);
        let theme = editor_ui::EditorTheme::current(state.dpi);
        let d = |n| theme.scale(n);
        let x = r.right - d(state.inspector_width) + d(10);
        let color = theme.palette.text_secondary.0;
        let bgr = [(color >> 16) as u8, (color >> 8) as u8, color as u8];
        let mut count = 0;
        for y in d(52)..d(83) {
            for xx in x..x + d(36) {
                let offset = (y as usize * raster.width as usize + xx as usize) * 4;
                if raster.pixels[offset..offset + 3] == bgr {
                    count += 1;
                }
            }
        }
        dots.push(count);
    }
    assert!(
        dots[1] > dots[0] * 4,
        "stroke indicator must scale with width: {dots:?}"
    );
    for _ in 0..40 {
        for id in [30, 36, 3, 1, 2, 4, 5, 46, 67, 38] {
            let button = unsafe { GetDlgItem(Some(hwnd), id) }.unwrap();
            let r = ui::rect(button);
            if r.right < 1 || r.bottom < 1 {
                continue;
            }
            let target = Surface::new(r.right as u32, r.bottom as u32).unwrap();
            let item = windows::Win32::UI::Controls::DRAWITEMSTRUCT {
                CtlType: windows::Win32::UI::Controls::ODT_BUTTON,
                CtlID: id as u32,
                hwndItem: button,
                hDC: target.dc,
                rcItem: r,
                ..Default::default()
            };
            assert!(desktop_ui::paint_button(&item));
            let raster = target.raster(r.right as u32, r.bottom as u32);
            let white = raster
                .pixels
                .chunks_exact(4)
                .filter(|p| p[..3] == [255, 255, 255])
                .count();
            assert!(
                white * 2 < (r.right * r.bottom) as usize,
                "button {id} became a white block"
            );
        }
    }
    if let Some(out) = std::env::var_os("PANZO_UI_REVIEW_DIR") {
        command(hwnd, 4);
        message(hwnd, WM_LBUTTONDBLCLK, 0, at);
        crate::platform::capture_tools::review_window_png(
            hwnd,
            &std::path::PathBuf::from(out).join("canvas-text-native.png"),
        );
    }
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

#[test]
fn image_font_style_persists_renders_and_undoes() {
    use panzo_core::text_style::{FontStyle, TextAlignment};
    let mut document = Document::new(Raster {
        width: 640,
        height: 360,
        pixels: vec![255; 640 * 360 * 4],
    });
    let mut mark = Mark {
        tool: Tool::Text,
        points: vec![POINT { x: 40, y: 30 }],
        text: "Wide WWW iii\nFont test".into(),
        font_size: 36,
        font: FontStyle {
            family: "Arial".into(),
            weight: 700,
            italic: true,
            underline: true,
            alignment: TextAlignment::Right,
        },
        ..Default::default()
    };
    document.commit(mark.clone());
    let a = document.render(None).unwrap();
    let encoded = serde_json::to_string(&document.state).unwrap();
    let restored = Document::from_state(
        document.original.clone(),
        serde_json::from_str(&encoded).unwrap(),
    )
    .unwrap();
    assert!(restored.state.marks[0].same(&mark));
    assert_eq!(a.pixels, restored.render(None).unwrap().pixels);
    mark.font.family = "Courier New".into();
    mark.font.weight = 400;
    mark.font.italic = false;
    mark.font.underline = false;
    mark.font.alignment = TextAlignment::Left;
    assert!(document.replace_mark(0, mark));
    let b = document.render(None).unwrap();
    assert_ne!(a.pixels, b.pixels);
    document.undo();
    assert_eq!(a.pixels, document.render(None).unwrap().pixels);
    let mut legacy: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    legacy["marks"][0].as_object_mut().unwrap().remove("font");
    let restored = Document::from_state(
        document.original.clone(),
        serde_json::from_value(legacy).unwrap(),
    )
    .unwrap();
    assert_eq!(restored.state.marks[0].font, FontStyle::default());
}

#[test]
fn text_region_wraps_persists_and_undoes() {
    let mut d = Document::new(Raster {
        width: 640,
        height: 360,
        pixels: vec![255; 640 * 360 * 4],
    });
    let mark = Mark {
        tool: Tool::Text,
        points: vec![POINT { x: 30, y: 20 }],
        text: "Panzo words wrap within a resized text region".into(),
        font_size: 24,
        text_box: Some([140, 150]),
        ..Default::default()
    };
    d.commit(mark.clone());
    let r = mark.bounds();
    assert_eq!((r.right - r.left, r.bottom - r.top), (140, 150));
    let image = d.render(None).unwrap();
    let mut last = 0;
    for y in 0..image.height {
        for x in 0..image.width {
            let i = (y * image.width + x) as usize * 4;
            if image.pixels[i..i + 3] != [255; 3] {
                assert!((30..170).contains(&x));
                assert!((20..170).contains(&y));
                last = last.max(y);
            }
        }
    }
    assert!(last > 65, "text must wrap into multiple lines");
    let state: super::super::image_document::ImageState =
        serde_json::from_str(&serde_json::to_string(&d.state).unwrap()).unwrap();
    let restored = Document::from_state(d.original.clone(), state).unwrap();
    assert_eq!(restored.render(None).unwrap().pixels, image.pixels);
    let mut wide = mark.clone();
    wide.text_box = Some([450, 150]);
    d.replace_mark(0, wide);
    assert_ne!(d.render(None).unwrap().pixels, image.pixels);
    d.undo();
    assert_eq!(d.render(None).unwrap().pixels, image.pixels);
    let mut old = serde_json::to_value(&d.state).unwrap();
    old["marks"][0].as_object_mut().unwrap().remove("text_box");
    let old: super::super::image_document::ImageState = serde_json::from_value(old).unwrap();
    assert!(old.marks[0].text_box.is_none());
}

#[test]
#[ignore = "requires isolated Windows desktop/profile"]
fn native_text_region_style_resize_undo() {
    use crate::platform::workspace_ui as ui;
    use windows::Win32::UI::{Controls::RichEdit::*, WindowsAndMessaging::*};
    let library = std::env::current_dir()
        .unwrap()
        .join(".tmp")
        .join(format!("region-review-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&library).unwrap();
    pin_manager::configure(&library, false);
    let d = Rc::new(RefCell::new(Document::new(Raster {
        width: 640,
        height: 360,
        pixels: vec![255; 640 * 360 * 4],
    })));
    d.borrow_mut().commit(Mark {
        tool: Tool::Text,
        points: vec![POINT { x: 80, y: 60 }],
        text: "Text region regression words wrap here".into(),
        text_box: Some([260, 120]),
        ..Default::default()
    });
    let h = create_picture(d.clone(), &library, false).unwrap();
    struct Cleanup(HWND, POINT);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            unsafe {
                let _ = SetCursorPos(self.1.x, self.1.y);
                let _ = DestroyWindow(self.0);
            }
        }
    }
    let mut old = POINT::default();
    unsafe {
        GetCursorPos(&mut old).unwrap();
        SetWindowPos(h, None, 60, 60, 1000, 760, SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
    }
    let _cleanup = Cleanup(h, old);
    unsafe { picture_mut(h) }
        .unwrap()
        .begin_inline_text(h, Some(0), POINT { x: 80, y: 60 });
    let input = unsafe { picture_mut(h) }.unwrap().inline_input;
    let family = unsafe { picture_mut(h) }
        .unwrap()
        .font_controls
        .as_ref()
        .unwrap()
        .family;
    let name = ui::wide("Courier New");
    let index = unsafe {
        SendMessageW(
            family,
            CB_FINDSTRINGEXACT,
            Some(WPARAM(usize::MAX)),
            Some(LPARAM(name.as_ptr() as isize)),
        )
        .0
    };
    assert!(index >= 0);
    unsafe {
        SendMessageW(family, CB_SETCURSEL, Some(WPARAM(index as usize)), None);
        SendMessageW(
            h,
            WM_COMMAND,
            Some(WPARAM(2100 | ((CBN_SELCHANGE as usize) << 16))),
            Some(LPARAM(family.0 as isize)),
        );
    }
    assert_eq!(
        unsafe { picture_mut(h) }.unwrap().font_style.family,
        "Courier New"
    );
    for id in [2101, 2102, 2103] {
        message(h, WM_COMMAND, id, POINT::default());
    }
    let s = unsafe { picture_mut(h) }.unwrap();
    assert!(s.font_style.italic && s.font_style.underline && s.font_style.weight == 700);
    assert!(
        s.property_preview.is_none(),
        "active inline text must have a single drawing layer"
    );
    let mut format = CHARFORMAT2W::default();
    format.Base.cbSize = std::mem::size_of::<CHARFORMAT2W>() as u32;
    unsafe {
        SendMessageW(
            input,
            EM_GETCHARFORMAT,
            Some(WPARAM(SCF_ALL as usize)),
            Some(LPARAM((&mut format as *mut _) as isize)),
        );
    }
    assert!(
        format.Base.dwEffects.contains(CFE_ITALIC) && format.Base.dwEffects.contains(CFE_UNDERLINE)
    );
    assert_eq!(format.wWeight, 700);
    let r = ui::rect(h);
    let surface = Surface::new(r.right as u32, r.bottom as u32).unwrap();
    s.paint(h, surface.dc);
    let raster = surface.raster(r.right as u32, r.bottom as u32);
    assert!(
        raster
            .pixels
            .chunks_exact(4)
            .any(|p| p[2] > 150 && p[1] < 150),
        "shared editing raster must contain the text once"
    );
    // Drag the east handle to narrow the region. Font size must stay unchanged.
    let mut edge = RECT::default();
    unsafe {
        GetWindowRect(input, &mut edge).unwrap();
        SetCursorPos(edge.right - 4, (edge.top + edge.bottom) / 2).unwrap();
    }
    message(
        input,
        WM_NCLBUTTONDOWN,
        HTBORDER as usize,
        POINT {
            x: edge.right - 4,
            y: (edge.top + edge.bottom) / 2,
        },
    );
    unsafe {
        SetCursorPos(edge.right - 94, (edge.top + edge.bottom) / 2).unwrap();
    }
    message(input, WM_MOUSEMOVE, 1, POINT::default());
    message(input, WM_LBUTTONUP, 0, POINT::default());
    let s = unsafe { picture_mut(h) }.unwrap();
    assert!(s.inline_text.as_ref().unwrap().size.unwrap()[0] < 240);
    assert_eq!(s.font_size, 24);
    s.finish_inline_text(h, true).unwrap();
    let saved = d.borrow().state.marks[0].clone();
    assert!(saved.text_box.unwrap()[0] < 240);
    assert!(saved.font.italic);
    d.borrow_mut().undo();
    assert_eq!(d.borrow().state.marks[0].text_box, Some([260, 120]));
    d.borrow_mut().redo();
    assert!(d.borrow().state.marks[0].same(&saved));
    // The font-size control offers common presets and accepts non-preset values.
    let size = unsafe { GetDlgItem(Some(h), 51) }.unwrap();
    let preset = ui::wide("8");
    assert!(
        unsafe {
            SendMessageW(
                size,
                CB_FINDSTRINGEXACT,
                Some(WPARAM(usize::MAX)),
                Some(LPARAM(preset.as_ptr() as isize)),
            )
            .0
        } >= 0
    );
    ui::text(size, "37");
    message(
        h,
        WM_COMMAND,
        51 | ((CBN_EDITCHANGE as usize) << 16),
        POINT::default(),
    );
    assert_eq!(unsafe { picture_mut(h) }.unwrap().font_size, 37);
    let preview = unsafe { picture_mut(h) }
        .unwrap()
        .font_controls
        .as_ref()
        .unwrap()
        .preview;
    assert!(!unsafe { IsWindowVisible(preview) }.as_bool());
}

#[test]
#[ignore = "requires isolated Windows desktop/profile"]
fn native_live_color_history_alignment_and_single_undo() {
    use crate::platform::{ui_preferences, workspace_ui as ui};
    use panzo_core::text_style::TextAlignment;
    use windows::Win32::UI::{Input::KeyboardAndMouse::IsWindowEnabled, WindowsAndMessaging::*};
    let library = std::env::current_dir()
        .unwrap()
        .join(".tmp")
        .join(format!("color-review-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&library).unwrap();
    pin_manager::configure(&library, false);
    let document = Rc::new(RefCell::new(Document::new(Raster {
        width: 640,
        height: 360,
        pixels: vec![255; 640 * 360 * 4],
    })));
    let original = Mark {
        tool: Tool::Text,
        points: vec![POINT { x: 80, y: 60 }],
        text: "Live color".into(),
        text_box: Some([260, 120]),
        ..Default::default()
    };
    document.borrow_mut().commit(original.clone());
    let h = create_picture(document.clone(), &library, false).unwrap();
    struct Cleanup(HWND);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
    let _cleanup = Cleanup(h);
    let s = unsafe { picture_mut(h) }.unwrap();
    s.selected = Some(0);
    s.select_properties(h);
    for (id, align) in [
        (2110, TextAlignment::Center),
        (2111, TextAlignment::Right),
        (2104, TextAlignment::Left),
    ] {
        message(h, WM_COMMAND, id, POINT::default());
        assert_eq!(
            unsafe { picture_mut(h) }.unwrap().font_style.alignment,
            align
        );
    }
    message(h, WM_COMMAND, 38, POINT::default());
    let popup = unsafe { FindWindowW(w!("PanzoColorPopover"), None) }.unwrap();
    assert!(unsafe { IsWindowEnabled(h) }.as_bool());
    assert!(unsafe { GetDlgItem(Some(popup), 1) }.is_err());
    let hex = unsafe { GetDlgItem(Some(popup), 10) }.unwrap();
    let before = ui_preferences::recent_colors();
    for (text, bgra) in [
        ("#112233", [0x33, 0x22, 0x11, 255]),
        ("#32A0E0", [0xE0, 0xA0, 0x32, 255]),
    ] {
        ui::text(hex, text);
        assert_eq!(unsafe { picture_mut(h) }.unwrap().color, bgra);
        assert_eq!(
            unsafe { picture_mut(h) }
                .unwrap()
                .property_preview
                .as_ref()
                .unwrap()
                .color,
            bgra
        );
        assert_eq!(document.borrow().state.marks[0].color, original.color);
        assert_eq!(ui_preferences::recent_colors(), before);
    }
    ui::text(hex, "#123");
    message(popup, WM_ACTIVATE, WA_INACTIVE as usize, POINT::default());
    let mut pending = MSG::default();
    unsafe {
        while PeekMessageW(&mut pending, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&pending);
            DispatchMessageW(&pending);
        }
    }
    assert!(!unsafe { IsWindow(Some(popup)) }.as_bool());
    assert_eq!(ui_preferences::recent_colors()[0], [0xE0, 0xA0, 0x32, 255]);
    assert_eq!(
        document.borrow().state.marks[0].color,
        [0xE0, 0xA0, 0x32, 255]
    );
    document.borrow_mut().undo();
    assert!(document.borrow().state.marks[0].same(&original));
    document.borrow_mut().redo();
    assert_eq!(
        document.borrow().state.marks[0].color,
        [0xE0, 0xA0, 0x32, 255]
    );
    for i in 0..20 {
        ui_preferences::remember_color([i, 0, 40, 255]).unwrap();
    }
    assert_eq!(ui_preferences::recent_colors().len(), 16);
    ui_preferences::remember_color([3, 0, 40, 64]).unwrap();
    let history = ui_preferences::recent_colors();
    assert_eq!(history[0], [3, 0, 40, 255]);
    assert_eq!(history.len(), 16);
    assert_eq!(history.iter().filter(|c| c[..3] == [3, 0, 40]).count(), 1);
    let settings = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
        .join("Panzo/settings.json");
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(settings).unwrap()).unwrap();
    assert_eq!(value["recent_colors"][0], "#280003");
}

#[test]
#[ignore = "requires isolated Windows desktop/profile"]
fn native_text_activation_preserves_styles_and_identical_pixels() {
    use crate::platform::workspace_ui as ui;
    use panzo_core::text_style::{FontStyle, TextAlignment};
    use windows::Win32::UI::WindowsAndMessaging::*;
    let library = std::env::current_dir()
        .unwrap()
        .join(".tmp")
        .join(format!("activation-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&library).unwrap();
    pin_manager::configure(&library, false);
    let doc = Rc::new(RefCell::new(Document::new(Raster {
        width: 960,
        height: 540,
        pixels: vec![255; 960 * 540 * 4],
    })));
    doc.borrow_mut().commit(Mark {
        tool: Tool::Text,
        points: vec![POINT { x: 100, y: 100 }],
        text: "Abc 中文\r\nSecond line".into(),
        font_size: 36,
        text_box: Some([300, 160]),
        ..Default::default()
    });
    let h = create_picture(doc.clone(), &library, false).unwrap();
    struct Cleanup(HWND);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
    let _cleanup = Cleanup(h);
    unsafe {
        SetWindowPos(h, None, 30, 30, 1200, 850, SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
    }
    let mut cases = 0;
    for zoom in [0.55, 0.815, 1.0, 1.25] {
        for align in [
            TextAlignment::Left,
            TextAlignment::Center,
            TextAlignment::Right,
        ] {
            let s = unsafe { picture_mut(h) }.unwrap();
            s.tool = Tool::Text;
            s.viewport.zoom = Some(zoom);
            s.begin_inline_text(h, Some(0), POINT { x: 100, y: 100 });
            s.font_style = FontStyle {
                family: "Arial".into(),
                weight: 700,
                italic: true,
                underline: true,
                alignment: align,
            };
            s.color = [85, 108, 209, 137];
            s.inline_font_height = 0;
            s.layout_inline_text(h);
            let mut format_rect = RECT::default();
            unsafe {
                SendMessageW(
                    s.inline_input,
                    windows::Win32::UI::Controls::EM_GETRECT,
                    None,
                    Some(LPARAM((&mut format_rect as *mut _) as isize)),
                );
            }
            assert_eq!(
                (format_rect.left, format_rect.top),
                (4, 4),
                "native caret inset must stay in display pixels"
            );
            let r = ui::rect(h);
            let frame = Surface::new(r.right as u32, r.bottom as u32).unwrap();
            s.paint(h, frame.dc);
            let before = frame.raster(r.right as u32, r.bottom as u32);
            let area = s.canvas_area(h);
            s.finish_inline_text(h, true).unwrap();
            s.selected = None;
            s.select_properties(h);
            s.paint(h, frame.dc);
            let after = frame.raster(r.right as u32, r.bottom as u32);
            for y in area.top.max(0)..area.bottom.min(r.bottom) {
                let a = (y as usize * r.right as usize + area.left.max(0) as usize) * 4;
                let b = (y as usize * r.right as usize + area.right.min(r.right) as usize) * 4;
                assert_eq!(
                    &before.pixels[a..b],
                    &after.pixels[a..b],
                    "text moved at zoom {zoom}, alignment {align:?}, row {y}"
                );
            }
            cases += 1;
        }
    }
    let s = unsafe { picture_mut(h) }.unwrap();
    s.tool = Tool::Text;
    s.viewport.zoom = Some(1.0);
    s.selected = Some(0);
    s.select_properties(h);
    s.font_style.family = "Courier New".into();
    s.font_style.underline = false;
    s.preview_properties();
    let at = source_client(h, POINT { x: 110, y: 110 });
    message(h, WM_LBUTTONDOWN, 1, at);
    message(h, WM_LBUTTONUP, 0, at);
    let s = unsafe { picture_mut(h) }.unwrap();
    assert!(s.inline_text.is_some());
    assert_eq!(s.font_style.family, "Courier New");
    assert!(!s.font_style.underline);
    assert_eq!(doc.borrow().state.marks[0].font.family, "Courier New");
    let empty = source_client(h, POINT { x: 700, y: 350 });
    message(h, WM_LBUTTONDOWN, 1, empty);
    message(h, WM_LBUTTONUP, 0, empty);
    let s = unsafe { picture_mut(h) }.unwrap();
    assert!(s.inline_text.is_none() && s.selected.is_none());
    assert_eq!(doc.borrow().state.marks.len(), 1);
    let before = s.font_style.clone();
    s.command(h, 2103).unwrap();
    assert_eq!(s.font_style, before);
    eprintln!(
        "{cases} zoom/alignment image comparisons exact; styles retained; outside click deactivates"
    );
}

#[test]
#[ignore = "requires isolated Windows desktop/profile; run release for timing"]
fn native_text_drag_cache_and_middle_pan() {
    use crate::platform::workspace_ui as ui;
    use windows::Win32::UI::WindowsAndMessaging::*;
    let library = std::env::current_dir()
        .unwrap()
        .join(".tmp")
        .join(format!("drag-pan-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&library).unwrap();
    pin_manager::configure(&library, false);
    let doc = Rc::new(RefCell::new(Document::new(Raster {
        width: 3840,
        height: 2160,
        pixels: vec![240; 3840 * 2160 * 4],
    })));
    let original = Mark {
        tool: Tool::Text,
        points: vec![POINT { x: 400, y: 400 }],
        text: "Drag 中文 text".into(),
        font_size: 64,
        text_box: Some([520, 180]),
        ..Default::default()
    };
    doc.borrow_mut().commit(original.clone());
    let h = create_picture(doc.clone(), &library, false).unwrap();
    struct Cleanup(HWND);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
    let _cleanup = Cleanup(h);
    unsafe {
        SetWindowPos(h, None, 30, 30, 1200, 850, SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
    }
    let s = unsafe { picture_mut(h) }.unwrap();
    s.tool = Tool::Text;
    let r = ui::rect(h);
    let surface = Surface::new(r.right as u32, r.bottom as u32).unwrap();
    let p = source_client(h, POINT { x: 450, y: 450 });
    let revision = doc.borrow().revision;
    message(h, WM_LBUTTONDOWN, 1, p);
    let start = std::time::Instant::now();
    let frames = 40;
    for i in 1..=frames {
        message(
            h,
            WM_MOUSEMOVE,
            1,
            POINT {
                x: p.x + i * 2,
                y: p.y + i,
            },
        );
        unsafe { picture_mut(h) }.unwrap().paint(h, surface.dc);
    }
    let drag_ms = start.elapsed().as_secs_f64() * 1000. / frames as f64;
    assert_eq!(
        doc.borrow().revision,
        revision,
        "drag must not create per-frame undo steps"
    );
    let last = unsafe { picture_mut(h) }
        .unwrap()
        .object_drag
        .as_ref()
        .unwrap()
        .preview
        .clone();
    message(
        h,
        WM_LBUTTONUP,
        0,
        POINT {
            x: p.x + frames * 2,
            y: p.y + frames,
        },
    );
    assert!(doc.borrow().state.marks[0].same(&last));
    doc.borrow_mut().undo();
    assert!(doc.borrow().state.marks[0].same(&original));
    doc.borrow_mut().redo();
    let s = unsafe { picture_mut(h) }.unwrap();
    s.begin_inline_text(h, Some(0), last.points[0]);
    s.font_style.italic = true;
    s.font_style.underline = true;
    s.layout_inline_text(h);
    s.paint(h, surface.dc);
    let input = s.inline_input;
    let initial_view = s.view(h);
    let initial_point = s.inline_text.as_ref().unwrap().point;
    let initial_style = s.font_style.clone();
    let revision = doc.borrow().revision;
    let mut input_rect = RECT::default();
    unsafe {
        GetWindowRect(input, &mut input_rect).unwrap();
    }
    // The child must forward middle press without committing text or stealing focus.
    message(input, WM_MBUTTONDOWN, 0x10, POINT { x: 30, y: 30 });
    let s = unsafe { picture_mut(h) }.unwrap();
    let (start, _, middle) = s.pan_drag.unwrap();
    assert!(middle);
    let begin = std::time::Instant::now();
    for i in 1..=40 {
        message(
            h,
            WM_MOUSEMOVE,
            0x10,
            POINT {
                x: start.x + i * 2,
                y: start.y + i,
            },
        );
        unsafe { picture_mut(h) }.unwrap().paint(h, surface.dc);
    }
    let pan_ms = begin.elapsed().as_secs_f64() * 1000. / 40.;
    message(
        h,
        WM_MBUTTONUP,
        0,
        POINT {
            x: start.x + 80,
            y: start.y + 40,
        },
    );
    let s = unsafe { picture_mut(h) }.unwrap();
    assert!(s.pan_drag.is_none());
    assert_eq!(s.inline_text.as_ref().unwrap().point, initial_point);
    assert_eq!(s.font_style, initial_style);
    assert_eq!(ui::value(input), last.text);
    assert_eq!(doc.borrow().revision, revision);
    let view = s.view(h);
    assert_eq!(
        (view.left - initial_view.left, view.top - initial_view.top),
        (80, 40)
    );
    let mut next = RECT::default();
    unsafe {
        GetWindowRect(input, &mut next).unwrap();
    }
    assert_eq!(
        (next.left - input_rect.left, next.top - input_rect.top),
        (80, 40)
    );
    assert_eq!(
        unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() },
        input
    );
    // Capture loss ends a gesture; subsequent mouse moves no longer pan.
    let center = POINT {
        x: view.left + 10,
        y: view.top + 10,
    };
    message(h, WM_MBUTTONDOWN, 0x10, center);
    message(h, WM_CANCELMODE, 0, center);
    message(
        h,
        WM_MOUSEMOVE,
        0x10,
        POINT {
            x: center.x + 200,
            y: center.y + 200,
        },
    );
    assert_eq!(unsafe { picture_mut(h) }.unwrap().view(h), view);
    // Compare cache updates with fresh full renders after styles, moves and transforms.
    let mut cache = None;
    for rotation in 0..4 {
        doc.borrow_mut().state.rotation = rotation;
        doc.borrow_mut().state.flip_x = rotation % 2 == 0;
        doc.borrow_mut().revision += 1;
        for i in 0..5 {
            let mut mark = last.clone();
            mark.points[0].x += i * 37;
            mark.points[0].y += i * 13;
            mark.font.underline = i % 2 == 0;
            picture_preview::DraftCache::update(&mut cache, &doc.borrow(), &mark, Some(0));
            let full = doc.borrow().render_replacement(&mark, Some(0)).unwrap();
            let levels = preview_levels(&full);
            let view = RECT {
                left: 0,
                top: 0,
                right: 600,
                bottom: 400,
            };
            let expected = levels
                .iter()
                .find(|l| l.width >= 600 && l.height >= 400)
                .unwrap();
            assert!(
                cache.as_ref().unwrap().image(view).pixels == expected.pixels,
                "rotation={rotation} step={i}"
            );
        }
    }
    doc.borrow_mut().state.rotation = 0;
    doc.borrow_mut().state.flip_x = false;
    let start = std::time::Instant::now();
    for i in 0..frames {
        let mut mark = last.clone();
        mark.points[0].x += i * 3;
        let full = doc.borrow().render_replacement(&mark, Some(0)).unwrap();
        std::hint::black_box(preview_levels(&full));
    }
    let full_ms = start.elapsed().as_secs_f64() * 1000. / frames as f64;
    eprintln!(
        "4K canvas: drag {drag_ms:.2} ms/frame (includes first cache build), pan {pan_ms:.2} ms/frame; full-render baseline {full_ms:.2} ms/frame"
    );
}
