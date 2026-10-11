#![allow(clippy::cast_possible_wrap)] // Control indexes are bounded by small static field lists.
//! Image documents and OCR results remain attached while other workspaces are visible.
use super::editor_ui::IconKind as I;
use super::image_icons as P;
use super::{
    capture_tools::{self, Raster},
    image_tasks::Output,
    workspace::{self, Target},
    workspace_ui as ui,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::WindowsAndMessaging::{
            CB_ADDSTRING, CB_DELETESTRING, CB_SETCURSEL, CBN_SELCHANGE, DefWindowProcW,
            GWLP_USERDATA, GetWindowLongPtrW, IsWindow, SendMessageW, SetWindowLongPtrW, WM_CLOSE,
            WM_COMMAND, WM_NCDESTROY, WM_SIZE,
        },
    },
    core::w,
};
struct Document {
    id: u64,
    hwnd: HWND,
    text: String,
    status: String,
    cancel: Option<Arc<AtomicBool>>,
    tab: HWND,
    close: HWND,
    source: Option<PathBuf>,
    prompt: String,
    mode: usize,
    language: usize,
    task: usize,
    result_revision: Option<u64>,
}
impl Drop for Document {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.tab);
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.close);
        }
    }
}

struct State {
    library: PathBuf,
    inspector: bool,
    collapsed: bool,
    inspector_hidden: bool,
    inspector_width: Option<i32>,
    context_state: Option<(u64, bool, i32)>,
    separator: HWND,
    panel_title: HWND,
    result_title: HWND,
    mode_buttons: [HWND; 2],
    panel_toggle: HWND,
    save: HWND,
    more: HWND,
    fit: HWND,
    undo: HWND,
    empty_open: HWND,
    empty_capture: HWND,
    documents: Vec<Document>,
    pending_sources: std::collections::HashMap<isize, PathBuf>,
    next_id: u64,
    selected: Option<usize>,
    chooser: HWND,
    toolbar: Vec<HWND>,
    empty: HWND,
    mode: HWND,
    language: HWND,
    languages: Vec<String>,
    task: HWND,
    prompt: HWND,
    service: HWND,
    result: HWND,
    status: HWND,
    recognize: HWND,
    cancel: HWND,
    copy: HWND,
    settings: HWND,
}
pub(crate) fn create(parent: HWND, library: &Path) -> Result<HWND, String> {
    ui::register(w!("PanzoImageWorkspace"), Some(procedure))?;
    let h = ui::panel(parent, w!("PanzoImageWorkspace"))?;
    ui::register(w!("PanzoImageSeparator"), Some(separator_proc))?;
    let languages = super::ocr::languages().unwrap_or_default();
    let tags = languages.iter().map(|v| v.tag.clone()).collect();
    let names = std::iter::once("自动（用户语言）".to_owned())
        .chain(languages.iter().map(|v| format!("{} ({})", v.name, v.tag)))
        .collect::<Vec<_>>();
    let online = super::vision_settings::load().is_ok_and(|s| s.default_online && s.has_key());
    let s = State {
        library: library.into(),
        inspector: false,
        collapsed: true,
        inspector_hidden: false,
        inspector_width: None,
        context_state: None,
        separator: ui::panel(h, w!("PanzoImageSeparator"))?,
        panel_title: ui::label(h, "文字识别", 33)?,
        result_title: ui::label(h, "识别结果 · 可直接校对", 34)?,
        mode_buttons: [
            ui::button(h, "文字 OCR", 15)?,
            ui::button(h, "多模态识图", 16)?,
        ],
        panel_toggle: ui::button(h, "展开 / 收起属性面板", 12)?,
        save: ui::button(h, "保存图片", 13)?,
        more: ui::button(h, "工作区菜单", 17)?,
        fit: ui::button(h, "居中并适应窗口", 19)?,
        undo: ui::button(h, "撤销 · Ctrl+Z", 35)?,
        empty_open: ui::button(h, "打开图片", 2)?,
        empty_capture: ui::button(h, "区域截图", 1)?,

        documents: Vec::new(),
        pending_sources: std::collections::HashMap::new(),
        next_id: 1,
        selected: None,
        chooser: ui::combo(h, &[], 0, 20)?,
        toolbar: [
            "区域截图",
            "打开",
            "长截图",
            "拼图",
            "批处理",
            "关闭",
            "图片插件",
        ]
        .iter()
        .enumerate()
        .map(|(i, s)| ui::button(h, s, if i == 6 { 11 } else { 1 + i }))
        .collect::<Result<_, _>>()?,
        empty: ui::label(
            h,
            "截图或打开图片\r\n\r\n在这里标注、裁剪、识别文字或理解图片。\r\n切换功能标签会保留当前内容。",
            30,
        )?,
        mode: ui::combo(h, &["离线 OCR", "多模态识图"], usize::from(online), 21)?,
        language: ui::combo(
            h,
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
            0,
            22,
        )?,
        languages: tags,
        task: ui::combo(
            h,
            &["提取文字", "提取表格", "描述图片", "自定义提问"],
            0,
            23,
        )?,
        prompt: ui::edit(h, "", 24, true)?,
        service: ui::label(h, "补充要求 / 提问（可选）", 31)?,
        result: ui::edit(h, "", 25, true)?,
        status: ui::label(h, "识别结果将显示在此处，可编辑和复制。", 32)?,
        recognize: ui::button(h, "开始识别", 7)?,
        cancel: ui::button(h, "取消识别", 8)?,
        copy: ui::button(h, "复制结果", 9)?,
        settings: ui::button(h, "服务设置", 10)?,
    };
    for (b, icon) in s.toolbar.iter().zip([
        P::CAPTURE,
        P::FOLDER,
        I::ScrollCapture,
        I::Collage,
        I::Batch,
        I::Close,
        I::Diagnostics,
    ]) {
        super::desktop_ui::style(*b, Some(icon), true, false, true);
    }
    super::desktop_ui::style(s.recognize, None, false, true, false);
    super::desktop_ui::style(s.save, Some(P::SAVE), true, false, true);
    super::desktop_ui::style(s.undo, Some(P::UNDO), true, false, true);
    super::desktop_ui::style(s.fit, Some(P::FIT), true, false, true);
    super::desktop_ui::style(s.more, Some(P::MENU), true, false, true);
    super::desktop_ui::style(s.panel_toggle, Some(P::PANEL), true, false, true);
    for (button, icon) in [
        (s.settings, I::Settings),
        (s.copy, I::Copy),
        (s.empty_open, I::Folder),
        (s.empty_capture, I::Capture),
    ] {
        super::desktop_ui::style(button, Some(icon), true, false, true);
    }
    unsafe {
        SendMessageW(
            s.result,
            windows::Win32::UI::Controls::EM_SETLIMITTEXT,
            Some(WPARAM(1_000_000)),
            None,
        );
        SetWindowLongPtrW(h, GWLP_USERDATA, Box::into_raw(Box::new(s)) as isize);
    }
    Ok(h)
}
unsafe fn state(h: HWND) -> Option<&'static mut State> {
    unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State).as_mut() }
}
pub(crate) fn add(panel: HWND, image: HWND, select: bool) {
    if let Some(s) = unsafe { state(panel) } {
        s.save_text();
        let id = s.next_id;
        s.next_id += 1;
        let tab = ui::button(panel, &format!("图片 {id}"), 100).unwrap_or_default();
        super::desktop_ui::style(tab, Some(P::IMAGE), false, false, false);
        let close = ui::button(panel, "关闭图片", 101).unwrap_or_default();
        super::desktop_ui::style(close, Some(P::CLOSE), true, false, true);
        s.documents.push(Document {
            id,
            hwnd: image,
            text: String::new(),
            status: String::new(),
            cancel: None,
            tab,
            close,
            source: s.pending_sources.remove(&(image.0 as isize)),
            prompt: String::new(),
            mode: ui::selection(s.mode),
            language: 0,
            task: 0,
            result_revision: None,
        });
        let name = ui::wide(&format!("图片 {id}"));
        unsafe {
            SendMessageW(
                s.chooser,
                CB_ADDSTRING,
                None,
                Some(LPARAM(name.as_ptr() as isize)),
            );
        }
        if select || s.selected.is_none() {
            s.select(panel, s.documents.len() - 1);
        } else {
            s.layout(panel);
        }
    }
}
pub(crate) fn progress(panel: HWND, id: u64, text: &str) {
    if let Some(s) = unsafe { state(panel) } {
        if let Some(d) = s.documents.iter_mut().find(|d| d.id == id) {
            d.status = text.into();
        }
        s.refresh();
    }
}
pub(crate) fn complete(panel: HWND, id: u64, revision: u64, result: Result<Output, String>) {
    if let Some(s) = unsafe { state(panel) } {
        s.save_text();
        let Some(index) = s.documents.iter().position(|d| d.id == id) else {
            return;
        };
        let d = &mut s.documents[index];
        d.cancel = None;
        match result {
            Ok(Output::Text(text)) => {
                d.result_revision = Some(revision);
                d.text = text;
                d.status = format!("图片 {id} · 版本 {revision} 的识别结果（可编辑）");
            }
            Err(e) => {
                d.status = if d.text.is_empty() {
                    e
                } else {
                    format!("{e}\r\n下方保留上次识别结果，可重试或切换识别方式。")
                }
            }
            Ok(Output::Image(image)) => {
                let path = self::workspace::result_image(image, &s.library);
                d.status = match path {
                    Ok(_) => format!("图片 {id} · 版本 {revision} 处理完成，输出已加入图片列表"),
                    Err(e) => e,
                };
            }
        }
        if s.selected == Some(index) {
            ui::text(s.result, &s.documents[index].text);
        }
        s.refresh();
    }
}
impl State {
    fn save_text(&mut self) {
        if let Some(i) = self.selected {
            self.documents[i].text = ui::value(self.result);
            self.documents[i].prompt = ui::value(self.prompt);
            self.documents[i].mode = ui::selection(self.mode);
            self.documents[i].language = ui::selection(self.language);
            self.documents[i].task = ui::selection(self.task);
        }
    }
    fn select(&mut self, h: HWND, i: usize) {
        self.save_text();
        if i >= self.documents.len() {
            return;
        }
        self.selected = Some(i);
        ui::text(self.prompt, &self.documents[i].prompt);
        unsafe {
            for (control, value) in [
                (self.mode, self.documents[i].mode),
                (self.language, self.documents[i].language),
                (self.task, self.documents[i].task),
            ] {
                SendMessageW(control, CB_SETCURSEL, Some(WPARAM(value)), None);
            }
            SendMessageW(self.chooser, CB_SETCURSEL, Some(WPARAM(i)), None);
        }
        ui::text(self.result, &self.documents[i].text);
        self.layout(h);
        self.refresh();
    }
    fn refresh(&self) {
        for i in [3, 4] {
            unsafe {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                    self.toolbar[i],
                    super::plugins::enabled(super::plugins::IMAGES),
                );
            }
        }
        let d = self.selected.map(|i| &self.documents[i]);
        ui::text(
            self.status,
            d.map_or("截图或打开图片后开始识别。", |d| {
                if d.status.is_empty() {
                    "结果将显示在此处"
                } else {
                    d.status.as_str()
                }
            }),
        );
        let busy = d.is_some_and(|d| d.cancel.is_some());
        let ocr = d.is_some() && !self.collapsed && !self.inspector_hidden;
        let has_result = !ui::value(self.result).is_empty();
        ui::show(self.result, ocr && has_result);
        ui::show(self.status, ocr && !has_result);
        let stale = d.is_some_and(|d| {
            d.result_revision.is_some_and(|v| {
                capture_tools::workspace_metadata(d.hwnd)
                    .is_some_and(|(_, _, current)| current != v)
            })
        });
        ui::text(
            self.result_title,
            if stale {
                "识别结果 · 图片已修改，请重新识别"
            } else if busy && d.is_some_and(|d| !d.text.is_empty()) {
                "识别中 · 下方为上次结果"
            } else {
                "识别结果 · 可直接校对"
            },
        );
        unsafe {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                self.recognize,
                d.is_some()
                    && !busy
                    && super::plugins::enabled(if ui::selection(self.mode) == 0 {
                        super::plugins::OCR
                    } else {
                        super::plugins::VISION
                    }),
            );
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(self.cancel, busy);
        }
    }
    #[allow(clippy::too_many_lines, clippy::many_single_char_names)] // Persistent controls share one DPI-aware layout pass.
    fn layout(&self, h: HWND) {
        let _batch = ui::layout_batch(h);
        let r = ui::rect(h);
        let d = |v| ui::scale(h, v);
        let width = self.inspector_width.unwrap_or_else(|| {
            super::workbench_layout::inspector(
                r.right * 96 / super::editor_ui::window_dpi(h) as i32,
            )
        });
        let side = if self.selected.is_some() && !self.inspector && !self.inspector_hidden {
            d(width)
        } else {
            0
        };
        let x = r.right - side;
        let context_right = if side == 0 { r.right - d(40) } else { x };
        let ocr = side > 0 && !self.collapsed;
        for b in &self.toolbar {
            ui::show(*b, false);
        }
        ui::show(self.more, true);
        ui::place(self.more, d(8), d(6), d(28), d(28));
        ui::show(self.fit, self.selected.is_some());
        ui::place(self.fit, context_right - d(40), d(6), d(28), d(28));
        ui::place(self.panel_toggle, r.right - d(40), d(6), d(28), d(28));
        ui::show(self.panel_toggle, self.selected.is_some());
        ui::show(self.save, false);
        let mut capacity = usize::try_from(((context_right - d(300)) / d(210)).max(1)).unwrap_or(1);
        if self.documents.len() > capacity {
            capacity = usize::try_from(((context_right - d(440)) / d(210)).max(1)).unwrap_or(1);
        }
        let first = self.selected.unwrap_or(0).saturating_sub(capacity - 1);
        let count = self.documents.len().min(capacity);
        let action_x = d(100) + d(210) * count as i32;
        for (index, b) in self.toolbar.iter().take(2).enumerate() {
            ui::show(*b, true);
            ui::place(*b, action_x + d(36) * index as i32, d(6), d(30), d(28));
        }
        ui::show(self.undo, self.selected.is_some());
        ui::place(self.undo, action_x + d(91), d(5), d(30), d(30));
        ui::place(self.chooser, context_right - d(216), d(6), d(112), d(28));
        ui::show(self.chooser, self.documents.len() > capacity);
        for (i, doc) in self.documents.iter().enumerate() {
            let visible = self.selected == Some(i);
            capture_tools::workspace_inspector(
                doc.hwnd,
                if ocr || self.inspector || self.inspector_hidden {
                    0
                } else {
                    width
                },
            );
            ui::place(
                doc.hwnd,
                0,
                d(40),
                if ocr { x } else { r.right },
                (r.bottom - d(40)).max(1),
            );
            ui::show(doc.hwnd, visible);
            ui::place(
                doc.tab,
                d(92) + d(210) * i.saturating_sub(first) as i32,
                d(6),
                d(180),
                d(30),
            );
            ui::place(
                doc.close,
                d(274) + d(210) * i.saturating_sub(first) as i32,
                d(8),
                d(22),
                d(24),
            );
            ui::show(doc.close, i >= first && i < first + capacity);
            ui::show(doc.tab, i >= first && i < first + capacity);
            super::desktop_ui::selected(doc.tab, visible);
        }
        ui::show(self.empty_open, self.selected.is_none());
        ui::show(self.empty_capture, self.selected.is_none());
        ui::place(
            self.empty_open,
            x / 2 - d(52),
            r.bottom / 2 + d(20),
            d(44),
            d(36),
        );
        ui::place(
            self.empty_capture,
            x / 2 + d(8),
            r.bottom / 2 + d(20),
            d(44),
            d(36),
        );
        for c in [
            self.empty,
            self.mode,
            self.mode_buttons[0],
            self.mode_buttons[1],
        ] {
            ui::show(c, false);
        }
        let online = ui::selection(self.mode) == 1;
        for c in [
            self.panel_title,
            self.result_title,
            self.settings,
            self.result,
            self.status,
            self.recognize,
            self.copy,
        ] {
            ui::show(c, ocr);
        }
        let busy = self
            .selected
            .is_some_and(|i| self.documents[i].cancel.is_some());
        ui::show(self.cancel, ocr && busy);
        for b in self.mode_buttons {
            ui::show(b, ocr);
        }
        ui::show(self.settings, ocr && online);
        ui::show(self.language, ocr && !online);
        for c in [self.task, self.service, self.prompt] {
            ui::show(c, ocr && online);
        }
        ui::text(self.panel_title, "识别方式");
        ui::show(self.panel_title, false);
        for (i, b) in self.mode_buttons.iter().enumerate() {
            super::desktop_ui::style(*b, None, false, false, false);
            super::desktop_ui::selected(*b, i == usize::from(online));
            let width = (side - d(41)) / 2;
            ui::place(
                *b,
                x + d(19) + (width + d(3)) * i as i32,
                d(88),
                width,
                d(27),
            );
        }
        ui::place(self.language, x + side - d(188), d(180), d(172), d(30));
        ui::place(self.task, x + side - d(188), d(180), d(172), d(30));
        ui::text(self.service, "自定义模型");
        ui::place(self.prompt, x + d(16), d(220), side - d(32), d(76));
        ui::place(self.service, x + d(16), d(303), d(100), d(26));
        super::desktop_ui::style(self.settings, None, true, false, false);
        ui::place(self.settings, r.right - d(92), d(303), d(76), d(26));
        let result_y = if online { 375 } else { 255 };
        ui::show(self.result_title, false);
        let has_result = self
            .selected
            .is_some_and(|i| !self.documents[i].text.is_empty());
        ui::show(self.result, ocr && has_result);
        ui::show(self.status, ocr && !has_result);
        if !busy && !has_result {
            ui::text(self.status, "结果将显示在此处");
        }
        ui::place(
            self.status,
            x + d(16),
            d(result_y + 32),
            side - d(32),
            d(40),
        );
        ui::place(
            self.result,
            x + d(16),
            d(result_y + 32),
            side - d(32),
            (r.bottom - d(result_y + 102)).max(d(60)),
        );
        let button_width = side - d(if busy { 130 } else { 70 });
        ui::place(
            self.recognize,
            x + d(16),
            r.bottom - d(42),
            button_width,
            d(30),
        );
        ui::place(self.cancel, r.right - d(98), r.bottom - d(42), d(44), d(30));
        super::desktop_ui::style(self.copy, Some(P::COPY), true, false, true);
        ui::place(self.copy, r.right - d(46), r.bottom - d(42), d(30), d(30));
        ui::place(
            self.separator,
            x - d(3),
            d(40),
            d(6),
            (r.bottom - d(40)).max(1),
        );
        ui::show(self.separator, side > 0);
        if side > 0 {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                    self.separator,
                    Some(windows::Win32::UI::WindowsAndMessaging::HWND_TOP),
                    0,
                    0,
                    0,
                    0,
                    windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
                );
            }
        }
        if !INSPECTOR_DRAG.with(|v|v.get().is_some()) {
            self.refresh();
            super::desktop_ui::invalidate(h);
        }
    }
    fn paint(&self, h: HWND, dc: windows::Win32::Graphics::Gdi::HDC) {
        use super::{
            desktop_ui as v,
            editor_ui::{self, EditorTheme},
        };
        let r = ui::rect(h);
        let t = EditorTheme::current(editor_ui::window_dpi(h));
        let d = |n| t.scale(n);
        let side = if self.selected.is_some() && !self.inspector && !self.inspector_hidden {
            d(self
                .inspector_width
                .unwrap_or_else(|| super::workbench_layout::inspector(r.right * 96 / t.dpi as i32)))
        } else {
            0
        };
        let x = r.right - side;
        let context_right = if side == 0 { r.right - d(40) } else { x };
        editor_ui::fill_rect(dc, &r, t.palette.panel);
        v::line(dc, v::rect(0, d(39), r.right, 1), t);
        if let Some(i) = self.selected {
            let doc = &self.documents[i];
            v::text(
                dc,
                v::rect(d(42), d(6), d(48), d(28)),
                if capture_tools::workspace_dirty(doc.hwnd) {
                    "未保存"
                } else {
                    "已保存"
                },
                t.palette.text_muted,
                t.dpi,
                false,
            );
            v::text(
                dc,
                v::rect(context_right - d(94), d(6), d(48), d(28)),
                &format!("{:.0}%", capture_tools::workspace_zoom(doc.hwnd)),
                t.palette.text_secondary,
                t.dpi,
                true,
            );
        }
        if self.selected.is_some() {
            let mut capacity = ((context_right - d(300)) / d(210)).max(1) as usize;
            if self.documents.len() > capacity {
                capacity = ((context_right - d(440)) / d(210)).max(1) as usize;
            }
            let action_x = d(100) + d(210) * self.documents.len().min(capacity) as i32;
            v::line(dc, v::rect(action_x + d(80), d(11), 1, d(18)), t);
        }
        if side > 0 {
            v::line(dc, v::rect(x, 0, 1, r.bottom), t);
            v::text(
                dc,
                v::rect(x + d(16), d(6), side - d(60), d(28)),
                "工具与属性",
                t.palette.text_secondary,
                t.dpi,
                false,
            );
        }
        if side > 0 && !self.collapsed {
            let online = ui::selection(self.mode) == 1;
            let heading = |y, label: &str| {
                v::text_style(
                    dc,
                    v::rect(x + d(16), d(y), side - d(32), d(20)),
                    label,
                    t.palette.text,
                    t.dpi,
                    false,
                    editor_ui::TextStyle::Heading,
                )
            };
            heading(54, "识别方式");
            editor_ui::rounded_surface(
                dc,
                v::rect(x + d(16), d(85), side - d(32), d(33)),
                t.palette.raised,
                t.palette.raised,
                d(4),
            );
            v::line(dc, v::rect(x + d(16), d(133), side - d(32), 1), t);
            heading(
                147,
                if online {
                    "识图任务"
                } else {
                    "识别语言"
                },
            );
            v::text(
                dc,
                v::rect(x + d(16), d(180), d(72), d(30)),
                if online { "任务" } else { "语言" },
                t.palette.text_secondary,
                t.dpi,
                false,
            );
            let y = if online { 375 } else { 255 };
            v::line(dc, v::rect(x + d(16), d(y - 14), side - d(32), 1), t);
            heading(y, "识别结果");
            v::line(dc, v::rect(x, r.bottom - d(54), side, 1), t);
        }
        if self.selected.is_none() {
            editor_ui::fill_rect(dc, &v::rect(0, d(40), x, r.bottom - d(40)), v::canvas(t));
            v::text(
                dc,
                v::rect(0, r.bottom / 2 - d(48), x, d(32)),
                "尚未打开图片",
                t.palette.text,
                t.dpi,
                true,
            );
        }
    }
    fn recognize(&mut self) -> Result<(), String> {
        let i = self.selected.ok_or("请先截图或打开图片")?;
        if self.documents[i].cancel.is_some() {
            return Err("当前图片正在识别，请等待或取消".into());
        }
        let (image, revision) = capture_tools::workspace_snapshot(self.documents[i].hwnd)?;
        let id = self.documents[i].id;
        let target = Target::Image { id, revision };
        let lease = super::plugins::acquire(if ui::selection(self.mode) == 0 {
            super::plugins::OCR
        } else {
            super::plugins::VISION
        })?;
        let token = if ui::selection(self.mode) == 0 {
            let lang = ui::selection(self.language)
                .checked_sub(1)
                .and_then(|i| self.languages.get(i).cloned());
            workspace::start_job(target, "离线 OCR", move |cancel, _| {
                let _lease = lease;
                let text = super::ocr::recognize(&image, lang.as_deref())?;
                if cancel.load(Ordering::Acquire) {
                    Err("识别已取消".into())
                } else {
                    Ok(Output::Text(text))
                }
            })
        } else {
            let settings = super::vision_settings::load()?;
            settings.validate()?;
            if !settings.has_key() {
                return Err("请在设置窗口配置视觉模型和 API Key，保存后返回此处识别。".into());
            }
            let preset = match ui::selection(self.task) {
                1 => super::vision::TABLE_PROMPT,
                2 => super::vision::DESCRIBE_PROMPT,
                3 => "",
                _ => super::vision::OCR_PROMPT,
            };
            let question = ui::value(self.prompt);
            let prompt = [preset, question.trim()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if prompt.is_empty() {
                return Err("请输入要询问的内容".into());
            }
            workspace::start_job(target, "多模态识图", move |cancel, _| {
                let _lease = lease;
                super::vision::recognize(&settings, &image, &prompt, &cancel).map(Output::Text)
            })
        };
        self.documents[i].cancel = Some(token);
        self.documents[i].status =
            format!("正在识别图片 {id} · 版本 {revision}，可切换功能继续工作…");
        self.refresh();
        Ok(())
    }
    #[allow(clippy::too_many_lines)] // Workspace command dispatch preserves existing task ownership.
    fn command(&mut self, h: HWND, id: usize) -> Result<(), String> {
        match id {
            1 => workspace::video_command(1),
            2 => {
                let paths = super::image_workflows::choose_images(h, true, true)?;
                for path in paths {
                    let _ = super::asset_index::register(&self.library, &path);
                    open_source(&path, &self.library)?;
                }
            }
            3 => workspace::video_command(21),
            4 => super::workspace_image_tools::open(false, &self.library)?,
            5 => super::workspace_image_tools::open(true, &self.library)?,
            6 => {
                if let Some(i) = self.selected {
                    let doc = &self.documents[i];
                    unsafe {
                        SendMessageW(doc.hwnd, WM_CLOSE, None, None);
                    }
                    if !unsafe { IsWindow(Some(doc.hwnd)) }.as_bool() {
                        if let Some(c) = &doc.cancel {
                            c.store(true, Ordering::Release);
                        }
                        self.documents.remove(i);
                        unsafe {
                            SendMessageW(self.chooser, CB_DELETESTRING, Some(WPARAM(i)), None);
                        }
                        self.selected = None;
                        if self.documents.is_empty() {
                            ui::text(self.result, "");
                            self.layout(h);
                        } else {
                            self.select(h, i.min(self.documents.len() - 1));
                        }
                    }
                }
            }
            7 => self.recognize()?,
            8 => {
                if let Some(c) = self
                    .selected
                    .and_then(|i| self.documents[i].cancel.as_ref())
                {
                    c.store(true, Ordering::Release);
                }
            }
            9 => capture_tools::copy_text(h, &ui::value(self.result))?,
            10 => super::workspace_settings::open()?,
            12 => {
                self.inspector_hidden = !self.inspector_hidden;
                self.layout(h);
            }
            13 => {
                if let Some(i) = self.selected {
                    unsafe {
                        SendMessageW(self.documents[i].hwnd, WM_COMMAND, Some(WPARAM(10)), None);
                    }
                }
            }
            35 | 36 => {
                if let Some(i) = self.selected {
                    unsafe {
                        SendMessageW(
                            self.documents[i].hwnd,
                            WM_COMMAND,
                            Some(WPARAM(if id == 35 { 7 } else { 60 })),
                            None,
                        );
                    }
                }
            }
            17 => {
                use super::{
                    app_menu::{self, Row},
                    editor_ui::{EditorTheme, IconKind as I},
                };
                let rows = [
                    (36, "更多图片工具…", P::MENU),
                    (3, "滚动长截图", I::ScrollCapture),
                    (4, "拼图", I::Collage),
                    (5, "批处理", I::Log),
                    (11, "图片插件", I::Diagnostics),
                    (6, "关闭当前图片", I::Close),
                ]
                .into_iter()
                .map(|(id, n, i)| Row::command(id, n, i, true))
                .collect();
                if let Some(c) = app_menu::show_rows(
                    h,
                    super::desktop_ui::rect(ui::scale(h, 8), 0, ui::scale(h, 64), ui::scale(h, 37)),
                    EditorTheme::current(super::editor_ui::window_dpi(h)),
                    rows,
                    false,
                )
                .map_err(|e| e.to_string())?
                {
                    self.command(h, usize::try_from(c).unwrap_or_default())?;
                }
            }
            15 | 16 => {
                if let Some(i) = self.selected {
                    capture_tools::workspace_recognition(self.documents[i].hwnd, id == 16);
                }
                self.collapsed = false;
                self.inspector_hidden = false;
                unsafe {
                    SendMessageW(self.mode, CB_SETCURSEL, Some(WPARAM(id - 15)), None);
                }
                self.layout(h);
            }
            18 => {
                self.collapsed = true;
                self.layout(h);
            }
            19 => {
                if let Some(i) = self.selected {
                    unsafe {
                        SendMessageW(self.documents[i].hwnd, WM_COMMAND, Some(WPARAM(63)), None);
                    }
                }
                self.layout(h);
            }
            11 => {
                self.selected.ok_or("请先打开图片")?;
                super::plugin_ui::image_tools()?;
            }
            _ => {}
        }
        Ok(())
    }
}
unsafe extern "system" fn procedure(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if let Some(r) = super::app_menu::window_message(msg, lp) {
        return r;
    }
    let Some(s) = (unsafe { state(h) }) else {
        return unsafe { DefWindowProcW(h, msg, wp, lp) };
    };
    match msg {
        windows::Win32::UI::WindowsAndMessaging::WM_PAINT => {
            use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &raw mut ps) };
            let frame = super::desktop_ui::BufferedFrame::new(dc, ui::rect(h));
            let dc = frame.dc;
            s.paint(h, dc);
            drop(frame);
            unsafe {
                let _ = EndPaint(h, &raw const ps);
            }
        }
        WM_SIZE => s.layout(h),
        WM_COMMAND => {
            let id = wp.0 & 0xffff;
            let code = wp.0 >> 16;
            if (id == 100 || id == 101) && code == 0 {
                if let Some(i) = s
                    .documents
                    .iter()
                    .position(|d| (if id == 100 { d.tab } else { d.close }).0 as isize == lp.0)
                {
                    s.select(h, i);
                    if id == 101 {
                        if let Err(e) = s.command(h, 6) {
                            workspace::notice_error(&e);
                        }
                    }
                }
            } else if id == 20 && code == CBN_SELCHANGE as usize {
                s.select(h, ui::selection(s.chooser));
            } else if id == 21 && code == CBN_SELCHANGE as usize {
                let online = ui::selection(s.mode) == 1;
                let service = super::vision_settings::load().ok();
                ui::text(
                    s.service,
                    &if online {
                        service.map_or("在设置中配置服务".into(), |v| {
                            format!("发送到 {} · {}", v.base_url, v.model)
                        })
                    } else {
                        "本机离线处理 · 补充要求仅用于多模态识图".into()
                    },
                );
                s.refresh();
                s.layout(h);
            } else if (id <= 19 || matches!(id, 35 | 36))
                && code == 0
                && let Err(e) = s.command(h, id)
            {
                ui::text(s.status, &e);
            }
        }
        WM_NCDESTROY => unsafe {
            SetWindowLongPtrW(h, GWLP_USERDATA, 0);
            drop(Box::from_raw(std::ptr::from_mut(s)));
        },
        _ => return unsafe { DefWindowProcW(h, msg, wp, lp) },
    }
    LRESULT(0)
}

