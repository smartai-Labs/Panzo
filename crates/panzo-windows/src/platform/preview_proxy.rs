//! Disposable, short-GOP local scrub media. Never used for exact seek or export.
//! One builder per process; capped disk use, cooperative cancellation, no UI-thread join.
use crate::platform::d3d11::D3d11Device;
use crate::platform::fmp4_writer::{Nv12Fmp4Writer, Nv12VideoConfig};
use crate::platform::media_decoder::{DecodedBgraFrame, MfBgraDecoder};
use crate::platform::video_processor::Nv12VideoProcessor;
use panzo_core::TimeTick;
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::UNIX_EPOCH;

const MAX_BYTES: u64 = 512 * 1024 * 1024;
const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;
const FRAME_STEP: i64 = 333_333; // At most 30 proxy frames/sec; source timing is retained.
static BUILDING: AtomicBool = AtomicBool::new(false);
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        BUILDING.store(false, Ordering::Release);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewProxy {
    pub path: PathBuf,
    pub native_size: (u32, u32),
    /// Encoded (container-quantized) PTS and its original source PTS.
    pub mapping: Vec<(TimeTick, TimeTick)>,
    source_size: u64,
    source_modified_ns: u128,
}

impl PreviewProxy {
    pub fn proxy_time(&self, source: TimeTick) -> TimeTick {
        let index = self
            .mapping
            .partition_point(|(_, pts)| *pts <= source)
            .saturating_sub(1);
        self.mapping[index].0
    }

    pub fn restore_source_time(&self, frame: &mut DecodedBgraFrame) -> Result<(), String> {
        if (frame.width, frame.height) != self.native_size {
            return Err("预览缓存尺寸不匹配，需要重新生成".into());
        }
        let index = self
            .mapping
            .binary_search_by_key(&frame.pts, |(pts, _)| *pts)
            .map_err(|insertion| {
                format!(
                    "proxy timestamp {} is not in its source mapping (previous={:?}, next={:?})",
                    frame.pts.as_i64(),
                    insertion
                        .checked_sub(1)
                        .and_then(|i| self.mapping.get(i))
                        .map(|p| p.0.as_i64()),
                    self.mapping.get(insertion).map(|p| p.0.as_i64()),
                )
            })?;
        frame.pts = self.mapping[index].1;
        frame.native_size = Some(self.native_size);
        Ok(())
    }
}

#[derive(Default)]
struct State {
    cancel: AtomicBool,
    paused: AtomicBool,
    pause_lock: Mutex<()>,
    wake: Condvar,
    progress: AtomicU32,
    result: Mutex<Option<Result<Arc<PreviewProxy>, String>>>,
}

pub struct ProxyJob(Arc<State>);
impl ProxyJob {
    pub fn start(source: &Path) -> Result<Self, String> {
        let state = Arc::new(State::default());
        let worker = Arc::clone(&state);
        let source = source.to_owned();
        std::thread::Builder::new()
            .name("panzo-scrub-proxy".into())
            .spawn(move || {
                let result = load_or_build(&source, &worker).map(Arc::new);
                *worker
                    .result
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
            })
            .map_err(|error| error.to_string())?;
        Ok(Self(state))
    }

