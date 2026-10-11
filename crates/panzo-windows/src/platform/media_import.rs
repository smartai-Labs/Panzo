//! Cancellable MP4 import into an isolated Panzo project. The input is read-only.
use crate::platform::{
    audio::AudioManifest,
    audio_decode::import_audio,
    d3d11::D3d11Device,
    fmp4_writer::{Nv12Fmp4Writer, Nv12VideoConfig, normalize_export_timing},
    manifest_store::{ManifestStore, persist_json_atomically},
    media_decoder::MfBgraDecoder,
    video_processor::Nv12VideoProcessor,
};
use panzo_core::{
    CaptureDescriptor, PhysicalRect, PhysicalSize, ProjectLayout, ProjectManifest, ProjectState,
    ProjectWriter, RecordingLock, TimeTick,
    project::{CaptureKind, TimebaseDescriptor},
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread::{self, JoinHandle},
};
pub struct ImportJob {
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<AtomicU32>,
    result: Arc<Mutex<Option<Result<PathBuf, String>>>>,
    worker: Option<JoinHandle<()>>,
}
impl ImportJob {
    pub fn start(source: PathBuf, library: PathBuf) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0));
        let result = Arc::new(Mutex::new(None));
        let (c, p, r) = (cancel.clone(), progress.clone(), result.clone());
        let worker = thread::spawn(move || {
            let outcome = import_mp4(&source, &library, &c, &p);
            *r.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(outcome);
        });
        Self {
            cancel,
            progress,
            result,
            worker: Some(worker),
        }
    }
    pub fn take_result(&mut self) -> Option<Result<PathBuf, String>> {
        let result = self
            .result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if result.is_some()
            && let Some(worker) = self.worker.take()
        {
            let _ = worker.join();
        }
        result
    }
}
impl Drop for ImportJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
struct Staging {
    path: PathBuf,
    committed: bool,
}
impl Drop for Staging {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
fn checked(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("导入已取消".into())
    } else {
        Ok(())
    }
}
#[allow(clippy::too_many_lines)] // Import stages share the staging guard and cancellation token.
pub fn import_mp4(
    source: &Path,
    library: &Path,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<PathBuf, String> {
    if !source.is_file()
        || !source
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("mp4"))
    {
        return Err("请选择本地 MP4 文件".into());
    }
    checked(cancel)?;
    let index = MfBgraDecoder::native_index(source, || cancel.load(Ordering::Acquire))
        .map_err(|e| e.to_string())?;
    if index.is_empty() {
        return Err("该文件没有可导入的视频帧".into());
    }
    let mut decoder = MfBgraDecoder::open(source).map_err(|e| e.to_string())?;
    let mut pending = decoder
        .read_next()
        .map_err(|e| e.to_string())?
        .ok_or("该文件没有视频帧")?;
    let origin = pending.pts;
    let width = pending.width & !1;
    let height = pending.height & !1;
    if width < 2 || height < 2 {
        return Err("视频尺寸无效".into());
    }
    let id = uuid::Uuid::new_v4();
    let staging_parent = library.join(".panzo-imports");
    std::fs::create_dir_all(&staging_parent).map_err(|e| e.to_string())?;
    let root = staging_parent.join(id.to_string());
    let mut manifest = ProjectManifest::new_v01(
        id,
        env!("CARGO_PKG_VERSION"),
        crate::platform::recorder::utc_now(),
        TimebaseDescriptor {
            ticks_per_second: 10_000_000,
            session_start_qpc: 0,
            qpc_frequency: 10_000_000,
        },
        CaptureDescriptor {
            kind: CaptureKind::Imported,
            monitor_id: String::new(),
            window_id: None,
            window_title: source.file_name().map(|s| s.to_string_lossy().into_owned()),
            desktop_rect_px: PhysicalRect {
                x: 0,
                y: 0,
                width,
                height,
            },
            content_size_px: PhysicalSize { width, height },
            target_fps: 60,
            pixel_format: "BGRA8".into(),
            color_mode: "sdr-srgb".into(),
            cursor_captured_in_video: true,
        },
    );
    let layout = ProjectLayout::create_new(
        &root,
        &manifest,
        &RecordingLock {
            schema_version: 1,
            process_id: std::process::id(),
            session_id: id.to_string(),
            last_checkpoint_tick: TimeTick::ZERO,
        },
    )
    .map_err(|e| e.to_string())?;
    let mut staging = Staging {
        path: root.clone(),
        committed: false,
    };
    let events = ProjectWriter::create(&layout).map_err(|e| e.to_string())?;
    events.finish().map_err(|e| e.to_string())?;
    let device = D3d11Device::create_hardware().map_err(|e| e.to_string())?;
    let converter = Nv12VideoProcessor::create_with_output(
        &device,
        pending.width,
        pending.height,
        width,
        height,
    )
    .map_err(|e| e.to_string())?;
    let raw = root.join("media/screen.part.mp4");
    let media = root.join("media/screen.mp4");
    let mut writer = Nv12Fmp4Writer::create_with_config(
        &raw,
        Nv12VideoConfig::native_screen_60(width, height).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut frames = 0;
    loop {
        checked(cancel)?;
        let next = decoder.read_next().map_err(|e| e.to_string())?;
        let duration = next
            .as_ref()
            .map_or(pending.duration.unwrap_or(TimeTick(166_667)).0, |f| {
                f.pts.0 - pending.pts.0
            });
        if duration > 0 {
            let nv12 = converter
                .convert_bgra_bytes(&pending.pixels)
                .map_err(|e| e.to_string())?;
            writer
                .write_nv12(&nv12, pending.pts.0 - origin.0, duration)
                .map_err(|e| e.to_string())?;
            frames += 1;
            progress.store(
                (frames * 80 / index.len()).min(80) as u32,
                Ordering::Release,
            );
        }
        let Some(next) = next else {
            break;
        };
        if next.width != pending.width || next.height != pending.height {
            return Err("暂不支持中途改变尺寸的视频".into());
        }
        pending = next;
    }
    let report = writer.finish().map_err(|e| e.to_string())?;
    drop(decoder);
    normalize_export_timing(&raw, &media, cancel).map_err(|e| e.to_string())?;
    std::fs::remove_file(&raw).map_err(|e| e.to_string())?;
    let duration = TimeTick(report.duration_tick);
    manifest.media.screen = "media/screen.mp4".into();
    manifest.media.duration_tick = duration.0;
    manifest.state = ProjectState::Processing;
    let store = ManifestStore::new(&root);
    store.persist(&manifest).map_err(|e| e.to_string())?;
    progress.store(85, Ordering::Release);
    if let Some(track) = import_audio(source, &root, origin, duration, cancel)? {
        AudioManifest::save(&root, vec![track])?;
    }
    crate::platform::recorder::ensure_recording_edit_files(
        &layout,
        &[],
        duration,
        manifest.capture.desktop_rect_px,
    )
    .map_err(|e| e.to_string())?;
    let mut settings: panzo_core::WorkbenchSettings = serde_json::from_slice(
        &std::fs::read(root.join("edit/workbench.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    settings.camera_enabled = true;
    settings.automatic_camera = Some(panzo_core::camera_policy::AutoCameraSettings::default());
    settings.cursor.visible = false;
    persist_json_atomically(root.join("edit/workbench.json"), &settings)
        .map_err(|e| e.to_string())?;
    persist_json_atomically(root.join("diagnostics/import.json"),&serde_json::json!({"fileName":source.file_name().unwrap_or_default().to_string_lossy(),"sourceOriginTick":origin,"importedFrames":frames,"hasCursorEvents":false,"audio":"stereo PCM"})).map_err(|e|e.to_string())?;
    checked(cancel)?;
    manifest.state = ProjectState::Ready;
    store.persist(&manifest).map_err(|e| e.to_string())?;
    store.remove_recording_lock().map_err(|e| e.to_string())?;
    let output = library.join(format!("导入-{id}.panzo"));
    std::fs::rename(&root, &output).map_err(|e| e.to_string())?;
    staging.committed = true;
    progress.store(100, Ordering::Release);
    let _ = crate::platform::asset_index::register(library, source);
    let _ = crate::platform::asset_index::register(library, &output);
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires generated tone.mp4 and new PANZO_STAGE_C_IMPORT_DIR; no user capture"]
    #[allow(clippy::too_many_lines)]
    fn stage_c_import_edit_range_export() {
        use crate::platform::{
            audio_decode::AudioDecoder,
            editor::ProjectEditorSession,
            export::{ExportOptions, ExportPreset, ExportRequest, ExportSnapshot, ProjectExporter},
        };
        let root = PathBuf::from(
            std::env::var_os("PANZO_STAGE_C_IMPORT_DIR").expect("isolated test directory"),
        );
        std::fs::create_dir_all(&root).unwrap();
        let source =
            PathBuf::from(std::env::var_os("PANZO_STAGE_C_SOURCE").expect("synthetic input"));
        let original = std::fs::read(&source).unwrap();
        let cancel = AtomicBool::new(false);
        let progress = AtomicU32::new(0);
        let project = import_mp4(&source, &root, &cancel, &progress).unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert_eq!(progress.load(Ordering::Acquire), 100);
        assert_eq!(
            ProjectLayout::open(&project)
                .unwrap()
                .load_manifest()
                .unwrap()
                .capture
                .kind,
            CaptureKind::Imported
        );
        assert!(
            crate::platform::audio::AudioMixer::open(&project)
                .unwrap()
                .has_audio()
        );
        let mut editor = ProjectEditorSession::open(&project).unwrap();
        let id = editor.video_edit().clips[0].id.clone();
        editor
            .trim_video(&id, TimeTick::from_millis(500), TimeTick::from_millis(2500))
            .unwrap();
        editor.set_video_speed(&id, 200).unwrap();
        let mut settings = editor.settings().clone();
        settings.audio.imported_gain = 0.5;
        editor.replace_media_settings(settings).unwrap();
        assert!(editor.undo());
        assert_eq!(editor.settings().audio.imported_gain, 1.0);
        assert!(editor.redo());
        editor
            .save(panzo_core::CameraEditKind::Style, None)
            .unwrap();
        let reopened = ProjectEditorSession::open(&project).unwrap();
        assert_eq!(reopened.settings().audio.imported_gain, 0.5);
        assert_eq!(reopened.project_duration(), TimeTick::from_millis(1000));
        let output = root.join("edited-range.mp4");
        let report = ProjectExporter::export_snapshot_with_options(
            &ExportRequest {
                project_root: project.clone(),
                output_path: output.clone(),
                maximum_duration: None,
            },
            &ExportSnapshot {
                camera: reopened.camera().clone(),
                settings: reopened.settings().clone(),
            },
            ExportPreset::Native,
            ExportOptions {
                fps: 30,
                range: Some((TimeTick::from_millis(200), TimeTick::from_millis(800))),
                ..ExportOptions::default()
            },
            &cancel,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(report.frames_written, 18);
        let mut video = MfBgraDecoder::open(&output).unwrap();
        let mut frames = Vec::new();
        while let Some(f) = video.read_next().unwrap() {
            frames.push(f.pts.0);
        }
        assert_eq!(frames.len(), 18);
        assert_eq!(frames[0], 0);
        assert!(frames.windows(2).all(|v| v[1] > v[0]));
        let mut audio = AudioDecoder::open(&output).unwrap().unwrap();
        let mut samples = Vec::new();
        let mut audio_end = 0;
        while let Some(packet) = audio.read().unwrap() {
            audio_end = packet.tick.0
                + i64::try_from(packet.samples.len() / packet.channels).unwrap() * 10_000_000
                    / i64::from(packet.rate);
            samples.extend(packet.samples);
        }
        let rms = (samples.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>()
            / samples.len() as f64)
            .sqrt();
        assert!((1500.0..3500.0).contains(&rms), "rms {rms}");
        assert!((audio_end - report.duration_tick.0).abs() < 1_000_000);
        cancel.store(true, Ordering::Release);
        let cancelled = import_mp4(&source, &root, &cancel, &progress).unwrap_err();
        assert!(cancelled.contains("取消"));
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert_eq!(
            std::fs::read_dir(root.join(".panzo-imports"))
                .unwrap()
                .count(),
            0
        );
        std::fs::write(root.join("import-export-proof.json"),serde_json::to_vec_pretty(&serde_json::json!({"project":project,"frames":frames.len(),"durationTick":report.duration_tick.0,"audioEndTick":audio_end,"rms":rms,"originalUnchanged":true,"cancelCleanup":true})).unwrap()).unwrap();
    }
}

#[cfg(test)]
#[path = "stage_d_video_review.rs"]
mod stage_d_video_review;
