//! Media jobs return immutable assets/cues; the UI commits results through editor undo.
use super::{
    audio::{AudioMixer, RATE, frames_at},
    media_plugin_runtime, plugins, speech_settings,
};
use panzo_core::{
    TimeTick, VideoEdit, WorkbenchSettings,
    media_edit::{AudioClip, AudioRole, SubtitleCue, TimeAnchor},
};
use panzo_plugin_api::media::{self, Output, Task};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::Duration,
};
pub(crate) enum ResultData {
    Audio(AudioClip),
    Subtitles(Vec<SubtitleCue>, bool),
}
pub(crate) enum Event {
    Countdown(u8),
    Progress(String),
    Done(Result<ResultData, String>),
}
pub(crate) struct Job {
    pub events: Receiver<Event>,
    pub cancel: Arc<AtomicBool>,
    pub stop: Option<PathBuf>,
    pub meter: Option<PathBuf>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn temp(root: &Path) -> Result<Temp, String> {
    let p = root
        .join("media")
        .join(format!("job-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    Ok(Temp(p))
}
fn plugin() -> Result<plugins::Lease, String> {
    plugins::acquire(media::PLUGIN_ID)
        .map_err(|_| "请先在插件面板安装并启用「声音、旁白与 AI 字幕」插件".into())
}
pub(crate) fn audio(
    root: PathBuf,
    source: Option<PathBuf>,
    device: Option<String>,
    _start: TimeTick,
    role: AudioRole,
    _remaining: TimeTick,
) -> Result<Job, String> {
    let lease = plugin()?;
    let tmp = temp(&root)?;
    let stop = tmp.0.join("stop");
    let meter = tmp.0.join("meter");
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let token = cancel.clone();
    let record = source.is_none();
    let signal = stop.clone();
    let levels = meter.clone();
    std::thread::spawn(move || {
        let result = (|| {
            let _tmp = tmp;
            let output = _tmp.0.join("audio.pcm");
            let task = if let Some(path) = &source {
                Task::DecodeAudio {
                    source: path.to_string_lossy().into_owned(),
                    output: output.to_string_lossy().into_owned(),
                }
            } else {
                for n in (1..=3).rev() {
                    let _ = tx.send(Event::Countdown(n));
                    for _ in 0..10 {
                        if token.load(Ordering::Acquire) || signal.exists() {
                            return Err("旁白录制已取消".into());
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
                let _ = tx.send(Event::Progress("正在录制旁白…".into()));
                Task::RecordVoice {
                    output: output.to_string_lossy().into_owned(),
                    device,
                    stop_file: signal.to_string_lossy().into_owned(),
                    meter_file: levels.to_string_lossy().into_owned(),
                    seconds: 3600,
                }
            };
            let response = media_plugin_runtime::execute(
                &lease,
                task,
                &token,
                Duration::from_secs(if record { 3615 } else { 600 }),
            )?;
            let Output::Audio { frames, waveform } = response else {
                return Err("音频插件返回类型错误".into());
            };
            if frames == 0
                || frames > u64::from(RATE) * 86400
                || std::fs::metadata(&output)
                    .map_err(|_| "插件没有返回音频文件")?
                    .len()
                    != frames * 4
            {
                return Err("插件音频文件无效".into());
            }
            let id = uuid::Uuid::new_v4().to_string();
            let relative = format!("media/audio-clips/{id}.pcm");
            let path = root.join(&relative);
            let clip = AudioClip {speed_percent:100,
                id: id.clone(),
                name: source.as_ref().and_then(|s| s.file_stem()).map_or_else(
                    || "旁白录音".into(),
                    |s| s.to_string_lossy().chars().take(120).collect(),
                ),
                path: relative,
                asset_frames: frames,
                start: TimeTick::ZERO,
                source_in: TimeTick::ZERO,
                source_out: super::audio::tick_at(frames),
                role,
                gain: 1.0,
                muted: false,
                fade_in: TimeTick::ZERO,
                fade_out: TimeTick::ZERO,
                waveform,
            };
            if !clip.valid() {
                return Err("音频片段参数无效，播放头须位于视频范围内".into());
            }
            if token.load(Ordering::Acquire) {
                return Err("音频任务已取消".into());
            }
            std::fs::create_dir_all(root.join("media/audio-clips")).map_err(|e| e.to_string())?;
            std::fs::rename(output, path).map_err(|e| e.to_string())?;
            Ok(ResultData::Audio(clip))
        })();
        let _ = tx.send(Event::Done(result));
    });
    Ok(Job {
        events: rx,
        cancel,
        stop: record.then_some(stop),
        meter: record.then_some(meter),
    })
}
pub(crate) fn subtitles(
    root: PathBuf,
    mut settings: WorkbenchSettings,
    video: VideoEdit,
    source: usize,
) -> Result<Job, String> {
    let lease = plugin()?;
    let service = speech_settings::load()?.service()?;
    let tmp = temp(&root)?;
    let duration = settings.project_duration(video.source_duration_tick);
    let clips = transcription_clips(&mut settings, source);
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let token = cancel.clone();
    std::thread::spawn(move || {
        let result = (|| {
            let _tmp = tmp;
            let path = _tmp.0.join("segment.wav");
            let mut mixer = AudioMixer::with_clips(&root, &clips)?;
            if !mixer.has_audio() { return Err("工程没有可识别的声音，请先导入音频或录制旁白".into()); }
            let step = if service.response_format == "json" {
                5
            } else {
                60
            };
            let mut start = TimeTick::ZERO;
            let mut cues = Vec::new();
            let mut approximate = false;
            while start < duration {
                if token.load(Ordering::Acquire) {
                    return Err("字幕生成已取消，已有字幕保留".into());
                }
                let end = TimeTick((start.0 + step * 10_000_000).min(duration.0));
                let _ = tx.send(Event::Progress(format!(
                    "AI 字幕 · {:.0}% · {:.0}/{:.0} 秒",
                    start.0 as f64 / duration.0 as f64 * 100.0,
                    start.0 as f64 / 1e7,
                    duration.0 as f64 / 1e7
                )));
                let total = frames_at(TimeTick(end.0 - start.0));
                let mut samples = Vec::with_capacity(total as usize * 2);
                let mut written = 0;
                while written < total {
                    if token.load(Ordering::Acquire) {
                        return Err("字幕生成已取消".into());
                    }
                    let n = (total - written).min(4800) as usize;
                    samples.extend(mixer.render(written, n, start, &video, settings.audio)?);
                    written += n as u64;
                }
                if samples.iter().any(|s| s.unsigned_abs() > 32) {
                    write_wav(&path, &samples)?;
                    let response = media_plugin_runtime::execute(
                        &lease,
                        Task::Transcribe {
                            audio: path.to_string_lossy().into_owned(),
                            duration_seconds: (end.0 - start.0) as f64 / 1e7,
                            service: service.clone(),
                        },
                        &token,
                        Duration::from_secs(service.timeout_seconds + 10),
                    )?;
                    let Output::Subtitles {
                        segments,
                        approximate: coarse,
                    } = response
                    else {
                        return Err("字幕插件返回类型错误".into());
                    };
                    approximate |= coarse;
                    for s in segments {
                        if !s.start.is_finite()
                            || !s.end.is_finite()
                            || s.start < 0.0
                            || s.end <= s.start
                            || s.end > (end.0 - start.0) as f64 / 1e7 + 0.01
                        {
                            return Err("插件字幕时间超出音频段".into());
                        }
                        let cue = SubtitleCue {
                            id: uuid::Uuid::new_v4().to_string(),
                            start: TimeTick(start.0 + (s.start * 1e7).round() as i64),
                            end: TimeTick((start.0 + (s.end * 1e7).round() as i64).min(end.0)),
                            text: s.text,
                            anchor: TimeAnchor::Project,
                        };
                        if !cue.valid() {
                            return Err("插件返回无效字幕".into());
                        }
                        cues.push(cue);
                        if cues.len() > panzo_core::media_edit::MAX_MEDIA_ITEMS {
                            return Err("字幕超过一万条，请缩短视频后重试".into());
                        }
                    }
                }
                start = end;
            }
            if token.load(Ordering::Acquire) {
                return Err("字幕生成已取消".into());
            }
            if cues.is_empty() {
                return Err("所选音轨没有识别到语音，已有字幕保留".into());
            }
            Ok(ResultData::Subtitles(cues, approximate))
        })();
        let _ = tx.send(Event::Done(result));
    });
    Ok(Job {
        events: rx,
        cancel,
        stop: None,
        meter: None,
    })
}
fn write_wav(path: &Path, samples: &[i16]) -> Result<(), String> {
    let mono = samples
        .chunks_exact(6)
        .map(|c| (c.iter().map(|v| i32::from(*v)).sum::<i32>() / 6) as i16)
        .collect::<Vec<_>>();
    let size = (mono.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(size as usize + 44);
    bytes.extend(b"RIFF");
    bytes.extend((size + 36).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(16000u32.to_le_bytes());
    bytes.extend(32000u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(size.to_le_bytes());
    bytes.extend(mono.iter().flat_map(|s| s.to_le_bytes()));
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

/// Incremental 100 ms peaks from the capture worker's actual PCM file.
/// Reads are bounded per UI tick; no full-file scan or synthetic trailing samples.
#[derive(Default)]
pub(crate) struct LiveWaveform {
    pub frames: u64,
    pub peaks: Vec<f32>,
}
impl LiveWaveform {
    pub fn read_more(&mut self, path: &Path) -> Result<bool, std::io::Error> {
        use std::io::{Read,Seek,SeekFrom};
        let mut file=std::fs::File::open(path)?;
        let available=(file.metadata()?.len()/4).min(u64::from(RATE)*3600);
        let count=available.saturating_sub(self.frames).min(u64::from(RATE));
        if count==0 {return Ok(false);}
        file.seek(SeekFrom::Start(self.frames*4))?;
        let mut bytes=vec![0;count as usize*4];file.read_exact(&mut bytes)?;
        self.append(&bytes);
        Ok(true)
    }
    fn append(&mut self, bytes:&[u8]) {
        for frame in bytes.chunks_exact(4) {
            let index=(self.frames/4800) as usize;
            if self.peaks.len()==index {self.peaks.push(0.0);}
            let peak=i16::from_le_bytes([frame[0],frame[1]]).unsigned_abs()
                .max(i16::from_le_bytes([frame[2],frame[3]]).unsigned_abs()) as f32/32768.0;
            self.peaks[index]=self.peaks[index].max(peak);
            self.frames+=1;
        }
    }
}
#[cfg(test)]
mod live_waveform_tests {
    use super::*;
    #[test]
    fn partial_bins_keep_real_peaks_and_silence() {
        let mut wave=LiveWaveform::default();
        wave.append(&[0,0,0,64].repeat(2400));
        assert_eq!(wave.frames,2400);assert_eq!(wave.peaks,vec![0.5]);
        wave.append(&[0,0,0,0].repeat(4800));
        assert_eq!(wave.frames,7200);assert_eq!(wave.peaks,vec![0.5,0.0]);
        wave.append(&[0,128,0,0].repeat(2400));
        assert_eq!(wave.frames,9600);assert_eq!(wave.peaks,vec![0.5,1.0]);
    }
}

fn transcription_clips(settings: &mut WorkbenchSettings, source: usize) -> Vec<panzo_core::media_edit::AudioClip> {
    if source != 3 {
        settings.audio.enabled = true;
        settings.audio.system_gain = if source == 0 { 1.0 } else { 0.0 };
        settings.audio.imported_gain = if source == 0 { 1.0 } else { 0.0 };
        settings.audio.microphone_gain = if source == 1 { 1.0 } else { 0.0 };
    }
    let mut clips = settings.mixing_clips();
    if source != 3 {
        clips.retain(|c| match source {
            0 => settings.visual_clips.iter().any(|v| c.id == v.id || c.id.starts_with(&format!("{}-",v.id))) && c.path != "media/microphone.pcm",
            1 => c.path == "media/microphone.pcm",
            2 => c.role == AudioRole::Narration,
            _ => false,
        });
    }
    settings.audio.source_muted |= !settings.source_video_enabled || settings.track_muted("video") || source == 2;
    clips
}

#[cfg(test)]
mod transcription_tests {
 use super::*;
 #[test]
 fn current_mix_includes_placed_video_and_respects_track_mute_and_source_disable() {
  let mut settings=WorkbenchSettings::default();settings.source_video_enabled=false;
  settings.visual_assets.push(panzo_core::timeline::VisualAsset{id:"video".into(),name:"video".into(),path:"media/clip.mp4".into(),kind:panzo_core::timeline::VisualKind::Video,duration:TimeTick(30_000_000),width:640,height:360,audio_path:Some(format!("media/audio-clips/{}.pcm",uuid::Uuid::new_v4())),audio_frames:144000,source_audio:vec![]});
  settings.insert_visual_asset("video","video",TimeTick(20_000_000)).unwrap();
  let mix=transcription_clips(&mut settings,3);assert_eq!(mix.len(),1);assert_eq!(mix[0].start,TimeTick(20_000_000));assert!(settings.audio.source_muted);assert!(!mix[0].muted);assert_eq!(settings.project_duration(TimeTick(10_000_000)),TimeTick(50_000_000));
  settings.tracks.iter_mut().find(|t|t.id=="video").unwrap().muted=true;assert!(transcription_clips(&mut settings,3)[0].muted);
 }
}