pub(crate) fn set_inspector(panel: HWND, visible: bool) {
    if let Some(s) = unsafe { state(panel) } {
        s.inspector = visible;
    }
}
pub(crate) fn prune(panel: HWND) {
    if let Some(s) = unsafe { state(panel) } {
        for doc in &mut s.documents {
            let dirty = if capture_tools::workspace_dirty(doc.hwnd) {
                " *"
            } else {
                ""
            };
            let name = doc
                .source
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("图片 {}", doc.id));
            ui::text(doc.tab, &format!("{name}{dirty}"));
        }
        s.refresh();
        let current = s.selected.and_then(|i| s.documents.get(i)).map(|doc| {
            (
                doc.id,
                capture_tools::workspace_dirty(doc.hwnd),
                capture_tools::workspace_zoom(doc.hwnd).round() as i32,
            )
        });
        if current != s.context_state {
            s.context_state = current;
            let r = super::desktop_ui::rect(0, 0, ui::rect(panel).right, ui::scale(panel, 40));
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(panel), Some(&r), false);
            }
        }
        if s.documents
            .iter()
            .all(|d| unsafe { IsWindow(Some(d.hwnd)) }.as_bool())
        {
            return;
        }
        s.save_text();
        let selected = s.selected.and_then(|i| s.documents.get(i)).map(|d| d.id);
        s.documents.retain(|d| {
            let exists = unsafe { IsWindow(Some(d.hwnd)) }.as_bool();
            if !exists && let Some(c) = &d.cancel {
                c.store(true, Ordering::Release);
            }
            exists
        });
        unsafe {
            SendMessageW(
                s.chooser,
                windows::Win32::UI::WindowsAndMessaging::CB_RESETCONTENT,
                None,
                None,
            );
        }
        for d in &s.documents {
            let name = ui::wide(&format!("图片 {}", d.id));
            unsafe {
                SendMessageW(
                    s.chooser,
                    CB_ADDSTRING,
                    None,
                    Some(LPARAM(name.as_ptr() as isize)),
                );
            }
        }
        s.selected = None;
        if s.documents.is_empty() {
            ui::text(s.result, "");
            s.layout(panel);
        } else {
            s.select(
                panel,
                s.documents
                    .iter()
                    .position(|d| Some(d.id) == selected)
                    .unwrap_or(0),
            );
        }
    }
}

