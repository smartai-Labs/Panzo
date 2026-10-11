//! Project-bin imports run off the UI thread and never create timeline clips.
use super::{
    audio_decode::{AudioDecoder, resample},
    capture_tools::Raster,
    media_decoder::MfBgraDecoder,
};
use panzo_core::{
    TimeTick,
    media_edit::AudioAsset,
    timeline::{VisualAsset, VisualKind},
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
};
pub(crate) enum Imported {
    Audio(AudioAsset),
    Visual(VisualAsset),
    Subtitle(panzo_core::timeline::SubtitleAsset),
}
/// Owns only files created by this import until the project accepts its metadata.
pub(crate) struct StagedImport {
    pub asset: Option<Imported>,
    paths: Vec<PathBuf>,
}
impl StagedImport {
    pub fn commit(mut self) { self.paths.clear(); }
}
impl Drop for StagedImport {
    fn drop(&mut self) { for path in &self.paths { let _ = std::fs::remove_file(path); } }
}
pub(crate) struct Job {
    pub result: Receiver<Result<StagedImport, String>>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl Job {
    pub fn start(root: PathBuf, path: PathBuf) -> Self {
        let (tx, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        std::thread::spawn(move || {
            let mut staged = StagedImport { asset: None, paths: Vec::new() };
            let result = import(&root, &path, &stop, &mut staged.paths).and_then(|asset| {
                if stop.load(Ordering::Acquire) { return Err("导入已取消".into()); }
                staged.asset = Some(asset); Ok(staged)
            });
            let _ = tx.send(result);
        });
        Self { result, cancel }
    }
}
fn pcm(root: &Path, path: &Path, cancel: &AtomicBool, paths: &mut Vec<PathBuf>) -> Result<Option<AudioAsset>, String> {
    use std::io::{Seek, SeekFrom, Write};
    let Some(mut decoder) = AudioDecoder::open(path)? else {
        return Ok(None);
    };
    let id = uuid::Uuid::new_v4().to_string();
    let relative = format!("media/audio-clips/{id}.pcm");
    let target = root.join(&relative);
    std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
    paths.push(target.clone());
    let mut file = std::fs::File::create(&target).map_err(|e| e.to_string())?;
    let mut frames = 0u64;
    let mut waveform = Vec::new();
    let mut stride = 1usize;
    let mut count = 0;
    let mut peak = 0f32;
    let result = (|| {
        while let Some(packet) = decoder.read()? {
            if cancel.load(Ordering::Acquire) {
                return Err("导入已取消".into());
            }
            let (start, samples) = resample(
                &packet,
                TimeTick::ZERO,
                TimeTick(panzo_core::media_edit::MAX_MEDIA_TICK),
            );
            if start > 48000 * 86400 {
                return Err("音频超过 24 小时".into());
            }
            file.seek(SeekFrom::Start(start * 4))
                .map_err(|e| e.to_string())?;
            let bytes = samples
                .iter()
                .flat_map(|s| s.to_le_bytes())
                .collect::<Vec<_>>();
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            frames = frames.max(start + samples.len() as u64 / 2);
            peak = peak.max(
                samples
                    .iter()
                    .map(|s| s.unsigned_abs() as f32 / 32768.0)
                    .fold(0f32, f32::max),
            );
            count += 1;
            if count >= stride {
                waveform.push(peak);
                peak = 0.0;
                count = 0;
            }
            if waveform.len() >= 2048 {
                waveform = waveform
                    .chunks(2)
                    .map(|v| v.iter().copied().fold(0f32, f32::max))
                    .collect();
                stride *= 2;
            }
        }
        if count > 0 {
            waveform.push(peak);
        }
        file.flush().map_err(|e| e.to_string())?;
        if frames == 0 {
            return Err("没有音频内容".into());
        }
        Ok(())
    })();
    drop(file);
    if let Err(e) = result {
        let _ = std::fs::remove_file(&target);
        return Err(e);
    }
    Ok(Some(AudioAsset {
        id,
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        path: relative,
        asset_frames: frames,
        waveform,
    }))
}
fn import(root: &Path, path: &Path, cancel: &AtomicBool, paths: &mut Vec<PathBuf>) -> Result<Imported, String> {
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if ["wav", "mp3", "m4a", "aac", "wma", "flac", "ogg", "pcm"].contains(&ext.as_str()) {
        return pcm(root, path, cancel, paths)?
            .map(Imported::Audio)
            .ok_or("文件没有音频".into());
    }
    if ext == "srt" {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let cues = panzo_core::media_edit::parse_srt(&text)?;
        if cues.is_empty() {
            return Err("字幕文件为空".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let relative = format!("media/subtitles/{id}.srt");
        std::fs::create_dir_all(root.join("media/subtitles")).map_err(|e| e.to_string())?;
        paths.push(root.join(&relative));
        std::fs::copy(path, root.join(&relative)).map_err(|e| e.to_string())?;
        return Ok(Imported::Subtitle(panzo_core::timeline::SubtitleAsset {
            id,
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            path: relative,
            cues,
        }));
    }
    let kind = if ext == "gif" {
        VisualKind::Gif
    } else if ["png", "jpg", "jpeg", "bmp", "webp"].contains(&ext.as_str()) {
        VisualKind::Image
    } else if ["mp4", "mov", "mkv", "avi", "wmv"].contains(&ext.as_str()) {
        VisualKind::Video
    } else {
        return Err("请选择视频、图片或音频文件".into());
    };
    let (width, height, duration, audio) = if kind == VisualKind::Gif {
        let c = super::gif_player::Clip::load(path)?;
        (
            c.frames[0].width,
            c.frames[0].height,
            TimeTick((c.duration_ms * 10000) as i64),
            None,
        )
    } else if kind == VisualKind::Image {
        let r = Raster::load(path)?;
        (r.width, r.height, TimeTick(50_000_000), None)
    } else {
        let index = MfBgraDecoder::native_index(path, || cancel.load(Ordering::Acquire))
            .map_err(|e| e.to_string())?;
        let first = index.first().ok_or("视频没有画面")?;
        let last = index.last().unwrap();
        let duration = TimeTick(last.pts.0 - first.pts.0 + last.duration.map_or(166667, |d| d.0));
        (
            first.width,
            first.height,
            duration,
            pcm(root, path, cancel, paths)?,
        )
    };
    if cancel.load(Ordering::Acquire) {
        return Err("导入已取消".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let relative = format!("media/assets/{id}.{ext}");
    let target = root.join(&relative);
    std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
    paths.push(target.clone());
    std::fs::copy(path, &target).map_err(|e| e.to_string())?;
    Ok(Imported::Visual(VisualAsset {
        id,
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        path: relative,
        kind,
        duration,
        width,
        height,
        audio_path: audio.as_ref().map(|a| a.path.clone()),
        audio_frames: audio.map_or(0, |a| a.asset_frames),
        source_audio: Vec::new(),
    }))
}
pub(crate) fn choose(owner: windows::Win32::Foundation::HWND) -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(path) = REVIEW_PATH.with(|p| p.borrow_mut().take()) {
        return Some(path);
    }
    use std::os::windows::ffi::OsStringExt;
    use windows::{
        Win32::UI::Controls::Dialogs::*,
        core::{PCWSTR, PWSTR},
    };
    let filter = super::workspace_ui::wide(
        "媒体素材\0*.mp4;*.mov;*.mkv;*.avi;*.wmv;*.png;*.jpg;*.jpeg;*.bmp;*.webp;*.gif;*.wav;*.mp3;*.m4a;*.aac;*.wma;*.flac;*.ogg;*.srt\0所有文件\0*.*\0\0",
    );
    let title = super::workspace_ui::wide("导入项目素材");
    let mut file = vec![0u16; 32768];
    let mut d = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrTitle: PCWSTR(title.as_ptr()),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut d) }.as_bool() {
        return None;
    }
    let n = file.iter().position(|c| *c == 0)?;
    Some(std::ffi::OsString::from_wide(&file[..n]).into())
}

#[cfg(test)]
thread_local! {static REVIEW_PATH:std::cell::RefCell<Option<PathBuf>>=const{std::cell::RefCell::new(None)};}
#[cfg(test)]
pub(crate) fn review_choose(path: PathBuf) {
    REVIEW_PATH.with(|p| *p.borrow_mut() = Some(path));
}

#[cfg(test)]
mod transaction_tests {
 use super::*;
 #[test]
 fn staged_files_are_removed_unless_metadata_is_committed() {
  let root=std::env::temp_dir().join(format!("panzo-import-{}",uuid::Uuid::new_v4()));std::fs::create_dir_all(&root).unwrap();
  let source=root.join("original.png");image::RgbaImage::from_pixel(8,8,image::Rgba([30,90,40,255])).save(&source).unwrap();
  for commit in [false,true] {
   let mut staged=StagedImport{asset:None,paths:Vec::new()};let a=import(&root,&source,&AtomicBool::new(false),&mut staged.paths).unwrap();staged.asset=Some(a);let paths=staged.paths.clone();assert!(!paths.is_empty());
   if commit {staged.commit();}else{drop(staged);}
   assert!(source.exists());assert!(paths.iter().all(|p|p.exists()==commit));
  }
  std::fs::remove_dir_all(&root).unwrap();
 }
}
