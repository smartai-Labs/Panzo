use crate::camera::{
    CAMERA_SCHEMA_VERSION, CameraSegment, CameraSegmentKind, CameraState, CameraTrack, Easing,
    GeneratorMetadata, SegmentOrigin, SourceRevision,
};
use crate::events::ClickEvent;
use crate::geometry::NormalizedPoint;
use crate::time::{TICKS_PER_SECOND, TimeTick};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MIN_TRANSITION_TICK: TimeTick = TimeTick(2_000_000);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannerParams {
    pub parameter_set: String,
    pub scale: f64,
    pub pre_roll: TimeTick,
    pub zoom_in_duration: TimeTick,
    pub zoom_out_duration: TimeTick,
    pub cluster_gap: TimeTick,
    pub near_distance: f64,
    pub far_distance: f64,
    pub pan_duration: TimeTick,
    pub max_pan_duration: TimeTick,
    pub max_center_speed: f64,
    pub min_camera_decision_interval: TimeTick,
    pub focus_hold_after_last_click: TimeTick,
}

impl Default for PlannerParams {
    fn default() -> Self {
        Self {
            parameter_set: "default-v1".into(),
            scale: 1.6,
            pre_roll: TimeTick::from_millis(300),
            zoom_in_duration: TimeTick::from_millis(350),
            zoom_out_duration: TimeTick::from_millis(450),
            cluster_gap: TimeTick::from_millis(1_500),
            near_distance: 0.25,
            far_distance: 0.45,
            pan_duration: TimeTick::from_millis(350),
            max_pan_duration: TimeTick::from_millis(800),
            max_center_speed: 1.0,
            min_camera_decision_interval: TimeTick::from_millis(500),
            focus_hold_after_last_click: TimeTick::from_millis(1_200),
        }
    }
}

