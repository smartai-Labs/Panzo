//! Bounded software-Present cadence, not a physical input-to-photon measurement.
use panzo_core::TimeTick;
use std::{collections::VecDeque, time::Instant};

/// Request submission to software Present, NOT physical pointer-to-photon latency.
#[derive(Default)]
pub struct ScrubMetrics {
    start: Option<Instant>,
    samples: VecDeque<(u64, u64)>,
    updates: VecDeque<Instant>,
    minimum_updates: Option<usize>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrubMetricsReport {
    pub samples: usize,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
    pub time_error_p95_tick: u64,
    pub minimum_updates_per_two_seconds: Option<usize>,
}
impl ScrubMetrics {
    pub fn start(&mut self, now: Instant) {
        *self = Self {
            start: Some(now),
            ..Self::default()
        };
    }
    pub fn present(
        &mut self,
        now: Instant,
        request_age_us: u64,
        time_error_tick: u64,
        changed: bool,
    ) {
        self.samples.push_back((request_age_us, time_error_tick));
        if self.samples.len() > 3600 {
            self.samples.pop_front();
        }
        if changed {
            self.updates.push_back(now);
        }
        self.finish(now);
    }
    pub fn finish(&mut self, now: Instant) {
        let window = std::time::Duration::from_secs(2);
        while self
            .updates
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) > window)
        {
            self.updates.pop_front();
        }
        if self
            .start
            .is_some_and(|start| now.saturating_duration_since(start) >= window)
        {
            self.minimum_updates = Some(
                self.minimum_updates
                    .unwrap_or(usize::MAX)
                    .min(self.updates.len()),
            );
        }
    }
    pub fn report(&self) -> ScrubMetricsReport {
        let mut age: Vec<_> = self.samples.iter().map(|v| v.0).collect();
        let mut error: Vec<_> = self.samples.iter().map(|v| v.1).collect();
        age.sort_unstable();
        error.sort_unstable();
        let p = |values: &[u64], percent: usize| {
            values
                .get((values.len() * percent).div_ceil(100).saturating_sub(1))
                .copied()
                .unwrap_or(0)
        };
        ScrubMetricsReport {
            samples: age.len(),
            p50_us: p(&age, 50),
            p95_us: p(&age, 95),
            p99_us: p(&age, 99),
            max_us: age.last().copied().unwrap_or(0),
            time_error_p95_tick: p(&error, 95),
            minimum_updates_per_two_seconds: self.minimum_updates,
        }
    }
}

#[derive(Default)]
pub struct PresentationMetrics {
    previous: Option<(Instant, TimeTick)>,
    intervals: VecDeque<u64>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cadence {
    pub samples: usize,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
}
impl PresentationMetrics {
    pub fn pause(&mut self) {
        self.previous = None;
    }
    pub fn present(&mut self, now: Instant, tick: TimeTick, playing: bool) {
        if !playing {
            self.pause();
            return;
        }
        // Re-presenting the same source frame is not new video feedback. Keep its
        // original timestamp so duplicates cannot hide a stalled decoder.
        if self.previous.is_some_and(|(_, pts)| pts == tick) {
            return;
        }
        if let Some((previous, pts)) = self.previous
            && tick > pts
        {
            self.intervals.push_back(
                u64::try_from(now.saturating_duration_since(previous).as_micros())
                    .unwrap_or(u64::MAX),
            );
            if self.intervals.len() > 3600 {
                self.intervals.pop_front();
            }
        }
        self.previous = Some((now, tick));
    }
    pub fn report(&self) -> Cadence {
        let mut sorted: Vec<_> = self.intervals.iter().copied().collect();
        sorted.sort_unstable();
        let percentile = |p: usize| {
            sorted
                .get((sorted.len() * p).div_ceil(100).saturating_sub(1))
                .copied()
                .unwrap_or(0)
        };
        Cadence {
            samples: sorted.len(),
            p95_us: percentile(95),
            p99_us: percentile(99),
            max_us: sorted.last().copied().unwrap_or(0),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn pause_is_excluded_and_budget_is_bounded() {
        let mut metric = PresentationMetrics::default();
        let start = Instant::now();
        for i in 0..4000 {
            metric.present(
                start + Duration::from_millis(i * 16),
                TimeTick(i64::try_from(i).unwrap()),
                true,
            );
        }
        assert_eq!(metric.report().samples, 3600);
        assert_eq!(metric.report().max_us, 16000);
        metric.pause();
        metric.present(start + Duration::from_secs(1000), TimeTick(9999), true);
        assert_eq!(metric.report().max_us, 16000);
    }

    #[test]
    fn duplicate_presents_do_not_hide_a_video_stall() {
        let mut metric = PresentationMetrics::default();
        let start = Instant::now();
        metric.present(start, TimeTick(0), true);
        metric.present(start + Duration::from_millis(90), TimeTick(0), true);
        metric.present(start + Duration::from_millis(100), TimeTick(1), true);
        assert_eq!(metric.report().max_us, 100_000);
    }

    #[test]
    fn scrub_counts_changed_frames_and_includes_release_stalls() {
        let mut metric = ScrubMetrics::default();
        let start = Instant::now();
        metric.start(start);
        for i in 0..301 {
            metric.present(
                start + Duration::from_millis(i * 10),
                10_000,
                333_333,
                i % 2 == 0,
            );
        }
        let report = metric.report();
        assert_eq!(report.p95_us, 10_000);
        assert!(report.minimum_updates_per_two_seconds.unwrap() >= 100);
        metric.finish(start + Duration::from_secs(5));
        assert_eq!(metric.report().minimum_updates_per_two_seconds, Some(1));
    }
}
