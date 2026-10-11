// Sample positions and validated form values are clamped to nonnegative ranges before conversion.
#![allow(clippy::cast_sign_loss)]
//! Decode an MP4 audio stream to PCM; import and validation share this reader.
use crate::platform::{
    apartment::WinRtApartment,
    audio::{AudioKind, AudioTrack, PcmWriter, RATE, frames_at},
};
use panzo_core::TimeTick;
use std::{
    os::windows::ffi::OsStrExt,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFSourceReader, MF_E_INVALIDSTREAMNUMBER, MF_MT_AUDIO_BITS_PER_SAMPLE,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_FIRST_AUDIO_STREAM,
    MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION,
    MFAudioFormat_PCM, MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Audio,
    MFSTARTUP_FULL, MFShutdown, MFStartup,
};
use windows::core::PCWSTR;
struct Foundation;
impl Foundation {
    fn start() -> Result<Self, String> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.map_err(|e| e.to_string())?;
        Ok(Self)
    }
}
impl Drop for Foundation {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
        }
    }
}
pub struct AudioDecoder {
    reader: Option<IMFSourceReader>,
    raw: Option<(std::fs::File,u64)>,
    _foundation: Foundation,
    _apartment: WinRtApartment,
    rate: u32,
    channels: usize,
    ended: bool,
}
pub struct AudioPacket {
    pub tick: TimeTick,
    pub rate: u32,
    pub channels: usize,
    pub samples: Vec<i16>,
}
impl AudioDecoder {
    pub fn open(path: &Path) -> Result<Option<Self>, String> {
        let apartment = WinRtApartment::multi_threaded().map_err(|e| e.to_string())?;
        let foundation = Foundation::start()?;
        if path.extension().is_some_and(|v|v.eq_ignore_ascii_case("pcm")) {
            let file=std::fs::File::open(path).map_err(|e|e.to_string())?;
            if file.metadata().map_err(|e|e.to_string())?.len()%4!=0{return Err("PCM 数据不完整".into());}
            return Ok(Some(Self {reader:None,raw:Some((file,0)),_foundation:foundation,_apartment:apartment,rate:48000,channels:2,ended:false}));
        }
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let reader =
            unsafe { MFCreateSourceReaderFromURL(PCWSTR(wide.as_ptr()), None::<&IMFAttributes>) }
                .map_err(|e| e.to_string())?;
        let stream = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0.cast_unsigned();
        match unsafe { reader.GetNativeMediaType(stream, 0) } {
            Err(e) if e.code() == MF_E_INVALIDSTREAMNUMBER => return Ok(None),
            Err(e) => return Err(e.to_string()),
            Ok(_) => {}
        }
        unsafe { reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0.cast_unsigned(), false) }
            .map_err(|e| e.to_string())?;
        unsafe { reader.SetStreamSelection(stream, true) }.map_err(|e| e.to_string())?;
        let media = unsafe { MFCreateMediaType() }.map_err(|e| e.to_string())?;
        (|| unsafe {
            media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            media.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            media.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            reader.SetCurrentMediaType(stream, None, &media)
        })()
        .map_err(|e: windows::core::Error| e.to_string())?;
        let format = unsafe { reader.GetCurrentMediaType(stream) }.map_err(|e| e.to_string())?;
        let rate = unsafe { format.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND) }
            .map_err(|e| e.to_string())?;
        let channels = unsafe { format.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS) }
            .map_err(|e| e.to_string())? as usize;
        if !(8_000..=192_000).contains(&rate) || !(1..=8).contains(&channels) {
            return Err("不支持的音频采样率或声道数".into());
        }
        Ok(Some(Self {
            reader:Some(reader), raw:None,
            _foundation: foundation,
            _apartment: apartment,
            rate,
            channels,
            ended: false,
        }))
    }
    pub fn seek_raw(&mut self,tick:TimeTick)->Result<(),String>{
        if let Some((file,frames))=&mut self.raw{use std::io::{Seek,SeekFrom};*frames=super::audio::frames_at(tick).min(file.metadata().map_err(|e|e.to_string())?.len()/4);file.seek(SeekFrom::Start(*frames*4)).map_err(|e|e.to_string())?;self.ended=false;}Ok(())
    }
    pub fn read(&mut self) -> Result<Option<AudioPacket>, String> {
        if self.ended {
            return Ok(None);
        }
        if let Some((file,frames))=&mut self.raw {
            use std::io::Read;
            let mut bytes=vec![0;4800*4];let n=file.read(&mut bytes).map_err(|e|e.to_string())?;
            if n==0{self.ended=true;return Ok(None);}bytes.truncate(n);
            let tick=TimeTick((*frames as i128*10_000_000/48000)as i64);*frames+=(n/4)as u64;
            return Ok(Some(AudioPacket{tick,rate:48000,channels:2,samples:bytes.chunks_exact(2).map(|b|i16::from_le_bytes([b[0],b[1]])).collect()}));
        }
        loop {
            let mut flags = 0;
            let mut tick = 0;
            let mut sample = None;
            unsafe {
                self.reader.as_ref().expect("encoded audio reader").ReadSample(
                    MF_SOURCE_READER_FIRST_AUDIO_STREAM.0.cast_unsigned(),
                    0,
                    None,
                    Some(&raw mut flags),
                    Some(&raw mut tick),
                    Some(&raw mut sample),
                )
            }
            .map_err(|e| e.to_string())?;
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0.cast_unsigned() != 0 {
                return Err("导入音频格式中途发生变化".into());
            }
            self.ended = flags & MF_SOURCE_READERF_ENDOFSTREAM.0.cast_unsigned() != 0;
            if let Some(sample) = sample {
                let buffer =
                    unsafe { sample.ConvertToContiguousBuffer() }.map_err(|e| e.to_string())?;
                let mut data = std::ptr::null_mut();
                let mut length = 0;
                unsafe { buffer.Lock(&raw mut data, None, Some(&raw mut length)) }
                    .map_err(|e| e.to_string())?;
                let bytes = unsafe { std::slice::from_raw_parts(data, length as usize) };
                let samples = bytes
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]))
                    .collect::<Vec<_>>();
                unsafe { buffer.Unlock() }.map_err(|e| e.to_string())?;
                return Ok(Some(AudioPacket {
                    tick: TimeTick(tick),
                    rate: self.rate,
                    channels: self.channels,
                    samples,
                }));
            }
            if self.ended {
                return Ok(None);
            }
        }
    }
}
pub fn import_audio(
    source: &Path,
    root: &Path,
    origin: TimeTick,
    duration: TimeTick,
    cancel: &AtomicBool,
) -> Result<Option<AudioTrack>, String> {
    let Some(mut decoder) = AudioDecoder::open(source)? else {
        return Ok(None);
    };
    let mut writer = PcmWriter::create(root, AudioKind::Imported)?;
    while let Some(packet) = decoder.read()? {
        if cancel.load(Ordering::Acquire) {
            return Err("导入已取消".into());
        }
        let (start, samples) = resample(&packet, origin, duration);
        writer.write_at(start, &samples)?;
    }
    writer.finish(duration).map(Some)
}
pub(super) fn resample(packet: &AudioPacket, origin: TimeTick, duration: TimeTick) -> (u64, Vec<i16>) {
    let begin = packet.tick.0 - origin.0;
    let native_frames = packet.samples.len() / packet.channels;
    let end = begin + (native_frames as i128 * 10_000_000 / i128::from(packet.rate)) as i64;
    let first = frames_at(TimeTick(begin.max(0)));
    let last = frames_at(TimeTick(end.min(duration.0).max(0)));
    let mut samples = Vec::with_capacity(last.saturating_sub(first) as usize * 2);
    let channel = |frame: usize, c: usize| -> f64 {
        if native_frames == 0 {
            return 0.0;
        }
        let frame = frame.min(native_frames - 1);
        let base = frame * packet.channels;
        let primary = f64::from(packet.samples[base + if packet.channels == 1 { 0 } else { c }]);
        if packet.channels <= 2 {
            return primary;
        }
        let center = f64::from(packet.samples[base + 2]) * 0.707;
        let surround = if packet.channels > 4 + c {
            f64::from(packet.samples[base + 4 + c]) * 0.5
        } else {
            0.0
        };
        (primary + center + surround) / 2.207
    };
    for output in first..last {
        let position = (output as f64 / f64::from(RATE) - begin as f64 / 10_000_000.0)
            * f64::from(packet.rate);
        let frame = position.floor().max(0.0) as usize;
        let fraction = position.fract().max(0.0);
        for c in 0..2 {
            let a = channel(frame, c);
            let b = channel(frame + 1, c);
            samples.push((a + (b - a) * fraction).clamp(-32768.0, 32767.0).round() as i16);
        }
    }
    (first, samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires hardware encoder and a new PANZO_STAGE_C_DIR; uses a generated tone only"]
    fn stage_c_audio_video_roundtrip() {
        use crate::platform::{
            fmp4_writer::{Nv12Fmp4Writer, Nv12VideoConfig, normalize_export_timing},
            media_decoder::MfBgraDecoder,
        };
        let root = std::path::PathBuf::from(
            std::env::var_os("PANZO_STAGE_C_DIR").expect("isolated output directory"),
        );
        std::fs::create_dir_all(&root).unwrap();
        let raw = root.join("tone-raw.mp4");
        let output = root.join("tone.mp4");
        let mut writer = Nv12Fmp4Writer::create_with_audio(
            &raw,
            Nv12VideoConfig::sharing(320, 180, 30).unwrap(),
            true,
        )
        .unwrap();
        let mut image = vec![16; 320 * 180 * 3 / 2];
        image[320 * 180..].fill(128);
        for i in 0..90u64 {
            let start = i64::try_from(i * 10_000_000 / 30).unwrap();
            let end = i64::try_from((i + 1) * 10_000_000 / 30).unwrap();
            writer.write_nv12(&image, start, end - start).unwrap();
            let samples = (i * 1600..(i + 1) * 1600)
                .flat_map(|frame| {
                    let sample = (8000.0
                        * (frame as f64 * 440.0 * std::f64::consts::TAU / 48000.0).sin())
                        as i16;
                    [sample, sample]
                })
                .collect::<Vec<_>>();
            writer.write_audio(&samples, i * 1600).unwrap();
        }
        let report = writer.finish().unwrap();
        assert!((report.inspection.last_video_end_tick - 30_000_000).abs() < 10);
        normalize_export_timing(&raw, &output, &AtomicBool::new(false)).unwrap();
        let mut video = MfBgraDecoder::open(&output).unwrap();
        let mut frames = 0;
        while video.read_next().unwrap().is_some() {
            frames += 1;
        }
        assert_eq!(frames, 90);
        let mut decoder = AudioDecoder::open(&output).unwrap().unwrap();
        let mut count = 0;
        let mut energy = 0.0;
        let mut end = 0;
        while let Some(packet) = decoder.read().unwrap() {
            end = packet.tick.0
                + (i64::try_from(packet.samples.len()).unwrap()
                    / i64::try_from(packet.channels).unwrap())
                    * 10_000_000
                    / i64::from(packet.rate);
            count += packet.samples.len();
            energy += packet
                .samples
                .iter()
                .map(|s| f64::from(*s).powi(2))
                .sum::<f64>();
        }
        assert!(count > 280_000);
        assert!((energy / count as f64).sqrt() > 3000.0);
        assert!((end - 30_000_000).abs() < 1_000_000, "audio end {end}");
        std::fs::write(root.join("aac-proof.json"),serde_json::to_vec_pretty(&serde_json::json!({"videoFrames":frames,"videoDuration":report.inspection.last_video_end_tick,"audioEndTick":end,"audioSamples":count,"rms":(energy/count as f64).sqrt()})).unwrap()).unwrap();
    }
}
