// Win32 callbacks use packed signed coordinates and bounded DPI/control counts.
#![allow(
    clippy::wildcard_imports,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::struct_excessive_bools
)]
//! Read-only asset viewport. Decode is isolated from the UI and never creates editor documents.
use super::{
    asset_index::{Asset, Kind},
    capture_tools::Raster,
    desktop_ui as ui,
    editor_ui::{self, IconKind},
    gif_player::{Clip, Player},
    media_decoder::{MfBgraDecoder, TargetFrameRead},
    workspace_ui as controls,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::Instant,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus},
            WindowsAndMessaging::*,
        },
    },
    core::w,
};
const TIMER: usize = 93;
enum Request {
    Load(u64, Asset),
    Frame(u64, i64),
    Stop,
}
enum Event {
    Image(u64, Raster),
    Audio(u64, Raster, i64),
    Gif(u64, Arc<Clip>),
    Video(u64, Raster, i64),
    Frame(u64, Raster),
    Error(u64, String),
}
struct Worker {
    tx: Sender<Request>,
    rx: Receiver<Event>,
    generation: Arc<AtomicU64>,
    library_seek_recovery:Arc<std::sync::atomic::AtomicBool>,
}
impl Worker {
    fn new() -> Self {
        let (tx, input) = mpsc::channel();
        let (output, rx) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let epoch = generation.clone();
        let library_seek_recovery=Arc::new(std::sync::atomic::AtomicBool::new(false));
        let recover_seek=library_seek_recovery.clone();
        std::thread::spawn(move || {
            let mut video: Option<(
                u64,
                MfBgraDecoder,
                Vec<super::media_decoder::DecodedFrameMetadata>,
                i64,
                i64,
                std::path::PathBuf,
            )> = None;
            while let Ok(request) = input.recv() {
                match request {
                    Request::Stop => break,
                    Request::Load(id, asset) => {
                        video = None;
                        if epoch.load(Ordering::Acquire) != id {
                            continue;
                        }
                        let result = (|| -> Result<Event, String> {
                            if asset.missing || !asset.path.exists() {
                                return Err("文件已移动或删除。请重新定位文件后刷新素材库。".into());
                            }
                            match asset.kind {
                                Kind::Audio => {
                                    let (r, d) = super::asset_audio::waveform(&asset.path, || {
                                        epoch.load(Ordering::Acquire) != id
                                    })?;
                                    Ok(Event::Audio(id, r, d))
                                }
                                Kind::Image => {
                                    Raster::load(&asset.path).map(|r| Event::Image(id, r))
                                }
                                Kind::Gif => {
                                    Clip::load(&asset.path).map(|clip| Event::Gif(id, clip))
                                }
                                Kind::Video | Kind::Project => {
                                    let source = super::asset_index::video_source(&asset)?;
                                    let mut decoder =
                                        MfBgraDecoder::open_with_preview_limit(&source, Some(1280))
                                            .map_err(|e| e.to_string())?;
                                    let first = decoder
                                        .read_next()
                                        .map_err(|e| e.to_string())?
                                        .ok_or("视频没有可预览的画面")?;
                                    let frame = Raster {
                                        width: first.width,
                                        height: first.height,
                                        pixels: first.pixels.as_ref().clone(),
                                    };
                                    // Show the poster immediately; indexing remains cancellable on this worker.
                                    if output.send(Event::Video(id, frame.clone(), 0)).is_err() {
                                        return Err("预览已关闭".into());
                                    }
                                    let index = MfBgraDecoder::native_index(&source, || {
                                        epoch.load(Ordering::Acquire) != id
                                    })
                                    .map_err(|e| e.to_string())?;
                                    let origin = index.first().map_or(0, |f| f.pts.0);
                                    let duration = index.last().map_or(0, |f| {
                                        (f.pts.0 - origin + f.duration.map_or(333_333, |d| d.0))
                                            .max(0)
                                    });
                                    video = Some((id, decoder, index, origin, first.pts.0,source));
                                    Ok(Event::Video(id, frame, duration))
                                }
                            }
                        })();
                        if epoch.load(Ordering::Acquire) == id
                            && output
                                .send(result.unwrap_or_else(|e| Event::Error(id, e)))
                                .is_err()
                        {
                            break;
                        }
                    }
                    Request::Frame(id, tick) => {
                        if epoch.load(Ordering::Acquire) != id {
                            continue;
                        }
                        let Some((loaded, decoder, index, origin, last, path)) = &mut video else {
                            continue;
                        };
                        if *loaded != id {
                            continue;
                        }
                        let source = tick + *origin;
                        let i = index
                            .partition_point(|f| f.pts.0 <= source)
                            .saturating_sub(1);
                        let target = index.get(i).map_or(source, |f| f.pts.0);
                        let result = (|| -> Result<Raster, String> {
                            if target <= *last || target - *last > 10_000_000 {
                                if let Err(error)=decoder.seek(panzo_core::TimeTick(target)) {
                                    if !recover_seek.load(Ordering::Acquire){return Err(error.to_string());}
                                    // Some MF sources reject random offsets. The library can reopen
                                    // and decode forward, with the same cancellation checks below.
                                    *decoder=MfBgraDecoder::open_with_preview_limit(&*path,Some(1280)).map_err(|e|e.to_string())?;
                                }
                            }
                            loop {
                                if epoch.load(Ordering::Acquire) != id {
                                    return Err("预览已切换".into());
                                }
                                match decoder
                                    .read_next_toward(panzo_core::TimeTick(target))
                                    .map_err(|e| e.to_string())?
                                {
                                    TargetFrameRead::AtOrAfter(f) => {
                                        *last = f.pts.0;
                                        return Ok(Raster {
                                            width: f.width,
                                            height: f.height,
                                            pixels: f.pixels.as_ref().clone(),
                                        });
                                    }
                                    TargetFrameRead::Before(_) => {}
                                    TargetFrameRead::End => return Err("已到达视频结尾".into()),
                                }
                            }
                        })();
                        if epoch.load(Ordering::Acquire) == id
                            && output
                                .send(
                                    result.map_or_else(
                                        |e| Event::Error(id, e),
                                        |r| Event::Frame(id, r),
                                    ),
                                )
                                .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
        Self { tx, rx, generation,library_seek_recovery }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let _ = self.tx.send(Request::Stop);
    }
}
#[derive(Clone, Copy)]
struct View {
    zoom: f64,
    pan: (f64, f64),
    fit: bool,
}
impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 1.,
            pan: (0., 0.),
            fit: true,
        }
    }
}
impl View {
    fn scale(&self, image: &Raster, area: RECT) -> f64 {
        if self.fit {
            (f64::from((area.right - area.left).max(1)) / f64::from(image.width))
                .min(f64::from((area.bottom - area.top).max(1)) / f64::from(image.height))
                .min(1.)
        } else {
            self.zoom
        }
    }
    fn rect(&self, image: &Raster, area: RECT) -> RECT {
        let scale = self.scale(image, area);
        let width = (f64::from(image.width) * scale).round() as i32;
        let height = (f64::from(image.height) * scale).round() as i32;
        ui::rect(
            area.left + ((area.right - area.left - width) / 2) + self.pan.0 as i32,
            area.top + ((area.bottom - area.top - height) / 2) + self.pan.1 as i32,
            width,
            height,
        )
    }
    fn zoom_at(&mut self, image: &Raster, area: RECT, point: POINT, factor: f64) {
        let old = self.scale(image, area);
        let new = (old * factor).clamp(0.05, 16.);
        let cx = f64::from(area.left + area.right) / 2.;
        let cy = f64::from(area.top + area.bottom) / 2.;
        self.pan = (
            (f64::from(point.x) - cx) - (f64::from(point.x) - cx - self.pan.0) * new / old,
            (f64::from(point.y) - cy) - (f64::from(point.y) - cy - self.pan.1) * new / old,
        );
        self.fit = false;
        self.zoom = new;
    }
}
/// Optional editable composition layer, painted inside the viewport's buffered frame.
/// Selection decorations are never part of the exported raster.
pub(crate) struct CollageLayer {
    pub geometry: super::collage_model::Geometry,
    pub sources: Vec<Arc<Raster>>,
    pub sizes: Vec<(u32, u32)>,
    pub photos: Vec<super::collage_model::Photo>,
    pub fit: super::collage_model::Fit,
    pub background: [u8; 4],
    pub selected: usize,
    pub live: bool,
}
struct Preview {
    bin_mode:bool,
    text_content:Option<String>,
    video_audio:Option<std::path::PathBuf>,
    collage: Option<CollageLayer>,
    canvas_only: bool,
    compact: bool,
    library_mode:bool,
    library_controls:Option<(bool,bool,bool,bool)>,
    worker: Worker,
    generation: u64,
    image: Option<Raster>,
    gif: Option<Player>,
    video: bool,
    audio_path: Option<std::path::PathBuf>,
    audio_player: Option<super::asset_audio::Playback>,
    duration: i64,
    position: i64,
    started: Instant,
    playing: bool,
    in_flight: bool,
    queued_seek: Option<i64>,
    visible: bool,
    title: String,
    status: String,
    error: bool,
    view: View,
    drag: Option<(POINT, (f64, f64))>,
    buttons: Vec<HWND>,
    dpi: u32,
}
impl Preview {
    fn frame(&self) -> Option<&Raster> {
        self.gif.as_ref().map(Player::frame).or(self.image.as_ref())
    }
    fn current_tick(&self) -> i64 {
        (self.position
            + if self.playing {
                (self.started.elapsed().as_secs_f64() * 10_000_000.) as i64
            } else {
                0
            })
        .min(self.duration)
    }
    fn pause(&mut self) {
        self.position = self.current_tick();
        self.playing = false;
        self.audio_player = None;
    }
    fn area(&self, hwnd: HWND) -> RECT {
        let r = controls::rect(hwnd);
        let d = |n| n * self.dpi as i32 / 96;
        if self.canvas_only {return ui::rect(d(26),d(20),(r.right-d(52)).max(1),(r.bottom-d(40)).max(1));}
        if self.bin_mode{return ui::rect(d(6),d(32),(r.right-d(12)).max(1),(r.bottom-d(110)).max(1));}
        if self.compact {return r;}
        if self.library_mode{return ui::rect(d(12),d(12),(r.right-d(24)).max(1),(r.bottom-d(116)).max(1));}
        ui::rect(
            d(12),
            d(44),
            (r.right - d(24)).max(1),
            (r.bottom - d(148)).max(1),
        )
    }
    fn timeline(&self, hwnd: HWND) -> RECT {
        let r = controls::rect(hwnd);
        let d = |n| n * self.dpi as i32 / 96;
        ui::rect(d(18), r.bottom - d(if self.bin_mode{56}else{70}), (r.right - d(36)).max(1), d(18))
    }
    fn layout(&mut self, hwnd: HWND) {
        self.dpi = editor_ui::window_dpi(hwnd);
        let r = controls::rect(hwnd);
        let d = |n| n * self.dpi as i32 / 96;
        for (i, b) in self.buttons.iter().enumerate() {
            if self.bin_mode {controls::show(*b,i==0||i==3);controls::place(*b,d(if i==0{6}else{40}),r.bottom-d(32),d(28),d(26));continue;}
            controls::show(*b, !self.canvas_only && (!self.compact || (i==0 && (self.video || self.gif.is_some() || self.audio_path.is_some()))));
            controls::place(*b, if self.compact {(r.right-d(30))/2}else{d(12+34*i as i32)}, if self.compact {(r.bottom-d(28))/2}else{r.bottom-d(42)},d(30),d(28));
        }
        ui::invalidate(hwnd);
    }
    fn controls(&mut self) {
        if self.library_mode {
            let can_play=self.gif.is_some()||((self.video||self.audio_path.is_some())&&self.duration>0);
            let playing=self.gif.as_ref().map_or(self.playing,|g|g.playing);
            let signature=(can_play,playing,self.frame().is_some(),self.video||self.gif.is_some()||self.audio_path.is_some());
            if self.library_controls==Some(signature){return;}
            self.library_controls=Some(signature);
        }
        if self.compact && !self.library_mode {if let Ok(h)=unsafe{GetParent(self.buttons[0])} {if let Ok(parent)=unsafe{GetParent(h)} {ui::invalidate(parent);}}}
        if let Ok(h)=unsafe {GetParent(self.buttons[0])} {self.layout(h);}
        use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
        let can_play =
            self.gif.is_some() || ((self.video || self.audio_path.is_some()) && self.duration > 0);
        unsafe {
            let _ = EnableWindow(self.buttons[0], can_play);
        }
        let playing = self.gif.as_ref().map_or(self.playing, |g| g.playing);
        ui::style(
            self.buttons[0],
            Some(if playing {
                IconKind::Pause
            } else {
                IconKind::Play
            }),
            true,
            false,
            true,
        );
        for b in &self.buttons[1..] {
            unsafe {
                let _ = EnableWindow(*b, self.frame().is_some());
            }
        }
    }
    fn toggle(&mut self) {
        if let Some(gif) = &mut self.gif {
            gif.toggle();
        } else if (self.video || self.audio_path.is_some()) && self.duration > 0 {
            if self.playing {
                self.pause();
            } else {
                if self.position >= self.duration {
                    self.position = 0;
                }
                self.started = Instant::now();
                self.playing = true;
                if let Some(path) = self.audio_path.as_ref().or(self.video_audio.as_ref()) {
                    self.audio_player = Some(super::asset_audio::Playback::start(
                        path.clone(),
                        self.position,
                    ));
                }
            }
        }
        self.controls();
    }
    fn seek(&mut self, tick: i64) {
        if self.audio_path.is_some() {
            self.position = tick.clamp(0, self.duration.saturating_sub(1));
            self.started = Instant::now();
            if self.playing {
                self.audio_player = Some(super::asset_audio::Playback::start(
                    self.audio_path.clone().unwrap(),
                    self.position,
                ));
            }
            return;
        }
        if !self.video || self.duration <= 0 {
            return;
        }
        self.position = tick.clamp(0, self.duration.saturating_sub(1));
        self.started = Instant::now();
        if self.playing{if let Some(path)=&self.video_audio{self.audio_player=Some(super::asset_audio::Playback::start(path.clone(),self.position));}}
        self.queued_seek = Some(self.position);
        if !self.in_flight {
            self.queued_seek = None;
            self.in_flight = self
                .worker
                .tx
                .send(Request::Frame(self.generation, self.position))
                .is_ok();
        }
    }
    fn tick(&mut self, hwnd: HWND) {
        let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
        if self.visible != visible {
            self.visible = visible;
            if let Some(g) = &mut self.gif {
                g.set_visible(visible);
            }
            if !visible {
                self.pause();
                self.controls();
            }
        }
        let mut changed = false;
        if let Some(result) = self
            .audio_player
            .as_ref()
            .and_then(|p| p.result.try_recv().ok())
        {
            self.pause();
            changed = true;
            if let Err(e) = result {
                self.status = e;
                self.error = true;
            }
        }
        while let Ok(event) = self.worker.rx.try_recv() {
            match event {
                Event::Audio(id, image, duration) if id == self.generation => {
                    self.image = Some(image);
                    self.duration = duration;
                    self.status = format!("声音 · {:.1} 秒", duration as f64 / 10_000_000.);
                    self.error = false;
                    changed = true;
                }
                Event::Image(id, image) if id == self.generation => {
                    self.image = Some(image);
                    self.status = "".into();
                    self.error = false;
                    changed = true;
                }
                Event::Gif(id, clip) if id == self.generation => {
                    let mut player = Player::new(clip);
                    player.set_visible(visible);
                    self.gif = Some(player);
                    self.status = "GIF 动图 · 可播放 / 暂停".into();
                    self.error = false;
                    changed = true;
                }
                Event::Video(id, image, duration) if id == self.generation => {
                    self.image = Some(image);
                    self.duration = duration;
                    self.status = if duration == 0 {
                        "正在读取视频时间轴…"
                    } else {
                        "静音预览"
                    }
                    .into();
                    self.error = false;
                    changed = true;
                }
                Event::Frame(id, image) if id == self.generation => {
                    self.image = Some(image);
                    self.in_flight = false;
                    changed = true;
                }
                Event::Error(id, error) if id == self.generation => {
                    self.status = format!("无法预览：{error}");
                    self.error = true;
                    self.in_flight = false;
                    self.pause();
                    changed = true;
                }
                _ => {}
            }
        }
        if !self.in_flight
            && let Some(tick) = self.queued_seek.take()
        {
            self.in_flight = self
                .worker
                .tx
                .send(Request::Frame(self.generation, tick))
                .is_ok();
        }
        if visible {
            if let Some(g) = &mut self.gif {
                changed |= g.tick();
            }
            if self.playing {
                let position = self.current_tick();
                if self.video && !self.in_flight {
                    self.in_flight = self
                        .worker
                        .tx
                        .send(Request::Frame(self.generation, position.saturating_sub(1)))
                        .is_ok();
                }
                if position >= self.duration {
                    self.pause();
                }
                changed = true;
            }
        }
        if changed {
            self.controls();
            ui::invalidate(hwnd);
        }
    }
}
pub(crate) fn create(parent: HWND) -> Result<HWND, String> {
    let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    unsafe {
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: module.into(),
            lpszClassName: w!("PanzoAssetPreview"),
            style: CS_DBLCLKS,
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
            ..Default::default()
        });
    }
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            w!("PanzoAssetPreview"),
            w!("素材预览"),
            WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_TABSTOP,
            0,
            0,
            1,
            1,
            Some(parent),
            None,
            Some(module.into()),
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    ui::install_canvas(hwnd);
    let mut buttons = Vec::new();
    for (id, label, icon) in [
        (1, "播放 / 暂停", IconKind::Play),
        (2, "缩小", IconKind::ZoomOut),
        (3, "放大", IconKind::ZoomIn),
        (4, "适应窗口", IconKind::Fit),
        (5, "实际大小 100%", IconKind::OneToOne),
    ] {
        let b = controls::button(hwnd, label, id)?;
        ui::style(b, Some(icon), true, false, true);
        buttons.push(b);
    }
    let mut state = Box::new(Preview {
        bin_mode:false,text_content:None,video_audio:None,
        collage: None,
        canvas_only: false,
        compact: false,
        library_mode:false,
        library_controls:None,
        worker: Worker::new(),
        generation: 0,
        image: None,
        gif: None,
        video: false,
        audio_path: None,
        audio_player: None,
        duration: 0,
        position: 0,
        started: Instant::now(),
        playing: false,
        in_flight: false,
        queued_seek: None,
        visible: true,
        title: "预览".into(),
        status: "选择素材".into(),
        error: false,
        view: View::default(),
        drag: None,
        buttons,
        dpi: editor_ui::window_dpi(hwnd),
    });
    state.controls();
    state.layout(hwnd);
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
        SetTimer(Some(hwnd), TIMER, 30, None);
    }
    Ok(hwnd)
}
fn state(hwnd: HWND) -> Option<&'static mut Preview> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Preview;
    if pointer.is_null() {
        None
    } else {
        Some(unsafe { &mut *pointer })
    }
}
fn reset(state: &mut Preview) {
    state.video_audio=None;state.text_content=None;
    state.collage = None;
    state.generation = state.worker.generation.fetch_add(1, Ordering::AcqRel) + 1;
    state.image = None;
    state.gif = None;
    state.video = false;
    state.audio_player = None;
    state.audio_path = None;
    state.duration = 0;
    state.position = 0;
    state.playing = false;
    state.in_flight = false;
    state.queued_seek = None;
    state.error = false;
    state.view = View::default();
    state.drag = None;
}
pub(crate) fn load(hwnd: HWND, asset: Asset) {
    if let Some(state) = state(hwnd) {
        reset(state);
        state.title = asset
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        state.video = matches!(asset.kind, Kind::Video | Kind::Project);
        state.audio_path = (asset.kind == Kind::Audio).then(|| asset.path.clone());
        if asset.kind == Kind::Project {
            state.title.push_str(" · 原始录制");
        }
        state.status = "正在加载预览…".into();
        let _ = state.worker.tx.send(Request::Load(state.generation, asset));
        state.controls();
        ui::invalidate(hwnd);
    }
}
pub(crate) fn empty(hwnd: HWND, title: &str) {
    if let Some(state) = state(hwnd) {
        reset(state);
        state.title = title.into();
        state.status = "暂无图片".into();
        state.controls();
        ui::invalidate(hwnd);
    }
}
pub(crate) fn image(hwnd: HWND, title: &str, image: Result<Raster, String>) {
    if let Some(state) = state(hwnd) {
        reset(state);
        state.title = title.into();
        match image {
            Ok(image) => {
                state.image = Some(image);
                state.status = "".into();
            }
            Err(error) => {
                state.error = true;
                state.status = error;
            }
        }
        state.controls();
        ui::invalidate(hwnd);
    }
}
pub(crate) fn notice(hwnd: HWND, message: &str) {
    if let Some(state) = state(hwnd) {
        state.status = message.into();
        state.error = true;
        ui::invalidate(hwnd);
    }
}
pub(crate) fn clear(hwnd: HWND) {
    if let Some(state) = state(hwnd) {
        reset(state);
        state.title = "预览".into();
        state.status = "选择素材".into();
        state.controls();
        ui::invalidate(hwnd);
    }
}
fn point(lp: LPARAM) -> POINT {
    POINT {
        x: i32::from(lp.0 as u16 as i16),
        y: i32::from((lp.0 >> 16) as u16 as i16),
    }
}
fn contains(r: RECT, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}
fn paint(state: &Preview, hwnd: HWND, dc: HDC) {
    let r = controls::rect(hwnd);
    let t = editor_ui::EditorTheme::current(state.dpi);
    let d = |n| t.scale(n);
    fill(dc, r, t.palette.panel);
    let area = state.area(hwnd);
    fill(dc, area, ui::canvas(t));
    if !state.canvas_only && !state.compact && !state.library_mode {
        ui::text(
            dc,
            ui::rect(d(14), d(8), (r.right - d(28)).max(1), d(28)),
            &state.title,
            t.palette.text,
            t.dpi,
            false,
        );
    }
    if let Some(text)=&state.text_content {let mut r=area;let mut text=controls::wide(text);unsafe{SetTextColor(dc,t.palette.text);SetBkMode(dc,TRANSPARENT);let old=SelectObject(dc,editor_ui::font_handle(editor_ui::TextStyle::Caption,t.dpi).into());DrawTextW(dc,&mut text,&mut r,DT_LEFT|DT_WORDBREAK);SelectObject(dc,old);}}
    else if let Some(image) = state.frame() {
        unsafe {
            let saved = SaveDC(dc);
            IntersectClipRect(dc, area.left, area.top, area.right, area.bottom);
            let viewport = state.view.rect(image, area);
            super::capture_tools::draw_raster(dc, image, viewport);
            if let Some(layer) = &state.collage {
                paint_collage(dc, viewport, layer, t);
            }
            let _ = RestoreDC(dc, saved);
        }
    } else {
        ui::text(
            dc,
            ui::rect(
                area.left + 16,
                area.top + area.bottom / 3,
                (area.right - area.left - 32).max(1),
                64,
            ),
            if state.error {
                "预览不可用"
            } else if state.status == "暂无图片" {
                "暂无图片"
            } else if state.status.starts_with("选择素材") {
                "选择素材"
            } else {
                "正在加载…"
            },
            t.palette.text_muted,
            t.dpi,
            true,
        );
    }
    if state.canvas_only || state.compact {
        return;
    }
    let status = if state.audio_path.is_some() {
        state.status.clone()
    } else if let Some(image) = state.frame() {
        let size = state
            .collage
            .as_ref()
            .map_or((image.width, image.height), |c| {
                (c.geometry.width, c.geometry.height)
            });
        let info = format!(
            "{} × {}  ·  {:.0}%",
            size.0,
            size.1,
            state.view.scale(image, area) * 100.
        );
        if state.status.is_empty() {
            info
        } else {
            format!("{info}  ·  {}", state.status)
        }
    } else {
        state.status.clone()
    };
    ui::text(
        dc,
        ui::rect(d(14), r.bottom - d(if state.bin_mode{78}else{94}), (r.right - d(28)).max(1), d(22)),
        &status,
        if state.error {
            t.palette.text
        } else {
            t.palette.text_muted
        },
        t.dpi,
        false,
    );
    if (state.video || state.audio_path.is_some()) && state.duration > 0 {
        let track = state.timeline(hwnd);
        let middle = i32::midpoint(track.top, track.bottom);
        fill(
            dc,
            ui::rect(track.left, middle - 2, track.right - track.left, 4),
            t.palette.divider,
        );
        let fraction = state.current_tick() as f64 / state.duration as f64;
        fill(
            dc,
            ui::rect(
                track.left,
                middle - 2,
                (f64::from(track.right - track.left) * fraction) as i32,
                4,
            ),
            t.palette.text_muted,
        );
        let elapsed = state.current_tick() / 10_000_000;
        let duration = state.duration / 10_000_000;
        ui::text(
            dc,
            ui::rect(d(if state.bin_mode{76}else{190}), r.bottom - d(if state.bin_mode{32}else{42}), (r.right - d(if state.bin_mode{80}else{204})).max(1), d(28)),
            &format!(
                "{:02}:{:02} / {:02}:{:02}",
                elapsed / 60,
                elapsed % 60,
                duration / 60,
                duration % 60
            ),
            t.palette.text_muted,
            t.dpi,
            false,
        );
    }
}
unsafe extern "system" fn proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Preview;
    if pointer.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wp, lp) };
    }
    if message == WM_NCDESTROY {
        unsafe {
            let _ = KillTimer(Some(hwnd), TIMER);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(pointer));
        }
        return unsafe { DefWindowProcW(hwnd, message, wp, lp) };
    }
    let state = unsafe { &mut *pointer };
    match message {
        WM_TIMER if wp.0 == TIMER => state.tick(hwnd),
        WM_SIZE => state.layout(hwnd),
        WM_COMMAND if wp.0 >> 16 == 0 => {
            match wp.0 & 0xffff {
                1 => state.toggle(),
                2 | 3 => {
                    let area = state.area(hwnd);
                    if let Some(image) = state.frame() {
                        let image = Raster {
                            width: image.width,
                            height: image.height,
                            pixels: Vec::new(),
                        };
                        state.view.zoom_at(
                            &image,
                            area,
                            POINT {
                                x: i32::midpoint(area.left, area.right),
                                y: i32::midpoint(area.top, area.bottom),
                            },
                            if wp.0 & 0xffff == 2 { 0.8 } else { 1.25 },
                        );
                    }
                }
                4 => state.view = View::default(),
                5 => {
                    state.view = View {
                        zoom: 1.,
                        pan: (0., 0.),
                        fit: false,
                    }
                }
                _ => {}
            }
            ui::invalidate(hwnd);
        }
        WM_MOUSEWHEEL => {
            let mut p = point(lp);
            unsafe {
                let _ = ScreenToClient(hwnd, &raw mut p);
            }
            if let Some(image) = state.frame() {
                let image = Raster {
                    width: image.width,
                    height: image.height,
                    pixels: Vec::new(),
                };
                state.view.zoom_at(
                    &image,
                    state.area(hwnd),
                    p,
                    if ((wp.0 >> 16) as u16 as i16) > 0 {
                        1.15
                    } else {
                        1. / 1.15
                    },
                );
                ui::invalidate(hwnd);
            }
        }
        WM_LBUTTONDOWN => {
            let p = point(lp);
            unsafe {
                let _ = SetFocus(Some(hwnd));
            }
            if (state.video || state.audio_path.is_some()) && contains(state.timeline(hwnd), p) {
                let r = state.timeline(hwnd);
                state.seek(
                    (f64::from(p.x - r.left) / f64::from((r.right - r.left).max(1))
                        * state.duration as f64) as i64,
                );
                ui::invalidate(hwnd);
            } else if contains(state.area(hwnd), p) {
                state.drag = Some((p, state.view.pan));
                unsafe {
                    let _ = SetCapture(hwnd);
                }
            }
        }
        WM_MOUSEMOVE => {
            if let Some((start, pan)) = state.drag {
                let p = point(lp);
                state.view.pan = (
                    pan.0 + f64::from(p.x - start.x),
                    pan.1 + f64::from(p.y - start.y),
                );
                ui::invalidate(hwnd);
            }
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED => {
            state.drag = None;
            if message == WM_LBUTTONUP {
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
        }
        WM_LBUTTONDBLCLK => {
            state.view = View::default();
            ui::invalidate(hwnd);
        }
        WM_KEYDOWN => {
            match wp.0 {
                32 => state.toggle(),
                37 => state.seek(state.current_tick() - 5 * 10_000_000),
                39 => state.seek(state.current_tick() + 5 * 10_000_000),
                48 => state.view = View::default(),
                _ => {}
            }
            ui::invalidate(hwnd);
        }
        WM_PRINTCLIENT => {
            paint(state, hwnd, HDC(wp.0 as *mut _));
            return LRESULT(0);
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let screen = unsafe { BeginPaint(hwnd, &raw mut ps) };
            let rect = controls::rect(hwnd);
            unsafe {
                let memory = CreateCompatibleDC(Some(screen));
                let bitmap = CreateCompatibleBitmap(screen, rect.right.max(1), rect.bottom.max(1));
                let old = SelectObject(memory, bitmap.into());
                paint(state, hwnd, memory);
                let _ = BitBlt(
                    screen,
                    0,
                    0,
                    rect.right,
                    rect.bottom,
                    Some(memory),
                    0,
                    0,
                    SRCCOPY,
                );
                SelectObject(memory, old);
                let _ = DeleteObject(bitmap.into());
                let _ = DeleteDC(memory);
                let _ = EndPaint(hwnd, &raw const ps);
            }
            return LRESULT(0);
        }
        WM_ERASEBKGND => return LRESULT(1),
        _ => return unsafe { DefWindowProcW(hwnd, message, wp, lp) },
    }
    LRESULT(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointer_zoom_keeps_same_image_coordinate() {
        let image = Raster {
            width: 1000,
            height: 500,
            pixels: vec![],
        };
        let area = ui::rect(0, 0, 500, 500);
        let mut view = View::default();
        let point = POINT { x: 123, y: 180 };
        let before = view.rect(&image, area);
        let x = f64::from(point.x - before.left) / view.scale(&image, area);
        let y = f64::from(point.y - before.top) / view.scale(&image, area);
        view.zoom_at(&image, area, point, 2.);
        let after = view.rect(&image, area);
        assert!((f64::from(point.x - after.left) / view.scale(&image, area) - x).abs() < 1.1);
        assert!((f64::from(point.y - after.top) / view.scale(&image, area) - y).abs() < 1.1);
    }
}

fn fill(dc: HDC, rect: RECT, color: windows::Win32::Foundation::COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(dc, &raw const rect, brush);
        let _ = DeleteObject(brush.into());
    }
}
#[cfg(test)]
mod worker_tests {
    use super::*;
    use std::time::Duration;
    fn file(extension: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "panzo-preview-{}.{}",
            uuid::Uuid::new_v4(),
            extension
        ))
    }
    fn asset(path: std::path::PathBuf, kind: Kind) -> Asset {
        Asset {
            path,
            kind,
            modified: 0,
            bytes: 0,
            missing: false,
        }
    }
    #[test]
    fn asynchronous_image_and_gif_preview_preserves_dimensions_and_animation() {
        let png = file("png");
        let gif = file("gif");
        image::RgbaImage::from_pixel(40, 20, image::Rgba([12, 34, 56, 255]))
            .save(&png)
            .unwrap();
        let mut encoder = image::codecs::gif::GifEncoder::new(std::fs::File::create(&gif).unwrap());
        for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
            encoder
                .encode_frame(image::Frame::from_parts(
                    image::RgbaImage::from_pixel(16, 12, image::Rgba(color)),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(80, 1),
                ))
                .unwrap();
        }
        drop(encoder);
        let worker = Worker::new();
        worker.generation.store(1, Ordering::Release);
        worker
            .tx
            .send(Request::Load(1, asset(png.clone(), Kind::Image)))
            .unwrap();
        match worker.rx.recv_timeout(Duration::from_secs(3)).unwrap() {
            Event::Image(1, image) => {
                assert_eq!((image.width, image.height), (40, 20));
                assert_eq!(&image.pixels[..4], &[56, 34, 12, 255]);
            }
            _ => panic!("image event"),
        }
        worker.generation.store(2, Ordering::Release);
        worker
            .tx
            .send(Request::Load(2, asset(gif.clone(), Kind::Gif)))
            .unwrap();
        match worker.rx.recv_timeout(Duration::from_secs(3)).unwrap() {
            Event::Gif(2, clip) => {
                assert_eq!(clip.frames.len(), 2);
                let mut player = Player::new(clip);
                assert!(player.playing);
                player.toggle();
                assert!(!player.playing);
            }
            _ => panic!("gif event"),
        }
        drop(worker);
        std::fs::remove_file(png).unwrap();
        std::fs::remove_file(gif).unwrap();
    }
    #[test]
    fn missing_and_corrupt_files_report_an_error_without_panicking() {
        for missing in [true, false] {
            let path = file("png");
            if !missing {
                std::fs::write(&path, b"invalid").unwrap();
            }
            let worker = Worker::new();
            worker.generation.store(1, Ordering::Release);
            worker
                .tx
                .send(Request::Load(1, asset(path.clone(), Kind::Image)))
                .unwrap();
            assert!(matches!(
                worker.rx.recv_timeout(Duration::from_secs(3)).unwrap(),
                Event::Error(1, _)
            ));
            if !missing {
                std::fs::remove_file(path).unwrap();
            }
        }
    }
    #[test]
    #[ignore = "requires PANZO_ASSET_TEST_MP4 pointing to a synthetic local MP4"]
    fn video_poster_duration_and_random_seeks_use_existing_decoder() {
        let source = std::path::PathBuf::from(
            std::env::var_os("PANZO_ASSET_TEST_MP4").expect("synthetic MP4"),
        );
        let worker = Worker::new();
        worker.library_seek_recovery.store(true,Ordering::Release);
        worker.generation.store(1, Ordering::Release);
        worker
            .tx
            .send(Request::Load(1, asset(source, Kind::Video)))
            .unwrap();
        let duration = loop {
            match worker.rx.recv_timeout(Duration::from_secs(30)).unwrap() {
                Event::Video(1, image, duration) => {
                    assert!(image.width > 0 && image.height > 0);
                    if duration > 0 {
                        break duration;
                    }
                }
                Event::Error(_, error) => panic!("{error}"),
                _ => {}
            }
        };
        for tick in [duration / 2, 0, duration - 1] {
            worker.tx.send(Request::Frame(1, tick)).unwrap();
            match worker.rx.recv_timeout(Duration::from_secs(15)).unwrap() {
                Event::Frame(1, image) => assert!(!image.pixels.is_empty()),
                Event::Error(_, error) => panic!("{error}"),
                _ => panic!("frame"),
            }
        }
    }
}
#[cfg(test)]
pub(crate) fn review_loaded(hwnd: HWND) -> bool {
    state(hwnd).is_some_and(|s| s.frame().is_some())
}
pub(crate) fn collage_layer(hwnd: HWND, layer: CollageLayer) {
    if let Some(s) = state(hwnd) {
        s.collage = Some(layer);
        s.status.clear();
        s.error = false;
        controls::show(s.buttons[0], false);
        ui::invalidate(hwnd);
    }
}
pub(crate) fn in_image_area(hwnd: HWND, p: POINT) -> bool {
    state(hwnd).is_some_and(|s| contains(s.area(hwnd), p))
}
fn paint_collage(dc: HDC, viewport: RECT, layer: &CollageLayer, theme: editor_ui::EditorTheme) {
    use super::collage_model::placement;
    let sx = f64::from(viewport.right - viewport.left) / f64::from(layer.geometry.width);
    let sy = f64::from(viewport.bottom - viewport.top) / f64::from(layer.geometry.height);
    let map = |x: f64, y: f64, w: f64, h: f64| RECT {
        left: viewport.left + (x * sx).round() as i32,
        top: viewport.top + (y * sy).round() as i32,
        right: viewport.left + ((x + w) * sx).round() as i32,
        bottom: viewport.top + ((y + h) * sy).round() as i32,
    };
    if layer.live {
        fill(
            dc,
            viewport,
            editor_ui::rgb(
                layer.background[2],
                layer.background[1],
                layer.background[0],
            ),
        );
    }
    for (i, cell) in layer.geometry.cells.iter().enumerate() {
        let (Some(photo), Some(source), Some(size)) = (
            layer.photos.get(i),
            layer.sources.get(i),
            layer.sizes.get(i),
        ) else {
            continue;
        };
        let placed = placement(*size, *cell, layer.fit, photo);
        let cr = map(
            f64::from(cell.x),
            f64::from(cell.y),
            f64::from(cell.width),
            f64::from(cell.height),
        );
        if layer.live {
            unsafe {
                let saved = SaveDC(dc);
                IntersectClipRect(dc, cr.left, cr.top, cr.right, cr.bottom);
                super::capture_tools::draw_raster(
                    dc,
                    source,
                    map(
                        f64::from(cell.x) + placed.x,
                        f64::from(cell.y) + placed.y,
                        placed.width,
                        placed.height,
                    ),
                );
                let _ = RestoreDC(dc, saved);
            }
        }
        if i != layer.selected {
            continue;
        }
        // In a grid, selection identifies the fixed slot, including its padding.
        // It must not suggest that dragging a contained photo moves grid boundaries.
        let r = if layer.geometry.cells.len() >= 4 {
            cr
        } else {
            let x = placed.x.max(0.);
            let y = placed.y.max(0.);
            let width = (placed.x + placed.width).min(f64::from(cell.width)) - x;
            let height = (placed.y + placed.height).min(f64::from(cell.height)) - y;
            map(f64::from(cell.x) + x, f64::from(cell.y) + y, width, height)
        };
        let color = editor_ui::rgb(242, 96, 70);
        let edge = theme.scale(2).max(1);
        for line in [
            ui::rect(r.left, r.top, r.right - r.left, edge),
            ui::rect(r.left, r.bottom - edge, r.right - r.left, edge),
            ui::rect(r.left, r.top, edge, r.bottom - r.top),
            ui::rect(r.right - edge, r.top, edge, r.bottom - r.top),
        ] {
            fill(dc, line, color);
        }
        let badge = ui::rect(
            r.left + theme.scale(8),
            r.top + theme.scale(8),
            theme.scale(26),
            theme.scale(21),
        );
        editor_ui::rounded_surface(dc, badge, color, color, theme.scale(3));
        ui::text(
            dc,
            badge,
            &format!("{:02}", i + 1),
            editor_ui::rgb(255, 255, 255),
            theme.dpi,
            true,
        );
    }
}
/// Coordinates relative to the displayed raster, used by the collage crop interaction.
pub(crate) fn normalized_point(hwnd: HWND, point: POINT) -> Option<(f64, f64)> {
    let state = state(hwnd)?;
    let image = state.frame()?;
    let rect = state.view.rect(image, state.area(hwnd));
    Some((
        f64::from(point.x - rect.left) / f64::from((rect.right - rect.left).max(1)),
        f64::from(point.y - rect.top) / f64::from((rect.bottom - rect.top).max(1)),
    ))
}

