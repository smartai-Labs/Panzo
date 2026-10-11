use crate::camera::{CameraState, CameraTrack, CameraValidationError};
use crate::events::{CursorEvent, EVENT_SCHEMA_VERSION};
use crate::time::{TimeError, TimeMapping, TimeTick};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const CURSOR_SIZE_PX: u32 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputPixelFormat {
    Bgra8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputDescriptor {
    pub width: u32,
    pub height: u32,
    pub pixel_format: OutputPixelFormat,
}

impl OutputDescriptor {
    pub fn bgra8(width: u32, height: u32) -> Result<Self, RenderEvaluationError> {
        if width == 0 || height == 0 {
            return Err(RenderEvaluationError::EmptyOutput);
        }
        Ok(Self {
            width,
            height,
            pixel_format: OutputPixelFormat::Bgra8,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraTransform {
    pub center_x: f64,
    pub center_y: f64,
    pub scale: f64,
    /// Scale of the fitted video frame, independent of camera zoom.
    #[serde(default = "unit_content_scale")]
    pub content_scale: f64,
    #[serde(default, skip_serializing_if = "crate::VideoCrop::is_full")]
    pub crop: crate::VideoCrop,
}

const fn unit_content_scale() -> f64 { 1.0 }

impl From<CameraState> for CameraTransform {
    fn from(state: CameraState) -> Self {
        Self {
            center_x: state.center_x,
            center_y: state.center_y,
            scale: state.scale,
            content_scale: 1.0,
            crop: crate::VideoCrop::default(),
        }
    }
}

impl CameraTransform {
    pub fn with_crop(mut self, crop: crate::VideoCrop) -> Self {
        self.crop = crop;
        let half_x = 0.5 * crop.width() / self.scale;
        let half_y = 0.5 * crop.height() / self.scale;
        let left = f64::from(crop.left) / 1000.0 + half_x;
        let top = f64::from(crop.top) / 1000.0 + half_y;
        self.center_x = self.center_x.clamp(
            left,
            (1.0 - f64::from(crop.right) / 1000.0 - half_x).max(left),
        );
        self.center_y = self.center_y.clamp(
            top,
            (1.0 - f64::from(crop.bottom) / 1000.0 - half_y).max(top),
        );
        self
    }
    pub fn source_to_output(self, source_x: f64, source_y: f64) -> RenderPoint {
        RenderPoint {
            x: (source_x - self.center_x).mul_add(self.scale / self.crop.width(), 0.5),
            y: (source_y - self.center_y).mul_add(self.scale / self.crop.height(), 0.5),
        }
    }

    pub fn output_to_source(self, output_x: f64, output_y: f64) -> RenderPoint {
        RenderPoint {
            x: (output_x - 0.5) * self.crop.width() / self.scale + self.center_x,
            y: (output_y - 0.5) * self.crop.height() / self.scale + self.center_y,
        }
    }

    pub fn source_rect(self) -> SourceRect {
        let half_extent = 0.5 / self.scale;
        SourceRect {
            left: self.center_x - half_extent * self.crop.width(),
            top: self.center_y - half_extent * self.crop.height(),
            right: self.center_x + half_extent * self.crop.width(),
            bottom: self.center_y + half_extent * self.crop.height(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderPoint {
    pub x: f64,
    pub y: f64,
}

impl RenderPoint {
    pub fn is_inside_output(self) -> bool {
        // A crop pinned to an edge may round to -1e-16 after the reciprocal zoom.
        // Keep the edge cursor visible without admitting a real off-canvas position.
        const EDGE_EPSILON: f64 = 1.0e-12;
        (-EDGE_EPSILON..=1.0 + EDGE_EPSILON).contains(&self.x)
            && (-EDGE_EPSILON..=1.0 + EDGE_EPSILON).contains(&self.y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl SourceRect {
    pub fn is_inside_source(self) -> bool {
        self.left >= 0.0 && self.top >= 0.0 && self.right <= 1.0 && self.bottom <= 1.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFrameSelection {
    pub frame_index: usize,
    pub presentation_tick: TimeTick,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFrameTimeline {
    presentation_ticks: Vec<TimeTick>,
}

impl SourceFrameTimeline {
    pub fn new(presentation_ticks: Vec<TimeTick>) -> Result<Self, RenderEvaluationError> {
        if presentation_ticks.is_empty() {
            return Err(RenderEvaluationError::EmptySourceTimeline);
        }
        let mut previous = None;
        for tick in &presentation_ticks {
            if *tick < TimeTick::ZERO {
                return Err(RenderEvaluationError::NegativeSourceTimestamp(*tick));
            }
            if let Some(previous) = previous
                && *tick <= previous
            {
                return Err(RenderEvaluationError::NonMonotonicSourceTimeline {
                    previous,
                    current: *tick,
                });
            }
            previous = Some(*tick);
        }
        Ok(Self { presentation_ticks })
    }

    pub fn presentation_ticks(&self) -> &[TimeTick] {
        &self.presentation_ticks
    }

    pub fn select(&self, source_tick: TimeTick) -> SourceFrameSelection {
        let upper = self
            .presentation_ticks
            .partition_point(|candidate| *candidate <= source_tick);
        let frame_index = upper.saturating_sub(1);
        SourceFrameSelection {
            frame_index,
            presentation_tick: self.presentation_ticks[frame_index],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorRenderState {
    pub source_x: f64,
    pub source_y: f64,
    pub output_x: f64,
    pub output_y: f64,
    pub visible: bool,
    pub size_px: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CursorTimeline {
    events: Vec<CursorEvent>,
    follow: std::cell::RefCell<crate::camera_policy::FollowCache>,
}

impl CursorTimeline {
    pub fn new(events: Vec<CursorEvent>) -> Result<Self, RenderEvaluationError> {
        let mut previous = None;
        for event in &events {
            if event.schema_version != EVENT_SCHEMA_VERSION {
                return Err(RenderEvaluationError::UnsupportedCursorSchema(
                    event.schema_version,
                ));
            }
            if event.time_tick < TimeTick::ZERO {
                return Err(RenderEvaluationError::NegativeCursorTimestamp(
                    event.time_tick,
                ));
            }
            if let Some(previous) = previous
                && event.time_tick < previous
            {
                return Err(RenderEvaluationError::NonMonotonicCursorTimeline {
                    previous,
                    current: event.time_tick,
                });
            }
            if !event.normalized_x.is_finite() || !event.normalized_y.is_finite() {
                return Err(RenderEvaluationError::NonFiniteCursorPosition);
            }
            previous = Some(event.time_tick);
        }
        Ok(Self {
            events,
            follow: std::cell::RefCell::default(),
        })
    }

    pub fn events(&self) -> &[CursorEvent] {
        &self.events
    }

    pub fn evaluate(
        &self,
        source_tick: TimeTick,
        camera: CameraTransform,
    ) -> Option<CursorRenderState> {
        let upper = self
            .events
            .partition_point(|event| event.time_tick <= source_tick);
        let previous = self.events.get(upper.checked_sub(1)?)?;
        let (source_x, source_y) = self
            .events
            .get(upper)
            .filter(|next| next.time_tick > previous.time_tick)
            .map_or((previous.normalized_x, previous.normalized_y), |next| {
                let elapsed = (source_tick.0 - previous.time_tick.0) as f64;
                let duration = (next.time_tick.0 - previous.time_tick.0) as f64;
                let progress = (elapsed / duration).clamp(0.0, 1.0);
                (
                    previous.normalized_x + (next.normalized_x - previous.normalized_x) * progress,
                    previous.normalized_y + (next.normalized_y - previous.normalized_y) * progress,
                )
            });
        let output = camera.source_to_output(source_x, source_y);
        Some(CursorRenderState {
            source_x,
            source_y,
            output_x: output.x,
            output_y: output.y,
            visible: previous.visible && output.is_inside_output(),
            size_px: CURSOR_SIZE_PX,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameEvaluation {
    pub project_tick: TimeTick,
    pub source_tick: TimeTick,
    pub source_frame: SourceFrameSelection,
    pub camera_state: CameraState,
    pub camera_transform: CameraTransform,
    pub cursor: Option<CursorRenderState>,
    pub output: OutputDescriptor,
}

pub struct FrameEvaluator<'a> {
    source_frames: &'a SourceFrameTimeline,
    camera: &'a CameraTrack,
    cursor: &'a CursorTimeline,
}

impl FrameEvaluation {
    /// Normalized canvas rectangle shared by GPU rendering and pointer mapping.
    pub fn content_rect(&self, source: (u32, u32), inset: f64) -> SourceRect {
        let w = f64::from(source.0) * self.camera_transform.crop.width();
        let h = f64::from(source.1) * self.camera_transform.crop.height();
        let ow = f64::from(self.output.width);
        let oh = f64::from(self.output.height);
        // The UI stores pixels normalized by the shorter output dimension.
        let margin = inset * ow.min(oh);
        let fit = ((ow - 2.0 * margin) / w)
            .min((oh - 2.0 * margin) / h)
            .min(1.0) * self.camera_transform.content_scale;
        let width = w * fit / ow;
        let height = h * fit / oh;
        SourceRect {
            left: (1.0 - width) / 2.0,
            top: (1.0 - height) / 2.0,
            right: f64::midpoint(1.0, width),
            bottom: f64::midpoint(1.0, height),
        }
    }
}

impl<'a> FrameEvaluator<'a> {
    pub fn new(
        source_frames: &'a SourceFrameTimeline,
        camera: &'a CameraTrack,
        cursor: &'a CursorTimeline,
    ) -> Result<Self, RenderEvaluationError> {
        camera.validate()?;
        Ok(Self {
            source_frames,
            camera,
            cursor,
        })
    }

    pub fn evaluate(
        &self,
        project_tick: TimeTick,
        output: OutputDescriptor,
        time_mapping: &impl TimeMapping,
    ) -> Result<FrameEvaluation, RenderEvaluationError> {
        if project_tick < TimeTick::ZERO {
            return Err(RenderEvaluationError::NegativeProjectTimestamp(
                project_tick,
            ));
        }
        let source_tick = time_mapping.source_time(project_tick)?;
        if source_tick < TimeTick::ZERO {
            return Err(RenderEvaluationError::NegativeSourceTimestamp(source_tick));
        }
        let source_frame = self.source_frames.select(source_tick);
        let camera_state = self.camera.evaluate(source_tick);
        let camera_transform = CameraTransform::from(camera_state);
        let cursor = self.cursor.evaluate(source_tick, camera_transform);
        Ok(FrameEvaluation {
            project_tick,
            source_tick,
            source_frame,
            camera_state,
            camera_transform,
            cursor,
            output,
        })
    }

    pub fn evaluate_edit(
        &self,
        project_tick: TimeTick,
        output: OutputDescriptor,
        settings: &crate::WorkbenchSettings,
    ) -> Result<FrameEvaluation, RenderEvaluationError> {
        let filtered;
        let camera=if settings.follow_positions.is_empty()&&!settings.tracks.iter().any(|t|t.kind==crate::timeline::TrackKind::Follow&&t.muted){self.camera}else{filtered={let mut c=self.camera.clone();c.segments.retain(|s|!settings.follow_positions.contains_key(&s.id)&&!settings.track_muted(settings.clip_tracks.get(&s.id).map_or("follow",String::as_str)));c};&filtered};
        let base=FrameEvaluator{source_frames:self.source_frames,camera,cursor:self.cursor};
        let placed_source=settings.visual_clips.iter().rev().find(|c|c.asset_id=="__source__"&&c.active(project_tick));
        let mut frame = if let Some(c)=placed_source{base.evaluate(c.source_tick(project_tick),output,&crate::IdentityTimeMapping)?}else if let Some(video) = &settings.video {
            base.evaluate(project_tick.min(TimeTick(video.duration().0 - 1)), output, video)?
        } else {
            base.evaluate(project_tick, output, &crate::IdentityTimeMapping)?
        };
        frame.project_tick = project_tick;
        if let Some(automatic) = &settings.automatic_camera {
            if !(settings.manual_camera_enabled
                && crate::camera_policy::manual_active(camera, frame.source_tick))
            {
                frame.camera_state = automatic.evaluate(
                    frame.source_tick,
                    camera,
                    self.cursor.events(),
                    &mut self.cursor.follow.borrow_mut(),
                );
            }
            frame.camera_transform = frame.camera_state.into();
            frame.cursor = self
                .cursor
                .evaluate(frame.source_tick, frame.camera_transform);
        }
        if let Some((segment,(a,b)))=self.camera.segments.iter().filter(|s|!settings.track_muted(settings.clip_tracks.get(&s.id).map_or("follow",String::as_str))).filter_map(|s|settings.follow_positions.get(&s.id).filter(|(a,b)|project_tick>=*a&&project_tick<*b).map(|r|(s,*r))).max_by_key(|(s,_)|settings.tracks.iter().position(|t|Some(&t.id)==settings.clip_tracks.get(&s.id)).unwrap_or(0)){
            let mut s=segment.clone();s.start_tick=a;s.end_tick=b;frame.camera_state=s.evaluate(project_tick);frame.camera_transform=frame.camera_state.into();frame.cursor=self.cursor.evaluate(frame.source_tick,frame.camera_transform);
        }
        if !settings.camera_enabled {
            frame.camera_state = CameraState::BASE;
            frame.camera_transform = frame.camera_state.into();
            frame.cursor = self
                .cursor
                .evaluate(frame.source_tick, frame.camera_transform);
        }
        let crop = placed_source.map(|c|c.crop).unwrap_or_else(||settings
            .video
            .as_ref()
            .and_then(|video| video.clip_at(project_tick.min(TimeTick(video.duration().0 - 1))))
            .map_or_else(crate::VideoCrop::default, |span| span.clip.crop));
        if !crop.is_full() {
            frame.camera_transform = frame.camera_transform.with_crop(crop);
            frame.cursor = self
                .cursor
                .evaluate(frame.source_tick, frame.camera_transform);
        }
        // Placed visual clips apply their own transform in the compositor.
        if placed_source.is_none() {
            frame.camera_transform.content_scale = settings.video.as_ref()
                .and_then(|v| v.clip_at(project_tick.min(TimeTick(v.duration().0 - 1))))
                .map_or(1.0, |s| f64::from(s.clip.scale_per_mille) / 1000.0);
        }
        if placed_source.is_none() && (!settings.source_video_enabled || !settings.tracks.iter().any(|t|t.id=="video") || settings.video.as_ref().is_some_and(|v|project_tick>v.duration() || (project_tick==v.duration() && settings.project_duration(v.source_duration_tick)>v.duration()))) {
            frame.source_tick=TimeTick(-1);frame.cursor=None;
        }
        Ok(frame)
    }
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum RenderEvaluationError {
    #[error("output dimensions must be positive")]
    EmptyOutput,
    #[error("V0.1 output must be 16:9, got aspect ratio {0}")]
    UnsupportedOutputAspectRatio(f64),
    #[error("source frame timeline must contain at least one frame")]
    EmptySourceTimeline,
    #[error("source frame timestamp must be non-negative, got {0:?}")]
    NegativeSourceTimestamp(TimeTick),
    #[error("source frame timeline moved backwards or duplicated from {previous:?} to {current:?}")]
    NonMonotonicSourceTimeline {
        previous: TimeTick,
        current: TimeTick,
    },
    #[error("unsupported cursor schema version {0}")]
    UnsupportedCursorSchema(u32),
    #[error("cursor timestamp must be non-negative, got {0:?}")]
    NegativeCursorTimestamp(TimeTick),
    #[error("cursor timeline moved backwards from {previous:?} to {current:?}")]
    NonMonotonicCursorTimeline {
        previous: TimeTick,
        current: TimeTick,
    },
    #[error("cursor position must be finite")]
    NonFiniteCursorPosition,
    #[error("project timestamp must be non-negative, got {0:?}")]
    NegativeProjectTimestamp(TimeTick),
    #[error(transparent)]
    Time(#[from] TimeError),
    #[error(transparent)]
    Camera(#[from] CameraValidationError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{
        CAMERA_SCHEMA_VERSION, CameraSegment, CameraSegmentKind, Easing, GeneratorMetadata,
        SegmentOrigin, SourceRevision,
    };
    use crate::events::CursorEventKind;
    use crate::time::IdentityTimeMapping;

    fn cursor(id: u64, millis: i64, x: f64, y: f64, visible: bool) -> CursorEvent {
        CursorEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            id,
            time_tick: TimeTick::from_millis(millis),
            kind: CursorEventKind::Move,
            desktop_x: 0,
            desktop_y: 0,
            content_x: 0,
            content_y: 0,
            normalized_x: x,
            normalized_y: y,
            visible,
            geometry_revision: 0,
        }
    }

    fn camera_track() -> CameraTrack {
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
                id: "zoom".into(),
                kind: CameraSegmentKind::ZoomIn,
                start_tick: TimeTick::from_millis(1_000),
                end_tick: TimeTick::from_millis(2_000),
                from: CameraState::BASE,
                to: CameraState {
                    center_x: 0.5,
                    center_y: 0.5,
                    scale: 2.0,
                },
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
    fn crop_fits_inside_equal_pixel_margins_then_applies_clip_scale() {
        let frames=SourceFrameTimeline::new(vec![TimeTick::ZERO]).unwrap();
        let camera=camera_track();let cursor=CursorTimeline::new(vec![]).unwrap();
        let evaluator=FrameEvaluator::new(&frames,&camera,&cursor).unwrap();
        for (ow,oh) in [(1920,1080),(1080,1920),(1080,1080)] {
            let output=OutputDescriptor::bgra8(ow,oh).unwrap();
            let mut settings=crate::WorkbenchSettings::default();
            settings.video=Some(crate::VideoEdit::full(TimeTick(1000)).unwrap());
            settings.canvas.inset=100.0/f64::from(ow.min(oh));
            let crop=crate::VideoCrop{left:100,top:50,right:200,bottom:150};
            settings.video.as_mut().unwrap().set_crop("source-0",crop).unwrap();
            let frame=evaluator.evaluate_edit(TimeTick::ZERO,output,&settings).unwrap();
            let rect=frame.content_rect((2560,1440),settings.canvas.inset);
            let horizontal=rect.left*f64::from(ow);let vertical=rect.top*f64::from(oh);
            assert!(horizontal>=100.0-1e-9 && vertical>=100.0-1e-9);
            assert!((horizontal.min(vertical)-100.0).abs()<1e-9);
            assert!(((rect.right-rect.left)*f64::from(ow)/((rect.bottom-rect.top)*f64::from(oh)) - 2560.0*0.7/(1440.0*0.8)).abs()<1e-9);
            settings.video.as_mut().unwrap().set_scale("source-0",1500).unwrap();
            let enlarged=evaluator.evaluate_edit(TimeTick::ZERO,output,&settings).unwrap();
            let enlarged=enlarged.content_rect((2560,1440),settings.canvas.inset);
            assert!(((enlarged.right-enlarged.left)/(rect.right-rect.left)-1.5).abs()<1e-9);
            assert_eq!(frame.camera_transform.crop,crop);
            // Camera zoom changes source sampling without changing canvas margins.
            let mut zoomed=frame;zoomed.camera_transform.scale=2.0;
            assert_eq!(zoomed.content_rect((2560,1440),settings.canvas.inset),rect);
        }
    }

    #[test]
    fn moved_follow_evaluates_at_project_position_and_track_mute_is_local(){
        let frames=SourceFrameTimeline::new(vec![TimeTick::ZERO,TimeTick::from_millis(5000)]).unwrap();let camera=camera_track();let cursor=CursorTimeline::new(vec![]).unwrap();let eval=FrameEvaluator::new(&frames,&camera,&cursor).unwrap();let mut settings=crate::WorkbenchSettings::default();settings.video=Some(crate::VideoEdit::full(TimeTick::from_millis(6000)).unwrap());settings.follow_positions.insert("zoom".into(),(TimeTick::from_millis(3000),TimeTick::from_millis(4000)));let output=OutputDescriptor::bgra8(640,360).unwrap();
        assert_eq!(eval.evaluate_edit(TimeTick::from_millis(1500),output,&settings).unwrap().camera_state,CameraState::BASE);
        assert!(eval.evaluate_edit(TimeTick::from_millis(3500),output,&settings).unwrap().camera_state.scale>1.1);
        settings.tracks.iter_mut().find(|t|t.id=="follow").unwrap().muted=true;
        assert_eq!(eval.evaluate_edit(TimeTick::from_millis(3500),output,&settings).unwrap().camera_state,CameraState::BASE);
    }

    #[test]
    fn automatic_ranges_survive_trim_speed_and_manual_priority() {
        use crate::camera_policy::{AutoCameraConfig, AutoCameraSettings, FollowMode};
        let frames =
            SourceFrameTimeline::new(vec![TimeTick::ZERO, TimeTick::from_millis(3000)]).unwrap();
        let mut camera = camera_track();
        camera.segments[0].origin = SegmentOrigin::Manual;
        let cursor = CursorTimeline::new(vec![
            cursor(1, 0, 0.8, 0.6, true),
            cursor(2, 3000, 0.8, 0.6, true),
        ])
        .unwrap();
        let mut settings = crate::WorkbenchSettings {
            automatic_camera: Some(AutoCameraSettings::default()),
            ..crate::WorkbenchSettings::default()
        };
        let descriptor = OutputDescriptor::bgra8(320, 180).unwrap();
        let eval = FrameEvaluator::new(&frames, &camera, &cursor).unwrap();
        let at = TimeTick::from_millis(2200);
        assert_eq!(
            eval.evaluate_edit(at, descriptor, &settings)
                .unwrap()
                .camera_state
                .scale,
            2.0
        );
        settings.manual_camera_enabled = false;
        settings.automatic_camera.as_mut().unwrap().default = AutoCameraConfig {
            enabled: true,
            mode: FollowMode::Click,
            ..Default::default()
        };
        assert_eq!(
            eval.evaluate_edit(at, descriptor, &settings)
                .unwrap()
                .camera_state,
            CameraState::BASE
        );
        let policy = settings.automatic_camera.as_mut().unwrap();
        policy.default = AutoCameraConfig {
            enabled: true,
            ..Default::default()
        };
        policy.set_range(
            TimeTick::from_millis(2000),
            TimeTick::from_millis(2500),
            AutoCameraConfig::default(),
        );
        let mut video = crate::VideoEdit::full(TimeTick::from_millis(3000)).unwrap();
        let id = video.clips[0].id.clone();
        video
            .trim(
                &id,
                TimeTick::from_millis(1000),
                TimeTick::from_millis(3000),
            )
            .unwrap();
        video.set_speed_percent(&id, 200).unwrap();
        settings.video = Some(video);
        let frame = eval
            .evaluate_edit(TimeTick::from_millis(650), descriptor, &settings)
            .unwrap();
        assert_eq!(frame.source_tick, TimeTick::from_millis(2300));
        assert_eq!(frame.camera_state, CameraState::BASE);
        let frame = eval
            .evaluate_edit(TimeTick::from_millis(900), descriptor, &settings)
            .unwrap();
        assert!(frame.camera_state.scale > 1.4);
        settings.camera_enabled = false;
        assert_eq!(
            eval.evaluate_edit(TimeTick::from_millis(900), descriptor, &settings)
                .unwrap()
                .camera_state,
            CameraState::BASE
        );
    }

    #[test]
    fn rapid_camera_switches_are_continuous_and_do_not_change_prior_frames() {
        use crate::camera_policy::{AutoCameraConfig, AutoCameraSettings, FollowCache};
        let camera = camera_track();
        let events = vec![
            cursor(1, 0, 0.8, 0.6, true),
            cursor(2, 3000, 0.8, 0.6, true),
        ];
        let mut cache = FollowCache::default();
        let mut policy = AutoCameraSettings {
            default: AutoCameraConfig {
                enabled: true,
                ..Default::default()
            },
            ranges: Vec::new(),
        };
        let before = policy.evaluate(TimeTick::from_millis(900), &camera, &events, &mut cache);
        policy.set_range(
            TimeTick::from_millis(1000),
            TimeTick::from_millis(1100),
            AutoCameraConfig::default(),
        );
        assert_eq!(
            before,
            policy.evaluate(TimeTick::from_millis(900), &camera, &events, &mut cache)
        );
        for boundary in [10_000_000, 11_000_000] {
            let a = policy.evaluate(TimeTick(boundary - 1), &camera, &events, &mut cache);
            let b = policy.evaluate(TimeTick(boundary), &camera, &events, &mut cache);
            assert!((a.scale - b.scale).abs() < 0.00001);
        }
    }
    #[test]
    fn crop_preserves_aspect_and_maps_cursor_and_camera_inside_remaining_image() {
        let crop = crate::VideoCrop {
            left: 250,
            right: 0,
            top: 100,
            bottom: 200,
        };
        for scale in [1.0, 1.6, 4.0] {
            let transform = CameraTransform::from(CameraState {
                center_x: 0.9,
                center_y: 0.1,
                scale,
            })
            .with_crop(crop);
            let rect = transform.source_rect();
            assert!(
                rect.left >= 0.25 - 1e-12
                    && rect.top >= 0.1 - 1e-12
                    && rect.right <= 1.0 + 1e-12
                    && rect.bottom <= 0.8 + 1e-12
            );
            let source = transform.output_to_source(0.3, 0.7);
            let output = transform.source_to_output(source.x, source.y);
            assert!((output.x - 0.3).abs() < 1e-12 && (output.y - 0.7).abs() < 1e-12);
        }
        let frames = SourceFrameTimeline::new(vec![TimeTick::ZERO, TimeTick(100)]).unwrap();
        let camera = camera_track();
        let cursor = CursorTimeline::new(vec![cursor(1, 0, 0.1, 0.5, true)]).unwrap();
        let mut settings = crate::WorkbenchSettings::default();
        let mut video = crate::VideoEdit::full(TimeTick(200)).unwrap();
        video.split(TimeTick(100), "right".into()).unwrap();
        video.set_crop("right", crop).unwrap();
        settings.video = Some(video);
        let eval = FrameEvaluator::new(&frames, &camera, &cursor).unwrap();
        let descriptor = OutputDescriptor::bgra8(1920, 1080).unwrap();
        assert!(
            eval.evaluate_edit(TimeTick(99), descriptor, &settings)
                .unwrap()
                .cursor
                .unwrap()
                .visible
        );
        for tick in [100, 199, 200] {
            let frame = eval
                .evaluate_edit(TimeTick(tick), descriptor, &settings)
                .unwrap();
            assert!(!frame.cursor.unwrap().visible);
            let rect = frame.content_rect((1920, 1080), 0.09);
            let aspect = (rect.right - rect.left) * 1920.0 / ((rect.bottom - rect.top) * 1080.0);
            assert!((aspect - 1920.0 * 0.75 / (1080.0 * 0.7)).abs() < 1e-12);
        }
        for margin in 0..=950 {
            CameraTransform::from(CameraState::BASE).with_crop(crate::VideoCrop {
                left: margin,
                right: 950 - margin,
                top: margin,
                bottom: 950 - margin,
            });
        }
    }

    #[test]
    fn selects_nearest_vfr_frame_not_later_than_source_time() {
        let timeline = SourceFrameTimeline::new(vec![
            TimeTick::ZERO,
            TimeTick::from_millis(17),
            TimeTick::from_millis(41),
        ])
        .unwrap();
        assert_eq!(timeline.select(TimeTick::ZERO).frame_index, 0);
        assert_eq!(timeline.select(TimeTick::from_millis(40)).frame_index, 1);
        assert_eq!(timeline.select(TimeTick::from_millis(41)).frame_index, 2);
        assert_eq!(timeline.select(TimeTick::from_millis(5_000)).frame_index, 2);
    }

    #[test]
    fn cursor_interpolates_position_without_leaking_future_visibility() {
        let timeline = CursorTimeline::new(vec![
            cursor(1, 0, 0.2, 0.4, true),
            cursor(2, 100, 0.8, 0.6, false),
        ])
        .unwrap();
        let middle = timeline
            .evaluate(TimeTick::from_millis(50), CameraState::BASE.into())
            .unwrap();
        assert!((middle.source_x - 0.5).abs() < 1.0e-9);
        assert!((middle.source_y - 0.5).abs() < 1.0e-9);
        assert!(middle.visible);
        assert!(
            !timeline
                .evaluate(TimeTick::from_millis(100), CameraState::BASE.into())
                .unwrap()
                .visible
        );
    }

    #[test]
    fn corner_cursor_stays_visible_through_zoom_without_roundoff_flicker() {
        for (x, y) in [(0.0, 0.0), (1.0, 1.0), (0.0, 1.0), (1.0, 0.0)] {
            let events = CursorTimeline::new(vec![cursor(1, 0, x, y, true)]).unwrap();
            let focus =
                CameraState::focused(crate::NormalizedPoint::new(x, y).unwrap(), 1.6).unwrap();
            for step in 0..=350 {
                let camera = CameraState::BASE
                    .interpolate(focus, Easing::Smootherstep.apply(step as f64 / 350.0));
                let visible = events
                    .evaluate(TimeTick::from_millis(step), camera.into())
                    .unwrap();
                assert!(
                    visible.visible,
                    "corner ({x},{y}) hidden at {step}: {visible:?}"
                );
            }
        }
        assert!(!RenderPoint { x: -0.001, y: 0.5 }.is_inside_output());
        assert!(
            !RenderPoint {
                x: f64::NAN,
                y: 0.5
            }
            .is_inside_output()
        );
    }

    #[test]
    fn camera_transform_round_trips_and_never_exposes_outside_source() {
        let state = CameraState {
            center_x: 0.25,
            center_y: 0.25,
            scale: 2.0,
        };
        state.validate().unwrap();
        let transform = CameraTransform::from(state);
        let output = transform.source_to_output(0.3, 0.4);
        let source = transform.output_to_source(output.x, output.y);
        assert!((source.x - 0.3).abs() < 1.0e-9);
        assert!((source.y - 0.4).abs() < 1.0e-9);
        assert!(transform.source_rect().is_inside_source());
    }

    #[test]
    fn shared_frame_evaluator_combines_time_camera_cursor_and_output() {
        let frames = SourceFrameTimeline::new(vec![
            TimeTick::ZERO,
            TimeTick::from_millis(1_000),
            TimeTick::from_millis(2_000),
        ])
        .unwrap();
        let cursors = CursorTimeline::new(vec![cursor(1, 0, 0.5, 0.5, true)]).unwrap();
        let camera = camera_track();
        let evaluator = FrameEvaluator::new(&frames, &camera, &cursors).unwrap();
        let evaluation = evaluator
            .evaluate(
                TimeTick::from_millis(1_500),
                OutputDescriptor::bgra8(1_920, 1_080).unwrap(),
                &IdentityTimeMapping,
            )
            .unwrap();
        assert_eq!(evaluation.source_frame.frame_index, 1);
        assert!(evaluation.camera_state.scale > 1.0);
        assert_eq!(evaluation.cursor.unwrap().size_px, CURSOR_SIZE_PX);
    }

    #[test]
    fn speed_changes_preserve_source_frame_camera_and_cursor_alignment() {
        let frames =
            SourceFrameTimeline::new((0..=30).map(|i| TimeTick::from_millis(i * 100)).collect())
                .unwrap();
        let cursors = CursorTimeline::new(vec![
            cursor(1, 0, 0.1, 0.2, true),
            cursor(2, 2000, 0.8, 0.7, true),
        ])
        .unwrap();
        let camera = camera_track();
        let evaluator = FrameEvaluator::new(&frames, &camera, &cursors).unwrap();
        let output = OutputDescriptor::bgra8(1200, 800).unwrap();
        let baseline = evaluator
            .evaluate_edit(
                TimeTick::from_millis(1500),
                output,
                &crate::WorkbenchSettings::default(),
            )
            .unwrap();
        for (speed, millis) in [(25, 6000), (50, 3000), (125, 1200), (200, 750), (400, 375)] {
            let mut video = crate::VideoEdit::full(TimeTick::from_millis(3000)).unwrap();
            video.set_speed_percent("source-0", speed).unwrap();
            let settings = crate::WorkbenchSettings {
                video: Some(video),
                ..Default::default()
            };
            let actual = evaluator
                .evaluate_edit(TimeTick::from_millis(millis), output, &settings)
                .unwrap();
            assert_eq!(actual.source_tick, baseline.source_tick);
            assert_eq!(actual.source_frame, baseline.source_frame);
            assert_eq!(actual.camera_state, baseline.camera_state);
            assert_eq!(actual.cursor, baseline.cursor);
        }
    }

    #[test]
    fn deleted_source_interval_moves_camera_and_cursor_together() {
        let frames =
            SourceFrameTimeline::new((0..=30).map(|i| TimeTick::from_millis(i * 100)).collect())
                .unwrap();
        let cursors = CursorTimeline::new(vec![
            cursor(1, 0, 0.1, 0.2, true),
            cursor(2, 2000, 0.8, 0.7, true),
        ])
        .unwrap();
        let camera = camera_track();
        let evaluator = FrameEvaluator::new(&frames, &camera, &cursors).unwrap();
        let mut video = crate::VideoEdit::full(TimeTick::from_millis(3000)).unwrap();
        video
            .split(TimeTick::from_millis(500), "middle".into())
            .unwrap();
        video
            .split(TimeTick::from_millis(1500), "last".into())
            .unwrap();
        video.delete("middle").unwrap();
        let mut settings = crate::WorkbenchSettings {
            video: Some(video),
            ..crate::WorkbenchSettings::default()
        };
        let output = OutputDescriptor::bgra8(1200, 800).unwrap();
        let original = evaluator
            .evaluate(TimeTick::from_millis(1750), output, &IdentityTimeMapping)
            .unwrap();
        let edited = evaluator
            .evaluate_edit(TimeTick::from_millis(750), output, &settings)
            .unwrap();
        assert_eq!(edited.source_tick, original.source_tick);
        assert_eq!(edited.source_frame, original.source_frame);
        assert_eq!(edited.camera_state, original.camera_state);
        assert_eq!(edited.cursor, original.cursor);
        settings.camera_enabled = false;
        assert_eq!(
            evaluator
                .evaluate_edit(TimeTick::from_millis(750), output, &settings)
                .unwrap()
                .camera_state,
            CameraState::BASE
        );
    }
}
