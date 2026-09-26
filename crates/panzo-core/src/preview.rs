use crate::time::TimeTick;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Adjacent project output frame at 60 fps. Ceil tick rounding never sticks on a frame.
pub fn step_frame_tick(tick: TimeTick, duration: TimeTick, forward: bool) -> TimeTick {
    let scale = i128::from(crate::TICKS_PER_SECOND);
    let frame = (i128::from(tick.0) * 60).div_euclid(scale);
    let boundary = (frame * scale + 59).div_euclid(60);
    let target = if forward {
        frame + 1
    } else if i128::from(tick.0) <= boundary {
        frame - 1
    } else {
        frame
    };
    TimeTick(((target * scale + 59).div_euclid(60)).clamp(0, i128::from(duration.0.max(0))) as i64)
}

#[cfg(test)]
mod frame_step_tests {
    use super::*;
    #[test]
    fn frame_steps_are_reversible_and_clamp_at_both_ends() {
        let duration = TimeTick::from_millis(2000);
        let mut tick = TimeTick::ZERO;
        for n in 1..=120 {
            tick = step_frame_tick(tick, duration, true);
            assert_eq!(tick.0, (n * crate::TICKS_PER_SECOND + 59) / 60);
        }
        assert_eq!(step_frame_tick(tick, duration, true), duration);
        for n in (0..120).rev() {
            tick = step_frame_tick(tick, duration, false);
            assert_eq!(tick.0, (n * crate::TICKS_PER_SECOND + 59) / 60);
        }
        assert_eq!(step_frame_tick(tick, duration, false), TimeTick::ZERO);
        assert_eq!(
            step_frame_tick(TimeTick(180_000), duration, false),
            TimeTick(166_667)
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlaybackState {
    Paused,
    Playing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSnapshot {
    pub playback: PlaybackState,
    pub project_tick: TimeTick,
    pub duration_tick: TimeTick,
    pub camera_diagnostics_visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewController {
    duration_tick: TimeTick,
    project_tick: TimeTick,
    playback: PlaybackState,
    camera_diagnostics_visible: bool,
}

impl PreviewController {
    pub fn new(duration_tick: TimeTick) -> Result<Self, PreviewControllerError> {
        if duration_tick <= TimeTick::ZERO {
            return Err(PreviewControllerError::InvalidDuration(duration_tick));
        }
        Ok(Self {
            duration_tick,
            project_tick: TimeTick::ZERO,
            playback: PlaybackState::Paused,
            camera_diagnostics_visible: false,
        })
    }

    pub const fn snapshot(&self) -> PreviewSnapshot {
        PreviewSnapshot {
            playback: self.playback,
            project_tick: self.project_tick,
            duration_tick: self.duration_tick,
            camera_diagnostics_visible: self.camera_diagnostics_visible,
        }
    }

    pub fn play(&mut self) -> PreviewSnapshot {
        if self.project_tick >= self.duration_tick {
            self.project_tick = TimeTick::ZERO;
        }
        self.playback = PlaybackState::Playing;
        self.snapshot()
    }

    pub fn set_duration(&mut self, duration: TimeTick) -> Result<(), PreviewControllerError> {
        if duration <= TimeTick::ZERO {
            return Err(PreviewControllerError::InvalidDuration(duration));
        }
        self.duration_tick = duration;
        self.project_tick = self.project_tick.min(duration);
        if self.project_tick == duration {
            self.playback = PlaybackState::Paused;
        }
        Ok(())
    }

    pub fn pause(&mut self) -> PreviewSnapshot {
        self.playback = PlaybackState::Paused;
        self.snapshot()
    }

    pub fn toggle_playback(&mut self) -> PreviewSnapshot {
        match self.playback {
            PlaybackState::Paused => self.play(),
            PlaybackState::Playing => self.pause(),
        }
    }

    pub fn seek(
        &mut self,
        project_tick: TimeTick,
    ) -> Result<PreviewSnapshot, PreviewControllerError> {
        if project_tick < TimeTick::ZERO {
            return Err(PreviewControllerError::NegativeSeek(project_tick));
        }
        self.project_tick = project_tick.min(self.duration_tick);
        if self.project_tick >= self.duration_tick {
            self.playback = PlaybackState::Paused;
        }
        Ok(self.snapshot())
    }

    pub fn jump_to_start(&mut self) -> PreviewSnapshot {
        self.project_tick = TimeTick::ZERO;
        self.snapshot()
    }

    pub fn advance(
        &mut self,
        elapsed_tick: TimeTick,
    ) -> Result<PreviewSnapshot, PreviewControllerError> {
        if elapsed_tick < TimeTick::ZERO {
            return Err(PreviewControllerError::NegativeAdvance(elapsed_tick));
        }
        if self.playback == PlaybackState::Paused {
            return Ok(self.snapshot());
        }
        let advanced = self
            .project_tick
            .checked_add(elapsed_tick)
            .unwrap_or(self.duration_tick);
        self.project_tick = advanced.min(self.duration_tick);
        if self.project_tick >= self.duration_tick {
            self.playback = PlaybackState::Paused;
        }
        Ok(self.snapshot())
    }

    pub fn toggle_camera_diagnostics(&mut self) -> PreviewSnapshot {
        self.camera_diagnostics_visible = !self.camera_diagnostics_visible;
        self.snapshot()
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum PreviewControllerError {
    #[error("preview duration must be positive: {0:?}")]
    InvalidDuration(TimeTick),
    #[error("preview seek cannot be negative: {0:?}")]
    NegativeSeek(TimeTick),
    #[error("preview advance cannot be negative: {0:?}")]
    NegativeAdvance(TimeTick),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_pause_seek_and_end_are_deterministic() {
        let duration = TimeTick::from_millis(2_000);
        let mut controller = PreviewController::new(duration).unwrap();
        assert_eq!(controller.snapshot().playback, PlaybackState::Paused);

        controller.play();
        controller.advance(TimeTick::from_millis(500)).unwrap();
        assert_eq!(
            controller.snapshot().project_tick,
            TimeTick::from_millis(500)
        );
        controller.pause();
        controller.advance(TimeTick::from_millis(500)).unwrap();
        assert_eq!(
            controller.snapshot().project_tick,
            TimeTick::from_millis(500)
        );

        controller.seek(TimeTick::from_millis(1_900)).unwrap();
        controller.play();
        let ended = controller.advance(TimeTick::from_millis(200)).unwrap();
        assert_eq!(ended.project_tick, duration);
        assert_eq!(ended.playback, PlaybackState::Paused);

        let restarted = controller.play();
        assert_eq!(restarted.project_tick, TimeTick::ZERO);
        assert_eq!(restarted.playback, PlaybackState::Playing);
    }

    #[test]
    fn seek_clamps_and_diagnostics_toggle_without_changing_time() {
        let duration = TimeTick::from_millis(1_000);
        let mut controller = PreviewController::new(duration).unwrap();
        let clamped = controller.seek(TimeTick::from_millis(2_000)).unwrap();
        assert_eq!(clamped.project_tick, duration);
        assert_eq!(clamped.playback, PlaybackState::Paused);
        assert!(matches!(
            controller.seek(TimeTick(-1)),
            Err(PreviewControllerError::NegativeSeek(TimeTick(-1)))
        ));

        controller.jump_to_start();
        let toggled = controller.toggle_camera_diagnostics();
        assert!(toggled.camera_diagnostics_visible);
        assert_eq!(toggled.project_tick, TimeTick::ZERO);
    }
}
