// Native message routing imports Win32 constants; short coordinates match GDI naming.
#![allow(clippy::wildcard_imports, clippy::many_single_char_names)]
//! Searchable assets with independent read-only preview and retained list state.
use super::{
    CaptureOutcome, HWND, OUTCOME, Path, PathBuf, Raster, WM_CAPTURE_COMPLETE, copy_image,
    pin_manager, show_image,
};
use crate::platform::{
    asset_index::{self, Asset, Kind},
    asset_preview, desktop_ui as ui,
    pane_splitter::{Axis, Drag as PaneDrag, Side, Splitter},
    editor_ui::{self, IconKind},
    workspace_ui as controls,
};
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::{self, Receiver, Sender},
    time::Instant,
};
use windows::{
    Win32::{
        Foundation::{LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, SetFocus},
            WindowsAndMessaging::*,
        },
    },
    core::w,
};
#[path = "library_information.rs"]
mod library_information;
#[path = "library_interaction.rs"]
mod library_interaction;
#[path = "library_preview.rs"]
mod library_preview;
#[path = "library_popup_frame.rs"]
mod library_popup_frame;
thread_local! {static WINDOWS:std::cell::RefCell<Vec<HWND>>=const{std::cell::RefCell::new(Vec::new())};}
pub(super) fn close_all() {
    for hwnd in WINDOWS.with(|v| v.borrow().clone()) {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}
pub(super) fn dialog_message(message: &MSG) -> bool {
    if library_preview::dialog_message(message) || library_information::dialog_message(message) {
        return true;
    }
    if message.message != WM_KEYDOWN {
        return false;
    }
    for hwnd in WINDOWS.with(|v| v.borrow().clone()) {
        if message.hwnd != hwnd && !unsafe { IsChild(hwnd, message.hwnd) }.as_bool() {
            continue;
        }
        if message.wParam.0 == 9 {
            return unsafe { IsDialogMessageW(hwnd, message) }.as_bool();
        }
        let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut History;
        if pointer.is_null() {
            continue;
        }
        let state = unsafe { &mut *pointer };
        if message.wParam.0 == 27 && state.splitter_drag.is_some() {
            state.finish_inspector_drag(hwnd, true);
            return true;
        }
        if message.hwnd == state.list {
            if message.wParam.0 == 32 {
                state.run(hwnd, 20);
                return true;
            }
            if message.wParam.0 == 13 {
                state.run(hwnd, 20);
                return true;
            }
        }
        if message.wParam.0 == 27 && state.quick {
            state.quick = false;
            state.layout(hwnd);
            unsafe {
                let _ = SetFocus(Some(state.list));
            }
            return true;
        }
        if message.wParam.0 == 27 && !crate::platform::workspace::active() {
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            return true;
        }
    }
    false
}
#[derive(Clone)]
enum Entry {
    File(Asset),
    Pin(String, bool),
}
impl Entry {
    fn key(&self) -> String {
        match self {
            Self::File(a) => a.path.to_string_lossy().to_lowercase(),
            Self::Pin(id, _) => format!("pin:{id}"),
        }
    }
    fn preview_key(&self) -> String {
        match self {
            Self::File(asset) => format!(
                "{}:{}:{}:{}",
                self.key(),
                asset.modified,
                asset.bytes,
                asset.missing
            ),
            Self::Pin(id, active) => format!("pin:{id}:{active}"),
        }
    }
    fn name(&self) -> String {
        match self {
            Self::File(a) => a
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            Self::Pin(id, _) => format!("贴图 {}", id.chars().take(8).collect::<String>()),
        }
    }
    fn category(&self) -> usize {
        match self {
            Self::File(a) => {
                if a.kind.is_image() {
                    1
                } else if a.kind == Kind::Audio {
                    3
                } else if a.kind == Kind::Project {
                    4
                } else {
                    2
                }
            }
            Self::Pin(..) => 5,
        }
    }
    fn label(&self) -> &str {
        match self {
            Self::File(a) => {
                if a.missing {
                    "文件不存在"
                } else {
                    a.kind.label()
                }
            }
            Self::Pin(_, active) => {
                if *active {
                    "桌面贴图"
                } else {
                    "最近关闭的贴图"
                }
            }
        }
    }
}
#[derive(Clone, PartialEq, Eq, Hash)]
struct ThumbKey {
    path: PathBuf,
    modified: u128,
    bytes: u64,
}
impl From<&Asset> for ThumbKey {
    fn from(a: &Asset) -> Self {
        Self {
            path: a.path.clone(),
            modified: a.modified,
            bytes: a.bytes,
        }
    }
}
struct Thumb {
    image: Option<Raster>,
    touched: Instant,
    error: Option<String>,
}
struct History {
    owner: HWND,
    library: PathBuf,
    search: HWND,
    directory: HWND,
    location: HWND,
    preview_summary: String,
    inspector_body: HWND,
    splitter: HWND,
    expand: HWND,
    inspector_width: Option<i32>,
    inspector_scroll: i32,
    splitter_drag: Option<(PaneDrag, Option<i32>)>,
    splitter_hover: bool,
    scroll_drag: Option<(HWND, i32)>,
    wheel_remainder: i32,
    reflow: Option<library_interaction::Reflow>,
    item_zoom: i32,
    popup: Option<HWND>,
    grid: HWND,
    grid_mode: bool,
    grid_scroll: i32,
    grid_scroll_by_filter: HashMap<usize, i32>,
    view_buttons: [HWND; 2],
    list: HWND,
    preview: HWND,
    entries: Vec<Entry>,
    visible: Vec<Entry>,
    buttons: Vec<HWND>,
    categories: Vec<HWND>,
    filter_kind: usize,
    dpi: u32,
    thumbs: HashMap<ThumbKey, Thumb>,
    pending: HashSet<ThumbKey>,
    thumb_tx: Sender<Asset>,
    thumb_rx: Receiver<(ThumbKey, Result<Raster, String>)>,
    index_rx: Option<Receiver<Vec<Asset>>>,
    preview_key: Option<String>,
    selection_by_filter: HashMap<usize, (String, usize)>,
    quick: bool,
}
pub(super) fn show(owner: HWND, library: &Path) -> Result<(), String> {
    if let Some(hwnd) = WINDOWS.with(|v| v.borrow().first().copied()) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
        let state = unsafe { &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut History) };
        state.reload();
        return Ok(());
    }
    create(owner, library, None).map(|_| ())
}
pub(super) fn create_embedded(parent: HWND, owner: HWND, library: &Path) -> Result<HWND, String> {
    create(owner, library, Some(parent))
}
pub(super) fn thumbnail(asset: &Asset) -> Result<Raster, String> {
    let image = if asset.kind == Kind::Audio {
        crate::platform::asset_audio::waveform(&asset.path, || false)?.0
    } else if asset.kind.is_image() {
        Raster::load(&asset.path)?
    } else {
        let source = asset_index::video_source(asset)?;
        let mut decoder = crate::platform::media_decoder::MfBgraDecoder::open_with_preview_limit(
            source,
            Some(160),
        )
        .map_err(|e| e.to_string())?;
        let f = decoder
            .read_next()
            .map_err(|e| e.to_string())?
            .ok_or("没有视频画面")?;
        Raster {
            width: f.width,
            height: f.height,
            pixels: f.pixels.as_ref().clone(),
        }
    };
    let raw =
        image::RgbaImage::from_raw(image.width, image.height, image.pixels).ok_or("无效图像")?;
    let small = image::DynamicImage::ImageRgba8(raw)
        .thumbnail(240, 150)
        .to_rgba8();
    Ok(Raster {
        width: small.width(),
        height: small.height(),
        pixels: small.into_raw(),
    })
}
fn create(owner: HWND, library: &Path, parent: Option<HWND>) -> Result<HWND, String> {
    let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    unsafe {
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(history_proc),
            hInstance: module.into(),
            lpszClassName: w!("PanzoCaptureHistory"),
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
            ..Default::default()
        });
    }
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            w!("PanzoCaptureHistory"),
            w!("Panzo · 素材"),
            if parent.is_some() {
                WS_CHILD | WS_CLIPCHILDREN
            } else {
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN
            },
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1060,
            680,
            parent,
            None,
            Some(module.into()),
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    ui::install(hwnd);
    unsafe {
        let _ = windows::Win32::UI::Shell::SetWindowSubclass(
            hwnd,
            Some(library_surface),
            0x41535354,
            0,
        );
    }
    let search = controls::edit(hwnd, "", 1, false)?;
    let cue = super::wide("搜索素材");
    unsafe {
        SendMessageW(
            search,
            windows::Win32::UI::Controls::EM_SETCUEBANNER,
            Some(WPARAM(1)),
            Some(LPARAM(cue.as_ptr() as isize)),
        );
    }
    // Never opt into the native scrollbar: clearing WS_VSCROLL after creation
    // leaves LISTBOX's internal scrolling policy enabled. LB_SETTOPINDEX can
    // revive its non-client scrollbar and flash white before our buffered paint.
    let list = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("LISTBOX"),
            w!("素材列表"),
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WINDOW_STYLE(
                    (LBS_NOTIFY | LBS_NOINTEGRALHEIGHT | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS)
                        as u32,
                ),
            0,
            0,
            1,
            1,
            Some(hwnd),
            Some(HMENU(2usize as *mut _)),
            Some(module.into()),
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    ui::list(list);
    controls::register(w!("PanzoAssetGrid"), Some(grid_proc))?;
    let grid = controls::panel(hwnd, w!("PanzoAssetGrid"))?;
    unsafe {
        SetClassLongPtrW(
            grid,
            GCL_STYLE,
            (GetClassLongPtrW(grid, GCL_STYLE) | CS_DBLCLKS.0 as usize) as isize,
        );
    }
    unsafe {
        SetWindowLongPtrW(
            grid,
            GWL_STYLE,
            GetWindowLongPtrW(grid, GWL_STYLE) | WS_TABSTOP.0 as isize,
        );
    }
    controls::text(grid, "素材网格");
    let directory = crate::platform::path_field::create(hwnd, &library.to_string_lossy(), 32)?;
    // Buffer the path field and its nested label together during live resizing.
    unsafe {
        SetWindowLongPtrW(
            directory,
            GWL_EXSTYLE,
            GetWindowLongPtrW(directory, GWL_EXSTYLE) | WS_EX_COMPOSITED.0 as isize,
        );
    }
    let location = controls::button(hwnd, "本地素材", 33)?;
    ui::style(location, Some(IconKind::Folder), true, false, false);
    let view_buttons = [
        controls::button(hwnd, "网格视图", 30)?,
        controls::button(hwnd, "列表视图", 31)?,
    ];
    for (b, i) in view_buttons.iter().zip([IconKind::Collage, IconKind::Log]) {
        ui::style(*b, Some(i), true, false, true);
    }
    controls::register(w!("PanzoLibraryInspector"), Some(inspector_proc))?;
    let inspector_body = controls::panel(hwnd, w!("PanzoLibraryInspector"))?;
    controls::show(inspector_body, true);
    ui::install(inspector_body);
    controls::register(w!("PanzoLibrarySplitter"), Some(splitter_proc))?;
    let splitter = controls::panel(hwnd, w!("PanzoLibrarySplitter"))?;
    controls::show(splitter, true);
    unsafe {
        let _ = SetWindowPos(
            splitter,
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
    let preview = asset_preview::create(inspector_body)?;
    asset_preview::library_view(preview, true);
    let expand = controls::button(preview, "放大预览", 90)?;
    ui::style(
        expand,
        Some(crate::platform::image_icons::FIT),
        true,
        false,
        true,
    );
    unsafe {
        let _ = windows::Win32::UI::Shell::SetWindowSubclass(
            preview,
            Some(preview_overlay),
            0x4c505256,
            hwnd.0 as usize,
        );
    }
    unsafe {
        let _ = windows::Win32::UI::Shell::SetWindowSubclass(
            list,
            Some(list_surface),
            0x4c4c4953,
            hwnd.0 as usize,
        );
    }
    let mut buttons = Vec::new();
    for (id, label, icon) in [
        (3, "进入编辑", IconKind::Edit),
        (4, "复制图片", IconKind::Copy),
        (5, "贴到桌面", IconKind::Pin),
        (6, "在资源管理器中显示", IconKind::Folder),
        (7, "恢复全部贴图交互", IconKind::Restore),
        (8, "刷新素材", IconKind::Reset),
        (9, "清理已关闭贴图的恢复记录", IconKind::Delete),
        (10, "贴图分组", IconKind::Menu),
        (15, "弹窗预览（空格）", crate::platform::image_icons::FIT),
    ] {
        let b = controls::button(hwnd, label, id)?;
        ui::style(b, Some(icon), true, false, true);
        buttons.push(b);
    }
    for i in 1..=3 {
        unsafe {
            let _ = SetParent(buttons[i], Some(inspector_body));
        }
    }
    let categories = ["全部素材", "图片与截图", "视频", "声音", "项目", "桌面贴图"]
        .iter()
        .enumerate()
        .map(|(i, s)| controls::button(hwnd, s, 40 + i))
        .collect::<Result<Vec<_>, _>>()?;
    let (tx, input) = mpsc::channel::<Asset>();
    let (output, rx) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(asset) = input.recv() {
            let key = ThumbKey::from(&asset);
            let result = thumbnail(&asset);
            if output.send((key, result)).is_err() {
                break;
            }
        }
    });
    let mut state = Box::new(History {
        owner,
        library: library.into(),
        search,
        directory,
        location,
        preview_summary: String::new(),
        inspector_body,
        splitter,
        expand,
        inspector_width: None,
        inspector_scroll: 0,
        splitter_drag: None,
        splitter_hover: false,
        scroll_drag: None,
        wheel_remainder: 0,
        reflow: None,
        item_zoom: 100,
        popup: None,
        grid,
        grid_mode: true,
        grid_scroll: 0,
        grid_scroll_by_filter: HashMap::new(),
        view_buttons,
        list,
        preview,
        entries: Vec::new(),
        visible: Vec::new(),
        buttons,
        categories,
        filter_kind: 0,
        dpi: editor_ui::window_dpi(hwnd),
        thumbs: HashMap::new(),
        pending: HashSet::new(),
        thumb_tx: tx,
        thumb_rx: rx,
        index_rx: None,
        preview_key: None,
        selection_by_filter: HashMap::new(),
        quick: false,
    });
    state.reload();
    state.layout(hwnd);
    WINDOWS.with(|v| v.borrow_mut().push(hwnd));
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
        let _ = ShowWindow(hwnd, SW_SHOW);
        SetTimer(Some(hwnd), 81, 80, None);
    }
    Ok(hwnd)
}
impl History {
    fn selected(&self) -> Option<Entry> {
        let i = unsafe { SendMessageW(self.list, LB_GETCURSEL, None, None) }.0;
        usize::try_from(i)
            .ok()
            .and_then(|i| self.visible.get(i))
            .cloned()
    }
    fn remember_selection(&mut self) {
        self.grid_scroll_by_filter
            .insert(self.filter_kind, self.grid_scroll);
        if let Some(entry) = self.selected() {
            let top = unsafe { SendMessageW(self.list, LB_GETTOPINDEX, None, None) }
                .0
                .max(0) as usize;
            self.selection_by_filter
                .insert(self.filter_kind, (entry.key(), top));
        }
    }
    fn reload(&mut self) {
        if self.index_rx.is_some() {
            return;
        }
        let library = self.library.clone();
        let (tx, rx) = mpsc::channel();
        self.index_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(asset_index::scan(&library));
        });
    }
    fn apply_index(&mut self, assets: Vec<Asset>) {
        self.remember_selection();
        self.entries = assets.into_iter().map(Entry::File).collect();
        for (id, active) in pin_manager::entries() {
            self.entries.push(Entry::Pin(id, active));
        }
        self.filter(false);
    }
    fn filter(&mut self, remember: bool) {
        self.stop_reflow();
        if remember {
            self.remember_selection();
        }
        let mut text = [0u16; 512];
        let len = unsafe { GetWindowTextW(self.search, &mut text) }.max(0) as usize;
        let query = String::from_utf16_lossy(&text[..len]).to_lowercase();
        let saved = self.selection_by_filter.get(&self.filter_kind).cloned();
        self.visible = self
            .entries
            .iter()
            .filter(|e| {
                (self.filter_kind == 0 || self.filter_kind == e.category())
                    && (e.name().to_lowercase().contains(&query) || e.key().contains(&query))
            })
            .cloned()
            .collect();
        unsafe {
            SendMessageW(self.list, WM_SETREDRAW, Some(WPARAM(0)), None);
            SendMessageW(self.list, LB_RESETCONTENT, None, None);
        }
        for entry in &self.visible {
            let label = super::wide(&entry.name());
            unsafe {
                SendMessageW(
                    self.list,
                    LB_ADDSTRING,
                    None,
                    Some(LPARAM(label.as_ptr() as isize)),
                );
            }
        }
        if !self.visible.is_empty() {
            let selected = saved
                .as_ref()
                .and_then(|(key, _)| self.visible.iter().position(|e| e.key() == *key))
                .unwrap_or(0);
            unsafe {
                SendMessageW(self.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
                if let Some((_, top)) = saved {
                    SendMessageW(
                        self.list,
                        LB_SETTOPINDEX,
                        Some(WPARAM(top.min(self.visible.len() - 1))),
                        None,
                    );
                }
            }
        }
        unsafe {
            SendMessageW(self.list, WM_SETREDRAW, Some(WPARAM(1)), None);
        }
        self.grid_scroll = self
            .grid_scroll_by_filter
            .get(&self.filter_kind)
            .copied()
            .unwrap_or(0)
            .clamp(0, self.grid_max());
        controls::show(
            self.list,
            !self.grid_mode && !self.quick && !self.visible.is_empty(),
        );
        controls::show(
            self.grid,
            self.grid_mode && !self.quick && !self.visible.is_empty(),
        );
        ui::invalidate(self.grid);
        ui::invalidate(self.list);
        for b in &self.categories {
            ui::invalidate(*b);
        }
        self.selection();
    }
    fn selection(&mut self) {
        let selected = self.selected();
        let key = selected.as_ref().map(Entry::preview_key);
        if key != self.preview_key {
            match &selected {
                Some(Entry::File(asset)) => asset_preview::load(self.preview, asset.clone()),
                Some(Entry::Pin(id, _)) => {
                    asset_preview::image(self.preview, "桌面贴图", pin_manager::preview_image(id));
                }
                None => asset_preview::clear(self.preview),
            }
            self.preview_key = key;
            self.inspector_scroll = 0;
            if let Ok(parent) = unsafe { GetParent(self.list) } {
                self.layout_inspector(parent);
            }
        }
        controls::text(
            self.buttons[0],
            match selected.as_ref() {
                Some(Entry::File(a)) if a.missing => "重新定位素材",
                Some(Entry::File(a)) if a.kind.is_image() => "在图片工具中打开",
                Some(Entry::File(a)) if a.kind == Kind::Audio => "添加到当前项目",
                Some(Entry::File(a)) if a.kind == Kind::Project => "继续编辑项目",
                Some(Entry::File(_)) => "在视频工作台中打开",
                Some(Entry::Pin(..)) => "显示桌面贴图",
                None => "打开素材",
            },
        );
        ui::invalidate(self.grid);
        ui::invalidate(self.inspector_body);
        let exists = selected.as_ref().is_some_and(|e| match e {
            Entry::File(_) => true,
            Entry::Pin(..) => true,
        });
        let image = selected
            .as_ref()
            .is_some_and(|e| matches!(e,Entry::File(a)if a.kind.is_image()&&!a.missing));
        let file = matches!(selected, Some(Entry::File(_)));
        for (i, enabled) in [(0, exists), (1, image), (2, image), (3, file)] {
            unsafe {
                let _ = EnableWindow(self.buttons[i], enabled);
            }
        }
    }
    fn request_thumbs(&mut self) {
        let top = if self.grid_mode {
            (self.grid_scroll / self.grid_row()).max(0) as usize * self.grid_columns()
        } else {
            unsafe { SendMessageW(self.list, LB_GETTOPINDEX, None, None) }
                .0
                .max(0) as usize
        };
        let count = if self.grid_mode {
            ((controls::rect(self.grid).bottom / self.grid_row() + 2).max(1) as usize)
                * self.grid_columns()
        } else {
            (controls::rect(self.list).bottom / self.item_dip(58).max(1) + 3).max(1) as usize
        };
        for entry in self.visible.iter().skip(top).take(count) {
            if let Entry::File(asset) = entry {
                let key = ThumbKey::from(asset);
                if let Some(thumb) = self.thumbs.get_mut(&key) {
                    thumb.touched = Instant::now();
                } else if !asset.missing && self.pending.len() < 24 && self.pending.insert(key) {
                    let _ = self.thumb_tx.send(asset.clone());
                }
            }
        }
        if self.thumbs.len() > 384 {
            let mut keys = self
                .thumbs
                .iter()
                .map(|(key, value)| (value.touched, key.clone()))
                .collect::<Vec<_>>();
            keys.sort_by_key(|v| v.0);
            for (_, key) in keys.into_iter().take(self.thumbs.len() - 320) {
                self.thumbs.remove(&key);
            }
        }
    }
    fn geometry(&self, hwnd: HWND) -> LibraryLayout {
        LibraryLayout::with_width(controls::rect(hwnd), self.dpi, self.inspector_width)
    }
    fn layout(&mut self, hwnd: HWND) {
        let old_columns = self.grid_columns();
        let old_cells = self.capture_grid_cells();
        let batch = controls::layout_batch(hwnd);
        self.dpi = editor_ui::window_dpi(hwnd);
        let g = self.geometry(hwnd);
        let dpi = self.dpi;
        let d = |n| editor_ui::scale_dip(n, dpi);
        let put =
            |h, r: RECT| controls::place(h, r.left, r.top, r.right - r.left, r.bottom - r.top);
        controls::place(
            self.directory,
            d(158),
            d(6),
            (g.right - d(208)).max(1),
            d(28),
        );
        put(self.search, g.search);
        for (i, b) in self.view_buttons.iter().enumerate() {
            controls::place(*b, g.right - d(84) + d(i as i32 * 36), d(52), d(30), d(30));
            ui::selected(*b, self.grid_mode == (i == 0));
        }
        for (i, b) in self.categories.iter().enumerate() {
            controls::place(*b, d(10), d(84 + i as i32 * 34), g.side - d(20), d(34));
            ui::selected(*b, i == self.filter_kind);
        }
        controls::place(self.location, d(10), d(340), g.side - d(20), d(34));
        put(self.list, g.content);
        put(self.grid, g.content);
        controls::show(
            self.list,
            !self.grid_mode && !self.quick && !self.visible.is_empty(),
        );
        controls::show(
            self.grid,
            self.grid_mode && !self.quick && !self.visible.is_empty(),
        );
        controls::place(
            self.inspector_body,
            g.right,
            self.dip(40),
            g.width - g.right,
            g.height - self.dip(94),
        );
        controls::place(
            self.splitter,
            g.right - self.dip(6),
            self.dip(40),
            self.dip(6),
            g.height - self.dip(40),
        );
        for (i, b) in self.buttons.iter().enumerate() {
            controls::show(*b, [0, 1, 2, 3, 5, 8].contains(&i) || self.filter_kind == 5);
        }
        controls::place(
            self.buttons[0],
            g.right + d(16),
            g.height - d(42),
            g.width - g.right - d(32),
            d(30),
        );
        ui::style(self.buttons[0], None, false, true, false);
        controls::place(self.buttons[5], g.right - d(40), d(6), d(28), d(28));
        controls::place(self.buttons[8], g.width - d(40), d(6), d(28), d(28));
        self.layout_inspector(hwnd);
        if self.filter_kind == 5 {
            for (column, i) in [4, 6, 7].into_iter().enumerate() {
                controls::show(self.buttons[i], true);
                controls::place(
                    self.buttons[i],
                    d(12 + column as i32 * 34),
                    g.height - d(42),
                    d(28),
                    d(28),
                );
            }
        }
        // Commit the new client sizes before calculating the grid's scroll range.
        drop(batch);
        unsafe {
            let height = self.item_dip(58) as isize;
            if SendMessageW(self.list, LB_GETITEMHEIGHT, Some(WPARAM(0)), None).0 != height {
                SendMessageW(
                    self.list,
                    LB_SETITEMHEIGHT,
                    Some(WPARAM(0)),
                    Some(LPARAM(height)),
                );
            }
        }
        self.grid_scroll = self.grid_scroll.clamp(0, self.grid_max());
        self.start_reflow(old_columns, old_cells);
        ui::invalidate(hwnd);
        ui::invalidate(self.grid);
    }
    fn grid_columns(&self) -> usize {
        ((self.grid_width() + self.dip(14)) / (self.item_dip(170) + self.dip(14))).max(1) as usize
    }
    fn item_dip(&self, n: i32) -> i32 {
        self.dip(n) * self.item_zoom / 100
    }
    fn zoom_items(&mut self, hwnd: HWND, delta: i32) {
        self.stop_reflow();
        let next = (self.item_zoom + if delta > 0 { 10 } else { -10 }).clamp(70, 220);
        if next == self.item_zoom {
            return;
        }
        let anchor = (self.grid_scroll / self.grid_row()).max(0) as usize * self.grid_columns();
        let top = unsafe { SendMessageW(self.list, LB_GETTOPINDEX, None, None) }
            .0
            .max(0) as usize;
        self.item_zoom = next;
        self.layout(hwnd);
        self.grid_scroll =
            ((anchor / self.grid_columns()) as i32 * self.grid_row()).clamp(0, self.grid_max());
        unsafe {
            SendMessageW(self.list, LB_SETTOPINDEX, Some(WPARAM(top)), None);
        }
        ui::invalidate(self.grid);
        ui::invalidate(self.list);
    }
    fn layout_inspector(&mut self, hwnd: HWND) {
        let g = self.geometry(hwnd);
        let dpi = self.dpi;
        let d = |n| editor_ui::scale_dip(n, dpi);
        let maximum = (g.actions + d(28) + d(16) - (g.height - d(54))).max(0);
        self.inspector_scroll = self.inspector_scroll.clamp(0, maximum);
        let offset = d(40) + self.inspector_scroll;
        controls::place(
            self.preview,
            d(16),
            g.preview.top - offset,
            g.preview.right - g.preview.left,
            g.preview.bottom - g.preview.top,
        );
        controls::place(
            self.expand,
            g.preview.right - g.preview.left - d(38),
            g.preview.bottom - g.preview.top - d(38),
            d(30),
            d(30),
        );
        for i in 1..=3 {
            controls::place(
                self.buttons[i],
                d(16 + 36 * (i as i32 - 1)),
                g.actions - offset,
                d(28),
                d(28),
            );
        }
        ui::invalidate(self.inspector_body);
    }
    fn dip(&self, n: i32) -> i32 {
        n * self.dpi as i32 / 96
    }
    fn card_width(&self) -> i32 {
        self.item_dip(170).min(self.grid_width()).max(1)
    }
    fn grid_row(&self) -> i32 {
        self.card_width() * 100 / 155 + self.item_dip(64)
    }
    fn grid_max(&self) -> i32 {
        ((self.visible.len().div_ceil(self.grid_columns()) as i32) * self.grid_row()
            - controls::rect(self.grid).bottom)
            .max(0)
    }
    fn grid_rect(&self, i: usize) -> RECT {
        let cols = self.grid_columns();
        let step = self.card_width() + self.dip(14);
        ui::rect(
            i as i32 % cols as i32 * step,
            i as i32 / cols as i32 * self.grid_row() - self.grid_scroll,
            self.card_width(),
            self.grid_row() - self.dip(14),
        )
    }
    fn detail(&self, entry: &Entry) -> String {
        match entry {
            Entry::File(a) => {
                if a.missing {
                    return "文件不存在".into();
                }
                format!("{} · {}", entry.label(), file_size(a.bytes))
            }
            Entry::Pin(..) => entry.label().into(),
        }
    }
    fn draw_thumbnail(&self, dc: HDC, area: RECT, entry: &Entry) {
        let t = editor_ui::EditorTheme::current(self.dpi);
        super::fill(dc, area, t.palette.raised);
        let image = if let Entry::File(a) = entry {
            self.thumbs
                .get(&ThumbKey::from(a))
                .and_then(|t| t.image.as_ref())
        } else {
            None
        };
        if let Some(image) = image {
            let scale = ((area.right - area.left) as f64 / image.width as f64)
                .min((area.bottom - area.top) as f64 / image.height as f64);
            let w = (image.width as f64 * scale) as i32;
            let h = (image.height as f64 * scale) as i32;
            super::draw_raster(
                dc,
                image,
                ui::rect(
                    area.left + (area.right - area.left - w) / 2,
                    area.top + (area.bottom - area.top - h) / 2,
                    w,
                    h,
                ),
            );
        } else {
            editor_ui::draw_icon(
                dc,
                ui::rect(
                    (area.left + area.right - self.dip(26)) / 2,
                    (area.top + area.bottom - self.dip(26)) / 2,
                    self.dip(26),
                    self.dip(26),
                ),
                category_icon(entry.category()),
                t.palette.text_muted,
            );
        }
    }
    fn paint_grid(&self, dc: HDC) {
        let r = controls::rect(self.grid);
        let t = editor_ui::EditorTheme::current(self.dpi);
        let d = |n| self.item_dip(n);
        super::fill(dc, r, t.palette.panel);
        let sel = unsafe { SendMessageW(self.list, LB_GETCURSEL, None, None) }.0;
        for i in self.grid_paint_range() {
            let entry = &self.visible[i];
            let cell = self.visual_grid_rect(i);
            if cell.bottom < 0 || cell.top > r.bottom {
                continue;
            }
            let selected = i as isize == sel;
            editor_ui::rounded_surface(
                dc,
                cell,
                if selected {
                    t.palette.raised
                } else {
                    t.palette.panel
                },
                if selected {
                    t.palette.raised
                } else {
                    t.palette.panel
                },
                d(4),
            );
            let poster = ui::rect(
                cell.left + 1,
                cell.top + 1,
                cell.right - cell.left - 2,
                (cell.right - cell.left) * 100 / 155,
            );
            self.draw_thumbnail(dc, poster, entry);
            let badge = match entry {
                Entry::File(a) if a.missing => "缺失".to_owned(),
                Entry::File(a) if a.kind.is_image() => a
                    .path
                    .extension()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_uppercase(),
                _ => match entry.category() {
                    2 => "视频",
                    3 => "音频",
                    4 => "项目",
                    _ => "贴图",
                }
                .into(),
            };
            let bw = d((badge.chars().count() as i32 * 11 + 10).max(32));
            let tag = ui::rect(poster.right - bw - d(6), poster.bottom - d(24), bw, d(18));
            editor_ui::rounded_surface(dc, tag, t.palette.raised, t.palette.raised, d(2));
            ui::text_style(
                dc,
                tag,
                &badge,
                t.palette.text,
                (t.dpi as i32 * self.item_zoom / 100) as u32,
                true,
                editor_ui::TextStyle::Caption,
            );
            ui::text_style(
                dc,
                ui::rect(
                    cell.left + d(9),
                    poster.bottom + d(6),
                    cell.right - cell.left - d(18),
                    d(22),
                ),
                &entry.name(),
                t.palette.text,
                (t.dpi as i32 * self.item_zoom / 100) as u32,
                false,
                editor_ui::TextStyle::Body,
            );
            ui::text_style(
                dc,
                ui::rect(
                    cell.left + d(9),
                    poster.bottom + d(29),
                    cell.right - cell.left - d(18),
                    d(18),
                ),
                &self.detail(entry),
                t.palette.text_muted,
                (t.dpi as i32 * self.item_zoom / 100) as u32,
                false,
                editor_ui::TextStyle::Caption,
            );
        }
        self.paint_scrollbar(self.grid, dc);
    }

    fn run(&mut self, hwnd: HWND, id: usize) {
        if let Err(error) = self.command(hwnd, id) {
            asset_preview::notice(self.preview, &format!("操作未完成：{error}"));
        }
    }
    fn command(&mut self, hwnd: HWND, id: usize) -> Result<(), String> {
        if (40..=45).contains(&id) {
            self.remember_selection();
            self.filter_kind = id - 40;
            self.filter(false);
            self.layout(hwnd);
            return Ok(());
        }
        match id {
            33 => {
                if let Some(path) = crate::platform::path_field::choose(hwnd, &self.library)? {
                    controls::text(self.directory, &path.to_string_lossy());
                    if path != self.library {
                        self.library = path;
                        self.index_rx = None;
                        self.reload();
                    }
                }
                return Ok(());
            }
            30 | 31 => {
                self.stop_reflow();
                self.grid_mode = id == 30;
                self.layout(hwnd);
                return Ok(());
            }
            7 => {
                pin_manager::recover_interaction()?;
                self.reload();
                return Ok(());
            }
            8 => {
                self.preview_key = None;
                self.thumbs.retain(|_, v| v.error.is_none());
                self.reload();
                return Ok(());
            }
            9 => {
                if unsafe {
                    crate::platform::app_dialog::message(
                        Some(hwnd),
                        w!("清理最近关闭贴图的恢复记录？已保存的文件和当前桌面贴图会保留。"),
                        w!("清理恢复记录"),
                        MB_YESNO | MB_ICONQUESTION,
                    )
                } == IDYES
                {
                    pin_manager::clear_closed()?;
                    self.reload();
                }
                return Ok(());
            }
            10 => return crate::platform::capture_tools::manage_pin_groups_async(),
            15 | 20 => {
                if let Some(entry) = self.selected() {
                    if let Some(popup) = self.popup.take() {
                        unsafe {
                            let _ = DestroyWindow(popup);
                        }
                    }
                    asset_preview::pause(self.preview);
                    self.popup = Some(library_preview::show(
                        hwnd,
                        self.owner,
                        &self.library,
                        entry,
                    )?);
                }
                return Ok(());
            }
            21 | 22 => {
                if let Some(entry) = self.selected() {
                    open_entry(self.owner, &self.library, &entry, id == 22)?;
                }
                return Ok(());
            }
            23 => {
                self.file_information(hwnd);
                return Ok(());
            }
            24 | 25 => {
                self.remove_entry(hwnd, id == 25)?;
                return Ok(());
            }
            26 => {
                if let Some(Entry::File(a)) = self.selected() {
                    crate::platform::capture_tools::copy_text(hwnd, &a.path.to_string_lossy())?;
                }
                return Ok(());
            }

            _ => {}
        }
        let Some(entry) = self.selected() else {
            return Ok(());
        };
        match entry {
            Entry::Pin(pin, active) if id == 3 => {
                if active {
                    pin_manager::show_pin(&pin)?;
                } else {
                    pin_manager::restore_closed(Some(&pin))?;
                }
                self.reload();
            }
            Entry::File(asset) => {
                if id == 3 && asset.missing {
                    if let Some(path) = relocate_dialog(hwnd, asset.kind) {
                        asset_index::relocate(&self.library, &asset.path, &path)?;
                        self.selection_by_filter
                            .insert(self.filter_kind, (path.to_string_lossy().to_lowercase(), 0));
                        self.preview_key = None;
                        self.reload();
                    }
                    return Ok(());
                }
                if id != 6 && asset.missing {
                    return Err("文件已移动或删除，请重新定位后刷新".into());
                }
                match id {
                    3 if asset.kind == Kind::Audio => {
                        let video = crate::platform::workspace::video_handle()
                            .ok_or("请先打开一个视频项目")?;
                        crate::platform::media_panel::import_audio_path(video, asset.path)?;
                        crate::platform::workspace::switch(1);
                    }
                    3 if asset.kind == Kind::Project => {
                        OUTCOME.with(|out| {
                            *out.borrow_mut() = Some(CaptureOutcome::Project(asset.path));
                        });
                        unsafe {
                            let _ = PostMessageW(
                                Some(self.owner),
                                WM_CAPTURE_COMPLETE,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                        if crate::platform::workspace::active() {
                            crate::platform::workspace::switch(1);
                        } else {
                            unsafe {
                                let _ = DestroyWindow(hwnd);
                            }
                        }
                    }
                    3 if asset.kind == Kind::Video => {
                        crate::platform::recorder_window::import_workspace_video(
                            self.owner,
                            &asset.path,
                        )?;
                        crate::platform::workspace::switch(1);
                    }
                    3 => {
                        crate::platform::image_workspace::open_source(&asset.path, &self.library)?;
                    }
                    4 if asset.kind.is_image() => copy_image(hwnd, &Raster::load(&asset.path)?)?,
                    5 if asset.kind.is_image() => {
                        show_image(Raster::load(&asset.path)?, &self.library, true)?;
                        self.reload();
                    }
                    6 => {
                        std::process::Command::new("explorer.exe")
                            .arg("/select,")
                            .arg(asset.path)
                            .spawn()
                            .map_err(|e| e.to_string())?;
                    }
                    _ => return Err("该操作不适用于当前素材".into()),
                }
            }
            Entry::Pin(..) => {}
        }
        Ok(())
    }
    fn draw_item(&self, item: &windows::Win32::UI::Controls::DRAWITEMSTRUCT) {
        #[cfg(test)]
        tests::record_list_paint(item.hDC);
        let Some(entry) = self.visible.get(item.itemID as usize) else {
            return;
        };
        let t = editor_ui::EditorTheme::current(self.dpi);
        let d = |n| self.item_dip(n);
        let r = item.rcItem;
        let selected = item.itemState.0 & windows::Win32::UI::Controls::ODS_SELECTED.0 != 0;
        super::fill(item.hDC, r, t.palette.panel);
        let card = ui::rect(r.left, r.top, r.right - r.left, r.bottom - r.top - d(3));
        editor_ui::rounded_surface(
            item.hDC,
            card,
            if selected {
                t.palette.raised
            } else {
                t.palette.panel
            },
            if selected {
                t.palette.raised
            } else {
                t.palette.panel
            },
            d(4),
        );
        self.draw_thumbnail(
            item.hDC,
            ui::rect(r.left + d(2), r.top + d(2), d(80), d(50)),
            entry,
        );
        let wide = r.right - r.left >= d(430);
        ui::text_style(
            item.hDC,
            ui::rect(
                r.left + d(94),
                r.top + d(if wide { 14 } else { 6 }),
                r.right - r.left - d(if wide { 238 } else { 104 }),
                d(24),
            ),
            &entry.name(),
            t.palette.text,
            (t.dpi as i32 * self.item_zoom / 100) as u32,
            false,
            editor_ui::TextStyle::Body,
        );
        ui::text_style(
            item.hDC,
            if wide {
                ui::rect(r.right - d(134), r.top + d(14), d(124), d(24))
            } else {
                ui::rect(
                    r.left + d(94),
                    r.top + d(30),
                    r.right - r.left - d(104),
                    d(18),
                )
            },
            &self.detail(entry),
            t.palette.text_muted,
            (t.dpi as i32 * self.item_zoom / 100) as u32,
            false,
            editor_ui::TextStyle::Caption,
        );
    }
    fn paint_information(&self, hwnd: HWND, dc: HDC) {
        let parent = unsafe { GetParent(hwnd) }.unwrap_or(hwnd);
        let g = self.geometry(parent);
        let t = editor_ui::EditorTheme::current(self.dpi);
        let d = |n| self.dip(n);
        let saved = unsafe { SaveDC(dc) };
        unsafe {
            let _ = SetViewportOrgEx(dc, -g.right, -d(40) - self.inspector_scroll, None);
        }
        if let Some(entry) = self.selected() {
            let left = g.right + d(16);
            let width = g.width - g.right - d(32);
            ui::text_style(
                dc,
                ui::rect(left, g.preview.bottom + d(12), width, d(24)),
                &entry.name(),
                t.palette.text,
                t.dpi,
                false,
                editor_ui::TextStyle::Body,
            );
            ui::text_style(
                dc,
                ui::rect(left, g.preview.bottom + d(40), width, d(20)),
                &asset_preview::summary(self.preview),
                t.palette.text_muted,
                t.dpi,
                false,
                editor_ui::TextStyle::Body,
            );
            ui::line(dc, ui::rect(left, g.info - d(14), width, 1), t);
            ui::text_style(
                dc,
                ui::rect(left, g.info, width, d(22)),
                "文件信息",
                t.palette.text,
                t.dpi,
                false,
                editor_ui::TextStyle::Heading,
            );
            let rows = match &entry {
                Entry::File(a) => vec![
                    (
                        "类型",
                        format!(
                            "{} {}",
                            a.path
                                .extension()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_uppercase(),
                            entry.label()
                        ),
                    ),
                    ("修改时间", modified_label(a.modified)),
                    ("大小", file_size(a.bytes)),
                    (
                        "位置",
                        a.path
                            .parent()
                            .unwrap_or(&a.path)
                            .to_string_lossy()
                            .into_owned(),
                    ),
                ],
                Entry::Pin(..) => vec![("类型", entry.label().to_owned())],
            };
            for (row, (label, value)) in rows.iter().enumerate() {
                let y = g.info + d(32) + row as i32 * g.info_step;
                ui::text_style(
                    dc,
                    ui::rect(left, y, d(74), d(24)),
                    label,
                    t.palette.text_muted,
                    t.dpi,
                    false,
                    editor_ui::TextStyle::Body,
                );
                ui::text_style(
                    dc,
                    ui::rect(left + d(74), y, width - d(74), d(24)),
                    value,
                    t.palette.text_secondary,
                    t.dpi,
                    false,
                    editor_ui::TextStyle::Body,
                );
            }
        } else {
            ui::text_style(
                dc,
                ui::rect(
                    g.right + d(16),
                    g.preview.bottom + d(16),
                    g.width - g.right - d(32),
                    d(30),
                ),
                "选择素材查看预览与信息",
                t.palette.text_muted,
                t.dpi,
                false,
                editor_ui::TextStyle::Body,
            );
        }
        unsafe {
            let _ = RestoreDC(dc, saved);
        }
    }
    fn file_information(&self, hwnd: HWND) {
        if let Some(entry) = self.selected() {
            show_information(hwnd, &entry);
        }
    }
    fn remove_entry(&mut self, hwnd: HWND, source: bool) -> Result<(), String> {
        let Some(Entry::File(asset)) = self.selected() else {
            return Ok(());
        };
        let message = if source {
            format!(
                "将以下源文件移入回收站，并移除素材记录？\n{}\n引用此素材的工程可能无法继续读取它。",
                asset.path.display()
            )
        } else {
            format!(
                "从素材库移除这条记录？源文件会保留，刷新后不会自动重新显示。\n{}",
                asset.path.display()
            )
        };
        let text = super::wide(&message);
        if unsafe {
            crate::platform::app_dialog::message(
                Some(hwnd),
                windows::core::PCWSTR(text.as_ptr()),
                if source {
                    w!("删除源文件")
                } else {
                    w!("删除素材记录")
                },
                MB_YESNO | MB_ICONWARNING,
            )
        } != IDYES
        {
            return Ok(());
        }
        if let Some(popup) = self.popup.take() {
            unsafe {
                let _ = DestroyWindow(popup);
            }
        }
        if source {
            asset_preview::clear(self.preview);
            recycle_source(hwnd, &asset.path)?;
        }
        asset_index::forget(&self.library, &asset.path)?;
        self.index_rx = None;
        self.reload();
        Ok(())
    }
    fn context_menu(&mut self, hwnd: HWND, surface: HWND, lp: LPARAM) {
        let mut point = POINT {
            x: lp.0 as u16 as i16 as i32,
            y: (lp.0 >> 16) as u16 as i16 as i32,
        };
        if lp.0 != -1 {
            unsafe {
                let _ = ScreenToClient(surface, &raw mut point);
            }
            let index = if surface == self.grid {
                (0..self.visible.len()).find(|i| {
                    let r = self.visual_grid_rect(*i);
                    point.x >= r.left && point.x < r.right && point.y >= r.top && point.y < r.bottom
                })
            } else {
                let hit = unsafe {
                    SendMessageW(
                        self.list,
                        LB_ITEMFROMPOINT,
                        None,
                        Some(LPARAM(
                            ((point.y as u16 as u32) << 16 | point.x as u16 as u32) as isize,
                        )),
                    )
                }
                .0 as usize;
                (hit >> 16 == 0).then_some(hit & 65535)
            };
            let Some(index) = index.filter(|i| *i < self.visible.len()) else {
                return;
            };
            unsafe {
                SendMessageW(self.list, LB_SETCURSEL, Some(WPARAM(index)), None);
            }
            self.selection();
            ui::invalidate(self.list);
        } else {
            point = POINT {
                x: self.dip(30),
                y: self.dip(40),
            };
        }
        unsafe {
            let _ = ClientToScreen(surface, &raw mut point);
            let _ = ScreenToClient(hwnd, &raw mut point);
        }
        let Some(entry) = self.selected() else {
            return;
        };
        let file = matches!(&entry, Entry::File(_));
        let present = matches!(&entry,Entry::File(a) if !a.missing);
        let image = matches!(&entry,Entry::File(a) if a.kind.is_image()&&!a.missing);
        use crate::platform::app_menu::{self, Row};
        let rows = vec![
            Row::command(20, "预览", IconKind::Fit, true),
            Row::separator(),
            Row::command(21, "在图片工具中打开", IconKind::Image, image),
            Row::command(22, "添加到视频工作台", IconKind::Play, present),
            Row::separator(),
            Row::command(4, "复制图片", IconKind::Copy, image),
            Row::command(5, "贴到桌面", IconKind::Pin, image),
            Row::command(26, "复制文件路径", IconKind::Copy, file),
            Row::command(6, "在资源管理器中显示", IconKind::Folder, file),
            Row::command(23, "查看文件信息", IconKind::Log, true),
            Row::separator(),
            Row::command(24, "删除记录（保留源文件）", IconKind::Delete, file),
            Row::command(25, "删除源文件（移入回收站）", IconKind::Delete, present),
        ];
        match app_menu::show_rows(
            hwnd,
            ui::rect(point.x, point.y, 1, 1),
            editor_ui::EditorTheme::current(self.dpi),
            rows,
            false,
        ) {
            Ok(Some(id)) => self.run(hwnd, id as usize),
            Ok(None) => {}
            Err(e) => asset_preview::notice(self.preview, &e.to_string()),
        }
    }
    fn draw_category(&self, item: &windows::Win32::UI::Controls::DRAWITEMSTRUCT) {
        let i = item.CtlID as usize - 40;
        let t = editor_ui::EditorTheme::current(self.dpi);
        let d = |n| t.scale(n);
        let r = item.rcItem;
        let selected = i == self.filter_kind;
        let mut mouse = POINT::default();
        unsafe {
            let _ = GetCursorPos(&raw mut mouse);
            let _ = ScreenToClient(item.hwndItem, &raw mut mouse);
        }
        let hover = mouse.x >= 0 && mouse.x < r.right && mouse.y >= 0 && mouse.y < r.bottom;
        let bg = if selected || hover {
            t.palette.raised
        } else {
            t.palette.panel
        };
        super::fill(item.hDC, r, t.palette.panel);
        editor_ui::rounded_surface(item.hDC, r, bg, bg, d(4));
        let color = if selected {
            t.palette.accent
        } else {
            t.palette.text_secondary
        };
        editor_ui::draw_icon(
            item.hDC,
            ui::rect(d(10), d(9), d(16), d(16)),
            category_icon(i),
            color,
        );
        ui::text_style(
            item.hDC,
            ui::rect(d(34), 0, r.right - d(59), r.bottom),
            ["全部素材", "图片与截图", "视频", "声音", "项目", "桌面贴图"][i],
            color,
            t.dpi,
            false,
            editor_ui::TextStyle::Body,
        );
        let count = self
            .entries
            .iter()
            .filter(|e| i == 0 || e.category() == i)
            .count();
        ui::text_style(
            item.hDC,
            ui::rect(r.right - d(26), 0, d(25), r.bottom),
            &count.to_string(),
            t.palette.text_muted,
            t.dpi,
            true,
            editor_ui::TextStyle::Caption,
        );
        if item.itemState.0 & windows::Win32::UI::Controls::ODS_FOCUS.0 != 0 {
            editor_ui::selection_outline(item.hDC, r, t.palette.accent);
        }
    }
}
// Library-only geometry. No changes to shared workbench dimensions or controls.
struct LibraryLayout {
    width: i32,
    height: i32,
    side: i32,
    right: i32,
    search: RECT,
    content: RECT,
    preview: RECT,
    info: i32,
    info_step: i32,
    actions: i32,
}
impl LibraryLayout {
    #[cfg(test)]
    fn new(r: RECT, dpi: u32) -> Self {
        Self::with_width(r, dpi, None)
    }
    fn with_width(r: RECT, dpi: u32, inspector: Option<i32>) -> Self {
        let d = |n| editor_ui::scale_dip(n, dpi);
        let width = r.right;
        let height = r.bottom;
        let side = d(if width <= d(1000) {
            172
        } else if width <= d(1150) {
            172
        } else {
            172
        });
        let desired = inspector.unwrap_or_else(|| {
            crate::platform::workbench_layout::inspector(width * 96 / dpi as i32)
        });
        let right = width
            - d(desired.clamp(
                264,
                ((width - side - d(200)) * 96 / dpi as i32).clamp(264, 640),
            ));
        let content = ui::rect(
            side + d(18),
            d(93),
            (right - side - 2 * d(18)).max(1),
            (height - d(107)).max(1),
        );
        let search = ui::rect(
            side + d(46),
            d(52),
            (right - side - d(150)).clamp(d(70), d(232)),
            d(30),
        );
        let preview = ui::rect(
            right + d(16),
            d(56),
            width - right - d(32),
            (width - right - d(32)) * 2 / 3,
        );
        let info = preview.bottom + d(90);
        let info_step = d(32);
        let actions = info + d(32) + 3 * info_step + d(32);
        Self {
            width,
            height,
            side,
            right,
            search,
            content,
            preview,
            info,
            info_step,
            actions,
        }
    }
}
fn category_icon(i: usize) -> IconKind {
    [
        IconKind::Collage,
        IconKind::Image,
        IconKind::Play,
        IconKind::Sound,
        IconKind::Folder,
        IconKind::Pin,
    ][i.min(5)]
}
fn file_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1048576.)
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}
fn modified_label(milliseconds: u128) -> String {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    // Bind only the time conversion used here; no workspace dependency changes.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn FileTimeToLocalFileTime(input: *const FILETIME, output: *mut FILETIME) -> i32;
        fn FileTimeToSystemTime(input: *const FILETIME, output: *mut SYSTEMTIME) -> i32;
    }
    if milliseconds == 0 {
        return "—".into();
    }
    let ticks = (milliseconds.saturating_mul(10_000) + 116_444_736_000_000_000)
        .min(u64::MAX as u128) as u64;
    let utc = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut local = FILETIME::default();
    let mut time = SYSTEMTIME::default();
    if unsafe { FileTimeToLocalFileTime(&utc, &raw mut local) } != 0
        && unsafe { FileTimeToSystemTime(&local, &raw mut time) } != 0
    {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute
        )
    } else {
        "—".into()
    }
}
unsafe extern "system" fn library_surface(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if message == WM_DRAWITEM && (40..=45).contains(&wp.0) && lp.0 != 0 {
        let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const History;
        if !pointer.is_null() {
            unsafe {
                (&*pointer)
                    .draw_category(&*(lp.0 as *const windows::Win32::UI::Controls::DRAWITEMSTRUCT));
            }
            return LRESULT(1);
        }
    }
    unsafe { windows::Win32::UI::Shell::DefSubclassProc(hwnd, message, wp, lp) }
}
unsafe extern "system" fn history_proc(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut History;
    if pointer.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wp, lp) };
    }
    if message == WM_NCDESTROY {
        WINDOWS.with(|v| v.borrow_mut().retain(|h| *h != hwnd));
        unsafe {
            if let Some(popup) = (*pointer).popup.take() {
                let _ = DestroyWindow(popup);
            }
            let _ = KillTimer(Some(hwnd), 81);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(pointer));
        }
        return unsafe { DefWindowProcW(hwnd, message, wp, lp) };
    }
    let state = unsafe { &mut *pointer };
    match message {
        WM_COMMAND => {
            let id = wp.0 & 0xffff;
            let code = (wp.0 >> 16) & 0xffff;
            if id == 32 && code == EN_CHANGE as usize {
                let value = controls::value(state.directory);
                if let Ok(path) = crate::platform::path_field::validate(&value) {
                    if path.is_dir() && path != state.library {
                        state.library = path;
                        state.index_rx = None;
                        state.reload();
                    }
                }
            } else if id == 1 && code == EN_CHANGE as usize {
                state.filter(true);
            } else if id == 2 && code == LBN_SELCHANGE as usize {
                state.selection();
            } else if id == 2 && code == LBN_DBLCLK as usize {
                state.run(hwnd, 20);
            } else if id >= 3 && code == 0 {
                state.run(hwnd, id);
            }
        }
        WM_TIMER if wp.0 == 81 => {
            let summary = asset_preview::summary(state.preview);
            if summary != state.preview_summary {
                state.preview_summary = summary;
                ui::invalidate(state.inspector_body);
            }
            if let Some(rx) = &state.index_rx
                && let Ok(assets) = rx.try_recv()
            {
                state.index_rx = None;
                state.apply_index(assets);
                ui::invalidate(hwnd);
            }
            let mut changed = false;
            while let Ok((key, result)) = state.thumb_rx.try_recv() {
                state.pending.remove(&key);
                let (image, error) = match result {
                    Ok(image) => (Some(image), None),
                    Err(error) => (None, Some(error)),
                };
                state.thumbs.insert(
                    key,
                    Thumb {
                        image,
                        error,
                        touched: Instant::now(),
                    },
                );
                changed = true;
            }
            if unsafe { IsWindowVisible(hwnd) }.as_bool() {
                state.request_thumbs();
            }
            if changed {
                ui::invalidate(state.list);
                ui::invalidate(state.grid);
            }
        }
        WM_DRAWITEM if wp.0 == 2 => {
            state.draw_item(unsafe {
                &*(lp.0 as *const windows::Win32::UI::Controls::DRAWITEMSTRUCT)
            });
            return LRESULT(1);
        }
        WM_SIZE => state.layout(hwnd),
        WM_DPICHANGED => {
            state.layout(hwnd);
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let target = unsafe { BeginPaint(hwnd, &raw mut ps) };
            let r = controls::rect(hwnd);
            let frame = ui::BufferedFrame::new(target, r);
            let dc = frame.dc;
            let t = editor_ui::EditorTheme::current(state.dpi);
            super::fill(dc, r, t.palette.panel);
            let d = |n| t.scale(n);
            let g = state.geometry(hwnd);
            ui::text_style(
                dc,
                ui::rect(d(16), d(6), d(35), d(28)),
                "本地",
                t.palette.text_muted,
                t.dpi,
                false,
                editor_ui::TextStyle::Caption,
            );
            ui::text_style(
                dc,
                ui::rect(d(59), d(6), d(78), d(28)),
                "素材库",
                t.palette.text,
                t.dpi,
                false,
                editor_ui::TextStyle::Body,
            );
            ui::line(dc, ui::rect(d(147), d(12), 1, d(16)), t);
            ui::text_style(
                dc,
                ui::rect(d(20), d(53), g.side - d(40), d(22)),
                "素材库",
                t.palette.text_muted,
                t.dpi,
                false,
                editor_ui::TextStyle::Caption,
            );
            ui::text_style(
                dc,
                ui::rect(d(20), d(307), g.side - d(40), d(22)),
                "位置",
                t.palette.text_muted,
                t.dpi,
                false,
                editor_ui::TextStyle::Caption,
            );
            ui::text_style(
                dc,
                ui::rect(g.right + d(16), d(6), g.width - g.right - d(64), d(28)),
                "预览与信息",
                t.palette.text_secondary,
                t.dpi,
                false,
                editor_ui::TextStyle::Body,
            );
            ui::line(dc, ui::rect(0, d(39), r.right, 1), t);
            ui::line(dc, ui::rect(g.side, d(40), 1, r.bottom - d(40)), t);
            ui::line(dc, ui::rect(g.right - d(6), 0, d(1).max(1), d(40)), t);
            ui::line(
                dc,
                ui::rect(g.right, r.bottom - d(54), r.right - g.right, 1),
                t,
            );
            let search_box = ui::rect(
                g.search.left - d(28),
                g.search.top,
                g.search.right - g.search.left + d(28),
                d(30),
            );
            editor_ui::rounded_surface(dc, search_box, t.palette.raised, t.palette.raised, d(4));
            editor_ui::draw_icon(
                dc,
                ui::rect(search_box.left + d(8), search_box.top + d(7), d(16), d(16)),
                IconKind::Search,
                t.palette.text_muted,
            );
            if g.right - g.search.right > d(180) {
                ui::text_style(
                    dc,
                    ui::rect(g.right - d(157), d(52), d(65), d(30)),
                    "最近修改",
                    t.palette.text_muted,
                    t.dpi,
                    false,
                    editor_ui::TextStyle::Caption,
                );
            }
            if state.visible.is_empty() && !state.quick {
                ui::text(
                    dc,
                    ui::rect(g.side + d(18), d(160), g.right - g.side - d(36), d(40)),
                    if state.index_rx.is_some() {
                        "正在扫描素材…"
                    } else {
                        "没有匹配的素材"
                    },
                    t.palette.text_muted,
                    t.dpi,
                    true,
                );
            }
            drop(frame);
            unsafe {
                let _ = EndPaint(hwnd, &raw const ps);
            }
            return LRESULT(0);
        }
        WM_GETMINMAXINFO => unsafe {
            (*(lp.0 as *mut MINMAXINFO)).ptMinTrackSize = POINT { x: 760, y: 440 };
        },
        WM_ERASEBKGND => return LRESULT(1),
        _ => return unsafe { DefWindowProcW(hwnd, message, wp, lp) },
    }
    LRESULT(0)
}
unsafe extern "system" fn grid_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Ok(parent) = (unsafe { GetParent(h) }) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    let Some(s) = (unsafe { (GetWindowLongPtrW(parent, GWLP_USERDATA) as *mut History).as_mut() })
    else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    if s.handle_scroll_input(h, m, w, l) {
        return LRESULT(0);
    }
    match m {
        WM_TIMER if w.0 == 82 => {
            s.advance_reflow();
            ui::invalidate(h);
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &mut ps) };
            let frame = ui::BufferedFrame::new(dc, controls::rect(h));
            s.paint_grid(frame.dc);
            drop(frame);
            unsafe {
                let _ = EndPaint(h, &ps);
            }
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_CONTEXTMENU => {
            s.context_menu(parent, h, l);
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
            let x = l.0 as u16 as i16 as i32;
            let y = (l.0 >> 16) as u16 as i16 as i32;
            unsafe {
                let _ = SetFocus(Some(h));
            }
            if let Some(i) = (0..s.visible.len()).find(|i| {
                let r = s.visual_grid_rect(*i);
                x >= r.left && x < r.right && y >= r.top && y < r.bottom
            }) {
                unsafe {
                    SendMessageW(s.list, LB_SETCURSEL, Some(WPARAM(i)), None);
                }
                s.selection();
                if m == WM_LBUTTONDBLCLK {
                    s.run(parent, 20);
                }
            }
        }
        WM_KEYDOWN => {
            let selected = unsafe { SendMessageW(s.list, LB_GETCURSEL, None, None) }
                .0
                .max(0) as i32;
            let step = match w.0 {
                37 => -1,
                39 => 1,
                38 => -(s.grid_columns() as i32),
                40 => s.grid_columns() as i32,
                13 => {
                    s.run(parent, 20);
                    return LRESULT(0);
                }
                32 => {
                    s.run(parent, 20);
                    return LRESULT(0);
                }
                _ => return unsafe { DefWindowProcW(h, m, w, l) },
            };
            if !s.visible.is_empty() {
                let i = (selected + step).clamp(0, s.visible.len() as i32 - 1);
                unsafe {
                    SendMessageW(s.list, LB_SETCURSEL, Some(WPARAM(i as usize)), None);
                }
                let r = s.grid_rect(i as usize);
                if r.top < 0 {
                    s.grid_scroll += r.top;
                } else if r.bottom > controls::rect(h).bottom {
                    s.grid_scroll += r.bottom - controls::rect(h).bottom;
                }
                s.selection();
            }
        }
        WM_GETDLGCODE => return LRESULT(DLGC_WANTARROWS as isize),
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}
fn show_information(hwnd: HWND, entry: &Entry) {
    if let Err(error) = library_information::show(hwnd, entry.clone()) {
        let text = super::wide(&error);
        unsafe {
            crate::platform::app_dialog::message(
                Some(hwnd),
                windows::core::PCWSTR(text.as_ptr()),
                w!("文件信息"),
                MB_OK,
            );
        }
    }
}