#[cfg(test)]
pub(crate) fn review() -> serde_json::Value {
    let s = unsafe { state(workspace::image_parent().unwrap()) }.unwrap();
    serde_json::json!({"selected":s.selected,"documents":s.documents.iter().map(|d|serde_json::json!({"id":d.id,"hwnd":d.hwnd.0 as usize,"text":d.text,"status":d.status})).collect::<Vec<_>>()})
}

pub(crate) fn refresh_plugins() {
    if let Some(panel) = workspace::image_parent()
        && let Some(s) = unsafe { state(panel) }
    {
        s.refresh();
    }
}
pub(crate) fn is_busy(id: u64) -> bool {
    workspace::image_parent()
        .and_then(|h| unsafe { state(h) })
        .and_then(|s| s.documents.iter().find(|d| d.id == id))
        .is_none_or(|d| d.cancel.is_some())
}
pub(crate) fn set_running(id: u64, cancel: Arc<AtomicBool>) {
    if let Some(s) = workspace::image_parent().and_then(|h| unsafe { state(h) })
        && let Some(d) = s.documents.iter_mut().find(|d| d.id == id)
    {
        d.cancel = Some(cancel);
        d.status = "插件正在处理，可切换标签继续工作…".into();
        s.refresh();
    }
}

pub(crate) fn current_document() -> Result<(HWND, u64), String> {
    let s = workspace::image_parent()
        .and_then(|h| unsafe { state(h) })
        .ok_or("图片工作区尚未就绪")?;
    let d = s
        .selected
        .and_then(|i| s.documents.get(i))
        .ok_or("请先选择图片")?;
    Ok((d.hwnd, d.id))
}

