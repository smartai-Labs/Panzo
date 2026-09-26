//! Bounded, latest-request-wins media service. All MF calls and destruction stay on its
//! owner thread; window handlers only exchange plans and reference-counted frames.
use crate::platform::media_decoder::{DecodedBgraFrame, MfBgraDecoder, TargetFrameRead};
use panzo_core::TimeTick;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const CACHE_BYTES: usize = 64 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
static ACTIVE_WORKERS: AtomicUsize = AtomicUsize::new(0);
const MAX_WORKERS: usize = 8;

#[derive(Clone, Debug)]
pub struct DecodePlan {
    pub project_tick: TimeTick,
    pub target_pts: TimeTick,
    /// Earlier indexed anchors, ending at stream start, for exact-seek recovery.
    pub fallback_pts: Vec<TimeTick>,
    pub exact: bool,
    /// Playback-only read-ahead, capped by the caller to two indexed frames.
    /// These pixels enter the same bounded cache; they are never presented unsolicited.
    pub prefetch_until: Option<TimeTick>,
}

#[derive(Debug)]
pub struct DecodeResult {
    pub generation: u64,
    pub project_tick: TimeTick,
    pub submitted_at: Instant,
    pub frame: Result<Arc<DecodedBgraFrame>, String>,
}

struct Request {
    generation: u64,
    submitted_at: Instant,
    plan: DecodePlan,
}

#[derive(Default)]
struct Mailbox {
    pending: Option<Request>,
    result: Option<DecodeResult>,
    generation: u64,
    cancelled_through: u64,
    stopped: bool,
}

impl Mailbox {
    fn submit(&mut self, plan: DecodePlan) -> Result<u64, String> {
        if self.stopped {
            return Err("preview decoder is stopped".into());
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("decoder generation exhausted")?;
        self.pending = Some(Request {
            generation: self.generation,
            submitted_at: Instant::now(),
            plan,
        });
        Ok(self.generation)
    }

    fn cancel(&mut self) {
        self.cancelled_through = self.generation;
        self.pending = None;
        self.result = None;
    }

    fn cancelled(&self, request: &Request) -> bool {
        self.stopped
            || request.generation <= self.cancelled_through
            || (request.plan.exact && request.generation != self.generation)
    }
}

#[derive(Default)]
struct Shared {
    mailbox: Mutex<Mailbox>,
    wake: Condvar,
    completed: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

pub struct DecodeWorker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    proxy: Option<Arc<crate::platform::preview_proxy::ProxyJob>>,
}

struct WorkerPermit;
impl Drop for WorkerPermit {
    fn drop(&mut self) {
        ACTIVE_WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}

impl DecodeWorker {
    pub fn start(path: &Path, preview_limit: Option<u32>) -> Result<Self, String> {
        Self::start_inner(path, preview_limit, false)
    }

    /// Keep spatial detail while using a short-GOP cache for interactive seeking.
    /// Exact seek and playback workers never switch to this derived media.
    pub fn scrub(path: &Path) -> Result<Self, String> {
        Self::start_inner(path, None, true)
    }

    pub fn thumbnails(path: &Path) -> Result<Self, String> {
        Self::start_inner(path, Some(160), false)
    }

