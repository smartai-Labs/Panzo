use anyhow::Result;
use panzo_app::default_project_library;
use panzo_windows::platform::camera_regeneration::CameraTrackRegenerator;
use panzo_windows::platform::capture_probe::CaptureProbe;
use panzo_windows::platform::compositor::RenderHashProbe;
use panzo_windows::platform::editor::{EditorAcceptanceProbe, EditorAtomicProbe};
use panzo_windows::platform::editor_transaction::EditorTransactionStage;
use panzo_windows::platform::export::{ExportRequest, ProjectExporter};
use panzo_windows::platform::fmp4_writer::{Fmp4WriterProbe, inspect_fmp4};
use panzo_windows::platform::input_capture::InputCaptureService;
use panzo_windows::platform::media_decoder::DecoderProbe;
use panzo_windows::platform::media_foundation::EncoderProbe;
use panzo_windows::platform::preflight::PreflightReport;
use panzo_windows::platform::preview::PreviewSessionProbe;
use panzo_windows::platform::preview_window::PreviewWindow;
use panzo_windows::platform::recorder::{CaptureRecorder, PrimaryMonitorRecorder, RecordingSource};
use panzo_windows::platform::recorder_window::RecorderWindow;
use panzo_windows::platform::recovery::RecoveryScanner;
use panzo_windows::platform::stability::StabilityProbe;
use panzo_windows::platform::stability_stimulus::StabilityStimulus;
use panzo_windows::platform::video_processor::VideoProcessorProbe;
use panzo_windows::platform::window_capture::{
    enumerate_capturable_windows, find_capturable_window,
};
use std::time::Duration;