/// Update the same composition without resetting the user's zoom and pan.
pub(crate) fn update_image(hwnd: HWND, title: &str, raster: Raster) {
    let view = state(hwnd).map(|s| s.view);
    image(hwnd, title, Ok(raster));
    if let (Some(view), Some(state)) = (view, state(hwnd)) {
        state.view = view;
    }
}

pub(crate) fn canvas_only(hwnd: HWND) {
    if let Some(s) = state(hwnd) {
        s.canvas_only = true;
        s.layout(hwnd);
    }
}
pub(crate) fn fit(hwnd: HWND) {
    if let Some(s) = state(hwnd) {
        s.view = View::default();
        ui::invalidate(hwnd);
    }
}

/// The library supplies the surrounding title, metadata and fixed primary action.
pub(crate) fn library_view(hwnd:HWND,compact:bool){if let Some(s)=state(hwnd){s.library_mode=true;s.worker.library_seek_recovery.store(true,Ordering::Release);s.library_controls=None;s.compact=compact;s.layout(hwnd);}}
pub(crate) fn compact(hwnd: HWND) {if let Some(s)=state(hwnd) {s.compact=true;s.layout(hwnd);}}
pub(crate) fn summary(hwnd:HWND)->String {state(hwnd).map_or(String::new(),|s| {
 if s.error {return s.status.clone();}
 if let Some(image)=s.frame() {if s.audio_path.is_some(){s.status.clone()}else{format!("{} × {} px",image.width,image.height)}}else{s.status.clone()}
})}

pub(crate) fn project_bin(hwnd:HWND){if let Some(s)=state(hwnd){s.bin_mode=true;s.compact=false;s.layout(hwnd);}}
pub(crate) fn video_audio(hwnd:HWND,path:Option<std::path::PathBuf>){if let Some(s)=state(hwnd){s.video_audio=path;}}

pub(crate) fn text_view(hwnd:HWND,title:&str,text:&str){if let Some(s)=state(hwnd){reset(s);s.title=title.into();s.text_content=Some(text.into());s.status.clear();s.controls();ui::invalidate(hwnd);}}

pub(crate) fn title(hwnd:HWND,title:&str){if let Some(s)=state(hwnd){s.title=title.into();ui::invalidate(hwnd);}}

pub(crate) fn pause(hwnd:HWND) { if let Some(s)=state(hwnd) { s.pause(); if let Some(g)=&mut s.gif {g.playing=false;} s.controls(); } }

pub(crate) fn toggle(hwnd:HWND) {if let Some(s)=state(hwnd) {s.toggle();}}

#[cfg(test)]
pub(crate) fn review_playback(hwnd:HWND)->(bool,i64){state(hwnd).map_or((false,0),|s|(s.duration>0,s.current_tick()))}