fn open_entry(owner: HWND, library: &Path, entry: &Entry, video: bool) -> Result<(), String> {
    let Entry::File(asset) = entry else {
        return Err("桌面贴图请使用素材库的显示贴图操作".into());
    };
    if asset.missing {
        return Err("源文件不存在，请先重新定位素材".into());
    }
    if !video {
        if !asset.kind.is_image() {
            return Err("该素材不是图片".into());
        }
        return crate::platform::image_workspace::open_source(&asset.path, library);
    }
    if asset.kind == Kind::Project {
        OUTCOME.with(|out| *out.borrow_mut() = Some(CaptureOutcome::Project(asset.path.clone())));
        unsafe {
            let _ = PostMessageW(Some(owner), WM_CAPTURE_COMPLETE, WPARAM(0), LPARAM(0));
        }
    } else if let Some(workspace) = crate::platform::workspace::video_handle() {
        crate::platform::preview_window::import_project_asset_path(workspace, asset.path.clone())?;
    } else if asset.kind == Kind::Video {
        crate::platform::recorder_window::import_workspace_video(owner, &asset.path)?;
    } else {
        return Err("请先在视频工作台打开一个项目，再添加图片或声音素材".into());
    }
    crate::platform::workspace::switch(1);
    Ok(())
}
fn recycle_source(owner: HWND, path: &Path) -> Result<(), String> {
    use windows::Win32::{
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        UI::Shell::{
            FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOFX_ADDUNDORECORD, FOFX_EARLYFAILURE,
            FOFX_RECYCLEONDELETE, FileOperation, IFileOperation, IShellItem,
            SHCreateItemFromParsingName,
        },
    };
    if !path.is_absolute() || path.parent().is_none() || path.file_name().is_none() {
        return Err("无效的源文件路径".into());
    }
    let wide = super::wide(&path.to_string_lossy());
    let operation: IFileOperation =
        unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| e.to_string())?;
    let item: IShellItem =
        unsafe { SHCreateItemFromParsingName(windows::core::PCWSTR(wide.as_ptr()), None) }
            .map_err(|e| e.to_string())?;
    unsafe {
        operation.SetOwnerWindow(owner).map_err(|e| e.to_string())?;
        operation
            .SetOperationFlags(
                (FOFX_RECYCLEONDELETE | FOFX_EARLYFAILURE | FOFX_ADDUNDORECORD)
                    | FOF_NOCONFIRMATION
                    | FOF_NOERRORUI
                    | FOF_SILENT,
            )
            .map_err(|e| e.to_string())?;
        operation
            .DeleteItem(&item, None)
            .map_err(|e| e.to_string())?;
        operation
            .PerformOperations()
            .map_err(|e| format!("未删除源文件：{e}"))?;
        if operation
            .GetAnyOperationsAborted()
            .map_err(|e| e.to_string())?
            .as_bool()
        {
            return Err("删除已取消，源文件与记录保留".into());
        }
    }
    Ok(())
}
unsafe extern "system" fn list_surface(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let parent = HWND(data as *mut _);
    if let Some(s) = unsafe { (GetWindowLongPtrW(parent, GWLP_USERDATA) as *mut History).as_mut() }
    {
        if s.handle_scroll_input(h, m, w, l) {
            return LRESULT(0);
        }
        if m == WM_PAINT {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &mut ps) };
            let frame = ui::BufferedFrame::new(dc, controls::rect(h));
            s.paint_list(frame.dc);
            drop(frame);
            unsafe {
                let _ = EndPaint(h, &ps);
            }
            return LRESULT(0);
        }
        if m == WM_CONTEXTMENU {
            s.context_menu(parent, h, l);
            return LRESULT(0);
        }
    }
    let result = unsafe { windows::Win32::UI::Shell::DefSubclassProc(h, m, w, l) };
    if matches!(
        m,
        WM_LBUTTONDOWN | WM_KEYDOWN | LB_SETCURSEL | LB_SETTOPINDEX
    ) {
        ui::invalidate(h);
    }
    result
}
unsafe extern "system" fn preview_overlay(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let parent = HWND(data as *mut _);
    if m == WM_COMMAND && w.0 & 65535 == 90 {
        unsafe {
            let _ = PostMessageW(Some(parent), WM_COMMAND, WPARAM(20), LPARAM(0));
        }
        return LRESULT(0);
    }
    // Wheel over the preview scrolls overflowing details; Ctrl+wheel keeps preview image zoom.
    if m == WM_MOUSEWHEEL && w.0 & 8 == 0 {
        if let Ok(body) = unsafe { GetParent(h) } {
            unsafe {
                SendMessageW(body, m, Some(w), Some(l));
            }
        }
        return LRESULT(0);
    }
    unsafe { windows::Win32::UI::Shell::DefSubclassProc(h, m, w, l) }
}
unsafe extern "system" fn inspector_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Ok(parent) = (unsafe { GetParent(h) }) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    let Some(s) = (unsafe { (GetWindowLongPtrW(parent, GWLP_USERDATA) as *mut History).as_mut() })
    else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    match m {
        WM_COMMAND => unsafe {
            let _ = PostMessageW(Some(parent), m, w, l);
        },
        WM_MOUSEWHEEL => {
            s.inspector_scroll -= ((w.0 >> 16) as u16 as i16 as i32) * s.dip(48) / 120;
            s.layout_inspector(parent);
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &raw mut ps) };
            let r = controls::rect(h);
            let frame = ui::BufferedFrame::new(dc, r);
            let t = editor_ui::EditorTheme::current(s.dpi);
            super::fill(frame.dc, r, t.palette.panel);
            s.paint_information(h, frame.dc);
            let g = s.geometry(parent);
            let full = g.actions + s.dip(44) - s.dip(40);
            if full > r.bottom {
                let height = (r.bottom * r.bottom / full).max(s.dip(24));
                let y = s.inspector_scroll * (r.bottom - height) / (full - r.bottom).max(1);
                super::fill(
                    frame.dc,
                    ui::rect(r.right - s.dip(3), y, s.dip(3), height),
                    t.palette.border,
                );
            }
            drop(frame);
            unsafe {
                let _ = EndPaint(h, &ps);
            }
        }
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}
unsafe extern "system" fn splitter_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Ok(parent) = (unsafe { GetParent(h) }) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    let Some(s) = (unsafe { (GetWindowLongPtrW(parent, GWLP_USERDATA) as *mut History).as_mut() })
    else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    let splitter = Splitter::new(controls::rect(h), Axis::Vertical, Side::After);
    match m {
        WM_SETCURSOR => {
            unsafe { SetCursor(LoadCursorW(None, splitter.cursor()).ok()); }
            return LRESULT(1);
        }
        WM_MOUSEMOVE if s.splitter_drag.is_none() => {
            if !s.splitter_hover {
                s.splitter_hover = true;
                let mut tracking = windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT>() as u32,
                    dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::TME_LEAVE,
                    hwndTrack: h,
                    ..Default::default()
                };
                unsafe { let _ = windows::Win32::UI::Input::KeyboardAndMouse::TrackMouseEvent(&mut tracking); }
                ui::invalidate(h);
            }
        }
        windows::Win32::UI::Controls::WM_MOUSELEAVE => {
            s.splitter_hover = false;
            ui::invalidate(h);
        }
        WM_LBUTTONDOWN => {
            let p = splitter_parent_point(h, parent, l);
            let g = s.geometry(parent);
            unsafe { let _ = SetFocus(Some(h)); }
            s.splitter_drag = Some((
                splitter.begin(h, p.x, p.y, g.width-g.right, 0),
                s.inspector_width,
            ));
            s.splitter_hover = true;
            ui::invalidate(h);
        }
        WM_MOUSEMOVE if s.splitter_drag.is_some() => {
            let p = splitter_parent_point(h, parent, l);
            s.move_inspector_drag(parent, p);
        }
        WM_LBUTTONUP => s.finish_inspector_drag(parent, false),
        WM_CAPTURECHANGED | WM_CANCELMODE => s.finish_inspector_drag(parent, true),
        WM_KEYDOWN if w.0 == 27 => s.finish_inspector_drag(parent, true),
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &raw mut ps) };
            let frame = ui::BufferedFrame::new(dc, controls::rect(h));
            splitter.paint(frame.dc, editor_ui::EditorTheme::current(s.dpi), s.splitter_hover, s.splitter_drag.is_some());
            drop(frame);
            unsafe { let _ = EndPaint(h, &ps); }
        }
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}
/// Pointer positions must stay in the parent coordinate space as the divider moves.
fn splitter_parent_point(h: HWND, parent: HWND, l: LPARAM) -> POINT {
    let mut p = POINT { x:l.0 as u16 as i16 as i32, y:(l.0 >> 16) as u16 as i16 as i32 };
    unsafe { let _=ClientToScreen(h,&mut p); let _=ScreenToClient(parent,&mut p); }
    p
}
#[cfg(test)]
#[path = "library_scroll_review.rs"]
mod library_scroll_review;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    thread_local! {static LIST_PAINTS:std::cell::Cell<Option<(usize,usize)>>=const{std::cell::Cell::new(None)};}
    pub(super) fn record_list_paint(dc: HDC) {
        LIST_PAINTS.with(|v| {
            if let Some((all, direct)) = v.get() {
                v.set(Some((
                    all + 1,
                    direct + usize::from(unsafe { GetObjectType(HGDIOBJ(dc.0)) } != OBJ_MEMDC.0 as u32),
                )));
            }
        });
    }
    fn pump_until(hwnd: HWND, condition: impl Fn(&History) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            unsafe {
                SendMessageW(hwnd, WM_TIMER, Some(WPARAM(81)), None);
            }
            let history = unsafe { &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const History) };
            if condition(history) {
                break;
            }
            assert!(Instant::now() < deadline, "asset UI did not settle");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn library_layout_keeps_metadata_actions_and_search_separate_at_scaled_sizes() {
        for dpi in [96, 120, 144, 192] {
            for (width, height) in [(960, 600), (1440, 780), (860, 480), (1200, 700)] {
                let d = |n| editor_ui::scale_dip(n, dpi);
                let g = LibraryLayout::new(ui::rect(0, 0, d(width), d(height)), dpi);
                assert!(g.search.right + d(8) <= g.right - d(84));
                assert!(g.preview.right <= g.width - d(16));
                assert!(g.info + d(32) + 3 * g.info_step + d(24) <= g.actions);
                let scroll = (g.actions + d(44) - (g.height - d(54))).max(0);
                assert!(g.actions + d(28) - scroll <= g.height - d(54));
                assert!(
                    ((g.preview.right - g.preview.left) * 2 / 3
                        - (g.preview.bottom - g.preview.top))
                        .abs()
                        <= 1
                );
                assert!(g.content.right + d(18) <= g.right);
            }
        }
    }
    #[test]
    #[ignore = "Creates isolated native review windows; set PANZO_LIBRARY_REVIEW_DIR and a disposable LOCALAPPDATA"]
    fn library_visual_review() {
        use windows::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
        };
        let old_dpi =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        use crate::platform::ui_preferences::{self, ThemeMode};
        let output =
            PathBuf::from(std::env::var_os("PANZO_LIBRARY_REVIEW_DIR").expect("review directory"));
        std::fs::create_dir_all(&output).unwrap();
        let library = output.join("fixtures");
        std::fs::create_dir_all(&library).unwrap();
        let colors = [
            [130u8, 157, 164],
            [184, 159, 132],
            [141, 161, 144],
            [155, 153, 177],
        ];
        for (i, name) in [
            "山间手记.png",
            "产品说明.png",
            "灵感截图 02.png",
            "剪辑封面.png",
            "片尾画面.png",
            "长文件名素材示例：一次捕捉到完整表达.png",
        ]
        .iter()
        .enumerate()
        {
            let c = colors[i % 4];
            let image = image::RgbaImage::from_fn(640, 400, |x, y| {
                let ridge = ((x as f32 / 100.).sin() * 70. + 190.) as u32;
                let shift = if y > ridge { 0 } else { 40 };
                image::Rgba([
                    c[0].saturating_add(shift),
                    c[1].saturating_add(shift),
                    c[2].saturating_add(shift),
                    255,
                ])
            });
            image.save(library.join(name)).unwrap();
        }
        // A short real PCM audio fixture exercises waveform rendering and the audio category.
        let mut wav = Vec::new();
        let samples = 8000u32;
        wav.extend(b"RIFF");
        wav.extend((36 + samples * 2).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(8000u32.to_le_bytes());
        wav.extend(16000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((samples * 2).to_le_bytes());
        for i in 0..samples {
            let value = ((i as f32 * 0.17).sin() * 4000.) as i16;
            wav.extend(value.to_le_bytes());
        }
        std::fs::write(library.join("旁白 · 第一版.wav"), wav).unwrap();
        let module = unsafe { GetModuleHandleW(None) }.unwrap();
        let owner = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Panzo · 素材库布局验收"),
                WS_OVERLAPPEDWINDOW,
                40,
                40,
                1460,
                850,
                None,
                None,
                Some(module.into()),
                None,
            )
        }
        .unwrap();
        let hwnd = create_embedded(owner, owner, &library).unwrap();
        controls::place(hwnd, 0, 0, 1440, 780);
        pump_until(hwnd, |s| s.index_rx.is_none() && s.visible.len() == 7);
        unsafe {
            let _ = ShowWindow(owner, SW_SHOW);
        }
        pump_until(hwnd, |s| s.thumbs.len() == 7);
        let state = unsafe { &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut History) };
        let image = state
            .visible
            .iter()
            .position(|e| e.category() == 1)
            .unwrap();
        unsafe {
            SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(image)), None);
        }
        state.selection();
        let preview = state.preview;
        pump_until(hwnd, |_| {
            unsafe {
                SendMessageW(preview, WM_TIMER, Some(WPARAM(93)), None);
            }
            asset_preview::review_loaded(preview)
        });
        let original = ui_preferences::theme();
        for (name, mode, width, height, grid) in [
            ("01-grid-dark", ThemeMode::Dark, 1440, 780, true),
            ("02-list-dark", ThemeMode::Dark, 1440, 780, false),
            ("03-grid-light", ThemeMode::Light, 1440, 780, true),
            ("04-compact-light", ThemeMode::Light, 960, 600, true),
            ("05-compact-list", ThemeMode::Dark, 960, 600, false),
        ] {
            ui_preferences::set_theme(mode).unwrap();
            controls::place(owner, 40, 40, width + 16, height + 39);
            controls::place(hwnd, 0, 0, width, height);
            let state = unsafe { &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut History) };
            state.command(hwnd, if grid { 30 } else { 31 }).unwrap();
            assert_eq!(
                unsafe { SendMessageW(state.list, LB_GETITEMHEIGHT, Some(WPARAM(0)), None) }.0,
                state.dip(58) as isize
            );
            ui::refresh(owner);
            unsafe {
                let _ = ShowWindow(owner, SW_SHOW);
            }
            super::super::review_chrome_png(owner, &output.join(format!("{name}.png")));
        }
        let state = unsafe { &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut History) };
        let selected = state.selected().unwrap().key();
        controls::text(state.search, "不存在的素材");
        state.filter(true);
        assert!(state.visible.is_empty());
        assert!(!unsafe { IsWindowVisible(state.list) }.as_bool());
        assert!(!unsafe { IsWindowVisible(state.grid) }.as_bool());
        super::super::review_chrome_png(owner, &output.join("06-search-empty.png"));
        controls::text(state.search, "");
        state.filter(false);
        assert_eq!(state.selected().unwrap().key(), selected);
        state.command(hwnd, 43).unwrap();
        assert_eq!(state.visible.len(), 1);
        state.command(hwnd, 30).unwrap();
        let audio_preview = state.preview;
        pump_until(hwnd, |_| {
            unsafe {
                SendMessageW(audio_preview, WM_TIMER, Some(WPARAM(93)), None);
            }
            asset_preview::review_loaded(audio_preview)
        });
        super::super::review_chrome_png(owner, &output.join("07-audio.png"));
        state.command(hwnd, 45).unwrap();
        for i in [4, 6, 7] {
            assert!(unsafe { IsWindowVisible(state.buttons[i]) }.as_bool());
        }
        super::super::review_chrome_png(owner, &output.join("08-pins.png"));
        state.command(hwnd, 41).unwrap();
        state.command(hwnd, 30).unwrap();
        let selected_preview = state.preview;
        pump_until(hwnd, |_| {
            unsafe {
                SendMessageW(selected_preview, WM_TIMER, Some(WPARAM(93)), None);
            }
            asset_preview::review_loaded(selected_preview)
        });
        // Compare the actual drag surface with a clean repaint. The normal review
        // screenshot helper forces a repaint first and would hide stale pixels.
        if std::env::var_os("PANZO_LIBRARY_DRAG_REVIEW").is_some() {
            fn snapshot(hwnd: HWND) -> Raster {
                let mut origin = POINT::default();
                unsafe {
                    let _ = ClientToScreen(hwnd, &mut origin);
                    let _ = GdiFlush();
                    let _ = windows::Win32::Graphics::Dwm::DwmFlush();
                }
                let r = controls::rect(hwnd);
                let (desktop, image) = super::super::screen().unwrap();
                image
                    .crop(panzo_core::PhysicalRect {
                        x: origin.x - desktop.x,
                        y: origin.y - desktop.y,
                        width: r.right as u32,
                        height: r.bottom as u32,
                    })
                    .unwrap()
            }
            controls::place(owner, 40, 40, 1320, 800);
            controls::place(hwnd, 0, 0, 1280, 720);
            let mut report = Vec::new();
            let mut timing = Vec::new();
            let mut failures = 0;
            for (name, mode, grid) in [
                ("dark-grid", ThemeMode::Dark, true),
                ("dark-list", ThemeMode::Dark, false),
                ("light-grid", ThemeMode::Light, true),
                ("light-list", ThemeMode::Light, false),
            ] {
                ui_preferences::set_theme(mode).unwrap();
                state.command(hwnd, if grid { 30 } else { 31 }).unwrap();
                unsafe {
                    let _ = SetFocus(Some(state.splitter));
                }
                super::super::review_present(owner);
                unsafe {
                    SendMessageW(state.splitter, WM_LBUTTONDOWN, None, None);
                }
                for step in 0..48 {
                    let width = if step < 24 {
                        264 + step * 16
                    } else {
                        640 - (step - 24) * 16
                    };
                    let mut point = POINT {
                        x: 1280 - state.dip(width) - state.dip(6),
                        y: state.dip(160),
                    };
                    let frame_started = Instant::now();
                    unsafe {
                        let _ = ClientToScreen(hwnd, &mut point);
                        let _ = ScreenToClient(state.splitter, &mut point);
                        let position = LPARAM(
                            ((point.y as u16 as u32) << 16 | point.x as u16 as u32) as isize,
                        );
                        SendMessageW(state.splitter, WM_MOUSEMOVE, None, Some(position));
                        // A burst of pointer messages can keep WM_PAINT queued. Capture
                        // what users see before pumping or forcing any repaint.
                    }
                    timing.push(format!(
                        "{name} step={step} milliseconds={:.3}",
                        frame_started.elapsed().as_secs_f64() * 1000.
                    ));
                    if step % 6 == 0 || step == 47 {
                        let actual = snapshot(hwnd);
                        unsafe {
                            let _ = RedrawWindow(
                                Some(hwnd),
                                None,
                                None,
                                RDW_INVALIDATE | RDW_NOERASE | RDW_ALLCHILDREN | RDW_UPDATENOW,
                            );
                        }
                        let expected = snapshot(hwnd);
                        let changed = actual
                            .pixels
                            .chunks_exact(4)
                            .zip(expected.pixels.chunks_exact(4))
                            .filter(|(a, b)| a[..3] != b[..3])
                            .count();
                        report.push(format!(
                            "{name} step={step} width={width} differing_pixels={changed}"
                        ));
                        if changed > 0 {
                            failures += 1;
                            actual
                                .save(&output.join(format!("drag-{name}-{step}-actual.png")))
                                .unwrap();
                            expected
                                .save(&output.join(format!("drag-{name}-{step}-expected.png")))
                                .unwrap();
                        }
                    }
                }
                unsafe {
                    SendMessageW(state.splitter, WM_LBUTTONUP, None, None);
                }
                snapshot(hwnd)
                    .save(&output.join(format!("drag-{name}-final.png")))
                    .unwrap();
            }
            std::fs::write(output.join("drag-pixel-check.txt"), report.join("\n")).unwrap();
            std::fs::write(output.join("drag-latency.txt"), timing.join("\n")).unwrap();
            assert_eq!(failures, 0, "dragged surface differs from a clean repaint");
            ui_preferences::set_theme(ThemeMode::Dark).unwrap();
            controls::place(owner, 40, 40, 976, 639);
            controls::place(hwnd, 0, 0, 960, 600);
            state.command(hwnd, 30).unwrap();
        }
        let reflow_client = controls::rect(hwnd);
        let mut reflow_owner = RECT::default();
        unsafe { GetWindowRect(owner, &mut reflow_owner).unwrap(); }
        // Use DIP dimensions so this really crosses a column boundary at high DPI.
        controls::place(owner, 40, 40, state.dip(1296), state.dip(760));
        controls::place(hwnd, 0, 0, state.dip(1280), state.dip(720));
        // Reflow uses the current painted position as its origin and settles in 160 ms.
        state.command(hwnd, 30).unwrap();
        state.inspector_width = Some(264);
        state.layout(hwnd);
        state.stop_reflow();
        let reflow_index = state.grid_columns() - 1;
        let before_reflow = state.grid_rect(reflow_index);
        unsafe { SendMessageW(state.splitter, WM_LBUTTONDOWN, None, None); }
        state.inspector_width = Some(500);
        state.layout(hwnd);
        unsafe { SendMessageW(state.splitter, WM_LBUTTONUP, None, None); }
        let final_cell = state.grid_rect(reflow_index);
        if crate::platform::system_preferences::animations() {
            assert!(state.reflow.is_some());
            assert_eq!(state.visual_grid_rect(reflow_index), before_reflow);
            std::thread::sleep(Duration::from_millis(55));
            unsafe {
                SendMessageW(state.grid, WM_TIMER, Some(WPARAM(82)), None);
            }
            let middle = state.visual_grid_rect(reflow_index);
            assert_ne!(middle, before_reflow);
            assert_ne!(middle, final_cell);
            super::super::review_chrome_png(owner, &output.join("19-card-reflow-middle.png"));
            unsafe {
                SendMessageW(state.grid, WM_TIMER, Some(WPARAM(82)), None);
            }
            assert!(state.reflow.is_none());
            assert_eq!(state.visual_grid_rect(reflow_index), final_cell);
        }
        controls::place(owner, reflow_owner.left, reflow_owner.top,
            reflow_owner.right-reflow_owner.left, reflow_owner.bottom-reflow_owner.top);
        controls::place(hwnd, 0, 0, reflow_client.right, reflow_client.bottom);
        assert_eq!(
            unsafe { GetWindowLongPtrW(state.directory, GWL_EXSTYLE) }
                & WS_EX_COMPOSITED.0 as isize,
            WS_EX_COMPOSITED.0 as isize,
            "path and label share a composed resize frame"
        );
        state.inspector_width = Some(264);
        state.layout(hwnd);
        state.stop_reflow();
        let original_size = state.grid_rect(0);
        let selected = state.selected().unwrap().key();
        for _ in 0..4 {
            unsafe {
                SendMessageW(
                    state.grid,
                    WM_MOUSEWHEEL,
                    Some(WPARAM((120usize << 16) | 8)),
                    Some(LPARAM(0)),
                );
            }
        }
        assert_eq!(state.item_zoom, 140);
        assert!(state.grid_rect(0).right > original_size.right);
        assert_eq!(state.selected().unwrap().key(), selected);
        super::super::review_chrome_png(owner, &output.join("09-grid-zoom.png"));
        state.command(hwnd, 31).unwrap();
        unsafe {
            SendMessageW(
                state.list,
                WM_MOUSEWHEEL,
                Some(WPARAM((120usize << 16) | 8)),
                Some(LPARAM(0)),
            );
        }
        assert_eq!(
            unsafe { SendMessageW(state.list, LB_GETITEMHEIGHT, None, None) }.0,
            state.item_dip(58) as isize
        );
        super::super::review_chrome_png(owner, &output.join("10-list-zoom.png"));
        // Simulate splitter drag through its actual pointer message path.
        let mut drag = POINT {
            x: controls::rect(hwnd).right - state.dip(370) - state.dip(6),
            y: state.dip(160),
        };
        unsafe {
            let _ = ClientToScreen(hwnd, &raw mut drag);
            let _ = ScreenToClient(state.splitter, &mut drag);
            let position = LPARAM(((drag.y as u16 as u32) << 16 | drag.x as u16 as u32) as isize);
            SendMessageW(state.splitter, WM_LBUTTONDOWN, None, None);
            SendMessageW(state.splitter, WM_MOUSEMOVE, None, Some(position));
            SendMessageW(state.splitter, WM_LBUTTONUP, None, None);
        }
        assert!((state.inspector_width.unwrap() - 370).abs() <= 1);
        let g = state.geometry(hwnd);
        assert_eq!(
            controls::rect(state.preview).bottom,
            (g.preview.right - g.preview.left) * 2 / 3
        );
        super::super::review_chrome_png(owner, &output.join("11-resized-inspector.png"));
        unsafe {
            SendMessageW(
                state.inspector_body,
                WM_MOUSEWHEEL,
                Some(WPARAM(((-120i16) as u16 as usize) << 16)),
                None,
            );
        }
        assert!(state.inspector_scroll > 0);
        super::super::review_chrome_png(owner, &output.join("12-inspector-scroll.png"));
        crate::platform::app_menu::review::next_popup(output.join("16-context-menu.png"));
        state.context_menu(hwnd, state.list, LPARAM(-1));
        unsafe {
            SendMessageW(state.expand, BM_CLICK, None, None);
        }
        let mut pending = MSG::default();
        unsafe {
            while PeekMessageW(
                &raw mut pending,
                Some(hwnd),
                WM_COMMAND,
                WM_COMMAND,
                PM_REMOVE,
            )
            .as_bool()
            {
                DispatchMessageW(&pending);
            }
        }
        let popup = state.popup.take().expect("expand button opens preview");
        library_popup_frame::review(popup, 46);
        let popup_preview = library_preview::review_preview(popup);
        pump_until(hwnd, |_| {
            unsafe {
                SendMessageW(popup_preview, WM_TIMER, Some(WPARAM(93)), None);
            }
            asset_preview::review_loaded(popup_preview)
        });
        super::super::review_chrome_png(popup, &output.join("13-popup-image.png"));
        let message = MSG {
            hwnd: popup,
            message: WM_KEYDOWN,
            wParam: WPARAM(27),
            ..Default::default()
        };
        assert!(library_preview::dialog_message(&message));
        unsafe {
            let _ = DestroyWindow(popup);
        }
        for (name, mode) in [
            ("17-file-information-dark", ThemeMode::Dark),
            ("18-file-information-light", ThemeMode::Light),
        ] {
            ui_preferences::set_theme(mode).unwrap();
            let information = library_information::show(hwnd, state.selected().unwrap()).unwrap();
            library_popup_frame::review(information, 48);
            library_information::review_read_only(information);
            super::super::review_chrome_png(information, &output.join(format!("{name}.png")));
            unsafe {
                let _ = DestroyWindow(information);
            }
        }
        ui_preferences::set_theme(ThemeMode::Dark).unwrap();
        if let Some(source) = std::env::var_os("PANZO_ASSET_TEST_MP4") {
            let source = PathBuf::from(source);
            asset_index::register(&library, &source).unwrap();
            state.reload();
            pump_until(hwnd, |s| s.index_rx.is_none());
            let state = unsafe { &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut History) };
            state.command(hwnd, 42).unwrap();
            assert_eq!(state.visible.len(), 1);
            state.inspector_scroll = 0;
            state.layout_inspector(hwnd);
            let preview = state.preview;
            pump_until(hwnd, |_| {
                unsafe {
                    SendMessageW(preview, WM_TIMER, Some(WPARAM(93)), None);
                }
                asset_preview::review_playback(preview).0
            });
            asset_preview::toggle(preview);
            super::super::review_present(owner);
            unsafe {
                SendMessageW(hwnd, WM_TIMER, Some(WPARAM(81)), None);
                let _ = ValidateRect(Some(state.inspector_body), None);
                let _ = ValidateRect(Some(hwnd), None);
            }
            let mut body_invalidations = 0;
            for _ in 0..45 {
                std::thread::sleep(Duration::from_millis(20));
                unsafe {
                    SendMessageW(preview, WM_TIMER, Some(WPARAM(93)), None);
                    SendMessageW(hwnd, WM_TIMER, Some(WPARAM(81)), None);
                    if GetUpdateRect(state.inspector_body, None, false).as_bool() {
                        body_invalidations += 1;
                        let _ = ValidateRect(Some(state.inspector_body), None);
                    }
                }
            }
            assert!(
                asset_preview::review_playback(preview).1 > 5_000_000,
                "video playback did not advance"
            );
            assert!(
                body_invalidations <= 1,
                "metadata invalidated {body_invalidations} times during playback"
            );
            super::super::review_chrome_png(owner, &output.join("14-video-playing.png"));
            asset_preview::pause(preview);
            unsafe {
                SendMessageW(
                    hwnd,
                    WM_COMMAND,
                    Some(WPARAM(2 | ((LBN_DBLCLK as usize) << 16))),
                    None,
                );
            }
            let popup = state.popup.take().expect("double click opens preview");
            library_popup_frame::review(popup, 46);
            let popup_preview = library_preview::review_preview(popup);
            pump_until(hwnd, |_| {
                unsafe {
                    SendMessageW(popup_preview, WM_TIMER, Some(WPARAM(93)), None);
                }
                asset_preview::review_playback(popup_preview).0
            });
            super::super::review_chrome_png(popup, &output.join("15-popup-video.png"));
            unsafe {
                let _ = DestroyWindow(popup);
            }
        }
        // Recycle only a newly generated disposable fixture, never any user material.
        let disposable = library.join("delete-test.png");
        std::fs::write(&disposable, b"disposable").unwrap();
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
        }
        recycle_source(owner, &disposable).unwrap();
        assert!(!disposable.exists());
        let output_dir = output.join("custom-exports");
        let material_dir = output.join("custom-materials");
        ui_preferences::set_directories(material_dir.clone(), output_dir.clone()).unwrap();
        ui_preferences::set_theme(ThemeMode::Light).unwrap();
        assert_eq!(ui_preferences::output_directory(), Some(output_dir.clone()));
        assert_eq!(
            ui_preferences::library_directory(),
            Some(material_dir.clone())
        );
        let persisted: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
                    .join("Panzo/settings.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            persisted["output_directory"],
            output_dir.to_string_lossy().as_ref()
        );
        assert_eq!(
            persisted["library_directory"],
            material_dir.to_string_lossy().as_ref()
        );
        ui_preferences::set_theme(original).unwrap();
        unsafe {
            let _ = DestroyWindow(owner);
            let _ = SetThreadDpiAwarenessContext(old_dpi);
        }
    }
    #[test]
    fn selecting_preview_retains_list_state_and_loads_thumbnails_beyond_120() {
        let library =
            std::env::temp_dir().join(format!("panzo-history-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&library).unwrap();
        for i in 0..145 {
            image::RgbaImage::from_pixel(12, 8, image::Rgba([i as u8, 32, 96, 255]))
                .save(library.join(format!("image-{i:03}.png")))
                .unwrap();
        }
        let module = unsafe { GetModuleHandleW(None) }.unwrap();
        let owner = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("isolated asset test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                1200,
                760,
                None,
                None,
                Some(module.into()),
                None,
            )
        }
        .unwrap();
        let documents_before = super::super::PICTURES.with(|pictures| pictures.borrow().len());
        let hwnd = create_embedded(owner, owner, &library).unwrap();
        controls::place(hwnd, 0, 0, 1200, 700);
        pump_until(hwnd, |s| s.index_rx.is_none() && s.visible.len() == 145);
        let state = unsafe { &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut History) };
        state.grid_mode = false;
        state.layout(hwnd);
        unsafe {
            SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(130)), None);
            SendMessageW(state.list, LB_SETTOPINDEX, Some(WPARAM(125)), None);
        }
        state.selection();
        state.request_thumbs();
        let selected = state.selected().unwrap().key();
        // Wheel, precision-wheel accumulation, and thumb tracking retain selection.
        controls::place(owner, 40, 40, 1240, 800);
        super::super::review_present(owner);
        library_scroll_review::run(hwnd, state);
        state.command(hwnd, 31).unwrap();
        unsafe { SendMessageW(state.list, LB_SETTOPINDEX, Some(WPARAM(125)), None); }
        LIST_PAINTS.with(|v| v.set(Some((0, 0))));
        let wheel = |h: HWND, delta: i16| unsafe {
            SendMessageW(
                h,
                WM_MOUSEWHEEL,
                Some(WPARAM((delta as u16 as usize) << 16)),
                None,
            )
        };
        wheel(state.list, -120);
        assert_eq!(state.review_scrollbar(state.list).2, 128);
        wheel(state.list, 120);
        for _ in 0..4 {
            wheel(state.list, -30);
        }
        assert_eq!(state.review_scrollbar(state.list).2, 128);
        for surface in [state.list, state.grid] {
            if surface == state.grid {
                state.command(hwnd, 30).unwrap();
            }
            let (thumb, maximum, _) = state.review_scrollbar(surface);
            assert!(maximum > 0);
            let at = |x: i32, y: i32| LPARAM(((y as u16 as u32) << 16 | x as u16 as u32) as isize);
            unsafe {
                SendMessageW(
                    surface,
                    WM_LBUTTONDOWN,
                    None,
                    Some(at(thumb.left + 1, thumb.top + 2)),
                );
                SendMessageW(
                    surface,
                    WM_MOUSEMOVE,
                    None,
                    Some(at(thumb.left + 1, controls::rect(surface).bottom + 100)),
                );
                SendMessageW(surface, WM_LBUTTONUP, None, None);
            }
            assert_eq!(
                state.review_scrollbar(surface).2,
                maximum,
                "thumb reaches final item"
            );
            wheel(surface, 120);
            assert!(
                state.review_scrollbar(surface).2 < maximum,
                "wheel moves away from the end"
            );
            assert_eq!(state.selected().unwrap().key(), selected);
        }
        let (painted, direct) = LIST_PAINTS.with(|v| v.replace(None).unwrap());
        assert!(painted > 0, "scroll redraws the visible list");
        assert_eq!(
            direct, 0,
            "scrolling rows and scrollbar must only paint to the shared buffer"
        );
        state.command(hwnd, 31).unwrap();
        unsafe {
            SendMessageW(state.list, LB_SETTOPINDEX, Some(WPARAM(125)), None);
        }
        let preview = state.preview;
        assert_eq!(
            unsafe { GetWindowLongPtrW(state.list, GWL_STYLE) } & WS_VSCROLL.0 as isize,
            0,
            "the native white scrollbar must remain removed after filling the list"
        );
        let key = match state.selected().unwrap() {
            Entry::File(asset) => ThumbKey::from(&asset),
            _ => panic!("file"),
        };
        pump_until(hwnd, |s| s.thumbs.contains_key(&key));
        let state = unsafe { &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut History) };
        state.command(hwnd, 42).unwrap();
        assert!(state.visible.is_empty());
        assert!(
            !unsafe {
                windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(state.buttons[1])
            }
            .as_bool()
        );
        state.command(hwnd, 40).unwrap();
        assert_eq!(state.selected().unwrap().key(), selected);
        assert_eq!(
            unsafe { SendMessageW(state.list, LB_GETTOPINDEX, None, None) }.0,
            125
        );
        state.command(hwnd, 15).unwrap();
        let popup = state.popup.take().unwrap();
        assert!(unsafe { IsWindow(Some(popup)) }.as_bool());
        unsafe {
            let _ = DestroyWindow(popup);
        }
        state.reload();
        pump_until(hwnd, |s| s.index_rx.is_none());
        let state = unsafe { &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut History) };
        assert_eq!(state.selected().unwrap().key(), selected);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !asset_preview::review_loaded(preview) {
            unsafe {
                SendMessageW(preview, WM_TIMER, Some(WPARAM(93)), None);
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            super::super::PICTURES.with(|pictures| pictures.borrow().len()),
            documents_before,
            "preview must not create an image document"
        );
        if let Some(path) = std::env::var_os("PANZO_ASSET_REVIEW_PNG") {
            unsafe {
                let _ = ShowWindow(owner, SW_SHOW);
            }
            super::super::review_chrome_png(owner, &PathBuf::from(path));
        }
        unsafe {
            let _ = DestroyWindow(owner);
        }
        std::thread::sleep(Duration::from_millis(100));
        std::fs::remove_dir_all(library).unwrap();
    }
}

fn relocate_dialog(owner: HWND, kind: Kind) -> Option<PathBuf> {
    if kind == Kind::Project {
        return crate::platform::recorder_window::choose_project_path(owner);
    }
    use std::os::windows::ffi::OsStringExt;
    use windows::{
        Win32::UI::Controls::Dialogs::*,
        core::{PCWSTR, PWSTR},
    };
    let filter = controls::wide(
        "支持的素材\0*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.mp4;*.wav;*.mp3;*.m4a;*.aac;*.flac\0所有文件\0*.*\0\0",
    );
    let title = controls::wide("重新定位素材");
    let mut path = vec![0u16; 32768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrTitle: PCWSTR(title.as_ptr()),
        lpstrFile: PWSTR(path.as_mut_ptr()),
        nMaxFile: path.len() as u32,
        Flags: OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut dialog) }.as_bool() {
        return None;
    }
    let end = path.iter().position(|c| *c == 0)?;
    Some(std::ffi::OsString::from_wide(&path[..end]).into())
}
