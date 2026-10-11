//! Portable audio clips and subtitle cues. Processing plugins never own project data.
use crate::{TimeTick, VideoEdit};
use serde::{Deserialize, Serialize};
pub const MAX_MEDIA_ITEMS: usize = 10_000;
pub const MAX_MEDIA_TICK: i64 = 24 * 60 * 60 * 10_000_000;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TimeAnchor {
    Source,
    #[default]
    Project,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioRole {
    #[default]
    Narration,
    Music,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClip {
    #[serde(default="normal_speed")]
    pub speed_percent:u16,
    pub id: String,
    pub name: String,
    /// Immutable project-relative 48 kHz stereo i16 PCM asset.
    pub path: String,
    pub asset_frames: u64,
    pub start: TimeTick,
    pub source_in: TimeTick,
    pub source_out: TimeTick,
    pub role: AudioRole,
    pub gain: f64,
    pub muted: bool,
    pub fade_in: TimeTick,
    pub fade_out: TimeTick,
    pub waveform: Vec<f32>,
}
fn normal_speed()->u16{100}
impl AudioClip {
    pub fn duration(&self) -> TimeTick {
        TimeTick(((self.source_out.0-self.source_in.0)*100+i64::from(self.speed_percent)-1)/i64::from(self.speed_percent.max(1)))
    }
    pub fn end(&self) -> TimeTick {
        TimeTick(self.start.0.saturating_add(self.duration().0))
    }
    pub fn valid(&self) -> bool {
        let asset = self
            .path
            .strip_prefix("media/audio-clips/")
            .and_then(|s| s.strip_suffix(".pcm"));
        (25..=400).contains(&self.speed_percent) && !self.id.is_empty()
            && self.id.len() <= 80
            && self.name.len() <= 512
            && (asset.is_some_and(|s| uuid::Uuid::parse_str(s).is_ok() && s.len() == 36)||["media/system.pcm","media/microphone.pcm","media/imported.pcm"].contains(&self.path.as_str()))
            && self.asset_frames > 0
            && self.asset_frames <= 48_000 * 24 * 60 * 60
            && self.start.0 >= 0
            && self.start.0 <= MAX_MEDIA_TICK
            && self.source_in.0 >= 0
            && self.source_out > self.source_in
            && self.source_out.0 <= (self.asset_frames as i128 * 10_000_000 / 48_000) as i64
            && self.end().0 <= MAX_MEDIA_TICK
            && self.gain.is_finite()
            && (0.0..=2.0).contains(&self.gain)
            && self.fade_in.0 >= 0
            && self.fade_out.0 >= 0
            && self.fade_in <= self.duration()
            && self.fade_out <= self.duration()
            && self.waveform.len() <= 2048
            && self
                .waveform
                .iter()
                .all(|n| n.is_finite() && (0.0..=1.0).contains(n))
    }
    pub fn gain_at(&self, tick: TimeTick) -> f64 {
        if self.muted || tick < self.start || tick >= self.end() {
            return 0.0;
        }
        let local = tick.0 - self.start.0;
        let attack = if self.fade_in.0 > 0 {
            (local as f64 / self.fade_in.0 as f64).min(1.0)
        } else {
            1.0
        };
        let release = if self.fade_out.0 > 0 {
            ((self.duration().0 - local) as f64 / self.fade_out.0 as f64).min(1.0)
        } else {
            1.0
        };
        self.gain * attack.min(release)
    }
    pub fn split(&self, at: TimeTick) -> Result<(Self, Self), String> {
        if at <= self.start || at >= self.end() {
            return Err("播放头须位于音频片段内".into());
        }
        let mut a = self.clone();
        let mut b = self.clone();
        a.source_out = TimeTick(self.source_in.0 + (at.0-self.start.0)*i64::from(self.speed_percent)/100);
        b.id = uuid::Uuid::new_v4().to_string();
        b.start = at;
        b.source_in = a.source_out;
        a.fade_in = a.fade_in.min(a.duration());
        a.fade_out = TimeTick::ZERO;
        b.fade_in = TimeTick::ZERO;
        b.fade_out = b.fade_out.min(b.duration());
        Ok((a, b))
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleCue {
    pub id: String,
    pub start: TimeTick,
    pub end: TimeTick,
    pub text: String,
    #[serde(default)]
    pub anchor: TimeAnchor,
}
impl SubtitleCue {
    pub fn valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 80
            && self.start.0 >= 0
            && self.end > self.start
            && self.end.0 <= MAX_MEDIA_TICK
            && self.text.len() <= 4096
            && !self
                .text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
    }
    pub fn active(&self, project: TimeTick, source: TimeTick) -> bool {
        let t = if self.anchor == TimeAnchor::Source {
            source
        } else {
            project
        };
        t >= self.start && t < self.end
    }
    pub fn ranges(&self, video: &VideoEdit) -> Vec<(TimeTick, TimeTick)> {
        if self.anchor == TimeAnchor::Source {
            video
                .project_ranges(self.start, self.end)
                .into_iter()
                .map(|(_, a, b)| (a, b))
                .collect()
        } else {
            vec![(self.start, self.end)]
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SubtitleStyle {
    pub font: crate::text_style::FontStyle,
    pub opacity: u8,
    pub enabled: bool,
    pub font_size: f64,
    pub color: String,
    pub background: bool,
    pub bottom: f64,
}
impl Default for SubtitleStyle {
    fn default() -> Self {
        Self {
            font: crate::text_style::FontStyle { alignment: crate::text_style::TextAlignment::Center, ..Default::default() },
            opacity: 255,
            enabled: true,
            font_size: 0.045,
            color: "#FFFFFF".into(),
            background: true,
            bottom: 0.08,
        }
    }
}
impl SubtitleStyle {
    pub fn valid(&self) -> bool {
        self.font.valid() && self.font_size.is_finite()
            && (1.0 / 8192.0..=16.0).contains(&self.font_size)
            && self.bottom.is_finite()
            && (0.0..=0.8).contains(&self.bottom)
            && crate::editor::parse_hex_color(&self.color).is_ok()
    }
}
pub fn validate(clips: &[AudioClip], cues: &[SubtitleCue], style: &SubtitleStyle) -> bool {
    let mut ids = std::collections::HashSet::new();
    clips.len() <= 256
        && cues.len() <= MAX_MEDIA_ITEMS
        && style.valid()
        && clips.iter().all(|c| c.valid() && ids.insert(&c.id))
        && cues.iter().all(|c| c.valid() && ids.insert(&c.id))
}
pub fn time_string(t: TimeTick) -> String {
    let ms = t.0.max(0) / 10_000;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}
pub fn parse_time(s: &str) -> Result<TimeTick, String> {
    let s = s.trim().replace(',', ".");
    let pieces = s.split(':').collect::<Vec<_>>();
    let seconds = if pieces.len() == 1 {
        pieces[0].parse::<f64>().map_err(|_| "时间格式无效")?
    } else if pieces.len() == 3 {
        let h = pieces[0].parse::<u32>().map_err(|_| "小时无效")?;
        let m = pieces[1].parse::<u32>().map_err(|_| "分钟无效")?;
        let s = pieces[2].parse::<f64>().map_err(|_| "秒数无效")?;
        if m >= 60 || !(0.0..60.0).contains(&s) {
            return Err("时间格式无效".into());
        }
        f64::from(h) * 3600.0 + f64::from(m) * 60.0 + s
    } else {
        return Err("使用秒数或 00:00:00,000".into());
    };
    if !seconds.is_finite() || !(0.0..=86_400.0).contains(&seconds) {
        return Err("时间超出范围".into());
    }
    Ok(TimeTick((seconds * 10_000_000.0).round() as i64))
}
pub fn parse_srt(text: &str) -> Result<Vec<SubtitleCue>, String> {
    if text.len() > 8 * 1024 * 1024 {
        return Err("字幕文件超过 8 MB".into());
    }
    let text = text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let lines = text.lines().collect::<Vec<_>>();
    let mut i = 0;
    let mut cues = Vec::new();
    while i < lines.len() {
        if lines[i].trim().is_empty() {
            i += 1;
            continue;
        }
        if !lines[i].contains("-->") {
            lines[i].trim().parse::<u32>().map_err(|_| "SRT 序号无效")?;
            i += 1;
        }
        let timing = lines.get(i).ok_or("SRT 缺少时间")?;
        let (a, b) = timing.split_once("-->").ok_or("SRT 时间格式无效")?;
        let start = parse_time(a)?;
        let end = parse_time(b)?;
        i += 1;
        let begin = i;
        while i < lines.len() && !lines[i].trim().is_empty() {
            i += 1;
        }
        let cue = SubtitleCue {
            id: uuid::Uuid::new_v4().to_string(),
            start,
            end,
            text: lines[begin..i].join("\n"),
            anchor: TimeAnchor::Project,
        };
        if !cue.valid() || cues.len() >= MAX_MEDIA_ITEMS {
            return Err("字幕为空、时间无效或条目过多".into());
        }
        cues.push(cue);
    }
    cues.sort_by_key(|c| c.start);
    Ok(cues)
}
pub fn to_srt(cues: &[SubtitleCue], video: &VideoEdit) -> String {
    let mut spans = cues
        .iter()
        .flat_map(|c| {
            c.ranges(video)
                .into_iter()
                .map(move |(a, b)| (a, b, &c.text))
        })
        .collect::<Vec<_>>();
    spans.sort_by_key(|v| v.0);
    spans
        .iter()
        .enumerate()
        .map(|(i, (a, b, t))| {
            format!(
                "{}\r\n{} --> {}\r\n{}\r\n\r\n",
                i + 1,
                time_string(*a),
                time_string(*b),
                t.replace("\r\n", "\n").replace('\n', "\r\n")
            )
        })
        .collect()
}
/// Bind generated project-time cues to original frames so subsequent cuts retain sync.
pub fn bind_to_source(cues: &[SubtitleCue], video: &VideoEdit) -> Vec<SubtitleCue> {
    let mut result = Vec::new();
    for c in cues {
        for span in video.spans() {
            let a = c.start.max(span.project_in);
            let b = c.end.min(span.project_out);
            if b <= a {
                continue;
            }
            let mut next = c.clone();
            next.id = uuid::Uuid::new_v4().to_string();
            next.anchor = TimeAnchor::Source;
            next.start = TimeTick(
                span.clip.source_in_tick.0
                    + span.clip.source_delta(TimeTick(a.0 - span.project_in.0)).0,
            );
            next.end = TimeTick(
                span.clip.source_in_tick.0
                    + span.clip.source_delta(TimeTick(b.0 - span.project_in.0)).0,
            )
            .min(span.clip.source_out_tick);
            if next.valid() {
                result.push(next);
            }
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn srt_roundtrip_and_validation() {
        let cues =
            parse_srt("\u{feff}1\r\n00:00:01,200 --> 00:00:03,000\r\n你好\r\nPanzo\r\n").unwrap();
        let video = VideoEdit::full(TimeTick::from_millis(5000)).unwrap();
        assert_eq!(
            parse_srt(&to_srt(&cues, &video)).unwrap()[0].text,
            "你好\nPanzo"
        );
        assert!(parse_srt("1\n00:00:02,000 --> 00:00:01,000\na").is_err());
        assert!(parse_time("NaN").is_err());
        assert!(parse_time("00:62:01").is_err());
    }
    #[test]
    fn captions_follow_trim_and_speed() {
        let mut video = VideoEdit::full(TimeTick::from_millis(6000)).unwrap();
        let id = video.clips[0].id.clone();
        video
            .trim(
                &id,
                TimeTick::from_millis(1000),
                TimeTick::from_millis(5000),
            )
            .unwrap();
        let bound = bind_to_source(
            &parse_srt("1\n00:00:01,000 --> 00:00:03,000\n字幕").unwrap(),
            &video,
        );
        assert_eq!(bound[0].start, TimeTick::from_millis(2000));
        video.set_speed_percent(&id, 200).unwrap();
        assert!(to_srt(&bound, &video).contains("00:00:00,500 --> 00:00:01,500"));
    }
}

/// Project-bin metadata survives removal of all timeline instances.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioAsset {
    pub id: String,
    pub name: String,
    pub path: String,
    pub asset_frames: u64,
    pub waveform: Vec<f32>,
}
impl AudioAsset {
    pub fn from_clip(clip:&AudioClip)->Self {
        Self{id:clip.id.clone(),name:clip.name.clone(),path:clip.path.clone(),asset_frames:clip.asset_frames,waveform:clip.waveform.clone()}
    }
    pub fn valid(&self)->bool {
        let name=self.path.strip_prefix("media/audio-clips/").and_then(|s|s.strip_suffix(".pcm"));
        !self.id.is_empty() && self.id.len()<=80 && self.name.len()<=512
            && name.is_some_and(|s|s.len()==36 && uuid::Uuid::parse_str(s).is_ok())
            && self.asset_frames>0 && self.asset_frames<=48_000*24*60*60
            && self.waveform.len()<=2048 && self.waveform.iter().all(|v|v.is_finite() && (0.0..=1.0).contains(v))
    }
}

/// One display column represents a range of source samples, never just a point.
pub fn waveform_peak(peaks: &[f32], start: f64, end: f64) -> f32 {
    if peaks.is_empty() || !start.is_finite() || !end.is_finite() {return 0.0;}
    let a=(start.clamp(0.0,1.0)*peaks.len() as f64).floor() as usize;
    let b=(end.clamp(0.0,1.0)*peaks.len() as f64).ceil() as usize;
    if a>=peaks.len() {return 0.0;}
    peaks[a..b.max(a+1).min(peaks.len())].iter().copied().filter(|p|p.is_finite()).fold(0.0,f32::max).clamp(0.0,1.0)
}
/// Resolve against neighbours while dragging. Moving picks the nearest whole gap;
/// trimming stops at the neighbouring edge and never pushes another clip.
pub fn resolve_clip_placement(wanted:(i64,i64), original:(i64,i64), moving:bool, trim_start:bool, occupied:&[(i64,i64)]) -> Option<(i64,i64)> {
    let mut a=wanted.0;let mut b=wanted.1;
    if moving {
        let length=b-a;
        let legal=|a:i64|a>=0 && a.saturating_add(length)<=MAX_MEDIA_TICK && !occupied.iter().any(|(x,y)|a<*y && a+length>*x);
        if !legal(a) {
            a=std::iter::once(0).chain(occupied.iter().flat_map(|(x,y)|[*x-length,*y]))
                .filter(|p|legal(*p)).min_by_key(|p|p.abs_diff(wanted.0))?;
            b=a+length;
        }
    } else if trim_start {
        for (x,y) in occupied {if *x<original.1 && *y<=original.0 {a=a.max(*y);}}
    } else {
        for (x,y) in occupied {if *y>original.0 && *x>=original.1 {b=b.min(*x);}}
    }
    (a<b && !occupied.iter().any(|(x,y)|a<*y && b>*x)).then_some((a,b))
}
#[cfg(test)]
mod display_tests {
    use super::*;
    #[test] fn peak_preserves_transients_and_silence() {
        assert_eq!(waveform_peak(&[0.0,0.8,0.0,0.0],0.0,1.0),0.8);
        assert_eq!(waveform_peak(&[0.0,0.0],0.0,1.0),0.0);
        assert_eq!(waveform_peak(&[],0.0,1.0),0.0);
    }
    #[test] fn move_and_trim_cannot_cross_neighbours() {
        assert_eq!(resolve_clip_placement((80,180),(0,100),true,false,&[(100,200)]),Some((0,100)));
        assert_eq!(resolve_clip_placement((0,140),(0,100),false,false,&[(100,200)]),Some((0,100)));
        assert_eq!(resolve_clip_placement((170,300),(200,300),false,true,&[(100,200)]),Some((200,300)));
    }
}