pub(crate) fn picture_command(command: usize) {
    if let Ok((h, _)) = current_document() {
        unsafe {
            SendMessageW(h, WM_COMMAND, Some(WPARAM(command)), None);
        }
    }
}

pub(crate) fn resize_inspector(width: i32) {
    if let Some(h) = workspace::image_parent() {
        if let Some(s) = unsafe { state(h) } {
            let width = Some(width.clamp(264, 420));
            if s.inspector_width != width {
                s.inspector_width = width;
                s.layout(h);
            }
        }
    }
}

thread_local! {
    static INSPECTOR_DRAG: std::cell::Cell<Option<(HWND, Option<i32>, i32)>> = const { std::cell::Cell::new(None) };
}
unsafe extern "system" fn separator_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    use windows::Win32::{
        Foundation::POINT,
        Graphics::Gdi::*,
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    };
    match m {
        WM_ERASEBKGND => return LRESULT(1),
        WM_SETCURSOR => {
            unsafe {
                SetCursor(LoadCursorW(None, IDC_SIZEWE).ok());
            }
            return LRESULT(1);
        }
        WM_LBUTTONDOWN => {
            if let Ok(parent) = unsafe { GetParent(h) } {
                if let Some(s) = unsafe { state(parent) } {
                    let width = s.inspector_width.unwrap_or_else(|| super::workbench_layout::inspector(ui::rect(parent).right*96/super::editor_ui::window_dpi(parent) as i32));
                    INSPECTOR_DRAG.with(|v|v.set(Some((parent,s.inspector_width,width))));
                    unsafe { SetCapture(h); SetTimer(Some(h), 2, 16, None); }
                }
            }
            return LRESULT(0);
        }
        WM_TIMER if w.0 == 2 => {
            if let Some((_, _, width)) = INSPECTOR_DRAG.with(|v|v.get()) { resize_inspector(width); }
            return LRESULT(0);
        }
        WM_LBUTTONUP | WM_CANCELMODE | WM_CAPTURECHANGED => {
            if let Some((parent, original, width)) = INSPECTOR_DRAG.with(|v|v.take()) {
                if m == WM_LBUTTONUP { resize_inspector(width); }
                else if let Some(s) = unsafe { state(parent) } { s.inspector_width=original; s.layout(parent); }
            }
            unsafe {
                let _ = KillTimer(Some(h), 2);
                if GetCapture() == h { let _ = ReleaseCapture(); }
            }
            return LRESULT(0);
        }
        WM_MOUSEMOVE if unsafe { GetCapture() } == h => {
            if let Ok(parent) = unsafe { GetParent(h) } {
                let mut p = POINT { x: l.0 as i16 as i32, y:(l.0 >> 16) as i16 as i32 };
                unsafe {
                    let _ = ClientToScreen(h, &mut p);
                    let _ = ScreenToClient(parent, &mut p);
                }
                let width = ((ui::rect(parent).right - p.x) * 96
                    / super::editor_ui::window_dpi(parent) as i32)
                    .clamp(264, 420);
                INSPECTOR_DRAG.with(|v| { if let Some((parent,original,_))=v.get() {v.set(Some((parent,original,width)));} });
            }
            return LRESULT(0);
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &mut ps) };
            let r = ui::rect(h);
            let t = super::editor_ui::EditorTheme::current(super::editor_ui::window_dpi(h));
            super::editor_ui::fill_rect(dc, &r, t.palette.panel);
            super::desktop_ui::line(dc, super::desktop_ui::rect(r.right / 2, 0, 1, r.bottom), t);
            unsafe {
                let _ = EndPaint(h, &ps);
            }
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(h, m, w, l) }
}

