use crate::geometry::{GeometryError, NormalizedPoint};
use crate::time::TimeTick;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use thiserror::Error;

pub const CAMERA_SCHEMA_VERSION: u32 = 1;
const APPROX_EPSILON: f64 = 1.0e-9;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraState {
    pub center_x: f64,
    pub center_y: f64,
    pub scale: f64,
}

impl CameraState {
    pub const BASE: Self = Self {
        center_x: 0.5,
        center_y: 0.5,
        scale: 1.0,
    };

    pub fn focused(point: NormalizedPoint, scale: f64) -> Result<Self, GeometryError> {
        let clamped = point.clamped_for_scale(scale)?;
        Ok(Self {
            center_x: clamped.x,
            center_y: clamped.y,
            scale,
        })
    }

    pub fn validate(self) -> Result<(), CameraValidationError> {
        if !self.center_x.is_finite() || !self.center_y.is_finite() || !self.scale.is_finite() {
            return Err(CameraValidationError::NonFiniteState);
        }
        if self.scale < 1.0 {
            return Err(CameraValidationError::InvalidScale(self.scale));
        }

        let point = NormalizedPoint::new(self.center_x, self.center_y)
            .map_err(|_| CameraValidationError::CenterOutOfRange)?;
        let clamped = point
            .clamped_for_scale(self.scale)
            .map_err(|_| CameraValidationError::CenterOutOfRange)?;
        if (self.center_x - clamped.x).abs() > APPROX_EPSILON
            || (self.center_y - clamped.y).abs() > APPROX_EPSILON
        {
            return Err(CameraValidationError::CenterNotClamped);
        }
        Ok(())
    }

    pub fn interpolate(self, target: Self, progress: f64) -> Self {
        let progress = progress.clamp(0.0, 1.0);
        if progress == 0.0 {
            return self;
        }
        if progress == 1.0 {
            return target;
        }
        Self {
            center_x: self.center_x + (target.center_x - self.center_x) * progress,
            center_y: self.center_y + (target.center_y - self.center_y) * progress,
            // Interpolate the crop's extent, not magnification. All four crop edges then
            // move monotonically; an edge focus cannot slide out of view and back in.
            scale: 1.0 / ((1.0 - progress) / self.scale + progress / target.scale),
        }
    }

    /// Ignore automatic center adjustments smaller than 2.5% of the current viewport.
    /// Compare against the held center, so repeated small movements can still accumulate
    /// into a meaningful pan. Zoom changes are never suppressed.
    pub fn is_micro_pan_to(self, target: Self) -> bool {
        (self.scale - target.scale).abs() <= APPROX_EPSILON
            && (self.center_x - target.center_x).hypot(self.center_y - target.center_y) * self.scale
                <= 0.025
    }

