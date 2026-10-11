//! DPI-aware image editor and interactive desktop pin windows.
use super::{
    BeginPaint, COLORREF, CS_DBLCLKS, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
    DestroyWindow, Document, EndPaint, GWLP_USERDATA, GetCursorPos, GetKeyState, GetModuleHandleW,
    GetWindowLongPtrW, GetWindowRect, GetWindowTextW, HDC, HMENU, HWND, IDC_ARROW, LPARAM, LRESULT,
    LWA_ALPHA, LoadCursorW, MINMAXINFO, Mark, PAINTSTRUCT, PCWSTR, PICTURES, PINS, POINT, Path,
    PathBuf, PostMessageW, RECT, Raster, RefCell, RegisterClassW, ReleaseCapture, SW_HIDE, SW_SHOW,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetCapture, SetFocus,
    SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
    TextStyle, Tool, VK_CONTROL, VK_SHIFT, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CANCELMODE,
    WM_CAPTURECHANGED, WM_COMMAND, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT,
    WM_RBUTTONUP, WM_SETFONT, WM_SIZE, WNDCLASSW, WPARAM, WS_BORDER, WS_CHILD, WS_CLIPCHILDREN,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_TABSTOP,
    WS_VISIBLE, copy_image, draw_raster, editor_ui, fill, invalidate, pin_manager, point,
    save_dialog, title, w, wide,
};
use std::rc::Rc;
#[path = "image_interaction.rs"]
mod image_interaction;
#[path = "picture_preview.rs"]
mod picture_preview;
#[path = "picture_text.rs"]
mod picture_text;
use super::Surface;
use crate::platform::image_icons as P;
use image_interaction::{Crop, Viewport};
// Generate immutable display levels once per document revision. Resizing never
// resamples the full-resolution source; zooming to native pixels still uses it.
fn preview_levels(source: &Raster) -> Vec<Raster> {
    let Some(view) = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(
        source.width,
        source.height,
        source.pixels.as_slice(),
    ) else {
        return Vec::new();
    };
    [1024, 2048]
        .into_iter()
        .filter(|edge| source.width.max(source.height) > *edge)
        .map(|edge| {
            let ratio = edge as f64 / source.width.max(source.height) as f64;
            let raster = image::imageops::thumbnail(
                &view,
                (source.width as f64 * ratio).round().max(1.) as u32,
                (source.height as f64 * ratio).round().max(1.) as u32,
            );
            Raster {
                width: raster.width(),
                height: raster.height(),
                pixels: raster.into_raw(),
            }
        })
        .collect()
}
struct ObjectDrag {
    index: usize,
    start: POINT,
    original: Mark,
    preview: Mark,
}
struct DisplayCache {
    draft: Option<Mark>,
    revision: u64,
    excluded: Option<usize>,
    view: RECT,
    area: RECT,
    background: u32,
    surface: Surface,
    raster: std::cell::OnceCell<Raster>,
}

