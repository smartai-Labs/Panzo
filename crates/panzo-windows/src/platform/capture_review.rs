//! Native screenshots and pin recovery review on a disposable local library.
use super::*;
use std::rc::Rc;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GWL_EXSTYLE, GetDlgItem, GetWindowLongPtrW, LB_GETCOUNT, SendMessageW, WM_CLOSE,
    WM_COMMAND, WM_HOTKEY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WS_EX_TRANSPARENT,
};
fn pack(x: i32, y: i32) -> LPARAM {
    LPARAM(((y as u32 & 0xffff) << 16 | (x as u32 & 0xffff)) as isize)
}
fn command(hwnd: HWND, id: usize) {
    unsafe {
        SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(id)), None);
    }
}
fn drag(hwnd: HWND, start: (i32, i32), end: (i32, i32)) {
    let state = unsafe { picture_mut(hwnd) }.unwrap();
    let view = state.view(hwnd);
    let size = state.document.borrow().size();
    let point = |(x, y): (i32, i32)| {
        pack(
            view.left + x * (view.right - view.left) / size.0 as i32,
            view.top + y * (view.bottom - view.top) / size.1 as i32,
        )
    };
    unsafe {
        SendMessageW(hwnd, WM_LBUTTONDOWN, Some(WPARAM(0)), Some(point(start)));
        SendMessageW(hwnd, WM_MOUSEMOVE, Some(WPARAM(0)), Some(point(end)));
        SendMessageW(hwnd, WM_LBUTTONUP, Some(WPARAM(0)), Some(point(end)));
    }
}
pub(super) fn first_run(owner: HWND, library: &Path, directory: &Path) {
    let width = 800u32;
    let height = 450u32;
    let mut pixels = vec![255; width as usize * height as usize * 4];
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        let x = index % width as usize;
        let y = index / width as usize;
        pixel.copy_from_slice(if (x / 40 + y / 40).is_multiple_of(2) {
            &[245, 239, 224, 255]
        } else {
            &[230, 223, 203, 255]
        });
    }
    let editor = show_image(
        Raster {
            width,
            height,
            pixels,
        },
        library,
        false,
    )
    .unwrap();
    command(editor, 2);
    drag(editor, (80, 80), (300, 160));
    command(editor, 31);
    drag(editor, (350, 75), (590, 180));
    command(editor, 33);
    drag(editor, (65, 220), (370, 260));
    command(editor, 6);
    drag(editor, (540, 220), (730, 270));
    command(editor, 4);
    let field = unsafe { GetDlgItem(Some(editor), 20) }.unwrap();
    let label = wide("中文标注：可编辑\nPNG / JPEG · 桌面贴图");
    unsafe {
        let _ = SetWindowTextW(field, PCWSTR(label.as_ptr()));
    }
    drag(editor, (90, 320), (90, 320));
    let state = unsafe { picture_mut(editor) }.unwrap();
    assert_eq!(state.document.borrow().state.marks.len(), 5);
    command(editor, 30);
    drag(editor, (600, 240), (575, 210));
    command(editor, 7);
    command(editor, 8);
    command(editor, 44);
    let pin = create_picture(
        Rc::clone(&unsafe { picture_mut(editor) }.unwrap().document),
        library,
        true,
    )
    .unwrap();
    command(editor, 34);
    drag(editor, (430, 290), (430, 290));
    assert_eq!(
        unsafe { picture_mut(pin) }
            .unwrap()
            .document
            .borrow()
            .state
            .marks
            .len(),
        6
    );
    let document = Rc::clone(&unsafe { picture_mut(editor) }.unwrap().document);
    let image = document.borrow().render(None).unwrap();
    image.save(&directory.join("edited.png")).unwrap();
    image.save(&directory.join("edited.jpg")).unwrap();
    assert_eq!(
        Raster::load(&directory.join("edited.jpg")).unwrap().width,
        800
    );
    // Editor and pin share objects; displayed scale does not change exported dimensions.
    assert_eq!((image.width, image.height), (800, 450));
    assert_eq!(
        image.pixels,
        unsafe { picture_mut(pin) }
            .unwrap()
            .output()
            .unwrap()
            .pixels
    );
    set_pin_recovery_available(false);
    assert!(
        unsafe { picture_mut(pin) }
            .unwrap()
            .command(pin, 25)
            .is_err()
    );
    set_pin_recovery_available(true);
    command(pin, 25);
    assert_ne!(
        unsafe { GetWindowLongPtrW(pin, GWL_EXSTYLE) } as u32 & WS_EX_TRANSPARENT.0,
        0
    );
    unsafe {
        SendMessageW(owner, WM_HOTKEY, Some(WPARAM(5)), None);
    }
    assert!(!unsafe { picture_mut(pin) }.unwrap().passthrough);
    unsafe { picture_mut(pin) }.unwrap().opacity = 150;
    unsafe { picture_mut(pin) }.unwrap().apply_pin_style(pin);
    unsafe {
        let _ = SetWindowPos(pin, None, 100, 100, 640, 360, SWP_NOACTIVATE | SWP_NOZORDER);
    }
    command(pin, 24);
    command(pin, 22);
    // A second pin exercises close/restore and recovery of off-screen coordinates.
    let second = create_picture(Rc::clone(&document), library, true).unwrap();
    unsafe {
        SendMessageW(second, WM_CLOSE, Some(WPARAM(0)), None);
    }
    assert_eq!(
        pin_manager::entries()
            .iter()
            .filter(|(_, active)| !*active)
            .count(),
        1
    );
    restore_closed_pin().unwrap();
    let restored = PINS.with(|pins| *pins.borrow().last().unwrap());
    unsafe {
        let _ = SetWindowPos(
            restored,
            None,
            5000,
            3000,
            600,
            338,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
    pin_manager::save_session().unwrap();
    assert_eq!(
        pin_manager::entries()
            .iter()
            .filter(|(_, active)| *active)
            .count(),
        2
    );
    unsafe {
        let _ = SetForegroundWindow(editor);
    }
    review_window_png(editor, &directory.join("image-editor.png"));
    show_history(owner, library).unwrap();
    let history = unsafe { FindWindowW(w!("PanzoCaptureHistory"), None) }.unwrap();
    let list = unsafe { GetDlgItem(Some(history), 2) }.unwrap();
    assert!(unsafe { SendMessageW(list, LB_GETCOUNT, None, None) }.0 >= 3);
    review_window_png(history, &directory.join("history.png"));
    unsafe {
        SendMessageW(history, WM_CLOSE, Some(WPARAM(0)), None);
    }
    // Copy/save must be flattened images; editable objects are stored only in the pin session.
    let index = std::fs::read(library.join(".panzo-pins/session.json")).unwrap();
    assert!(String::from_utf8(index).unwrap().contains("中文标注"));
    unsafe {
        let _ = DestroyWindow(editor);
    }
}
pub(super) fn second_run(owner: HWND, library: &Path, directory: &Path) {
    let notices = configure(library, true);
    assert!(notices.is_empty(), "{notices:?}");
    set_pin_recovery_available(true);
    let pins = PINS.with(|pins| pins.borrow().clone());
    assert_eq!(pins.len(), 2);
    let mut found_hidden = false;
    for hwnd in &pins {
        let state = unsafe { picture_mut(*hwnd) }.unwrap();
        assert_eq!(state.document.borrow().state.marks.len(), 6);
        assert!(!state.passthrough);
        if state.opacity == 150 {
            found_hidden = true;
            assert!(state.locked);
            assert!(!unsafe { IsWindowVisible(*hwnd) }.as_bool());
        }
        let mut rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(*hwnd, &raw mut rect);
        }
        assert!(rect.left < 5000 && rect.top < 3000);
    }
    assert!(found_hidden);
    recover_pins().unwrap();
    for hwnd in &pins {
        assert!(!unsafe { picture_mut(*hwnd) }.unwrap().locked);
        assert!(unsafe { IsWindowVisible(*hwnd) }.as_bool());
    }
    let pin = pins[0];
    let state = unsafe { picture_mut(pin) }.unwrap();
    let original = state.output().unwrap();
    state.document.borrow_mut().delete(0);
    assert_eq!(state.document.borrow().state.marks.len(), 5);
    state.document.borrow_mut().undo();
    assert_eq!(state.output().unwrap().pixels, original.pixels);
    unsafe {
        let _ = SetWindowPos(pin, None, 90, 90, 800, 450, SWP_NOZORDER | SWP_NOACTIVATE);
        let _ = ShowWindow(pins[1], SW_HIDE);
    }
    review_present(pin);
    begin(owner, CaptureIntent::Screenshot).unwrap();
    let selector = unsafe { FindWindowW(w!("PanzoCaptureSelection"), None) }.unwrap();
    let x = 120 - unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let y = 125 - unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    unsafe {
        SendMessageW(selector, WM_MOUSEMOVE, Some(WPARAM(0)), Some(pack(x, y)));
    }
    review_present(selector);
    // Capture the selector over the app-owned synthetic reference area only.
    let (desktop, image) = screen().unwrap();
    image
        .crop(PhysicalRect {
            x: 90 - desktop.x,
            y: 90 - desktop.y,
            width: 800,
            height: 450,
        })
        .unwrap()
        .save(&directory.join("selection-magnifier.png"))
        .unwrap();
    unsafe {
        SendMessageW(selector, WM_LBUTTONDOWN, Some(WPARAM(0)), Some(pack(x, y)));
        SendMessageW(
            selector,
            WM_MOUSEMOVE,
            Some(WPARAM(0)),
            Some(pack(x + 300, y + 200)),
        );
        SendMessageW(
            selector,
            WM_LBUTTONUP,
            Some(WPARAM(0)),
            Some(pack(x + 300, y + 200)),
        );
        SendMessageW(selector, WM_KEYDOWN, Some(WPARAM(13)), None);
    }
    let Some(CaptureOutcome::Image(captured)) = take_outcome() else {
        panic!("screenshot selection")
    };
    assert_eq!((captured.width, captured.height), (300, 200));
    assert!(
        captured
            .pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[..3] == [245, 239, 224] || pixel[..3] == [230, 223, 203])
            .count()
            > 30_000,
        "selection must contain the synthetic pin, not an unrelated desktop window"
    );
    command(pin, 40);
    assert_eq!(
        unsafe { picture_mut(pin) }
            .unwrap()
            .document
            .borrow()
            .size(),
        (450, 800)
    );
    command(pin, 7);
    std::fs::write(directory.join("review-results.json"),serde_json::to_vec_pretty(&serde_json::json!({"nativeGestures":true,"editableMarks":6,"restoredPins":2,"passthroughRecovery":true,"opacityAndVisibilityRestored":true,"offscreenRestored":true,"pngJpeg":true,"physicalSelection":[300,200],"mixedDpiHardware":"not available in this automated review"})).unwrap()).unwrap();
}

pub(super) fn fresh_process(library: &Path, directory: &Path) {
    assert!(PINS.with(|pins| pins.borrow().is_empty()));
    assert!(configure(library, true).is_empty());
    let pins = PINS.with(|pins| pins.borrow().clone());
    assert_eq!(pins.len(), 2);
    let expected = Raster::load(&directory.join("edited.png")).unwrap();
    for hwnd in pins {
        let state = unsafe { picture_mut(hwnd) }.unwrap();
        assert_eq!(state.document.borrow().state.marks.len(), 6);
        assert_eq!(state.output().unwrap().pixels, expected.pixels);
    }
    std::fs::write(
        directory.join("fresh-process-restore.json"),
        format!(
            "{{\"processId\":{},\"pins\":2,\"editableObjectsPerPin\":6,\"pixelsMatch\":true}}",
            std::process::id()
        ),
    )
    .unwrap();
}
