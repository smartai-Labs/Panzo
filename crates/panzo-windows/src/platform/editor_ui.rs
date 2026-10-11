use super::ui_preferences::{self, ThemeMode};
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem;
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};
use windows::Win32::Foundation::{COLORREF, HWND, RECT, SIZE};
use windows::Win32::Graphics::Dwm::{
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, AlphaBlend, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection, CreateFontW,
    CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_END_ELLIPSIS, DT_LEFT,
    DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, FF_DONTCARE,
    FW_NORMAL, FW_SEMIBOLD, FillRect, GetDeviceCaps, GetTextExtentPoint32W, HGDIOBJ, LOGPIXELSY,
    OUT_DEFAULT_PRECIS, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, GetDpiForWindow,
    SetProcessDpiAwarenessContext,
};
use windows::core::w;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorPalette {
    pub window: COLORREF,
    pub toolbar: COLORREF,
    pub panel: COLORREF,
    pub raised: COLORREF,
    pub hover: COLORREF,
    pub pressed: COLORREF,
    pub border: COLORREF,
    pub divider: COLORREF,
    pub text: COLORREF,
    pub text_secondary: COLORREF,
    pub text_muted: COLORREF,
    pub accent: COLORREF,
    pub accent_hover: COLORREF,
    pub accent_pressed: COLORREF,
    pub danger: COLORREF,
    pub warning: COLORREF,
    pub track: COLORREF,
    pub track_alt: COLORREF,
    pub video_clip: COLORREF,
    pub video_border: COLORREF,
    pub camera_clip: COLORREF,
    pub camera_selected: COLORREF,
    pub camera_border: COLORREF,
    pub slider_thumb: COLORREF,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorMetrics {
    pub outer_margin: i32,
    pub gap: i32,
    pub toolbar_height: i32,
    pub timeline_height: i32,
    pub inspector_width: i32,
    pub button_height: i32,
    pub radius_small: i32,
    pub radius_panel: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorTheme {
    pub palette: EditorPalette,
    pub metrics: EditorMetrics,
    pub dpi: u32,
    pub mode: ThemeMode,
    pub text_scale: u32,
}

impl EditorTheme {
    pub const fn light() -> Self {
        Self {
            palette: EditorPalette {
                window: rgb(255, 255, 255),
                toolbar: rgb(255, 255, 255),
                panel: rgb(255, 255, 255),
                raised: rgb(244, 245, 247),
                hover: rgb(232, 235, 239),
                pressed: rgb(219, 224, 231),
                border: rgb(228, 230, 234),
                divider: rgb(224, 228, 234),
                text: rgb(35, 39, 45),
                text_secondary: rgb(91, 98, 108),
                text_muted: rgb(103, 110, 121),
                accent: rgb(58, 101, 164),
                accent_hover: rgb(49, 89, 147),
                accent_pressed: rgb(39, 73, 123),
                danger: rgb(184, 42, 35),
                warning: rgb(139, 82, 0),
                track: rgb(246, 247, 249),
                track_alt: rgb(246, 247, 249),
                video_clip: rgb(217, 227, 242),
                video_border: rgb(157, 177, 204),
                camera_clip: rgb(228, 220, 204),
                camera_selected: rgb(216, 203, 178),
                camera_border: rgb(151, 131, 93),
                slider_thumb: rgb(246, 247, 249),
            },
            metrics: EditorMetrics {
                outer_margin: 0,
                gap: 6,
                toolbar_height: super::workbench_layout::CONTEXT,
                timeline_height: super::workbench_layout::TIMELINE,
                inspector_width: super::workbench_layout::INSPECTOR,
                button_height: 32,
                radius_small: 7,
                radius_panel: 6,
            },
            dpi: 96,
            mode: ThemeMode::Light,
            text_scale: 100,
        }
    }

    pub fn for_dpi(dpi: u32) -> Self {
        let dpi = dpi.clamp(96, 384);
        let mut theme = Self::light();
        theme.dpi = dpi;
        theme.text_scale = super::system_preferences::text_scale();
        theme.metrics = EditorMetrics {
            outer_margin: scale_dip(theme.metrics.outer_margin, dpi),
            gap: scale_dip(theme.metrics.gap, dpi),
            toolbar_height: scale_dip(theme.metrics.toolbar_height, dpi),
            timeline_height: scale_dip(theme.metrics.timeline_height, dpi),
            inspector_width: scale_dip(theme.metrics.inspector_width, dpi),
            button_height: scale_dip(theme.metrics.button_height, dpi),
            radius_small: scale_dip(theme.metrics.radius_small, dpi),
            radius_panel: scale_dip(theme.metrics.radius_panel, dpi),
        };
        theme
    }

    pub fn current(dpi: u32) -> Self {
        Self::for_mode(dpi, ui_preferences::theme())
    }

    pub fn for_mode(dpi: u32, mode: ThemeMode) -> Self {
        let mut theme = Self::for_dpi(dpi);
        theme.set_mode(mode);
        theme
    }

    pub fn set_mode(&mut self, mode: ThemeMode) {
        self.mode = mode;
        self.palette = if mode == ThemeMode::Light {
            Self::light().palette
        } else {
            EditorPalette {
                window: rgb(48, 50, 53),
                toolbar: rgb(48, 50, 53),
                panel: rgb(48, 50, 53),
                raised: rgb(37, 40, 44),
                hover: rgb(65, 69, 76),
                pressed: rgb(40, 43, 49),
                border: rgb(40, 43, 48),
                divider: rgb(66, 70, 77),
                text: rgb(229, 231, 235),
                text_secondary: rgb(186, 193, 204),
                text_muted: rgb(159, 166, 177),
                accent: rgb(168, 192, 227),
                accent_hover: rgb(190, 207, 234),
                accent_pressed: rgb(149, 177, 216),
                danger: rgb(255, 129, 126),
                warning: rgb(255, 192, 103),
                track: rgb(38, 40, 44),
                track_alt: rgb(38, 40, 44),
                video_clip: rgb(59, 71, 91),
                video_border: rgb(98, 121, 151),
                camera_clip: rgb(91, 84, 71),
                camera_selected: rgb(111, 99, 76),
                camera_border: rgb(170, 151, 112),
                slider_thumb: rgb(217, 223, 232),
            }
        };
        if super::system_preferences::high_contrast() {
            use windows::Win32::Graphics::Gdi::*;
            let bg = COLORREF(unsafe { GetSysColor(COLOR_WINDOW) });
            let fg = COLORREF(unsafe { GetSysColor(COLOR_WINDOWTEXT) });
            let selection = COLORREF(unsafe { GetSysColor(COLOR_HIGHLIGHT) });
            self.palette.window = bg;
            self.palette.toolbar = bg;
            self.palette.panel = bg;
            self.palette.raised = bg;
            self.palette.track = bg;
            self.palette.track_alt = bg;
            self.palette.text = fg;
            self.palette.text_secondary = fg;
            self.palette.text_muted = fg;
            self.palette.danger = fg;
            self.palette.warning = fg;
            self.palette.accent = fg;
            self.palette.border = fg;
            self.palette.divider = bg;
            self.palette.hover = bg;
            self.palette.pressed = selection;
            self.palette.video_clip = bg;
            self.palette.camera_clip = bg;
            self.palette.video_border = fg;
            self.palette.camera_border = fg;
        }
    }

    pub fn scale(self, value: i32) -> i32 {
        scale_dip(value, self.dpi)
    }
}

pub fn scale_dip(value: i32, dpi: u32) -> i32 {
    ((value as i64 * dpi as i64 * i64::from(super::system_preferences::text_scale()) + 4800) / 9600)
        as i32
}

pub fn enable_per_monitor_dpi_awareness() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

pub fn window_dpi(hwnd: HWND) -> u32 {
    unsafe { GetDpiForWindow(hwnd) }.max(96)
}

pub fn system_dpi() -> u32 {
    unsafe { GetDpiForSystem() }.max(96)
}

pub fn available_window_size(width: i32, height: i32) -> (i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut area = RECT::default();
    if unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut area).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok()
    {
        (
            width.min((area.right - area.left - 32).max(320)),
            height.min((area.bottom - area.top - 32).max(240)),
        )
    } else {
        (width, height)
    }
}

pub fn apply_window_chrome(hwnd: HWND) {
    super::app_icon::apply(hwnd);
    let dark_mode = i32::from(ui_preferences::theme() == ThemeMode::Dark);
    let corner_preference = DWMWCP_ROUND;
    let theme = EditorTheme::current(window_dpi(hwnd));
    let caption_color = header_background(theme);
    let text_color = theme.palette.text;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            windows::Win32::Graphics::Dwm::DWMWA_CAPTION_COLOR,
            (&raw const caption_color).cast::<c_void>(),
            mem::size_of_val(&caption_color) as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            windows::Win32::Graphics::Dwm::DWMWA_TEXT_COLOR,
            (&raw const text_color).cast::<c_void>(),
            mem::size_of_val(&text_color) as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const dark_mode).cast::<c_void>(),
            mem::size_of_val(&dark_mode) as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const corner_preference).cast::<c_void>(),
            mem::size_of_val(&corner_preference) as u32,
        );
    }
}