pub(crate) fn open_source(path: &Path, library: &Path) -> Result<(), String> {
    if let Some(h) = workspace::image_parent() {
        if let Some(s) = unsafe { state(h) } {
            if s.pending_sources.iter().any(|(image, p)| {
                unsafe { IsWindow(Some(HWND(*image as *mut _))) }.as_bool()
                    && p.to_string_lossy()
                        .eq_ignore_ascii_case(&path.to_string_lossy())
            }) {
                workspace::switch(0);
                return Ok(());
            }
            if let Some(i) = s.documents.iter().position(|d| {
                d.source.as_ref().is_some_and(|p| {
                    p.to_string_lossy()
                        .eq_ignore_ascii_case(&path.to_string_lossy())
                })
            }) {
                s.select(h, i);
                workspace::image_view(None);
                workspace::switch(0);
                return Ok(());
            }
        }
    }
    let image = if path
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("gif"))
    {
        capture_tools::show_gif(path, library)?
    } else {
        capture_tools::show_image(Raster::load(path)?, library, false)?
    };
    if let Some(h) = workspace::image_parent() {
        if let Some(s) = unsafe { state(h) } {
            if let Some(doc) = s.documents.iter_mut().find(|d| d.hwnd == image) {
                doc.source = Some(path.to_path_buf());
            } else {
                s.pending_sources
                    .insert(image.0 as isize, path.to_path_buf());
            }
            prune(h);
        }
    }
    Ok(())
}

