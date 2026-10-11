//! Independent decoder for placed visual assets. Interactive drawing never waits for decoding.
use super::{
    capture_tools::Raster,
    media_decoder::{DecodedBgraFrame, DecodedFrameMetadata, MfBgraDecoder, TargetFrameRead},
};
use panzo_core::{
    TimeTick,
    timeline::{VisualAsset, VisualKind},
};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};
type Callback = Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;
#[derive(Clone, PartialEq, Eq)]
struct Key {
    limit: Option<u32>,
    path: String,
    tick: TimeTick,
}
struct Request {
    canceled: Arc<AtomicBool>,
    slot: String,
    key: Key,
    asset: VisualAsset,
}
struct Source {
    limit: Option<u32>,
    slot: String,
    path: String,
    decoder: Option<MfBgraDecoder>,
    index: Arc<Vec<DecodedFrameMetadata>>,
    frame: Option<DecodedBgraFrame>,
    image: Option<DecodedBgraFrame>,
    gif: Option<Arc<super::gif_player::Clip>>,
}
impl Source {
    fn open(
        root: &std::path::Path,
        asset: &VisualAsset,
        cancel: &impl Fn()->bool,
        preview_limit: Option<u32>,
        indexes: &mut HashMap<String, Arc<Vec<DecodedFrameMetadata>>>,
    ) -> Result<Self, String> {
        let path = root
            .join(&asset.path)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !path.starts_with(root.canonicalize().map_err(|e| e.to_string())?) {
            return Err("素材位于工程目录之外".into());
        }
        // Canonicalization validates containment; Media Foundation expects a DOS/UNC path,
        // not the verbatim \\?\ spelling returned by Windows canonicalize.
        let value = path.to_string_lossy();
        let path = if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{rest}"))
        } else if let Some(rest) = value.strip_prefix(r"\\?\") {
            PathBuf::from(rest)
        } else {
            path
        };
        if asset.kind == VisualKind::Gif {
            return Ok(Self {
                limit: preview_limit,
                slot: String::new(),
                path: asset.path.clone(),
                decoder: None,
                index: Arc::new(Vec::new()),
                frame: None,
                image: None,
                gif: Some(super::gif_player::Clip::load(&path)?),
            });
        }
        if asset.kind == VisualKind::Image {
            let r = Raster::load(&path)?;
            let native_size=Some((r.width,r.height));
            let r = if let Some(limit)=preview_limit {super::image_ops::resize(&r,limit)?}else{r};
            Ok(Self {
                limit: preview_limit,
                slot: String::new(),
                path: asset.path.clone(),
                decoder: None,
                index: Arc::new(Vec::new()),
                frame: None,
                gif: None,
                image: Some(DecodedBgraFrame {
                    native_size,
                    pts: TimeTick::ZERO,
                    duration: None,
                    width: r.width,
                    height: r.height,
                    pixels: Arc::new(r.pixels),
                }),
            })
        } else {
            let index = if let Some(index) = indexes.get(&asset.path) { index.clone() } else {
                let index = Arc::new(MfBgraDecoder::native_index(&path, cancel).map_err(|e| e.to_string())?);
                indexes.insert(asset.path.clone(), index.clone()); index
            };
            let decoder = MfBgraDecoder::open_with_preview_limit(&path, preview_limit).map_err(|e| e.to_string())?;
            Ok(Self {
                limit: preview_limit,
                slot: String::new(),
                path: asset.path.clone(),
                decoder: Some(decoder),
                index,
                frame: None,
                image: None,
                gif: None,
            })
        }
    }
    fn frame(&mut self, tick: TimeTick, cancel: &impl Fn()->bool) -> Result<DecodedBgraFrame, String> {
        if let Some(gif) = &self.gif {
            let index = gif.frame_at((tick.0.max(0) / 10000) as u64);
            let key = TimeTick(index as i64);
            if let Some(f) = &self.frame {
                if f.pts == key {
                    return Ok(f.clone());
                }
            }
            let r = &gif.frames[index];
            let f = DecodedBgraFrame {
                native_size: None,
                pts: key,
                duration: None,
                width: r.width,
                height: r.height,
                pixels: Arc::new(r.pixels.clone()),
            };
            self.frame = Some(f.clone());
            return Ok(f);
        }
        if let Some(image) = &self.image {
            return Ok(image.clone());
        }
        let origin = self.index.first().map_or(0, |f| f.pts.0);
        let at = TimeTick(origin + tick.0);
        let i = self
            .index
            .partition_point(|f| f.pts <= at)
            .saturating_sub(1);
        let target = self.index.get(i).ok_or("素材没有视频帧")?.pts;
        if let Some(f) = &self.frame {
            if f.pts == target {
                return Ok(f.clone());
            }
        }
        let decoder = self.decoder.as_mut().ok_or("解码器未就绪")?;
        if self
            .frame
            .as_ref()
            .is_none_or(|f| target < f.pts || target.0 - f.pts.0 > 10_000_000)
        {
            decoder.seek(target).map_err(|e| e.to_string())?;
        }
        loop {
            if cancel() {
                return Err("预览已关闭".into());
            }
            match decoder
                .read_next_toward(target)
                .map_err(|e| e.to_string())?
            {
                TargetFrameRead::Before(_) => {}
                TargetFrameRead::AtOrAfter(f) => {
                    self.frame = Some(f.clone());
                    return Ok(f);
                }
                TargetFrameRead::End => return self.frame.clone().ok_or("素材没有画面".into()),
            }
        }
    }
}
pub(crate) struct Visuals {
    quality: super::preview_quality::PreviewQuality,
    fixed_limit: Option<u32>,
    preview_cancel: Option<Arc<AtomicBool>>,
    tx: Sender<Request>,
    rx: Receiver<(String, Key, Result<DecodedBgraFrame, String>)>,
    pending: HashMap<String, Key>,
    ready: HashMap<String, (Key, Result<DecodedBgraFrame, String>)>,
    cancel: Arc<AtomicBool>,
    callback: Callback,
    pub interactive: bool,
}
impl Drop for Visuals {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl Visuals {
    pub fn new(root: PathBuf, interactive: bool) -> Self {
        Self::with_cancel(root, interactive, Arc::new(AtomicBool::new(false)))
    }
    pub fn with_cancel(root: PathBuf, interactive: bool, cancel: Arc<AtomicBool>) -> Self {
        Self::with_limit(root, interactive, cancel, None)
    }
    pub fn thumbnails(root:PathBuf)->Self {Self::with_limit(root,true,Arc::new(AtomicBool::new(false)),Some(160))}
    fn with_limit(root:PathBuf,interactive:bool,cancel:Arc<AtomicBool>,preview_limit:Option<u32>)->Self {
        let (tx, requests) = mpsc::channel::<Request>();
        let (output, rx) = mpsc::channel();
        let stop = cancel.clone();
        let callback: Callback = Arc::new(Mutex::new(None));
        let notify = callback.clone();
        std::thread::spawn(move || {
            let mut sources = VecDeque::<Source>::new();
            let mut indexes = HashMap::new();
            while let Ok(request) = requests.recv() {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                if request.canceled.load(Ordering::Acquire) { continue; }
                let canceled=||stop.load(Ordering::Acquire)||request.canceled.load(Ordering::Acquire);
                let result = (|| {
                    // LRU decoders and reusable indexes; one request per clip, not per file.
                    let mut source = if let Some(i) = sources.iter().position(|s| s.path == request.asset.path && s.limit==request.key.limit && (preview_limit==Some(160) || s.slot==request.slot || request.slot=="__preview_asset__" || s.slot=="__preview_asset__")) {
                        sources.remove(i).unwrap()
                    } else {
                        Source::open(&root, &request.asset, &canceled, request.key.limit, &mut indexes)?
                    };
                    source.slot=request.slot.clone();
                    let frame = source.frame(request.key.tick, &canceled);
                    sources.push_back(source);
                    if sources.len() > panzo_core::timeline::MAX_TRACKS { sources.pop_front(); }
                    frame
                })();
                if indexes.len()>panzo_core::timeline::MAX_TRACKS {
                    indexes.retain(|path,_|sources.iter().any(|s|&s.path==path));
                }
                if request.canceled.load(Ordering::Acquire) { continue; }
                if output.send((request.slot, request.key, result)).is_err() {
                    break;
                }
                let cb = notify.lock().unwrap_or_else(|e| e.into_inner()).clone();
                if let Some(cb) = cb {
                    cb();
                }
            }
        });
        Self {
            quality:Default::default(),fixed_limit:preview_limit,
            preview_cancel:None,
            tx,
            rx,
            pending: HashMap::new(),
            ready: HashMap::new(),
            cancel,
            callback,
            interactive,
        }
    }
    pub fn set_quality(&mut self,quality:super::preview_quality::PreviewQuality){self.quality=quality;}
    #[cfg(test)]
    pub fn review_size(&self,slot:&str)->Option<(u32,u32)>{self.ready.get(slot).and_then(|(_,f)|f.as_ref().ok()).map(|f|(f.width,f.height))}
    pub fn bound_cache(&mut self,limit:usize) { if self.ready.len()>limit { let drop=self.ready.keys().take(self.ready.len()-limit).cloned().collect::<Vec<_>>();for k in drop {self.ready.remove(&k);} } }
    pub fn retain_slots(&mut self, slots: &[&str]) {
        if !slots.contains(&"__preview_asset__") {
            if let Some(cancel)=self.preview_cancel.take(){cancel.store(true,Ordering::Release);}
            self.pending.remove("__preview_asset__");
        }
        self.ready.retain(|id, _| slots.contains(&id.as_str()));
    }
    pub fn callback(&self, cb: Arc<dyn Fn() + Send + Sync>) {
        *self.callback.lock().unwrap_or_else(|e| e.into_inner()) = Some(cb);
    }
    pub fn frame(
        &mut self,
        slot: &str,
        asset: &VisualAsset,
        tick: TimeTick,
    ) -> Result<Option<DecodedBgraFrame>, String> {
        let key = Key {
            limit:self.fixed_limit.or_else(||self.quality.limit(asset.width,asset.height)),
            path: asset.path.clone(),
            tick: if asset.kind == VisualKind::Image {
                TimeTick::ZERO
            } else {
                tick
            },
        };
        for (slot, key, result) in self.rx.try_iter() {
            if self.pending.get(&slot).is_some_and(|pending|*pending==key) {self.pending.remove(&slot);}
            if self.pending.get(&slot).is_none_or(|pending|pending.limit==key.limit) {
                self.ready.insert(slot, (key, result));
            }
        }
        if self.cancel.load(Ordering::Acquire) { return Err("素材解码已取消".into()); }
        if let Some((old, result)) = self.ready.get(slot) {
            if *old == key && !self.pending.contains_key(slot) {
                return result.clone().map(Some);
            }
        }
        let replace=self.pending.get(slot).is_some_and(|p|p.limit!=key.limit) || slot=="__preview_asset__" && self.pending.get(slot).is_some_and(|pending|pending.path!=key.path || pending.tick.0.abs_diff(key.tick.0)>5_000_000);
        if !self.pending.contains_key(slot) || replace {
            let canceled=Arc::new(AtomicBool::new(false));
            if slot=="__preview_asset__" {
                if let Some(previous)=self.preview_cancel.replace(canceled.clone()){previous.store(true,Ordering::Release);}
            }
            self.tx
                .send(Request {
                    canceled,
                    slot: slot.to_owned(),
                    key: key.clone(),
                    asset: asset.clone(),
                })
                .map_err(|_| "素材解码器已退出")?;
            self.pending.insert(slot.to_owned(), key.clone());
        }
        if !self.interactive {
            loop {
                if self.cancel.load(Ordering::Acquire) { return Err("素材解码已取消".into()); }
                let (ready_slot, ready_key, result) = match self.rx.recv_timeout(std::time::Duration::from_millis(20)) {
                    Ok(v) => v,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => return Err("素材解码器已退出".into()),
                };
                let matches = ready_slot == slot && ready_key == key;
                self.pending.remove(&ready_slot);
                self.ready.insert(ready_slot, (ready_key, result));
                if matches { return self.ready.get(slot).unwrap().1.clone().map(Some); }
            }
        }
        if let Some((old, result)) = self.ready.get(slot) {
            if old.path==key.path {return result.clone().map(Some);}
        }
        Ok(None)
    }
}

#[cfg(test)]
mod reliability_tests {
    use super::*;
    fn asset()->VisualAsset { VisualAsset {id:"asset".into(),name:"asset".into(),path:"media/video.mp4".into(),kind:VisualKind::Video,duration:TimeTick(100),width:1,height:1,audio_path:None,audio_frames:0,source_audio:vec![]} }
    fn frame(tick:i64)->DecodedBgraFrame { DecodedBgraFrame {native_size:None,pts:TimeTick(tick),duration:None,width:1,height:1,pixels:Arc::new(vec![tick as u8,0,0,255])} }
    #[test]
    fn simultaneous_clips_of_one_file_settle_without_repeated_requests() {
        let (tx, requests)=mpsc::channel();let (output,rx)=mpsc::channel();
        let mut v=Visuals {quality:Default::default(),fixed_limit:None,preview_cancel:None,tx,rx,pending:HashMap::new(),ready:HashMap::new(),cancel:Arc::new(AtomicBool::new(false)),callback:Arc::new(Mutex::new(None)),interactive:true};
        let asset=asset();assert!(v.frame("first",&asset,TimeTick(1)).unwrap().is_none());assert!(v.frame("second",&asset,TimeTick(5)).unwrap().is_none());
        for _ in 0..2 {let r=requests.recv().unwrap();let t=r.key.tick.0;output.send((r.slot,r.key,Ok(frame(t)))).unwrap();}
        for _ in 0..30 {assert_eq!(v.frame("first",&asset,TimeTick(1)).unwrap().unwrap().pixels[0],1);assert_eq!(v.frame("second",&asset,TimeTick(5)).unwrap().unwrap().pixels[0],5);}
        assert!(requests.try_recv().is_err());assert!(v.pending.is_empty());
    }
    #[test]
    fn quality_switch_reissues_same_frame_and_rejects_old_quality_response() {
        use super::super::preview_quality::PreviewQuality;
        let (tx,requests)=mpsc::channel();let (output,rx)=mpsc::channel();
        let mut v=Visuals {quality:Default::default(),fixed_limit:None,preview_cancel:None,tx,rx,pending:HashMap::new(),ready:HashMap::new(),cancel:Arc::new(AtomicBool::new(false)),callback:Arc::new(Mutex::new(None)),interactive:true};
        let mut a=asset();a.width=2560;a.height=1440;
        v.set_quality(PreviewQuality::Quarter);v.frame("clip",&a,TimeTick(1)).unwrap();
        let low=requests.recv().unwrap();assert_eq!(low.key.limit,Some(640));
        v.set_quality(PreviewQuality::Original);v.frame("clip",&a,TimeTick(1)).unwrap();
        let high=requests.recv().unwrap();assert_eq!(high.key.limit,None);
        output.send((low.slot,low.key,Ok(frame(9)))).unwrap();
        assert!(v.frame("clip",&a,TimeTick(1)).unwrap().is_none());
        output.send((high.slot,high.key,Ok(frame(1)))).unwrap();
        assert_eq!(v.frame("clip",&a,TimeTick(1)).unwrap().unwrap().pixels[0],1);
        assert!(requests.try_recv().is_err());
    }
    #[test]
    fn waiting_export_frame_observes_cancellation_without_decoder_response() {
        let (tx,_requests)=mpsc::channel();let (_output,rx)=mpsc::channel();let cancel=Arc::new(AtomicBool::new(false));
        let mut v=Visuals {quality:Default::default(),fixed_limit:None,preview_cancel:None,tx,rx,pending:HashMap::new(),ready:HashMap::new(),cancel:cancel.clone(),callback:Arc::new(Mutex::new(None)),interactive:false};
        let started=std::time::Instant::now();let canceler=std::thread::spawn(move||{std::thread::sleep(std::time::Duration::from_millis(30));cancel.store(true,Ordering::Release);});
        assert!(v.frame("clip",&asset(),TimeTick(1)).is_err());canceler.join().unwrap();assert!(started.elapsed()<std::time::Duration::from_secs(1));
    }
}
