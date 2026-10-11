//! Collage is an image workspace with persistent drafts, never a modal processing dialog.
#![allow(
    clippy::wildcard_imports,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
use super::image_icons as P;
use super::{
    collage_model::{self, Fit, Options, Photo, Rendered},
    desktop_ui as desk,
    editor_ui::{self, IconKind},
    workspace_ui as ui,
};
use std::{
    cell::Cell,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::*,
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, ReleaseCapture, SetCapture},
            Shell::{
                DefSubclassProc, DragAcceptFiles, DragFinish, DragQueryFileW, HDROP,
                RemoveWindowSubclass, SetWindowSubclass,
            },
            WindowsAndMessaging::*,
        },
    },
    core::w,
};
thread_local! {static WINDOW:Cell<HWND>=const{Cell::new(HWND(std::ptr::null_mut()))};}
struct Work {
    cancel: Arc<AtomicBool>,
    receiver: Receiver<Result<Rendered, String>>,
}
impl Drop for Work {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
struct State {
    library: std::path::PathBuf,
    photos: Vec<Photo>,
    selected: usize,
    fields: Vec<HWND>,
    groups: Vec<Vec<HWND>>,
    choices: [usize; 4],
    background: [u8; 4],
    live: bool,
    labels: Vec<HWND>,
    buttons: Vec<(usize, HWND)>,
    status: HWND,
    preview: HWND,
    body: HWND,
    pending: Option<Instant>,
    work: Option<Work>,
    rendered: Option<Rendered>,
    syncing: bool,
    scroll: i32,
    dirty: bool,
    crop_drag: Option<(usize, (f64, f64), Photo)>,
    thumb_drag: Option<usize>,
    thumb_scroll: i32,
    separators: Vec<HWND>,
}
fn state(h: HWND) -> Option<&'static mut State> {
    unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State).as_mut() }
}
pub(crate) fn open(library: &Path) -> Result<(), String> {
    let old = WINDOW.with(Cell::get);
    if unsafe { IsWindow(Some(old)) }.as_bool() {
        super::workspace::image_view(Some(old));
        return Ok(());
    }
    let parent = super::workspace::root().ok_or("工作区尚未就绪")?;
    ui::register(w!("PanzoCollage"), Some(procedure))?;
    let h = ui::panel(parent, w!("PanzoCollage"))?;
    let result = (|| {
        ui::register(w!("PanzoCollageProperties"), Some(body_proc))?;
        let body = ui::panel(h, w!("PanzoCollageProperties"))?;
        desk::install(body);
        ui::show(body, true);
        let mut separators = Vec::new();
        for id in 0..3 {
            let line = ui::label(body, "", 700 + id)?;
            separators.push(line);
        }
        let mut fields = Vec::new();
        let mut labels = Vec::new();
        let mut groups = Vec::new();
        for (i, (label, values)) in [
            ("排版布局", &["两图", "三图", "四宫格", "九宫格"][..]),
            ("排列与显示", &["横向", "纵向"][..]),
            ("照片显示", &["自动贴合", "完整显示", "填满裁切"][..]),
            (
                "单格比例",
                &["1:1", "4:3", "3:4", "16:9", "9:16", "自动"][..],
            ),
        ]
        .iter()
        .enumerate()
        {
            labels.push(ui::label(h, label, 200 + i)?);
            let mut group = Vec::new();
            for (j, value) in values.iter().enumerate() {
                let b = ui::button(h, value, 300 + i * 10 + j)?;
                if i == 0 {
                    desk::choice(b, desk::ChoiceVisual::Layout([2, 3, 4, 9][j]));
                }
                group.push(b);
            }
            groups.push(group);
        }
        for (i, (label, value, presets)) in [
            (
                "长边 · px",
                "2048",
                &[
                    "512", "1024", "1376", "1920", "2048", "2560", "3840", "4096", "8192", "16000",
                ][..],
            ),
            (
                "间距 · px",
                "0",
                &[
                    "0", "2", "4", "8", "12", "16", "24", "32", "48", "64", "100",
                ][..],
            ),
        ]
        .iter()
        .enumerate()
        {
            labels.push(ui::label(h, label, 204 + i)?);
            fields.push(ui::number(h, value, presets, 104 + i, None)?);
        }
        labels.push(ui::label(h, "背景", 206)?);
        let background = ui::button(h, "选择背景颜色", 106)?;
        desk::choice(background, desk::ChoiceVisual::Swatch([255; 4]));
        fields.push(background);
        let mut buttons = Vec::new();
        for (id, label, icon) in [
            (1, "添加图片", IconKind::Add),
            (2, "将选中照片前移", IconKind::ArrowLeft),
            (3, "将选中照片后移", IconKind::ArrowRight),
            (4, "替换选中照片", IconKind::Folder),
            (5, "移除选中照片", IconKind::Delete),
            (6, "重置照片裁剪", IconKind::Reset),
            (7, "生成拼图", IconKind::Check),
            (8, "返回图片工具", IconKind::Back),
            (9, "居中并适应窗口", IconKind::Fit),
            (120, "选择", P::SELECT),
            (121, "裁剪", P::CROP),
            (122, "画笔", P::PEN),
            (123, "矩形", P::RECTANGLE),
            (124, "箭头", P::ARROW),
            (125, "文字", P::TEXT),
            (126, "马赛克", P::BLUR),
            (127, "文字 OCR", P::OCR),
            (128, "多模态识图", P::SPARK),
            (129, "拼图", P::COLLAGE),
            (130, "贴图", P::PIN),
        ] {
            let b = ui::button(h, label, id)?;
            desk::style(b, Some(icon), id != 7 && id != 1, id == 7, true);
            buttons.push((id, b));
        }
        let preview = super::asset_preview::create(h)?;
        super::asset_preview::canvas_only(preview);
        ui::show(preview, true);
        unsafe {
            let _ = SetWindowSubclass(preview, Some(preview_proc), 0x50434f4c, h.0 as usize);
        }
        super::asset_preview::empty(preview, "拼图预览");
        Ok::<_, String>(State {
            library: library.into(),
            photos: Vec::new(),
            selected: 0,
            fields,
            groups,
            choices: [2, 0, 0, 0],
            background: [255; 4],
            live: false,
            labels,
            buttons,
            status: ui::label(h, "", 20)?,
            preview,
            body,
            pending: None,
            work: None,
            rendered: None,
            syncing: false,
            scroll: 0,
            dirty: true,
            crop_drag: None,
            thumb_drag: None,
            thumb_scroll: 0,
            separators,
        })
    })();
    let s = match result {
        Ok(s) => s,
        Err(e) => {
            unsafe {
                let _ = DestroyWindow(h);
            }
            return Err(e);
        }
    };
    unsafe {
        SetWindowLongPtrW(h, GWLP_USERDATA, Box::into_raw(Box::new(s)) as isize);
        SetTimer(Some(h), 95, 40, None);
        DragAcceptFiles(h, true);
    }
    WINDOW.with(|v| v.set(h));
    if let Some(s) = state(h) {
        for child in s
            .fields
            .iter()
            .chain(s.labels.iter())
            .chain(s.groups.iter().flatten())
            .copied()
            .chain((2..=6).map(|id| s.button(id)))
        {
            unsafe {
                let _ = SetParent(child, Some(s.body));
            }
            ui::forget(child);
        }
        s.sync_selection();
        s.layout(h);
    }
    super::workspace::image_view(Some(h));
    Ok(())
}
impl State {
    fn options(&self) -> Result<Options, String> {
        let integer = |i: usize, min, max| {
            ui::value(self.fields[i])
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|n| (min..=max).contains(n))
                .ok_or_else(|| {
                    format!(
                        "{}请输入 {min}–{max} 之间的整数",
                        if i == 0 { "长边" } else { "间距" }
                    )
                })
        };
        Ok(Options {
            count: [2, 3, 4, 9][self.choices[0]],
            vertical: self.choices[1] == 1,
            fit: [Fit::Natural, Fit::Contain, Fit::Cover][self.choices[2]],
            ratio: [1., 4. / 3., 3. / 4., 16. / 9., 9. / 16.][self.choices[3]],
            edge: integer(0, 128, 16000)?,
            gap: integer(1, 0, 100)?,
            background: self.background,
        })
    }
    fn queue(&mut self) {
        self.live = false;
        if let Some(work) = &self.work {
            work.cancel.store(true, Ordering::Release);
        }
        self.dirty = true;
        self.pending = Some(Instant::now() + Duration::from_millis(100));
        unsafe {
            let _ = EnableWindow(self.button(7), false);
        }
        ui::text(self.status, "正在更新预览…");
    }
    fn button(&self, id: usize) -> HWND {
        self.buttons.iter().find(|(i, _)| *i == id).unwrap().1
    }
    fn sync_selection(&mut self) {
        self.syncing = true;
        self.selected = self.selected.min(self.photos.len().saturating_sub(1));
        ui::text(
            self.groups[3][5],
            if self.choices[0] >= 2 {
                "自动"
            } else {
                "自动"
            },
        );
        for (i, group) in self.groups.iter().enumerate() {
            for (j, b) in group.iter().enumerate() {
                let selected = if i == 2 && self.choices[2] == 0 {
                    1
                } else if i == 3 && self.choices[0] < 2 && self.choices[2] == 0 {
                    5
                } else {
                    self.choices[i]
                };
                desk::selected(*b, selected == j);
                unsafe {
                    let _ = EnableWindow(*b, i != 1 || self.choices[0] < 2);
                }
            }
        }
        unsafe {
            for id in 2..=6 {
                let _ = EnableWindow(
                    self.button(id),
                    !self.photos.is_empty()
                        && (id != 2 || self.selected > 0)
                        && (id != 3 || self.selected + 1 < self.photos.len()),
                );
            }
            let _ = EnableWindow(
                self.button(7),
                !self.dirty
                    && self.rendered.is_some()
                    && self.options().is_ok_and(|o| self.photos.len() >= o.count),
            );
        }
        unsafe {
            let _ = EnableWindow(self.button(1), self.photos.len() < 9);
        }
        self.syncing = false;
        self.sync_preview();
    }
    fn sync_preview(&self) {
        if self.dirty && !self.live {
            return;
        }
        if let (Some(r), Ok(o)) = (&self.rendered, self.options()) {
            super::asset_preview::collage_layer(
                self.preview,
                super::asset_preview::CollageLayer {
                    geometry: r.geometry.clone(),
                    sources: r.sources.clone(),
                    sizes: r.sizes.clone(),
                    photos: self.photos.clone(),
                    fit: o.fit,
                    background: o.background,
                    selected: self.selected,
                    live: self.live,
                },
            );
        }
    }
    fn gesture_changed(&mut self) {
        if let Some(work) = &self.work {
            work.cancel.store(true, Ordering::Release);
        }
        if !self.live {
            unsafe {
                let _ = EnableWindow(self.button(7), false);
            }
            ui::text(self.status, "");
        }
        self.dirty = true;
        self.live = true;
        self.pending = Some(Instant::now() + Duration::from_millis(180));
        self.sync_preview();
    }
    fn hit_photo(&self, p: POINT) -> Option<(usize, (f64, f64))> {
        if (self.dirty && !self.live) || !super::asset_preview::in_image_area(self.preview, p) {
            return None;
        }
        let (x, y) = super::asset_preview::normalized_point(self.preview, p)?;
        let r = self.rendered.as_ref()?;
        let (gx, gy) = (
            x * f64::from(r.geometry.width),
            y * f64::from(r.geometry.height),
        );
        let i = r.geometry.cells.iter().position(|c| {
            gx >= f64::from(c.x)
                && gx < f64::from(c.x + c.width)
                && gy >= f64::from(c.y)
                && gy < f64::from(c.y + c.height)
        })?;
        (i < self.photos.len()).then_some((i, (x, y)))
    }
    fn side(&self, h: HWND) -> i32 {
        ui::scale(
            h,
            super::workbench_layout::inspector(
                ui::rect(h).right * 96 / editor_ui::window_dpi(h) as i32,
            ),
        )
    }
    fn layout(&mut self, h: HWND) {
        let _batch = ui::layout_batch(h);
        let r = ui::rect(h);
        let d = |v| ui::scale(h, v);
        let side = self.side(h);
        let left = r.right - side;
        self.scroll = self.scroll.clamp(0, (d(502) - (r.bottom - d(100))).max(0));
        let x = d(16);
        let width = side - d(32);
        let y = |n| d(n) - self.scroll;
        ui::place(
            self.preview,
            d(44),
            d(40),
            left - d(44),
            (r.bottom - d(154)).max(1),
        );
        ui::place(self.body, left, d(40), side, (r.bottom - d(100)).max(1));
        desk::scroll_panel(self.body, d(502), (r.bottom - d(100)).max(1), self.scroll);
        ui::place(self.button(8), d(8), d(6), d(28), d(28));
        ui::place(self.button(9), left - d(40), d(6), d(28), d(28));
        for i in 0..11 {
            let b = self.button(120 + i);
            desk::style(
                b,
                Some(
                    [
                        P::SELECT,
                        P::CROP,
                        P::PEN,
                        P::RECTANGLE,
                        P::ARROW,
                        P::TEXT,
                        P::BLUR,
                        P::OCR,
                        P::SPARK,
                        P::COLLAGE,
                        P::PIN,
                    ][i],
                ),
                true,
                false,
                true,
            );
            desk::selected(b, i == 9);
            ui::place(
                b,
                d(5),
                if i == 10 {
                    r.bottom - d(70)
                } else {
                    d(48 + 38 * i as i32 + if i >= 7 { 14 } else { 0 })
                },
                d(32),
                d(32),
            );
        }
        let add = self.thumbnail_rect(h, self.photos.len());
        desk::choice(self.button(1), desk::ChoiceVisual::AddCell);
        ui::show(self.button(1), add.left >= 0 && add.right <= left);
        ui::place(
            self.button(1),
            add.left,
            add.top,
            add.right - add.left,
            add.bottom - add.top,
        );
        ui::place(self.button(7), left + d(16), r.bottom - d(46), width, d(30));
        desk::style(self.button(7), None, false, true, false);
        for (i, line) in self.separators.iter().enumerate() {
            desk::separator(*line);
            ui::place(*line, x, y([112, 252, 336][i]), width, 1);
        }
        let tops = [16, 126, 204, 268];
        for (i, group) in self.groups.iter().enumerate() {
            ui::show(self.labels[i], i != 2);
            if i != 2 {
                ui::place(self.labels[i], x, y(tops[i]), width, d(22));
            }
            let order: Vec<usize> = match i {
                0 => vec![0, 1, 2, 3],
                1 => vec![0, 1],
                2 => vec![2, 1],
                _ => vec![5, 0, 1, 2],
            };
            for (j, b) in group.iter().enumerate() {
                let index = order.iter().position(|v| *v == j);
                ui::show(*b, index.is_some());
                if let Some(index) = index {
                    let gap = d(6);
                    let count = order.len() as i32;
                    let cell = (width - gap * (count - 1)) / count;
                    ui::place(
                        *b,
                        x + index as i32 * (cell + gap),
                        y(if i == 2 { 204 } else { tops[i] + 30 }),
                        cell,
                        d(if i == 0 { 59 } else { 27 }),
                    );
                }
            }
        }
        for i in 0..3 {
            let top = 352 + i as i32 * 38;
            ui::place(self.labels[i + 4], x, y(top), d(110), d(28));
            ui::place(
                self.fields[i],
                x + width - d(if i == 2 { 126 } else { 92 }),
                y(top),
                d(if i == 2 { 126 } else { 92 }),
                d(28),
            );
        }
        for id in 2..=6 {
            ui::show(self.button(id), false);
        }
        ui::place(self.status, left + d(16), r.bottom - d(84), width, d(32));
        desk::invalidate(h);
    }
    fn thumbnail_rect(&self, h: HWND, index: usize) -> RECT {
        let r = ui::rect(h);
        let d = |v| ui::scale(h, v);
        let left = r.right - self.side(h);
        let step = d(83);
        let total = step * (self.photos.len() as i32 + 1) - d(8);
        let available = left - d(24);
        let scroll = self.thumb_scroll.clamp(0, (total - available).max(0));
        desk::rect(
            if total <= available {
                (left - total) / 2
            } else {
                d(12) - scroll
            } + index as i32 * step,
            r.bottom - d(103),
            d(75),
            d(54),
        )
    }

    fn thumb_at(&self, h: HWND, p: POINT) -> Option<usize> {
        (0..self.photos.len()).find(|&i| contains(self.thumbnail_rect(h, i), p))
    }
    fn add(&mut self, h: HWND, paths: Vec<std::path::PathBuf>) {
        for path in paths {
            if self.photos.len() >= 9 {
                break;
            }
            if !self.photos.iter().any(|p| p.path == path) {
                self.photos.push(Photo::new(path));
            }
        }
        self.sync_selection();
        self.queue();
        self.layout(h);
    }
    fn command(&mut self, h: HWND, id: usize) -> Result<(), String> {
        match id {
            120..=130 => {
                if id != 129 {
                    super::workspace::image_view(None);
                    super::image_workspace::picture_command(
                        [30, 36, 3, 1, 2, 4, 5, 46, 67, 68, 11][id - 120],
                    );
                }
            }
            1 => {
                if self.photos.len() >= 9 {
                    return Ok(());
                }
                let paths = super::image_workflows::choose_images(h, true, false)?;
                self.add(h, paths);
            }
            2 | 3 => {
                if !self.photos.is_empty() {
                    let to = if id == 2 {
                        self.selected.saturating_sub(1)
                    } else {
                        (self.selected + 1).min(self.photos.len() - 1)
                    };
                    self.photos.swap(self.selected, to);
                    self.selected = to;
                    self.sync_selection();
                    self.queue();
                }
            }
            4 => {
                if let Some(path) = super::image_workflows::choose_images(h, false, false)?
                    .into_iter()
                    .next()
                {
                    if let Some(p) = self.photos.get_mut(self.selected) {
                        *p = Photo::new(path);
                        self.sync_selection();
                        self.queue();
                    }
                }
            }
            5 => {
                if self.selected < self.photos.len() {
                    self.photos.remove(self.selected);
                    self.sync_selection();
                    self.queue();
                    self.layout(h);
                }
            }
            6 => {
                if let Some(p) = self.photos.get_mut(self.selected) {
                    p.x = 0.5;
                    p.y = 0.5;
                    p.zoom = 1.;
                    self.sync_selection();
                    self.queue();
                }
            }
            7 => {
                let _lease = super::plugins::acquire(super::plugins::IMAGES)?;
                if self.dirty {
                    return Err("请等待当前预览完成".into());
                }
                let o = self.options()?;
                if self.photos.len() < o.count {
                    return Err(format!("还需要添加 {} 张图片", o.count - self.photos.len()));
                }
                let r = self.rendered.as_ref().ok_or("请等待当前预览完成")?;
                super::capture_tools::show_image(r.image.clone(), &self.library, false)?;
            }
            8 => super::workspace::image_view(None),
            9 => super::asset_preview::fit(self.preview),
            106 => {
                desk::open_color(h, self.fields[2], self.background, true)?;
            }
            104 | 105 => {
                self.queue();
            }
            300..=335 => {
                let group = (id - 300) / 10;
                let option = (id - 300) % 10;
                if group < 4
                    && option < self.groups[group].len()
                    && (group != 1 || self.choices[0] < 2)
                {
                    if group == 3 && option == 5 {
                        if self.choices[0] >= 2 {
                            self.choices[3] = 0;
                        } else {
                            self.choices[2] = 0;
                        }
                    } else {
                        self.choices[group] = option;
                        if group == 3 && self.choices[0] < 2 && self.choices[2] == 0 {
                            self.choices[2] = 2;
                        }
                    }
                    self.queue();
                    self.sync_selection();
                }
            }
            _ => {}
        }
        desk::invalidate(h);
        Ok(())
    }
    fn poll(&mut self, h: HWND) {
        if self.crop_drag.is_none()
            && self.work.is_none()
            && self.pending.is_some_and(|t| Instant::now() >= t)
        {
            self.pending = None;
            match self.options() {
                Ok(options) => {
                    let photos = self.photos.clone();
                    let (cancel, worker_cancel) = {
                        let c = Arc::new(AtomicBool::new(false));
                        (c.clone(), c)
                    };
                    let (tx, receiver) = mpsc::channel();
                    match super::plugins::acquire(super::plugins::IMAGES) {
                        Ok(lease) => {
                            std::thread::spawn(move || {
                                let _lease = lease;
                                let _ = tx.send(collage_model::render(
                                    &photos,
                                    options,
                                    &worker_cancel,
                                ));
                            });
                            self.work = Some(Work { cancel, receiver });
                        }
                        Err(e) => ui::text(self.status, &e),
                    }
                }
                Err(e) => {
                    ui::text(self.status, &e);
                    super::asset_preview::notice(self.preview, "参数无效，画面保留上次预览。");
                }
            }
        }
        let cancelled = self
            .work
            .as_ref()
            .is_some_and(|w| w.cancel.load(Ordering::Acquire));
        let ready = self.work.as_ref().and_then(|w| w.receiver.try_recv().ok());
        if let Some(result) = ready {
            self.work = None;
            if cancelled {
                return;
            }
            match result {
                Ok(r) => {
                    let o = self.options().unwrap();
                    let title = format!("拼图预览 · {} × {} px", r.image.width, r.image.height);
                    super::asset_preview::update_image(self.preview, &title, r.preview.clone());
                    ui::text(
                        self.status,
                        &if self.photos.len() < o.count {
                            format!(
                                "还需要 {} 张图片。添加后可生成。",
                                o.count - self.photos.len()
                            )
                        } else if self.photos.len() > o.count {
                            format!("前 {} 张参与拼图，其余照片保留。", o.count)
                        } else {
                            String::new()
                        },
                    );
                    self.rendered = Some(r);
                    self.dirty = false;
                    self.live = false;
                    self.sync_selection();
                    desk::invalidate(h);
                }
                Err(e) => {
                    ui::text(self.status, &e);
                    super::asset_preview::notice(self.preview, &e);
                }
            }
        }
    }
}
fn point(l: LPARAM) -> POINT {
    POINT {
        x: i32::from(l.0 as u16 as i16),
        y: i32::from((l.0 >> 16) as u16 as i16),
    }
}
fn contains(r: RECT, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Some(s) = state(h) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    match m {
        WM_SIZE => s.layout(h),
        WM_ERASEBKGND => return LRESULT(1),
        WM_TIMER => s.poll(h),
        desk::COLOR_PREVIEW => {
            s.background = (w.0 as u32).to_le_bytes();
            desk::choice(s.fields[2], desk::ChoiceVisual::Swatch(s.background));
            s.queue();
        }
        WM_COMMAND => {
            let id = w.0 & 0xffff;
            let n = w.0 >> 16;
            if !s.syncing
                && (n == BN_CLICKED as usize
                    || n == CBN_SELCHANGE as usize
                    || n == CBN_EDITCHANGE as usize)
            {
                if let Err(e) = s.command(h, id) {
                    if let Some(work) = &s.work {
                        work.cancel.store(true, Ordering::Release);
                    }
                    s.pending = None;
                    s.dirty = true;
                    ui::text(s.status, &e);
                    unsafe {
                        let _ = EnableWindow(s.button(7), false);
                    }
                }
            }
        }
        WM_MOUSEWHEEL => {
            let mut point = point(l);
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(h, &mut point);
            }
            let delta = i32::from((w.0 >> 16) as u16 as i16) / 120;
            let r = ui::rect(h);
            if point.y >= r.bottom - ui::scale(h, 114) && point.x < r.right - s.side(h) {
                let total = ui::scale(h, 83) * (s.photos.len() as i32 + 1) - ui::scale(h, 8);
                let available = r.right - s.side(h) - ui::scale(h, 24);
                s.thumb_scroll = (s.thumb_scroll - delta * ui::scale(h, 83))
                    .clamp(0, (total - available).max(0));
            } else {
                s.scroll -= delta * ui::scale(h, 48);
            }
            s.layout(h);
        }
        WM_RBUTTONUP => {
            if let Some(index) = s.thumb_at(h, point(l)) {
                s.selected = index;
                s.sync_selection();
                let rows = [
                    super::app_menu::Row::command(4, "替换照片", IconKind::Folder, true),
                    super::app_menu::Row::command(5, "移除照片", IconKind::Delete, true),
                    super::app_menu::Row::command(6, "重置照片位置", IconKind::Restore, true),
                ];
                if let Ok(Some(id)) = super::app_menu::show_rows(
                    h,
                    desk::rect(point(l).x, point(l).y, 1, 1),
                    editor_ui::EditorTheme::current(editor_ui::window_dpi(h)),
                    rows.to_vec(),
                    false,
                ) {
                    let _ = s.command(h, id as usize);
                }
            }
        }
        WM_DROPFILES => {
            let drop = HDROP(w.0 as *mut _);
            let mut paths = Vec::new();
            unsafe {
                let n = DragQueryFileW(drop, u32::MAX, None);
                for i in 0..n {
                    let len = DragQueryFileW(drop, i, None);
                    let mut value = vec![0; len as usize + 1];
                    DragQueryFileW(drop, i, Some(&mut value));
                    paths.push(std::path::PathBuf::from(String::from_utf16_lossy(
                        &value[..len as usize],
                    )));
                }
                DragFinish(drop);
            }
            s.add(h, paths);
        }
        WM_LBUTTONDOWN => {
            if let Some(i) = s.thumb_at(h, point(l)) {
                s.selected = i;
                s.thumb_drag = Some(i);
                s.sync_selection();
                desk::invalidate(h);
                unsafe {
                    SetCapture(h);
                }
            }
        }
        WM_LBUTTONUP => {
            if let Some(from) = s.thumb_drag.take() {
                unsafe {
                    let _ = ReleaseCapture();
                }
                if let Some(to) = s.thumb_at(h, point(l)) {
                    if from != to {
                        let p = s.photos.remove(from);
                        s.photos.insert(to, p);
                        s.selected = to;
                        s.sync_selection();
                        s.queue();
                    }
                }
            }
        }
        WM_CAPTURECHANGED => s.thumb_drag = None,
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &mut ps) };
            let r = ui::rect(h);
            let buffer = desk::BufferedFrame::new(dc, r);
            let dc = buffer.dc;
            let t = editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
            fill(dc, r, t.palette.panel);
            desk::line(
                dc,
                desk::rect(
                    ui::scale(h, 44) - 1,
                    ui::scale(h, 40),
                    1,
                    r.bottom - ui::scale(h, 40),
                ),
                t,
            );
            desk::line(
                dc,
                desk::rect(ui::scale(h, 10), ui::scale(h, 316), ui::scale(h, 24), 1),
                t,
            );
            let left = r.right - s.side(h);
            desk::line(dc, desk::rect(0, ui::scale(h, 39), r.right, 1), t);
            desk::line(
                dc,
                desk::rect(left, r.bottom - ui::scale(h, 60), r.right - left, 1),
                t,
            );
            desk::text(
                dc,
                desk::rect(
                    ui::scale(h, 48),
                    ui::scale(h, 6),
                    ui::scale(h, 160),
                    ui::scale(h, 28),
                ),
                "拼图",
                t.palette.text_secondary,
                t.dpi,
                false,
            );
            desk::text(
                dc,
                desk::rect(
                    left + ui::scale(h, 16),
                    ui::scale(h, 6),
                    s.side(h) - ui::scale(h, 32),
                    ui::scale(h, 28),
                ),
                "拼图与排版",
                t.palette.text_secondary,
                t.dpi,
                false,
            );
            fill(dc, desk::rect(left, 0, 1, r.bottom), t.palette.divider);
            desk::line(
                dc,
                desk::rect(
                    ui::scale(h, 44),
                    r.bottom - ui::scale(h, 114),
                    left - ui::scale(h, 44),
                    1,
                ),
                t,
            );
            desk::line(dc, desk::rect(0, r.bottom - ui::scale(h, 38), left, 1), t);
            let saved = unsafe { windows::Win32::Graphics::Gdi::SaveDC(dc) };
            unsafe {
                windows::Win32::Graphics::Gdi::IntersectClipRect(
                    dc,
                    0,
                    r.bottom - ui::scale(h, 114),
                    left,
                    r.bottom - ui::scale(h, 38),
                );
            }
            for (i, p) in s.photos.iter().enumerate() {
                let rect = s.thumbnail_rect(h, i);
                fill(
                    dc,
                    rect,
                    if i == s.selected {
                        t.palette.hover
                    } else {
                        t.palette.raised
                    },
                );
                if let Some(image) = s.rendered.as_ref().and_then(|r| r.thumbnails.get(i)) {
                    let inset = 3;
                    let scale = (f64::from(rect.right - rect.left - inset * 2)
                        / f64::from(image.width))
                    .min(f64::from(rect.bottom - rect.top - inset * 2) / f64::from(image.height));
                    let width = (f64::from(image.width) * scale) as i32;
                    let height = (f64::from(image.height) * scale) as i32;
                    super::capture_tools::draw_raster(
                        dc,
                        image,
                        desk::rect(
                            rect.left + (rect.right - rect.left - width) / 2,
                            rect.top + (rect.bottom - rect.top - height) / 2,
                            width,
                            height,
                        ),
                    );
                }
                if i == s.selected {
                    editor_ui::selection_outline(dc, rect, t.palette.accent);
                }
                let _ = p;
            }
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::RestoreDC(dc, saved);
            }
            desk::text_style(
                dc,
                desk::rect(
                    ui::scale(h, 16),
                    r.bottom - ui::scale(h, 38),
                    left - ui::scale(h, 32),
                    ui::scale(h, 38),
                ),
                &format!("拼图 · {} 张图片", s.photos.len()),
                t.palette.text_muted,
                t.dpi,
                false,
                editor_ui::TextStyle::Caption,
            );
            drop(buffer);
            unsafe {
                let _ = EndPaint(h, &ps);
            }
        }
        WM_NCDESTROY => {
            WINDOW.with(|v| {
                if v.get() == h {
                    v.set(HWND::default());
                }
            });
            unsafe {
                KillTimer(Some(h), 95).ok();
                SetWindowLongPtrW(h, GWLP_USERDATA, 0);
                drop(Box::from_raw(s as *mut State));
            }
            return unsafe { DefWindowProcW(h, m, w, l) };
        }
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}
unsafe extern "system" fn preview_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    let parent = HWND(data as *mut _);
    if let Some(s) = state(parent) {
        match m {
            WM_LBUTTONDOWN if w.0 & 8 == 0 => {
                if let Some((i, start)) = s.hit_photo(point(l)) {
                    s.selected = i;
                    s.crop_drag = Some((i, start, s.photos[i].clone()));
                    unsafe {
                        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(h));
                        SetCapture(h);
                    }
                    s.sync_selection();
                    desk::invalidate(parent);
                    return LRESULT(0);
                }
            }
            WM_MOUSEMOVE => {
                if let Some((i, (start_x, start_y), origin)) = s.crop_drag.clone() {
                    if let (Some((x, y)), Some(r), Ok(o)) = (
                        super::asset_preview::normalized_point(h, point(l)),
                        &s.rendered,
                        s.options(),
                    ) {
                        let delta = (
                            (x - start_x) * f64::from(r.geometry.width),
                            (y - start_y) * f64::from(r.geometry.height),
                        );
                        if let (Some(photo), Some(size), Some(cell)) =
                            (s.photos.get_mut(i), r.sizes.get(i), r.geometry.cells.get(i))
                        {
                            collage_model::pan(photo, &origin, *size, *cell, o.fit, delta);
                            s.gesture_changed();
                        }
                        return LRESULT(0);
                    }
                }
            }
            WM_MOUSEWHEEL if w.0 & 8 == 0 => {
                let mut p = point(l);
                unsafe {
                    let _ = ScreenToClient(h, &mut p);
                }
                if let Some((i, (x, y))) = s.hit_photo(p) {
                    s.selected = i;
                    if let (Some(r), Ok(o)) = (&s.rendered, s.options()) {
                        let cell = r.geometry.cells[i];
                        let anchor = (
                            x * f64::from(r.geometry.width) - f64::from(cell.x),
                            y * f64::from(r.geometry.height) - f64::from(cell.y),
                        );
                        let factor = 1.12_f64.powf(f64::from((w.0 >> 16) as u16 as i16) / 120.);
                        collage_model::zoom_at(
                            &mut s.photos[i],
                            r.sizes[i],
                            cell,
                            o.fit,
                            factor,
                            anchor,
                        );
                        s.gesture_changed();
                        s.sync_selection();
                        desk::invalidate(parent);
                    }
                    return LRESULT(0);
                }
            }
            WM_LBUTTONDBLCLK => {
                if let Some((i, _)) = s.hit_photo(point(l)) {
                    s.selected = i;
                    let photo = &mut s.photos[i];
                    photo.x = 0.5;
                    photo.y = 0.5;
                    photo.zoom = 1.;
                    s.gesture_changed();
                    s.sync_selection();
                    return LRESULT(0);
                }
            }
            WM_LBUTTONUP => {
                if s.crop_drag.take().is_some() {
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    return LRESULT(0);
                }
            }
            WM_CAPTURECHANGED => s.crop_drag = None,
            WM_NCDESTROY => unsafe {
                let _ = RemoveWindowSubclass(h, Some(preview_proc), id);
            },
            _ => {}
        }
    }
    unsafe { DefSubclassProc(h, m, w, l) }
}