    pub fn progress(&self) -> u32 {
        self.0.progress.load(Ordering::Acquire)
    }
    pub fn result(&self) -> Option<Result<Arc<PreviewProxy>, String>> {
        self.0
            .result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    pub fn cancel(&self) {
        self.0.cancel.store(true, Ordering::Release);
        self.0.wake.notify_all();
    }
    pub fn pause_preparation(&self, paused: bool) {
        self.0.paused.store(paused, Ordering::Release);
        if !paused {
            self.0.wake.notify_all();
        }
    }
    pub fn is_paused(&self) -> bool {
        self.0.paused.load(Ordering::Acquire)
    }
}
impl Drop for ProxyJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn fingerprint(source: &Path) -> Result<(u64, u128), String> {
    let metadata = fs::metadata(source).map_err(|e| e.to_string())?;
    let modified = metadata
        .modified()
        .map_err(|e| e.to_string())?
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    Ok((metadata.len(), modified))
}

fn load_or_build(source: &Path, state: &State) -> Result<PreviewProxy, String> {
    let (source_size, source_modified_ns) = fingerprint(source)?;
    let directory = source
        .parent()
        .and_then(Path::parent)
        .ok_or("source has no project directory")?
        .join("cache")
        // Previous proxies used implicit matrix/range conversion. Rebuild rather
        // than displaying their darker pixels during a drag and changing on release.
        .join("scrub-v3-native-bt709");
    let key = format!("{source_size:x}-{source_modified_ns:x}");
    let index_path = directory.join(format!("{key}.json"));
    let video_path = directory.join(format!("{key}.mp4"));
    if let Ok(metadata) = fs::metadata(&index_path)
        && metadata.len() <= MAX_INDEX_BYTES
        && let Ok(bytes) = fs::read(&index_path)
        && let Ok(mut proxy) = serde_json::from_slice::<PreviewProxy>(&bytes)
        && proxy.source_size == source_size
        && proxy.source_modified_ns == source_modified_ns
        && !proxy.mapping.is_empty()
        && proxy
            .mapping
            .windows(2)
            .all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1)
        && fs::metadata(&video_path).is_ok_and(|m| m.len() > 0 && m.len() <= MAX_BYTES)
    {
        proxy.path = video_path; // Never trust a path supplied in a cache JSON.
        state.progress.store(100, Ordering::Release);
        return Ok(proxy);
    }
    if BUILDING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("其他工程正在准备预览代理，本工程继续使用原素材".into());
    }
    let _permit = Permit;
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let project = source
        .parent()
        .and_then(Path::parent)
        .ok_or("项目路径无效")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !directory
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(project)
    {
        return Err("预览缓存目录不可指向工程外部".into());
    }
    // OS-owned lock is released even after a process crash. A bad derived cache
    // must be rebuildable; never remove a file named by untrusted cache JSON.
    let _lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(directory.join("build.lock"))
        .map_err(|_| "另一个进程正在准备预览；继续使用原素材")?;
    for invalid in [&index_path, &video_path] {
        if invalid.exists() {
            fs::remove_file(invalid).map_err(|e| e.to_string())?;
        }
    }
    // Own only these uniquely named files. Cancellation cannot remove another job's output.
    let temporary = directory.join(format!("{}.part.mp4", uuid::Uuid::new_v4()));
    let temporary_index = directory.join(format!("{}.part.json", uuid::Uuid::new_v4()));
    let result = build(source, &temporary, source_size, source_modified_ns, state);
    let result = result.and_then(|mut proxy| {
        check_cancel(state)?;
        if fingerprint(source)? != (source_size, source_modified_ns) {
            return Err("源素材在预览准备期间发生变化".into());
        }
        proxy.path.clone_from(&video_path);
        fs::write(
            &temporary_index,
            serde_json::to_vec(&proxy).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        // Already-published cache is immutable. Another process may have won the race.
        fs::rename(&temporary, &video_path).map_err(|e| e.to_string())?;
        fs::rename(&temporary_index, &index_path).map_err(|e| e.to_string())?;
        state.progress.store(100, Ordering::Release);
        Ok(proxy)
    });
    let _ = fs::remove_file(&temporary);
    let _ = fs::remove_file(&temporary_index);
    result
}

fn check_cancel(state: &State) -> Result<(), String> {
    // Background optimization must yield to playback / pointer interaction. The
    // wait owns no GPU/context lock and is cancellable even while the UI is closed.
    let mut guard = state
        .pause_lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    while state.paused.load(Ordering::Acquire) && !state.cancel.load(Ordering::Acquire) {
        guard = state
            .wake
            .wait_timeout(guard, std::time::Duration::from_millis(100))
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0;
    }
    if state.cancel.load(Ordering::Acquire) {
        Err("预览代理准备已取消".into())
    } else {
        Ok(())
    }
}