impl PlannerParams {
    pub fn validate(&self) -> Result<(), PlannerError> {
        if !self.scale.is_finite() || self.scale < 1.0 {
            return Err(PlannerError::InvalidParameter("scale"));
        }
        if !self.near_distance.is_finite()
            || !self.far_distance.is_finite()
            || self.near_distance < 0.0
            || self.far_distance <= self.near_distance
        {
            return Err(PlannerError::InvalidParameter("nearDistance/farDistance"));
        }
        if !self.max_center_speed.is_finite() || self.max_center_speed <= 0.0 {
            return Err(PlannerError::InvalidParameter("maxCenterSpeed"));
        }

        for (name, tick) in [
            ("preRoll", self.pre_roll),
            ("zoomInDuration", self.zoom_in_duration),
            ("zoomOutDuration", self.zoom_out_duration),
            ("clusterGap", self.cluster_gap),
            ("panDuration", self.pan_duration),
            ("maxPanDuration", self.max_pan_duration),
            (
                "minCameraDecisionInterval",
                self.min_camera_decision_interval,
            ),
            ("focusHoldAfterLastClick", self.focus_hold_after_last_click),
        ] {
            if tick < TimeTick::ZERO {
                return Err(PlannerError::InvalidParameter(name));
            }
        }
        if self.max_pan_duration < self.pan_duration {
            return Err(PlannerError::InvalidParameter("maxPanDuration"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PlannerInput<'a> {
    pub recording_duration: TimeTick,
    pub clicks: &'a [ClickEvent],
    pub click_track_hash: &'a str,
    pub capture_geometry_hash: &'a str,
}

#[derive(Debug, Clone)]
pub struct CameraPlanner {
    params: PlannerParams,
    generator_version: String,
}

impl CameraPlanner {
    pub fn new(
        params: PlannerParams,
        generator_version: impl Into<String>,
    ) -> Result<Self, PlannerError> {
        params.validate()?;
        Ok(Self {
            params,
            generator_version: generator_version.into(),
        })
    }

    pub fn params(&self) -> &PlannerParams {
        &self.params
    }

    pub fn plan(&self, input: PlannerInput<'_>) -> Result<CameraTrack, PlannerError> {
        if input.recording_duration <= TimeTick::ZERO {
            return Err(PlannerError::InvalidRecordingDuration(
                input.recording_duration,
            ));
        }

        let decisions = self.decisions(input.clicks, input.recording_duration);
        let mut builder = SegmentBuilder::new(input.recording_duration);

        if let Some(first) = decisions.first() {
            self.add_zoom_in(&mut builder, first);
            let mut previous = first;

            for decision in decisions.iter().skip(1) {
                let gap = decision.time_tick.0 - previous.time_tick.0;
                if gap > self.params.cluster_gap.0
                    && self.can_return_to_base_before(&builder, previous, decision)
                {
                    let out_start =
                        TimeTick(previous.time_tick.0 + self.params.focus_hold_after_last_click.0);
                    builder.add_transition(
                        CameraSegmentKind::ZoomOut,
                        out_start,
                        self.params.zoom_out_duration,
                        CameraState::BASE,
                        previous.source_click_ids.clone(),
                    );
                    self.add_zoom_in(&mut builder, decision);
                } else {
                    self.add_continuous_move(&mut builder, previous, decision);
                }
                previous = decision;
            }

            let out_start =
                TimeTick(previous.time_tick.0 + self.params.focus_hold_after_last_click.0);
            builder.add_transition(
                CameraSegmentKind::ZoomOut,
                out_start,
                self.params.zoom_out_duration,
                CameraState::BASE,
                previous.source_click_ids.clone(),
            );
        }

        let track = CameraTrack {
            schema_version: CAMERA_SCHEMA_VERSION,
            generator: GeneratorMetadata {
                name: "rule-planner".into(),
                version: self.generator_version.clone(),
                parameter_set: self.params.parameter_set.clone(),
            },
            source_revision: SourceRevision {
                click_track_hash: input.click_track_hash.into(),
                capture_geometry_hash: input.capture_geometry_hash.into(),
            },
            base_state: CameraState::BASE,
            segments: builder.segments,
        };
        track.validate()?;
        Ok(track)
    }

    fn decisions(&self, clicks: &[ClickEvent], recording_duration: TimeTick) -> Vec<Decision> {
        let mut candidates: Vec<_> = clicks
            .iter()
            .filter(|click| click.triggers_camera())
            .filter(|click| {
                click.time_tick >= TimeTick::ZERO && click.time_tick <= recording_duration
            })
            .filter_map(|click| {
                click.normalized_point().map(|point| Decision {
                    time_tick: click.time_tick,
                    point,
                    source_click_ids: vec![click.id.clone()],
                })
            })
            .collect();

        candidates.sort_by(|left, right| {
            left.time_tick
                .cmp(&right.time_tick)
                .then_with(|| left.source_click_ids[0].cmp(&right.source_click_ids[0]))
        });

        let mut decisions: Vec<Decision> = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            if let Some(previous) = decisions.last_mut()
                && candidate.time_tick.0 - previous.time_tick.0
                    <= self.params.min_camera_decision_interval.0
            {
                previous.time_tick = candidate.time_tick;
                previous.point = candidate.point;
                previous.source_click_ids.extend(candidate.source_click_ids);
                continue;
            }
            decisions.push(candidate);
        }
        decisions
    }

    fn add_zoom_in(&self, builder: &mut SegmentBuilder, decision: &Decision) {
        let start_tick = TimeTick(decision.time_tick.0.saturating_sub(self.params.pre_roll.0));
        let target = CameraState::focused(decision.point, self.params.scale)
            .expect("validated planner scale and normalized event point");
        builder.add_transition(
            CameraSegmentKind::ZoomIn,
            start_tick,
            self.params.zoom_in_duration,
            target,
            decision.source_click_ids.clone(),
        );
    }

    fn add_continuous_move(
        &self,
        builder: &mut SegmentBuilder,
        previous: &Decision,
        decision: &Decision,
    ) {
        let distance = previous.point.distance(decision.point);
        let target = CameraState::focused(decision.point, self.params.scale)
            .expect("validated planner scale and normalized event point");

        // Use the center we actually held, not the last click: tiny movements must
        // accumulate before a pan, including when edge clamping changes the target.
        if builder.state.is_micro_pan_to(target) {
            return;
        }

        if distance >= self.params.far_distance {
            let total_reframe_duration =
                self.params.zoom_out_duration.0 + self.params.zoom_in_duration.0;
            let ideal_start = decision.time_tick.0.saturating_sub(total_reframe_duration);
            if builder.last_end_tick.0 <= ideal_start {
                builder.add_transition(
                    CameraSegmentKind::ReframeOut,
                    TimeTick(ideal_start),
                    self.params.zoom_out_duration,
                    CameraState::BASE,
                    decision.source_click_ids.clone(),
                );
                builder.add_transition(
                    CameraSegmentKind::ReframeIn,
                    builder.last_end_tick,
                    self.params.zoom_in_duration,
                    target,
                    decision.source_click_ids.clone(),
                );
                return;
            }
        }

        let duration = self.pan_duration(distance);
        let start_tick = TimeTick(
            decision
                .time_tick
                .0
                .saturating_sub(duration.0 / 2)
                .max(builder.last_end_tick.0),
        );
        builder.add_transition(
            CameraSegmentKind::Pan,
            start_tick,
            duration,
            target,
            decision.source_click_ids.clone(),
        );
    }

    fn pan_duration(&self, distance: f64) -> TimeTick {
        let speed_duration =
            (distance / self.params.max_center_speed * TICKS_PER_SECOND as f64).round() as i64;
        TimeTick(speed_duration.clamp(self.params.pan_duration.0, self.params.max_pan_duration.0))
    }

    fn can_return_to_base_before(
        &self,
        builder: &SegmentBuilder,
        previous: &Decision,
        next: &Decision,
    ) -> bool {
        let out_start = (previous.time_tick.0 + self.params.focus_hold_after_last_click.0)
            .max(builder.last_end_tick.0);
        let out_end = out_start + self.params.zoom_out_duration.0;
        let next_in_start = next.time_tick.0.saturating_sub(self.params.pre_roll.0);
        out_end <= next_in_start
    }
}

#[derive(Debug, Clone)]
struct Decision {
    time_tick: TimeTick,
    point: NormalizedPoint,
    source_click_ids: Vec<String>,
}

struct SegmentBuilder {
    recording_duration: TimeTick,
    last_end_tick: TimeTick,
    state: CameraState,
    next_id: u64,
    segments: Vec<CameraSegment>,
}

impl SegmentBuilder {
    fn new(recording_duration: TimeTick) -> Self {
        Self {
            recording_duration,
            last_end_tick: TimeTick::ZERO,
            state: CameraState::BASE,
            next_id: 1,
            segments: Vec::new(),
        }
    }

    fn add_transition(
        &mut self,
        kind: CameraSegmentKind,
        requested_start: TimeTick,
        requested_duration: TimeTick,
        target: CameraState,
        source_click_ids: Vec<String>,
    ) {
        if self.state.approx_eq(target) || requested_duration <= TimeTick::ZERO {
            return;
        }

        let start_tick = TimeTick(requested_start.0.max(self.last_end_tick.0).max(0));
        if start_tick >= self.recording_duration {
            return;
        }
        let end_tick = TimeTick(
            start_tick
                .0
                .saturating_add(requested_duration.0)
                .min(self.recording_duration.0),
        );
        if end_tick.0 - start_tick.0 < MIN_TRANSITION_TICK.0 {
            return;
        }

        let segment = CameraSegment {
            id: format!("camera-segment-{:04}", self.next_id),
            kind,
            start_tick,
            end_tick,
            from: self.state,
            to: target,
            easing: Easing::Smootherstep,
            origin: SegmentOrigin::Auto,
            source_click_ids,
            locked: false,
            user_modified: false,
            focus: None,
        };
        self.next_id += 1;
        self.last_end_tick = end_tick;
        self.state = target;
        self.segments.push(segment);
    }
}

#[derive(Debug, Error)]
pub enum PlannerError {
    #[error("invalid planner parameter: {0}")]
    InvalidParameter(&'static str),
    #[error("recording duration must be positive, got {0:?}")]
    InvalidRecordingDuration(TimeTick),
    #[error(transparent)]
    InvalidCameraTrack(#[from] crate::camera::CameraValidationError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EVENT_SCHEMA_VERSION, MouseAction, MouseButton};

    fn click(id: &str, millis: i64, x: f64, y: f64) -> ClickEvent {
        ClickEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            id: id.into(),
            time_tick: TimeTick::from_millis(millis),
            button: MouseButton::Left,
            action: MouseAction::Down,
            desktop_x: 0,
            desktop_y: 0,
            normalized_x: x,
            normalized_y: y,
            inside_capture: true,
            geometry_revision: 0,
        }
    }

    fn plan(clicks: &[ClickEvent]) -> CameraTrack {
        CameraPlanner::new(PlannerParams::default(), "0.1.0")
            .unwrap()
            .plan(PlannerInput {
                recording_duration: TimeTick::from_millis(5_000),
                clicks,
                click_track_hash: "sha256:clicks",
                capture_geometry_hash: "sha256:geometry",
            })
            .unwrap()
    }

    #[test]
    fn single_click_generates_zoom_hold_and_zoom_out() {
        let track = plan(&[click("click-1", 1_000, 0.5, 0.5)]);
        assert_eq!(track.segments.len(), 2);
        assert_eq!(track.segments[0].kind, CameraSegmentKind::ZoomIn);
        assert_eq!(track.segments[1].kind, CameraSegmentKind::ZoomOut);
        assert_eq!(
            track.evaluate(TimeTick::from_millis(5_000)),
            CameraState::BASE
        );
    }

    #[test]
    fn near_clicks_pan_without_breathing() {
        let track = plan(&[
            click("click-1", 1_000, 0.5, 0.5),
            click("click-2", 2_000, 0.6, 0.55),
        ]);
        assert!(
            track
                .segments
                .iter()
                .any(|segment| segment.kind == CameraSegmentKind::Pan)
        );
        assert_eq!(
            track
                .segments
                .iter()
                .filter(|segment| segment.kind == CameraSegmentKind::ZoomIn)
                .count(),
            1
        );
    }

    #[test]
    fn far_clicks_reframe_when_time_allows() {
        let track = plan(&[
            click("click-1", 800, 0.1, 0.5),
            click("click-2", 2_200, 0.9, 0.5),
        ]);
        assert!(
            track
                .segments
                .iter()
                .any(|segment| segment.kind == CameraSegmentKind::ReframeOut)
        );
        assert!(
            track
                .segments
                .iter()
                .any(|segment| segment.kind == CameraSegmentKind::ReframeIn)
        );
    }

    #[test]
    fn tiny_click_movements_hold_focus_but_accumulated_movement_eventually_pans() {
        let track = plan(&[
            click("click-1", 1000, 0.5, 0.5),
            click("click-2", 1800, 0.507, 0.5),
            click("click-3", 2600, 0.514, 0.5),
            click("click-4", 3400, 0.522, 0.5),
        ]);
        let pans: Vec<_> = track
            .segments
            .iter()
            .filter(|s| s.kind == CameraSegmentKind::Pan)
            .collect();
        assert_eq!(pans.len(), 1);
        assert_eq!(pans[0].source_click_ids, ["click-4"]);
        assert_eq!(pans[0].from.center_x, 0.5);
        assert_eq!(pans[0].to.center_x, 0.522);
        track.validate().unwrap();
    }

    #[test]
    fn edge_clicks_hold_the_clamped_focus_and_extend_the_hold() {
        let track = plan(&[
            click("first", 1000, 0.01, 0.99),
            click("nearby", 2000, 0.02, 0.98),
        ]);
        assert_eq!(track.segments.len(), 2);
        assert_eq!(track.segments[1].kind, CameraSegmentKind::ZoomOut);
        assert_eq!(track.segments[1].start_tick, TimeTick::from_millis(3200));
    }

    #[test]
    fn dense_clicks_merge_to_last_focus() {
        let track = plan(&[
            click("click-1", 1_000, 0.4, 0.4),
            click("click-2", 1_200, 0.6, 0.6),
        ]);
        assert_eq!(track.segments[0].source_click_ids.len(), 2);
        assert_eq!(
            track
                .segments
                .iter()
                .filter(|segment| segment.kind == CameraSegmentKind::ZoomIn)
                .count(),
            1
        );
    }

    #[test]
    fn edge_clicks_are_clamped() {
        let track = plan(&[click("click-1", 1_000, 1.0, 1.0)]);
        let focused = track.segments[0].to;
        let expected = 1.0 - 0.5 / 1.6;
        assert!((focused.center_x - expected).abs() < 1.0e-9);
        assert!((focused.center_y - expected).abs() < 1.0e-9);
        track.validate().unwrap();
    }

    #[test]
    fn output_is_hash_stable() {
        let clicks = [
            click("click-1", 1_000, 0.4, 0.4),
            click("click-2", 2_000, 0.6, 0.6),
        ];
        let expected = plan(&clicks).canonical_hash().unwrap();
        for _ in 0..100 {
            assert_eq!(plan(&clicks).canonical_hash().unwrap(), expected);
        }
    }
}