pub fn sync_window_theme(theme: &mut EditorTheme, hwnd: HWND) -> bool {
    let mode = ui_preferences::theme();
    let mut next = *theme;
    next.set_mode(mode);
    let text_scale = super::system_preferences::text_scale();
    if text_scale != next.text_scale {
        let scale = |value: i32| {
            ((i64::from(value) * i64::from(text_scale)) / i64::from(next.text_scale.max(1))) as i32
        };
        let m = &mut next.metrics;
        m.outer_margin = scale(m.outer_margin);
        m.gap = scale(m.gap);
        m.toolbar_height = scale(m.toolbar_height);
        m.timeline_height = scale(m.timeline_height);
        m.inspector_width = scale(m.inspector_width);
        m.button_height = scale(m.button_height);
        m.radius_small = scale(m.radius_small);
        m.radius_panel = scale(m.radius_panel);
        next.text_scale = text_scale;
    }
    if *theme == next {
        return false;
    }
    *theme = next;
    apply_window_chrome(hwnd);
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, false);
    }
    true
}

pub fn theme_command(command: i32) -> Option<Result<(), String>> {
    let mode = match command {
        2501 => ThemeMode::Light,
        2502 => ThemeMode::Dark,
        _ => return None,
    };
    Some(ui_preferences::set_theme(mode))
}