    #[allow(clippy::too_many_lines)] // Worker lifetime and cancellation stay in one auditable owner-thread closure.
    fn start_inner(
        path: &Path,
        preview_limit: Option<u32>,
        allow_proxy: bool,
    ) -> Result<Self, String> {
        // A driver call is not safely interruptible. Do not accumulate unlimited detached
        // decoders if repeated window opens encounter a wedged driver.
        ACTIVE_WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_WORKERS).then_some(count + 1)
            })
            .map_err(
                |_| "preview decoder capacity exhausted; close other editors or restart Panzo",
            )?;
        let permit = WorkerPermit;
        let shared = Arc::new(Shared::default());
        let worker_shared = Arc::clone(&shared);
        let path = path.to_owned();
        let proxy = allow_proxy
            .then(|| crate::platform::preview_proxy::ProxyJob::start(&path).ok())
            .flatten()
            .map(Arc::new);
        let worker_proxy = proxy.clone();
        let thread = thread::Builder::new()
            .name("panzo-media-preview".into())
            .spawn(move || {
                let _permit = permit;
                let mut decoder = MfBgraDecoder::open_with_preview_limit(path, preview_limit)
                    .map_err(|error| error.to_string());
                let budget = if preview_limit == Some(160) {
                    2 * 1024 * 1024
                } else if allow_proxy {
                    // Keep the short GOP behind a scrub target, so backward gestures
                    // can reuse decoded native-resolution frames instead of seeking each time.
                    96 * 1024 * 1024
                } else {
                    CACHE_BYTES
                };
                let mut cache = FrameCache::new(budget);
                let mut position = None;
                let mut proxy_info = None;
                loop {
                    let request = {
                        let mut mailbox = lock(&worker_shared.mailbox);
                        while mailbox.pending.is_none() && !mailbox.stopped {
                            mailbox = worker_shared
                                .wake
                                .wait(mailbox)
                                .unwrap_or_else(PoisonError::into_inner);
                        }
                        if mailbox.stopped {
                            break;
                        }
                        mailbox.pending.take().expect("pending request")
                    };
                    if proxy_info.is_none()
                        && let Some(Ok(info)) = worker_proxy.as_ref().and_then(|job| job.result())
                        && let Ok(proxy_decoder) =
                            MfBgraDecoder::open_with_preview_limit(&info.path, preview_limit)
                    {
                        decoder = Ok(proxy_decoder);
                        cache = FrameCache::new(budget);
                        position = None;
                        proxy_info = Some(info);
                    }
                    let mapped_request = proxy_info.as_ref().map(|proxy| Request {
                        generation: request.generation,
                        submitted_at: request.submitted_at,
                        plan: DecodePlan {
                            project_tick: request.plan.project_tick,
                            target_pts: proxy.proxy_time(request.plan.target_pts),
                            fallback_pts: vec![TimeTick::ZERO],
                            exact: false,
                            prefetch_until: None,
                        },
                    });
                    let mut frame = match &mut decoder {
                        Ok(decoder) => decode(
                            decoder,
                            mapped_request.as_ref().unwrap_or(&request),
                            &worker_shared,
                            &mut cache,
                            &mut position,
                        ),
                        Err(error) => Err(error.clone()),
                    };
                    if let Some(proxy) = &proxy_info {
                        frame = frame.and_then(|frame| {
                            // Only metadata is copied; proxy and presented frames share
                            // immutable pixels, while the cache keeps proxy timestamps.
                            let mut mapped = (*frame).clone();
                            proxy.restore_source_time(&mut mapped)?;
                            Ok(Arc::new(mapped))
                        });
                    }
                    let can_prefetch = frame.is_ok() && proxy_info.is_none();
                    let mut mailbox = lock(&worker_shared.mailbox);
                    if !mailbox.cancelled(&request) {
                        mailbox.result = Some(DecodeResult {
                            generation: request.generation,
                            project_tick: request.plan.project_tick,
                            submitted_at: request.submitted_at,
                            frame,
                        });
                        worker_shared.wake.notify_all();
                        drop(mailbox);
                        if let Some(notify) = lock(&worker_shared.completed).as_ref() {
                            notify();
                        }
                        if can_prefetch && let Ok(decoder) = &mut decoder {
                            prefetch(decoder, &request, &worker_shared, &mut cache, &mut position);
                        }
                    }
                }
                // Decoder / MF / apartment are released here, never by the window destructor.
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            shared,
            thread: Some(thread),
            proxy,
        })
    }

    pub fn proxy_status(&self) -> Option<String> {
        self.proxy.as_ref().map(|job| match job.result() {
            Some(Ok(_)) => "流畅预览已就绪 · 精确定位与导出使用原素材".into(),
            Some(Err(error)) => format!("预览使用原素材：{error}"),
            None if job.is_paused() => "优先播放 / 拖动 · 暂停后继续准备流畅预览".into(),
            None => format!("正在准备流畅预览 {}% · 可继续编辑", job.progress()),
        })
    }

    pub fn pause_proxy_preparation(&self, paused: bool) {
        if let Some(proxy) = &self.proxy {
            proxy.pause_preparation(paused);
        }
    }

    pub fn ready_proxy(&self) -> Option<Arc<crate::platform::preview_proxy::PreviewProxy>> {
        self.proxy.as_ref()?.result()?.ok()
    }

    pub fn submit(&self, plan: DecodePlan) -> Result<u64, String> {
        let generation = lock(&self.shared.mailbox).submit(plan)?;
        self.shared.wake.notify_all();
        Ok(generation)
    }

    pub fn cancel(&self) {
        lock(&self.shared.mailbox).cancel();
    }

    pub fn set_completion_callback(&self, callback: impl Fn() + Send + Sync + 'static) {
        *lock(&self.shared.completed) = Some(Arc::new(callback));
    }

    pub fn take_latest(&self) -> Option<DecodeResult> {
        lock(&self.shared.mailbox).result.take()
    }

    /// Only for pre-window initialization / command-line probes, not interactive handlers.
    pub fn wait(&self, generation: u64) -> Result<Arc<DecodedBgraFrame>, String> {
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let mut mailbox = lock(&self.shared.mailbox);
        loop {
            if let Some(result) = mailbox.result.take()
                && result.generation == generation
            {
                return result.frame;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || mailbox.stopped {
                mailbox.cancel();
                return Err("preview decode timed out (5 seconds)".into());
            }
            mailbox = self
                .shared
                .wake
                .wait_timeout(mailbox, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl Drop for DecodeWorker {
    fn drop(&mut self) {
        if let Some(proxy) = &self.proxy {
            proxy.cancel();
        }
        let mut mailbox = lock(&self.shared.mailbox);
        mailbox.stopped = true;
        mailbox.cancel();
        drop(mailbox);
        self.shared.wake.notify_all();
        if let Some(thread) = self.thread.take()
            && thread.is_finished()
        {
            let _ = thread.join();
        }
        // Otherwise detach: its owned state stays alive until the in-flight MF call returns.
    }
}

fn prefetch(
    decoder: &mut MfBgraDecoder,
    request: &Request,
    shared: &Shared,
    cache: &mut FrameCache,
    position: &mut Option<TimeTick>,
) {
    let Some(until) = request.plan.prefetch_until else {
        return;
    };
    // Publish the requested frame first. Incoming input/pause cancels read-ahead at
    // sample boundaries. No recursive jobs, extra queues, or UI-thread waits.
    for _ in 0..2 {
        if position.is_none_or(|pts| pts >= until) || lock(&shared.mailbox).cancelled(request) {
            break;
        }
        if let Ok(Some(frame)) = decoder.read_next() {
            *position = Some(frame.pts);
            cache.insert(Arc::new(frame));
        } else {
            *position = None;
            break;
        }
    }
}

fn decode(
    decoder: &mut MfBgraDecoder,
    request: &Request,
    shared: &Shared,
    cache: &mut FrameCache,
    position: &mut Option<TimeTick>,
) -> Result<Arc<DecodedBgraFrame>, String> {
    let check = || -> Result<(), String> {
        if lock(&shared.mailbox).cancelled(request) {
            return Err("preview request cancelled".into());
        }
        if request.submitted_at.elapsed() > REQUEST_TIMEOUT {
            return Err("preview decode timed out (5 seconds)".into());
        }
        Ok(())
    };
    check()?;
    let plan = &request.plan;
    if let Some(frame) = cache.get(plan.target_pts) {
        return Ok(frame);
    }
    let sequential = position.is_some_and(|pts| {
        pts < plan.target_pts && plan.target_pts.as_i64() - pts.as_i64() <= 2_000_000
    });
    let retain_preroll = !plan.exact && position.is_some_and(|pts| pts > plan.target_pts);
    let mut last_observed = None;
    'recovery: for recovery in 0..2 {
        if recovery != 0 {
            // Repeated MF seeks can retain a shifted timestamp grid, or premature
            // EOS, even when seeking back to zero. Recreate once on the owner thread
            // and retain the original deadline/cancellation and exact frame check.
            check()?;
            decoder.reopen().map_err(|error| error.to_string())?;
            *position = None;
        }
        for (attempt, anchor) in std::iter::once(plan.target_pts)
            .chain(plan.fallback_pts.iter().copied())
            .enumerate()
        {
            check()?;
            if recovery != 0 || attempt != 0 || !sequential {
                // Invalidate before entering MF, including when SetCurrentPosition fails.
                *position = None;
                decoder.seek(anchor).map_err(|error| error.to_string())?;
            }
            loop {
                check()?;
                // Coalescing input must not mean displaying a keyframe seconds too early.
                // Keep 50 ms of preroll only while moving backward. Forward jumps skip
                // redundant full-resolution BGRA copies without reducing displayed detail.
                let copy_from = if retain_preroll {
                    TimeTick((plan.target_pts.0 - 500_000).max(0))
                } else {
                    plan.target_pts
                };
                let read = decoder
                    .read_next_toward(copy_from)
                    .map_err(|error| error.to_string())?;
                check()?;
                match read {
                    TargetFrameRead::Before(pts) => {
                        *position = Some(pts);
                        last_observed = Some(pts);
                    }
                    TargetFrameRead::AtOrAfter(frame) => {
                        *position = Some(frame.pts);
                        last_observed = Some(frame.pts);
                        if frame.pts < plan.target_pts {
                            cache.insert(Arc::new(frame));
                            continue;
                        }
                        if !plan.exact || frame.pts == plan.target_pts {
                            let frame = Arc::new(frame);
                            cache.insert(Arc::clone(&frame));
                            return Ok(frame);
                        }
                        break;
                    }
                    TargetFrameRead::End => {
                        *position = None;
                        if recovery == 0 {
                            continue 'recovery;
                        }
                        break;
                    }
                }
            }
        }
    }
    Err(format!(
        "source did not return exact indexed frame {:?} (last decoded: {last_observed:?})",
        plan.target_pts,
    ))
}

struct FrameCache {
    frames: VecDeque<Arc<DecodedBgraFrame>>,
    bytes: usize,
    limit: usize,
}

impl FrameCache {
    fn new(limit: usize) -> Self {
        Self {
            frames: VecDeque::new(),
            bytes: 0,
            limit,
        }
    }
    fn get(&mut self, pts: TimeTick) -> Option<Arc<DecodedBgraFrame>> {
        let index = self.frames.iter().position(|frame| frame.pts == pts)?;
        let frame = self.frames.remove(index)?;
        self.frames.push_back(Arc::clone(&frame));
        Some(frame)
    }
    fn insert(&mut self, frame: Arc<DecodedBgraFrame>) {
        if frame.pixels.len() > self.limit || self.get(frame.pts).is_some() {
            return;
        }
        while self.bytes + frame.pixels.len() > self.limit {
            if let Some(old) = self.frames.pop_front() {
                self.bytes -= old.pixels.len();
            }
        }
        self.bytes += frame.pixels.len();
        self.frames.push_back(frame);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan(exact: bool, tick: i64) -> DecodePlan {
        DecodePlan {
            project_tick: TimeTick(tick),
            target_pts: TimeTick(tick),
            fallback_pts: vec![],
            exact,
            prefetch_until: None,
        }
    }
    #[test]
    fn closing_does_not_join_an_in_flight_decoder() {
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let thread = thread::spawn(move || {
            let _ = release_rx.recv_timeout(Duration::from_secs(2));
            let _ = done_tx.send(());
        });
        let worker = DecodeWorker {
            shared: Arc::new(Shared::default()),
            thread: Some(thread),
            proxy: None,
        };
        let started = Instant::now();
        drop(worker);
        let elapsed = started.elapsed();
        let _ = release_tx.send(());
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            elapsed < Duration::from_millis(250),
            "window shutdown joined the decoder"
        );
    }

    #[test]
    fn ten_thousand_requests_use_one_pending_slot() {
        let mut mailbox = Mailbox::default();
        for tick in 0..10_000 {
            mailbox.submit(plan(true, tick)).unwrap();
        }
        assert_eq!(mailbox.pending.unwrap().plan.target_pts, TimeTick(9_999));
    }
    #[test]
    fn newer_exact_request_cancels_work_but_scrub_can_make_progress() {
        let mut mailbox = Mailbox::default();
        mailbox.submit(plan(true, 1)).unwrap();
        let exact = mailbox.pending.take().unwrap();
        mailbox.submit(plan(false, 2)).unwrap();
        let scrub = mailbox.pending.take().unwrap();
        mailbox.submit(plan(false, 3)).unwrap();
        assert!(mailbox.cancelled(&exact));
        assert!(!mailbox.cancelled(&scrub));
        mailbox.cancel();
        assert!(mailbox.cancelled(&scrub));
        assert!(mailbox.pending.is_none());
    }
    #[test]
    fn cache_is_byte_bounded_lru_and_oversized_frames_are_not_retained() {
        let frame = |tick, size| {
            Arc::new(DecodedBgraFrame {
                native_size: None,
                pts: TimeTick(tick),
                duration: None,
                width: 1,
                height: 1,
                pixels: Arc::new(vec![0; size]),
            })
        };
        let mut cache = FrameCache::new(8);
        cache.insert(frame(1, 4));
        cache.insert(frame(2, 4));
        assert!(cache.get(TimeTick(1)).is_some());
        cache.insert(frame(3, 4));
        assert!(cache.get(TimeTick(2)).is_none());
        cache.insert(frame(4, 9));
        assert_eq!(cache.bytes, 8);
        assert_eq!(cache.frames.len(), 2);
    }
}
