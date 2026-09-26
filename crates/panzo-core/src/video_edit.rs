//! Non-destructive, single-source editing. All intervals are half-open source ticks.
use crate::time::{TimeError, TimeMapping, TimeTick};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoClip {
    pub id: String,
    pub source_in_tick: TimeTick,
    pub source_out_tick: TimeTick,
    #[serde(default = "normal_speed")]
    pub speed_percent: u16,
    #[serde(default)]
    pub crop: VideoCrop,
}

/// Margins removed from the source image, in thousandths. At least 5% remains.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoCrop {
    pub left: u16,
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
}

impl VideoCrop {
    pub fn validate(self) -> Result<(), VideoEditError> {
        if u32::from(self.left) + u32::from(self.right) > 950
            || u32::from(self.top) + u32::from(self.bottom) > 950
        {
            return Err(VideoEditError::InvalidCrop);
        }
        Ok(())
    }

    pub fn is_full(&self) -> bool {
        *self == Self::default()
    }
    pub fn width(self) -> f64 {
        1.0 - (f64::from(self.left) + f64::from(self.right)) / 1000.0
    }
    pub fn height(self) -> f64 {
        1.0 - (f64::from(self.top) + f64::from(self.bottom)) / 1000.0
    }

    #[allow(clippy::cast_sign_loss)] // Rounded margins are clamped to 0..475.
    pub fn centered_aspect(width: u32, height: u32, ratio: f64) -> Self {
        let aspect = f64::from(width) / f64::from(height.max(1));
        let mut crop = Self::default();
        if aspect > ratio {
            let margin = ((1.0 - ratio / aspect) * 500.0).round().clamp(0.0, 475.0) as u16;
            crop.left = margin;
            crop.right = margin;
        } else {
            let margin = ((1.0 - aspect / ratio) * 500.0).round().clamp(0.0, 475.0) as u16;
            crop.top = margin;
            crop.bottom = margin;
        }
        crop
    }
}

const fn normal_speed() -> u16 {
    100
}

impl VideoClip {
    pub fn duration(&self) -> TimeTick {
        self.project_offset(TimeTick(
            self.source_out_tick.0.saturating_sub(self.source_in_tick.0),
        ))
    }

    /// Project duration needed to reach a source boundary. Round forward, never into the prior frame.
    pub fn project_offset(&self, source_offset: TimeTick) -> TimeTick {
        let numerator = i128::from(source_offset.0) * 100;
        let speed = i128::from(self.speed_percent.max(1));
        TimeTick(i64::try_from((numerator + speed - 1) / speed).unwrap_or(i64::MAX))
    }