#[allow(clippy::too_many_lines)]
fn main() -> Result<()> {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("recording-editing-probe") => {
            let source = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: panzo-cli recording-editing-probe <source.panzo> <output-directory>"
                )
            })?;
            let output = std::env::args()
                .nth(3)
                .ok_or_else(|| anyhow::anyhow!("missing output directory"))?;
            let report = panzo_windows::platform::recording_editing_probe::exercise(
                std::path::Path::new(&source),
                std::path::Path::new(&output),
            )
            .map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Some("preview-proxy-probe") => {
            let source = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!("usage: panzo-cli preview-proxy-probe <source.mp4>")
            })?;
            let start = std::time::Instant::now();
            let job = panzo_windows::platform::preview_proxy::ProxyJob::start(
                std::path::Path::new(&source),
            )
            .map_err(anyhow::Error::msg)?;
            let mut previous = u32::MAX;
            loop {
                if let Some(result) = job.result() {
                    let proxy = result.map_err(anyhow::Error::msg)?;
                    println!(
                        "Proxy ready: {}\nSource geometry: {:?}\nFrames: {}\nPreparation elapsed ms: {}",
                        proxy.path.display(),
                        proxy.native_size,
                        proxy.mapping.len(),
                        start.elapsed().as_millis()
                    );
                    break;
                }
                let progress = job.progress();
                if previous != progress {
                    println!("Preparing preview: {progress}%");
                    previous = progress;
                }
                if start.elapsed() > Duration::from_hours(1) {
                    job.cancel();
                    anyhow::bail!("preview preparation exceeded one hour");
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        Some("capture-probe") => {
            let requested_frames = std::env::args()
                .nth(2)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(10);
            let report = CaptureProbe::primary_monitor(requested_frames, Duration::from_secs(5))?;
            println!("{report}");
        }
        Some("encoder-probe") => {
            let report = EncoderProbe::hardware_h264()?;
            println!("{report}");
        }
        Some("video-probe") => {
            let report = VideoProcessorProbe::bgra_to_nv12_1080p()?;
            println!("{report}");
        }
        Some("fmp4-probe") => {
            let output = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!("usage: panzo-cli fmp4-probe <output.mp4> [seconds]")
            })?;
            let duration_seconds = std::env::args()
                .nth(3)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(2);
            let report = Fmp4WriterProbe::write_black_video(output, duration_seconds)?;
            println!("{report}");
        }
        Some("fmp4-inspect") => {
            let input = std::env::args()
                .nth(2)
                .ok_or_else(|| anyhow::anyhow!("usage: panzo-cli fmp4-inspect <input.mp4>"))?;
            let report = inspect_fmp4(input)?;
            println!("{report:#?}");
        }
        Some("decode-probe") => {
            let input = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!("usage: panzo-cli decode-probe <input.mp4> [frames]")
            })?;
            let maximum_frames = std::env::args()
                .nth(3)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(120_u32);
            let report = DecoderProbe::inspect(input, maximum_frames)?;
            println!("{report}");
        }
        Some("decode-seek-probe") => {
            let input = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!("usage: panzo-cli decode-seek-probe <input.mp4> [milliseconds]")
            })?;
            let milliseconds = std::env::args()
                .nth(3)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(1_000_i64);
            let report =
                DecoderProbe::inspect_seek(input, panzo_core::TimeTick::from_millis(milliseconds))?;
            println!("{report}");
        }
        Some("render-hash-probe") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!("usage: panzo-cli render-hash-probe <project.panzo> [milliseconds]")
            })?;
            let milliseconds = std::env::args()
                .nth(3)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(10_000_i64);
            let output = panzo_core::OutputDescriptor::bgra8(1_920, 1_080)?;
            let report = RenderHashProbe::project_frame(
                project_root,
                panzo_core::TimeTick::from_millis(milliseconds),
                output,
            )?;
            println!("{report}");
        }
        Some("preview-session-probe") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: panzo-cli preview-session-probe <project.panzo> [seek-tick]"
                )
            })?;
            let output = panzo_core::OutputDescriptor::bgra8(960, 540)?;
            let report = if let Some(seek_tick) = std::env::args().nth(3) {
                PreviewSessionProbe::inspect_at(
                    project_root,
                    output,
                    panzo_core::TimeTick(seek_tick.parse()?),
                )?
            } else {
                PreviewSessionProbe::inspect(project_root, output)?
            };
            println!("{report}");
        }
        Some("editor" | "preview") => {
            let project_root = std::env::args()
                .nth(2)
                .ok_or_else(|| anyhow::anyhow!("usage: panzo-cli editor <project.panzo>"))?;
            PreviewWindow::run(project_root)?;
        }
        Some("editor-acceptance-probe") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!("usage: panzo-cli editor-acceptance-probe <acceptance-copy.panzo>")
            })?;
            let report = EditorAcceptanceProbe::exercise(project_root)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Some("editor-atomic-inspect") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!("usage: panzo-cli editor-atomic-inspect <project.panzo>")
            })?;
            let report = EditorAtomicProbe::inspect(project_root)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Some("editor-atomic-save-probe") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!("usage: panzo-cli editor-atomic-save-probe <project.panzo>")
            })?;
            let report = EditorAtomicProbe::save(project_root)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Some("editor-atomic-crash-probe") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: panzo-cli editor-atomic-crash-probe <project.panzo> <stage>"
                )
            })?;
            let stage: EditorTransactionStage = std::env::args()
                .nth(3)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "usage: panzo-cli editor-atomic-crash-probe <project.panzo> <stage>"
                    )
                })?
                .parse()?;
            EditorAtomicProbe::crash(project_root, stage)?;
            anyhow::bail!("editor atomic crash probe did not terminate at {stage}");
        }
        Some("preview-smoke") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!("usage: panzo-cli preview-smoke <project.panzo> [milliseconds]")
            })?;
            let milliseconds = std::env::args()
                .nth(3)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(1_500_u64);
            let report = PreviewWindow::smoke(project_root, Duration::from_millis(milliseconds))?;
            println!("{report}");
        }
        Some("preview-paused-smoke") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: panzo-cli preview-paused-smoke <project.panzo> [milliseconds]"
                )
            })?;
            let milliseconds = std::env::args()
                .nth(3)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(1_500_u64);
            let report =
                PreviewWindow::smoke_paused(project_root, Duration::from_millis(milliseconds))?;
            println!("{report}");
        }
        Some("preview-trim-smoke") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: panzo-cli preview-trim-smoke <project.panzo> [milliseconds]"
                )
            })?;
            let milliseconds = std::env::args()
                .nth(3)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(1_500_u64);
            let report =
                PreviewWindow::smoke_trim(project_root, Duration::from_millis(milliseconds))?;
            println!("{report}");
            anyhow::ensure!(
                report.trim_status.passed_incrementally(),
                "Editor trim smoke did not complete through the incremental preview path"
            );
        }
        Some("preview-queued-playback-smoke") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: panzo-cli preview-queued-playback-smoke <project.panzo> [milliseconds]"
                )
            })?;
            let milliseconds = std::env::args()
                .nth(3)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(2_000_u64);
            let report = PreviewWindow::smoke_queued_playback(
                project_root,
                Duration::from_millis(milliseconds),
            )?;
            println!("{report}");
            anyhow::ensure!(
                report.queued_playback_completed,
                "Editor queued playback did not start before the smoke deadline"
            );
        }
        Some("preview-scrub-smoke") => {
            let project = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: panzo-cli preview-scrub-smoke <project.panzo> [milliseconds >= 4000]"
                )
            })?;
            let milliseconds = std::env::args()
                .nth(3)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(5_000_u64);
            let report = PreviewWindow::smoke_scrub(project, Duration::from_millis(milliseconds))?;
            println!("{report}");
            anyhow::ensure!(
                report.scrub_presented_frames >= 10
                    && report.scrub_distinct_frames >= 10
                    && report.scrub_reduced_frames == 0
                    && report.scrub_max_time_error_tick <= 3_000_000
                    && report.scrub_metrics.p95_us <= 150_000
                    && report.scrub_metrics.max_us <= 300_000
                    && report.scrub_metrics.time_error_p95_tick <= 1_500_000
                    && report.scrub_max_gap_micros <= 300_000
                    && report.scrub_settled
                    && report.queued_playback_completed
                    && report.media_errors == 0,
                "continuous scrub did not present frames and settle exactly without errors"
            );
        }
        Some("preview-lifecycle-smoke") => {
            let project = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!("usage: panzo-cli preview-lifecycle-smoke <project.panzo> [count]")
            })?;
            let count = std::env::args()
                .nth(3)
                .map(|v| v.parse::<u32>())
                .transpose()?
                .unwrap_or(20);
            anyhow::ensure!((1..=100).contains(&count), "count must be 1..100");
            for index in 0..count {
                let report = PreviewWindow::smoke_paused(&project, Duration::from_millis(250))?;
                anyhow::ensure!(
                    report.window_created && report.graceful_close && report.media_errors == 0,
                    "lifecycle iteration failed"
                );
                println!("Open/close {} / {}: passed", index + 1, count);
            }
        }
        Some("window-stimulus") => {
            let seconds = std::env::args()
                .nth(2)
                .unwrap_or_else(|| "10".into())
                .parse::<u64>()?;
            let width = std::env::args()
                .nth(3)
                .unwrap_or_else(|| "1201".into())
                .parse::<i32>()?;
            let height = std::env::args()
                .nth(4)
                .unwrap_or_else(|| "901".into())
                .parse::<i32>()?;
            println!(
                "{}",
                StabilityStimulus::run_windowed(Duration::from_secs(seconds), width, height)?
            );
        }
        Some("preview-reopen-smoke") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: panzo-cli preview-reopen-smoke <project.panzo> [milliseconds]"
                )
            })?;
            let milliseconds = std::env::args()
                .nth(3)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(500_u64);
            let duration = Duration::from_millis(milliseconds);
            let first = PreviewWindow::smoke_paused(&project_root, duration)?;
            let second = PreviewWindow::smoke_paused(&project_root, duration)?;
            println!("First open:\n{first}\nSecond open:\n{second}");
        }
        Some("camera-regenerate-probe") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!("usage: panzo-cli camera-regenerate-probe <project.panzo>")
            })?;
            let report = CameraTrackRegenerator::regenerate(project_root)?;
            println!("{report}");
        }
        Some("export-probe") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: panzo-cli export-probe <project.panzo> <output.mp4> [seconds]"
                )
            })?;
            let output_path = std::env::args().nth(3).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: panzo-cli export-probe <project.panzo> <output.mp4> [seconds]"
                )
            })?;
            let maximum_duration = std::env::args()
                .nth(4)
                .map(|value| value.parse::<i64>())
                .transpose()?
                .map(|seconds| panzo_core::TimeTick(seconds * panzo_core::TICKS_PER_SECOND));
            let report = ProjectExporter::export(&ExportRequest {
                project_root: project_root.into(),
                output_path: output_path.into(),
                maximum_duration,
            })?;
            println!("{report}");
        }
        Some("input-probe") => {
            let duration_seconds = std::env::args()
                .nth(2)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(2_u64);
            let mut service = InputCaptureService::start()?;
            std::thread::sleep(Duration::from_secs(duration_seconds));
            service.stop()?;
            let stats = service.stats();
            let events = service.drain();
            println!("Panzo M1 input probe");
            println!("  Sample attempts: {}", stats.sample_attempts);
            println!("  Cursor read failures: {}", stats.sample_read_failures);
            println!("  Raw events: {}", events.len());
            println!("  Queue overflow: {}", stats.overflowed);
        }
        Some("record-probe") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!("usage: panzo-cli record-probe <project.panzo> [seconds]")
            })?;
            let duration_seconds = std::env::args()
                .nth(3)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(5_u64);
            let report = PrimaryMonitorRecorder::record(
                project_root,
                Duration::from_secs(duration_seconds),
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Some("window-list") => {
            println!("Panzo capturable windows");
            for target in enumerate_capturable_windows()? {
                println!(
                    "  0x{:X}\tpid={}\t{}x{} @ {},{}\t{}",
                    target.raw_handle(),
                    target.process_id,
                    target.desktop_rect.width,
                    target.desktop_rect.height,
                    target.desktop_rect.x,
                    target.desktop_rect.y,
                    target.title
                );
            }
        }
        Some("window-record-probe") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: panzo-cli window-record-probe <project.panzo> <title-substring> [seconds]"
                )
            })?;
            let title_substring = std::env::args().nth(3).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: panzo-cli window-record-probe <project.panzo> <title-substring> [seconds]"
                )
            })?;
            let duration_seconds = std::env::args()
                .nth(4)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(5_u64);
            let target = find_capturable_window(&title_substring)?;
            let source = RecordingSource::Window(target);
            let report = CaptureRecorder::record(
                project_root,
                Duration::from_secs(duration_seconds),
                &source,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Some("stability-probe") => {
            let project_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: panzo-cli stability-probe <project.panzo> [required-seconds]"
                )
            })?;
            let required_seconds = std::env::args()
                .nth(3)
                .map(|value| value.parse::<i64>())
                .transpose()?
                .unwrap_or(30 * 60);
            let required_duration_tick = required_seconds
                .checked_mul(panzo_core::TICKS_PER_SECOND)
                .ok_or_else(|| anyhow::anyhow!("required duration is too large"))?;
            let report = StabilityProbe::inspect(project_root, required_duration_tick)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !report.acceptance.passed {
                anyhow::bail!("M4 stability thresholds were not met");
            }
        }
        Some("stability-stimulus") => {
            let duration_seconds = std::env::args()
                .nth(2)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(30_u64);
            let report = StabilityStimulus::run(Duration::from_secs(duration_seconds))?;
            println!("{report}");
        }
        Some("recorder") => {
            let project_library = std::env::args()
                .nth(2)
                .map_or_else(default_project_library, std::path::PathBuf::from);
            RecorderWindow::run(project_library)?;
        }
        Some("recorder-smoke") => {
            let project_library = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!("usage: panzo-cli recorder-smoke <project-library> [milliseconds]")
            })?;
            let milliseconds = std::env::args()
                .nth(3)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(2_000_u64);
            let report =
                RecorderWindow::smoke(project_library, Duration::from_millis(milliseconds))?;
            println!("{report}");
        }
        Some("recorder-open-last-smoke") => {
            let project_library = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!(
                    "usage: panzo-cli recorder-open-last-smoke <project-library> [milliseconds]"
                )
            })?;
            let milliseconds = std::env::args()
                .nth(3)
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(500_u64);
            let report = RecorderWindow::smoke_open_last(
                project_library,
                Duration::from_millis(milliseconds),
            )?;
            println!("{report}");
        }
        Some("recovery-scan") => {
            let library_root = std::env::args().nth(2).ok_or_else(|| {
                anyhow::anyhow!("usage: panzo-cli recovery-scan <project-library>")
            })?;
            let report = RecoveryScanner::new(library_root).scan_and_recover()?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        None | Some(_) => {
            let report = PreflightReport::collect()?;
            println!("{report}");
            println!();
            println!("Run panzo-cli capture-probe [frame-count] for the WGC M0 probe.");
            println!("Run panzo-cli encoder-probe for the Media Foundation M0 probe.");
            println!("Run panzo-cli video-probe for the D3D11 BGRA-to-NV12 M0 probe.");
            println!("Run panzo-cli fmp4-probe <output.mp4> [seconds] for the fMP4 M0 probe.");
            println!(
                "Run panzo-cli decode-probe <input.mp4> [frames] for the M3 BGRA decoder probe."
            );
            println!(
                "Run panzo-cli decode-seek-probe <input.mp4> [milliseconds] for the M3 seek probe."
            );
            println!(
                "Run panzo-cli render-hash-probe <project.panzo> [milliseconds] for IT-RENDER-001."
            );
            println!(
                "Run panzo-cli preview-session-probe <project.panzo> [seek-tick] for the M3 interactive session probe."
            );
            println!("Run panzo-cli editor <project.panzo> for the Editor Workbench.");
            println!(
                "Run panzo-cli editor-acceptance-probe <acceptance-copy.panzo> for non-destructive edit persistence acceptance."
            );
            println!(
                "Run panzo-cli preview-smoke <project.panzo> [milliseconds] for an automated UI smoke."
            );
            println!(
                "Run panzo-cli preview-paused-smoke <project.panzo> [milliseconds] for paused repaint regression."
            );
            println!(
                "Run panzo-cli preview-trim-smoke <project.panzo> [milliseconds] for non-blocking Camera Segment trim regression."
            );
            println!(
                "Run panzo-cli preview-reopen-smoke <project.panzo> [milliseconds] for same-process Editor reopen regression."
            );
            println!(
                "Run panzo-cli camera-regenerate-probe <project.panzo> for atomic camera regeneration."
            );
            println!(
                "Run panzo-cli export-probe <project.panzo> <output.mp4> [seconds] for IT-EXPORT-001."
            );
            println!("Run panzo-cli input-probe [seconds] for the M1 input probe.");
            println!(
                "Run panzo-cli record-probe <project.panzo> [seconds] for the M1 recording probe."
            );
            println!("Run panzo-cli window-list to enumerate capturable top-level windows.");
            println!(
                "Run panzo-cli window-record-probe <project.panzo> <title-substring> [seconds] for Window Capture."
            );
            println!(
                "Run panzo-cli stability-probe <project.panzo> [required-seconds] for M4 threshold evaluation."
            );
            println!(
                "Run panzo-cli stability-stimulus [seconds] for the M4 full-screen 60 Hz animation source."
            );
            println!("Run panzo-cli recorder [project-library] for the V0.1 Recorder Window.");
            println!(
                "Run panzo-cli recorder-smoke <project-library> [milliseconds] for its automated UI acceptance."
            );
            println!(
                "Run panzo-cli recorder-open-last-smoke <project-library> [milliseconds] for restart discovery acceptance."
            );
            println!("Run panzo-cli recovery-scan <library-root> for M1 startup recovery.");
        }
    }
    Ok(())
}
