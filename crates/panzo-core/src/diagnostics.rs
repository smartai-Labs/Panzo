use serde::{Deserialize, Serialize};

use crate::time::TICKS_PER_SECOND;

pub const MAX_BACKPRESSURE_PER_MILLION: u64 = 5_000;
pub const MAX_AVERAGE_ENCODED_FPS_MILLI: u64 = 60_500;
pub const MAX_CAPTURE_FRAME_GAP_P99_TICK: i64 = 333_400;
pub const MAX_CAPTURE_FRAME_GAP_TICK: i64 = 1_000_000;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureMetrics {
    pub frames_received: u64,
    pub frames_encoded: u64,
    pub dropped_by_backpressure: u64,
    #[serde(default)]
    pub dropped_by_frame_rate_limit: u64,
    pub duplicate_timestamp: u64,
    pub max_frame_gap_tick: i64,
    #[serde(default)]
    pub frame_gap_p99_tick: i64,
    #[serde(default)]
    pub frame_gap_sample_count: u64,
    pub capture_queue_peak: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputMetrics {
    pub cursor_events_raw: u64,
    pub cursor_events_persisted: u64,
    pub click_events: u64,
    pub queue_peak: u32,
    pub queue_overflow: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraMetrics {
    pub trigger_clicks: u64,
    pub clusters: u64,
    pub segments: u64,
    pub zoom_in: u64,
    pub pan: u64,
    pub reframe: u64,
    pub zoom_out: u64,
    pub clamp_applied: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportMetrics {
    pub frames_rendered: u64,
    pub duration_tick: i64,
    pub wall_time_ms: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMetrics {
    pub capture: CaptureMetrics,
    pub input: InputMetrics,
    pub camera: CameraMetrics,
    pub export: ExportMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStabilityReport {
    pub passed: bool,
    pub required_duration_tick: i64,
    pub actual_duration_tick: i64,
    pub backpressure_per_million: u64,
    pub average_encoded_fps_milli: u64,
    pub frame_gap_p99_tick: i64,
    pub max_frame_gap_tick: i64,
    pub frame_gap_sample_count: u64,
    pub capture_queue_peak: u32,
    pub capture_queue_capacity: u32,
    pub input_queue_overflow: u64,
    pub failures: Vec<String>,
}

impl RecordingStabilityReport {
    pub fn evaluate(
        metrics: &ProjectMetrics,
        actual_duration_tick: i64,
        required_duration_tick: i64,
        capture_queue_capacity: u32,
    ) -> Self {
        let capture = &metrics.capture;
        let backpressure_per_million =
            rate_per_million(capture.dropped_by_backpressure, capture.frames_received);
        let average_encoded_fps_milli =
            average_fps_milli(capture.frames_encoded, actual_duration_tick);
        let mut failures = Vec::new();

        if actual_duration_tick < required_duration_tick {
            failures.push(format!(
                "duration {actual_duration_tick} is below required {required_duration_tick} ticks"
            ));
        }
        if capture.frames_received == 0 {
            failures.push("no capture frames were received".into());
        }
        if capture.frames_encoded == 0 {
            failures.push("no video frames were encoded".into());
        }
        if ratio_exceeds_per_million(
            capture.dropped_by_backpressure,
            capture.frames_received,
            MAX_BACKPRESSURE_PER_MILLION,
        ) {
            failures.push(format!(
                "backpressure rate {backpressure_per_million} per million exceeds {MAX_BACKPRESSURE_PER_MILLION}"
            ));
        }
        if average_fps_exceeds(
            capture.frames_encoded,
            actual_duration_tick,
            MAX_AVERAGE_ENCODED_FPS_MILLI,
        ) {
            failures.push(format!(
                "average encoded frame rate {average_encoded_fps_milli} milli-FPS exceeds {MAX_AVERAGE_ENCODED_FPS_MILLI}"
            ));
        }
        if capture.frame_gap_sample_count == 0 {
            failures.push("no capture frame-gap samples were recorded".into());
        }
        if capture.frame_gap_p99_tick > MAX_CAPTURE_FRAME_GAP_P99_TICK {
            failures.push(format!(
                "capture frame-gap P99 {} exceeds {} ticks",
                capture.frame_gap_p99_tick, MAX_CAPTURE_FRAME_GAP_P99_TICK
            ));
        }
        if capture.max_frame_gap_tick > MAX_CAPTURE_FRAME_GAP_TICK {
            failures.push(format!(
                "maximum capture frame gap {} exceeds {} ticks",
                capture.max_frame_gap_tick, MAX_CAPTURE_FRAME_GAP_TICK
            ));
        }
        if capture.capture_queue_peak > capture_queue_capacity {
            failures.push(format!(
                "capture queue peak {} exceeds capacity {capture_queue_capacity}",
                capture.capture_queue_peak
            ));
        }
        if metrics.input.queue_overflow != 0 {
            failures.push(format!(
                "input queue overflow count is {}",
                metrics.input.queue_overflow
            ));
        }

        Self {
            passed: failures.is_empty(),
            required_duration_tick,
            actual_duration_tick,
            backpressure_per_million,
            average_encoded_fps_milli,
            frame_gap_p99_tick: capture.frame_gap_p99_tick,
            max_frame_gap_tick: capture.max_frame_gap_tick,
            frame_gap_sample_count: capture.frame_gap_sample_count,
            capture_queue_peak: capture.capture_queue_peak,
            capture_queue_capacity,
            input_queue_overflow: metrics.input.queue_overflow,
            failures,
        }
    }
}

fn rate_per_million(numerator: u64, denominator: u64) -> u64 {
    if denominator == 0 {
        return if numerator == 0 { 0 } else { u64::MAX };
    }
    u64::try_from((u128::from(numerator) * 1_000_000_u128) / u128::from(denominator))
        .unwrap_or(u64::MAX)
}

fn ratio_exceeds_per_million(numerator: u64, denominator: u64, limit: u64) -> bool {
    denominator == 0
        || u128::from(numerator) * 1_000_000_u128 > u128::from(denominator) * u128::from(limit)
}

fn average_fps_milli(frames: u64, duration_tick: i64) -> u64 {
    let Ok(duration_tick) = u128::try_from(duration_tick) else {
        return u64::MAX;
    };
    if duration_tick == 0 {
        return if frames == 0 { 0 } else { u64::MAX };
    }
    u64::try_from(
        (u128::from(frames) * u128::try_from(TICKS_PER_SECOND).unwrap_or(0) * 1_000_u128)
            / duration_tick,
    )
    .unwrap_or(u64::MAX)
}

fn average_fps_exceeds(frames: u64, duration_tick: i64, limit_fps_milli: u64) -> bool {
    let Ok(duration_tick) = u128::try_from(duration_tick) else {
        return true;
    };
    duration_tick == 0
        || u128::from(frames) * u128::try_from(TICKS_PER_SECOND).unwrap_or(0) * 1_000_u128
            > duration_tick * u128::from(limit_fps_milli)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_metrics_at_the_v01_stability_limits() {
        let metrics = ProjectMetrics {
            capture: CaptureMetrics {
                frames_received: 10_000,
                frames_encoded: 600,
                dropped_by_backpressure: 50,
                max_frame_gap_tick: MAX_CAPTURE_FRAME_GAP_TICK,
                frame_gap_p99_tick: MAX_CAPTURE_FRAME_GAP_P99_TICK,
                frame_gap_sample_count: 999,
                capture_queue_peak: 4,
                ..CaptureMetrics::default()
            },
            ..ProjectMetrics::default()
        };
        let report = RecordingStabilityReport::evaluate(
            &metrics,
            10 * TICKS_PER_SECOND,
            10 * TICKS_PER_SECOND,
            4,
        );
        assert!(report.passed, "{:?}", report.failures);
        assert_eq!(report.backpressure_per_million, 5_000);
        assert_eq!(report.average_encoded_fps_milli, 60_000);
    }

    #[test]
    fn reports_each_breached_stability_limit() {
        let metrics = ProjectMetrics {
            capture: CaptureMetrics {
                frames_received: 100,
                frames_encoded: 100,
                dropped_by_backpressure: 1,
                max_frame_gap_tick: MAX_CAPTURE_FRAME_GAP_TICK + 1,
                frame_gap_p99_tick: MAX_CAPTURE_FRAME_GAP_P99_TICK + 1,
                frame_gap_sample_count: 1,
                capture_queue_peak: 5,
                ..CaptureMetrics::default()
            },
            input: InputMetrics {
                queue_overflow: 1,
                ..InputMetrics::default()
            },
            ..ProjectMetrics::default()
        };
        let report =
            RecordingStabilityReport::evaluate(&metrics, TICKS_PER_SECOND, 2 * TICKS_PER_SECOND, 4);
        assert!(!report.passed);
        assert_eq!(report.failures.len(), 7);
    }

    #[test]
    fn does_not_round_a_threshold_breach_down_to_a_pass() {
        let metrics = ProjectMetrics {
            capture: CaptureMetrics {
                frames_received: 2_000_000,
                frames_encoded: 121_001,
                dropped_by_backpressure: 10_001,
                max_frame_gap_tick: 100_000,
                frame_gap_p99_tick: 100_000,
                frame_gap_sample_count: 1,
                capture_queue_peak: 1,
                ..CaptureMetrics::default()
            },
            ..ProjectMetrics::default()
        };
        let duration = 2_000 * TICKS_PER_SECOND;
        let report = RecordingStabilityReport::evaluate(&metrics, duration, duration, 4);
        assert_eq!(report.backpressure_per_million, 5_000);
        assert_eq!(report.average_encoded_fps_milli, 60_500);
        assert!(!report.passed);
        assert_eq!(report.failures.len(), 2);
    }
}