fn build(
    source: &Path,
    temporary: &Path,
    source_size: u64,
    source_modified_ns: u128,
    state: &State,
) -> Result<PreviewProxy, String> {
    check_cancel(state)?;
    let index = MfBgraDecoder::native_index(source, || state.cancel.load(Ordering::Acquire))
        .map_err(|e| e.to_string())?;
    let last = index.last().ok_or("source has no frames")?;
    let total = last
        .pts
        .as_i64()
        .saturating_add(last.duration.unwrap_or(TimeTick(FRAME_STEP)).as_i64())
        .max(1);
    // Spatial resolution must not drop as soon as the user touches the timeline.
    // Keep the source dimensions and native screen bitrate; bound cache size instead.
    let mut decoder = MfBgraDecoder::open(source).map_err(|e| e.to_string())?;
    let device = D3d11Device::create_hardware().map_err(|e| e.to_string())?;
    let converter = Nv12VideoProcessor::create_with_output(
        &device,
        decoder.width(),
        decoder.height(),
        decoder.width(),
        decoder.height(),
    )
    .map_err(|e| e.to_string())?;
    let mut config = Nv12VideoConfig::native_screen_60(decoder.width(), decoder.height())
        .map_err(|e| e.to_string())?;
    config.frames_per_second = 30;
    config.gop_frames = 3;
    let mut writer =
        Nv12Fmp4Writer::create_with_config(temporary, config).map_err(|e| e.to_string())?;
    let mut pending: Option<DecodedBgraFrame> = None;
    let mut sources = Vec::new();
    let mut native_size = (0, 0);
    let mut next_tick = 0_i64;
    loop {
        check_cancel(state)?;
        let Some(frame) = decoder.read_next().map_err(|e| e.to_string())? else {
            break;
        };
        if frame.pts.as_i64() < next_tick {
            continue;
        }
        next_tick = (frame.pts.as_i64() / FRAME_STEP + 1) * FRAME_STEP;
        if let Some(previous) = pending.take() {
            let nv12 = converter
                .convert_bgra_bytes(&previous.pixels)
                .map_err(|e| e.to_string())?;
            writer
                .write_nv12(
                    &nv12,
                    previous.pts.as_i64(),
                    frame.pts.as_i64() - previous.pts.as_i64(),
                )
                .map_err(|e| e.to_string())?;
            sources.push(previous.pts);
        }
        native_size = frame.native_size.unwrap_or((frame.width, frame.height));
        state.progress.store(
            u32::try_from((frame.pts.as_i64().saturating_mul(100) / total).clamp(0, 99))
                .unwrap_or(0),
            Ordering::Release,
        );
        pending = Some(frame);
        if sources.len() % 120 == 0 && fs::metadata(temporary).is_ok_and(|m| m.len() > MAX_BYTES) {
            return Err("预览代理超过 512 MiB 预算，继续使用原素材".into());
        }
    }
    let last = pending.ok_or("source has no preview frames")?;
    let nv12 = converter
        .convert_bgra_bytes(&last.pixels)
        .map_err(|e| e.to_string())?;
    writer
        .write_nv12(&nv12, last.pts.as_i64(), (total - last.pts.as_i64()).max(1))
        .map_err(|e| e.to_string())?;
    sources.push(last.pts);
    check_cancel(state)?;
    let report = writer.finish().map_err(|e| e.to_string())?;
    if report.output_bytes > MAX_BYTES {
        return Err("预览代理超过磁盘预算".into());
    }
    let proxy_frames =
        MfBgraDecoder::native_index(temporary, || state.cancel.load(Ordering::Acquire))
            .map_err(|e| e.to_string())?;
    if sources.len() != proxy_frames.len() {
        return Err("预览代理帧数与源时间映射不一致".into());
    }
    Ok(PreviewProxy {
        path: temporary.to_owned(),
        native_size,
        mapping: proxy_frames
            .into_iter()
            .zip(sources)
            .map(|(frame, source)| (frame.pts, source))
            .collect(),
        source_size,
        source_modified_ns,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paused_preparation_resumes_and_cancel_wakes_without_gpu_work() {
        let state = Arc::new(State::default());
        let job = ProxyJob(state.clone());
        job.pause_preparation(true);
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = state.clone();
        let thread = std::thread::spawn(move || {
            tx.send(check_cancel(&worker)).unwrap();
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_millis(20))
                .is_err()
        );
        job.pause_preparation(false);
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(1))
                .unwrap()
                .is_ok()
        );
        thread.join().unwrap();
        job.pause_preparation(true);
        job.cancel();
        assert!(check_cancel(&state).is_err());
    }
    #[test]
    fn container_quantization_maps_back_to_source_without_changing_native_geometry() {
        let proxy = PreviewProxy {
            path: PathBuf::new(),
            native_size: (2560, 1440),
            mapping: vec![
                (TimeTick(0), TimeTick(0)),
                (TimeTick(333_333), TimeTick(333_401)),
                (TimeTick(667_000), TimeTick(667_031)),
            ],
            source_size: 1,
            source_modified_ns: 1,
        };
        assert_eq!(proxy.proxy_time(TimeTick(500_000)), TimeTick(333_333));
        let mut frame = DecodedBgraFrame {
            pts: TimeTick(333_333),
            duration: None,
            width: 2560,
            height: 1440,
            pixels: Arc::new(vec![42; 2560 * 1440 * 4]),
            native_size: None,
        };
        let cached = frame.clone();
        proxy.restore_source_time(&mut frame).unwrap();
        assert!(Arc::ptr_eq(&frame.pixels, &cached.pixels));
        assert_eq!(cached.pts, TimeTick(333_333));
        assert_eq!(frame.pixels.len(), 2560 * 1440 * 4);
        assert_eq!(frame.pts, TimeTick(333_401));
        assert_eq!(frame.native_size, Some((2560, 1440)));
        frame.width = 640;
        assert!(proxy.restore_source_time(&mut frame).is_err());
        frame.width = 2560;
        frame.pts = TimeTick(123);
        assert!(proxy.restore_source_time(&mut frame).is_err());
    }
}
