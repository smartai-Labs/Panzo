use crate::{
    CameraSegment, CameraSegmentKind, CameraState, CameraTrack, Easing, NormalizedPoint,
    SegmentOrigin, TimeTick,
};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};
use thiserror::Error;

pub const WORKBENCH_SCHEMA_VERSION: u32 = 5;
const MAX_HISTORY: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackgroundKind {
    Solid,
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundSettings {
    pub kind: BackgroundKind,
    pub color: String,
    pub image: Option<String>,
}

impl Default for BackgroundSettings {
    fn default() -> Self {
        Self {
            kind: BackgroundKind::Solid,
            color: "#111827".into(),
            image: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<crate::CanvasSize>,
    pub inset: f64,
    pub corner_radius: f64,
    pub shadow: f64,
}

impl Default for CanvasSettings {
    fn default() -> Self {
        Self {
            size: None,
            inset: 0.0,
            corner_radius: 0.0,
            shadow: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorStyle {
    pub visible: bool,
    pub scale: f64,
}

impl Default for CursorStyle {
    fn default() -> Self {
        Self {
            visible: true,
            scale: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchSettings {
    pub schema_version: u32,
    pub revision: u64,
    pub background: BackgroundSettings,
    pub canvas: CanvasSettings,
    pub cursor: CursorStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<crate::VideoEdit>,
    #[serde(default = "camera_enabled_default")]
    pub camera_enabled: bool,
}

fn camera_enabled_default() -> bool {
    true
}

impl Default for WorkbenchSettings {
    fn default() -> Self {
        Self {
            schema_version: WORKBENCH_SCHEMA_VERSION,
            revision: 0,
            background: BackgroundSettings::default(),
            canvas: CanvasSettings::default(),
            cursor: CursorStyle::default(),
            video: None,
            camera_enabled: true,
        }
    }
}

impl WorkbenchSettings {
    pub fn video_edit(
        &self,
        source_duration: TimeTick,
    ) -> Result<crate::VideoEdit, crate::VideoEditError> {
        match &self.video {
            Some(video) if video.source_duration_tick == source_duration => {
                video.validate()?;
                Ok(video.clone())
            }
            Some(_) => Err(crate::VideoEditError::InvalidRange),
            None => crate::VideoEdit::full(source_duration),
        }
    }

    pub fn validate(&self) -> Result<(), WorkbenchValidationError> {
        if !matches!(
            self.schema_version,
            1 | 2 | 3 | 4 | WORKBENCH_SCHEMA_VERSION
        ) || (self.schema_version < 5 && self.canvas.size.is_some())
            || (self.schema_version == 1 && (self.video.is_some() || !self.camera_enabled))
            || (self.schema_version < 3
                && self
                    .video
                    .as_ref()
                    .is_some_and(|video| video.clips.iter().any(|clip| clip.speed_percent != 100)))
            || (self.schema_version < 4
                && self
                    .video
                    .as_ref()
                    .is_some_and(|video| video.clips.iter().any(|clip| !clip.crop.is_full())))
        {
            return Err(WorkbenchValidationError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        parse_hex_color(&self.background.color)?;
        if self.canvas.size.is_some_and(|size| !size.is_valid()) {
            return Err(WorkbenchValidationError::InvalidCanvasSize);
        }
        if let Some(video) = &self.video {
            video.validate()?;
        }
        match self.background.kind {
            BackgroundKind::Solid if self.background.image.is_some() => {
                return Err(WorkbenchValidationError::UnexpectedBackgroundImage);
            }
            BackgroundKind::Image => {
                let image = self
                    .background
                    .image
                    .as_deref()
                    .ok_or(WorkbenchValidationError::MissingBackgroundImage)?;
                validate_project_relative_path(image)?;
            }
            BackgroundKind::Solid => {}
        }
        validate_range("canvas.inset", self.canvas.inset, 0.0, 0.25)?;
        validate_range("canvas.cornerRadius", self.canvas.corner_radius, 0.0, 0.10)?;
        validate_range("canvas.shadow", self.canvas.shadow, 0.0, 1.0)?;
        validate_range("cursor.scale", self.cursor.scale, 0.25, 4.0)?;
        Ok(())
    }

    pub fn background_rgba(&self) -> Result<[f32; 4], WorkbenchValidationError> {
        let [red, green, blue] = parse_hex_color(&self.background.color)?;
        Ok([
            f32::from(red) / 255.0,
            f32::from(green) / 255.0,
            f32::from(blue) / 255.0,
            1.0,
        ])
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraEditKind {
    Add,
    Update,
    Delete,
    Undo,
    Redo,
    Style,
    ResetToAuto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraEditRecord {
    pub revision: u64,
    pub kind: CameraEditKind,
    pub segment_id: Option<String>,
    pub before_hash: String,
    pub after_hash: String,
}

#[derive(Debug, Clone)]
pub struct CameraTrackEditor {
    track: CameraTrack,
    duration: TimeTick,
    undo: Vec<CameraTrack>,
    redo: Vec<CameraTrack>,
    history_enabled: bool,
}

impl CameraTrackEditor {
    pub fn new(track: CameraTrack, duration: TimeTick) -> Result<Self, CameraEditError> {
        if duration <= TimeTick::ZERO {
            return Err(CameraEditError::InvalidProjectDuration(duration));
        }
        track.validate()?;
        validate_track_duration(&track, duration)?;
        Ok(Self {
            track,
            duration,
            undo: Vec::new(),
            redo: Vec::new(),
            history_enabled: true,
        })
    }

    /// Use a single outer history when camera edits are part of a full project command.
    pub fn without_history(mut self) -> Self {
        self.history_enabled = false;
        self.undo.clear();
        self.redo.clear();
        self
    }

    pub const fn track(&self) -> &CameraTrack {
        &self.track
    }

    pub fn into_track(self) -> CameraTrack {
        self.track
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn update_segment(
        &mut self,
        id: &str,
        start_tick: TimeTick,
        end_tick: TimeTick,
        target: CameraState,
    ) -> Result<(), CameraEditError> {
        let index = segment_index(&self.track, id)?;
        let focus = self.track.segments[index].focus.filter(|point| {
            CameraState::focused(*point, target.scale)
                .is_ok_and(|focused| focused.approx_eq(target))
        });
        self.update_segment_with_focus(id, start_tick, end_tick, target, focus)
    }

    pub fn update_segment_focus(
        &mut self,
        id: &str,
        start_tick: TimeTick,
        end_tick: TimeTick,
        focus: NormalizedPoint,
        scale: f64,
    ) -> Result<(), CameraEditError> {
        let focus = NormalizedPoint::new(focus.x, focus.y)?;
        let target = CameraState::focused(focus, scale)?;
        self.update_segment_with_focus(id, start_tick, end_tick, target, Some(focus))
    }

    fn update_segment_with_focus(
        &mut self,
        id: &str,
        start_tick: TimeTick,
        end_tick: TimeTick,
        target: CameraState,
        focus: Option<NormalizedPoint>,
    ) -> Result<(), CameraEditError> {
        let mut candidate = self.track.clone();
        let index = segment_index(&candidate, id)?;
        validate_segment_range(&candidate, index, start_tick, end_tick, self.duration)?;
        target.validate()?;
        let segment = &mut candidate.segments[index];
        segment.start_tick = start_tick;
        segment.end_tick = end_tick;
        segment.to = target;
        segment.focus = focus;
        segment.locked = true;
        segment.user_modified = true;
        repair_continuity(&mut candidate);
        self.commit(candidate)
    }

    pub fn move_segment(&mut self, id: &str, delta: TimeTick) -> Result<(), CameraEditError> {
        let index = segment_index(&self.track, id)?;
        let segment = &self.track.segments[index];
        let start_tick = segment
            .start_tick
            .checked_add(delta)
            .ok_or(CameraEditError::TimeOverflow)?;
        let end_tick = segment
            .end_tick
            .checked_add(delta)
            .ok_or(CameraEditError::TimeOverflow)?;
        self.update_segment(id, start_tick, end_tick, segment.to)
    }

    pub fn set_segment_target(
        &mut self,
        id: &str,
        target: CameraState,
    ) -> Result<(), CameraEditError> {
        let index = segment_index(&self.track, id)?;
        let segment = &self.track.segments[index];
        self.update_segment(id, segment.start_tick, segment.end_tick, target)
    }

    pub fn add_segment(
        &mut self,
        id: impl Into<String>,
        kind: CameraSegmentKind,
        start_tick: TimeTick,
        end_tick: TimeTick,
        target: CameraState,
    ) -> Result<String, CameraEditError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(CameraEditError::EmptySegmentId);
        }
        if self.track.segments.iter().any(|segment| segment.id == id) {
            return Err(CameraEditError::DuplicateSegmentId(id));
        }
        target.validate()?;
        if start_tick < TimeTick::ZERO || start_tick >= end_tick || end_tick > self.duration {
            return Err(CameraEditError::InvalidSegmentRange {
                start_tick,
                end_tick,
            });
        }
        let mut candidate = self.track.clone();
        let index = candidate
            .segments
            .partition_point(|segment| segment.start_tick < start_tick);
        validate_insert_range(&candidate, index, start_tick, end_tick)?;
        let from = candidate.evaluate(start_tick);
        candidate.segments.insert(
            index,
            CameraSegment {
                id: id.clone(),
                kind,
                start_tick,
                end_tick,
                from,
                to: target,
                easing: Easing::Smootherstep,
                origin: SegmentOrigin::Manual,
                source_click_ids: Vec::new(),
                locked: true,
                user_modified: true,
                focus: None,
            },
        );
        repair_continuity(&mut candidate);
        self.commit(candidate)?;
        Ok(id)
    }

    pub fn delete_segment(&mut self, id: &str) -> Result<(), CameraEditError> {
        let mut candidate = self.track.clone();
        let index = segment_index(&candidate, id)?;
        candidate.segments.remove(index);
        repair_continuity(&mut candidate);
        self.commit(candidate)
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.track, previous));
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.track, next));
        true
    }

    fn commit(&mut self, candidate: CameraTrack) -> Result<(), CameraEditError> {
        candidate.validate()?;
        validate_track_duration(&candidate, self.duration)?;
        if candidate == self.track {
            return Ok(());
        }
        if !self.history_enabled {
            self.track = candidate;
            return Ok(());
        }
        self.undo
            .push(std::mem::replace(&mut self.track, candidate));
        if self.undo.len() > MAX_HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        Ok(())
    }
}

fn segment_index(track: &CameraTrack, id: &str) -> Result<usize, CameraEditError> {
    track
        .segments
        .iter()
        .position(|segment| segment.id == id)
        .ok_or_else(|| CameraEditError::UnknownSegment(id.into()))
}

fn validate_track_duration(track: &CameraTrack, duration: TimeTick) -> Result<(), CameraEditError> {
    if let Some(segment) = track
        .segments
        .iter()
        .find(|segment| segment.end_tick > duration)
    {
        return Err(CameraEditError::SegmentAfterProjectEnd {
            id: segment.id.clone(),
            end_tick: segment.end_tick,
            duration,
        });
    }
    Ok(())
}

fn validate_segment_range(
    track: &CameraTrack,
    index: usize,
    start_tick: TimeTick,
    end_tick: TimeTick,
    duration: TimeTick,
) -> Result<(), CameraEditError> {
    if start_tick < TimeTick::ZERO || start_tick >= end_tick || end_tick > duration {
        return Err(CameraEditError::InvalidSegmentRange {
            start_tick,
            end_tick,
        });
    }
    if index > 0 && start_tick < track.segments[index - 1].end_tick {
        return Err(CameraEditError::OverlappingPrevious);
    }
    if index + 1 < track.segments.len() && end_tick > track.segments[index + 1].start_tick {
        return Err(CameraEditError::OverlappingNext);
    }
    Ok(())
}

fn validate_insert_range(
    track: &CameraTrack,
    index: usize,
    start_tick: TimeTick,
    end_tick: TimeTick,
) -> Result<(), CameraEditError> {
    if index > 0 && start_tick < track.segments[index - 1].end_tick {
        return Err(CameraEditError::OverlappingPrevious);
    }
    if index < track.segments.len() && end_tick > track.segments[index].start_tick {
        return Err(CameraEditError::OverlappingNext);
    }
    Ok(())
}

fn repair_continuity(track: &mut CameraTrack) {
    let mut expected = track.base_state;
    for segment in &mut track.segments {
        if !segment.from.approx_eq(expected) {
            segment.from = expected;
            segment.locked = true;
            segment.user_modified = true;
        }
        expected = segment.to;
    }
}

fn parse_hex_color(value: &str) -> Result<[u8; 3], WorkbenchValidationError> {
    let bytes = value.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(u8::is_ascii_hexdigit) {
        return Err(WorkbenchValidationError::InvalidColor(value.into()));
    }
    let red = parse_hex_pair(&value[1..3])?;
    let green = parse_hex_pair(&value[3..5])?;
    let blue = parse_hex_pair(&value[5..7])?;
    Ok([red, green, blue])
}

fn parse_hex_pair(value: &str) -> Result<u8, WorkbenchValidationError> {
    u8::from_str_radix(value, 16)
        .map_err(|_| WorkbenchValidationError::InvalidColor(format!("#{value}")))
}

fn validate_range(
    name: &'static str,
    value: f64,
    minimum: f64,
    maximum: f64,
) -> Result<(), WorkbenchValidationError> {
    if !value.is_finite() || !(minimum..=maximum).contains(&value) {
        return Err(WorkbenchValidationError::InvalidRange {
            name,
            value,
            minimum,
            maximum,
        });
    }
    Ok(())
}

fn validate_project_relative_path(value: &str) -> Result<(), WorkbenchValidationError> {
    let path = Path::new(value);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(WorkbenchValidationError::InvalidProjectPath(value.into()));
    }
    Ok(())
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum WorkbenchValidationError {
    #[error("画布宽高须为 64–8192 的偶数，且总像素不超过 33554432")]
    InvalidCanvasSize,
    #[error(transparent)]
    Video(#[from] crate::VideoEditError),
    #[error("unsupported workbench schema version {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("invalid #RRGGBB color: {0}")]
    InvalidColor(String),
    #[error("solid background must not specify an image")]
    UnexpectedBackgroundImage,
    #[error("image background requires a project-relative image path")]
    MissingBackgroundImage,
    #[error("invalid project-relative asset path: {0}")]
    InvalidProjectPath(String),
    #[error("{name} must be finite and within {minimum}..={maximum}, got {value}")]
    InvalidRange {
        name: &'static str,
        value: f64,
        minimum: f64,
        maximum: f64,
    },
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum CameraEditError {
    #[error(transparent)]
    Geometry(#[from] crate::geometry::GeometryError),
    #[error("project duration must be positive, got {0:?}")]
    InvalidProjectDuration(TimeTick),
    #[error("camera segment does not exist: {0}")]
    UnknownSegment(String),
    #[error("camera segment id must not be empty")]
    EmptySegmentId,
    #[error("camera segment id already exists: {0}")]
    DuplicateSegmentId(String),
    #[error(
        "camera segment range must be within the project and have positive duration: {start_tick:?}..{end_tick:?}"
    )]
    InvalidSegmentRange {
        start_tick: TimeTick,
        end_tick: TimeTick,
    },
    #[error("camera segment overlaps its predecessor")]
    OverlappingPrevious,
    #[error("camera segment overlaps its successor")]
    OverlappingNext,
    #[error("camera segment {id} ends at {end_tick:?}, after project end {duration:?}")]
    SegmentAfterProjectEnd {
        id: String,
        end_tick: TimeTick,
        duration: TimeTick,
    },
    #[error("camera edit timestamp overflowed")]
    TimeOverflow,
    #[error(transparent)]
    InvalidTrack(#[from] crate::camera::CameraValidationError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_unicode_color_is_rejected_without_slicing_panic() {
        for value in ["#a中ab", "#中abc", "#💙ab", "#12345g", "112233"] {
            assert!(parse_hex_color(value).is_err(), "{value}");
        }
        assert_eq!(parse_hex_color("#A1b2C3").unwrap(), [161, 178, 195]);
    }
    use crate::camera::CAMERA_SCHEMA_VERSION;
    use crate::{GeneratorMetadata, NormalizedPoint, SourceRevision, TICKS_PER_SECOND};

    fn state(x: f64, y: f64, scale: f64) -> CameraState {
        CameraState::focused(NormalizedPoint::new(x, y).unwrap(), scale).unwrap()
    }

    fn track() -> CameraTrack {
        let focused = state(0.5, 0.5, 1.6);
        CameraTrack {
            schema_version: CAMERA_SCHEMA_VERSION,
            generator: GeneratorMetadata {
                name: "test".into(),
                version: "1".into(),
                parameter_set: "default".into(),
            },
            source_revision: SourceRevision {
                click_track_hash: "clicks".into(),
                capture_geometry_hash: "geometry".into(),
            },
            base_state: CameraState::BASE,
            segments: vec![
                CameraSegment {
                    id: "in".into(),
                    kind: CameraSegmentKind::ZoomIn,
                    start_tick: TimeTick::from_millis(1_000),
                    end_tick: TimeTick::from_millis(1_500),
                    from: CameraState::BASE,
                    to: focused,
                    easing: Easing::Smootherstep,
                    origin: SegmentOrigin::Auto,
                    source_click_ids: vec!["click-1".into()],
                    locked: false,
                    user_modified: false,
                    focus: None,
                },
                CameraSegment {
                    id: "out".into(),
                    kind: CameraSegmentKind::ZoomOut,
                    start_tick: TimeTick::from_millis(3_000),
                    end_tick: TimeTick::from_millis(3_500),
                    from: focused,
                    to: CameraState::BASE,
                    easing: Easing::Smootherstep,
                    origin: SegmentOrigin::Auto,
                    source_click_ids: vec!["click-1".into()],
                    locked: false,
                    user_modified: false,
                    focus: None,
                },
            ],
        }
    }

    #[test]
    fn default_workbench_is_valid_and_identity_layout() {
        let settings = WorkbenchSettings::default();
        settings.validate().unwrap();
        assert_eq!(settings.canvas.inset, 0.0);
        assert_eq!(
            settings.background_rgba().unwrap(),
            [17.0 / 255.0, 24.0 / 255.0, 39.0 / 255.0, 1.0]
        );
    }

    #[test]
    fn rejects_external_background_asset() {
        let mut settings = WorkbenchSettings::default();
        settings.background.kind = BackgroundKind::Image;
        settings.background.image = Some("../outside.png".into());
        assert!(matches!(
            settings.validate(),
            Err(WorkbenchValidationError::InvalidProjectPath(_))
        ));
    }

    #[test]
    fn updates_target_and_repairs_successor_continuity() {
        let mut editor = CameraTrackEditor::new(track(), TimeTick(5 * TICKS_PER_SECOND)).unwrap();
        let target = state(0.65, 0.5, 2.0);
        editor.set_segment_target("in", target).unwrap();
        let edited = editor.track();
        assert_eq!(edited.segments[0].to, target);
        assert_eq!(edited.segments[1].from, target);
        assert!(edited.segments[0].user_modified);
        assert!(edited.segments[1].user_modified);
        edited.validate().unwrap();
    }

    #[test]
    fn rejects_overlap_without_mutating_track() {
        let original = track();
        let mut editor =
            CameraTrackEditor::new(original.clone(), TimeTick(5 * TICKS_PER_SECOND)).unwrap();
        let result = editor.update_segment(
            "in",
            TimeTick::from_millis(1_000),
            TimeTick::from_millis(3_100),
            editor.track().segments[0].to,
        );
        assert_eq!(result.unwrap_err(), CameraEditError::OverlappingNext);
        assert_eq!(editor.track(), &original);
    }

    #[test]
    fn full_frame_focus_survives_zoom_timing_history_and_serialization() {
        for (x, y) in [
            (0.0, 0.0),
            (1.0, 0.0),
            (0.0, 1.0),
            (1.0, 1.0),
            (0.0, 0.5),
            (1.0, 0.5),
            (0.5, 0.0),
            (0.5, 1.0),
        ] {
            let original = track();
            let mut editor =
                CameraTrackEditor::new(original.clone(), TimeTick::from_millis(5000)).unwrap();
            let focus = NormalizedPoint::new(x, y).unwrap();
            editor
                .update_segment_focus(
                    "in",
                    TimeTick::from_millis(1000),
                    TimeTick::from_millis(1500),
                    focus,
                    1.6,
                )
                .unwrap();
            assert_eq!(editor.track().segments[0].focus_point(), focus);
            assert_eq!(editor.track().segments[0].to, state(x, y, 1.6));
            assert!(editor.undo());
            assert_eq!(editor.track(), &original);
            assert!(editor.redo());
            editor
                .move_segment("in", TimeTick::from_millis(100))
                .unwrap();
            for scale in [1.0, 4.0, 1.6] {
                editor.set_segment_target("in", state(x, y, scale)).unwrap();
                assert_eq!(editor.track().segments[0].focus_point(), focus);
                editor.track().validate().unwrap();
            }
            let encoded = editor.track().canonical_json().unwrap();
            let decoded: CameraTrack = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(decoded, *editor.track());
            assert_eq!(decoded.segments[1].from, decoded.segments[0].to);
        }
    }

    #[test]
    fn invalid_focus_is_rejected_without_mutation_and_old_tracks_keep_their_hash() {
        let original = track();
        let encoded = original.canonical_json().unwrap();
        assert!(
            !String::from_utf8(encoded.clone())
                .unwrap()
                .contains("\"focus\"")
        );
        assert_eq!(
            serde_json::from_slice::<CameraTrack>(&encoded).unwrap(),
            original
        );
        let mut editor =
            CameraTrackEditor::new(original.clone(), TimeTick::from_millis(5000)).unwrap();
        for focus in [
            NormalizedPoint { x: -0.1, y: 0.5 },
            NormalizedPoint {
                x: f64::NAN,
                y: 0.5,
            },
        ] {
            assert!(
                editor
                    .update_segment_focus(
                        "in",
                        TimeTick::from_millis(1000),
                        TimeTick::from_millis(1500),
                        focus,
                        1.6
                    )
                    .is_err()
            );
            assert_eq!(*editor.track(), original);
        }
        let mut malformed = original.clone();
        malformed.segments[0].focus = Some(NormalizedPoint { x: 0.0, y: 1.0 });
        assert!(malformed.validate().is_err());
    }

    #[test]
    fn add_delete_undo_and_redo_preserve_a_valid_track() {
        let mut editor = CameraTrackEditor::new(track(), TimeTick(8 * TICKS_PER_SECOND)).unwrap();
        let added = editor
            .add_segment(
                "manual-pan",
                CameraSegmentKind::Pan,
                TimeTick::from_millis(5_000),
                TimeTick::from_millis(5_500),
                state(0.6, 0.5, 1.6),
            )
            .unwrap();
        assert_eq!(added, "manual-pan");
        assert_eq!(editor.track().segments[2].origin, SegmentOrigin::Manual);
        editor.delete_segment("manual-pan").unwrap();
        assert_eq!(editor.track().segments.len(), 2);
        assert!(editor.undo());
        assert_eq!(editor.track().segments.len(), 3);
        assert!(editor.redo());
        assert_eq!(editor.track().segments.len(), 2);
        editor.track().validate().unwrap();
    }
}
