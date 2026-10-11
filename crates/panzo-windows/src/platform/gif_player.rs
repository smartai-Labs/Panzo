//! Bounded GIF decoding and a wall-clock animation player; no decoder runs while idle.
use super::capture_tools::Raster;
use image::AnimationDecoder;
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
pub(crate) struct Clip {
    pub frames: Vec<Raster>,
    ends: Vec<u64>,
    pub bytes: usize,
    pub duration_ms: u64,
}
impl Clip {
    pub fn load(path: &Path) -> Result<Arc<Self>, String> {
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 64 * 1024 * 1024 {
            return Err("GIF 文件超过 64 MB 上限".into());
        }
        let mut decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(
            std::fs::File::open(path).map_err(|e| e.to_string())?,
        ))
        .map_err(|e| e.to_string())?;
        image::ImageDecoder::set_limits(&mut decoder, image::Limits::default())
            .map_err(|e| e.to_string())?;
        let mut frames = Vec::new();
        let mut ends = Vec::new();
        let mut bytes = 0;
        let mut duration = 0;
        for frame in decoder.into_frames() {
            let frame = frame.map_err(|e| e.to_string())?;
            let (n, d) = frame.delay().numer_denom_ms();
            let delay = (u64::from(n) / u64::from(d.max(1))).clamp(20, 60_000);
            let mut buffer = frame.into_buffer();
            bytes += buffer.len();
            if bytes > 96 * 1024 * 1024 || frames.len() >= 500 {
                return Err("GIF 解码超过 96 MB 或 500 帧上限，请先缩小尺寸或缩短动图".into());
            }
            for pixel in buffer.pixels_mut() {
                pixel.0.swap(0, 2);
            }
            frames.push(Raster {
                width: buffer.width(),
                height: buffer.height(),
                pixels: buffer.into_raw(),
            });
            duration += delay;
            ends.push(duration);
        }
        if frames.is_empty() {
            return Err("GIF 没有可播放帧".into());
        }
        Ok(Arc::new(Self {
            frames,
            ends,
            bytes,
            duration_ms: duration,
        }))
    }
    pub(crate) fn frame_at(&self, millis: u64) -> usize {
        self.ends
            .partition_point(|end| *end <= millis % self.duration_ms)
            .min(self.frames.len() - 1)
    }
}
pub(crate) struct Player {
    pub clip: Arc<Clip>,
    pub playing: bool,
    pub visible: bool,
    pub index: usize,
    elapsed: Duration,
    started: Instant,
}
impl Player {
    pub fn new(clip: Arc<Clip>) -> Self {
        Self {
            clip,
            playing: true,
            visible: true,
            index: 0,
            elapsed: Duration::ZERO,
            started: Instant::now(),
        }
    }
    fn time(&self) -> Duration {
        self.elapsed
            + if self.playing && self.visible {
                self.started.elapsed()
            } else {
                Duration::ZERO
            }
    }
    pub fn set_visible(&mut self, visible: bool) {
        self.elapsed = self.time();
        self.started = Instant::now();
        self.visible = visible;
    }
    pub fn toggle(&mut self) {
        self.elapsed = self.time();
        self.started = Instant::now();
        self.playing = !self.playing;
    }
    pub fn tick(&mut self) -> bool {
        let next = self
            .clip
            .frame_at(self.time().as_millis().min(u128::from(u64::MAX)) as u64);
        let changed = next != self.index;
        self.index = next;
        changed
    }
    pub fn frame(&self) -> &Raster {
        &self.clip.frames[self.index]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timeline_uses_frame_delays_and_loops() {
        let clip = Clip {
            frames: vec![
                Raster {
                    width: 1,
                    height: 1,
                    pixels: vec![0; 4]
                };
                3
            ],
            ends: vec![100, 350, 400],
            bytes: 12,
            duration_ms: 400,
        };
        assert_eq!(clip.frame_at(99), 0);
        assert_eq!(clip.frame_at(100), 1);
        assert_eq!(clip.frame_at(399), 2);
        assert_eq!(clip.frame_at(400), 0);
    }
}