/// Brushes have process lifetime, bounded to two colours per theme. Used by native
/// text fields so editing a dark property does not flash a white rectangle.
pub fn native_control_color(
    message: u32,
    wp: windows::Win32::Foundation::WPARAM,
) -> Option<windows::Win32::Foundation::LRESULT> {
    static BRUSHES: OnceLock<[usize; 4]> = OnceLock::new();
    use windows::Win32::Graphics::Gdi::{HDC, SetBkColor, SetTextColor};
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC,
    };
    if !matches!(
        message,
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX | WM_CTLCOLORBTN
    ) {
        return None;
    }
    let theme = EditorTheme::current(96);
    let field = matches!(message, WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX);
    let color = if field {
        theme.palette.raised
    } else {
        theme.palette.window
    };
    let brushes = BRUSHES.get_or_init(|| {
        let light = EditorTheme::light().palette;
        let dark = EditorTheme::for_mode(96, ThemeMode::Dark).palette;
        [light.window, light.raised, dark.window, dark.raised]
            .map(|c| unsafe { CreateSolidBrush(c).0 as usize })
    });
    unsafe {
        let dc = HDC(wp.0 as *mut c_void);
        SetBkColor(dc, color);
        SetTextColor(dc, theme.palette.text);
    }
    let index = usize::from(theme.mode == ThemeMode::Dark) * 2 + usize::from(field);
    Some(windows::Win32::Foundation::LRESULT(
        brushes[index].cast_signed(),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonRole {
    Standard,
    Primary,
    Destructive,
    Toolbar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlInteraction {
    Idle,
    Hovered,
    Pressed,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconKind {
    /// Approved image-workspace SVG, separate from other workspaces.
    Prototype(&'static str),
    Panel,
    Vision,
    Waveform,
    Music,
    Sound,
    Muted,
    Eye,
    Microphone,
    Subtitles,
    StrokeDot,
    Panzo,
    Cut,
    Paste,
    Copy,
    Sun,
    Moon,
    Capture,
    ScrollCapture,
    Collage,
    Batch,
    Rectangle,
    Text,
    Pen,
    Pin,
    Palette,
    Check,
    OneToOne,
    Lock,
    PreviousFrame,
    NextFrame,
    Minimize,
    Maximize,
    WindowRestore,
    Close,
    Menu,
    Crop,
    Back,
    Monitor,
    Window,
    Split,
    Camera,
    Cursor,
    Home,
    Play,
    Pause,
    Undo,
    Redo,
    Add,
    Delete,
    Reset,
    Restore,
    Diagnostics,
    Settings,
    ZoomOut,
    ZoomIn,
    Fit,
    Save,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    ChevronDown,
    ChevronUp,
    Search,
    More,
    ChevronRight,
    Record,
    RecordOutline,
    Stop,
    Export,
    Edit,
    Folder,
    Log,
    Image,
    Background,
    Inset,
    Radius,
    Shadow,
    TrimStartEarlier,
    TrimStartLater,
    TrimEndEarlier,
    TrimEndLater,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonVisualState {
    pub interaction: ControlInteraction,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSeverity {
    Information,
    Success,
    Warning,
    Error,
}

impl StatusSeverity {
    pub const fn color(self, theme: EditorTheme) -> COLORREF {
        match self {
            Self::Error => theme.palette.danger,
            Self::Warning => theme.palette.warning,
            Self::Information => theme.palette.text_secondary,
            Self::Success => theme.palette.accent,
        }
    }
}

/// Presentation data for the existing status area; never infer severity from translated text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusMessage {
    pub severity: StatusSeverity,
    pub summary: String,
    pub detail: Option<String>,
}

/// Unresolved operation results take precedence over unrelated transient messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StatusScope {
    Save,
    RecoveryDraft,
    Export,
    Background,
    Preview,
}

#[derive(Default)]
pub(crate) struct StatusFeedback {
    problems: Vec<(StatusScope, StatusMessage)>,
    diagnostics: std::collections::VecDeque<String>,
}

impl StatusFeedback {
    pub(crate) fn remember(&mut self, summary: &str, detail: &str) {
        let entry: String = format!("{summary}\n{detail}").chars().take(4096).collect();
        if self.diagnostics.back() == Some(&entry) {
            return;
        }
        if self.diagnostics.len() == 16 {
            self.diagnostics.pop_front();
        }
        self.diagnostics.push_back(entry);
    }

    pub(crate) fn report(&mut self, scope: StatusScope, message: StatusMessage) {
        if self
            .problems
            .iter()
            .any(|(key, old)| *key == scope && *old == message)
        {
            return;
        }
        self.remember(
            &message.summary,
            message.detail.as_deref().unwrap_or_default(),
        );
        self.resolve(scope);
        self.problems.push((scope, message));
        self.problems.sort_by_key(|(key, _)| *key);
    }

    pub(crate) fn resolve(&mut self, scope: StatusScope) {
        self.problems.retain(|(key, _)| *key != scope);
    }

    pub(crate) fn current(&self) -> Option<&StatusMessage> {
        self.problems.first().map(|(_, message)| message)
    }

    #[cfg(test)]
    pub(crate) fn details(&self) -> String {
        self.diagnostics
            .iter()
            .rev()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

impl StatusMessage {
    pub fn new(severity: StatusSeverity, summary: impl Into<String>) -> Self {
        Self {
            severity,
            summary: summary.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn color(&self, theme: EditorTheme) -> COLORREF {
        self.severity.color(theme)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderAction {
    Menu,
    Settings,
    NewRecording,
    OpenProject,
    Save,
    Export,
    Minimize,
    Maximize,
    Close,
}

impl HeaderAction {
    pub const ALL: [Self; 9] = [
        Self::Settings,
        Self::NewRecording,
        Self::OpenProject,
        Self::Save,
        Self::Export,
        Self::Minimize,
        Self::Maximize,
        Self::Close,
        Self::Menu,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Shared main-window caption geometry, independent of either page's state.
#[derive(Clone, Copy)]
pub struct HeaderLayout {
    pub bar: RECT,
    pub title: RECT,
    pub buttons: [RECT; 9],
}

impl HeaderLayout {
    pub fn new(client: RECT, theme: EditorTheme, _caption_bounds: Option<RECT>) -> Self {
        let s = |n| theme.scale(n);
        // Page tools share one client row, independent of the native window frame.
        let height = s(28);
        let top = client.top + (theme.metrics.toolbar_height - height) / 2;
        let button = |left| RECT {
            left,
            right: left + s(28),
            top,
            bottom: top + height,
        };
        let mut buttons = [RECT::default(); 9];
        buttons[HeaderAction::Menu.index()] = button(client.left + s(8));
        Self {
            bar: RECT {
                bottom: client.top + theme.metrics.toolbar_height,
                ..client
            },
            title: RECT::default(),
            buttons,
        }
    }

    pub fn rect(self, action: HeaderAction) -> RECT {
        self.buttons[action.index()]
    }

    pub fn hit_action(self, x: i32, y: i32) -> Option<HeaderAction> {
        HeaderAction::ALL.into_iter().find(|action| {
            let r = self.rect(*action);
            x >= r.left && x < r.right && y >= r.top && y < r.bottom
        })
    }
}

pub fn draw_header(
    dc: windows::Win32::Graphics::Gdi::HDC,
    layout: HeaderLayout,
    theme: EditorTheme,
    states: [ButtonVisualState; 2],
    focused: Option<HeaderAction>,
) {
    // Opaque pixels keep the system's active/inactive backdrop from bleeding
    // through. GDI FillRect alone clears alpha and cannot guarantee this.
    fill_opaque_rect(dc, layout.bar, header_background(theme));
    for (index, action) in [HeaderAction::Menu, HeaderAction::Settings]
        .into_iter()
        .enumerate()
    {
        let icon = if action == HeaderAction::Menu {
            IconKind::Menu
        } else {
            IconKind::Settings
        };
        draw_header_button(
            dc,
            layout.rect(action),
            icon,
            theme,
            states[index],
            focused == Some(action),
        );
    }
}

pub fn header_background(theme: EditorTheme) -> COLORREF {
    theme.palette.toolbar
}

pub fn fill_opaque_rect(dc: windows::Win32::Graphics::Gdi::HDC, rect: RECT, color: COLORREF) {
    let width = (rect.right - rect.left).max(0).cast_unsigned() as usize;
    let height = (rect.bottom - rect.top).max(0).cast_unsigned() as usize;
    let pixel = [
        (color.0 >> 16) as u8,
        (color.0 >> 8) as u8,
        color.0 as u8,
        255,
    ];
    draw_alpha_image(dc, rect, &pixel.repeat(width * height));
}

/// Premultiplied hover/focus shapes preserve the opaque header beneath them.
fn draw_header_button(
    dc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    icon: IconKind,
    theme: EditorTheme,
    state: ButtonVisualState,
    focused: bool,
) {
    type Key = (IconKind, u16, u16, Option<u32>, u32, Option<u32>);
    static CACHE: OnceLock<Mutex<HashMap<Key, Vec<u8>>>> = OnceLock::new();
    let width = u16::try_from(rect.right - rect.left).unwrap_or(0);
    let height = u16::try_from(rect.bottom - rect.top).unwrap_or(0);
    if width == 0 || height == 0 {
        return;
    }
    let (fill, _, ink) = button_colors(theme, ButtonRole::Toolbar, state);
    let fill = matches!(
        state.interaction,
        ControlInteraction::Hovered | ControlInteraction::Pressed
    )
    .then_some(fill.0);
    let focus = focused.then_some(theme.palette.accent_hover.0);
    let key = (icon, width, height, fill, ink.0, focus);
    let Ok(mut cache) = CACHE.get_or_init(|| Mutex::new(HashMap::new())).lock() else {
        return;
    };
    let pixels = cache.entry(key).or_insert_with(|| {
        let color = |value: u32| format!("#{:02x}{:02x}{:02x}", value & 255, (value >> 8) & 255, (value >> 16) & 255);
        let background = fill.map_or_else(|| "none".into(), color);
        let focus = focus.map_or_else(String::new, |value| format!(r#"<rect x="2" y="2" width="24" height="24" rx="4" fill="none" stroke="{}" stroke-width="1.25"/>"#, color(value)));
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 28 28"><rect width="28" height="28" rx="6" fill="{background}"/><g transform="translate(5 5) scale(0.75)" fill="none" stroke="{}" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">{}</g>{focus}</svg>"#, color(ink.0), icon_svg_body(icon));
        let Ok(tree) = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()) else { return Vec::new(); };
        let Some(mut pixmap) = tiny_skia::Pixmap::new(u32::from(width), u32::from(height)) else { return Vec::new(); };
        resvg::render(&tree, tiny_skia::Transform::from_scale(f32::from(width) / 28.0, f32::from(height) / 28.0), &mut pixmap.as_mut());
        let mut pixels = pixmap.take();
        for pixel in pixels.chunks_exact_mut(4) { pixel.swap(0, 2); }
        pixels
    });
    draw_alpha_image(dc, rect, pixels);
}

impl ButtonVisualState {
    pub const fn enabled() -> Self {
        Self {
            interaction: ControlInteraction::Idle,
            selected: false,
        }
    }
}

/// Resolves the transient pointer state for a button. A press is only visible while the pointer
/// remains over the same captured control; dragging away returns it to idle and releasing over a
/// different control must not activate either button.
pub const fn button_interaction(
    enabled: bool,
    pointer_down: bool,
    pointer_over: bool,
) -> ControlInteraction {
    if !enabled {
        ControlInteraction::Disabled
    } else if pointer_down && pointer_over {
        ControlInteraction::Pressed
    } else if pointer_over {
        ControlInteraction::Hovered
    } else {
        ControlInteraction::Idle
    }
}

pub fn complete_pointer_activation<T: Copy + Eq>(
    pressed: &mut Option<T>,
    released_over: Option<T>,
) -> Option<T> {
    pressed
        .take()
        .filter(|pressed_control| Some(*pressed_control) == released_over)
}

pub fn fill_rect(hdc: windows::Win32::Graphics::Gdi::HDC, rect: &RECT, color: COLORREF) {
    unsafe {
        let previous = windows::Win32::Graphics::Gdi::SetDCBrushColor(hdc, color);
        let brush = windows::Win32::Graphics::Gdi::HBRUSH(
            windows::Win32::Graphics::Gdi::GetStockObject(windows::Win32::Graphics::Gdi::DC_BRUSH)
                .0,
        );
        FillRect(hdc, rect, brush);
        windows::Win32::Graphics::Gdi::SetDCBrushColor(hdc, previous);
    }
}

pub fn rounded_surface(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    fill: COLORREF,
    stroke: COLORREF,
    radius: i32,
) {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let radius = radius.clamp(
        0,
        ((rect.right - rect.left).min(rect.bottom - rect.top)) / 2,
    );
    if radius == 0 {
        fill_rect(hdc, &rect, fill);
        return;
    }
    // Nine-slice rendering: only the small corners carry alpha. Resizing a large
    // preview well never rasterizes or allocates a full-window bitmap.
    fill_rect(
        hdc,
        &RECT {
            left: rect.left + radius,
            right: rect.right - radius,
            ..rect
        },
        fill,
    );
    fill_rect(
        hdc,
        &RECT {
            top: rect.top + radius,
            bottom: rect.bottom - radius,
            ..rect
        },
        fill,
    );
    if fill != stroke {
        for edge in [
            RECT {
                left: rect.left + radius,
                right: rect.right - radius,
                bottom: rect.top + 1,
                ..rect
            },
            RECT {
                left: rect.left + radius,
                right: rect.right - radius,
                top: rect.bottom - 1,
                ..rect
            },
            RECT {
                top: rect.top + radius,
                bottom: rect.bottom - radius,
                right: rect.left + 1,
                ..rect
            },
            RECT {
                top: rect.top + radius,
                bottom: rect.bottom - radius,
                left: rect.right - 1,
                ..rect
            },
        ] {
            fill_rect(hdc, &edge, stroke);
        }
    }
    let pixels = rounded_corner_pixels(radius, fill, stroke);
    with_alpha_pixels(hdc, radius * 2, radius * 2, &pixels, |source_dc| {
        for (x, y, source_x, source_y) in [
            (rect.left, rect.top, 0, 0),
            (rect.right - radius, rect.top, radius, 0),
            (rect.left, rect.bottom - radius, 0, radius),
            (rect.right - radius, rect.bottom - radius, radius, radius),
        ] {
            unsafe {
                let _ = AlphaBlend(
                    hdc,
                    x,
                    y,
                    radius,
                    radius,
                    source_dc,
                    source_x,
                    source_y,
                    radius,
                    radius,
                    alpha_blend_function(),
                );
            }
        }
    });
}

#[allow(clippy::cast_sign_loss)] // Coverage and color channels are clamped to nonnegative byte ranges.
fn rounded_corner_pixels(radius: i32, fill: COLORREF, stroke: COLORREF) -> Arc<Vec<u8>> {
    type CornerCache = HashMap<(i32, u32, u32), Arc<Vec<u8>>>;
    static CACHE: OnceLock<Mutex<CornerCache>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = (radius, fill.0, stroke.0);
    if let Some(pixels) = cache.get(&key) {
        return Arc::clone(pixels);
    }
    let side = radius * 2;
    let mut pixels = Vec::with_capacity(usize::try_from(side * side * 4).unwrap_or(0));
    for y in 0..side {
        for x in 0..side {
            let dx = (f64::from(x) + 0.5 - f64::from(radius)).abs();
            let dy = (f64::from(y) + 0.5 - f64::from(radius)).abs();
            let distance = dx.hypot(dy) - f64::from(radius);
            let outer = (0.5 - distance).clamp(0.0, 1.0);
            let inner = (-0.5 - distance).clamp(0.0, 1.0);
            for shift in [16, 8, 0] {
                let a = f64::from((fill.0 >> shift) & 255);
                let b = f64::from((stroke.0 >> shift) & 255);
                pixels.push((a * inner + b * (outer - inner)).round() as u8);
            }
            pixels.push((outer * 255.0).round() as u8);
        }
    }
    let pixels = Arc::new(pixels);
    if cache.len() >= 128 {
        cache.clear();
    }
    cache.insert(key, Arc::clone(&pixels));
    pixels
}

/// Keyboard focus uses an inset outline, never an underline beneath a button.
pub fn focus_ring(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    color: COLORREF,
    _radius: i32,
) {
    let inset = 2;
    let rect = RECT { left: rect.left + inset, top: rect.top + inset,
        right: rect.right - inset, bottom: rect.bottom - inset };
    if rect.right > rect.left && rect.bottom > rect.top {
        selection_outline(hdc, rect, color);
    }
}

/// Discrete selected objects and keyboard focus use a one-pixel outline.
pub fn selection_outline(hdc: windows::Win32::Graphics::Gdi::HDC, r: RECT, color: COLORREF) {
    for edge in [
        RECT {
            bottom: r.top + 1,
            ..r
        },
        RECT {
            top: r.bottom - 1,
            ..r
        },
        RECT {
            right: r.left + 1,
            ..r
        },
        RECT {
            left: r.right - 1,
            ..r
        },
    ] {
        fill_rect(hdc, &edge, color);
    }
}

pub fn draw_icon(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    icon: IconKind,
    color: COLORREF,
) {
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let size = width.min(height).max(1);
    let left = i32::midpoint(rect.left, rect.right) - size / 2;
    let top = i32::midpoint(rect.top, rect.bottom) - size / 2;
    let Some(pixels) = icon_pixels(icon, u16::try_from(size).unwrap_or(u16::MAX), color) else {
        return;
    };
    draw_icon_pixels(hdc, left, top, size, &pixels);
}

fn icon_pixels(icon: IconKind, size: u16, color: COLORREF) -> Option<Vec<u8>> {
    type IconCache = HashMap<(IconKind, u16, u32), Vec<u8>>;
    static CACHE: OnceLock<Mutex<IconCache>> = OnceLock::new();
    let key = (icon, size, color.0);
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()?;
    if let Some(pixels) = cache.get(&key) {
        return Some(pixels.clone());
    }

    let source = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
        if matches!(icon, IconKind::Prototype(_)) {
            "1.55"
        } else {
            "1.65"
        },
        icon_svg_body(icon)
    );
    let tree = resvg::usvg::Tree::from_str(&source, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(u32::from(size), u32::from(size))?;
    let scale = f32::from(size) / 24.0;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    let red = (color.0 & 0xff) as u8;
    let green = ((color.0 >> 8) & 0xff) as u8;
    let blue = ((color.0 >> 16) & 0xff) as u8;
    let mut pixels = Vec::with_capacity(usize::from(size) * usize::from(size) * 4);
    for rgba in pixmap.data().chunks_exact(4) {
        let alpha = rgba[3];
        let premultiply = |channel: u8| {
            u8::try_from((u16::from(channel) * u16::from(alpha) + 127) / 255).unwrap_or(255)
        };
        pixels.extend_from_slice(&[
            premultiply(blue),
            premultiply(green),
            premultiply(red),
            alpha,
        ]);
    }
    cache.insert(key, pixels.clone());
    Some(pixels)
}

fn draw_icon_pixels(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    left: i32,
    top: i32,
    size: i32,
    pixels: &[u8],
) {
    with_alpha_pixels(hdc, size, size, pixels, |source_dc| unsafe {
        let _ = AlphaBlend(
            hdc,
            left,
            top,
            size,
            size,
            source_dc,
            0,
            0,
            size,
            size,
            alpha_blend_function(),
        );
    });
}

const fn alpha_blend_function() -> BLENDFUNCTION {
    BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    }
}

pub fn draw_alpha_image(hdc: windows::Win32::Graphics::Gdi::HDC, rect: RECT, pixels: &[u8]) {
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0
        || height <= 0
        || pixels.len()
            != usize::try_from(width).unwrap_or(0) * usize::try_from(height).unwrap_or(0) * 4
    {
        return;
    }
    with_alpha_pixels(hdc, width, height, pixels, |source_dc| unsafe {
        let _ = AlphaBlend(
            hdc,
            rect.left,
            rect.top,
            width,
            height,
            source_dc,
            0,
            0,
            width,
            height,
            alpha_blend_function(),
        );
    });
}

type AlphaEntry = ((i32, i32, u64), Vec<u8>, super::capture_tools::Surface);
thread_local! {
    static ALPHA_BITMAPS: std::cell::RefCell<std::collections::VecDeque<AlphaEntry>> =
        const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}

// GDI thread state can already be gone when Windows runs TLS destructors.
// PaintSession explicitly releases these handles while the message loop is alive.
pub(crate) fn clear_alpha_cache() {
    ALPHA_BITMAPS.with(|cache| cache.borrow_mut().clear());
}

fn with_alpha_pixels(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    width: i32,
    height: i32,
    pixels: &[u8],
    draw: impl FnOnce(windows::Win32::Graphics::Gdi::HDC),
) {
    // Small immutable icon/corner bitmaps are uploaded once, not once per control
    // per frame. The bounded UI-thread cache owns and releases every GDI handle.
    if width <= 128
        && height <= 128
        && width > 0
        && height > 0
        && super::desktop_ui::has_paint_session()
    {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        pixels.hash(&mut hash);
        let key = (width, height, hash.finish());
        ALPHA_BITMAPS.with(|cache| {
            let mut cache = cache.borrow_mut();
            let index = cache
                .iter()
                .position(|(k, data, _)| *k == key && data.as_slice() == pixels);
            let entry = if let Some(index) = index {
                cache.remove(index).unwrap()
            } else {
                let Ok(surface) = super::capture_tools::Surface::new(width as u32, height as u32)
                else {
                    return;
                };
                surface.write_pixels(pixels);
                (key, pixels.to_vec(), surface)
            };
            draw(entry.2.dc);
            cache.push_back(entry);
            if cache.len() > 192 {
                cache.pop_front();
            }
        });
        return;
    }
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = ptr::null_mut::<c_void>();
    let Ok(bitmap) = (unsafe {
        CreateDIBSection(
            Some(hdc),
            &raw const bitmap_info,
            DIB_RGB_COLORS,
            &raw mut bits,
            None,
            0,
        )
    }) else {
        return;
    };
    if bits.is_null() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        return;
    }
    unsafe {
        ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), pixels.len());
    }
    let source_dc = unsafe { CreateCompatibleDC(Some(hdc)) };
    if source_dc.0.is_null() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        return;
    }
    let previous = unsafe { SelectObject(source_dc, HGDIOBJ(bitmap.0)) };
    draw(source_dc);
    unsafe {
        SelectObject(source_dc, previous);
        let _ = DeleteDC(source_dc);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
    }
}

const fn icon_svg_body(icon: IconKind) -> &'static str {
    match icon {
        IconKind::Prototype(svg) => svg,
        IconKind::Panel => "<rect x='3' y='4' width='18' height='16' rx='1'/><path d='M15 4v16'/>",
        IconKind::Vision => {
            "<path d='m12 3 2.4 6.6L21 12l-6.6 2.4L12 21l-2.4-6.6L3 12l6.6-2.4zM20 2v4M18 4h4'/>"
        }
        IconKind::Cut => {
            "<circle cx='6' cy='6' r='3'/><circle cx='6' cy='18' r='3'/><path d='m8 8 12 12M8 16 20 4'/>"
        }
        IconKind::Paste => {
            "<rect x='5' y='5' width='14' height='16' rx='1'/><rect x='9' y='3' width='6' height='4' rx='1'/><path d='M9 12h6M9 16h6'/>"
        }
        IconKind::Copy => {
            "<rect x='8' y='8' width='13' height='13' rx='1'/><path d='M16 8V3H3v13h5'/>"
        }
        IconKind::Sun => {
            "<circle cx='12' cy='12' r='4'/><path d='M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1.5 1.5M17.5 17.5 19 19M5 19l1.5-1.5M17.5 6.5 19 5'/>"
        }
        IconKind::Moon => "<path d='M20 15A9 9 0 0 1 9 4a9 9 0 1 0 11 11z'/>",
        IconKind::Capture => {
            "<path d='M8 3H3v5M16 3h5v5M21 16v5h-5M8 21H3v-5'/><rect x='7' y='7' width='10' height='10' rx='1'/>"
        }
        IconKind::ScrollCapture => {
            "<path d='M7 3H4v18h3M17 3h3v18h-3M12 4v16M9 7l3-3 3 3M9 17l3 3 3-3'/>"
        }
        IconKind::Collage => {
            "<rect x='3' y='3' width='18' height='18' rx='1'/><path d='M11 3v18M11 12h10'/>"
        }
        IconKind::Batch => {
            "<rect x='7' y='7' width='14' height='14' rx='1'/><path d='M17 7V3H3v14h4M11 11h6M11 15h6'/>"
        }
        IconKind::Rectangle => "<rect x='4' y='5' width='16' height='14' rx='1'/>",
        IconKind::Panzo => {
            "<path d='M4 10V7a3 3 0 013-3h11a3 3 0 013 3v7a3 3 0 01-3 3h-6l-5 5V13h11V7H8v3H4Z' fill='currentColor' stroke='none'/><path d='M4 13h3v9H4zM11 8.5l4 2-4 2z' fill='currentColor' stroke='none'/>"
        }
        IconKind::StrokeDot => "<circle cx='12' cy='12' r='4' fill='currentColor'/>",
        IconKind::Microphone => {
            "<rect x='8' y='2' width='8' height='13' rx='4'/><path d='M5 10v2a7 7 0 0 0 14 0v-2M12 19v3M8 22h8'/>"
        }
        IconKind::Subtitles => {
            "<rect x='3' y='4' width='18' height='16' rx='3'/><path d='M6 12h5m3 0h4M6 16h3m3 0h6'/>"
        }
        IconKind::Text => "<path d='M4 6V3h16v3M12 3v18M8 21h8'/>",
        IconKind::Pen => "<path d='m16 3 5 5-12 12-6 1 1-6zM13 6l5 5M4 15l5 5'/>",
        IconKind::Pin => "<path d='m15 3 6 6-3 1-3 4v3l-3-3-6 7 4-9-3-3h3l4-3z'/>",
        IconKind::Palette => {
            "<path d='M12 3a9 9 0 1 0 0 18h1a2 2 0 0 0 1-4 2 2 0 0 1 1-4h3a3 3 0 0 0 3-3c0-4-4-7-9-7z'/><circle cx='7' cy='9' r='1'/><circle cx='11' cy='6' r='1'/><circle cx='16' cy='7' r='1'/><circle cx='6' cy='14' r='1'/>"
        }
        IconKind::Check => "<path d='m4 12 5 5L20 6'/>",
        IconKind::OneToOne => "<path d='m3 8 3-2v12M15 8l3-2v12M11 9v1M11 14v1'/>",
        IconKind::Menu => "<path d='M4 6h16M4 12h16M4 18h16'/>",
        IconKind::Minimize => "<path d='M5 12h14'/>",
        IconKind::Maximize => "<rect x='5' y='5' width='14' height='14'/>",
        IconKind::WindowRestore => {
            "<path d='M9 7V4h11v11h-3'/><rect x='4' y='9' width='11' height='11'/>"
        }
        IconKind::Close => "<path d='m6 6 12 12M6 18 18 6'/>",
        IconKind::Lock => {
            "<rect x='5' y='10' width='14' height='11' rx='2'/><path d='M8 10V7a4 4 0 0 1 8 0v3'/>"
        }
        IconKind::PreviousFrame => "<path d='M6 5v14M18 5l-9 7 9 7z'/>",
        IconKind::NextFrame => "<path d='M18 5v14M6 5l9 7-9 7z'/>",
        IconKind::Crop => "<path d='M6 3v15h15M3 6h15v15'/>",
        IconKind::Settings => {
            "<path d='M9.5 3h5l.5 2.3 1.5.9 2.3-.7 2.5 4.3-1.8 1.6v1.8l1.8 1.6-2.5 4.3-2.3-.7-1.5.9-.5 2.3h-5L9 19.3l-1.5-.9-2.3.7-2.5-4.3 1.8-1.6v-1.8L2.7 9.8l2.5-4.3 2.3.7L9 5.3z'/><circle cx='12' cy='12' r='3' />"
        }
        IconKind::Back => r#"<path d="m14 6-6 6 6 6"/>"#,
        IconKind::Monitor => {
            r#"<rect x="3" y="4" width="18" height="13" rx="2"/><path d="M8 21h8m-4-4v4"/>"#
        }
        IconKind::Window => {
            r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18m-14-3h.01M10 6h.01"/>"#
        }
        IconKind::Split => {
            r#"<circle cx="6" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="m8.5 7.5 12 12m-12-3 12-12M12 12l-2 2"/>"#
        }
        IconKind::Camera => {
            r#"<rect x="3" y="6" width="12" height="12" rx="3"/><path d="m15 10 6-3v10l-6-3"/>"#
        }
        IconKind::Cursor => r#"<path d="m5 3 14 10-7 1-3 7z"/>"#,
        IconKind::Home => r#"<path d="M5 5v14"/><path d="m18 6-9 6 9 6z"/>"#,
        IconKind::Play => r#"<path d="m8 5 11 7-11 7z" fill="white" stroke="none"/>"#,
        IconKind::Pause => r#"<path d="M7 5h3v14H7zM14 5h3v14h-3z" fill="white" stroke="none"/>"#,
        IconKind::Undo => r#"<path d="m9 7-5 5 5 5"/><path d="M5 12h8a6 6 0 0 1 6 6"/>"#,
        IconKind::Redo => r#"<path d="m15 7 5 5-5 5"/><path d="M19 12h-8a6 6 0 0 0-6 6"/>"#,
        IconKind::Add => r#"<path d="M12 5v14M5 12h14"/>"#,
        IconKind::Delete => {
            r#"<path d="M5 7h14M9 7V4h6v3M8 10v7M12 10v7M16 10v7M7 7l1 13h8l1-13"/>"#
        }
        IconKind::Reset => {
            r#"<path d="m4 20 10-10M13 5l1-2 1 2 2 1-2 1-1 2-1-2-2-1zM18 12l.8-1.8.8 1.8 1.8.8-1.8.8-.8 1.8-.8-1.8-1.8-.8zM4 5l.7-1.5L5.5 5 7 5.8l-1.5.7L4.7 8 4 6.5l-1.5-.7z"/>"#
        }
        IconKind::Restore => r#"<path d="M4 10a8 8 0 1 1 1 8M4 4v6h6"/>"#,
        IconKind::Diagnostics => {
            r#"<path d="M4 6h8M16 6h4M4 12h3M11 12h9M4 18h10M18 18h2"/><circle cx="14" cy="6" r="2"/><circle cx="9" cy="12" r="2"/><circle cx="16" cy="18" r="2"/>"#
        }
        IconKind::ZoomOut => {
            r#"<circle cx="10.5" cy="10.5" r="6"/><path d="M15 15l5 5M7.5 10.5h6"/>"#
        }
        IconKind::ZoomIn => {
            r#"<circle cx="10.5" cy="10.5" r="6"/><path d="M15 15l5 5M7.5 10.5h6M10.5 7.5v6"/>"#
        }
        IconKind::Fit => r#"<path d="M9 4H4v5M15 4h5v5M20 15v5h-5M9 20H4v-5"/>"#,
        IconKind::Save => r#"<path d="M5 4h12l2 2v14H5zM8 4v6h8V4M8 16h8"/>"#,
        IconKind::ArrowLeft => r#"<path d="m10 6-6 6 6 6M4 12h16"/>"#,
        IconKind::ArrowRight => r#"<path d="m14 6 6 6-6 6M4 12h16"/>"#,
        IconKind::ArrowUp => r#"<path d="m6 10 6-6 6 6M12 4v16"/>"#,
        IconKind::ArrowDown => r#"<path d="m6 14 6 6 6-6M12 4v16"/>"#,
        IconKind::Search => "<circle cx='10.5' cy='10.5' r='6.5'/><path d='m16 16 5 5'/>",
        IconKind::More => "<circle cx='5' cy='12' r='1'/><circle cx='12' cy='12' r='1'/><circle cx='19' cy='12' r='1'/>",
        IconKind::ChevronUp => "<path d='m6 15 6-6 6 6'/>",
        IconKind::ChevronRight => "<path d='m9 6 6 6-6 6'/>",
        IconKind::ChevronDown => r#"<path d="m6 9 6 6 6-6"/>"#,
        IconKind::Waveform => {
            r#"<path d="M2 9v6M5 6v12M8 3v18M11 7v10M14 5v14M17 8v8M20 4v16M23 9v6"/>"#
        }
        IconKind::Music => {
            r#"<path d="M9 18V5l11-2v13M9 9l11-2"/><ellipse cx="6" cy="18" rx="3" ry="2"/><ellipse cx="17" cy="16" rx="3" ry="2"/>"#
        }
        IconKind::Sound => {
            r#"<path d="M4 9h4l5-4v14l-5-4H4zM16 8a6 6 0 0 1 0 8M19 5a10 10 0 0 1 0 14"/>"#
        }
        IconKind::Muted => r#"<path d="M4 9h4l5-4v14l-5-4H4zM17 9l5 6M22 9l-5 6"/>"#,
        IconKind::Eye => {
            r#"<path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>"#
        }
        IconKind::RecordOutline => r#"<circle cx="12" cy="12" r="7"/>"#,
        IconKind::Record => r#"<circle cx="12" cy="12" r="7" fill="white" stroke="none"/>"#,
        IconKind::Stop => {
            r#"<rect x="6" y="6" width="12" height="12" rx="2" fill="white" stroke="none"/>"#
        }
        IconKind::Export => r#"<path d="M12 15V3M7 8l5-5 5 5M5 14v6h14v-6"/>"#,
        IconKind::Edit => r#"<path d="m14 5 5 5L9 20H4v-5zM12 7l5 5"/>"#,
        IconKind::Folder => r#"<path d="M3 7h7l2 2h9v10H3zM3 7V5h7l2 2"/>"#,
        IconKind::Log => {
            r#"<path d="M8 6h12M8 12h12M8 18h12"/><circle cx="4" cy="6" r="1" fill="white" stroke="none"/><circle cx="4" cy="12" r="1" fill="white" stroke="none"/><circle cx="4" cy="18" r="1" fill="white" stroke="none"/>"#
        }
        IconKind::Image => {
            r#"<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="16.5" cy="8.5" r="1.5"/><path d="m4 17 5-5 4 4 2-2 5 5"/>"#
        }
        IconKind::Background => {
            r#"<rect x="4" y="4" width="16" height="16" rx="3"/><path d="M4 20 20 4"/>"#
        }
        IconKind::Inset => {
            r#"<rect x="3" y="3" width="18" height="18" rx="3"/><rect x="7" y="7" width="10" height="10" rx="1"/>"#
        }
        IconKind::Radius => r#"<path d="M5 20V9a5 5 0 0 1 5-5h9"/><path d="M9 20H5v-4"/>"#,
        IconKind::Shadow => {
            r#"<rect x="4" y="4" width="13" height="13" rx="2"/><path d="M8 20h10a2 2 0 0 0 2-2V8"/>"#
        }
        IconKind::TrimStartEarlier => r#"<path d="M8 4v16M8 12h12M13 7l-5 5 5 5"/>"#,
        IconKind::TrimStartLater => r#"<path d="M8 4v16M8 12h12M15 7l5 5-5 5"/>"#,
        IconKind::TrimEndEarlier => r#"<path d="M16 4v16M4 12h12M9 7l-5 5 5 5"/>"#,
        IconKind::TrimEndLater => r#"<path d="M16 4v16M4 12h12M11 7l5 5-5 5"/>"#,
    }
}

pub fn button_colors(
    theme: EditorTheme,
    role: ButtonRole,
    state: ButtonVisualState,
) -> (COLORREF, COLORREF, COLORREF) {
    let palette = theme.palette;
    if state.interaction == ControlInteraction::Disabled {
        let stroke = if role == ButtonRole::Toolbar {
            palette.panel
        } else {
            palette.divider
        };
        return (palette.panel, stroke, palette.text_muted);
    }
    let base = match role {
        ButtonRole::Primary => palette.accent,
        ButtonRole::Destructive
            if matches!(
                state.interaction,
                ControlInteraction::Hovered | ControlInteraction::Pressed
            ) =>
        {
            palette.danger
        }
        ButtonRole::Toolbar if !state.selected => palette.toolbar,
        ButtonRole::Standard | ButtonRole::Destructive => palette.panel,
        ButtonRole::Toolbar => palette.raised,
    };
    let fill = match state.interaction {
        ControlInteraction::Pressed => match role {
            ButtonRole::Primary => palette.accent_pressed,
            ButtonRole::Destructive => palette.danger,
            ButtonRole::Standard | ButtonRole::Toolbar => palette.pressed,
        },
        ControlInteraction::Hovered => match role {
            ButtonRole::Primary => palette.accent_hover,
            ButtonRole::Destructive => palette.danger,
            ButtonRole::Standard | ButtonRole::Toolbar => palette.hover,
        },
        ControlInteraction::Idle if state.selected && role == ButtonRole::Destructive => {
            palette.danger
        }
        ControlInteraction::Idle if state.selected => palette.panel,
        ControlInteraction::Idle => base,
        ControlInteraction::Disabled => unreachable!(),
    };
    // Flat command buttons use fill changes for hover/selection. Keyboard focus
    // uses a short inner marker without enclosing the control.
    let stroke = if matches!(role, ButtonRole::Primary | ButtonRole::Toolbar) {
        fill
    } else {
        palette.border
    };
    let text = if role == ButtonRole::Primary
        || (role == ButtonRole::Destructive
            && (state.selected
                || matches!(
                    state.interaction,
                    ControlInteraction::Hovered | ControlInteraction::Pressed
                )))
    {
        if theme.mode == ThemeMode::Dark {
            palette.window
        } else {
            rgb(255, 255, 255)
        }
    } else if role == ButtonRole::Toolbar {
        if state.selected {
            palette.accent
        } else {
            palette.text_secondary
        }
    } else {
        palette.text
    };
    (fill, stroke, text)
}

/// A single visual implementation for recorder and editor commands.
#[allow(clippy::too_many_arguments)] // Rendering inputs are explicit and contain no page state.
pub fn draw_button(
    dc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    label: &str,
    icon: IconKind,
    theme: EditorTheme,
    role: ButtonRole,
    state: ButtonVisualState,
    focused: bool,
) {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let (fill, stroke, text) = button_colors(theme, role, state);
    if role != ButtonRole::Toolbar
        || state.selected
        || matches!(
            state.interaction,
            ControlInteraction::Hovered | ControlInteraction::Pressed
        )
    {
        rounded_surface(dc, rect, fill, stroke, theme.metrics.radius_small);
    }
    let (icon_rect, label_rect) = button_content_rects(dc, rect, label, theme);
    draw_icon(dc, icon_rect, icon, text);
    draw_single_line(dc, label_rect, label, text, TextStyle::Button);
    if focused {
        focus_ring(
            dc,
            rect,
            theme.palette.accent_hover,
            theme.metrics.radius_small,
        );
    }
}

pub(crate) fn draw_single_line(
    dc: windows::Win32::Graphics::Gdi::HDC,
    mut rect: RECT,
    value: &str,
    color: COLORREF,
    style: TextStyle,
) {
    if value.is_empty() || rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let old = select_font(dc, style);
    let mut text: Vec<u16> = value.encode_utf16().collect();
    unsafe {
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, color);
        DrawTextW(
            dc,
            &mut text,
            &raw mut rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(dc, old);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextStyle {
    Menu,
    Caption,
    Micro,
    Body,
    Button,
    Heading,
    Title,
    Hero,
    HeroCompact,
}

thread_local! { static DRAWING_DPI: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }

pub struct DrawingScale(u32);
impl DrawingScale {
    pub fn enter(dpi: u32) -> Self {
        Self(DRAWING_DPI.with(|value| value.replace(dpi)))
    }
}
impl Drop for DrawingScale {
    fn drop(&mut self) {
        DRAWING_DPI.with(|value| value.set(self.0));
    }
}

pub fn select_font(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    style: TextStyle,
) -> windows::Win32::Graphics::Gdi::HGDIOBJ {
    let dpi = DRAWING_DPI.with(std::cell::Cell::get);
    let dpi = if dpi == 0 {
        u32::try_from(unsafe { GetDeviceCaps(Some(hdc), LOGPIXELSY) }).unwrap_or(96)
    } else {
        dpi
    };
    unsafe { SelectObject(hdc, font_handle(style, dpi)) }
}

pub fn font_handle(style: TextStyle, dpi: u32) -> HGDIOBJ {
    type FontCache = HashMap<(TextStyle, u32, u32), usize>;
    static FONTS: OnceLock<Mutex<FontCache>> = OnceLock::new();
    let dpi = dpi.clamp(96, 384);
    let (height, weight) = match style {
        TextStyle::Menu => (12, FW_NORMAL.0.cast_signed()),
        TextStyle::Micro => (10, FW_NORMAL.0.cast_signed()),
        TextStyle::Caption => (11, FW_NORMAL.0.cast_signed()),
        TextStyle::Body => (13, FW_NORMAL.0.cast_signed()),
        TextStyle::Button => (13, FW_NORMAL.0.cast_signed()),
        TextStyle::Heading => (14, FW_NORMAL.0.cast_signed()),
        TextStyle::Hero => (36, FW_SEMIBOLD.0.cast_signed()),
        TextStyle::HeroCompact => (30, FW_SEMIBOLD.0.cast_signed()),
        TextStyle::Title => (21, FW_SEMIBOLD.0.cast_signed()),
    };
    let key = (style, dpi, super::system_preferences::text_scale());
    let mut fonts = FONTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let raw = *fonts.entry(key).or_insert_with(|| {
        let font = unsafe {
            CreateFontW(
                -scale_dip(height, dpi),
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                u32::from(DEFAULT_PITCH.0 | FF_DONTCARE.0),
                // Explicitly use a Chinese UI family. Segoe UI's GDI fallback for Chinese
                // can resolve to SimSun, mixing serif labels with sans-serif controls.
                w!("Microsoft YaHei UI"),
            )
        };
        font.0 as usize
    });
    HGDIOBJ(raw as *mut c_void)
}

pub fn select_default_font(
    hdc: windows::Win32::Graphics::Gdi::HDC,
) -> windows::Win32::Graphics::Gdi::HGDIOBJ {
    select_font(hdc, TextStyle::Body)
}

pub fn measure_text_width(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    value: &str,
    style: TextStyle,
) -> i32 {
    let text: Vec<u16> = value.encode_utf16().collect();
    let previous_font = select_font(hdc, style);
    let mut size = SIZE::default();
    unsafe {
        let _ = GetTextExtentPoint32W(hdc, &text, &raw mut size);
        SelectObject(hdc, previous_font);
    }
    size.cx
}

/// Center icon and text together, including truncated labels in narrow controls.
pub fn button_content_rects(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    label: &str,
    theme: EditorTheme,
) -> (RECT, RECT) {
    let icon = theme.scale(18);
    let gap = if label.is_empty() { 0 } else { theme.scale(7) };
    let available = (rect.right - rect.left - theme.scale(20) - icon - gap).max(0);
    let width = measure_text_width(hdc, label, TextStyle::Button).min(available);
    let left = rect.left + ((rect.right - rect.left - icon - gap - width) / 2).max(0);
    let center = i32::midpoint(rect.top, rect.bottom);
    (
        RECT {
            left,
            top: center - icon / 2,
            right: left + icon,
            bottom: center + icon / 2,
        },
        RECT {
            left: left + icon + gap,
            right: left + icon + gap + width,
            ..rect
        },
    )
}

pub const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(red as u32 | (green as u32) << 8 | (blue as u32) << 16)
}

pub fn next_keyboard_focus<T: Copy + PartialEq>(
    order: &[T],
    current: Option<T>,
    reverse: bool,
    enabled: impl Fn(T) -> bool,
) -> Option<T> {
    let enabled_controls: Vec<_> = order
        .iter()
        .copied()
        .filter(|control| enabled(*control))
        .collect();
    if enabled_controls.is_empty() {
        return None;
    }
    let current_index = current.and_then(|focused| {
        enabled_controls
            .iter()
            .position(|control| *control == focused)
    });
    let next_index = if reverse {
        current_index.map_or(enabled_controls.len() - 1, |index| {
            index.checked_sub(1).unwrap_or(enabled_controls.len() - 1)
        })
    } else {
        current_index.map_or(0, |index| (index + 1) % enabled_controls.len())
    };
    Some(enabled_controls[next_index])
}

#[cfg(test)]
pub(crate) fn render_review_png(
    path: &std::path::Path,
    width: i32,
    height: i32,
    draw: impl FnOnce(windows::Win32::Graphics::Gdi::HDC, RECT),
) {
    use windows::Win32::Graphics::Gdi::GdiFlush;
    let dc = unsafe { CreateCompatibleDC(None) };
    assert!(!dc.0.is_null());
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = ptr::null_mut();
    let bitmap = unsafe {
        CreateDIBSection(
            Some(dc),
            &raw const info,
            DIB_RGB_COLORS,
            &raw mut bits,
            None,
            0,
        )
    }
    .unwrap();
    let previous = unsafe { SelectObject(dc, HGDIOBJ(bitmap.0)) };
    draw(
        dc,
        RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        },
    );
    unsafe {
        let _ = GdiFlush();
    }
    let mut rgba = unsafe {
        std::slice::from_raw_parts(
            bits.cast::<u8>(),
            usize::try_from(width * height * 4).unwrap(),
        )
    }
    .to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    unsafe {
        SelectObject(dc, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(dc);
    }
    image::save_buffer(
        path,
        &rgba,
        u32::try_from(width).unwrap(),
        u32::try_from(height).unwrap(),
        image::ColorType::Rgba8,
    )
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_save_survives_other_failures_and_recovery() {
        let mut feedback = StatusFeedback::default();
        let save =
            StatusMessage::new(StatusSeverity::Error, "修改尚未保存").with_detail("disk full");
        feedback.report(StatusScope::Save, save.clone());
        for _ in 0..100 {
            feedback.report(StatusScope::Save, save.clone());
        }
        feedback.report(
            StatusScope::Preview,
            StatusMessage::new(StatusSeverity::Warning, "预览未更新"),
        );
        assert_eq!(feedback.current().unwrap().summary, "修改尚未保存");
        feedback.resolve(StatusScope::Preview);
        assert_eq!(feedback.current().unwrap().summary, "修改尚未保存");
        assert_eq!(feedback.details().matches("disk full").count(), 1);
        feedback.resolve(StatusScope::Save);
        assert!(feedback.current().is_none());
        assert!(feedback.details().contains("disk full"));
    }

    #[test]
    fn diagnostics_are_bounded_and_unicode_safe() {
        let mut feedback = StatusFeedback::default();
        for i in 0..50 {
            feedback.remember(&i.to_string(), &"诊断".repeat(5000));
        }
        assert_eq!(feedback.diagnostics.len(), 16);
        assert!(
            feedback
                .diagnostics
                .iter()
                .all(|entry| entry.chars().count() <= 4096)
        );
    }

    #[test]
    fn page_header_ignores_native_frame_offsets_and_has_no_settings_button() {
        for dpi in [96, 120, 144, 192] {
            let theme = EditorTheme::for_dpi(dpi);
            for width in [1280, 1920] {
                let client = RECT {
                    right: width,
                    bottom: 900,
                    ..RECT::default()
                };
                for top in [-theme.scale(8), 0, theme.scale(7)] {
                    let native = RECT {
                        left: width - theme.scale(138),
                        right: width,
                        top,
                        bottom: top + theme.scale(30),
                    };
                    let layout = HeaderLayout::new(client, theme, Some(native));
                    let menu = layout.rect(HeaderAction::Menu);
                    assert!(
                        (i32::midpoint(menu.top, menu.bottom) - theme.metrics.toolbar_height / 2)
                            .abs()
                            <= 1
                    );
                    assert_eq!(
                        layout.hit_action(
                            i32::midpoint(menu.left, menu.right),
                            i32::midpoint(menu.top, menu.bottom)
                        ),
                        Some(HeaderAction::Menu)
                    );
                    for action in HeaderAction::ALL {
                        if action != HeaderAction::Menu {
                            assert_eq!(layout.rect(action), RECT::default());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn theme_switch_preserves_layout_and_dark_text_contrast() {
        let luminance = |color: COLORREF| {
            [0, 8, 16]
                .map(|shift| {
                    let c = f64::from((color.0 >> shift) & 255) / 255.0;
                    if c <= 0.04045 {
                        c / 12.92
                    } else {
                        ((c + 0.055) / 1.055).powf(2.4)
                    }
                })
                .into_iter()
                .zip([0.2126, 0.7152, 0.0722])
                .map(|(c, weight)| c * weight)
                .sum::<f64>()
        };
        let contrast = |a, b| {
            let (a, b) = (luminance(a), luminance(b));
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        };
        let mut theme = EditorTheme::for_dpi(144);
        theme.metrics.inspector_width = 540;
        theme.metrics.timeline_height = 480;
        let metrics = theme.metrics;
        theme.set_mode(ThemeMode::Dark);
        assert_eq!(theme.metrics, metrics);
        for background in [
            theme.palette.window,
            theme.palette.panel,
            theme.palette.raised,
        ] {
            assert!(contrast(theme.palette.text, background) >= 4.5);
            assert!(contrast(theme.palette.text_secondary, background) >= 4.5);
        }
        for interaction in [
            ControlInteraction::Idle,
            ControlInteraction::Hovered,
            ControlInteraction::Pressed,
        ] {
            let (fill, _, text) = button_colors(
                theme,
                ButtonRole::Primary,
                ButtonVisualState {
                    interaction,
                    selected: false,
                },
            );
            assert!(contrast(text, fill) >= 4.5);
        }
        theme.set_mode(ThemeMode::Light);
        assert_eq!(theme.metrics, metrics);
        assert_eq!(theme.palette, EditorTheme::light().palette);
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let theme = EditorTheme::for_mode(96, mode);
            for fill in [
                theme.palette.window,
                theme.palette.panel,
                theme.palette.raised,
            ] {
                for ink in [
                    theme.palette.text_muted,
                    theme.palette.danger,
                    theme.palette.warning,
                ] {
                    assert!(contrast(ink, fill) >= 4.5, "status contrast: {mode:?}");
                }
            }
        }
    }

    #[test]
    fn curved_controls_have_transparent_and_partial_coverage_at_all_dpi_sizes() {
        for radius in [5, 8, 12, 24] {
            let pixels = rounded_corner_pixels(radius, rgb(255, 255, 255), rgb(180, 190, 200));
            assert_eq!(pixels[3], 0, "square opaque corner at radius {radius}");
            assert!(pixels.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
            assert!(pixels.chunks_exact(4).any(|p| p == [255, 255, 255, 255]));
            assert!(
                pixels
                    .chunks_exact(4)
                    .all(|p| p[..3].iter().all(|c| *c <= p[3])),
                "alpha must be premultiplied"
            );
            assert!(Arc::ptr_eq(
                &pixels,
                &rounded_corner_pixels(radius, rgb(255, 255, 255), rgb(180, 190, 200))
            ));
        }
    }

    #[test]
    fn light_palette_has_distinct_interaction_states() {
        let theme = EditorTheme::light();
        let base = button_colors(theme, ButtonRole::Standard, ButtonVisualState::enabled());
        let hover = button_colors(
            theme,
            ButtonRole::Standard,
            ButtonVisualState {
                interaction: ControlInteraction::Hovered,
                ..ButtonVisualState::enabled()
            },
        );
        let pressed = button_colors(
            theme,
            ButtonRole::Standard,
            ButtonVisualState {
                interaction: ControlInteraction::Pressed,
                ..ButtonVisualState::enabled()
            },
        );
        assert_ne!(base.0, hover.0);
        assert_ne!(hover.0, pressed.0);
    }

    #[test]
    fn disabled_button_uses_muted_text() {
        let theme = EditorTheme::light();
        let colors = button_colors(
            theme,
            ButtonRole::Primary,
            ButtonVisualState {
                interaction: ControlInteraction::Disabled,
                ..ButtonVisualState::enabled()
            },
        );
        assert_eq!(colors.2, theme.palette.text_muted);
    }

    #[test]
    fn pointer_press_is_transient_and_only_visible_over_its_button() {
        assert_eq!(
            button_interaction(true, true, true),
            ControlInteraction::Pressed
        );
        assert_eq!(
            button_interaction(true, true, false),
            ControlInteraction::Idle
        );
        assert_eq!(
            button_interaction(true, false, true),
            ControlInteraction::Hovered
        );
        assert_eq!(
            button_interaction(false, true, true),
            ControlInteraction::Disabled
        );
    }

    #[test]
    fn pointer_activation_requires_release_over_the_original_button() {
        let mut pressed = Some(2_u8);
        assert_eq!(complete_pointer_activation(&mut pressed, Some(2)), Some(2));
        assert_eq!(pressed, None);

        pressed = Some(2);
        assert_eq!(complete_pointer_activation(&mut pressed, Some(3)), None);
        assert_eq!(pressed, None);

        assert_eq!(complete_pointer_activation(&mut pressed, Some(2)), None);
    }

    #[test]
    fn keyboard_focus_skips_disabled_controls_and_wraps_both_directions() {
        let order = [1_u8, 2, 3, 4];
        let enabled = |control| control != 2;
        assert_eq!(next_keyboard_focus(&order, None, false, enabled), Some(1));
        assert_eq!(
            next_keyboard_focus(&order, Some(1), false, enabled),
            Some(3)
        );
        assert_eq!(
            next_keyboard_focus(&order, Some(4), false, enabled),
            Some(1)
        );
        assert_eq!(next_keyboard_focus(&order, None, true, enabled), Some(4));
        assert_eq!(next_keyboard_focus(&order, Some(1), true, enabled), Some(4));
    }

    #[test]
    fn theme_metrics_scale_with_per_monitor_dpi() {
        let theme = EditorTheme::for_dpi(144);
        assert_eq!(theme.metrics.outer_margin, 0);
        assert_eq!(theme.metrics.gap, 9);
        assert_eq!(theme.metrics.button_height, 48);
        assert_eq!(theme.metrics.inspector_width, 456);
    }

    #[test]
    fn every_svg_icon_rasterizes_to_a_non_empty_alpha_mask() {
        let icons = [
            IconKind::Back,
            IconKind::Monitor,
            IconKind::Window,
            IconKind::ChevronDown,
            IconKind::Home,
            IconKind::Play,
            IconKind::Pause,
            IconKind::Undo,
            IconKind::Redo,
            IconKind::Add,
            IconKind::Delete,
            IconKind::Reset,
            IconKind::Diagnostics,
            IconKind::Settings,
            IconKind::ZoomOut,
            IconKind::ZoomIn,
            IconKind::Fit,
            IconKind::Save,
            IconKind::ArrowLeft,
            IconKind::ArrowRight,
            IconKind::ArrowUp,
            IconKind::ArrowDown,
            IconKind::Record,
            IconKind::Stop,
            IconKind::Export,
            IconKind::Edit,
            IconKind::Folder,
            IconKind::Log,
            IconKind::Image,
            IconKind::Background,
            IconKind::Inset,
            IconKind::Radius,
            IconKind::Shadow,
            IconKind::TrimStartEarlier,
            IconKind::TrimStartLater,
            IconKind::TrimEndEarlier,
            IconKind::TrimEndLater,
        ];
        for icon in icons {
            let pixels = icon_pixels(icon, 24, rgb(255, 255, 255)).expect("icon raster");
            assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] > 0), "{icon:?}");
        }
    }
}

/// Shared clip surface for video, camera, audio and subtitles.
pub(crate) fn track_clip(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    color: COLORREF,
    edge: COLORREF,
    selected: bool,
    theme: EditorTheme,
) {
    rounded_surface(
        hdc,
        rect,
        color,
        if selected { edge } else { color },
        theme.scale(4),
    );
}
pub(crate) fn track_grips(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    color: COLORREF,
    selected: bool,
    theme: EditorTheme,
) {
    if !selected { return; }
    let edge=theme.scale(2).max(1).min((rect.right-rect.left).max(1));
    for r in [
        RECT { bottom:rect.top+edge,..rect }, RECT { top:rect.bottom-edge,..rect },
        RECT { right:rect.left+edge,..rect }, RECT { left:rect.right-edge,..rect },
    ] { fill_rect(hdc,&r,theme.palette.accent); }
    if rect.right-rect.left < theme.scale(28) { return; }
    let half = theme
        .scale(6)
        .min((rect.bottom - rect.top - theme.scale(6)).max(0) / 2);
    let y = (rect.top + rect.bottom) / 2;
    if half > 0 {
        for x in [rect.left + theme.scale(4), rect.right - theme.scale(6)] {
            rounded_surface(
                hdc,
                RECT {
                    left: x,
                    right: x + theme.scale(2),
                    top: y - half,
                    bottom: y + half,
                },
                color,
                color,
                theme.scale(1),
            );
        }
    }
}

#[cfg(test)]
mod alpha_cache_lifetime_tests {
    use super::*;

    #[test]
    fn cached_gdi_handles_end_with_last_paint_session() {
        std::thread::spawn(|| {
            let target = super::super::capture_tools::Surface::new(2, 2).unwrap();
            let pixels = [255; 16];
            // Standalone drawing without an owner must not retain GDI handles.
            with_alpha_pixels(target.dc, 2, 2, &pixels, |_| {});
            assert!(ALPHA_BITMAPS.with(|cache| cache.borrow().is_empty()));
            let outer = super::super::desktop_ui::PaintSession::new();
            let inner = super::super::desktop_ui::PaintSession::new();
            with_alpha_pixels(target.dc, 2, 2, &pixels, |_| {});
            assert_eq!(ALPHA_BITMAPS.with(|cache| cache.borrow().len()), 1);
            drop(inner);
            assert_eq!(ALPHA_BITMAPS.with(|cache| cache.borrow().len()), 1);
            drop(outer);
            assert!(ALPHA_BITMAPS.with(|cache| cache.borrow().is_empty()));
            // Reopening a session gets a fresh cache and cleans it again.
            let restarted = super::super::desktop_ui::PaintSession::new();
            with_alpha_pixels(target.dc, 2, 2, &pixels, |_| {});
            drop(restarted);
            assert!(ALPHA_BITMAPS.with(|cache| cache.borrow().is_empty()));
        })
        .join()
        .unwrap();
    }
}
