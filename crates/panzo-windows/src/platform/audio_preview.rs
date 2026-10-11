//! Low-latency WASAPI preview; rendering and exported PCM use the same source-time mixer.
use crate::platform::{
    apartment::WinRtApartment,
    audio::{AudioMixer, RATE, frames_at},
    audio_capture::{open_client, pcm_format},
};
use panzo_core::{
    PlaybackState, PreviewSnapshot, TimeTick, VideoEdit, camera_policy::AudioSettings,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioRenderClient,
};
#[derive(Clone)]
struct Target {
    generation: u64,
    playing: bool,
    muted: bool,
    tick: TimeTick,
    video: Arc<VideoEdit>,
    settings: AudioSettings,
    clips: Vec<panzo_core::media_edit::AudioClip>,
}
pub struct AudioPreview {
    root: PathBuf,
    target: Arc<Mutex<Target>>,
    stop: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
    worker: Option<JoinHandle<()>>,
}
impl AudioPreview {
    pub fn new(
        root: &Path,
        video: VideoEdit,
        settings: AudioSettings,
        clips: Vec<panzo_core::media_edit::AudioClip>,
    ) -> Self {
        Self {
            root: root.into(),
            target: Arc::new(Mutex::new(Target {
                generation: 0,
                playing: false,
                muted: false,
                tick: TimeTick::ZERO,
                video: Arc::new(video),
                settings,
                clips,
            })),
            stop: Arc::new(AtomicBool::new(false)),
            error: Arc::default(),
            worker: None,
        }
    }
    pub fn edit(
        &mut self,
        video: VideoEdit,
        settings: AudioSettings,
        clips: Vec<panzo_core::media_edit::AudioClip>,
    ) {
        let mut t = self
            .target
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        t.video = Arc::new(video);
        t.settings = settings;
        t.clips = clips;
        t.generation += 1;
    }
    pub fn sync(&mut self, snapshot: PreviewSnapshot, reset: bool) {
        let mut t = self
            .target
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let playing = snapshot.playback == PlaybackState::Playing;
        let retry_failed_worker = reset || playing != t.playing;
        if reset || playing != t.playing {
            t.generation += 1;
        }
        t.playing = playing;
        t.tick = snapshot.project_tick;
        let has_audio = !t.clips.is_empty() || self.root.join("media/audio.json").is_file();
        drop(t);
        // A missing/disconnected audio device must not spawn a new worker every frame.
        if retry_failed_worker && self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            self.worker.take();
        }
        if playing && self.worker.is_none() && has_audio {
            let (root, target, stop, error) = (
                self.root.clone(),
                self.target.clone(),
                self.stop.clone(),
                self.error.clone(),
            );
            self.worker = Some(thread::spawn(move || {
                if let Err(message) = playback(&root, &target, &stop) {
                    *error
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(message);
                }
            }));
        }
    }
    pub fn muted(&self) -> bool {
        self.target.lock().unwrap_or_else(std::sync::PoisonError::into_inner).muted
    }
    pub fn toggle_mute(&self) {
        let mut target=self.target.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        target.muted = !target.muted;
    }
    pub fn take_warning(&self) -> Option<String> {
        self.error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}
impl Drop for AudioPreview {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn playback(root: &Path, target: &Mutex<Target>, stop: &AtomicBool) -> Result<(), String> {
    let _apartment = WinRtApartment::multi_threaded().map_err(|e| e.to_string())?;
    let mut mixer = AudioMixer::open(root)?;

    let client = open_client(false, None)?;
    let format = pcm_format();
    unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            400_000,
            0,
            &raw const format,
            None,
        )
    }
    .map_err(|e| format!("声音预览无法启动：{e}"))?;
    let render: IAudioRenderClient = unsafe { client.GetService() }.map_err(|e| e.to_string())?;
    let capacity = unsafe { client.GetBufferSize() }.map_err(|e| e.to_string())?;
    let mut generation = u64::MAX;
    let mut next = 0u64;
    let mut running = false;
    while !stop.load(Ordering::Acquire) {
        let t = target
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let padding =
            unsafe { client.GetCurrentPadding() }.map_err(|e| format!("声音输出设备中断：{e}"))?;
        let requested = frames_at(t.tick);
        let drift =
            next.saturating_sub(u64::from(padding)).abs_diff(requested) > u64::from(RATE) / 8;
        if t.generation != generation || (running && drift) {
            mixer.set_clips(root, &t.clips)?;
            unsafe {
                let _ = client.Stop();
                client.Reset().map_err(|e| e.to_string())?;
            }
            running = false;
            next = requested;
            generation = t.generation;
        }
        if !t.playing {
            if running {
                unsafe {
                    let _ = client.Stop();
                    let _ = client.Reset();
                }
                running = false;
            }
            thread::sleep(Duration::from_millis(5));
            continue;
        }
        let padding = unsafe { client.GetCurrentPadding() }.map_err(|e| e.to_string())?;
        let count = capacity
            .saturating_sub(padding)
            .min(1440u32.saturating_sub(padding));
        if count > 0 {
            let samples =
                mixer.render(next, count as usize, TimeTick::ZERO, &t.video, t.settings)?;
            let buffer = unsafe { render.GetBuffer(count) }.map_err(|e| e.to_string())?;
            let bytes = unsafe { std::slice::from_raw_parts_mut(buffer, count as usize * 4) };
            for (sample, slot) in samples.iter().zip(bytes.chunks_exact_mut(2)) {
                slot.copy_from_slice(&if t.muted { 0_i16 } else { *sample }.to_le_bytes());
            }
            unsafe { render.ReleaseBuffer(count, 0) }.map_err(|e| e.to_string())?;
            next += u64::from(count);
            if !running {
                unsafe { client.Start() }.map_err(|e| e.to_string())?;
                running = true;
            }
        }
        thread::sleep(Duration::from_millis(3));
    }
    unsafe {
        let _ = client.Stop();
    }
    Ok(())
}