    /// Signed project delta to source delta, also used by trim gestures.
    pub fn source_delta(&self, project_delta: TimeTick) -> TimeTick {
        let ticks = i128::from(project_delta.0) * i128::from(self.speed_percent) / 100;
        TimeTick(ticks.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoEdit {
    pub source_duration_tick: TimeTick,
    pub clips: Vec<VideoClip>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipSpan<'a> {
    pub clip: &'a VideoClip,
    pub project_in: TimeTick,
    pub project_out: TimeTick,
}

impl VideoEdit {
    pub fn full(duration: TimeTick) -> Result<Self, VideoEditError> {
        let edit = Self {
            source_duration_tick: duration,
            clips: vec![VideoClip {
                id: "source-0".into(),
                source_in_tick: TimeTick::ZERO,
                source_out_tick: duration,
                speed_percent: normal_speed(),
                crop: VideoCrop::default(),
            }],
        };
        edit.validate()?;
        Ok(edit)
    }

    pub fn validate(&self) -> Result<(), VideoEditError> {
        if self.source_duration_tick <= TimeTick::ZERO || self.clips.is_empty() {
            return Err(VideoEditError::Empty);
        }
        let mut previous = TimeTick::ZERO;
        let mut ids = HashSet::new();
        let mut duration = 0_i128;
        for clip in &self.clips {
            if clip.id.trim().is_empty() || !ids.insert(&clip.id) {
                return Err(VideoEditError::InvalidId);
            }
            if clip.source_in_tick < previous
                || clip.source_in_tick >= clip.source_out_tick
                || clip.source_out_tick > self.source_duration_tick
            {
                return Err(VideoEditError::InvalidRange);
            }
            if !(25..=400).contains(&clip.speed_percent) {
                return Err(VideoEditError::InvalidSpeed);
            }
            clip.crop.validate()?;
            let speed = i128::from(clip.speed_percent);
            duration += (i128::from(clip.source_out_tick.0 - clip.source_in_tick.0) * 100 + speed
                - 1)
                / speed;
            if duration > i128::from(i64::MAX) {
                return Err(VideoEditError::DurationOverflow);
            }
            previous = clip.source_out_tick;
        }
        Ok(())
    }

    pub fn duration(&self) -> TimeTick {
        TimeTick(
            self.clips
                .iter()
                .fold(0_i64, |total, clip| total.saturating_add(clip.duration().0)),
        )
    }

    pub fn spans(&self) -> impl Iterator<Item = ClipSpan<'_>> {
        self.clips.iter().scan(TimeTick::ZERO, |offset, clip| {
            let project_in = *offset;
            offset.0 += clip.duration().0;
            Some(ClipSpan {
                clip,
                project_in,
                project_out: *offset,
            })
        })
    }

    pub fn clip_at(&self, project_tick: TimeTick) -> Option<ClipSpan<'_>> {
        self.spans()
            .find(|span| project_tick >= span.project_in && project_tick < span.project_out)
    }

    /// Splitting preserves the left ID; the caller supplies a new stable right ID.
    pub fn split(
        &mut self,
        project_tick: TimeTick,
        right_id: String,
    ) -> Result<(), VideoEditError> {
        self.validate()?;
        let span = self
            .clip_at(project_tick)
            .ok_or(VideoEditError::InvalidRange)?;
        if project_tick == span.project_in {
            return Err(VideoEditError::InvalidRange);
        }
        let source_tick = self.source_time(project_tick)?;
        let mut candidate = self.clone();
        let index = candidate
            .clips
            .iter()
            .position(|clip| clip.id == span.clip.id)
            .ok_or(VideoEditError::MissingClip)?;
        let right = VideoClip {
            id: right_id,
            source_in_tick: source_tick,
            source_out_tick: candidate.clips[index].source_out_tick,
            speed_percent: candidate.clips[index].speed_percent,
            crop: candidate.clips[index].crop,
        };
        candidate.clips[index].source_out_tick = source_tick;
        candidate.clips.insert(index + 1, right);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> Result<(), VideoEditError> {
        let index = self
            .clips
            .iter()
            .position(|clip| clip.id == id)
            .ok_or(VideoEditError::MissingClip)?;
        if self.clips.len() == 1 {
            return Err(VideoEditError::Empty);
        }
        self.clips.remove(index);
        Ok(())
    }

    pub fn set_crop(&mut self, id: &str, crop: VideoCrop) -> Result<(), VideoEditError> {
        crop.validate()?;
        self.clips
            .iter_mut()
            .find(|clip| clip.id == id)
            .ok_or(VideoEditError::MissingClip)?
            .crop = crop;
        Ok(())
    }

    pub fn set_speed_percent(
        &mut self,
        id: &str,
        speed_percent: u16,
    ) -> Result<(), VideoEditError> {
        let mut candidate = self.clone();
        let clip = candidate
            .clips
            .iter_mut()
            .find(|clip| clip.id == id)
            .ok_or(VideoEditError::MissingClip)?;
        clip.speed_percent = speed_percent;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    pub fn trim(
        &mut self,
        id: &str,
        source_in: TimeTick,
        source_out: TimeTick,
    ) -> Result<(), VideoEditError> {
        let mut candidate = self.clone();
        let clip = candidate
            .clips
            .iter_mut()
            .find(|clip| clip.id == id)
            .ok_or(VideoEditError::MissingClip)?;
        clip.source_in_tick = source_in;
        clip.source_out_tick = source_out;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    /// Intersections keep effects source-anchored, with a hard cut at removed content.
    pub fn project_ranges(
        &self,
        source_in: TimeTick,
        source_out: TimeTick,
    ) -> Vec<(String, TimeTick, TimeTick)> {
        self.spans()
            .filter_map(|span| {
                let start = source_in.max(span.clip.source_in_tick);
                let end = source_out.min(span.clip.source_out_tick);
                (start < end).then(|| {
                    (
                        span.clip.id.clone(),
                        TimeTick(
                            span.project_in.0
                                + span
                                    .clip
                                    .project_offset(TimeTick(start.0 - span.clip.source_in_tick.0))
                                    .0,
                        ),
                        TimeTick(
                            span.project_in.0
                                + span
                                    .clip
                                    .project_offset(TimeTick(end.0 - span.clip.source_in_tick.0))
                                    .0,
                        ),
                    )
                })
            })
            .collect()
    }
}

impl TimeMapping for VideoEdit {
    fn source_time(&self, project_time: TimeTick) -> Result<TimeTick, TimeError> {
        // The transport may sit at the end marker; display the last retained instant.
        let tick = if project_time == self.duration() {
            TimeTick(project_time.0 - 1)
        } else {
            project_time
        };
        self.clip_at(tick)
            .map(|span| {
                TimeTick(
                    span.clip.source_in_tick.0
                        + span
                            .clip
                            .source_delta(TimeTick(tick.0 - span.project_in.0))
                            .0
                            .min(span.clip.source_out_tick.0 - span.clip.source_in_tick.0 - 1),
                )
            })
            .ok_or(TimeError::UnmappedTime(project_time))
    }

    fn project_time(&self, source_time: TimeTick) -> Result<TimeTick, TimeError> {
        self.spans()
            .find(|span| {
                source_time >= span.clip.source_in_tick && source_time < span.clip.source_out_tick
            })
            .map(|span| {
                TimeTick(
                    (span.project_in.0
                        + span
                            .clip
                            .project_offset(TimeTick(source_time.0 - span.clip.source_in_tick.0))
                            .0)
                        .min(span.project_out.0 - 1),
                )
            })
            .ok_or(TimeError::UnmappedTime(source_time))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VideoEditError {
    #[error("裁剪后宽度和高度至少保留原画面的 5%")]
    InvalidCrop,
    #[error("必须保留至少一个非空视频片段")]
    Empty,
    #[error("视频片段 ID 为空或重复")]
    InvalidId,
    #[error("裁剪范围越界、重叠或时长为零")]
    InvalidRange,
    #[error("未找到所选视频片段")]
    MissingClip,
    #[error("速度范围为 0.25–4 倍")]
    InvalidSpeed,
    #[error("变速后的总时长超出支持范围")]
    DurationOverflow,
    #[error(transparent)]
    Time(#[from] TimeError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_is_per_clip_preserves_time_and_split_inherits_it() {
        let mut video = VideoEdit::full(TimeTick(1000)).unwrap();
        video.set_speed_percent("source-0", 200).unwrap();
        let cropped = VideoCrop {
            left: 100,
            top: 50,
            right: 250,
            bottom: 100,
        };
        video.set_crop("source-0", cropped).unwrap();
        video.split(TimeTick(200), "right".into()).unwrap();
        assert_eq!(video.duration(), TimeTick(500));
        assert_eq!(video.clips[1].crop, cropped);
        video.set_crop("right", VideoCrop::default()).unwrap();
        assert_eq!(video.clips[0].crop, cropped);
        let before = video.clone();
        for crop in [
            VideoCrop {
                left: 951,
                ..VideoCrop::default()
            },
            VideoCrop {
                top: u16::MAX,
                ..VideoCrop::default()
            },
            VideoCrop {
                left: 500,
                right: 500,
                ..VideoCrop::default()
            },
        ] {
            assert_eq!(
                video.set_crop("source-0", crop),
                Err(VideoEditError::InvalidCrop)
            );
            assert_eq!(video, before);
        }
        let legacy = r#"{"id":"old","sourceInTick":0,"sourceOutTick":100,"speedPercent":125}"#;
        assert!(
            serde_json::from_str::<VideoClip>(legacy)
                .unwrap()
                .crop
                .is_full()
        );
        assert_eq!(
            serde_json::from_str::<VideoEdit>(&serde_json::to_string(&video).unwrap()).unwrap(),
            video
        );
    }
    fn edit() -> VideoEdit {
        VideoEdit::full(TimeTick(100)).unwrap()
    }
    #[test]
    fn split_preserves_every_tick_and_stable_left_id() {
        let mut edit = edit();
        edit.split(TimeTick(40), "right".into()).unwrap();
        assert_eq!(edit.clips[0].id, "source-0");
        assert_eq!(edit.duration(), TimeTick(100));
        for tick in 0..100 {
            assert_eq!(edit.source_time(TimeTick(tick)).unwrap(), TimeTick(tick));
        }
    }
    #[test]
    fn deletion_ripples_and_deleted_time_has_no_inverse() {
        let mut edit = edit();
        edit.split(TimeTick(30), "middle".into()).unwrap();
        edit.split(TimeTick(70), "tail".into()).unwrap();
        edit.delete("middle").unwrap();
        assert_eq!(edit.duration(), TimeTick(60));
        assert_eq!(edit.source_time(TimeTick(30)).unwrap(), TimeTick(70));
        assert!(edit.project_time(TimeTick(30)).is_err());
        assert_eq!(edit.project_time(TimeTick(70)).unwrap(), TimeTick(30));
        assert_eq!(edit.source_time(TimeTick(60)).unwrap(), TimeTick(99));
        assert!(edit.source_time(TimeTick(61)).is_err());
        assert_eq!(
            edit.project_ranges(TimeTick(20), TimeTick(80)),
            vec![
                ("source-0".into(), TimeTick(20), TimeTick(30)),
                ("tail".into(), TimeTick(30), TimeTick(40))
            ]
        );
    }
    #[test]
    fn invalid_operations_are_atomic() {
        let mut edit = edit();
        let before = edit.clone();
        assert!(edit.delete("source-0").is_err());
        assert!(edit.split(TimeTick(0), "r".into()).is_err());
        assert!(edit.trim("source-0", TimeTick(-1), TimeTick(90)).is_err());
        assert_eq!(edit, before);
        edit.split(TimeTick(50), "r".into()).unwrap();
        let before = edit.clone();
        assert!(edit.trim("r", TimeTick(49), TimeTick(90)).is_err());
        assert!(edit.split(TimeTick(20), "r".into()).is_err());
        assert_eq!(edit, before);
    }
    #[test]
    fn trim_round_trip_and_large_integer_duration() {
        let mut edit = VideoEdit::full(TimeTick(i64::MAX)).unwrap();
        edit.trim("source-0", TimeTick(12), TimeTick(i64::MAX - 1))
            .unwrap();
        assert_eq!(edit.source_time(TimeTick(0)).unwrap(), TimeTick(12));
        let json = serde_json::to_string(&edit).unwrap();
        assert_eq!(serde_json::from_str::<VideoEdit>(&json).unwrap(), edit);
    }

    #[test]
    fn mixed_speeds_map_clips_effects_and_end_markers() {
        let mut video = VideoEdit::full(TimeTick(1200)).unwrap();
        video.split(TimeTick(400), "middle".into()).unwrap();
        video.split(TimeTick(800), "last".into()).unwrap();
        video.set_speed_percent("source-0", 200).unwrap();
        video.set_speed_percent("middle", 50).unwrap();
        assert_eq!(video.duration(), TimeTick(1400));
        for (project, source) in [
            (0, 0),
            (100, 200),
            (200, 400),
            (600, 600),
            (1000, 800),
            (1400, 1199),
        ] {
            assert_eq!(
                video.source_time(TimeTick(project)).unwrap(),
                TimeTick(source)
            );
        }
        assert_eq!(video.project_time(TimeTick(600)).unwrap(), TimeTick(600));
        assert_eq!(
            video.project_ranges(TimeTick(300), TimeTick(900)),
            vec![
                ("source-0".into(), TimeTick(150), TimeTick(200)),
                ("middle".into(), TimeTick(200), TimeTick(1000)),
                ("last".into(), TimeTick(1000), TimeTick(1100)),
            ]
        );
        video.split(TimeTick(600), "split-slow".into()).unwrap();
        assert_eq!(video.clips[2].speed_percent, 50);
        assert_eq!(video.clips[2].source_in_tick, TimeTick(600));
        assert_eq!(video.duration(), TimeTick(1400));
        video
            .trim("source-0", TimeTick(100), TimeTick(400))
            .unwrap();
        assert_eq!(video.clips[0].speed_percent, 200);
        assert_eq!(video.duration(), TimeTick(1350));
        video.delete("middle").unwrap();
        assert_eq!(video.source_time(TimeTick(150)).unwrap(), TimeTick(600));
    }

    #[test]
    fn speed_rounding_is_monotonic_and_never_crosses_a_source_boundary() {
        for speed in [25, 33, 50, 100, 125, 150, 199, 200, 400] {
            let mut video = VideoEdit::full(TimeTick(117)).unwrap();
            video.trim("source-0", TimeTick(7), TimeTick(108)).unwrap();
            video.set_speed_percent("source-0", speed).unwrap();
            let mut previous = TimeTick::ZERO;
            for tick in 0..=video.duration().0 {
                let source = video.source_time(TimeTick(tick)).unwrap();
                assert!((7..108).contains(&source.0));
                assert!(source >= previous);
                previous = source;
            }
            let ranges = video.project_ranges(TimeTick(7), TimeTick(108));
            assert_eq!(ranges[0].2, video.duration());
            for source in 7..108 {
                let project = video.project_time(TimeTick(source)).unwrap();
                assert!(project < video.duration());
                assert!((video.source_time(project).unwrap().0 - source).abs() <= 3);
            }
        }
    }

    #[test]
    fn legacy_clips_default_to_normal_speed_and_invalid_speed_is_atomic() {
        let legacy = r#"{"sourceDurationTick":100,"clips":[{"id":"clip","sourceInTick":0,"sourceOutTick":100}]}"#;
        let mut video: VideoEdit = serde_json::from_str(legacy).unwrap();
        video.validate().unwrap();
        assert_eq!(video.clips[0].speed_percent, 100);
        let original = video.clone();
        for speed in [0, 24, 401, u16::MAX] {
            assert_eq!(
                video.set_speed_percent("clip", speed),
                Err(VideoEditError::InvalidSpeed)
            );
            assert_eq!(video, original);
        }
        video.set_speed_percent("clip", 125).unwrap();
        assert_eq!(
            serde_json::from_str::<VideoEdit>(&serde_json::to_string(&video).unwrap()).unwrap(),
            video
        );
        let mut large = VideoEdit::full(TimeTick(i64::MAX)).unwrap();
        assert_eq!(
            large.set_speed_percent("source-0", 25),
            Err(VideoEditError::DurationOverflow)
        );
        assert_eq!(large.clips[0].speed_percent, 100);
    }
}