/// Display name shared by the document tab and the image inspector.
pub(crate) fn document_name(image: HWND) -> String {
    workspace::image_parent()
        .and_then(|h| unsafe { state(h) })
        .and_then(|s| s.documents.iter().find(|d| d.hwnd == image))
        .map(|d| {
            d.source
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("图片 {}", d.id))
        })
        .unwrap_or_else(|| "图片".into())
}

#[cfg(test)]
pub(crate) fn review_panel_resize() {
    use windows::Win32::UI::WindowsAndMessaging::*;
    let parent=workspace::image_parent().unwrap();
    let s=unsafe{state(parent)}.unwrap();
    let separator=s.separator;let original=s.inspector_width;
    assert!(unsafe{IsWindowVisible(separator)}.as_bool());
    let image=s.documents[s.selected.unwrap()].hwnd;
    let revision=capture_tools::workspace_metadata(image).unwrap();
    unsafe { SendMessageW(separator,WM_LBUTTONDOWN,None,Some(LPARAM(2|(20<<16)))); }
    for delta in 1..=40 {
        unsafe { SendMessageW(separator,WM_MOUSEMOVE,None,Some(LPARAM(((20<<16)|((2-delta)&65535)) as isize))); }
    }
    assert_eq!(unsafe{state(parent)}.unwrap().inspector_width,original,"image mouse burst waits for frame timer");
    unsafe { SendMessageW(separator,WM_TIMER,Some(WPARAM(2)),None); }
    let changed=unsafe{state(parent)}.unwrap().inspector_width;
    assert!(changed.is_some());assert_ne!(changed,original);
    unsafe { SendMessageW(separator,WM_LBUTTONUP,None,None); }
    assert!(INSPECTOR_DRAG.with(|v|v.get()).is_none());
    assert_eq!(capture_tools::workspace_metadata(image).unwrap(),revision);
    unsafe {
        SendMessageW(separator,WM_LBUTTONDOWN,None,Some(LPARAM(2|(20<<16))));
        SendMessageW(separator,WM_MOUSEMOVE,None,Some(LPARAM((20<<16)|65500)));
        SendMessageW(separator,WM_TIMER,Some(WPARAM(2)),None);
        SendMessageW(separator,WM_CANCELMODE,None,None);
    }
    assert_eq!(unsafe{state(parent)}.unwrap().inspector_width,changed);
}
