use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const TICKS_PER_SECOND: i64 = 10_000_000;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct TimeTick(pub i64);

impl TimeTick {
    pub const ZERO: Self = Self(0);

    pub const fn from_millis(milliseconds: i64) -> Self {
        Self(milliseconds.saturating_mul(TICKS_PER_SECOND / 1_000))
    }

    pub const fn as_i64(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.0.checked_add(rhs.0).map(Self)
    }

    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        self.0.checked_sub(rhs.0).map(Self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QpcClock {
    frequency: i64,
    session_start_qpc: i64,
}

impl QpcClock {
    pub fn new(frequency: i64, session_start_qpc: i64) -> Result<Self, TimeError> {
        if frequency <= 0 {
            return Err(TimeError::InvalidFrequency(frequency));
        }

        Ok(Self {
            frequency,
            session_start_qpc,
        })
    }

    pub const fn frequency(self) -> i64 {
        self.frequency
    }

    pub const fn session_start_qpc(self) -> i64 {
        self.session_start_qpc
    }

    pub fn to_session_tick(self, event_qpc: i64) -> Result<TimeTick, TimeError> {
        let delta = i128::from(event_qpc) - i128::from(self.session_start_qpc);
        if delta < 0 {
            return Err(TimeError::BeforeSessionStart {
                event_qpc,
                session_start_qpc: self.session_start_qpc,
            });
        }

        let ticks = delta
            .checked_mul(i128::from(TICKS_PER_SECOND))
            .ok_or(TimeError::Overflow)?
            / i128::from(self.frequency);

        i64::try_from(ticks)
            .map(TimeTick)
            .map_err(|_| TimeError::Overflow)
    }

    pub fn qpc_from_system_relative_tick(
        frequency: i64,
        system_relative_tick: i64,
    ) -> Result<i64, TimeError> {
        if frequency <= 0 {
            return Err(TimeError::InvalidFrequency(frequency));
        }
        if system_relative_tick < 0 {
            return Err(TimeError::NegativeSystemRelativeTick(system_relative_tick));
        }

        let qpc = i128::from(system_relative_tick)
            .checked_mul(i128::from(frequency))
            .ok_or(TimeError::Overflow)?
            / i128::from(TICKS_PER_SECOND);

        i64::try_from(qpc).map_err(|_| TimeError::Overflow)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionClock {
    pub ticks_per_second: i64,
    pub session_start_qpc: i64,
    pub qpc_frequency: i64,
    pub first_frame_system_relative_tick: i64,
}

impl SessionClock {
    pub fn establish(
        qpc_frequency: i64,
        first_frame_system_relative_tick: i64,
    ) -> Result<Self, TimeError> {
        if first_frame_system_relative_tick < 0 {
            return Err(TimeError::NegativeSystemRelativeTick(
                first_frame_system_relative_tick,
            ));
        }
        let session_start_qpc = QpcClock::qpc_from_system_relative_tick(
            qpc_frequency,
            first_frame_system_relative_tick,
        )?;
        QpcClock::new(qpc_frequency, session_start_qpc)?;
        Ok(Self {
            ticks_per_second: TICKS_PER_SECOND,
            session_start_qpc,
            qpc_frequency,
            first_frame_system_relative_tick,
        })
    }

    pub fn event_tick(self, event_qpc: i64) -> Result<TimeTick, TimeError> {
        QpcClock::new(self.qpc_frequency, self.session_start_qpc)?.to_session_tick(event_qpc)
    }

    /// Returns elapsed recording time for control-loop scheduling.
    ///
    /// A newly delivered WGC frame can carry a presentation timestamp a few milliseconds ahead
    /// of an immediately sampled QPC value. Persisted input events must still use `event_tick` and
    /// reject pre-epoch values, while control-loop progress remains at zero until the epoch arrives.
    pub fn recording_tick(self, current_qpc: i64) -> Result<TimeTick, TimeError> {
        if current_qpc <= self.session_start_qpc {
            return Ok(TimeTick::ZERO);
        }
        self.event_tick(current_qpc)
    }

    pub fn frame_tick(self, system_relative_tick: i64) -> Result<TimeTick, TimeError> {
        let tick = system_relative_tick
            .checked_sub(self.first_frame_system_relative_tick)
            .ok_or(TimeError::Overflow)?;
        if tick < 0 {
            return Err(TimeError::FrameBeforeEpoch {
                frame_tick: system_relative_tick,
                epoch_tick: self.first_frame_system_relative_tick,
            });
        }
        Ok(TimeTick(tick))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoTimestampDecision {
    Accept(TimeTick),
    ReplacePrevious(TimeTick),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct VideoTimestampNormalizer {
    last_tick: Option<TimeTick>,
}

impl VideoTimestampNormalizer {
    pub const fn last_tick(self) -> Option<TimeTick> {
        self.last_tick
    }

    pub fn observe(&mut self, tick: TimeTick) -> Result<VideoTimestampDecision, TimeError> {
        match self.last_tick {
            None => {
                self.last_tick = Some(tick);
                Ok(VideoTimestampDecision::Accept(tick))
            }
            Some(previous) if tick > previous => {
                self.last_tick = Some(tick);
                Ok(VideoTimestampDecision::Accept(tick))
            }
            Some(previous) if tick == previous => Ok(VideoTimestampDecision::ReplacePrevious(tick)),
            Some(previous) => Err(TimeError::NonMonotonicVideoTimestamp {
                previous,
                current: tick,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFrameRateDecision {
    Accept(TimeTick),
    Drop(TimeTick),
}

/// Selects at most one source frame in each target-rate time slot.
///
/// Source timestamps are never rewritten, so accepted frames remain valid VFR samples. The
/// one-tick adjustment compensates for integer truncation at an exact slot boundary (for example,
/// `10_000_000 / 60 == 166_666` in the Panzo timebase).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoFrameRateLimiter {
    target_fps: u32,
    last_observed_tick: Option<TimeTick>,
    last_accepted_slot: Option<i128>,
}

impl VideoFrameRateLimiter {
    pub fn new(target_fps: u32) -> Result<Self, TimeError> {
        if target_fps == 0 {
            return Err(TimeError::InvalidTargetFrameRate(target_fps));
        }
        Ok(Self {
            target_fps,
            last_observed_tick: None,
            last_accepted_slot: None,
        })
    }

    pub const fn target_fps(self) -> u32 {
        self.target_fps
    }

    pub fn observe(&mut self, tick: TimeTick) -> Result<VideoFrameRateDecision, TimeError> {
        if tick.0 < 0 {
            return Err(TimeError::NegativeVideoTimestamp(tick));
        }
        if let Some(previous) = self.last_observed_tick
            && tick < previous
        {
            return Err(TimeError::NonMonotonicVideoTimestamp {
                previous,
                current: tick,
            });
        }
        self.last_observed_tick = Some(tick);

        let slot = (i128::from(tick.0) + 1)
            .checked_mul(i128::from(self.target_fps))
            .ok_or(TimeError::Overflow)?
            / i128::from(TICKS_PER_SECOND);
        if self.last_accepted_slot == Some(slot) {
            return Ok(VideoFrameRateDecision::Drop(tick));
        }
        debug_assert!(
            self.last_accepted_slot
                .is_none_or(|previous| slot > previous)
        );
        self.last_accepted_slot = Some(slot);
        Ok(VideoFrameRateDecision::Accept(tick))
    }
}

pub trait TimeMapping {
    fn source_time(&self, project_time: TimeTick) -> Result<TimeTick, TimeError>;
    fn project_time(&self, source_time: TimeTick) -> Result<TimeTick, TimeError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct IdentityTimeMapping;

impl TimeMapping for IdentityTimeMapping {
    fn source_time(&self, project_time: TimeTick) -> Result<TimeTick, TimeError> {
        Ok(project_time)
    }

    fn project_time(&self, source_time: TimeTick) -> Result<TimeTick, TimeError> {
        Ok(source_time)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TimeError {
    #[error("time has no corresponding retained content: {0:?}")]
    UnmappedTime(TimeTick),
    #[error("QPC frequency must be positive, got {0}")]
    InvalidFrequency(i64),
    #[error("system-relative QPC tick must be non-negative, got {0}")]
    NegativeSystemRelativeTick(i64),
    #[error("event QPC {event_qpc} is before session start QPC {session_start_qpc}")]
    BeforeSessionStart {
        event_qpc: i64,
        session_start_qpc: i64,
    },
    #[error("time conversion overflow")]
    Overflow,
    #[error("frame time {frame_tick} is earlier than epoch {epoch_tick}")]
    FrameBeforeEpoch { frame_tick: i64, epoch_tick: i64 },
    #[error("video timestamp moved backwards from {previous:?} to {current:?}")]
    NonMonotonicVideoTimestamp {
        previous: TimeTick,
        current: TimeTick,
    },
    #[error("target video frame rate must be positive, got {0}")]
    InvalidTargetFrameRate(u32),
    #[error("video timestamp must be non-negative, got {0:?}")]
    NegativeVideoTimestamp(TimeTick),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_qpc_without_floating_point() {
        let clock = QpcClock::new(24_000_000, 48_000_000).unwrap();
        assert_eq!(
            clock.to_session_tick(72_000_000).unwrap(),
            TimeTick(TICKS_PER_SECOND)
        );
    }

    #[test]
    fn rejects_events_before_epoch() {
        let clock = QpcClock::new(10_000_000, 100).unwrap();
        assert!(matches!(
            clock.to_session_tick(99),
            Err(TimeError::BeforeSessionStart { .. })
        ));
    }

    #[test]
    fn system_relative_tick_round_trip_is_bounded() {
        let frequency = 24_000_000;
        let system_tick = 9_876_543_210;
        let qpc = QpcClock::qpc_from_system_relative_tick(frequency, system_tick).unwrap();
        let clock = QpcClock::new(frequency, 0).unwrap();
        let round_trip = clock.to_session_tick(qpc).unwrap().as_i64();
        assert!((round_trip - system_tick).abs() <= 1);
    }

    #[test]
    fn identity_mapping_is_exact() {
        let mapping = IdentityTimeMapping;
        let tick = TimeTick(123_456_789);
        assert_eq!(mapping.source_time(tick).unwrap(), tick);
        assert_eq!(mapping.project_time(tick).unwrap(), tick);
    }

    #[test]
    fn session_clock_uses_first_frame_as_shared_epoch() {
        let clock = SessionClock::establish(10_000_000, 123_000_000).unwrap();
        assert_eq!(clock.session_start_qpc, 123_000_000);
        assert_eq!(clock.frame_tick(123_000_000).unwrap(), TimeTick::ZERO);
        assert_eq!(clock.event_tick(124_500_000).unwrap(), TimeTick(1_500_000));
        assert!(clock.frame_tick(122_999_999).is_err());
        assert!(clock.event_tick(122_999_999).is_err());
    }

    #[test]
    fn recording_tick_waits_at_zero_for_future_wgc_epoch() {
        let clock = SessionClock::establish(10_000_000, 123_000_000).unwrap();

        assert!(matches!(
            clock.event_tick(122_999_999),
            Err(TimeError::BeforeSessionStart { .. })
        ));
        assert_eq!(clock.recording_tick(122_999_999).unwrap(), TimeTick::ZERO);
        assert_eq!(clock.recording_tick(123_000_000).unwrap(), TimeTick::ZERO);
        assert_eq!(clock.recording_tick(123_010_000).unwrap(), TimeTick(10_000));
    }

    #[test]
    fn video_timestamp_normalizer_replaces_duplicates_and_rejects_regression() {
        let mut normalizer = VideoTimestampNormalizer::default();
        assert_eq!(
            normalizer.observe(TimeTick(10)).unwrap(),
            VideoTimestampDecision::Accept(TimeTick(10))
        );
        assert_eq!(
            normalizer.observe(TimeTick(10)).unwrap(),
            VideoTimestampDecision::ReplacePrevious(TimeTick(10))
        );
        assert_eq!(
            normalizer.observe(TimeTick(11)).unwrap(),
            VideoTimestampDecision::Accept(TimeTick(11))
        );
        assert!(matches!(
            normalizer.observe(TimeTick(9)),
            Err(TimeError::NonMonotonicVideoTimestamp { .. })
        ));
    }

    fn source_tick(frame_index: i64, source_fps: i64) -> TimeTick {
        TimeTick(frame_index * TICKS_PER_SECOND / source_fps)
    }

    fn accepted_frame_count(source_fps: i64, seconds: i64) -> usize {
        let mut limiter = VideoFrameRateLimiter::new(60).unwrap();
        (0..source_fps * seconds)
            .filter(|frame_index| {
                matches!(
                    limiter
                        .observe(source_tick(*frame_index, source_fps))
                        .unwrap(),
                    VideoFrameRateDecision::Accept(_)
                )
            })
            .count()
    }

    #[test]
    fn video_frame_rate_limiter_keeps_real_60_fps_ticks() {
        assert_eq!(accepted_frame_count(60, 10), 600);
    }

    #[test]
    fn video_frame_rate_limiter_reduces_high_refresh_sources_to_60_fps() {
        assert_eq!(accepted_frame_count(100, 10), 600);
        assert_eq!(accepted_frame_count(120, 10), 600);
        assert_eq!(accepted_frame_count(144, 10), 600);
    }

    #[test]
    fn video_frame_rate_limiter_preserves_vfr_ticks_and_does_not_burst_after_a_gap() {
        let mut limiter = VideoFrameRateLimiter::new(60).unwrap();
        assert_eq!(
            limiter.observe(TimeTick::ZERO).unwrap(),
            VideoFrameRateDecision::Accept(TimeTick::ZERO)
        );
        assert_eq!(
            limiter.observe(TimeTick::from_millis(5_000)).unwrap(),
            VideoFrameRateDecision::Accept(TimeTick::from_millis(5_000))
        );
        assert_eq!(
            limiter.observe(TimeTick::from_millis(5_001)).unwrap(),
            VideoFrameRateDecision::Drop(TimeTick::from_millis(5_001))
        );
    }

    #[test]
    fn video_frame_rate_limiter_rejects_invalid_input() {
        assert!(matches!(
            VideoFrameRateLimiter::new(0),
            Err(TimeError::InvalidTargetFrameRate(0))
        ));
        let mut limiter = VideoFrameRateLimiter::new(60).unwrap();
        assert!(matches!(
            limiter.observe(TimeTick(-1)),
            Err(TimeError::NegativeVideoTimestamp(TimeTick(-1)))
        ));
    }
}
