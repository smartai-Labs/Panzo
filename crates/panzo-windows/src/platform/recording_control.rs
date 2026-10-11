//! Shared recording clock and controls, sampled by UI, video and audio workers.
use super::{QpcSource, SessionClock, TimeTick};
use panzo_core::{
    camera_policy::{AutoCameraConfig, AutoCameraSettings},
    recording_time::RecordingTimeline,
};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RecordingOptions {
    pub fps: u32,
    pub countdown_seconds: u32,
    pub camera: AutoCameraConfig,
    pub cursor_visible: bool,
    pub audio: crate::platform::audio_capture::AudioCaptureOptions,
}
impl Default for RecordingOptions {
    fn default() -> Self {
        Self {
            fps: 60,
            countdown_seconds: 0,
            camera: AutoCameraConfig::default(),
            cursor_visible: true,
            audio: crate::platform::audio_capture::AudioCaptureOptions::default(),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub struct SessionState {
    pub clock: Option<SessionClock>,
    pub timeline: RecordingTimeline,
    pub camera_changes: Vec<(TimeTick, bool)>,
    pub camera_enabled: bool,
}
#[derive(Debug, Clone, Default)]
pub struct RecordingControl {
    stop_requested: Arc<AtomicBool>,
    recording_started: Arc<AtomicBool>,
    pub camera_enabled: bool,
    pub options: RecordingOptions,
    pub session: Arc<Mutex<SessionState>>,
    pub levels: Arc<[AtomicU32; 2]>,
}
impl RecordingControl {
    pub fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::Release);
    }
    pub fn stop_requested(&self) -> bool {
        self.stop_requested.load(Ordering::Acquire)
    }
    pub fn recording_started(&self) -> bool {
        self.recording_started.load(Ordering::Acquire)
    }
    pub(super) fn start_session(&self, clock: SessionClock) {
        let mut s = self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        s.clock = Some(clock);
        s.camera_enabled = self.camera_enabled;
        s.camera_changes = vec![(TimeTick::ZERO, self.camera_enabled)];
    }
    pub(super) fn mark_recording_started(&self) {
        self.recording_started.store(true, Ordering::Release);
    }
    pub fn paused(&self) -> bool {
        self.session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .timeline
            .paused()
    }
    pub fn camera_active(&self) -> bool {
        self.session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .camera_enabled
    }
    pub fn elapsed(&self) -> TimeTick {
        let s = self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::wall(&s).map_or(TimeTick::ZERO, |t| s.timeline.elapsed(t))
    }
    fn wall(s: &SessionState) -> Result<TimeTick, String> {
        let clock = s.clock.ok_or("录制尚未开始")?;
        let qpc = QpcSource::new()
            .map_err(|e| e.to_string())?
            .now()
            .map_err(|e| e.to_string())?;
        clock.recording_tick(qpc).map_err(|e| e.to_string())
    }
    pub fn toggle_pause(&self) -> Result<bool, String> {
        let mut s = self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let wall = Self::wall(&s)?;
        let paused = !s.timeline.paused();
        s.timeline.set_paused(wall, paused);
        Ok(paused)
    }
    pub fn set_camera_enabled(&self, enabled: bool) -> Result<(), String> {
        let mut s = self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let wall = Self::wall(&s)?;
        let tick = s.timeline.elapsed(wall);
        s.camera_enabled = enabled;
        if s.camera_changes.last().is_some_and(|(t, _)| *t == tick) {
            s.camera_changes.pop();
        }
        s.camera_changes.push((tick, enabled));
        Ok(())
    }
    pub fn map_tick(&self, wall: TimeTick) -> Option<TimeTick> {
        self.session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .timeline
            .map(wall)
    }
    pub fn source_tick(&self, system_tick: i64) -> Option<TimeTick> {
        let s = self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let clock = s.clock?;
        s.timeline.map(TimeTick(
            system_tick - clock.first_frame_system_relative_tick,
        ))
    }
    pub fn camera_settings(&self, duration: TimeTick) -> AutoCameraSettings {
        let s = self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut result = AutoCameraSettings {
            default: self.options.camera,
            ranges: Vec::new(),
        };
        result.default.enabled = self.camera_enabled;
        for (i, (start, enabled)) in s.camera_changes.iter().enumerate() {
            let end = s.camera_changes.get(i + 1).map_or(duration, |(t, _)| *t);
            if end > *start {
                result.set_range(
                    *start,
                    end,
                    AutoCameraConfig {
                        enabled: *enabled,
                        ..self.options.camera
                    },
                );
            }
        }
        result
    }
}
