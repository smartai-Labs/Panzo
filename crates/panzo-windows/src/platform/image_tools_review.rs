use super::*;
use windows::Win32::UI::WindowsAndMessaging::{WM_CLOSE, WM_TIMER};
pub(super) fn run(owner: HWND, library: &Path, root: &Path, round: usize) {
    if round == 0 {
        let gif = root.join("synthetic.gif");
        let file = std::fs::File::create(&gif).unwrap();
        let mut encoder = image::codecs::gif::GifEncoder::new(file);
        encoder
            .set_repeat(image::codecs::gif::Repeat::Infinite)
            .unwrap();
        for (rgba, delay) in [([255, 30, 20, 255], 100), ([20, 40, 255, 255], 300)] {
            encoder
                .encode_frame(image::Frame::from_parts(
                    image::RgbaImage::from_raw(160, 100, rgba.repeat(160 * 100)).unwrap(),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(delay, 1),
                ))
                .unwrap();
        }
        drop(encoder);
        let hwnd = show_gif(&gif, library).unwrap();
        let state = unsafe { picture_mut(hwnd) }.unwrap();
        assert_eq!(state.animation.as_ref().unwrap().clip.duration_ms, 400);
        assert_eq!(state.animation.as_ref().unwrap().clip.frames.len(), 2);
        state.group = "演示参考".into();
        let first = state.output().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(130));
        unsafe {
            SendMessageW(hwnd, WM_TIMER, Some(WPARAM(91)), None);
        }
        let state = unsafe { picture_mut(hwnd) }.unwrap();
        assert_ne!(state.output().unwrap().pixels, first.pixels);
        state.command(hwnd, 47).unwrap();
        assert!(!state.animation.as_ref().unwrap().playing);
        let stopped = state.output().unwrap().pixels;
        std::thread::sleep(std::time::Duration::from_millis(120));
        unsafe {
            SendMessageW(hwnd, WM_TIMER, Some(WPARAM(91)), None);
        }
        assert_eq!(
            unsafe { picture_mut(hwnd) }
                .unwrap()
                .output()
                .unwrap()
                .pixels,
            stopped
        );
        pin_manager::save_session().unwrap();
        std::fs::remove_file(&gif).unwrap();
        review_window_png(hwnd, &root.join("gif-pin.png"));
        scroll_capture::review(owner, library, root);
        crate::platform::image_tasks::review(library, root);
        let static_image = review_ocr_image();
        let editor = show_image(static_image.clone(), library, false).unwrap();
        review_window_png(editor, &root.join("image-editor-ocr.png"));
        unsafe {
            let _ = DestroyWindow(editor);
        }
        let a = root.join("拼图1.png");
        let b = root.join("拼图2.png");
        static_image.save(&a).unwrap();
        first.save(&b).unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let collage = crate::platform::image_ops::collage(
            &[a.clone(), b.clone()],
            crate::platform::image_ops::Layout::Vertical,
            800,
            16,
            [245, 245, 245, 255],
            &cancel,
        )
        .unwrap();
        collage.save(&root.join("collage.png")).unwrap();
        let report = crate::platform::image_ops::batch(
            &[a, b],
            &root.join("batch"),
            crate::platform::image_ops::OutputFormat::Jpeg,
            80,
            400,
            &cancel,
            |_| {},
        )
        .unwrap();
        assert!(report.contains("2/2"));
        std::fs::write(root.join("batch-report.txt"), report).unwrap();
    } else {
        assert!(configure(library, true).is_empty());
        let pins = PINS.with(|v| v.borrow().clone());
        let gif = pins
            .iter()
            .copied()
            .find(|h| unsafe { picture_mut(*h) }.is_some_and(|s| s.animation.is_some()))
            .expect("restored GIF");
        let state = unsafe { picture_mut(gif) }.unwrap();
        assert_eq!(state.group, "演示参考");
        assert_eq!(state.animation.as_ref().unwrap().playing, round == 2);
        assert!(state.gif_path.as_ref().unwrap().is_file());
        if round == 1 {
            state.command(gif, 47).unwrap();
        }
        unsafe {
            let _ = ShowWindow(gif, SW_HIDE);
        }
        assert!(
            !unsafe { picture_mut(gif) }
                .unwrap()
                .animation
                .as_ref()
                .unwrap()
                .visible
        );
        recover_pins().unwrap();
        assert!(
            unsafe { picture_mut(gif) }
                .unwrap()
                .animation
                .as_ref()
                .unwrap()
                .visible
        );
        unsafe {
            SendMessageW(gif, WM_CLOSE, None, None);
        }
        restore_closed_pin().unwrap();
        std::fs::write(root.join("image-tools-proof.json"),serde_json::to_vec_pretty(&serde_json::json!({"gifFrames":2,"gifDurationMs":400,"groupRestored":"演示参考","restoredWithoutOriginalGif":true,"pauseAndHideVerified":true,"closedPinRestored":true,"freshProcessRestore":round==2,"batchCompleted":2,"scroll":"scroll-proof.json"})).unwrap()).unwrap();
    }
}
