//! End-to-end export proof from a generated video, without capturing user content.
use super::*;
use crate::platform::{
    capture_tools::Raster,
    compositor::RenderHashProbe,
    editor::ProjectEditorSession,
    export::{
        ExportFormat, ExportOptions, ExportPreset, ExportRequest, ExportSnapshot, ProjectExporter,
    },
    preview::ProjectPreviewSession,
};
use image::AnimationDecoder;
use panzo_core::{AnnotationAnchor, AnnotationKind, OutputDescriptor, VideoAnnotation};
#[test]
#[ignore = "requires synthetic PANZO_STAGE_D_SOURCE video and new PANZO_STAGE_D_VIDEO_DIR"]
#[allow(clippy::too_many_lines)] // End-to-end evidence keeps project lifecycle and assertions in one test.
fn stage_d_annotations_export() {
    let root = PathBuf::from(std::env::var_os("PANZO_STAGE_D_VIDEO_DIR").expect("isolated output"));
    std::fs::create_dir_all(&root).unwrap();
    let input = PathBuf::from(std::env::var_os("PANZO_STAGE_D_SOURCE").expect("generated source"));
    let original = std::fs::read(&input).unwrap();
    let cancel = AtomicBool::new(false);
    let project = import_mp4(&input, &root, &cancel, &AtomicU32::new(0)).unwrap();
    let mut editor = ProjectEditorSession::open(&project).unwrap();
    let id = editor.video_edit().clips[0].id.clone();
    editor
        .trim_video(&id, TimeTick::from_millis(500), TimeTick::from_millis(2500))
        .unwrap();
    editor.set_video_speed(&id, 200).unwrap();
    let cover = VideoAnnotation {
        id: "cover".into(),
        start: TimeTick::from_millis(500),
        end: TimeTick::from_millis(1500),
        kind: AnnotationKind::Cover,
        anchor: AnnotationAnchor::Canvas,
        bounds: [0.2, 0.25, 0.8, 0.85],
        text: String::new(),
        color: "#00FF00".into(),
        stroke: 0.004,
        font_size: 0.08,
    };
    let title = VideoAnnotation {
        id: "title".into(),
        kind: AnnotationKind::Text,
        bounds: [0.08, 0.04, 0.9, 0.24],
        text: "Panzo 视频标注".into(),
        color: "#FFFFFF".into(),
        ..cover.clone()
    };
    let mut settings = editor.settings().clone();
    settings.annotations = vec![cover, title];
    editor.replace_media_settings(settings.clone()).unwrap();
    assert!(editor.undo());
    assert!(editor.settings().annotations.is_empty());
    assert!(editor.redo());
    editor
        .save(panzo_core::CameraEditKind::Style, None)
        .unwrap();
    drop(editor);
    let editor = ProjectEditorSession::open(&project).unwrap();
    assert_eq!(editor.settings().annotations, settings.annotations);
    let native = MfBgraDecoder::open(project.join("media/screen.mp4")).unwrap();
    let output = OutputDescriptor::bgra8(native.width(), native.height()).unwrap();
    drop(native);
    let active =
        RenderHashProbe::project_frame(&project, TimeTick::from_millis(100), output).unwrap();
    let expired =
        RenderHashProbe::project_frame(&project, TimeTick::from_millis(750), output).unwrap();
    assert!(active.hashes_match && expired.hashes_match);
    assert_ne!(active.preview_hash, expired.preview_hash);
    let mut preview = ProjectPreviewSession::open(&project, output).unwrap();
    preview.seek(TimeTick::from_millis(100)).unwrap();
    let frame = preview.render_current().unwrap().composited;
    let center = ((frame.height / 2 * frame.width + frame.width / 2) * 4) as usize;
    assert_eq!(&frame.pixels[center..center + 4], &[0, 255, 0, 255]);
    Raster {
        width: frame.width,
        height: frame.height,
        pixels: frame.pixels,
    }
    .save(&root.join("annotated-preview.png"))
    .unwrap();
    drop(preview);
    let mut results = Vec::new();
    for (format, fps) in [(ExportFormat::Mp4, 30), (ExportFormat::Gif, 15)] {
        let options = ExportOptions {
            format,
            fps,
            audio: false,
            ..Default::default()
        };
        let path = root.join(format!("annotations.{}", options.extension()));
        let report = ProjectExporter::export_snapshot_with_options(
            &ExportRequest {
                project_root: project.clone(),
                output_path: path.clone(),
                maximum_duration: None,
            },
            &ExportSnapshot {
                camera: editor.camera().clone(),
                settings: editor.settings().clone(),
            },
            ExportPreset::Native,
            options,
            &cancel,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(report.frames_written, fps);
        let (first, last) = if format == ExportFormat::Mp4 {
            let mut decoder = MfBgraDecoder::open(&path).unwrap();
            let mut colors = Vec::new();
            while let Some(f) = decoder.read_next().unwrap() {
                let p = ((f.height / 2 * f.width + f.width / 2) * 4) as usize;
                colors.push([f.pixels[p], f.pixels[p + 1], f.pixels[p + 2]]);
            }
            (*colors.first().unwrap(), *colors.last().unwrap())
        } else {
            let frames = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(
                std::fs::File::open(&path).unwrap(),
            ))
            .unwrap()
            .into_frames()
            .collect_frames()
            .unwrap();
            let color = |f: &image::Frame| {
                let b = f.buffer();
                let p = b.get_pixel(b.width() / 2, b.height() / 2);
                [p[2], p[1], p[0]]
            };
            (
                color(frames.first().unwrap()),
                color(frames.last().unwrap()),
            )
        };
        assert!(
            first[1] > 230 && first[0] < 25 && first[2] < 25,
            "{first:?}"
        );
        assert_ne!(last, first);
        results.push(serde_json::json!({"format":options.extension(),"fps":fps,"frames":report.frames_written,"firstCenterBgr":first,"lastCenterBgr":last}));
    }
    assert_eq!(std::fs::read(&input).unwrap(), original);
    std::fs::write(root.join("annotation-proof.json"),serde_json::to_vec_pretty(&serde_json::json!({"previewExportHashMatches":true,"activeHash":active.preview_hash,"expiredHash":expired.preview_hash,"undoRedoSaveReopen":true,"trimStartMs":500,"speedPercent":200,"sourceUnchanged":true,"exports":results})).unwrap()).unwrap();
}
