//! Source-time automatic camera policy; legacy projects retain their original camera track.
use crate::{CameraState, CameraTrack, CursorEvent, NormalizedPoint, SegmentOrigin, TimeTick};
use serde::{Deserialize, Serialize};
const TRANSITION: i64 = 2_500_000;
const STEP: i64 = 333_333;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FollowMode {
    #[default]
    Smooth,
    Click,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AutoCameraConfig {
    pub enabled: bool,
    pub mode: FollowMode,
    pub zoom: f64,
    pub response_ms: u32,
}
impl Default for AutoCameraConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: FollowMode::Smooth,
            zoom: 1.5,
            response_ms: 250,
        }
    }
}
impl AutoCameraConfig {
    pub fn valid(self) -> bool {
        self.zoom.is_finite()
            && (1.0..=3.0).contains(&self.zoom)
            && (80..=1000).contains(&self.response_ms)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoCameraRange {
    pub start: TimeTick,
    pub end: TimeTick,
    pub config: AutoCameraConfig,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AutoCameraSettings {
    pub default: AutoCameraConfig,
    pub ranges: Vec<AutoCameraRange>,
}
impl AutoCameraSettings {
    pub fn valid(&self) -> bool {
        self.default.valid()
            && self.ranges.len() <= 4096
            && self
                .ranges
                .iter()
                .all(|r| r.start >= TimeTick::ZERO && r.end > r.start && r.config.valid())
            && self.ranges.windows(2).all(|r| r[0].end <= r[1].start)
    }
    pub fn at(&self, tick: TimeTick) -> AutoCameraConfig {
        let upper = self.ranges.partition_point(|r| r.start <= tick);
        upper
            .checked_sub(1)
            .and_then(|i| self.ranges.get(i))
            .filter(|r| tick < r.end)
            .map_or(self.default, |r| r.config)
    }
    pub fn set_range(&mut self, start: TimeTick, end: TimeTick, config: AutoCameraConfig) {
        if start < TimeTick::ZERO || end <= start || !config.valid() {
            return;
        }
        let mut result = Vec::new();
        for range in &self.ranges {
            if range.end <= start || range.start >= end {
                result.push(range.clone());
            } else {
                if range.start < start {
                    result.push(AutoCameraRange {
                        start: range.start,
                        end: start,
                        config: range.config,
                    });
                }
                if range.end > end {
                    result.push(AutoCameraRange {
                        start: end,
                        end: range.end,
                        config: range.config,
                    });
                }
            }
        }
        result.push(AutoCameraRange { start, end, config });
        result.sort_by_key(|r| r.start);
        self.ranges = result;
    }
    /// Transitions start at the change timestamp, so toggling never edits earlier frames.
    pub fn evaluate(
        &self,
        tick: TimeTick,
        track: &CameraTrack,
        events: &[CursorEvent],
        cache: &mut FollowCache,
    ) -> CameraState {
        let mut current = self.at(TimeTick::ZERO);
        let mut boundary = TimeTick::ZERO;
        let mut previous = CameraState::BASE;
        for range in &self.ranges {
            for t in [range.start, range.end] {
                if t <= boundary || t > tick {
                    continue;
                }
                let next = self.at(t);
                if next == current {
                    continue;
                }
                let target = evaluate_config(current, t, track, events, cache);
                let elapsed = t.0 - boundary.0;
                previous = if elapsed >= TRANSITION {
                    target
                } else {
                    previous.interpolate(
                        target,
                        crate::Easing::Smootherstep.apply(elapsed as f64 / TRANSITION as f64),
                    )
                };
                boundary = t;
                current = next;
            }
        }
        let target = evaluate_config(current, tick, track, events, cache);
        let elapsed = tick.0 - boundary.0;
        if elapsed >= TRANSITION {
            target
        } else {
            previous.interpolate(
                target,
                crate::Easing::Smootherstep.apply(elapsed as f64 / TRANSITION as f64),
            )
        }
    }
}
fn evaluate_config(
    config: AutoCameraConfig,
    tick: TimeTick,
    track: &CameraTrack,
    events: &[CursorEvent],
    cache: &mut FollowCache,
) -> CameraState {
    if !config.enabled || events.is_empty() {
        return CameraState::BASE;
    }
    match config.mode {
        FollowMode::Smooth => cache.evaluate(events, config, tick),
        FollowMode::Click => {
            let mut state = CameraState::BASE;
            for segment in &track.segments {
                if segment.start_tick > tick {
                    break;
                }
                if segment.origin == SegmentOrigin::Manual
                    || segment.locked
                    || segment.user_modified
                {
                    continue;
                }
                if tick < segment.end_tick {
                    let fraction = (tick.0 - segment.start_tick.0) as f64
                        / (segment.end_tick.0 - segment.start_tick.0) as f64;
                    state = state.interpolate(segment.to, segment.easing.apply(fraction));
                    break;
                }
                state = segment.to;
            }
            let zoom = 1.0 + (state.scale - 1.0) * (config.zoom - 1.0) / 0.6;
            CameraState::focused(
                NormalizedPoint {
                    x: state.center_x,
                    y: state.center_y,
                },
                zoom.clamp(1.0, 3.0),
            )
            .unwrap_or(CameraState::BASE)
        }
    }
}
pub fn manual_active(track: &CameraTrack, tick: TimeTick) -> bool {
    track
        .segments
        .iter()
        .rev()
        .find(|s| s.start_tick <= tick)
        .is_some_and(|s| s.origin == SegmentOrigin::Manual || s.locked || s.user_modified)
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FollowCache {
    paths: Vec<(f64, u32, Vec<CameraState>)>,
}
impl FollowCache {
    fn evaluate(
        &mut self,
        events: &[CursorEvent],
        config: AutoCameraConfig,
        tick: TimeTick,
    ) -> CameraState {
        let index =
            if let Some(i) = self.paths.iter().position(|(zoom, response, _)| {
                *zoom == config.zoom && *response == config.response_ms
            }) {
                i
            } else {
                if self.paths.len() >= 2 {
                    self.paths.remove(0);
                }
                self.paths
                    .push((config.zoom, config.response_ms, build_path(events, config)));
                self.paths.len() - 1
            };
        let path = &self.paths[index].2;
        let step = path_step(events);
        let index = usize::try_from(tick.0.max(0) / step).unwrap_or(usize::MAX);
        match (path.get(index), path.get(index.saturating_add(1))) {
            (Some(a), Some(b)) => a.interpolate(*b, (tick.0 % step) as f64 / step as f64),
            _ => path.last().copied().unwrap_or(CameraState::BASE),
        }
    }
}
fn path_step(events: &[CursorEvent]) -> i64 {
    (events
        .last()
        .map_or(0, |e| e.time_tick.0)
        .saturating_add(20_000_000)
        / 500_000)
        .max(STEP)
}
fn build_path(events: &[CursorEvent], config: AutoCameraConfig) -> Vec<CameraState> {
    let end = events
        .last()
        .map_or(0, |e| e.time_tick.0)
        .saturating_add(20_000_000);
    let step = path_step(events);
    let mut path = Vec::new();
    let mut state = CameraState::BASE;
    let mut upper = 0;
    let mut last_visible = 0;
    let alpha = 1.0 - (-(step as f64) / (f64::from(config.response_ms) * 10_000.0)).exp();
    let mut tick = 0;
    while tick <= end {
        while upper < events.len() && events[upper].time_tick.0 <= tick {
            upper += 1;
        }
        let event = upper.checked_sub(1).and_then(|i| events.get(i));
        let visible = event.is_some_and(|e| {
            e.visible
                && (0.0..=1.0).contains(&e.normalized_x)
                && (0.0..=1.0).contains(&e.normalized_y)
        });
        if visible {
            last_visible = tick;
        }
        let zoom = if visible || (upper > 0 && tick - last_visible < 8_000_000) {
            config.zoom
        } else {
            1.0
        };
        let mut target = NormalizedPoint {
            x: state.center_x,
            y: state.center_y,
        };
        if let Some(event) = event.filter(|_| visible) {
            let safe = 0.18 / config.zoom;
            target.x += (event.normalized_x - state.center_x - safe).max(0.0)
                + (event.normalized_x - state.center_x + safe).min(0.0);
            target.y += (event.normalized_y - state.center_y - safe).max(0.0)
                + (event.normalized_y - state.center_y + safe).min(0.0);
        }
        let desired = CameraState::focused(target, zoom).unwrap_or(CameraState::BASE);
        state = state.interpolate(desired, alpha);
        path.push(state);
        let Some(next) = tick.checked_add(step) else {
            break;
        };
        tick = next;
    }
    path
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AudioSettings {
    pub enabled: bool,
    /// Mute source video/recording audio without discarding its mix levels.
    pub source_muted: bool,
    pub system_gain: f64,
    pub microphone_gain: f64,
    pub imported_gain: f64,
}
impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            source_muted: false,
            system_gain: 1.0,
            microphone_gain: 1.0,
            imported_gain: 1.0,
        }
    }
}
impl AudioSettings {
    pub fn valid(self) -> bool {
        [self.system_gain, self.microphone_gain, self.imported_gain]
            .iter()
            .all(|v| v.is_finite() && (0.0..=2.0).contains(v))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_replacement_preserves_surrounding_source_time() {
        let mut s = AutoCameraSettings::default();
        let on = AutoCameraConfig {
            enabled: true,
            ..Default::default()
        };
        s.set_range(TimeTick(10), TimeTick(90), on);
        s.set_range(TimeTick(30), TimeTick(50), AutoCameraConfig::default());
        assert!(s.valid());
        assert!(s.at(TimeTick(20)).enabled);
        assert!(!s.at(TimeTick(40)).enabled);
        assert!(s.at(TimeTick(60)).enabled);
        assert!(!s.at(TimeTick(100)).enabled);
        assert_eq!(
            serde_json::from_str::<AutoCameraSettings>(&serde_json::to_string(&s).unwrap())
                .unwrap(),
            s
        );
    }
    #[test]
    fn follows_movement_without_clicks_and_never_exposes_edges() {
        let events = (0..100)
            .map(|i| CursorEvent {
                schema_version: 1,
                id: u64::try_from(i).unwrap(),
                time_tick: TimeTick::from_millis(i * 40),
                kind: crate::CursorEventKind::Move,
                desktop_x: 0,
                desktop_y: 0,
                content_x: 0,
                content_y: 0,
                normalized_x: i as f64 / 99.0,
                normalized_y: 0.95,
                visible: true,
                geometry_revision: 0,
            })
            .collect::<Vec<_>>();
        let config = AutoCameraConfig {
            enabled: true,
            ..Default::default()
        };
        let path = build_path(&events, config);
        assert!(path.last().unwrap().center_x > 0.6);
        for state in path {
            state.validate().unwrap();
        }
    }
    #[test]
    fn leaving_region_returns_to_full_frame() {
        let mut e = CursorEvent {
            schema_version: 1,
            id: 1,
            time_tick: TimeTick::ZERO,
            kind: crate::CursorEventKind::Move,
            desktop_x: 0,
            desktop_y: 0,
            content_x: 0,
            content_y: 0,
            normalized_x: 0.9,
            normalized_y: 0.5,
            visible: true,
            geometry_revision: 0,
        };
        let mut events = vec![e.clone()];
        e.time_tick = TimeTick::from_millis(1000);
        e.visible = false;
        events.push(e);
        let path = build_path(
            &events,
            AutoCameraConfig {
                enabled: true,
                ..Default::default()
            },
        );
        assert!(path.last().unwrap().scale < 1.02);
    }
}