fn fill(dc: HDC, r: RECT, c: windows::Win32::Foundation::COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(c);
        FillRect(dc, &r, brush);
        let _ = DeleteObject(brush.into());
    }
}

unsafe extern "system" fn body_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if m == WM_VSCROLL {
        if let Ok(parent) = unsafe { GetParent(h) } {
            if let Some(s) = state(parent) {
                s.scroll = desk::panel_scroll_info(h).2;
                s.layout(parent);
                return LRESULT(0);
            }
        }
    }
    if matches!(m, WM_COMMAND | WM_MOUSEWHEEL) {
        if let Ok(parent) = unsafe { GetParent(h) } {
            return unsafe { SendMessageW(parent, m, Some(w), Some(l)) };
        }
    }
    unsafe { DefWindowProcW(h, m, w, l) }
}
pub(crate) fn export_active() -> bool {
    let h = WINDOW.with(Cell::get);
    if unsafe { IsWindowVisible(h) }.as_bool() {
        if let Some(s) = state(h) {
            if let Err(e) = s.command(h, 7) {
                ui::text(s.status, &e);
            }
            return true;
        }
    }
    false
}

#[cfg(test)]
pub(crate) fn review_add(paths: &[std::path::PathBuf]) {
    let h = WINDOW.with(Cell::get);
    if let Some(s) = state(h) {
        s.add(h, paths.to_vec());
        s.layout(h);
    }
}

#[cfg(test)]
pub(crate) fn review_ready() -> bool {
    state(WINDOW.with(Cell::get)).is_some_and(|s| !s.dirty && s.rendered.is_some())
}
