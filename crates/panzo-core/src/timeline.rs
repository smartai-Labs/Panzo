//! Project assets and typed, stable timeline tracks. Imported assets have no timeline position.
use crate::media_edit::{AudioClip, AudioRole, MAX_MEDIA_TICK, TimeAnchor};
use crate::{TimeTick, WorkbenchSettings};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_TRACKS: usize = 32;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrackKind {
    Video,
    Follow,
    Narration,
    Subtitle,
    Music,
}
impl TrackKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Video => "视频",
            Self::Follow => "跟随",
            Self::Narration => "旁白",
            Self::Subtitle => "字幕",
            Self::Music => "音乐",
        }
    }
    pub fn audio(self) -> bool {
        matches!(self, Self::Narration | Self::Music)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    #[serde(default)]
    pub muted: bool,
    pub id: String,
    pub name: String,
    pub kind: TrackKind,
}
pub fn default_tracks() -> Vec<Track> {
    [
        ("video", TrackKind::Video),
        ("follow", TrackKind::Follow),
        ("narration", TrackKind::Narration),
        ("music", TrackKind::Music),
        ("subtitle", TrackKind::Subtitle),
    ]
    .into_iter()
    .map(|(id, kind)| Track {
        muted: false,
        id: id.into(),
        name: kind.label().into(),
        kind,
    })
    .collect()
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VisualKind {
    Video,
    Image,
    Gif,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualAsset {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: VisualKind,
    pub duration: TimeTick,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub audio_path: Option<String>,
    #[serde(default)]
    pub audio_frames: u64,
    #[serde(default)]
    pub source_audio: Vec<(String, u64)>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualClip {
    #[serde(default = "center")]
    pub x: f64,
    #[serde(default = "center")]
    pub y: f64,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default = "one")]
    pub gain: f64,
    #[serde(default)]
    pub fade_in: TimeTick,
    #[serde(default)]
    pub fade_out: TimeTick,
    pub id: String,
    pub asset_id: String,
    pub track_id: String,
    pub start: TimeTick,
    pub source_in: TimeTick,
    pub source_out: TimeTick,
    pub speed_percent: u16,
    pub crop: crate::VideoCrop,
}
fn center() -> f64 {
    0.5
}
fn one() -> f64 {
    1.0
}
impl VisualClip {
    pub fn duration(&self) -> TimeTick {
        TimeTick(
            ((self.source_out.0 - self.source_in.0) * 100 + i64::from(self.speed_percent) - 1)
                / i64::from(self.speed_percent.max(1)),
        )
    }
    pub fn end(&self) -> TimeTick {
        TimeTick(self.start.0.saturating_add(self.duration().0))
    }
    pub fn active(&self, t: TimeTick) -> bool {
        t >= self.start && t < self.end()
    }
    pub fn source_tick(&self, t: TimeTick) -> TimeTick {
        TimeTick(self.source_in.0 + (t.0 - self.start.0) * i64::from(self.speed_percent) / 100)
    }
}
pub fn safe_asset_path(p: &str) -> bool {
    p.starts_with("media/")
        && !p.contains('\\')
        && !p.contains(':')
        && p.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}
impl WorkbenchSettings {
    pub fn track_muted(&self, id: &str) -> bool {
        self.tracks
            .iter()
            .find(|t| t.id == id)
            .is_some_and(|t| t.muted)
    }
    pub fn mixing_clips(&self) -> Vec<AudioClip> {
        let mut clips = self.audio_clips.clone();
        for c in &mut clips {
            c.muted |= self.track_muted(self.audio_track(c));
        }
        for c in &self.visual_clips {
            if let Some(a) = self.visual_assets.iter().find(|a| a.id == c.asset_id) {
                if let Some(path) = &a.audio_path {
                    clips.push(AudioClip {
                        speed_percent: c.speed_percent,
                        id: c.id.clone(),
                        name: a.name.clone(),
                        path: path.clone(),
                        asset_frames: a.audio_frames,
                        start: c.start,
                        source_in: c.source_in,
                        source_out: c.source_out.min(TimeTick(
                            (a.audio_frames as i128 * 10_000_000 / 48000) as i64,
                        )),
                        role: AudioRole::Music,
                        gain: c.gain,
                        muted: self.track_muted(&c.track_id),
                        fade_in: c.fade_in,
                        fade_out: c.fade_out,
                        waveform: Vec::new(),
                    });
                }
            }
        }
        for c in &self.visual_clips {
            if let Some(a) = self.visual_assets.iter().find(|a| a.id == c.asset_id) {
                for (i, (path, frames)) in a.source_audio.iter().enumerate() {
                    clips.push(AudioClip {
                        speed_percent: c.speed_percent,
                        id: format!("{}-{i}", c.id),
                        name: a.name.clone(),
                        path: path.clone(),
                        asset_frames: *frames,
                        start: c.start,
                        source_in: c.source_in,
                        source_out: c
                            .source_out
                            .min(TimeTick((*frames as i128 * 10_000_000 / 48000) as i64)),
                        role: AudioRole::Music,
                        gain: (c.gain
                            * match path.as_str() {
                                "media/system.pcm" => self.audio.system_gain,
                                "media/microphone.pcm" => self.audio.microphone_gain,
                                "media/imported.pcm" => self.audio.imported_gain,
                                _ => 1.0,
                            })
                        .min(2.0),
                        muted: self.track_muted(&c.track_id),
                        fade_in: c.fade_in,
                        fade_out: c.fade_out,
                        waveform: Vec::new(),
                    });
                }
            }
        }
        clips.retain(AudioClip::valid);
        clips
    }
    pub fn project_duration(&self, source_duration: TimeTick) -> TimeTick {
        let video = self.video_edit(source_duration).ok();
        let base = if self.source_video_enabled && self.tracks.iter().any(|t| t.id == "video") {
            video.as_ref().map_or(source_duration, |v| v.duration())
        } else {
            TimeTick::ZERO
        };
        let audio = self.audio_clips.iter().map(AudioClip::end);
        let visual = self.visual_clips.iter().map(VisualClip::end);
        let subtitles = self.subtitles.iter().flat_map(|c| {
            if c.anchor == TimeAnchor::Project {
                vec![c.end]
            } else {
                self.source_ranges(source_duration, c.start, c.end)
                    .into_iter()
                    .map(|(_, b)| b)
                    .collect()
            }
        });
        audio
            .chain(visual)
            .chain(subtitles)
            .chain(self.follow_positions.values().map(|(_, b)| *b))
            .fold(base, TimeTick::max)
            .max(TimeTick(1))
    }
    pub fn audio_track<'a>(&'a self, c: &'a AudioClip) -> &'a str {
        self.clip_tracks
            .get(&c.id)
            .map(String::as_str)
            .unwrap_or(if c.role == AudioRole::Music {
                "music"
            } else {
                "narration"
            })
    }
    pub fn subtitle_track<'a>(&'a self, id: &str) -> &'a str {
        self.clip_tracks.get(id).map_or("subtitle", String::as_str)
    }
    pub fn add_track(&mut self, kind: TrackKind) -> Result<String, String> {
        if self.tracks.len() >= MAX_TRACKS {
            return Err("最多支持 32 条轨道".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let n = self.tracks.iter().filter(|t| t.kind == kind).count() + 1;
        self.tracks.push(Track {
            muted: false,
            id: id.clone(),
            name: format!("{} {}", kind.label(), n),
            kind,
        });
        Ok(id)
    }
    pub fn remove_track(&mut self, id: &str) -> Result<(), String> {
        if !self.tracks.iter().any(|t| t.id == id) {
            return Err("轨道已删除".into());
        }
        let audio = self
            .audio_clips
            .iter()
            .filter(|c| self.audio_track(c) == id)
            .map(|c| c.id.clone())
            .collect::<Vec<_>>();
        let cues = self
            .subtitles
            .iter()
            .filter(|c| self.subtitle_track(&c.id) == id)
            .map(|c| c.id.clone())
            .collect::<Vec<_>>();
        self.audio_clips.retain(|c| !audio.contains(&c.id));
        self.subtitles.retain(|c| !cues.contains(&c.id));
        self.follow_positions
            .retain(|cid, _| self.clip_tracks.get(cid).map_or("follow", String::as_str) != id);
        self.visual_clips.retain(|c| c.track_id != id);
        self.clip_tracks.retain(|_, t| t != id);
        self.tracks.retain(|t| t.id != id);
        if id == "follow" && !self.tracks.iter().any(|t| t.kind == TrackKind::Follow) {
            self.camera_enabled = false;
        }
        if id == "video" {
            self.audio.source_muted = true;
            self.source_video_enabled = false;
        }
        Ok(())
    }
    pub fn insert_audio_asset(
        &mut self,
        id: &str,
        track_id: &str,
        start: TimeTick,
    ) -> Result<String, String> {
        let track = self
            .tracks
            .iter()
            .find(|t| t.id == track_id && t.kind.audio())
            .ok_or("请拖入旁白或音乐轨道")?;
        let a = self
            .audio_assets
            .iter()
            .find(|a| a.id == id)
            .ok_or("素材已删除")?;
        let id = uuid::Uuid::new_v4().to_string();
        let c = AudioClip {
            speed_percent: 100,
            id: id.clone(),
            name: a.name.clone(),
            path: a.path.clone(),
            asset_frames: a.asset_frames,
            start,
            source_in: TimeTick::ZERO,
            source_out: TimeTick((a.asset_frames as i128 * 10_000_000 / 48_000) as i64),
            role: if track.kind == TrackKind::Music {
                AudioRole::Music
            } else {
                AudioRole::Narration
            },
            gain: 1.0,
            muted: false,
            fade_in: TimeTick::ZERO,
            fade_out: TimeTick::ZERO,
            waveform: a.waveform.clone(),
        };
        if !c.valid() {
            return Err("片段时间超出范围".into());
        }
        self.clip_tracks.insert(id.clone(), track_id.into());
        self.audio_clips.push(c);
        Ok(id)
    }
    pub fn insert_visual_asset(
        &mut self,
        asset_id: &str,
        track_id: &str,
        start: TimeTick,
    ) -> Result<String, String> {
        if !self
            .tracks
            .iter()
            .any(|t| t.id == track_id && t.kind == TrackKind::Video)
        {
            return Err("请拖入视频轨道".into());
        }
        let a = self
            .visual_assets
            .iter()
            .find(|a| a.id == asset_id)
            .ok_or("素材已删除")?;
        let id = uuid::Uuid::new_v4().to_string();
        let c = VisualClip {
            x: 0.5,
            y: 0.5,
            scale: 1.0,
            gain: 1.0,
            fade_in: TimeTick::ZERO,
            fade_out: TimeTick::ZERO,
            id: id.clone(),
            asset_id: asset_id.into(),
            track_id: track_id.into(),
            start,
            source_in: TimeTick::ZERO,
            source_out: a.duration,
            speed_percent: 100,
            crop: Default::default(),
        };
        if c.end().0 > MAX_MEDIA_TICK {
            return Err("片段时间超出范围".into());
        }
        self.visual_clips.push(c);
        Ok(id)
    }
    pub fn validate_timeline(&self) -> bool {
        let mut ids = std::collections::HashSet::new();
        if self.tracks.len() > MAX_TRACKS
            || self.tracks.iter().any(|t| {
                t.id.is_empty() || t.id.len() > 80 || t.name.len() > 120 || !ids.insert(&t.id)
            })
        {
            return false;
        }
        let mut assets = std::collections::HashSet::new();
        if self.visual_assets.len() > 10000
            || self.visual_assets.iter().any(|a| {
                !assets.insert(&a.id)
                    || a.id.is_empty()
                    || !safe_asset_path(&a.path)
                    || a.source_audio
                        .iter()
                        .any(|(p, n)| !safe_asset_path(p) || *n > 48000 * 86400)
                    || a.audio_path.as_ref().is_some_and(|p| !safe_asset_path(p))
                    || a.duration.0 <= 0
                    || a.duration.0 > MAX_MEDIA_TICK
                    || a.width == 0
                    || a.height == 0
            })
        {
            return false;
        }
        let mut clips = std::collections::HashSet::new();
        if self.visual_clips.len() > 10000
            || self.visual_clips.iter().any(|c| {
                !clips.insert(&c.id)
                    || !c.x.is_finite()
                    || !c.y.is_finite()
                    || !c.scale.is_finite()
                    || !(0.05..=4.0).contains(&c.scale)
                    || !c.gain.is_finite()
                    || !(0.0..=2.0).contains(&c.gain)
                    || c.fade_in.0 < 0
                    || c.fade_out.0 < 0
                    || c.fade_in > c.duration()
                    || c.fade_out > c.duration()
                    || c.id.is_empty()
                    || !self
                        .tracks
                        .iter()
                        .any(|t| t.id == c.track_id && t.kind == TrackKind::Video)
                    || !self
                        .visual_assets
                        .iter()
                        .any(|a| a.id == c.asset_id && c.source_out <= a.duration)
                    || c.start.0 < 0
                    || c.source_in.0 < 0
                    || c.source_out <= c.source_in
                    || !(25..=400).contains(&c.speed_percent)
                    || c.end().0 > MAX_MEDIA_TICK
                    || c.crop.validate().is_err()
            })
        {
            return false;
        }
        if self.subtitle_assets.len() > 10000
            || self.subtitle_assets.iter().any(|a| {
                a.id.is_empty()
                    || !safe_asset_path(&a.path)
                    || a.cues.is_empty()
                    || a.cues.len() > 10000
                    || a.cues.iter().any(|c| !c.valid())
            })
        {
            return false;
        }
        if self.follow_positions.len() > 10000
            || self
                .follow_positions
                .values()
                .any(|(a, b)| a.0 < 0 || b <= a || b.0 > MAX_MEDIA_TICK)
        {
            return false;
        }
        self.clip_tracks.len() <= 20000
            && self
                .clip_tracks
                .values()
                .all(|id| self.tracks.iter().any(|t| &t.id == id))
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleAsset {
    pub id: String,
    pub name: String,
    pub path: String,
    pub cues: Vec<crate::media_edit::SubtitleCue>,
}
impl WorkbenchSettings {
    pub fn insert_subtitle_asset(
        &mut self,
        id: &str,
        track_id: &str,
        start: TimeTick,
    ) -> Result<String, String> {
        if !self
            .tracks
            .iter()
            .any(|t| t.id == track_id && t.kind == TrackKind::Subtitle)
        {
            return Err("请拖入字幕轨道".into());
        }
        let asset = self
            .subtitle_assets
            .iter()
            .find(|a| a.id == id)
            .ok_or("字幕素材已删除")?;
        let mut cues = asset.cues.clone();
        let first = uuid::Uuid::new_v4().to_string();
        for (i, c) in cues.iter_mut().enumerate() {
            c.id = if i == 0 {
                first.clone()
            } else {
                uuid::Uuid::new_v4().to_string()
            };
            c.start.0 += start.0;
            c.end.0 += start.0;
            c.anchor = TimeAnchor::Project;
            if !c.valid() {
                return Err("字幕超出时间范围".into());
            }
        }
        for c in &cues {
            self.clip_tracks.insert(c.id.clone(), track_id.into());
            self.subtitle_links.insert(c.id.clone(), id.into());
        }
        self.subtitles.extend(cues);
        Ok(first)
    }
}
pub type ClipTracks = BTreeMap<String, String>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_edit::AudioAsset;
    #[test]
    fn assets_do_not_extend_project_but_placed_trimmed_clips_do() {
        let mut s = WorkbenchSettings::default();
        let minute = TimeTick(600_000_000);
        s.audio_assets.push(AudioAsset {
            id: "a".into(),
            name: "music".into(),
            path: format!("media/audio-clips/{}.pcm", uuid::Uuid::new_v4()),
            asset_frames: 48_000 * 180,
            waveform: vec![],
        });
        assert_eq!(s.project_duration(minute), minute);
        s.insert_audio_asset("a", "music", TimeTick(300_000_000))
            .unwrap();
        assert_eq!(s.project_duration(minute), TimeTick(2_100_000_000));
        s.audio_clips[0].source_out = minute;
        assert_eq!(s.project_duration(minute), TimeTick(900_000_000));
        assert!(s.remove_unused_audio_asset("a").is_err());
        s.remove_track("music").unwrap();
        assert_eq!(s.project_duration(minute), minute);
        s.remove_unused_audio_asset("a").unwrap();
        s.validate().unwrap();
    }
    #[test]
    fn typed_tracks_persist_and_reject_incompatible_drops() {
        let mut s = WorkbenchSettings::default();
        let id = s.add_track(TrackKind::Music).unwrap();
        assert_ne!(id, "music");
        assert!(
            s.insert_audio_asset("missing", "video", TimeTick::ZERO)
                .is_err()
        );
        let restored: WorkbenchSettings =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s, restored);
        restored.validate().unwrap();
    }
}

impl WorkbenchSettings {
    pub fn source_time_at(&self, duration: TimeTick, tick: TimeTick) -> Option<TimeTick> {
        if self.source_video_enabled {
            self.video_edit(duration)
                .ok()
                .and_then(|v| crate::TimeMapping::source_time(&v, tick).ok())
        } else {
            self.visual_clips
                .iter()
                .rev()
                .find(|c| c.asset_id == "__source__" && c.active(tick))
                .map(|c| c.source_tick(tick))
        }
    }
    pub fn source_ranges(
        &self,
        duration: TimeTick,
        a: TimeTick,
        b: TimeTick,
    ) -> Vec<(TimeTick, TimeTick)> {
        if self.source_video_enabled {
            self.video_edit(duration).map_or(Vec::new(), |v| {
                v.project_ranges(a, b)
                    .into_iter()
                    .map(|(_, a, b)| (a, b))
                    .collect()
            })
        } else {
            self.visual_clips
                .iter()
                .filter(|c| c.asset_id == "__source__")
                .filter_map(|c| {
                    let start = a.max(c.source_in);
                    let end = b.min(c.source_out);
                    (end > start).then(|| {
                        (
                            TimeTick(
                                c.start.0
                                    + (start.0 - c.source_in.0) * 100 / i64::from(c.speed_percent),
                            ),
                            TimeTick(
                                c.start.0
                                    + (end.0 - c.source_in.0) * 100 / i64::from(c.speed_percent),
                            ),
                        )
                    })
                })
                .collect()
        }
    }
}

impl WorkbenchSettings {
    pub fn cue_ranges(
        &self,
        c: &crate::media_edit::SubtitleCue,
        duration: TimeTick,
    ) -> Vec<(TimeTick, TimeTick)> {
        if c.anchor == TimeAnchor::Project {
            vec![(c.start, c.end)]
        } else {
            self.source_ranges(duration, c.start, c.end)
        }
    }
}

// Half-open intervals: touching edges are legal, overlapping clips on one track are not.
// Kept separate from load validation so older projects can be opened and repaired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OccupiedInterval {
    pub track: String,
    pub id: String,
    pub start: TimeTick,
    pub end: TimeTick,
}
impl WorkbenchSettings {
    pub fn subtitle_style_for(&self, id: &str) -> &crate::media_edit::SubtitleStyle {
        self.subtitle_styles.get(id).unwrap_or(&self.subtitle_style)
    }
    pub fn occupied_intervals(&self, duration: TimeTick) -> Vec<OccupiedInterval> {
        let mut out = Vec::new();
        let mut add = |track: &str, id: &str, start, end| {
            out.push(OccupiedInterval { track: track.into(), id: id.into(), start, end });
        };
        if self.source_video_enabled && self.tracks.iter().any(|t| t.id == "video") {
            if let Ok(video) = self.video_edit(duration) {
                for span in video.spans() { add("video", &span.clip.id, span.project_in, span.project_out); }
            }
        }
        for c in &self.visual_clips { add(&c.track_id, &c.id, c.start, c.end()); }
        for c in &self.audio_clips { add(self.audio_track(c), &c.id, c.start, c.end()); }
        for c in &self.subtitles {
            for (a,b) in self.cue_ranges(c,duration) { add(self.subtitle_track(&c.id), &c.id, a,b); }
        }
        for (id,(a,b)) in &self.follow_positions {
            add(self.clip_tracks.get(id).map_or("follow",String::as_str), id, *a,*b);
        }
        out
    }
    pub fn ensure_timeline_edit(&self, previous: &Self, duration: TimeTick) -> Result<(), String> {
        Self::check_timeline_intervals(previous.occupied_intervals(duration), self.occupied_intervals(duration))
    }
    pub fn ensure_timeline_edit_with_camera(&self,previous:&Self,duration:TimeTick,camera:&crate::CameraTrack)->Result<(),String> {
        let ranges=|settings:&Self| {
            let mut result=settings.occupied_intervals(duration);
            for c in &camera.segments {
                if settings.follow_positions.contains_key(&c.id) {continue;}
                let track=settings.clip_tracks.get(&c.id).map_or("follow",String::as_str);
                if !settings.tracks.iter().any(|t|t.id==track) {continue;}
                for (a,b) in settings.source_ranges(duration,c.start_tick,c.end_tick) {
                    result.push(OccupiedInterval{track:track.into(),id:c.id.clone(),start:a,end:b});
                }
            }
            result
        };
        Self::check_timeline_intervals(ranges(previous),ranges(self))
    }
    fn check_timeline_intervals(old:Vec<OccupiedInterval>,mut next:Vec<OccupiedInterval>)->Result<(),String> {
        if old==next {return Ok(());}
        let mut old_by_id: BTreeMap<(&str,&str),Vec<&OccupiedInterval>> = BTreeMap::new();
        for c in &old { old_by_id.entry((&c.track,&c.id)).or_default().push(c); }
        next.sort_by(|a,b| (&a.track,a.start,a.end).cmp(&(&b.track,b.start,b.end)));
        let mut active: Vec<&OccupiedInterval> = Vec::new();
        for c in &next {
            active.retain(|a| a.track == c.track && a.end > c.start);
            for a in &active {
                if a.id == c.id { continue; }
                let intersection = (a.start.max(c.start), a.end.min(c.end));
                let prior_a = old_by_id.get(&(a.track.as_str(),a.id.as_str()));
                let prior_b = old_by_id.get(&(c.track.as_str(),c.id.as_str()));
                let preexisting = prior_a.zip(prior_b).is_some_and(|(aa,bb)| aa.iter().any(|x| bb.iter().any(|y| {
                    x.start.max(y.start) <= intersection.0 && x.end.min(y.end) >= intersection.1
                })));
                if !preexisting {
                    return Err("同一轨道的片段不能重叠，请放到空白位置或另一条同类型轨道".into());
                }
            }
            active.push(c);
        }
        Ok(())
    }
    pub fn available_start(&self, track: &str, desired: TimeTick, length: TimeTick, duration: TimeTick) -> TimeTick {
        let mut start = desired;
        let mut ranges = self.occupied_intervals(duration);
        ranges.retain(|c| c.track == track);
        ranges.sort_by_key(|c| c.start);
        for c in ranges {
            if start < c.end && start.0.saturating_add(length.0) > c.start.0 { start=c.end; }
        }
        start
    }
}
#[cfg(test)]
mod occupancy_tests {
    use super::*;
    fn settings() -> WorkbenchSettings {
        let mut s=WorkbenchSettings { source_video_enabled:false, ..Default::default() };
        s.visual_assets.push(VisualAsset { id:"a".into(),name:"A".into(),path:"media/a.mp4".into(),kind:VisualKind::Video,duration:TimeTick(100),width:10,height:10,audio_path:None,audio_frames:0,source_audio:vec![] });
        s.insert_visual_asset("a","video",TimeTick(0)).unwrap(); s
    }
    #[test] fn reject_overlap_but_allow_adjacent_and_other_tracks() {
        let s=settings(); let mut n=s.clone();
        n.insert_visual_asset("a","video",TimeTick(99)).unwrap();
        assert!(n.ensure_timeline_edit(&s,TimeTick(100)).is_err());
        n.visual_clips[1].start=TimeTick(100);
        assert!(n.ensure_timeline_edit(&s,TimeTick(100)).is_ok());
        n.visual_clips[1].start=TimeTick(0); n.visual_clips[1].track_id="other".into();
        assert!(n.ensure_timeline_edit(&s,TimeTick(100)).is_ok());
    }
    #[test] fn imported_legacy_overlap_can_be_repaired_without_blocking_style_changes() {
        let mut s=settings(); s.insert_visual_asset("a","video",TimeTick(50)).unwrap();
        let mut n=s.clone(); n.visual_clips[0].gain=0.5;
        assert!(n.ensure_timeline_edit(&s,TimeTick(100)).is_ok());
        n.visual_clips[1].start=TimeTick(40);
        assert!(n.ensure_timeline_edit(&s,TimeTick(100)).is_err());
        n.visual_clips[1].start=TimeTick(100);
        assert!(n.ensure_timeline_edit(&s,TimeTick(100)).is_ok());
    }
}
