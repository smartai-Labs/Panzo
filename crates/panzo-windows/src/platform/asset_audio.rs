//! Bounded, cancellable audio preview. Playback never creates or edits a project.
use super::{
    audio_decode::{AudioDecoder, resample},
    capture_tools::Raster,
};
use panzo_core::TimeTick;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::Duration,
};
pub(crate) fn waveform(path: &Path, cancel: impl Fn() -> bool) -> Result<(Raster, i64), String> {
    let mut reader = AudioDecoder::open(path)?.ok_or("素材中没有音频")?;
    let mut peaks = Vec::<u16>::new();
    let mut duration = 0;
    let mut stride = 1usize;
    let mut count = 0;
    let mut peak = 0;
    while let Some(p) = reader.read()? {
        if cancel() {
            return Err("预览已切换".into());
        }
        duration = duration
            .max(p.tick.0 + (p.samples.len() / p.channels) as i64 * 10_000_000 / p.rate as i64);
        peak = peak.max(
            p.samples
                .iter()
                .map(|s| s.unsigned_abs())
                .max()
                .unwrap_or(0),
        );
        count += 1;
        if count >= stride {
            peaks.push(peak);
            count = 0;
            peak = 0;
        }
        if peaks.len() >= 2048 {
            peaks = peaks.chunks(2).map(|v| *v.iter().max().unwrap()).collect();
            stride *= 2;
        }
    }
    if count > 0 {
        peaks.push(peak);
    }
    let (w, h) = (512usize, 160usize);
    let mut pixels = vec![0; w * h * 4];
    for px in pixels.chunks_exact_mut(4) {
        px.copy_from_slice(&[39, 42, 46, 255]);
    }
    for x in (4..w - 4).step_by(3) {
        let a = x * peaks.len() / w;
        let b = ((x + 3) * peaks.len() / w).max(a + 1).min(peaks.len());
        let peak = peaks
            .get(a..b)
            .and_then(|v| v.iter().max())
            .copied()
            .unwrap_or(0);
        let height = (peak as usize * (h - 20) / 65536).max(1);
        for y in h / 2 - height..=h / 2 + height {
            for xx in x..x + 2 {
                pixels[(y * w + xx) * 4..(y * w + xx) * 4 + 4]
                    .copy_from_slice(&[161, 176, 199, 255]);
            }
        }
    }
    Ok((
        Raster {
            width: w as u32,
            height: h as u32,
            pixels,
        },
        duration,
    ))
}
pub(crate) struct Playback {
    stop: Arc<AtomicBool>,
    pub(crate) result: Receiver<Result<(), String>>,
}
impl Drop for Playback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl Playback {
    pub(crate) fn start(path: PathBuf, position: i64) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = stop.clone();
        let (tx, result) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(play(&path, position, &cancel));
        });
        Self { stop, result }
    }
}
fn play(path: &Path, position: i64, cancel: &AtomicBool) -> Result<(), String> {
    use windows::Win32::Media::Audio::{
        AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
        AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioRenderClient,
    };
    let mut decoder = AudioDecoder::open(path)?.ok_or("没有可播放的音频")?;
    decoder.seek_raw(TimeTick(position))?;
    let client = super::audio_capture::open_client(false, None)?;
    let format = super::audio_capture::pcm_format();
    unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            400_000,
            0,
            &raw const format,
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    let render: IAudioRenderClient = unsafe { client.GetService() }.map_err(|e| e.to_string())?;
    let capacity = unsafe { client.GetBufferSize() }.map_err(|e| e.to_string())?;
    unsafe { client.Start() }.map_err(|e| e.to_string())?;
    let result = (|| {
        while !cancel.load(Ordering::Acquire) {
            let Some(packet) = decoder.read()? else { break };
            let (_, samples) = resample(&packet, TimeTick(position), TimeTick(i64::MAX / 4));
            let mut index = 0;
            while index < samples.len() && !cancel.load(Ordering::Acquire) {
                let padding = unsafe { client.GetCurrentPadding() }.map_err(|e| e.to_string())?;
                let frames =
                    (capacity.saturating_sub(padding) as usize).min((samples.len() - index) / 2);
                if frames == 0 {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                unsafe {
                    let buffer = render.GetBuffer(frames as u32).map_err(|e| e.to_string())?;
                    std::ptr::copy_nonoverlapping(
                        samples[index..].as_ptr(),
                        buffer.cast::<i16>(),
                        frames * 2,
                    );
                    render
                        .ReleaseBuffer(frames as u32, 0)
                        .map_err(|e| e.to_string())?;
                }
                index += frames * 2;
            }
        }
        while !cancel.load(Ordering::Acquire)
            && unsafe { client.GetCurrentPadding() }.map_err(|e| e.to_string())? > 0
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    })();
    unsafe {
        let _ = client.Stop();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn waveform_reads_real_pcm_duration_and_supports_cancellation() {
        let path =
            std::env::temp_dir().join(format!("panzo-waveform-{}.wav", uuid::Uuid::new_v4()));
        let frames = 48000u32;
        let bytes = frames * 4;
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((36 + bytes).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(48000u32.to_le_bytes());
        wav.extend(192000u32.to_le_bytes());
        wav.extend(4u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(bytes.to_le_bytes());
        for i in 0..frames {
            let v = if i % 240 < 120 { 12000i16 } else { -12000i16 };
            wav.extend(v.to_le_bytes());
            wav.extend(v.to_le_bytes());
        }
        std::fs::write(&path, wav).unwrap();
        let (r, d) = waveform(&path, || false).unwrap();
        assert_eq!((r.width, r.height), (512, 160));
        assert!((d - 10_000_000).abs() < 1000);
        assert!(r.pixels.chunks_exact(4).any(|p| p == [161, 176, 199, 255]));
        assert!(waveform(&path, || true).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