    pub fn approx_eq(self, other: Self) -> bool {
        (self.center_x - other.center_x).abs() <= APPROX_EPSILON
            && (self.center_y - other.center_y).abs() <= APPROX_EPSILON
            && (self.scale - other.scale).abs() <= APPROX_EPSILON
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraSegmentKind {
    ZoomIn,
    Pan,
    ReframeOut,
    ReframeIn,
    ZoomOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Easing {
    Smootherstep,
}

impl Easing {
    pub fn apply(self, progress: f64) -> f64 {
        let t = progress.clamp(0.0, 1.0);
        match self {
            Self::Smootherstep => {
                let t2 = t * t;
                let t3 = t2 * t;
                t3 * (t * (t * 6.0 - 15.0) + 10.0)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentOrigin {
    Auto,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraSegment {
    pub id: String,
    pub kind: CameraSegmentKind,
    pub start_tick: TimeTick,
    pub end_tick: TimeTick,
    pub from: CameraState,
    pub to: CameraState,
    pub easing: Easing,
    pub origin: SegmentOrigin,
    pub source_click_ids: Vec<String>,
    pub locked: bool,
    pub user_modified: bool,
    /// User-selected source point, independent of the crop's bounded center.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<NormalizedPoint>,
}

impl CameraSegment {
    pub fn focus_point(&self) -> NormalizedPoint {
        self.focus.unwrap_or(NormalizedPoint {
            x: self.to.center_x,
            y: self.to.center_y,
        })
    }

    pub fn evaluate(&self, time_tick: TimeTick) -> CameraState {
        if time_tick <= self.start_tick {
            return self.from;
        }
        if time_tick >= self.end_tick {
            return self.to;
        }

        let elapsed = (time_tick.0 - self.start_tick.0) as f64;
        let duration = (self.end_tick.0 - self.start_tick.0) as f64;
        self.from
            .interpolate(self.to, self.easing.apply(elapsed / duration))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratorMetadata {
    pub name: String,
    pub version: String,
    pub parameter_set: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRevision {
    pub click_track_hash: String,
    pub capture_geometry_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraTrack {
    pub schema_version: u32,
    pub generator: GeneratorMetadata,
    pub source_revision: SourceRevision,
    pub base_state: CameraState,
    pub segments: Vec<CameraSegment>,
}

impl CameraTrack {
    pub fn stabilize_auto_pans(&mut self) {
        let editable = |segment: &CameraSegment| {
            segment.origin == SegmentOrigin::Auto && !segment.locked && !segment.user_modified
        };
        let mut state = self.base_state;
        let mut kept = Vec::with_capacity(self.segments.len());
        for (index, segment) in self.segments.iter().enumerate() {
            // Keep the original entry state of every protected/manual successor.
            if segment.kind == CameraSegmentKind::Pan
                && editable(segment)
                && self.segments.get(index + 1).is_none_or(editable)
                && state.is_micro_pan_to(segment.to)
            {
                continue;
            }
            let mut segment = segment.clone();
            if !segment.from.approx_eq(state) {
                segment.from = state;
            }
            state = segment.to;
            kept.push(segment);
        }
        self.segments = kept;
    }

    pub fn validate(&self) -> Result<(), CameraValidationError> {
        if self.schema_version != CAMERA_SCHEMA_VERSION {
            return Err(CameraValidationError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        self.base_state.validate()?;

        let mut previous_end = TimeTick::ZERO;
        let mut expected_state = self.base_state;
        let mut ids = HashSet::with_capacity(self.segments.len());

        for (index, segment) in self.segments.iter().enumerate() {
            if !ids.insert(segment.id.as_str()) {
                return Err(CameraValidationError::DuplicateSegmentId(
                    segment.id.clone(),
                ));
            }
            if segment.start_tick >= segment.end_tick {
                return Err(CameraValidationError::InvalidSegmentRange {
                    index,
                    start_tick: segment.start_tick,
                    end_tick: segment.end_tick,
                });
            }
            if index > 0 && segment.start_tick < previous_end {
                return Err(CameraValidationError::OverlappingSegments { index });
            }
            segment.from.validate()?;
            segment.to.validate()?;
            if let Some(focus) = segment.focus {
                let point = NormalizedPoint::new(focus.x, focus.y)
                    .map_err(|_| CameraValidationError::InvalidFocus { index })?;
                let target = CameraState::focused(point, segment.to.scale)
                    .map_err(|_| CameraValidationError::InvalidFocus { index })?;
                if !target.approx_eq(segment.to) {
                    return Err(CameraValidationError::InvalidFocus { index });
                }
            }
            if !segment.from.approx_eq(expected_state) {
                return Err(CameraValidationError::DiscontinuousSegment { index });
            }

            previous_end = segment.end_tick;
            expected_state = segment.to;
        }

        Ok(())
    }

    pub fn evaluate(&self, time_tick: TimeTick) -> CameraState {
        let mut state = self.base_state;
        for segment in &self.segments {
            if time_tick < segment.start_tick {
                return state;
            }
            if time_tick <= segment.end_tick {
                return segment.evaluate(time_tick);
            }
            state = segment.to;
        }
        state
    }

    pub fn canonical_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn canonical_hash(&self) -> Result<String, serde_json::Error> {
        let bytes = self.canonical_json()?;
        let digest = Sha256::digest(bytes);
        Ok(format!("sha256:{digest:x}"))
    }
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum CameraValidationError {
    #[error("unsupported camera schema version {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("camera state contains a non-finite value")]
    NonFiniteState,
    #[error("camera scale must be at least 1.0, got {0}")]
    InvalidScale(f64),
    #[error("camera center is outside normalized coordinates")]
    CenterOutOfRange,
    #[error("camera center is not clamped for its scale")]
    CenterNotClamped,
    #[error("camera segment {index} has invalid range {start_tick:?}..{end_tick:?}")]
    InvalidSegmentRange {
        index: usize,
        start_tick: TimeTick,
        end_tick: TimeTick,
    },
    #[error("camera segment {index} overlaps its predecessor")]
    OverlappingSegments { index: usize },
    #[error("camera segment {index} is discontinuous")]
    DiscontinuousSegment { index: usize },
    #[error("duplicate camera segment id {0}")]
    DuplicateSegmentId(String),
    #[error("camera segment {index} has an invalid focus point")]
    InvalidFocus { index: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_track() -> CameraTrack {
        CameraTrack {
            schema_version: CAMERA_SCHEMA_VERSION,
            generator: GeneratorMetadata {
                name: "test".into(),
                version: "0.1.0".into(),
                parameter_set: "test".into(),
            },
            source_revision: SourceRevision {
                click_track_hash: "sha256:clicks".into(),
                capture_geometry_hash: "sha256:geometry".into(),
            },
            base_state: CameraState::BASE,
            segments: vec![CameraSegment {
                id: "segment-1".into(),
                kind: CameraSegmentKind::ZoomIn,
                start_tick: TimeTick(10),
                end_tick: TimeTick(20),
                from: CameraState::BASE,
                to: CameraState::focused(NormalizedPoint::new(0.5, 0.5).unwrap(), 1.6).unwrap(),
                easing: Easing::Smootherstep,
                origin: SegmentOrigin::Auto,
                source_click_ids: vec!["click-1".into()],
                locked: false,
                user_modified: false,
                focus: None,
            }],
        }
    }

    #[test]
    fn evaluates_before_during_and_after_segment() {
        let track = test_track();
        assert_eq!(track.evaluate(TimeTick(0)), CameraState::BASE);
        assert!(track.evaluate(TimeTick(15)).scale > 1.0);
        assert!((track.evaluate(TimeTick(20)).scale - 1.6).abs() < APPROX_EPSILON);
        assert!((track.evaluate(TimeTick(30)).scale - 1.6).abs() < APPROX_EPSILON);
    }

    #[test]
    fn edge_zoom_keeps_targets_inside_and_moves_them_monotonically() {
        use crate::CameraTransform;
        for (x, y) in [
            (0.0, 0.0),
            (0.01, 0.98),
            (1.0, 1.0),
            (0.99, 0.02),
            (0.4, 0.6),
        ] {
            for scale in [1.6, 2.0, 4.0] {
                let focus =
                    CameraState::focused(NormalizedPoint::new(x, y).unwrap(), scale).unwrap();
                for (from, to) in [(CameraState::BASE, focus), (focus, CameraState::BASE)] {
                    let a = CameraTransform::from(from).source_to_output(x, y);
                    let b = CameraTransform::from(to).source_to_output(x, y);
                    let mut previous = [a.x, a.y];
                    for step in 0..=200 {
                        let state = from
                            .interpolate(to, Easing::Smootherstep.apply(f64::from(step) / 200.0));
                        state.validate().unwrap();
                        let point = CameraTransform::from(state).source_to_output(x, y);
                        for (axis, value) in [point.x, point.y].into_iter().enumerate() {
                            let end = [b.x, b.y][axis];
                            let start = [a.x, a.y][axis];
                            assert!(
                                value >= start.min(end) - 1e-12 && value <= start.max(end) + 1e-12,
                                "reverse drift at ({x}, {y}), scale {scale}, step {step}: {value}"
                            );
                            assert!((value - previous[axis]) * (end - start) >= -1e-12);
                            assert!((-1e-12..=1.0 + 1e-12).contains(&value));
                            previous[axis] = value;
                        }
                    }
                    assert_eq!(from.interpolate(to, 0.0), from);
                    assert_eq!(from.interpolate(to, 1.0), to);
                }
            }
        }
    }

    fn track_with_micro_pan() -> CameraTrack {
        let mut track = test_track();
        let focused = track.segments[0].to;
        let moved = CameraState {
            center_x: focused.center_x + 0.01,
            ..focused
        };
        let template = track.segments[0].clone();
        track.segments.push(CameraSegment {
            id: "small-pan".into(),
            kind: CameraSegmentKind::Pan,
            start_tick: TimeTick(30),
            end_tick: TimeTick(40),
            from: focused,
            to: moved,
            ..template.clone()
        });
        track.segments.push(CameraSegment {
            id: "out".into(),
            kind: CameraSegmentKind::ZoomOut,
            start_tick: TimeTick(50),
            end_tick: TimeTick(60),
            from: moved,
            to: CameraState::BASE,
            ..template
        });
        track
    }

    #[test]
    fn stabilizing_old_auto_pans_preserves_continuity_and_is_idempotent() {
        let mut track = track_with_micro_pan();
        track.validate().unwrap();
        let original = track.clone();
        track.stabilize_auto_pans();
        assert_eq!(track.segments.len(), 2);
        assert_eq!(track.evaluate(TimeTick(40)), original.segments[0].to);
        assert_eq!(
            track.segments[1].start_tick,
            original.segments[2].start_tick
        );
        assert_eq!(track.segments[1].from, track.segments[0].to);
        track.validate().unwrap();
        let once = track.clone();
        track.stabilize_auto_pans();
        assert_eq!(track, once);
    }

    #[test]
    fn stabilization_preserves_manual_locked_modified_segments_and_their_entry_states() {
        for index in [1, 2] {
            for protection in 0..3 {
                let mut track = track_with_micro_pan();
                match protection {
                    0 => track.segments[index].origin = SegmentOrigin::Manual,
                    1 => track.segments[index].locked = true,
                    _ => track.segments[index].user_modified = true,
                }
                let original = track.clone();
                track.stabilize_auto_pans();
                assert_eq!(track, original);
                track.validate().unwrap();
            }
        }
    }

    #[test]
    fn smootherstep_has_stable_endpoints() {
        assert_eq!(Easing::Smootherstep.apply(0.0), 0.0);
        assert_eq!(Easing::Smootherstep.apply(1.0), 1.0);
    }

    #[test]
    fn validates_and_hashes_deterministically() {
        let track = test_track();
        track.validate().unwrap();
        assert_eq!(
            track.canonical_hash().unwrap(),
            track.canonical_hash().unwrap()
        );
    }

    #[test]
    fn rejects_unclamped_edge_focus() {
        let mut track = test_track();
        track.segments[0].to.center_x = 1.0;
        assert!(matches!(
            track.validate(),
            Err(CameraValidationError::CenterNotClamped)
        ));
    }
}