use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, EM_SETLIMITTEXT, ODS_SELECTED};
use windows::Win32::UI::WindowsAndMessaging::{
    BS_OWNERDRAW, CB_GETCURSEL, CB_SETCURSEL, CBN_SELCHANGE, ES_AUTOVSCROLL, ES_MULTILINE,
    ES_WANTRETURN, GWL_EXSTYLE, IDYES, IsChild, IsDialogMessageW, MB_ICONQUESTION, MB_YESNOCANCEL,
    SWP_FRAMECHANGED, WM_CLOSE, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_DRAWITEM, WM_EXITSIZEMOVE,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WS_EX_CONTROLPARENT, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT,
    WS_VSCROLL,
};
pub(super) type SharedDocument = Rc<RefCell<Document>>;
struct Child {
    handle: HWND,
    id: usize,
}
#[allow(clippy::struct_excessive_bools)] // Independent pin presentation options and editor save state.
pub(super) struct Picture {
    pub document: SharedDocument,
    pub library: PathBuf,
    pub pinned: bool,
    pub pin_id: Option<String>,
    pub group: String,
    pub animation: Option<crate::platform::gif_player::Player>,
    pub gif_path: Option<PathBuf>,
    pub opacity: u8,
    pub locked: bool,
    pub passthrough: bool,
    pub collapsed: bool,
    pub expanded_size: (i32, i32),
    pending: Option<Mark>,
    tool: Tool,
    pub(super) recognition: Option<bool>,
    color: [u8; 4],
    stroke: u32,
    filled: bool,
    font_size: u32,
    font_style: panzo_core::text_style::FontStyle,
    font_controls: Option<crate::platform::font_catalog::FontControls>,
    text_input: HWND,
    inline_input: HWND,
    inline_text: Option<picture_text::InlineText>,
    inline_font: windows::Win32::Graphics::Gdi::HFONT,
    inline_font_height: i32,
    inline_ime: bool,
    syncing_text_fields: bool,
    inline_move: Option<picture_text::TextDrag>,
    inline_caret_visible: bool,
    children: Vec<Child>,
    toolbar_height: i32,
    pub(super) inspector_width: i32,
    drag: Option<(POINT, RECT)>,
    selected: Option<usize>,
    property_preview: Option<Mark>,
    object_drag: Option<ObjectDrag>,
    viewport: Viewport,
    pan_drag: Option<(POINT, (f64, f64), bool)>,
    crop: Option<Crop>,
    crop_drag: Option<(usize, POINT, Crop)>,
    syncing_crop_fields: bool,
    syncing_color_fields: bool,
    display_cache: Option<DisplayCache>,
    draft_cache: Option<picture_preview::DraftCache>,
    framebuffer: Option<(u32, u32, Surface)>,
    dpi: u32,
    cached: Option<(u64, Raster, Vec<Raster>)>,
    dirty: bool,
    saved_revision: u64,
    display_size: (u32, u32),
}
const BUTTONS: &[(usize, &str, i32)] = &[
    (30, "选择", 54),
    (1, "矩形", 54),
    (31, "椭圆", 54),
    (32, "直线", 54),
    (2, "箭头", 54),
    (3, "画笔", 54),
    (33, "荧光", 54),
    (4, "文字", 54),
    (34, "序号", 54),
    (5, "马赛克", 66),
    (35, "模糊", 54),
    (6, "遮挡", 54),
    (36, "裁剪", 54),
    (7, "撤销", 54),
    (8, "重做", 54),
    (37, "删除", 54),
    (38, "颜色…", 66),
    (39, "应用样式", 78),
    (40, "旋转", 54),
    (41, "左右翻转", 78),
    (42, "上下翻转", 78),
    (43, "取消裁剪", 78),
    (9, "复制", 60),
    (44, "快速保存", 84),
    (10, "另存为…", 84),
    (11, "贴到桌面", 84),
    (45, "更新文字", 84),
    (46, "文字 OCR", 84),
    (67, "多模态识图", 84),
    (68, "拼图", 84),
    (60, "更多工具", 70),
    (61, "应用裁剪", 80),
    (62, "取消裁剪 (Esc)", 80),
    (63, "适应窗口", 70),
    (64, "实际像素大小", 70),
    (65, "缩小", 32),
    (66, "放大", 32),
    (82, "应用裁剪尺寸", 60),
];
fn tool_for(id: usize) -> Option<Tool> {
    Some(match id {
        1 => Tool::Rectangle,
        2 => Tool::Arrow,
        3 => Tool::Pen,
        4 => Tool::Text,
        5 => Tool::Mosaic,
        6 => Tool::Cover,
        30 => Tool::Select,
        31 => Tool::Ellipse,
        32 => Tool::Line,
        33 => Tool::Highlight,
        34 => Tool::Number,
        35 => Tool::Blur,
        36 => Tool::Crop,
        _ => return None,
    })
}
pub(super) fn create_picture(
    document: SharedDocument,
    library: &Path,
    pinned: bool,
) -> Result<HWND, String> {
    create_picture_with_clip(document, library, pinned, None)
}
pub(super) fn create_picture_with_clip(
    document: SharedDocument,
    library: &Path,
    pinned: bool,
    clip: Option<(std::sync::Arc<crate::platform::gif_player::Clip>, PathBuf)>,
) -> Result<HWND, String> {
    if pinned {
        pin_manager::check_budget(
            document
                .borrow()
                .original
                .pixels
                .len()
                .saturating_add(clip.as_ref().map_or(0, |(c, _)| c.bytes)),
        )?;
    }
    let embedded = (!pinned)
        .then(crate::platform::workspace::image_parent)
        .flatten();
    let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    unsafe {
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(picture_proc),
            hInstance: module.into(),
            lpszClassName: w!("PanzoPicture"),
            style: CS_DBLCLKS,
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
            ..Default::default()
        });
    }
    let size = document.borrow().size();
    let dpi = editor_ui::system_dpi();
    let scale = |value: i32| value * dpi as i32 / 96;
    let width = if pinned {
        (size.0 as i32).clamp(80, 900)
    } else {
        scale(1040)
    };
    let height = if pinned {
        (f64::from(width) * f64::from(size.1) / f64::from(size.0)) as i32
    } else {
        scale(820)
    };
    let hwnd = unsafe {
        CreateWindowExW(
            if pinned {
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED
            } else {
                WS_EX_CONTROLPARENT
            },
            w!("PanzoPicture"),
            w!("Panzo · 截图与标注"),
            if pinned {
                WS_POPUP | WS_BORDER
            } else if embedded.is_some() {
                WS_CHILD | WS_CLIPCHILDREN
            } else {
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN
            },
            if pinned { 80 } else { CW_USEDEFAULT },
            if pinned { 80 } else { CW_USEDEFAULT },
            width,
            height.max(60),
            embedded,
            None,
            Some(module.into()),
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    let mut state = Box::new(Picture {
        document,
        library: library.into(),
        pinned,
        pin_id: None,
        group: String::new(),
        gif_path: clip.as_ref().map(|(_, p)| p.clone()),
        animation: clip.map(|(c, _)| crate::platform::gif_player::Player::new(c)),
        opacity: 255,
        locked: false,
        passthrough: false,
        collapsed: false,
        expanded_size: (width, height),
        pending: None,
        tool: Tool::Select,
        recognition: None,
        color: [85, 108, 209, 255],
        stroke: 3,
        filled: false,
        font_size: 24,
        font_style: Default::default(),
        font_controls: None,
        text_input: HWND::default(),
        inline_input: HWND::default(),
        inline_text: None,
        inline_font: windows::Win32::Graphics::Gdi::HFONT::default(),
        inline_font_height: 0,
        inline_ime: false,
        syncing_text_fields: false,
        inline_move: None,
        inline_caret_visible: true,
        children: Vec::new(),
        toolbar_height: 0,
        inspector_width: 304,
        drag: None,
        selected: None,
        property_preview: None,
        object_drag: None,
        viewport: Viewport::default(),
        pan_drag: None,
        crop: None,
        crop_drag: None,
        syncing_crop_fields: false,
        syncing_color_fields: false,
        display_cache: None,
        draft_cache: None,
        framebuffer: None,
        dpi: editor_ui::window_dpi(hwnd),
        cached: None,
        dirty: false,
        saved_revision: 0,
        display_size: size,
    });
    if !pinned && let Err(error) = state.make_controls(hwnd) {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return Err(error);
    }
    let pointer = Box::into_raw(state);
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, pointer as isize);
    }
    PICTURES.with(|windows| windows.borrow_mut().push(hwnd));
    if pinned {
        PINS.with(|pins| pins.borrow_mut().push(hwnd));
        unsafe {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
        }
        if let Err(error) = pin_manager::register(hwnd) {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return Err(error);
        }
    } else {
        unsafe { &mut *pointer }.relayout(hwnd);
        editor_ui::apply_window_chrome(hwnd);
    }
    if embedded.is_some() {
        crate::platform::workspace::picture(hwnd);
    } else {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
    }
    if pinned {
        pin_manager::save_session()?;
    }
    Ok(hwnd)
}
impl Picture {
    fn make_controls(&mut self, hwnd: HWND) -> Result<(), String> {
        let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
        for &(id, label, _) in BUTTONS {
            let value = wide(label);
            let handle = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("BUTTON"),
                    PCWSTR(value.as_ptr()),
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
                    0,
                    0,
                    1,
                    1,
                    Some(hwnd),
                    Some(HMENU(id as *mut _)),
                    Some(module.into()),
                    None,
                )
            }
            .map_err(|e| e.to_string())?;
            self.children.push(Child { handle, id });
        }
        use crate::platform::workspace_ui as ui;
        for (id, value) in [(50, "3"), (51, "24"), (54, "100")] {
            let handle = if id == 51 {
                ui::number(
                    hwnd,
                    value,
                    &[
                        "8", "9", "10", "11", "12", "14", "16", "18", "20", "24", "28", "32", "36",
                        "48", "60", "72", "96", "120", "144", "256", "512",
                    ],
                    id,
                    None,
                )?
            } else {
                ui::edit(hwnd, value, id, false)?
            };
            if id != 51 {
                unsafe {
                    use windows::Win32::UI::WindowsAndMessaging::{ES_NUMBER, ES_RIGHT, GWL_STYLE};
                    SetWindowLongPtrW(
                        handle,
                        GWL_STYLE,
                        GetWindowLongPtrW(handle, GWL_STYLE) | (ES_RIGHT | ES_NUMBER) as isize,
                    );
                }
            }
            self.children.push(Child { handle, id });
        }
        self.children.push(Child {
            handle: ui::combo(
                hwnd,
                &[
                    "自由比例",
                    "原图比例",
                    "1 : 1",
                    "4 : 3",
                    "3 : 2",
                    "16 : 9",
                    "9 : 16",
                ],
                0,
                52,
            )?,
            id: 52,
        });
        for (i, label) in ["自由", "原图", "1:1", "4:3", "3:2", "16:9", "9:16"]
            .iter()
            .enumerate()
        {
            self.children.push(Child {
                handle: ui::button(hwnd, label, 90 + i)?,
                id: 90 + i,
            });
        }
        for (id, label) in [
            (100, "描边"),
            (101, "填充"),
            (110, "暖红"),
            (111, "浅蓝"),
            (112, "暖黄"),
            (113, "白色"),
            (120, "裁剪图片"),
            (121, "旋转图片"),
            (122, "识别文字"),
            (123, "多模态识图"),
            (124, "重置裁剪框"),
            (125, "旋转图片"),
        ] {
            self.children.push(Child {
                handle: ui::button(hwnd, label, id)?,
                id,
            });
        }
        self.text_input = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                w!(""),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WS_VSCROLL
                    | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32),
                0,
                0,
                100,
                56,
                Some(hwnd),
                Some(HMENU(20usize as *mut _)),
                Some(module.into()),
                None,
            )
        }
        .map_err(|e| e.to_string())?;
        crate::platform::desktop_ui::edit(self.text_input);
        self.make_inline_text(hwnd)?;
        self.font_controls = Some(crate::platform::font_catalog::FontControls::new(
            hwnd, 2100,
        )?);
        self.sync_font_controls();
        unsafe {
            SendMessageW(self.text_input, EM_SETLIMITTEXT, Some(WPARAM(4096)), None);
        }
        for (id, label) in [(80, "宽度"), (81, "高度")] {
            let control = crate::platform::workspace_ui::edit(hwnd, "", id, false)
                .map_err(|e| e.to_string())?;
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{ES_NUMBER, ES_RIGHT, GWL_STYLE};
                SetWindowLongPtrW(
                    control,
                    GWL_STYLE,
                    GetWindowLongPtrW(control, GWL_STYLE) | (ES_RIGHT | ES_NUMBER) as isize,
                );
            }
            let cue = wide(label);
            unsafe {
                SendMessageW(
                    control,
                    windows::Win32::UI::Controls::EM_SETCUEBANNER,
                    Some(WPARAM(1)),
                    Some(LPARAM(cue.as_ptr() as isize)),
                );
            }
            unsafe {
                SendMessageW(control, EM_SETLIMITTEXT, Some(WPARAM(6)), None);
            }
            self.children.push(Child {
                handle: control,
                id,
            });
        }
        Ok(())
    }
    pub(super) fn unsaved(&self) -> bool {
        self.inline_text_changed()
            || self.property_preview.is_some()
            || (self.dirty && self.document.borrow().revision != self.saved_revision)
    }
    fn contextual_tool(&self) -> Tool {
        self.selected
            .and_then(|i| self.document.borrow().state.marks.get(i).map(|m| m.tool))
            .unwrap_or(self.tool)
    }
    fn desktop_layout(&mut self, hwnd: HWND) {
        let _batch = crate::platform::workspace_ui::layout_batch(hwnd);
        use crate::platform::{desktop_ui as v, workspace_ui as ui};
        use editor_ui::IconKind as I;
        self.viewport.dpi = self.dpi;
        let r = ui::rect(hwnd);
        let d = |n| editor_ui::scale_dip(n, self.dpi);
        v::install_canvas(hwnd);
        let selected_tool = self.contextual_tool();
        let text = selected_tool == Tool::Text;
        let crop = self.crop.is_some();
        let color = !crop && !matches!(selected_tool, Tool::Select | Tool::Mosaic | Tool::Blur);
        let stroke = !crop
            && !matches!(
                selected_tool,
                Tool::Select | Tool::Text | Tool::Number | Tool::Cover
            );
        let font = matches!(selected_tool, Tool::Text | Tool::Number);
        let rail = [
            (30, P::SELECT),
            (36, P::CROP),
            (3, P::PEN),
            (1, P::RECTANGLE),
            (2, P::ARROW),
            (4, P::TEXT),
            (5, P::BLUR),
            (46, P::OCR),
            (67, P::SPARK),
            (68, P::COLLAGE),
            (11, P::PIN),
        ];
        let side = d(self.inspector_width);
        let property_x = r.right - side + d(16);
        for child in &self.children {
            let rail_index = rail.iter().position(|(id, _)| *id == child.id);
            let combo = matches!(child.id, 51 | 52);
            let edit = [50, 51, 54, 80, 81].contains(&child.id);
            if edit {
                let maximum = match child.id {
                    50 => 96,
                    51 => 512,
                    54 => 100,
                    80 => self.document.borrow().size().0 as i64,
                    _ => self.document.borrow().size().1 as i64,
                };
                let mut target = child.handle;
                if child.id == 51 {
                    let mut info = windows::Win32::UI::Controls::COMBOBOXINFO {
                        cbSize: std::mem::size_of::<windows::Win32::UI::Controls::COMBOBOXINFO>()
                            as u32,
                        ..Default::default()
                    };
                    if unsafe {
                        windows::Win32::UI::Controls::GetComboBoxInfo(child.handle, &mut info)
                    }
                    .is_ok()
                    {
                        target = info.hwndItem;
                    }
                }
                v::number_field(target, if child.id == 54 { 0 } else { 1 }, maximum, 1);
            }

            if combo {
                v::combo(child.handle);
            } else if !edit {
                v::button(child.handle);
                let icon = rail_index.map(|i| rail[i].1).or(match child.id {
                    38 => None,
                    39 | 45 | 61 | 82 => Some(I::Check),
                    62 => Some(I::Close),
                    63 => Some(I::Fit),
                    64 => Some(I::OneToOne),
                    65 => Some(I::ZoomOut),
                    66 => Some(I::ZoomIn),
                    _ => None,
                });
                v::style(child.handle, icon, true, false, icon.is_some());
                v::selected(
                    child.handle,
                    if let Some(online) = self.recognition {
                        child.id == if online { 67 } else { 46 }
                    } else {
                        tool_for(child.id) == Some(self.tool)
                    },
                );
            }
            unsafe {
                let font = editor_ui::font_handle(TextStyle::Body, self.dpi).0 as usize;
                if SendMessageW(
                    child.handle,
                    windows::Win32::UI::WindowsAndMessaging::WM_GETFONT,
                    None,
                    None,
                )
                .0 as usize
                    != font
                {
                    SendMessageW(
                        child.handle,
                        WM_SETFONT,
                        Some(WPARAM(font)),
                        Some(LPARAM(0)),
                    );
                }
            }
            let shown = rail_index.is_some()
                || ((side > 0 || [9, 44].contains(&child.id))
                    && match child.id {
                        38 | 54 => color,
                        110..=113 => {
                            color
                                && child.id - 110
                                    < crate::platform::ui_preferences::recent_colors().len()
                        }
                        100 | 101 => color && !text,
                        120..=123 => selected_tool == Tool::Select,
                        124 | 125 => crop,
                        39 => self.selected.is_some() && !text,
                        45 => false,
                        50 => stroke,
                        51 => font,
                        61 | 62 | 80 | 81 => crop,
                        90..=96 => crop && [90, 92, 93, 95].contains(&child.id),
                        9 | 44 => true,
                        _ => false,
                    });
            ui::show(child.handle, shown);
            if !shown {
                continue;
            }
            if matches!(child.id, 38 | 51 | 54 | 110..=113) {
                unsafe {
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                        child.handle,
                        !text || self.selected.is_some() || self.inline_text.is_some(),
                    );
                }
            }
            if let Some(i) = rail_index {
                ui::place(
                    child.handle,
                    d(5),
                    if child.id == 11 {
                        r.bottom - d(78)
                    } else {
                        d(8 + 38 * i as i32 + if i >= 7 { 14 } else { 0 })
                    },
                    d(32),
                    d(32),
                );
            } else if (90..=96).contains(&child.id) {
                let index = child.id - 90;
                let position = [90, 92, 93, 95]
                    .iter()
                    .position(|id| *id == child.id)
                    .unwrap_or(0);
                v::style(child.handle, None, false, false, false);
                let step = (side - d(47)) / 4;
                ui::place(
                    child.handle,
                    property_x + d(3) + (step + d(3)) * position as i32,
                    d(48),
                    step,
                    d(27),
                );
                let choice = self
                    .children
                    .iter()
                    .find(|c| c.id == 52)
                    .map_or(0, |c| unsafe {
                        SendMessageW(c.handle, CB_GETCURSEL, None, None).0 as usize
                    });
                v::selected(child.handle, index == choice);
            } else if (110..=113).contains(&child.id) {
                let Some(color) = crate::platform::ui_preferences::recent_colors()
                    .get(child.id - 110)
                    .copied()
                else {
                    continue;
                };
                v::choice(child.handle, v::ChoiceVisual::ColorDot(color));
                v::selected(child.handle, self.color[..3] == color[..3]);
                ui::place(
                    child.handle,
                    property_x + d(30) * (child.id - 110) as i32,
                    d(if text { 142 } else { 137 }),
                    d(21),
                    d(26),
                );
            } else if [100, 101].contains(&child.id) {
                let width = (side - d(41)) / 2;
                v::style(child.handle, None, false, false, false);
                v::selected(child.handle, self.filled == (child.id == 101));
                ui::place(
                    child.handle,
                    property_x + d(3) + (width + d(3)) * (child.id - 100) as i32,
                    d(225),
                    width,
                    d(27),
                );
            } else if (120..=125).contains(&child.id) {
                let (index, icon) = match child.id {
                    120 => (0, P::CROP),
                    121 => (1, P::REDO),
                    122 => (2, P::OCR),
                    123 => (3, P::SPARK),
                    124 => (0, P::REFRESH),
                    _ => (1, P::REDO),
                };
                v::style(child.handle, Some(icon), true, false, true);
                ui::place(
                    child.handle,
                    property_x + d(36) * index,
                    d(if crop { 276 } else { 279 }),
                    d(30),
                    d(30),
                );
            } else if [9, 44].contains(&child.id) {
                let index = i32::from(child.id == 44);
                v::style(
                    child.handle,
                    Some(if index == 0 { P::COPY } else { P::SAVE }),
                    true,
                    false,
                    true,
                );
                ui::place(
                    child.handle,
                    r.right - side - d(76) + d(index * 36),
                    r.bottom - d(32),
                    d(30),
                    d(28),
                );
            } else {
                let (y, w) = match child.id {
                    50 => (53, d(66)),
                    51 => (53, d(108)),
                    54 => (
                        if text { 333 } else { 260 },
                        if text { side - d(58) } else { d(66) },
                    ),
                    38 => (if text { 142 } else { 137 }, d(34)),
                    52 => (68, side - d(32)),
                    80 => (140, d(66)),
                    81 => (178, d(66)),
                    61 => (
                        r.bottom * 96 / self.dpi as i32 - 42,
                        self.inspector_width - 86,
                    ),
                    62 => (r.bottom * 96 / self.dpi as i32 - 42, 46),
                    39 | 45 => (
                        r.bottom * 96 / self.dpi as i32 - 42,
                        (self.inspector_width - 40) / 2,
                    ),
                    _ => (202, side - d(32)),
                };
                let px = if [50, 51, 54, 80, 81].contains(&child.id) {
                    r.right
                        - d(if [50, 51, 54, 80, 81].contains(&child.id) {
                            42
                        } else {
                            16
                        })
                        - w
                } else if child.id == 38 {
                    property_x
                        + d(30
                            * crate::platform::ui_preferences::recent_colors()
                                .len()
                                .min(4) as i32)
                } else if child.id == 61 {
                    property_x + d(54)
                } else {
                    property_x
                };
                if child.id == 38 {
                    v::choice(child.handle, v::ChoiceVisual::Swatch(self.color));
                }
                if child.id == 62 {
                    ui::text(child.handle, "取消");
                    v::style(child.handle, None, false, false, false);
                }
                if [61, 39, 45].contains(&child.id) {
                    v::style(child.handle, None, false, true, false);
                }
                ui::place(
                    child.handle,
                    px,
                    d(y),
                    if [61, 62, 39, 45].contains(&child.id) {
                        d(w)
                    } else {
                        w
                    },
                    d(if child.id == 38 {
                        26
                    } else if [50, 51, 54, 80, 81].contains(&child.id) {
                        28
                    } else {
                        30
                    }),
                );
            }
        }
        ui::show(
            self.text_input,
            text && side > 0 && (self.selected.is_some() || self.inline_text.is_some()),
        );
        ui::place(
            self.text_input,
            property_x,
            d(409),
            (side - d(32)).max(1),
            d(96).min((r.bottom - d(465)).max(d(40))),
        );
        if let Some(fonts) = &self.font_controls {
            fonts.show(text && side > 0);
            for h in fonts.handles() {
                unsafe {
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                        h,
                        self.inline_text.is_some() || self.selected.is_some(),
                    );
                }
            }
            fonts.place(hwnd, property_x, d(193), (side - d(32)).max(1));
        }
        self.toolbar_height = 0;
        self.layout_inline_text(hwnd);
        self.display_cache = None;
        invalidate(hwnd);
    }
    fn more_tools(&mut self, hwnd: HWND) -> Result<(), String> {
        use crate::platform::app_menu::{self, Row};
        use editor_ui::IconKind as I;
        let rows = BUTTONS
            .iter()
            .filter(|(id, _, _)| {
                ![
                    30, 36, 3, 1, 2, 4, 5, 7, 11, 60, 38, 39, 45, 46, 61, 62, 63, 64, 65, 66, 82,
                ]
                .contains(id)
            })
            .map(|(id, label, _)| Row::command(*id as i32, label, I::Edit, true))
            .collect();
        let t = editor_ui::EditorTheme::current(self.dpi);
        if let Some(id) = app_menu::show_rows(
            hwnd,
            RECT {
                left: t.scale(44),
                top: t.scale(350),
                right: t.scale(44),
                bottom: t.scale(365),
            },
            t,
            rows,
            false,
        )
        .map_err(|e| e.to_string())?
        {
            self.command(hwnd, id as usize)?;
        }
        Ok(())
    }
    pub fn relayout(&mut self, hwnd: HWND) {
        if self.pinned {
            return;
        }
        self.dpi = editor_ui::window_dpi(hwnd);
        self.desktop_layout(hwnd);
    }
    fn canvas_area(&self, hwnd: HWND) -> RECT {
        let mut r = crate::platform::workspace_ui::rect(hwnd);
        if !self.pinned {
            r.left = 44 * self.dpi as i32 / 96;
            r.top = self.toolbar_height;
            r.bottom -= 38 * self.dpi as i32 / 96;
            r.right -= editor_ui::scale_dip(self.inspector_width, self.dpi);
        }
        r
    }
    pub fn view(&self, hwnd: HWND) -> RECT {
        if self.pinned {
            return crate::platform::workspace_ui::rect(hwnd);
        }
        self.viewport
            .rect(self.canvas_area(hwnd), self.document.borrow().size())
    }
    fn image_point(&self, hwnd: HWND, point: POINT, clamp: bool) -> Option<POINT> {
        let rect = self.view(hwnd);
        let area = self.canvas_area(hwnd);
        if !clamp
            && (point.x < area.left
                || point.x >= area.right
                || point.y < area.top
                || point.y >= area.bottom)
        {
            return None;
        }
        if !clamp
            && (point.x < rect.left
                || point.y < rect.top
                || point.x >= rect.right
                || point.y >= rect.bottom)
        {
            return None;
        }
        let document = self.document.borrow();
        let size = document.size();
        Some(document.source_point(POINT {
            x: ((i64::from(point.x - rect.left) * i64::from(size.0))
                / i64::from((rect.right - rect.left).max(1))) as i32,
            y: ((i64::from(point.y - rect.top) * i64::from(size.1))
                / i64::from((rect.bottom - rect.top).max(1))) as i32,
        }))
    }
    pub fn output(&self) -> Result<Raster, String> {
        if let Some(player) = &self.animation {
            Document::from_state(player.frame().clone(), self.document.borrow().state.clone())?
                .render(None)
        } else {
            self.document.borrow().render(None)
        }
    }
    pub(super) fn workspace_zoom(&self, hwnd: HWND) -> f64 {
        let area = self.canvas_area(hwnd);
        let size = self.document.borrow().size();
        self.viewport.scale(area, size)
            / Viewport {
                dpi: self.dpi,
                ..Viewport::default()
            }
            .scale(area, size)
            * 100.0
    }
    pub fn memory_bytes(&self) -> usize {
        self.document
            .borrow()
            .original
            .pixels
            .len()
            .saturating_add(self.animation.as_ref().map_or(0, |p| p.clip.bytes))
    }
    pub fn animation_timer(&mut self, hwnd: HWND) {
        if let Some(player) = &mut self.animation {
            let visible = unsafe { super::IsWindowVisible(hwnd) }.as_bool();
            player.set_visible(visible);
            unsafe {
                if visible && player.playing {
                    windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(hwnd), 91, 20, None);
                } else {
                    let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(hwnd), 91);
                }
            }
        }
    }
    fn input_text(&self) -> String {
        let mut buffer = vec![0u16; 4097];
        let count = unsafe { GetWindowTextW(self.text_input, &mut buffer) }.max(0) as usize;
        String::from_utf16_lossy(&buffer[..count])
    }
    fn cancel_gesture(&mut self, hwnd: HWND) {
        self.pending = None;
        self.object_drag = None;
        self.property_preview = None;
        self.crop_drag = None;
        self.pan_drag = None;
        self.drag = None;
        self.display_cache = None;
        unsafe {
            let _ = ReleaseCapture();
        }
        invalidate(hwnd);
    }
    fn crop_fields(&mut self) {
        self.syncing_crop_fields = true;
        if let Some(crop) = self.crop {
            let [l, t, r, b] = crop.pixels();
            for (id, value) in [(80, r - l), (81, b - t)] {
                if let Some(c) = self.children.iter().find(|c| c.id == id) {
                    let value = wide(&value.to_string());
                    unsafe {
                        let _ = SetWindowTextW(c.handle, PCWSTR(value.as_ptr()));
                    }
                }
            }
        }
        self.syncing_crop_fields = false;
    }
    fn select_properties(&mut self, hwnd: HWND) {
        self.property_preview = None;
        if let Some(mark) = self
            .selected
            .and_then(|i| self.document.borrow().state.marks.get(i).cloned())
        {
            self.color = mark.color;
            self.stroke = mark.stroke;
            self.filled = mark.filled;
            self.font_size = mark.font_size;
            self.font_style = mark.font.clone();
            self.sync_font_controls();
            let value = wide(&mark.text);
            unsafe {
                let _ = SetWindowTextW(self.text_input, PCWSTR(value.as_ptr()));
            }
            for (id, value) in [(50, self.stroke), (51, self.font_size)] {
                if let Some(c) = self.children.iter().find(|c| c.id == id) {
                    crate::platform::workspace_ui::text(c.handle, &value.to_string());
                }
            }
        }
        self.desktop_layout(hwnd);
    }
    fn commit_properties(&mut self, hwnd: HWND) -> Result<(), String> {
        if let (Some(index), Some(mark)) = (self.selected, self.property_preview.take()) {
            if self.document.borrow_mut().replace_mark(index, mark) {
                self.cached = None;
                self.display_cache = None;
                self.dirty = true;
                notify_document(hwnd, &self.document)?;
            }
        }
        Ok(())
    }
    fn sync_font_controls(&mut self) {
        self.syncing_text_fields = true;
        if let Some(fonts) = &mut self.font_controls {
            fonts.load(&self.font_style);
        }
        self.syncing_text_fields = false;
    }
    pub fn command(&mut self, hwnd: HWND, id: usize) -> Result<(), String> {
        if self.tool == Tool::Text
            && self.selected.is_none()
            && self.inline_text.is_none()
            && matches!(id, 38 | 51 | 54 | 110..=113)
        {
            return Ok(());
        }

        if (2100..=2104).contains(&id) || matches!(id, 2110 | 2111) {
            if self.syncing_text_fields || (self.inline_text.is_none() && self.selected.is_none()) {
                return Ok(());
            }
            let font = self.font_controls.as_ref().unwrap().read(id);
            if !font.valid() {
                return Ok(());
            }
            self.font_style = font;
            self.inline_font_height = 0;
            self.sync_font_controls();
            self.preview_properties();
            self.layout_inline_text(hwnd);
            self.display_cache = None;
            invalidate(hwnd);
            return Ok(());
        }
        if tool_for(id).is_some() || matches!(id,7..=11|37|39..=45) {
            self.finish_inline_text(hwnd, true)?;
        }
        if let Some(mapped) = match id {
            120 => Some(36),
            121 | 125 => Some(40),
            122 => Some(46),
            123 => Some(67),
            124 => Some(36),
            _ => None,
        } {
            return self.command(hwnd, mapped);
        }
        if (110..=113).contains(&id) {
            if let Some(mut color) = crate::platform::ui_preferences::recent_colors()
                .get(id - 110)
                .copied()
            {
                self.commit_properties(hwnd)?;
                color[3] = self.color[3];
                self.set_color_preview(hwnd, color);
                self.commit_properties(hwnd)?;
                crate::platform::ui_preferences::remember_color(color)?;
                self.desktop_layout(hwnd);
            }
            return Ok(());
        }
        if [100, 101].contains(&id) {
            self.filled = id == 101;
            self.preview_properties();
            self.desktop_layout(hwnd);
        }

        if id == 7 && self.property_preview.is_some() {
            self.property_preview = None;
            self.select_properties(hwnd);
            return Ok(());
        }
        if tool_for(id).is_some() || matches!(id, 9 | 10 | 11 | 40..=44) {
            self.commit_properties(hwnd)?;
        }
        if self.crop.is_some() && matches!(id, 9 | 10 | 11 | 44) {
            return Err("请先确认或取消裁剪，再保存或复制图片".into());
        }
        if let Some(tool) = tool_for(id) {
            if !self.pinned {
                if let Some(parent) = crate::platform::workspace::image_parent() {
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                            Some(parent),
                            WM_COMMAND,
                            WPARAM(18),
                            LPARAM(0),
                        );
                    }
                }
            }
            self.cancel_gesture(hwnd);
            if tool != Tool::Text || self.contextual_tool() != Tool::Text {
                self.selected = None;
            }
            self.recognition = None;
            self.tool = tool;
            self.crop = if tool == Tool::Crop {
                Some(Crop::new(self.document.borrow().size()))
            } else {
                None
            };
            if tool == Tool::Crop {
                if let Some(c) = self.children.iter().find(|c| c.id == 52) {
                    unsafe {
                        SendMessageW(c.handle, CB_SETCURSEL, Some(WPARAM(0)), None);
                    }
                }
                self.crop_fields();
            }
            self.desktop_layout(hwnd);
            unsafe {
                let _ = SetFocus(Some(hwnd));
            }
            return Ok(());
        }
        let mut changed = false;
        match id {
            60 => {
                self.more_tools(hwnd)?;
            }
            7 => {
                self.cancel_gesture(hwnd);
                self.crop = None;
                self.document.borrow_mut().undo();
                self.selected = None;
                changed = true;
            }
            8 => {
                self.cancel_gesture(hwnd);
                self.crop = None;
                self.document.borrow_mut().redo();
                self.selected = None;
                changed = true;
            }
            9 => {
                let image = self.output()?;
                copy_image(hwnd, &image)?;
                pin_manager::remember_image(&image)?;
                self.saved_revision = self.document.borrow().revision;
                self.dirty = false;
                title(hwnd, "Panzo · 图片已复制，可从历史找回");
            }
            10 => {
                if let Some(path) = save_dialog(hwnd, &self.library) {
                    let image = self.output()?;
                    image.save(&path)?;
                    crate::platform::asset_index::register(&self.library, &path)?;
                    pin_manager::remember_image(&image)?;
                    self.saved_revision = self.document.borrow().revision;
                    self.dirty = false;
                    title(hwnd, "Panzo · 图片已保存");
                }
            }
            11 => {
                create_picture(Rc::clone(&self.document), &self.library, true)?;
                self.dirty = false;
            }
            12 => self.color = [55, 65, 225, 255],
            13 => self.color = [220, 125, 45, 255],
            14 => self.color = [25, 25, 25, 255],
            21 => {
                let document = if self.animation.is_some() {
                    Rc::new(RefCell::new(Document::new(self.output()?)))
                } else {
                    Rc::clone(&self.document)
                };
                create_picture(document, &self.library, false)?;
            }
            46 | 67 | 68 if crate::platform::workspace::active() => {
                if let Some(parent) = crate::platform::workspace::image_parent() {
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                            Some(parent),
                            WM_COMMAND,
                            WPARAM(if id == 68 {
                                4
                            } else if id == 67 {
                                16
                            } else {
                                15
                            }),
                            LPARAM(0),
                        );
                    }
                }
            }
            46 => crate::platform::image_workflows::ocr(hwnd, self.output()?, &self.library)?,
            47 => {
                if let Some(player) = &mut self.animation {
                    player.toggle();
                    self.animation_timer(hwnd);
                    pin_manager::save_session()?;
                }
            }
            48 => {
                if let Some(values) = crate::platform::editor_dialog::form_with_description(
                    hwnd,
                    "贴图分组",
                    "同组贴图可统一显示、隐藏与排列。留空表示未分组。",
                    &[("组名", self.group.clone())],
                    &[],
                    "保存分组",
                )
                .map_err(|e| e.to_string())?
                {
                    let group = values[0].trim();
                    if group.chars().count() > 64 {
                        return Err("组名最多 64 个字符".into());
                    }
                    self.group = group.into();
                    pin_manager::save_session()?;
                }
            }

            22 => {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                pin_manager::save_session()?;
            }
            23 => unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            24 => {
                self.locked = !self.locked;
                pin_manager::save_session()?;
            }
            25 => {
                if !self.passthrough && !pin_manager::recovery_available() {
                    return Err("恢复快捷键与托盘均不可用，无法开启鼠标穿透".into());
                }
                self.passthrough = !self.passthrough;
                self.apply_pin_style(hwnd);
                pin_manager::save_session()?;
            }
            26 => {
                let mut rect = RECT::default();
                unsafe {
                    let _ = GetWindowRect(hwnd, &raw mut rect);
                }
                self.collapsed = !self.collapsed;
                let (width, height) = if self.collapsed {
                    self.expanded_size = (rect.right - rect.left, rect.bottom - rect.top);
                    let size = self.document.borrow().size();
                    (
                        160,
                        (160.0 * f64::from(size.1) / f64::from(size.0)).clamp(40.0, 200.0) as i32,
                    )
                } else {
                    self.expanded_size
                };
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        0,
                        0,
                        width,
                        height,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                pin_manager::save_session()?;
            }
            27 => {
                let rect = self.view(hwnd);
                let image = self.output()?;
                let rgba = image::RgbaImage::from_raw(image.width, image.height, image.pixels)
                    .ok_or("图片数据无效")?;
                let scaled = image::imageops::resize(
                    &rgba,
                    rect.right.max(1) as u32,
                    rect.bottom.max(1) as u32,
                    image::imageops::FilterType::Lanczos3,
                );
                copy_image(
                    hwnd,
                    &Raster {
                        width: scaled.width(),
                        height: scaled.height(),
                        pixels: scaled.into_raw(),
                    },
                )?;
            }
            37 => {
                if let Some(index) = self.selected.take() {
                    self.document.borrow_mut().delete(index);
                    changed = true;
                }
            }
            38 => {
                let anchor = self
                    .children
                    .iter()
                    .find(|c| c.id == 38)
                    .map_or(hwnd, |c| c.handle);
                self.commit_properties(hwnd)?;
                crate::platform::desktop_ui::open_color(hwnd, anchor, self.color, false)?;
                return Ok(());
            }
            39 | 45 => {
                if let Some(index) = self.selected {
                    let replacement =
                        self.document
                            .borrow()
                            .state
                            .marks
                            .get(index)
                            .cloned()
                            .map(|mut mark| {
                                if id == 45 {
                                    mark.text = self.input_text();
                                }
                                mark.color = self.color;
                                mark.stroke = self.stroke;
                                mark.filled = self.filled;
                                mark.font_size = self.font_size;
                                mark.font = self.font_style.clone();
                                mark
                            });
                    if let Some(mark) = replacement {
                        changed = self.document.borrow_mut().replace_mark(index, mark);
                    }
                }
            }
            90..=96 => {
                if let Some(c) = self.children.iter().find(|c| c.id == 52) {
                    unsafe {
                        SendMessageW(c.handle, CB_SETCURSEL, Some(WPARAM(id - 90)), None);
                    }
                }
                self.command(hwnd, 52)?;
                self.desktop_layout(hwnd);
            }
            52 => {
                let index = self
                    .children
                    .iter()
                    .find(|c| c.id == 52)
                    .map(|c| unsafe { SendMessageW(c.handle, CB_GETCURSEL, None, None).0 })
                    .unwrap_or(0);
                if let Some(crop) = &mut self.crop {
                    let (w, h) = crop.size;
                    crop.set_ratio(match index {
                        1 => Some(w as f64 / h as f64),
                        2 => Some(1.),
                        3 => Some(4. / 3.),
                        4 => Some(3. / 2.),
                        5 => Some(16. / 9.),
                        6 => Some(9. / 16.),
                        _ => None,
                    });
                    self.crop_fields();
                }
            }
            61 => {
                self.command(hwnd, 82)?;
                self.crop_drag = None;
                if let Some(crop) = self.crop.take() {
                    let before = self.document.borrow().revision;
                    self.document.borrow_mut().crop_output(crop.pixels());
                    changed = before != self.document.borrow().revision;
                    self.viewport = Viewport {
                        dpi: self.dpi,
                        ..Viewport::default()
                    };
                }
                self.tool = Tool::Select;
                self.desktop_layout(hwnd);
            }
            62 => {
                self.cancel_gesture(hwnd);
                self.crop = None;
                self.tool = Tool::Select;
                self.desktop_layout(hwnd);
            }
            63 => {
                self.viewport = Viewport {
                    dpi: self.dpi,
                    ..Viewport::default()
                };
                self.display_cache = None;
            }
            64 => {
                self.viewport = Viewport {
                    dpi: self.dpi,
                    zoom: Some(1.0),
                    pan: (0., 0.),
                };
                self.display_cache = None;
            }
            65 | 66 => {
                let area = self.canvas_area(hwnd);
                let anchor = POINT {
                    x: (area.left + area.right) / 2,
                    y: (area.top + area.bottom) / 2,
                };
                self.viewport.zoom_at(
                    area,
                    self.document.borrow().size(),
                    anchor,
                    if id == 66 { 1.25 } else { 0.8 },
                );
                self.display_cache = None;
            }
            82 => {
                if let Some(crop) = &mut self.crop {
                    let mut values = [0.; 2];
                    for (i, id) in [80, 81].iter().enumerate() {
                        let c = self
                            .children
                            .iter()
                            .find(|c| c.id == *id)
                            .ok_or("裁剪尺寸不可用")?;
                        values[i] = crate::platform::workspace_ui::value(c.handle)
                            .trim()
                            .parse::<u32>()
                            .map_err(|_| "宽度和高度请输入正整数")?
                            as f64;
                    }
                    let w = values[0];
                    let h = crop.ratio.map_or(values[1], |r| w / r);
                    if w < 1. || h < 1. || w > crop.size.0 as f64 || h > crop.size.1 as f64 {
                        return Err("裁剪尺寸超出当前图片范围".into());
                    }
                    let l = crop.rect[0].min(crop.size.0 as f64 - w);
                    let t = crop.rect[1].min(crop.size.1 as f64 - h);
                    crop.rect = [l, t, l + w, t + h];
                    self.crop_fields();
                }
            }
            40..=43 => {
                self.cancel_gesture(hwnd);
                self.crop = None;
                self.viewport = Viewport {
                    dpi: self.dpi,
                    ..Viewport::default()
                };
                self.document.borrow_mut().transform(id);
                self.selected = None;
                changed = true;
            }
            44 => {
                pin_manager::remember_image(&self.output()?)?;
                self.saved_revision = self.document.borrow().revision;
                self.dirty = false;
                title(hwnd, "Panzo · 已快速保存到截图历史");
            }
            50 | 51 | 54 => {
                if let Some(child) = self.children.iter().find(|child| child.id == id) {
                    let max = if id == 54 {
                        100
                    } else if id == 50 {
                        128
                    } else {
                        512
                    };
                    if let Ok(value) = crate::platform::workspace_ui::value(child.handle)
                        .trim()
                        .parse::<u32>()
                    {
                        if ((if id == 54 { 0 } else { 1 })..=max).contains(&value) {
                            if id == 54 {
                                self.color[3] = (value * 255 / 100) as u8;
                            } else if id == 50 {
                                self.stroke = value;
                            } else {
                                self.font_size = value;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        if [38, 50, 51, 54].contains(&id) {
            self.preview_properties();
            self.layout_inline_text(hwnd);
        }
        if changed {
            if self.crop.is_none() && self.tool == Tool::Crop {
                self.tool = Tool::Select;
            }
            self.property_preview = None;
            self.dirty = true;
            self.cached = None;
            self.display_cache = None;
            if !self.pinned {
                self.desktop_layout(hwnd);
            }
            self.sync_pin_size(hwnd);
            notify_document(hwnd, &self.document)?;
        }
        if ![50, 51, 52, 54, 45, 80, 81, 82].contains(&id) {
            unsafe {
                let _ = SetFocus(Some(hwnd));
            }
        }
        invalidate(hwnd);
        Ok(())
    }
    fn sync_pin_size(&mut self, hwnd: HWND) {
        let size = self.document.borrow().size();
        if self.pinned && size != self.display_size {
            let mut rect = RECT::default();
            unsafe {
                let _ = GetWindowRect(hwnd, &raw mut rect);
            }
            let scale = f64::from(rect.right - rect.left) / f64::from(self.display_size.0.max(1));
            let width = (f64::from(size.0) * scale).clamp(40.0, 4096.0) as i32;
            let height = (f64::from(size.1) * scale).clamp(24.0, 4096.0) as i32;
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    width,
                    height,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            pin_manager::ensure_visible(hwnd);
        }
        self.display_size = size;
    }
    pub fn apply_pin_style(&self, hwnd: HWND) {
        unsafe {
            let mut style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            if self.passthrough {
                style |= WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0;
            } else {
                style &= !(WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0);
            }
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style as isize);
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), self.opacity, LWA_ALPHA);
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
    fn set_color_preview(&mut self, hwnd: HWND, color: [u8; 4]) {
        use crate::platform::{desktop_ui as v, workspace_ui as ui};
        self.color = color;
        self.preview_properties();
        self.syncing_color_fields = true;
        for child in &self.children {
            match child.id {
                38 => v::choice(child.handle, v::ChoiceVisual::Swatch(color)),
                110..=113 => v::selected(
                    child.handle,
                    crate::platform::ui_preferences::recent_colors()
                        .get(child.id - 110)
                        .is_some_and(|c| color[..3] == c[..3]),
                ),
                54 => ui::text(child.handle, &(u32::from(color[3]) * 100 / 255).to_string()),
                _ => {}
            }
        }
        self.syncing_color_fields = false;
        self.layout_inline_text(hwnd);
        invalidate(hwnd);
    }
    fn preview_properties(&mut self) {
        if self.inline_text.is_some() {
            self.property_preview = None;
            return;
        }
        self.property_preview = self
            .selected
            .and_then(|i| self.document.borrow().state.marks.get(i).cloned())
            .and_then(|original| {
                let mut mark = original.clone();
                mark.color = self.color;
                mark.stroke = self.stroke;
                mark.filled = self.filled;
                mark.font_size = self.font_size;
                mark.font = self.font_style.clone();
                if mark.tool == Tool::Text {
                    mark.text = self.input_text();
                }
                (!original.same(&mark)).then_some(mark)
            });
    }
    fn screen_mark(&self, mark: &Mark, view: RECT, area: RECT) -> Mark {
        let document = self.document.borrow();
        let (w, h) = document.size();
        let scale = (view.right - view.left) as f64 / w as f64;
        let mut result = mark.clone();
        result.points = mark
            .points
            .iter()
            .map(|p| {
                let p = document.display_point(*p);
                POINT {
                    x: view.left - area.left + (p.x as f64 * scale).round() as i32,
                    y: view.top - area.top
                        + (p.y as f64 * (view.bottom - view.top) as f64 / h as f64).round() as i32,
                }
            })
            .collect();
        result.stroke = (mark.stroke as f64 * scale).round().max(1.) as u32;
        result.font_size = (mark.font_size as f64 * scale).round().max(1.) as u32;
        result.text_box = mark
            .text_box
            .map(|s| s.map(|v| (v as f64 * scale).round().max(1.) as u32));
        result
    }
    fn preview_mark(
        &self,
        image: &mut Raster,
        mark: &Mark,
        view: RECT,
        area: RECT,
    ) -> Result<(), String> {
        if !matches!(mark.tool, Tool::Text | Tool::Number) {
            return super::image_document::render_mark(image, &self.screen_mark(mark, view, area));
        }
        let d = self.document.borrow();
        let (width, _) = d.size();
        let scale = (view.right - view.left) as f64 / width as f64;
        let r = mark.bounds();
        let r = RECT {
            right: r.right.min(d.original.width as i32),
            bottom: r.bottom.min(d.original.height as i32),
            ..r
        };
        if r.right <= r.left || r.bottom <= r.top {
            return Ok(());
        }
        // Never allocate an enlarged whole-image preview: at >100%, sample the
        // source-sized text mask into the visible viewport instead.
        let resolution = scale.min(1.0);
        let w = ((r.right - r.left) as f64 * resolution).ceil().max(1.) as u32;
        let h = ((r.bottom - r.top) as f64 * resolution).ceil().max(1.) as u32;
        let mut mask = Raster {
            width: w,
            height: h,
            pixels: vec![0; Raster::bytes(w, h)?],
        };
        let mut local = mark.clone();
        local.points = mark
            .points
            .iter()
            .map(|p| POINT {
                x: ((p.x - r.left) as f64 * resolution).round() as i32,
                y: ((p.y - r.top) as f64 * resolution).round() as i32,
            })
            .collect();
        local.font_size = (mark.font_size as f64 * resolution).round().max(1.) as u32;
        local.text_box = mark
            .text_box
            .map(|s| s.map(|v| (v as f64 * resolution).round().max(1.) as u32));
        super::image_document::render_mark(&mut mask, &local)?;
        let mut mask = image::RgbaImage::from_raw(w, h, mask.pixels).ok_or("文字预览数据无效")?;
        mask = match d.state.rotation {
            1 => image::imageops::rotate90(&mask),
            2 => image::imageops::rotate180(&mask),
            3 => image::imageops::rotate270(&mask),
            _ => mask,
        };
        if d.state.flip_x {
            image::imageops::flip_horizontal_in_place(&mut mask);
        }
        if d.state.flip_y {
            image::imageops::flip_vertical_in_place(&mut mask);
        }
        let a = d.display_edge(POINT {
            x: r.left,
            y: r.top,
        });
        let b = d.display_edge(POINT {
            x: r.right,
            y: r.bottom,
        });
        let left = view.left - area.left + (a.x.min(b.x) as f64 * scale).round() as i32;
        let top = view.top - area.top + (a.y.min(b.y) as f64 * scale).round() as i32;
        let dw = ((a.x - b.x).abs() as f64 * scale).round().max(1.) as i32;
        let dh = ((a.y - b.y).abs() as f64 * scale).round().max(1.) as i32;
        for y in top.max(0)..(top + dh).min(image.height as i32) {
            for x in left.max(0)..(left + dw).min(image.width as i32) {
                let sx = ((x - left) as u64 * mask.width() as u64 / dw as u64) as u32;
                let sy = ((y - top) as u64 * mask.height() as u64 / dh as u64) as u32;
                super::image_document::blend_pixel(image, x, y, mask.get_pixel(sx, sy).0);
            }
        }
        Ok(())
    }
    fn crop_screen(&self, hwnd: HWND, crop: Crop) -> RECT {
        let view = self.view(hwnd);
        let (w, h) = crop.size;
        let [l, t, r, b] = crop.rect;
        RECT {
            left: view.left + (l * (view.right - view.left) as f64 / w as f64).round() as i32,
            top: view.top + (t * (view.bottom - view.top) as f64 / h as f64).round() as i32,
            right: view.left + (r * (view.right - view.left) as f64 / w as f64).round() as i32,
            bottom: view.top + (b * (view.bottom - view.top) as f64 / h as f64).round() as i32,
        }
    }
    fn crop_hit(&self, hwnd: HWND, p: POINT) -> Option<usize> {
        let r = self.crop_screen(hwnd, self.crop?);
        let hit = (9 * self.dpi as i32 / 96).max(7);
        let handles = [
            (r.left, r.top),
            ((r.left + r.right) / 2, r.top),
            (r.right, r.top),
            (r.right, (r.top + r.bottom) / 2),
            (r.right, r.bottom),
            ((r.left + r.right) / 2, r.bottom),
            (r.left, r.bottom),
            (r.left, (r.top + r.bottom) / 2),
        ];
        handles
            .iter()
            .enumerate()
            .find(|(i, (x, y))| i % 2 == 0 && (p.x - x).abs() <= hit && (p.y - y).abs() <= hit)
            .map(|(i, _)| i)
            .or_else(|| {
                (p.x >= r.left && p.x <= r.right && p.y >= r.top && p.y <= r.bottom).then_some(8)
            })
    }
    fn paint(&mut self, hwnd: HWND, dc: HDC) {
        let theme = editor_ui::EditorTheme::current(self.dpi);
        let client = crate::platform::workspace_ui::rect(hwnd);
        let area = self.canvas_area(hwnd);
        let view = self.view(hwnd);
        let background = crate::platform::desktop_ui::canvas(theme);
        fill(dc, client, background);
        let revision = self.document.borrow().revision;
        let excluded = self
            .object_drag
            .as_ref()
            .map(|d| d.index)
            .or(self.property_preview.as_ref().and(self.selected))
            .or(self.inline_text.as_ref().and_then(|v| v.index));
        let draft = self
            .inline_mark()
            .or_else(|| {
                self.property_preview
                    .as_ref()
                    .filter(|m| m.tool == Tool::Text)
                    .cloned()
            })
            .or_else(|| {
                self.object_drag
                    .as_ref()
                    .filter(|d| d.preview.tool == Tool::Text)
                    .map(|d| d.preview.clone())
            });
        let stale = self.display_cache.as_ref().is_none_or(|c| {
            (match (&c.draft, &draft) {
                (None, None) => false,
                (Some(a), Some(b)) => !a.same(b),
                _ => true,
            }) || c.revision != revision
                || c.excluded != excluded
                || c.view != view
                || c.area != area
                || c.background != background.0
        });
        if stale && area.right > area.left && area.bottom > area.top {
            // A resize borrows the rendered document. Only an edit creates new pixels.
            // Document pixels outlive viewport changes. Only changed text regions
            // update preview levels; moving the viewport only blits these levels.
            let excluded_image = if draft.is_none() {
                excluded.and_then(|index| self.document.borrow().render_without(Some(index)).ok())
            } else {
                None
            };
            if let Some(mark) = &draft {
                picture_preview::DraftCache::update(
                    &mut self.draft_cache,
                    &self.document.borrow(),
                    mark,
                    excluded,
                );
            } else {
                self.draft_cache = None;
            }
            let draft_image = self
                .draft_cache
                .as_ref()
                .map(|cache| cache.image(view))
                .or(excluded_image.as_ref());
            if draft.is_none()
                && excluded.is_none()
                && self.cached.as_ref().is_none_or(|(v, _, _)| *v != revision)
            {
                self.cached = self.output().ok().map(|image| {
                    let levels = preview_levels(&image);
                    (revision, image, levels)
                });
            }
            let image = draft_image.or_else(|| {
                self.cached.as_ref().map(|(_, original, levels)| {
                    levels
                        .iter()
                        .find(|level| {
                            level.width >= (view.right - view.left).max(1) as u32
                                && level.height >= (view.bottom - view.top).max(1) as u32
                        })
                        .unwrap_or(original)
                })
            });
            if let Some(image) = image {
                let w = (area.right - area.left) as u32;
                let h = (area.bottom - area.top) as u32;
                if let Ok(surface) = Surface::new(w, h) {
                    fill(
                        surface.dc,
                        RECT {
                            left: 0,
                            top: 0,
                            right: w as i32,
                            bottom: h as i32,
                        },
                        background,
                    );
                    draw_raster(
                        surface.dc,
                        &image,
                        RECT {
                            left: view.left - area.left,
                            top: view.top - area.top,
                            right: view.right - area.left,
                            bottom: view.bottom - area.top,
                        },
                    );
                    self.display_cache = Some(DisplayCache {
                        draft: draft.clone(),
                        revision,
                        excluded,
                        view,
                        area,
                        background: background.0,
                        surface,
                        raster: std::cell::OnceCell::new(),
                    });
                }
            }
        }
        if let Some(cache) = &self.display_cache {
            let overlay = self
                .object_drag
                .as_ref()
                .map(|d| &d.preview)
                .or(self
                    .property_preview
                    .as_ref()
                    .filter(|_| self.inline_text.is_none()))
                .or(self.pending.as_ref())
                .filter(|m| draft.is_none() || m.tool != Tool::Text);
            if overlay.is_some() || self.crop.is_some() {
                let mut image = cache
                    .raster
                    .get_or_init(|| {
                        cache.surface.raster(
                            (area.right - area.left) as u32,
                            (area.bottom - area.top) as u32,
                        )
                    })
                    .clone();
                if let Some(mark) = overlay {
                    let _ = self.preview_mark(&mut image, mark, view, area);
                }
                if let Some(crop) = self.crop {
                    let r = self.crop_screen(hwnd, crop);
                    let gx = [
                        r.left + (r.right - r.left) / 3,
                        r.left + (r.right - r.left) * 2 / 3,
                    ];
                    let gy = [
                        r.top + (r.bottom - r.top) / 3,
                        r.top + (r.bottom - r.top) * 2 / 3,
                    ];
                    for y in 0..image.height as i32 {
                        for x in 0..image.width as i32 {
                            let px = x + area.left;
                            let py = y + area.top;
                            if px >= view.left
                                && px < view.right
                                && py >= view.top
                                && py < view.bottom
                                && (px < r.left || px >= r.right || py < r.top || py >= r.bottom)
                            {
                                let i = (y as usize * image.width as usize + x as usize) * 4;
                                for c in &mut image.pixels[i..i + 3] {
                                    *c = (*c as u16 * 136 / 255) as u8;
                                }
                            } else if px >= r.left
                                && px < r.right
                                && py >= r.top
                                && py < r.bottom
                                && (gx.contains(&px) || gy.contains(&py))
                            {
                                let i = (y as usize * image.width as usize + x as usize) * 4;
                                for c in &mut image.pixels[i..i + 3] {
                                    *c = ((*c as u16 * 170 + 255 * 85) / 255) as u8;
                                }
                            }
                        }
                    }
                }
                draw_raster(dc, &image, area);
            } else {
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::BitBlt(
                        dc,
                        area.left,
                        area.top,
                        area.right - area.left,
                        area.bottom - area.top,
                        Some(cache.surface.dc),
                        0,
                        0,
                        windows::Win32::Graphics::Gdi::SRCCOPY,
                    );
                }
            }
        }
        // Editor adornments are independent of document pixels and undo history.
        let saved = unsafe { windows::Win32::Graphics::Gdi::SaveDC(dc) };
        unsafe {
            windows::Win32::Graphics::Gdi::IntersectClipRect(
                dc,
                area.left,
                area.top,
                area.right,
                area.bottom,
            );
        }
        if let Some(crop) = self.crop {
            let r = self.crop_screen(hwnd, crop);
            let ink = editor_ui::rgb(247, 250, 252);
            let line = |rect| fill(dc, rect, ink);
            for rect in [
                RECT {
                    left: r.left,
                    top: r.top,
                    right: r.right,
                    bottom: r.top + 1,
                },
                RECT {
                    left: r.left,
                    top: r.bottom - 1,
                    right: r.right,
                    bottom: r.bottom,
                },
                RECT {
                    left: r.left,
                    top: r.top,
                    right: r.left + 1,
                    bottom: r.bottom,
                },
                RECT {
                    left: r.right - 1,
                    top: r.top,
                    right: r.right,
                    bottom: r.bottom,
                },
            ] {
                line(rect);
            }
            let handles = [
                (r.left, r.top),
                ((r.left + r.right) / 2, r.top),
                (r.right, r.top),
                (r.right, (r.top + r.bottom) / 2),
                (r.right, r.bottom),
                ((r.left + r.right) / 2, r.bottom),
                (r.left, r.bottom),
                (r.left, (r.top + r.bottom) / 2),
            ];
            for (i, (x, y)) in handles.into_iter().enumerate() {
                if i % 2 != 0 {
                    continue;
                }
                let n = (5 * self.dpi as i32 / 96).max(3);
                fill(
                    dc,
                    crate::platform::desktop_ui::rect(x - n, y - n, 2 * n, 2 * n),
                    editor_ui::rgb(85, 85, 85),
                );
                fill(
                    dc,
                    RECT {
                        left: x - n + 1,
                        top: y - n + 1,
                        right: x + n - 1,
                        bottom: y + n - 1,
                    },
                    ink,
                );
            }
            let [l, t, rr, b] = crop.pixels();
            let text = format!("{} × {}", rr - l, b - t);
            let width = theme.scale((text.chars().count() as i32 * 6 + 14).max(76));
            let label = crate::platform::desktop_ui::rect(
                ((r.left + r.right - width) / 2)
                    .clamp(area.left, (area.right - width).max(area.left)),
                (r.bottom + theme.scale(7)).min(area.bottom - theme.scale(22)),
                width,
                theme.scale(20),
            );
            editor_ui::rounded_surface(
                dc,
                label,
                editor_ui::rgb(22, 26, 31),
                editor_ui::rgb(22, 26, 31),
                theme.scale(3),
            );
            crate::platform::desktop_ui::text_style(
                dc,
                label,
                &text,
                editor_ui::rgb(255, 255, 255),
                self.dpi,
                true,
                TextStyle::Micro,
            );
        } else if let Some(mark) = self
            .selected
            .filter(|_| self.inline_text.is_none())
            .and_then(|i| self.document.borrow().state.marks.get(i).cloned())
        {
            let mark = self.object_drag.as_ref().map_or(&mark, |d| &d.preview);
            let b = mark.bounds();
            let corners = [
                POINT {
                    x: b.left,
                    y: b.top,
                },
                POINT {
                    x: b.right,
                    y: b.top,
                },
                POINT {
                    x: b.right,
                    y: b.bottom,
                },
                POINT {
                    x: b.left,
                    y: b.bottom,
                },
            ];
            let mut bounds = mark.clone();
            bounds.points = corners.to_vec();
            let mut projected = self.screen_mark(&bounds, view, RECT::default());
            projected.tool = Tool::Rectangle;
            let r = projected.bounds();
            let ink = theme.palette.text_secondary;
            for (x, y) in [
                (r.left, r.top),
                (r.right, r.top),
                (r.right, r.bottom),
                (r.left, r.bottom),
            ] {
                fill(
                    dc,
                    RECT {
                        left: x - 3,
                        top: y - 3,
                        right: x + 3,
                        bottom: y + 3,
                    },
                    ink,
                );
            }
        }
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::RestoreDC(dc, saved);
        }
        if !self.pinned {
            use crate::platform::desktop_ui as v;
            let d = |n| theme.scale(n);
            let side = d(self.inspector_width);
            let x = client.right - side;
            fill(dc, v::rect(0, 0, d(44), client.bottom), theme.palette.panel);
            v::line(dc, v::rect(d(11), d(276), d(22), 1), theme);
            fill(
                dc,
                v::rect(d(44), client.bottom - d(38), x - d(44), d(38)),
                theme.palette.panel,
            );
            v::line(dc, v::rect(d(43), 0, 1, client.bottom), theme);
            v::line(
                dc,
                v::rect(d(44), client.bottom - d(38), x - d(44), 1),
                theme,
            );
            let (w, h) = self.document.borrow().size();
            v::text(
                dc,
                v::rect(d(56), client.bottom - d(32), x - d(156), d(28)),
                &format!("{w} × {h} px"),
                theme.palette.text_muted,
                self.dpi,
                false,
            );
            if side > 0 {
                fill(dc, v::rect(x, 0, side, client.bottom), theme.palette.panel);
                v::line(dc, v::rect(x, 0, 1, client.bottom), theme);
                let tool = self.contextual_tool();
                let name = if self.crop.is_some() {
                    "裁剪比例"
                } else {
                    match tool {
                        Tool::Select => "图片",
                        Tool::Rectangle => "矩形",
                        Tool::Ellipse => "椭圆",
                        Tool::Text => "文字",
                        Tool::Pen => "画笔",
                        Tool::Arrow => "箭头",
                        Tool::Mosaic => "马赛克",
                        Tool::Blur => "模糊",
                        _ => "标注属性",
                    }
                };
                v::text_style(
                    dc,
                    v::rect(x + d(16), d(14), side - d(32), d(20)),
                    name,
                    theme.palette.text,
                    self.dpi,
                    false,
                    TextStyle::Heading,
                );
                let label = |y, text: &str| {
                    v::text(
                        dc,
                        v::rect(
                            x + d(16),
                            d(y),
                            side - d(32),
                            d(if tool == Tool::Text { 20 } else { 24 }),
                        ),
                        text,
                        theme.palette.text_secondary,
                        self.dpi,
                        false,
                    )
                };
                let divider = |y| v::line(dc, v::rect(x + d(16), d(y), side - d(32), 1), theme);
                if self.crop.is_some() {
                    editor_ui::rounded_surface(
                        dc,
                        v::rect(x + d(16), d(45), side - d(32), d(33)),
                        theme.palette.raised,
                        theme.palette.raised,
                        d(4),
                    );
                    divider(92);
                    v::text_style(
                        dc,
                        v::rect(x + d(16), d(106), side - d(32), d(20)),
                        "裁剪尺寸",
                        theme.palette.text,
                        self.dpi,
                        false,
                        TextStyle::Heading,
                    );
                    label(140, "宽度");
                    label(178, "高度");
                    for y in [140, 178] {
                        editor_ui::rounded_surface(
                            dc,
                            v::rect(
                                if tool == Tool::Text && y == 333 {
                                    x + d(16)
                                } else {
                                    client.right - d(108)
                                },
                                d(y),
                                if tool == Tool::Text && y == 333 {
                                    side - d(32)
                                } else {
                                    d(92)
                                },
                                d(28),
                            ),
                            theme.palette.raised,
                            theme.palette.raised,
                            d(3),
                        );
                        v::text_style(
                            dc,
                            v::rect(client.right - d(38), d(y), d(20), d(28)),
                            "px",
                            theme.palette.text_muted,
                            self.dpi,
                            false,
                            TextStyle::Caption,
                        );
                    }
                    divider(227);
                    v::text_style(
                        dc,
                        v::rect(x + d(16), d(241), side - d(32), d(20)),
                        "调整",
                        theme.palette.text,
                        self.dpi,
                        false,
                        TextStyle::Heading,
                    );
                    v::text_style(
                        dc,
                        v::rect(client.right - d(108), d(276), d(92), d(30)),
                        &format!("{w} × {h}"),
                        theme.palette.text_muted,
                        self.dpi,
                        true,
                        TextStyle::Caption,
                    );
                } else if tool == Tool::Select {
                    let name = crate::platform::image_workspace::document_name(hwnd);
                    editor_ui::draw_icon(
                        dc,
                        v::rect(x + d(16), d(48), d(17), d(17)),
                        P::IMAGE,
                        theme.palette.text_secondary,
                    );
                    v::text(
                        dc,
                        v::rect(x + d(41), d(45), side - d(57), d(22)),
                        &name,
                        theme.palette.text,
                        self.dpi,
                        false,
                    );
                    v::text_style(
                        dc,
                        v::rect(x + d(41), d(72), side - d(57), d(17)),
                        &format!("{w} × {h}"),
                        theme.palette.text_muted,
                        self.dpi,
                        false,
                        TextStyle::Caption,
                    );
                    divider(103);
                    v::text_style(
                        dc,
                        v::rect(x + d(16), d(117), side - d(32), d(20)),
                        "画布",
                        theme.palette.text,
                        self.dpi,
                        false,
                        TextStyle::Heading,
                    );
                    for (y, name, value) in [(151, "宽度", w), (189, "高度", h)] {
                        label(y, name);
                        editor_ui::rounded_surface(
                            dc,
                            v::rect(
                                if tool == Tool::Text && y == 333 {
                                    x + d(16)
                                } else {
                                    client.right - d(108)
                                },
                                d(y),
                                if tool == Tool::Text && y == 333 {
                                    side - d(32)
                                } else {
                                    d(92)
                                },
                                d(28),
                            ),
                            theme.palette.raised,
                            theme.palette.raised,
                            d(3),
                        );
                        v::text(
                            dc,
                            v::rect(client.right - d(106), d(y), d(64), d(28)),
                            &value.to_string(),
                            theme.palette.text,
                            self.dpi,
                            true,
                        );
                        v::text_style(
                            dc,
                            v::rect(client.right - d(38), d(y), d(20), d(28)),
                            "px",
                            theme.palette.text_muted,
                            self.dpi,
                            false,
                            TextStyle::Caption,
                        );
                    }
                    divider(233);
                    v::text_style(
                        dc,
                        v::rect(x + d(16), d(247), side - d(32), d(20)),
                        "常用操作",
                        theme.palette.text,
                        self.dpi,
                        false,
                        TextStyle::Heading,
                    );
                } else {
                    if matches!(tool, Tool::Text | Tool::Number) {
                        editor_ui::draw_icon(
                            dc,
                            v::rect(x + d(16), d(58), d(17), d(17)),
                            P::TEXT,
                            theme.palette.text_secondary,
                        );
                    } else {
                        let diameter = d(self.stroke as i32).clamp(d(2), d(28));
                        let circle = v::rect(
                            x + d(27) - diameter / 2,
                            d(67) - diameter / 2,
                            diameter,
                            diameter,
                        );
                        editor_ui::rounded_surface(
                            dc,
                            circle,
                            theme.palette.text_secondary,
                            theme.palette.text_secondary,
                            diameter / 2,
                        );
                    }
                    for (y, unit) in [
                        (
                            53,
                            if matches!(tool, Tool::Text | Tool::Number) {
                                "px"
                            } else {
                                "px"
                            },
                        ),
                        (if tool == Tool::Text { 333 } else { 260 }, "%"),
                    ] {
                        if y == 260 && matches!(tool, Tool::Mosaic | Tool::Blur) {
                            continue;
                        }
                        editor_ui::rounded_surface(
                            dc,
                            v::rect(
                                if tool == Tool::Text && y == 333 {
                                    x + d(16)
                                } else {
                                    client.right - d(108)
                                },
                                d(y),
                                if tool == Tool::Text && y == 333 {
                                    side - d(32)
                                } else {
                                    d(92)
                                },
                                d(28),
                            ),
                            theme.palette.raised,
                            theme.palette.raised,
                            d(3),
                        );
                        v::text_style(
                            dc,
                            v::rect(client.right - d(38), d(y), d(20), d(28)),
                            unit,
                            theme.palette.text_muted,
                            self.dpi,
                            false,
                            TextStyle::Caption,
                        );
                    }
                    if !matches!(tool, Tool::Mosaic | Tool::Blur) {
                        divider(100);
                        v::text_style(
                            dc,
                            v::rect(x + d(16), d(114), side - d(32), d(20)),
                            "颜色",
                            theme.palette.text,
                            self.dpi,
                            false,
                            TextStyle::Heading,
                        );
                        divider(179);
                        if tool != Tool::Text {
                            v::text_style(
                                dc,
                                v::rect(x + d(16), d(193), side - d(32), d(20)),
                                "样式",
                                theme.palette.text,
                                self.dpi,
                                false,
                                TextStyle::Heading,
                            );
                            editor_ui::rounded_surface(
                                dc,
                                v::rect(x + d(16), d(222), side - d(32), d(33)),
                                theme.palette.raised,
                                theme.palette.raised,
                                d(4),
                            );
                        }
                        label(if tool == Tool::Text { 305 } else { 260 }, "不透明度");
                    }
                    if tool == Tool::Text {
                        label(381, "文字内容");
                        if self.selected.is_none() && self.inline_text.is_none() {
                            v::text(
                                dc,
                                v::rect(x + d(16), d(409), side - d(32), d(48)),
                                "点击图片输入文字",
                                theme.palette.text_muted,
                                self.dpi,
                                false,
                            );
                        }
                    }
                }
                if self.crop.is_some() || self.selected.is_some() {
                    v::line(dc, v::rect(x, client.bottom - d(54), side, 1), theme);
                }
            }
        }
    }

    fn buffered_paint(&mut self, hwnd: HWND, target: HDC) {
        let r = crate::platform::workspace_ui::rect(hwnd);
        let w = r.right.max(1) as u32;
        let h = r.bottom.max(1) as u32;
        let buffer = self
            .framebuffer
            .take()
            .filter(|(bw, bh, _)| *bw == w && *bh == h)
            .or_else(|| Surface::new(w, h).ok().map(|surface| (w, h, surface)));
        if let Some((w, h, surface)) = buffer {
            self.paint(hwnd, surface.dc);
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::BitBlt(
                    target,
                    0,
                    0,
                    w as i32,
                    h as i32,
                    Some(surface.dc),
                    0,
                    0,
                    windows::Win32::Graphics::Gdi::SRCCOPY,
                );
            }
            self.framebuffer = Some((w, h, surface));
        }
    }
}
pub(super) fn notify_document(hwnd: HWND, document: &SharedDocument) -> Result<(), String> {
    for other in PICTURES.with(|list| list.borrow().clone()) {
        if other == hwnd {
            continue;
        }
        if let Some(state) = unsafe { picture_mut(other) }
            && Rc::ptr_eq(&state.document, document)
        {
            state.cached = None;
            state.display_cache = None;
            state.dirty = true;
            state.sync_pin_size(other);
            invalidate(other);
        }
    }
    pin_manager::save_session()
}
pub(super) unsafe fn picture_mut(hwnd: HWND) -> Option<&'static mut Picture> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Picture;
    unsafe { pointer.as_mut() }
}
pub(super) fn dialog_message(message: &windows::Win32::UI::WindowsAndMessaging::MSG) -> bool {
    if message.message != WM_KEYDOWN {
        return false;
    }
    for hwnd in PICTURES.with(|list| list.borrow().clone()) {
        if message.hwnd == hwnd || unsafe { IsChild(hwnd, message.hwnd) }.as_bool() {
            if let Some(state) = unsafe { picture_mut(hwnd) }
                && !state.pinned
            {
                if message.hwnd == state.inline_input {
                    return false;
                }
                if message.wParam.0 == 9 {
                    return unsafe { IsDialogMessageW(hwnd, message) }.as_bool();
                }
                if message.hwnd != hwnd
                    && ((message.wParam.0 == 13 && state.crop.is_some()) || message.wParam.0 == 27)
                {
                    unsafe {
                        SendMessageW(hwnd, WM_KEYDOWN, Some(message.wParam), Some(message.lParam));
                    }
                    return true;
                }
            }
            break;
        }
    }
    false
}
unsafe extern "system" fn picture_proc(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    if let Some(r) = crate::platform::app_menu::window_message(message, lp) {
        return r;
    }
    let Some(state) = (unsafe { picture_mut(hwnd) }) else {
        return unsafe { DefWindowProcW(hwnd, message, wp, lp) };
    };
    if message == WM_NCDESTROY {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if !state.inline_font.is_invalid() {
                let _ = windows::Win32::Graphics::Gdi::DeleteObject(state.inline_font.into());
            }
            drop(Box::from_raw(std::ptr::from_mut(state)));
        }
        PINS.with(|list| list.borrow_mut().retain(|item| *item != hwnd));
        PICTURES.with(|list| list.borrow_mut().retain(|item| *item != hwnd));
        return unsafe { DefWindowProcW(hwnd, message, wp, lp) };
    }
    if state.inline_message(hwnd, message, wp) {
        return LRESULT(0);
    }
    match message {
        windows::Win32::UI::WindowsAndMessaging::WM_SHOWWINDOW => {
            if let Some(p) = &mut state.animation {
                p.set_visible(wp.0 != 0);
            }
            unsafe {
                if wp.0 != 0 && state.animation.as_ref().is_some_and(|p| p.playing) {
                    windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(hwnd), 91, 20, None);
                } else {
                    let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(hwnd), 91);
                }
            }
        }
        windows::Win32::UI::WindowsAndMessaging::WM_TIMER if wp.0 == 91 => {
            if state
                .animation
                .as_mut()
                .is_some_and(crate::platform::gif_player::Player::tick)
            {
                state.cached = None;
                state.display_cache = None;
                invalidate(hwnd);
            }
        }
        WM_CLOSE => {
            let Some(_confirmation) = crate::platform::app_shell::begin_close_confirmation() else {
                return LRESULT(0);
            };
            if state.pinned {
                if let Err(error) = pin_manager::archive(hwnd) {
                    super::error(hwnd, &error);
                    return LRESULT(0);
                }
            } else if state.unsaved() {
                let answer = unsafe {
                    crate::platform::app_dialog::message(
                        Some(hwnd),
                        w!(
                            "是否保存当前图片后关闭？未保存的修改将被放弃，已贴出的图片仍保留编辑。"
                        ),
                        w!("关闭图片"),
                        MB_YESNOCANCEL | MB_ICONQUESTION,
                    )
                };
                if answer != IDYES && answer != windows::Win32::UI::WindowsAndMessaging::IDNO {
                    return LRESULT(0);
                }
                if answer == IDYES
                    && let Err(error) = state.command(hwnd, 44)
                {
                    super::error(hwnd, &error);
                    return LRESULT(0);
                }
            }
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
        crate::platform::desktop_ui::COLOR_PREVIEW => {
            state.set_color_preview(hwnd, (wp.0 as u32).to_le_bytes());
        }
        crate::platform::desktop_ui::COLOR_CLOSED => {
            state.set_color_preview(hwnd, (wp.0 as u32).to_le_bytes());
            let _ = state.commit_properties(hwnd);
            state.desktop_layout(hwnd);
        }
        WM_COMMAND => {
            let id = wp.0 & 0xffff;
            if state.syncing_color_fields && id == 54 {
                return LRESULT(0);
            }
            let notification = (wp.0 >> 16) & 0xffff;
            if (2100..=2104).contains(&id) || matches!(id, 2110 | 2111) {
                if !state.syncing_text_fields
                    && (notification == CBN_SELCHANGE as usize
                        || notification
                            == windows::Win32::UI::WindowsAndMessaging::CBN_EDITCHANGE as usize
                        || notification
                            == windows::Win32::UI::WindowsAndMessaging::CBN_KILLFOCUS as usize
                        || (matches!(id, 2101..=2104 | 2110 | 2111) && notification == 0))
                {
                    let _ = state.command(hwnd, id);
                }
                return LRESULT(0);
            }
            if matches!(id, 80 | 81)
                && !state.syncing_crop_fields
                && notification == windows::Win32::UI::WindowsAndMessaging::EN_CHANGE as usize
            {
                if let Some(mut crop) = state.crop {
                    if let Some(field) = state.children.iter().find(|c| c.id == id) {
                        if let Ok(value) =
                            crate::platform::workspace_ui::value(field.handle).parse::<u32>()
                        {
                            let mut w = crop.rect[2] - crop.rect[0];
                            let mut h = crop.rect[3] - crop.rect[1];
                            if id == 80 {
                                w = value as f64;
                                if let Some(r) = crop.ratio {
                                    h = w / r;
                                }
                            } else {
                                h = value as f64;
                                if let Some(r) = crop.ratio {
                                    w = h * r;
                                }
                            }
                            if w >= 1.
                                && h >= 1.
                                && w <= crop.size.0 as f64
                                && h <= crop.size.1 as f64
                            {
                                let x = crop.rect[0].min(crop.size.0 as f64 - w);
                                let y = crop.rect[1].min(crop.size.1 as f64 - h);
                                crop.rect = [x, y, x + w, y + h];
                                state.crop = Some(crop);
                                if crop.ratio.is_some() {
                                    state.syncing_crop_fields = true;
                                    if let Some(other) = state
                                        .children
                                        .iter()
                                        .find(|c| c.id == if id == 80 { 81 } else { 80 })
                                    {
                                        crate::platform::workspace_ui::text(
                                            other.handle,
                                            &format!("{:.0}", if id == 80 { h } else { w }),
                                        );
                                    }
                                    state.syncing_crop_fields = false;
                                }
                                invalidate(hwnd);
                            }
                        }
                    }
                }
            }
            if matches!(id, 80 | 81)
                && notification == windows::Win32::UI::WindowsAndMessaging::EN_KILLFOCUS as usize
            {
                let _ = state.command(hwnd, 82);
                invalidate(hwnd);
            }
            if id == 51 && notification == CBN_SELCHANGE as usize {
                if let Some(c) = state.children.iter().find(|c| c.id == 51) {
                    let value = crate::platform::font_catalog::combo_value(c.handle);
                    crate::platform::workspace_ui::text(c.handle, &value);
                }
            }
            if matches!(id, 50 | 51 | 54)
                && notification == windows::Win32::UI::WindowsAndMessaging::EN_CHANGE as usize
            {
                let _ = state.command(hwnd, id);
                invalidate(hwnd);
            }
            if matches!(id, 20 | picture_text::INPUT)
                && notification == windows::Win32::UI::WindowsAndMessaging::EN_CHANGE as usize
            {
                state.sync_inline_text(hwnd, id == picture_text::INPUT);
            }
            if ((matches!(id, 50 | 51 | 52 | 54) && notification == CBN_SELCHANGE as usize)
                || (matches!(id, 50 | 51 | 54)
                    && notification
                        == windows::Win32::UI::WindowsAndMessaging::CBN_EDITCHANGE as usize)
                || (!matches!(id, 20 | picture_text::INPUT | 50 | 51 | 52 | 80 | 81)
                    && notification == 0))
                && let Err(error) = state.command(hwnd, id)
            {
                super::error(hwnd, &error);
            }
        }
        WM_DRAWITEM => {
            let item = unsafe { &*(lp.0 as *const DRAWITEMSTRUCT) };
            let theme = editor_ui::EditorTheme::current(state.dpi);
            let selected = tool_for(item.CtlID as usize) == Some(state.tool);
            let bg = if selected || item.itemState.0 & ODS_SELECTED.0 != 0 {
                theme.palette.divider
            } else {
                theme.palette.panel
            };
            editor_ui::rounded_surface(item.hDC, item.rcItem, bg, theme.palette.divider, 5);
            let label = BUTTONS
                .iter()
                .find(|(id, _, _)| *id == item.CtlID as usize)
                .map_or("", |(_, label, _)| *label);
            let mut rect = item.rcItem;
            rect.left += 5;
            rect.top += 2;
            super::text_dpi(item.hDC, rect, label, theme.palette.text, state.dpi);
            return LRESULT(1);
        }
        WM_KEYDOWN => {
            if wp.0 == 27 && state.inline_text.is_some() {
                let _ = state.finish_inline_text(hwnd, false);
                return LRESULT(0);
            }
            if wp.0 == 27 && !state.pinned {
                state.cancel_gesture(hwnd);
                let _ = state.command(hwnd, if state.crop.is_some() { 62 } else { 30 });
                return LRESULT(0);
            }
            let command = match wp.0 {
                13 if state.crop.is_some() => 61,
                0x20 if state.animation.is_some() && state.pinned => 47,
                0x20 if state.pinned => 21,
                27 if !state.pinned && crate::platform::workspace::active() => 30,
                27 => 23,
                _ => crate::platform::shortcuts::app_action(
                    crate::platform::shortcuts::current_chord(wp.0 as u32),
                )
                .map(|i| match crate::platform::shortcuts::ACTIONS[i].target {
                    crate::platform::shortcuts::Target::Picture(c) => c,
                    crate::platform::shortcuts::Target::Workspace(202) => 10,
                    crate::platform::shortcuts::Target::Workspace(207) => 7,
                    crate::platform::shortcuts::Target::Workspace(208) => 8,
                    crate::platform::shortcuts::Target::Workspace(209) => 9,
                    crate::platform::shortcuts::Target::Workspace(240) => 37,
                    _ => 0,
                })
                .unwrap_or(0),
            };
            if command != 0
                && let Err(error) = state.command(hwnd, command)
            {
                super::error(hwnd, &error);
            }
        }
        WM_LBUTTONDOWN if !state.pan_drag.is_some_and(|(_, _, middle)| middle) => {
            let had_text = state.inline_text.is_some()
                || state.selected.is_some_and(|i| {
                    state
                        .document
                        .borrow()
                        .state
                        .marks
                        .get(i)
                        .is_some_and(|m| m.tool == Tool::Text)
                });
            if !state.pinned {
                if let Err(e) = state.commit_properties(hwnd) {
                    super::error(hwnd, &e);
                }
                let _ = state.finish_inline_text(hwnd, true);
            }
            unsafe {
                let _ = SetFocus(Some(hwnd));
            }
            let screen = point(lp);
            let area = state.canvas_area(hwnd);
            if had_text
                && !state.pinned
                && screen.x >= area.left
                && screen.x < area.right
                && screen.y >= area.top
                && screen.y < area.bottom
                && state.image_point(hwnd, screen, false).is_none()
            {
                state.selected = None;
                state.select_properties(hwnd);
            }
            if state.pinned {
                if !state.locked {
                    let mut p = POINT::default();
                    let mut r = RECT::default();
                    unsafe {
                        let _ = GetCursorPos(&raw mut p);
                        let _ = GetWindowRect(hwnd, &raw mut r);
                        SetCapture(hwnd);
                    }
                    state.drag = Some((p, r));
                }
            } else if screen.x >= area.left
                && screen.x < area.right
                && screen.y >= area.top
                && screen.y < area.bottom
            {
                if unsafe { GetKeyState(0x20) } < 0 {
                    state.pan_drag = Some((screen, state.viewport.pan, false));
                    unsafe {
                        SetCapture(hwnd);
                    }
                } else if let Some(crop) = state.crop {
                    if let Some(handle) = state.crop_hit(hwnd, screen) {
                        state.crop_drag = Some((handle, screen, crop));
                        unsafe {
                            SetCapture(hwnd);
                        }
                    }
                } else if let Some(p) = state.image_point(hwnd, screen, false) {
                    unsafe {
                        SetCapture(hwnd);
                    }
                    if state.tool == Tool::Text {
                        let hit =
                            state.document.borrow().hit(p).filter(|i| {
                                state.document.borrow().state.marks[*i].tool == Tool::Text
                            });
                        if let Some(index) = hit {
                            state.selected = Some(index);
                            let original = state.document.borrow().state.marks[index].clone();
                            state.object_drag = Some(ObjectDrag {
                                index,
                                start: p,
                                preview: original.clone(),
                                original,
                            });
                            state.select_properties(hwnd);
                        } else {
                            unsafe {
                                let _ = ReleaseCapture();
                            }
                            if had_text {
                                state.selected = None;
                                state.select_properties(hwnd);
                            } else {
                                state.begin_inline_text(hwnd, None, p);
                            }
                        }
                    } else if state.tool == Tool::Select {
                        if let Err(error) = state.commit_properties(hwnd) {
                            super::error(hwnd, &error);
                        }
                        state.selected = state.document.borrow().hit(p);
                        if let Some(index) = state.selected {
                            let original = state.document.borrow().state.marks[index].clone();
                            state.object_drag = Some(ObjectDrag {
                                index,
                                start: p,
                                preview: original.clone(),
                                original,
                            });
                        }
                        state.select_properties(hwnd);
                    } else {
                        let text = if state.tool == Tool::Number {
                            (state
                                .document
                                .borrow()
                                .state
                                .marks
                                .iter()
                                .filter(|m| m.tool == Tool::Number)
                                .count()
                                + 1)
                            .to_string()
                        } else {
                            state.input_text()
                        };
                        let mark = Mark {
                            text_box: None,
                            tool: state.tool,
                            points: vec![p],
                            color: state.color,
                            text,
                            stroke: state.stroke,
                            filled: state.filled,
                            font_size: state.font_size,
                            font: state.font_style.clone(),
                        };
                        if state.tool == Tool::Number {
                            state.document.borrow_mut().commit(mark);
                            state.dirty = true;
                            state.display_cache = None;
                            let _ = notify_document(hwnd, &state.document);
                        } else {
                            state.pending = Some(mark);
                        }
                    }
                }
            }
            invalidate(hwnd);
        }
        WM_MBUTTONDOWN if !state.pinned => {
            let p = point(lp);
            let area = state.canvas_area(hwnd);
            if p.x >= area.left
                && p.x < area.right
                && p.y >= area.top
                && p.y < area.bottom
                && state.inline_move.is_none()
                && state.object_drag.is_none()
                && state.pending.is_none()
                && state.crop_drag.is_none()
            {
                state.pan_drag = Some((p, state.viewport.pan, true));
                unsafe {
                    SetCapture(hwnd);
                }
            }
        }
        WM_MBUTTONUP if state.pan_drag.is_some_and(|(_, _, middle)| middle) => {
            state.pan_drag = None;
            unsafe {
                let _ = ReleaseCapture();
            }
            invalidate(hwnd);
        }
        WM_MOUSEMOVE => {
            let p = point(lp);
            if let Some((start, r)) = state.drag {
                let mut current = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&raw mut current);
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        r.left + current.x - start.x,
                        r.top + current.y - start.y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            } else if let Some((start, pan, _)) = state.pan_drag {
                state.viewport.pan = (
                    pan.0 + (p.x - start.x) as f64,
                    pan.1 + (p.y - start.y) as f64,
                );
                state.layout_inline_text(hwnd);
                state.display_cache = None;
                invalidate(hwnd);
            } else if let Some((handle, start, crop)) = state.crop_drag {
                let v = state.view(hwnd);
                let dx =
                    (p.x - start.x) as f64 * crop.size.0 as f64 / (v.right - v.left).max(1) as f64;
                let dy =
                    (p.y - start.y) as f64 * crop.size.1 as f64 / (v.bottom - v.top).max(1) as f64;
                state.crop = Some(crop.dragged(handle, dx, dy));
                state.crop_fields();
                invalidate(hwnd);
            } else if !state.pinned
                && (state.object_drag.is_some() || state.pending.is_some())
                && let Some(p) = state.image_point(hwnd, p, true)
            {
                if let Some(drag) = &mut state.object_drag {
                    let d = state.document.borrow();
                    drag.preview = drag.original.translated(
                        p.x - drag.start.x,
                        p.y - drag.start.y,
                        (d.original.width, d.original.height),
                    );
                    if unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0
                        && drag.original.points.len() == 2
                        && !matches!(drag.original.tool, Tool::Pen | Tool::Text | Tool::Number)
                    {
                        drag.preview = drag.original.clone();
                        drag.preview.points[1] = p;
                    }
                } else if let Some(mark) = &mut state.pending {
                    if mark.tool != Tool::Pen {
                        mark.points.truncate(1);
                    }
                    if mark.points.len() < 100_000
                        && mark
                            .points
                            .last()
                            .is_none_or(|last| last.x != p.x || last.y != p.y)
                    {
                        mark.points.push(p);
                    }
                }
                invalidate(hwnd);
            }
        }
        WM_LBUTTONUP if !state.pan_drag.is_some_and(|(_, _, middle)| middle) => {
            let moved = state.drag.take().is_some();
            let mut edited = false;
            state.pan_drag = None;
            state.crop_drag = None;
            let mut edit_text = None;
            if let Some(drag) = state.object_drag.take() {
                if state.tool == Tool::Text && drag.original.same(&drag.preview) {
                    edit_text = Some((drag.index, drag.original.points[0]));
                }
                edited = state
                    .document
                    .borrow_mut()
                    .replace_mark(drag.index, drag.preview);
                state.display_cache = None;
            }
            if let Some(mark) = state.pending.take() {
                if mark.tool == Tool::Pen || mark.points.len() > 1 {
                    state.document.borrow_mut().commit(mark);
                    edited = true;
                }
            }
            // Clear the gesture before ReleaseCapture synchronously sends WM_CAPTURECHANGED.
            unsafe {
                let _ = ReleaseCapture();
            }
            if let Some((index, point)) = edit_text {
                state.begin_inline_text(hwnd, Some(index), point);
            }
            if moved && let Err(error) = pin_manager::save_session() {
                super::error(hwnd, &error);
            }
            if edited {
                state.dirty = true;
                state.cached = None;
                state.display_cache = None;
                if let Err(error) = notify_document(hwnd, &state.document) {
                    super::error(hwnd, &error);
                }
            }
            invalidate(hwnd);
        }
        WM_CANCELMODE | WM_CAPTURECHANGED => {
            let release_pan = message == WM_CANCELMODE && state.pan_drag.is_some();
            state.object_drag = None;
            state.pending = None;
            state.pan_drag = None;
            state.drag = None;
            if release_pan {
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            if let Some((_, _, original)) = state.crop_drag.take() {
                state.crop = Some(original);
                state.crop_fields();
            }
            state.display_cache = None;
            invalidate(hwnd);
        }
        WM_MOUSEWHEEL if !state.pinned => {
            let delta = ((wp.0 >> 16) as u16).cast_signed();
            let mut anchor = point(lp);
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &raw mut anchor);
            }
            let area = state.canvas_area(hwnd);
            if anchor.x >= area.left
                && anchor.x < area.right
                && anchor.y >= area.top
                && anchor.y < area.bottom
            {
                state.viewport.zoom_at(
                    area,
                    state.document.borrow().size(),
                    anchor,
                    1.2_f64.powf(delta as f64 / 120.),
                );
                state.display_cache = None;
                state.layout_inline_text(hwnd);
                invalidate(hwnd);
            }
        }
        windows::Win32::UI::WindowsAndMessaging::WM_SETCURSOR if !state.pinned => {
            use windows::Win32::UI::WindowsAndMessaging::{
                IDC_HAND, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE,
                SetCursor,
            };
            let mut p = POINT::default();
            unsafe {
                let _ = GetCursorPos(&raw mut p);
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &raw mut p);
            }
            let cursor = if state.pan_drag.is_some() || unsafe { GetKeyState(0x20) } < 0 {
                Some(IDC_HAND)
            } else {
                state.crop_hit(hwnd, p).map(|h| match h {
                    0 | 4 => IDC_SIZENWSE,
                    2 | 6 => IDC_SIZENESW,
                    1 | 5 => IDC_SIZENS,
                    3 | 7 => IDC_SIZEWE,
                    _ => IDC_SIZEALL,
                })
            };
            if let Some(cursor) = cursor {
                unsafe {
                    SetCursor(LoadCursorW(None, cursor).ok());
                }
                return LRESULT(1);
            }
            return unsafe { DefWindowProcW(hwnd, message, wp, lp) };
        }
        WM_LBUTTONDBLCLK if !state.pinned => {
            if let Some(p) = state.image_point(hwnd, point(lp), false) {
                let hit = state
                    .document
                    .borrow()
                    .hit(p)
                    .filter(|i| state.document.borrow().state.marks[*i].tool == Tool::Text);
                if let Some(index) = hit {
                    let at = state.document.borrow().state.marks[index].points[0];
                    state.begin_inline_text(hwnd, Some(index), at);
                }
            }
        }
        WM_LBUTTONDBLCLK if state.pinned => {
            let _ = state.command(hwnd, 26);
        }
        WM_MOUSEWHEEL if state.pinned && !state.locked => {
            let delta = ((wp.0 >> 16) as u16).cast_signed();
            if unsafe { GetKeyState(i32::from(VK_CONTROL.0)) } < 0 {
                state.opacity = (i32::from(state.opacity) + if delta > 0 { 15 } else { -15 })
                    .clamp(45, 255) as u8;
                state.apply_pin_style(hwnd);
            } else {
                let mut rect = RECT::default();
                unsafe {
                    let _ = GetWindowRect(hwnd, &raw mut rect);
                }
                let width = (f64::from(rect.right - rect.left)
                    * if delta > 0 { 1.1 } else { 1.0 / 1.1 })
                .clamp(40.0, 4096.0) as i32;
                let size = state.document.borrow().size();
                let height = (f64::from(width) * f64::from(size.1) / f64::from(size.0))
                    .clamp(24.0, 4096.0) as i32;
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        0,
                        0,
                        width,
                        height,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
            if let Err(error) = pin_manager::save_session() {
                super::error(hwnd, &error);
            }
        }
        WM_RBUTTONUP if state.pinned => {
            use crate::platform::app_menu::{self, Row};
            use editor_ui::IconKind as I;
            let mut rows = vec![
                Row::command(9, "复制原尺寸", I::Copy, true),
                Row::command(27, "按显示尺寸复制", I::Copy, true),
                Row::command(10, "另存为…", I::Save, true),
                Row::command(
                    21,
                    if state.animation.is_some() {
                        "提取当前帧编辑"
                    } else {
                        "编辑此贴图"
                    },
                    I::Edit,
                    true,
                ),
                Row::command(46, "识别文字 / 多模态识图", I::Text, true),
                Row::command(48, "设置分组…", I::Folder, true),
                Row::command(24, "锁定", I::Lock, true).checked(state.locked),
                Row::command(25, "鼠标穿透（从托盘恢复交互）", I::Cursor, true)
                    .checked(state.passthrough),
                Row::command(26, "缩略显示", I::Fit, true).checked(state.collapsed),
                Row::command(40, "顺时针旋转", I::Reset, true),
                Row::command(41, "左右翻转", I::ArrowRight, true),
                Row::command(42, "上下翻转", I::ArrowDown, true),
                Row::command(22, "隐藏贴图", I::Minimize, true),
                Row::command(23, "关闭并保留历史", I::Close, true),
            ];
            if let Some(player) = &state.animation {
                rows.push(Row::command(
                    47,
                    if player.playing {
                        "暂停 GIF"
                    } else {
                        "播放 GIF"
                    },
                    if player.playing { I::Pause } else { I::Play },
                    true,
                ));
            }
            let p = point(lp);
            let anchor = RECT {
                left: p.x,
                top: p.y,
                right: p.x,
                bottom: p.y,
            };
            match app_menu::show_rows(
                hwnd,
                anchor,
                editor_ui::EditorTheme::current(state.dpi),
                rows,
                false,
            ) {
                Ok(Some(id)) => {
                    if let Err(error) = state.command(hwnd, id as usize) {
                        super::error(hwnd, &error);
                    }
                }
                Err(error) => super::error(hwnd, &error.to_string()),
                _ => {}
            }
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(hwnd, &raw mut paint) };
            state.buffered_paint(hwnd, dc);
            unsafe {
                let _ = EndPaint(hwnd, &raw const paint);
            }
            if state.inline_text.is_some() {
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::RedrawWindow(
                        Some(state.inline_input),
                        None,
                        None,
                        windows::Win32::Graphics::Gdi::RDW_INVALIDATE
                            | windows::Win32::Graphics::Gdi::RDW_UPDATENOW,
                    );
                }
            }
            return LRESULT(0);
        }
        WM_SIZE => {
            state.relayout(hwnd);
            invalidate(hwnd);
        }
        WM_DPICHANGED => {
            crate::platform::app_icon::apply(hwnd);
            state.dpi = (wp.0 & 0xffff) as u32;
            let rect = unsafe { &*(lp.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
            state.relayout(hwnd);
        }
        WM_EXITSIZEMOVE if state.pinned => {
            if let Err(error) = pin_manager::save_session() {
                super::error(hwnd, &error);
            }
        }
        WM_DISPLAYCHANGE if state.pinned => {
            pin_manager::ensure_visible(hwnd);
        }
        WM_GETMINMAXINFO if !state.pinned => {
            let info = unsafe { &mut *(lp.0 as *mut MINMAXINFO) };
            info.ptMinTrackSize = POINT {
                x: 600 * state.dpi as i32 / 96,
                y: 540 * state.dpi as i32 / 96,
            };
        }
        WM_ERASEBKGND => return LRESULT(1),
        _ => return unsafe { DefWindowProcW(hwnd, message, wp, lp) },
    }
    LRESULT(0)
}

#[cfg(test)]
#[path = "picture_review.rs"]
mod picture_review;
