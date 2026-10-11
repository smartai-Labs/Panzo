//! Explicit desktop acceptance: native picture windows, WGC region capture and both encoders.
use super::*;
use crate::platform::capture_tools;
use crate::platform::export::{ExportFormat, ExportOptions, ExportPreset, ExportSnapshot};
use crate::platform::media_decoder::MfBgraDecoder;
use image::AnimationDecoder;

#[test]
#[ignore = "requires desktop/GPU and PANZO_STAGE_A_DIR; records only its own synthetic image window"]
#[allow(clippy::too_many_lines)]
fn stage_a_native_capture_and_export() {
    let directory = PathBuf::from(std::env::var_os("PANZO_STAGE_A_DIR").expect("review directory"));
    fs::create_dir_all(&directory).unwrap();
    let library = directory.join("library");
    fs::create_dir_all(&library).unwrap();
    let (done_sender, done_receiver) = mpsc::channel();
    let output = directory.clone();
    RecorderWindow::run_internal(
        &library,
        Automation::None,
        None,
        Some(Box::new(move |raw| {
            let hwnd = HWND(raw as *mut std::ffi::c_void);
            let state = unsafe { state_mut(hwnd) }.unwrap();
            let _drawing = crate::platform::editor_ui::DrawingScale::enter(state.theme.dpi);
            let client = client_rect(hwnd);
            crate::platform::editor_ui::render_review_png(
                &output.join("home.png"),
                client.right,
                client.bottom,
                |dc, rect| draw_scene(dc, rect, state),
            );
            assert!(state.home);
            let layout = ControlLayout::for_state(hwnd, state);
            for id in [
                RecorderControlId::Screenshot,
                RecorderControlId::NewRecording,
                RecorderControlId::Pin,
                RecorderControlId::Recent,
                RecorderControlId::OpenImage,
                RecorderControlId::OpenProject,
            ] {
                let rect = layout.control_rect(id).0;
                assert_eq!(
                    layout.hit_control(
                        i32::midpoint(rect.left, rect.right),
                        i32::midpoint(rect.top, rect.bottom)
                    ),
                    Some(id)
                );
            }
            state.activate_control(hwnd, RecorderControlId::NewRecording);
            assert!(!state.home);
            crate::platform::editor_ui::render_review_png(
                &output.join("recording-setup.png"),
                client.right,
                client.bottom,
                |dc, rect| draw_scene(dc, rect, state),
            );
            crate::platform::export_window::review_format_controls(hwnd, &output);
            let source = capture_tools::review_picture_flow(hwnd, &state.library_root, &output);
            std::thread::spawn(move || {
                // The source is an opaque topmost synthetic image; leave time for DWM composition.
                std::thread::sleep(Duration::from_millis(250));
                let outcome = std::panic::catch_unwind(|| verify_media(&output, source));
                let _ = done_sender.send(outcome.map_err(|_| "capture/export assertion failed"));
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        Some(HWND(raw as *mut std::ffi::c_void)),
                        WM_CLOSE,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            });
        })),
    )
    .unwrap();
    done_receiver
        .recv_timeout(Duration::from_mins(2))
        .unwrap()
        .unwrap();
}

#[allow(clippy::too_many_lines)]
fn verify_media(directory: &Path, region: panzo_core::PhysicalRect) {
    let project = directory.join("region.panzo");
    let mut control = RecordingControl::default();
    control.camera_enabled = true;
    CaptureRecorder::record_with_control(
        &project,
        Duration::from_secs(1),
        &control,
        &RecordingSource::Region(region),
    )
    .unwrap();
    let layout = ProjectLayout::open(&project).unwrap();
    let manifest = layout.load_manifest().unwrap();
    assert_eq!(
        manifest.capture.kind,
        panzo_core::project::CaptureKind::Region
    );
    let editor = crate::platform::editor::ProjectEditorSession::open(&project).unwrap();
    assert!(editor.settings().camera_enabled);
    let snapshot = ExportSnapshot {
        camera: editor.camera().clone(),
        settings: editor.settings().clone(),
    };
    let mut raw = MfBgraDecoder::open(project.join("media/screen.mp4")).unwrap();
    assert_eq!((raw.width(), raw.height()), (region.width, region.height));
    let frame = raw.read_next().unwrap().unwrap();
    // Capture crop is entirely inside the red half of our synthetic image.
    let center = ((region.height / 2 * region.width + region.width / 2) * 4) as usize;
    assert!(
        frame.pixels[center + 2] > 180 && frame.pixels[center] < 80,
        "crop must contain the synthetic red area"
    );
    let cancel = AtomicBool::new(false);
    let mut reports = Vec::new();
    for (format, fps) in [
        (ExportFormat::Mp4, 30),
        (ExportFormat::Mp4, 60),
        (ExportFormat::Gif, 15),
        (ExportFormat::Gif, 25),
    ] {
        let options = ExportOptions {
            format,
            fps,
            ..Default::default()
        };
        let request = ExportRequest {
            project_root: project.clone(),
            output_path: directory.join(format!("region-{fps}.{}", options.extension())),
            maximum_duration: Some(panzo_core::TimeTick::from_millis(900)),
        };
        let report = ProjectExporter::export_snapshot_with_options(
            &request,
            &snapshot,
            ExportPreset::Compact480,
            options,
            &cancel,
            |_, _| {},
        )
        .unwrap();
        assert_eq!((report.width, report.height), (region.width, region.height));
        if format == ExportFormat::Gif {
            let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(
                fs::File::open(&request.output_path).unwrap(),
            ))
            .unwrap();
            let frames = decoder.into_frames().collect_frames().unwrap();
            assert_eq!(frames.len(), report.frames_written as usize);
            let millis: f64 = frames
                .iter()
                .map(|frame| {
                    let (n, d) = frame.delay().numer_denom_ms();
                    f64::from(n) / f64::from(d)
                })
                .sum();
            assert!((millis - 900.0).abs() <= 10.0, "GIF duration {millis}");
        } else {
            let mut decoder = MfBgraDecoder::open(&request.output_path).unwrap();
            let mut ticks = Vec::new();
            while let Some(frame) = decoder.read_next().unwrap() {
                ticks.push(frame.pts.0);
            }
            assert_eq!(ticks.len(), report.frames_written as usize);
            assert!(ticks.windows(2).all(|pair| {
                (pair[1] - pair[0] - panzo_core::TICKS_PER_SECOND / i64::from(fps)).abs() < 3
            }));
        }
        reports.push(report.to_string());
        assert!(
            ProjectExporter::export_snapshot_with_options(
                &request,
                &snapshot,
                ExportPreset::Native,
                options,
                &cancel,
                |_, _| {}
            )
            .is_err()
        );
        let cancelled = ExportRequest {
            output_path: directory.join(format!("cancelled.{}", options.extension())),
            ..request
        };
        let cancellation = AtomicBool::new(false);
        let result = ProjectExporter::export_snapshot_with_options(
            &cancelled,
            &snapshot,
            ExportPreset::Compact480,
            options,
            &cancellation,
            |_, _| cancellation.store(true, Ordering::Release),
        );
        assert!(result.is_err());
        assert!(!cancelled.output_path.exists());
    }
    fs::write(directory.join("media-results.txt"), reports.join("\n\n")).unwrap();
}
