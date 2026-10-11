//! Codec/capture services called by the independent media plugin process.
use super::{
    audio::RATE,
    audio_decode::{AudioDecoder, resample},
};
use panzo_core::TimeTick;
use panzo_plugin_api::media::Output;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    time::{Duration, Instant},
};
struct AssetWriter {
    file: File,
    frames: u64,
}
impl AssetWriter {
    fn new(path: &Path) -> Result<Self, String> {
        Ok(Self {
            file: OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|e| e.to_string())?,
            frames: 0,
        })
    }
    fn write(&mut self, start: u64, samples: &[i16]) -> Result<(), String> {
        if start > u64::from(RATE) * 86400 || samples.len() % 2 != 0 {
            return Err("音频数据超限".into());
        }
        let skip = self
            .frames
            .saturating_sub(start)
            .min(samples.len() as u64 / 2) as usize
            * 2;
        let samples = &samples[skip..];
        if samples.is_empty() {
            return Ok(());
        }
        let start = start.max(self.frames);
        let end = start + samples.len() as u64 / 2;
        if end > u64::from(RATE) * 86400 {
            return Err("音频超过 24 小时".into());
        }
        self.file
            .seek(SeekFrom::Start(start * 4))
            .map_err(|e| e.to_string())?;
        self.file
            .write_all(
                &samples
                    .iter()
                    .flat_map(|s| s.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .map_err(|e| e.to_string())?;
        self.frames = end;
        Ok(())
    }
    fn finish(mut self) -> Result<Output, String> {
        if self.frames == 0 {
            return Err("音频没有可用采样".into());
        }
        self.file.sync_all().map_err(|e| e.to_string())?;
        let stride = self.frames.div_ceil(1024);
        let mut waveform = Vec::new();
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|e| e.to_string())?;
        let mut remaining = self.frames;
        while remaining > 0 {
            let frames = remaining.min(stride);
            let mut peak = 0u16;
            let mut left = frames * 4;
            let mut bytes = vec![0u8; 8192];
            while left > 0 {
                let n = left.min(bytes.len() as u64) as usize;
                self.file
                    .read_exact(&mut bytes[..n])
                    .map_err(|e| e.to_string())?;
                for b in bytes[..n].chunks_exact(2) {
                    peak = peak.max(i16::from_le_bytes([b[0], b[1]]).unsigned_abs());
                }
                left -= n as u64;
            }
            waveform.push(f32::from(peak) / 32768.0);
            remaining -= frames;
        }
        Ok(Output::Audio {
            frames: self.frames,
            waveform,
        })
    }
}
pub fn decode(source: &Path, output: &Path) -> Result<Output, String> {
    let mut decoder = AudioDecoder::open(source)?.ok_or("文件没有音轨")?;
    let mut writer = AssetWriter::new(output)?;
    let mut origin = None;
    while let Some(packet) = decoder.read()? {
        let start = *origin.get_or_insert(packet.tick);
        let (at, samples) = resample(&packet, start, TimeTick(86400 * 10_000_000));
        writer.write(at, &samples)?;
    }
    writer.finish()
}
pub fn record(
    output: &Path,
    device: Option<&str>,
    stop: &Path,
    meter: &Path,
    seconds: u64,
) -> Result<Output, String> {
    use windows::Win32::Media::Audio::{AUDCLNT_BUFFERFLAGS_SILENT, IAudioCaptureClient};
    let _apartment =
        super::apartment::WinRtApartment::multi_threaded().map_err(|e| e.to_string())?;
    let client = super::audio_capture::open_client(true, device)?;
    super::audio_capture::initialize_capture(&client, true)?;
    let capture: IAudioCaptureClient = unsafe { client.GetService() }.map_err(|e| e.to_string())?;
    let mut writer = AssetWriter::new(output)?;
    unsafe { client.Start() }.map_err(|e| e.to_string())?;
    let started = Instant::now();
    let until = Duration::from_secs(seconds.clamp(1, 3600));
    let mut last = Instant::now();
    let mut origin = None;
    let result = (|| {
        while !stop.exists() && started.elapsed() < until {
            while unsafe { capture.GetNextPacketSize() }.map_err(|e| e.to_string())? > 0 {
                let mut data = std::ptr::null_mut();
                let mut count = 0;
                let mut flags = 0;
                let mut position = 0;
                unsafe {
                    capture.GetBuffer(
                        &raw mut data,
                        &raw mut count,
                        &raw mut flags,
                        Some(&raw mut position),
                        None,
                    )
                }
                .map_err(|e| e.to_string())?;
                let samples = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                    vec![0i16; count as usize * 2]
                } else {
                    unsafe { std::slice::from_raw_parts(data, count as usize * 4) }
                        .chunks_exact(2)
                        .map(|b| i16::from_le_bytes([b[0], b[1]]))
                        .collect::<Vec<_>>()
                };
                let release = unsafe { capture.ReleaseBuffer(count) }.map_err(|e| e.to_string());
                release?;
                let base = *origin.get_or_insert(position);
                writer.write(position.saturating_sub(base), &samples)?;
                if last.elapsed() >= Duration::from_millis(100) {
                    let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
                    let _ = std::fs::write(
                        meter,
                        format!("{} {}", writer.frames, u32::from(peak) * 100 / 32768),
                    );
                    last = Instant::now();
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok::<_, String>(())
    })();
    unsafe {
        let _ = client.Stop();
    }
    result?;
    // Only captured samples define duration; stopping never appends fake trailing audio.
    writer.finish()
}
