// Sample positions and validated form values are clamped to nonnegative ranges before conversion.
#![allow(clippy::cast_sign_loss)]
//! Portable 48 kHz stereo PCM assets, bounded reads and source-time mixdown.
use panzo_core::{TimeMapping, TimeTick, VideoEdit, camera_policy::AudioSettings};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};
pub const RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioKind {
    System,
    Microphone,
    Imported,
}
impl AudioKind {
    pub const fn path(self) -> &'static str {
        match self {
            Self::System => "media/system.pcm",
            Self::Microphone => "media/microphone.pcm",
            Self::Imported => "media/imported.pcm",
        }
    }
    pub fn gain(self, s: AudioSettings) -> f64 {
        if !s.enabled || s.source_muted {
            return 0.0;
        }
        match self {
            Self::System => s.system_gain,
            Self::Microphone => s.microphone_gain,
            Self::Imported => s.imported_gain,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioTrack {
    pub kind: AudioKind,
    pub path: String,
    pub frames: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioManifest {
    pub schema: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub tracks: Vec<AudioTrack>,
}
impl AudioManifest {
    pub fn load(root: &Path) -> Result<Self, String> {
        let path = root.join("media/audio.json");
        if !path.exists() {
            return Ok(Self {
                schema: 1,
                sample_rate: RATE,
                channels: 2,
                tracks: Vec::new(),
            });
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if bytes.len() > 64 * 1024 {
            return Err("音频索引过大".into());
        }
        let m: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if m.schema != 1
            || m.sample_rate != RATE
            || m.channels != 2
            || m.tracks.len() > 3
            || m.tracks.iter().any(|t| t.path != t.kind.path())
        {
            return Err("无效音频索引".into());
        }
        for t in &m.tracks {
            if std::fs::metadata(root.join(&t.path))
                .map_err(|e| e.to_string())?
                .len()
                < t.frames.saturating_mul(4)
            {
                return Err("音频素材不完整".into());
            }
        }
        Ok(m)
    }
    pub fn save(root: &Path, tracks: Vec<AudioTrack>) -> Result<(), String> {
        crate::platform::manifest_store::persist_json_atomically(
            root.join("media/audio.json"),
            &Self {
                schema: 1,
                sample_rate: RATE,
                channels: 2,
                tracks,
            },
        )
        .map_err(|e| e.to_string())
    }
}
pub struct PcmWriter {
    file: File,
    kind: AudioKind,
    pub frames: u64,
}
impl PcmWriter {
    pub fn create(root: &Path, kind: AudioKind) -> Result<Self, String> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(kind.path()))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            file,
            kind,
            frames: 0,
        })
    }
    pub fn write_at(&mut self, start: u64, samples: &[i16]) -> Result<(), String> {
        if !samples.len().is_multiple_of(2) {
            return Err("音频数据未对齐".into());
        }
        let end = start.saturating_add(samples.len() as u64 / 2);
        if start < self.frames {
            let skip = (self.frames - start).min(samples.len() as u64 / 2) as usize * 2;
            return self.write_at(self.frames, &samples[skip..]);
        }
        if samples.is_empty() {
            return Ok(());
        }
        self.file
            .seek(SeekFrom::Start(start.checked_mul(4).ok_or("音频过长")?))
            .map_err(|e| e.to_string())?;
        let bytes = samples
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        self.file.write_all(&bytes).map_err(|e| e.to_string())?;
        self.frames = end;
        Ok(())
    }
    pub fn finish(self, duration: TimeTick) -> Result<AudioTrack, String> {
        let frames = frames_at(duration);
        self.file
            .set_len(frames.checked_mul(4).ok_or("音频过长")?)
            .map_err(|e| e.to_string())?;
        self.file.sync_all().map_err(|e| e.to_string())?;
        Ok(AudioTrack {
            kind: self.kind,
            path: self.kind.path().into(),
            frames,
        })
    }
}
pub fn frames_at(tick: TimeTick) -> u64 {
    ((i128::from(tick.0.max(0)) * i128::from(RATE)) / i128::from(panzo_core::TICKS_PER_SECOND))
        as u64
}
pub fn tick_at(frame: u64) -> TimeTick {
    TimeTick(((u128::from(frame) * panzo_core::TICKS_PER_SECOND as u128) / u128::from(RATE)) as i64)
}
struct PcmReader {
    file: File,
    track: AudioTrack,
    base: u64,
    samples: Vec<i16>,
}
impl PcmReader {
    fn value(&mut self, frame: u64, channel: usize) -> Result<f64, String> {
        if frame >= self.track.frames {
            return Ok(0.0);
        }
        let count = self.samples.len() / 2;
        if frame < self.base || frame >= self.base + count as u64 {
            self.base = frame;
            let n = (self.track.frames - frame).min(8192) as usize;
            let mut bytes = vec![0; n * 4];
            self.file
                .seek(SeekFrom::Start(frame * 4))
                .and_then(|_| self.file.read_exact(&mut bytes))
                .map_err(|e| e.to_string())?;
            self.samples = bytes
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect();
        }
        Ok(f64::from(self.samples[(frame - self.base) as usize * 2 + channel]) / 32768.0)
    }
}
#[derive(Default)]
pub struct AudioMixer {
    readers: Vec<PcmReader>,
    clips: Vec<(panzo_core::media_edit::AudioClip, PcmReader)>,
}
impl AudioMixer {
    pub fn open(root: &Path) -> Result<Self, String> {
        let m = AudioManifest::load(root)?;
        let readers = m
            .tracks
            .into_iter()
            .map(|track| {
                let file = File::open(root.join(&track.path)).map_err(|e| e.to_string())?;
                Ok(PcmReader {
                    file,
                    track,
                    base: 0,
                    samples: Vec::new(),
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { readers, clips: Vec::new() })
    }
    pub fn with_clips(root: &Path, clips: &[panzo_core::media_edit::AudioClip]) -> Result<Self, String> {
        let mut mixer = Self::open(root)?;
        mixer.set_clips(root, clips)?;
        Ok(mixer)
    }
    pub fn set_clips(&mut self, root: &Path, clips: &[panzo_core::media_edit::AudioClip]) -> Result<(), String> {
        if self.clips.iter().map(|(c,_)|c).eq(clips.iter()) { return Ok(()); }
        let mut readers = Vec::new();
        for clip in clips {
            if !clip.valid() { return Err("音频片段无效".into()); }
            let path = root.join(&clip.path).canonicalize().map_err(|_| "工程音频素材缺失")?;
            if !path.starts_with(root.canonicalize().map_err(|e|e.to_string())?) { return Err("音频素材超出工程目录".into()); }
            let file = File::open(path).map_err(|e|e.to_string())?;
            if file.metadata().map_err(|e|e.to_string())?.len() != clip.asset_frames * 4 { return Err("工程音频素材不完整".into()); }
            readers.push((clip.clone(), PcmReader { file, track: AudioTrack { kind: AudioKind::Imported, path: clip.path.clone(), frames: clip.asset_frames }, base: 0, samples: Vec::new() }));
        }
        self.clips = readers; Ok(())
    }
    pub fn has_audio(&self) -> bool {
        !self.readers.is_empty() || !self.clips.is_empty()
    }
    pub fn render(
        &mut self,
        start_frame: u64,
        frames: usize,
        project_start: TimeTick,
        video: &VideoEdit,
        settings: AudioSettings,
    ) -> Result<Vec<i16>, String> {
        let mut result = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let tick = TimeTick(project_start.0 + tick_at(start_frame + i as u64).0);
            let source = if tick < video.duration() {
                video.source_time(tick).ok()
            } else {
                None
            };
            for channel in 0..2 {
                let mut mixed = 0.0;
                if let Some(source) = source {
                    let position =
                        source.0 as f64 * f64::from(RATE) / panzo_core::TICKS_PER_SECOND as f64;
                    let frame = position.floor().max(0.0) as u64;
                    let fraction = position.fract();
                    for reader in &mut self.readers {
                        let gain = reader.track.kind.gain(settings);
                        if gain == 0.0 {
                            continue;
                        }
                        let a = reader.value(frame, channel)?;
                        let b = reader.value(frame + 1, channel)?;
                        mixed += (a + (b - a) * fraction) * gain;
                    }
                }
                if settings.enabled {
                    for (clip, reader) in &mut self.clips {
                        let gain = clip.gain_at(tick);
                        if gain == 0.0 { continue; }
                        let frame = frames_at(TimeTick(clip.source_in.0 + (tick.0-clip.start.0)*i64::from(clip.speed_percent)/100));
                        mixed += reader.value(frame, channel)? * gain;
                    }
                }
                result.push((mixed.clamp(-1.0, 1.0) * 32767.0).round() as i16);
            }
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_mute_preserves_mix_levels_through_save_and_reopen() {
        let original=AudioSettings{system_gain:0.35,microphone_gain:0.7,imported_gain:1.25,..Default::default()};
        let muted=AudioSettings{source_muted:true,..original};
        let mut loaded:AudioSettings=serde_json::from_slice(&serde_json::to_vec(&muted).unwrap()).unwrap();
        for kind in [AudioKind::System,AudioKind::Microphone,AudioKind::Imported] {assert_eq!(kind.gain(loaded),0.0);}
        loaded.source_muted=false;assert_eq!(loaded,original);
        assert!(!serde_json::from_str::<AudioSettings>(r#"{"enabled":true,"systemGain":0.5}"#).unwrap().source_muted);
    }
    #[test]
    fn pcm_gaps_and_mix_follow_trim_and_speed() {
        let root = std::env::temp_dir().join(format!("panzo-audio-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("media")).unwrap();
        let mut writer = PcmWriter::create(&root, AudioKind::System).unwrap();
        writer.write_at(480, &vec![10_000; 480 * 2]).unwrap();
        let track = writer.finish(TimeTick::from_millis(30)).unwrap();
        AudioManifest::save(&root, vec![track]).unwrap();
        let mut mixer = AudioMixer::open(&root).unwrap();
        let video = VideoEdit::full(TimeTick::from_millis(30)).unwrap();
        let samples = mixer
            .render(0, 1440, TimeTick::ZERO, &video, AudioSettings::default())
            .unwrap();
        assert!(samples[..960].iter().all(|s| *s == 0));
        assert!(samples[1000..1800].iter().all(|s| s.abs() > 9900));
        assert!(samples[2000..].iter().all(|s| *s == 0));
        assert!(
            mixer
                .render(
                    0,
                    100,
                    TimeTick::from_millis(10),
                    &video,
                    AudioSettings {
                        enabled: false,
                        ..Default::default()
                    }
                )
                .unwrap()
                .iter()
                .all(|s| *s == 0)
        );
        let mut edited = video.clone();
        let id = edited.clips[0].id.clone();
        edited
            .trim(&id, TimeTick::from_millis(10), TimeTick::from_millis(20))
            .unwrap();
        edited.set_speed_percent(&id, 200).unwrap();
        let fast = mixer
            .render(
                0,
                300,
                TimeTick::ZERO,
                &edited,
                AudioSettings {
                    system_gain: 0.5,
                    ..AudioSettings::default()
                },
            )
            .unwrap();
        assert_eq!(edited.duration(), TimeTick::from_millis(5));
        assert!(fast[20..440].iter().all(|s| (4990..5010).contains(s)));
        assert!(fast[480..].iter().all(|s| *s == 0));
        drop(mixer);
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[cfg(test)]mod clip_tests{
    use super::*;use panzo_core::media_edit::{AudioClip,AudioRole};
    #[test]fn independent_audio_clips_mix_trim_fade_and_mute(){
        let root=std::env::temp_dir().join(format!("panzo-clip-mix-{}",uuid::Uuid::new_v4()));std::fs::create_dir_all(root.join("media/audio-clips")).unwrap();let id=uuid::Uuid::new_v4().to_string();let relative=format!("media/audio-clips/{id}.pcm");
        std::fs::write(root.join(&relative),vec![10_000i16;48000*2].into_iter().flat_map(i16::to_le_bytes).collect::<Vec<_>>()).unwrap();
        let clip=AudioClip{speed_percent:100,id,name:"test".into(),path:relative,asset_frames:48000,start:TimeTick::from_millis(100),source_in:TimeTick::from_millis(200),source_out:TimeTick::from_millis(700),role:AudioRole::Narration,gain:0.5,muted:false,fade_in:TimeTick::from_millis(100),fade_out:TimeTick::from_millis(100),waveform:vec![0.3]};assert!(clip.valid());
        let video=VideoEdit::full(TimeTick::from_millis(1000)).unwrap();let mut mix=AudioMixer::with_clips(&root,&[clip.clone()]).unwrap();assert!(mix.has_audio());let pcm=mix.render(0,48000,TimeTick::ZERO,&video,AudioSettings::default()).unwrap();
        assert!(pcm[..9600].iter().all(|s|*s==0));assert!((2400..2600).contains(&pcm[7200*2]));assert!((4900..5100).contains(&pcm[14400*2]));assert!(pcm[28800*2..].iter().all(|s|*s==0));
        let(a,b)=clip.split(TimeTick::from_millis(350)).unwrap();assert_eq!(a.source_out,b.source_in);assert_eq!(a.end(),b.start);let mut muted=clip;muted.muted=true;mix.set_clips(&root,&[muted]).unwrap();assert!(mix.render(10000,100,TimeTick::ZERO,&video,AudioSettings::default()).unwrap().iter().all(|s|*s==0));drop(mix);std::fs::remove_dir_all(root).unwrap();
    }
}
