//! Approved prototype chrome. Layout, drawing and interaction share `ControlLayout`.
use super::editor_widgets::{SWATCHES, swatch_color, value_rect};
use super::{
    COLORREF, ControlId, ControlLayout, DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER, EditorTheme,
    IconKind, RECT, TICKS_PER_SECOND, TextStyle, WindowState, button, draw_control_tooltip,
    draw_icon, draw_text_styled, draw_timeline_ruler, fill, focus_ring, format_tick, rgb,
    rounded_surface, segment_rect, timeline_x,
};
type DC = windows::Win32::Graphics::Gdi::HDC;

pub(super) struct BackgroundPreview {
    source: std::sync::Arc<image::RgbaImage>,
    size: (u32, u32, i32),
    pixels: Vec<u8>,
}

impl BackgroundPreview {
    #[allow(clippy::cast_sign_loss)] // Coverage and input alpha are nonnegative.
    fn new(source: &std::sync::Arc<image::RgbaImage>, size: (u32, u32, i32)) -> Self {
        let mut pixels = image::imageops::resize(
            source.as_ref(),
            size.0,
            size.1,
            image::imageops::FilterType::Lanczos3,
        )
        .into_raw();
        let radius = f64::from(size.2);
        for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
            let x = (index % size.0 as usize) as f64 + 0.5;
            let y = (index / size.0 as usize) as f64 + 0.5;
            let dx = (radius - x.min(f64::from(size.0) - x)).max(0.0);
            let dy = (radius - y.min(f64::from(size.1) - y)).max(0.0);
            let coverage = (radius + 0.5 - dx.hypot(dy)).clamp(0.0, 1.0);
            let alpha = (f64::from(pixel[3]) * coverage).round() as u8;
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * u16::from(alpha) + 127) / 255) as u8;
            }
            pixel[3] = alpha;
        }
        Self {
            source: std::sync::Arc::clone(source),
            size,
            pixels,
        }
    }
}

fn draw_background_preview(hdc: DC, rect: RECT, state: &WindowState) {
    let theme = state.theme;
    let enabled = state.control_enabled(ControlId::BackgroundFile);
    let hovered = state.hot_control == Some(ControlId::BackgroundFile);
    let focused = state.keyboard_focus && state.focused_control == Some(ControlId::BackgroundFile);
    rounded_surface(
        hdc,
        rect,
        theme.palette.raised,
        theme.palette.raised,
        theme.scale(6),
    );
    if let Some(source) = state.session.background_preview() {
        let fit = super::video_rect(rect, source.width(), source.height());
        let size = (
            (fit.right - fit.left).cast_unsigned(),
            (fit.bottom - fit.top).cast_unsigned(),
            theme.scale(5),
        );
        let mut cache = state.background_preview.borrow_mut();
        if cache.as_ref().is_none_or(|cached| {
            !std::sync::Arc::ptr_eq(&cached.source, source) || cached.size != size
        }) {
            *cache = Some(BackgroundPreview::new(source, size));
        }
        if let Some(cached) = cache.as_ref() {
            crate::platform::editor_ui::draw_alpha_image(hdc, fit, &cached.pixels);
        }
    }
    if state.session.background_preview().is_none() || hovered || focused || !enabled {
        let cx = i32::midpoint(rect.left, rect.right);
        let cy = i32::midpoint(rect.top, rect.bottom);
        let badge = RECT {
            left: cx - theme.scale(16),
            right: cx + theme.scale(16),
            top: cy - theme.scale(16),
            bottom: cy + theme.scale(16),
        };
        rounded_surface(
            hdc,
            badge,
            theme.palette.panel,
            theme.palette.panel,
            theme.scale(6),
        );
        let inset = theme.scale(7);
        draw_icon(
            hdc,
            RECT {
                left: badge.left + inset,
                top: badge.top + inset,
                right: badge.right - inset,
                bottom: badge.bottom - inset,
            },
            IconKind::Image,
            if enabled {
                theme.palette.text_secondary
            } else {
                theme.palette.text_muted
            },
        );
    }
    if hovered || focused {
        focus_ring(
            hdc,
            rect,
            if focused {
                theme.palette.accent
            } else {
                theme.palette.border
            },
            theme.scale(6),
        );
    }
}
fn text(hdc: DC, r: RECT, value: &str, color: COLORREF, style: TextStyle) {
    draw_text_styled(
        hdc,
        r,
        value,
        color,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        style,
    );
}
fn line(hdc: DC, r: RECT, theme: EditorTheme) {
    fill(
        hdc,
        &RECT {
            bottom: r.top + 1,
            ..r
        },
        theme.palette.divider,
    );
}
fn pill(hdc: DC, r: RECT, value: &str, state: &WindowState, id: ControlId) {
    if r.right <= r.left {
        return;
    }
    rounded_surface(
        hdc,
        r,
        state.theme.palette.raised,
        state.theme.palette.divider,
        state.theme.scale(5),
    );
    draw_text_styled(
        hdc,
        r,
        value,
        if state.control_enabled(id) {
            state.theme.palette.text_secondary
        } else {
            state.theme.palette.text_muted
        },
        DT_CENTER | DT_SINGLELINE | DT_VCENTER,
        TextStyle::Body,
    );
    if state.keyboard_focus && state.focused_control == Some(id) {
        focus_ring(hdc, r, state.theme.palette.accent, state.theme.scale(5));
    }
}
pub(super) fn slider(hdc: DC, r: RECT, value: f64, theme: EditorTheme) {
    if r.right <= r.left {
        return;
    }
    let x = r.left
        + theme.scale(5)
        + ((f64::from(r.right - r.left - theme.scale(10)) * value.clamp(0.0, 1.0)).round() as i32);
    let y = i32::midpoint(r.top, r.bottom);
    let rail = RECT {
        left: r.left + theme.scale(5),
        top: y - theme.scale(2),
        right: r.right - theme.scale(5),
        bottom: y + theme.scale(2),
    };
    rounded_surface(
        hdc,
        rail,
        theme.palette.border,
        theme.palette.border,
        theme.scale(2),
    );
    if x > rail.left {
        rounded_surface(
            hdc,
            RECT { right: x, ..rail },
            theme.palette.accent,
            theme.palette.accent,
            theme.scale(2),
        );
    }
    rounded_surface(
        hdc,
        RECT {
            left: x - theme.scale(5),
            right: x + theme.scale(5),
            top: y - theme.scale(5),
            bottom: y + theme.scale(5),
        },
        theme.palette.slider_thumb,
        theme.palette.border,
        theme.scale(5),
    );
}
fn toggle(hdc: DC, r: RECT, label: &str, on: bool, state: &WindowState, id: ControlId) {
    let scale = |n| state.theme.scale(n);
    let trailing = id == ControlId::Cursor;
    let switch_left = if trailing {
        r.right - scale(28)
    } else {
        r.left
    };
    let switch = RECT {
        left: switch_left,
        top: (r.top + r.bottom - scale(17)) / 2,
        right: switch_left + scale(28),
        bottom: (r.top + r.bottom + scale(17)) / 2,
    };
    let color = if on {
        state.theme.palette.accent
    } else {
        state.theme.palette.border
    };
    rounded_surface(hdc, switch, color, color, scale(9));
    let left = switch.left + scale(if on { 13 } else { 2 });
    rounded_surface(
        hdc,
        RECT {
            left,
            top: switch.top + scale(2),
            right: left + scale(13),
            bottom: switch.top + scale(15),
        },
        state.theme.palette.slider_thumb,
        state.theme.palette.slider_thumb,
        scale(7),
    );
    text(
        hdc,
        RECT {
            left: r.left + scale(if trailing { 24 } else { 36 }),
            right: if trailing {
                switch.left - scale(8)
            } else {
                r.right
            },
            ..r
        },
        label,
        state.theme.palette.text_secondary,
        TextStyle::Body,
    );
    if trailing {
        draw_icon(
            hdc,
            RECT {
                left: r.left,
                right: r.left + scale(16),
                top: i32::midpoint(r.top, r.bottom) - scale(8),
                bottom: i32::midpoint(r.top, r.bottom) + scale(8),
            },
            IconKind::Cursor,
            state.theme.palette.text_muted,
        );
    }
    if state.keyboard_focus && state.focused_control == Some(id) {
        focus_ring(hdc, r, state.theme.palette.accent, scale(5));
    }
}
#[allow(clippy::too_many_lines)]
pub(super) fn draw_controls(hdc: DC, controls: &ControlLayout, state: &WindowState) {
    let theme = state.theme;
    let palette = theme.palette;
    let scale = |n| theme.scale(n);
    let snapshot = state.session.snapshot();
    let display_tick = state
        .timeline_scrub
        .map_or(snapshot.project_tick, |scrub| scrub.desired_tick);
    let header_controls = [ControlId::Menu, ControlId::Diagnostics];
    let header_states = header_controls.map(|id| crate::platform::editor_ui::ButtonVisualState {
        interaction: crate::platform::editor_ui::button_interaction(
            state.control_enabled(id),
            state.pressed_control == Some(id),
            state.hot_control == Some(id),
        ),
        selected: false,
    });
    let header_focus = [
        crate::platform::editor_ui::HeaderAction::Menu,
        crate::platform::editor_ui::HeaderAction::Settings,
    ]
    .into_iter()
    .zip(header_controls)
    .find_map(|(action, id)| {
        (state.keyboard_focus && state.focused_control == Some(id)).then_some(action)
    });
    crate::platform::editor_ui::draw_header(
        hdc,
        controls.header,
        theme,
        header_states,
        header_focus,
    );

    line(
        hdc,
        RECT {
            top: controls.timeline_header.bottom - 1,
            ..controls.timeline_header
        },
        theme,
    );
    for x in [98, 226] {
        fill(
            hdc,
            &RECT {
                left: scale(x),
                right: scale(x) + 1,
                top: controls.timeline_header.top + scale(14),
                bottom: controls.timeline_header.bottom - scale(14),
            },
            palette.border,
        );
    }
    line(hdc, controls.context_bar, theme);
    for id in [
        ControlId::Undo,
        ControlId::Redo,
        ControlId::PreviousFrame,
        ControlId::NextFrame,
        ControlId::Home,
        ControlId::Play,
        ControlId::InspectorToggle,
        ControlId::Split,
        ControlId::Delete,
        ControlId::Add,
        ControlId::CameraEnabled,
        ControlId::Reset,
        ControlId::CameraProperties,
        ControlId::VideoTrim,
        ControlId::TimelineZoomOut,
        ControlId::TimelineZoomIn,
        ControlId::TimelineFit,
    ] {
        let selected = match id {
            ControlId::CameraEnabled => state.editor.settings().camera_enabled,
            ControlId::InspectorToggle => controls.panel.right > controls.panel.left,
            _ => false,
        };
        button(
            hdc,
            controls.control_rect(id).0,
            state,
            id,
            selected,
            state.control_enabled(id),
        );
    }
    draw_text_styled(
        hdc,
        controls.time,
        &format!(
            "{}  /  {}",
            format_tick(display_tick),
            format_tick(snapshot.duration_tick)
        ),
        palette.text_secondary,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        TextStyle::Caption,
    );
    let max = (state.editor.project_duration().0 as f64 / (TICKS_PER_SECOND as f64 / 2.0)).max(1.0);
    let zoom = (state.editor.project_duration().0 as f64
        / state.timeline.visible_duration().0 as f64)
        .ln()
        / max.ln().max(f64::EPSILON);
    slider(hdc, controls.zoom_slider.0, zoom, theme);
    draw_tracks(hdc, controls, state);
    draw_selection(hdc, controls, state);
    if controls.panel.right > controls.panel.left {
        use windows::Win32::Graphics::Gdi::{IntersectClipRect, RestoreDC, SaveDC};
        let saved = unsafe { SaveDC(hdc) };
        unsafe {
            let _ = IntersectClipRect(
                hdc,
                controls.panel.left,
                controls.inspector_body_top,
                controls.panel.right,
                controls.panel.bottom - theme.metrics.radius_panel,
            );
        }
        let x = controls.panel.left + scale(20);
        let right = controls.panel.right - scale(20);
        let y = controls.inspector_content_top;
        let row = |top, height| RECT {
            left: x,
            top: y + scale(top),
            right,
            bottom: y + scale(top + height),
        };
        if state.inspector_video_tab {
            draw_video_properties(hdc, controls, state);
        } else {
            // Fixed groups do not shift when the timeline selection changes.
            for top in [58, 222, 454] {
                line(hdc, row(top, 1), theme);
            }
            text(hdc, row(74, 24), "背景", palette.text, TextStyle::Heading);
            text(hdc, row(238, 24), "画面", palette.text, TextStyle::Heading);
            let image = state.inspector_image_tab;
            for (id, selected) in [
                (ControlId::Background, !image),
                (ControlId::BackgroundImage, image),
            ] {
                button(
                    hdc,
                    controls.raw_control_rect(id).0,
                    state,
                    id,
                    selected,
                    true,
                );
            }
            if image {
                draw_background_preview(hdc, controls.background_file.0, state);
            } else {
                choice_button(
                    hdc,
                    controls.raw_control_rect(ControlId::BackgroundColor).0,
                    &format!("自定义  {}", state.editor.settings().background.color),
                    state,
                    ControlId::BackgroundColor,
                    false,
                );
            }
            for (id, r) in SWATCHES
                .into_iter()
                .zip(controls.swatches)
                .filter(|(_, r)| r.right > r.left)
            {
                let color = u32::from_str_radix(&swatch_color(id)[1..], 16).unwrap_or(0);
                let color = rgb((color >> 16) as u8, (color >> 8) as u8, color as u8);
                rounded_surface(hdc, r.0, color, color, scale(6));
                if state
                    .editor
                    .settings()
                    .background
                    .color
                    .eq_ignore_ascii_case(swatch_color(id))
                    || (state.keyboard_focus && state.focused_control == Some(id))
                {
                    focus_ring(hdc, r.0, palette.accent, scale(6));
                } else if state.hot_control == Some(id) {
                    focus_ring(hdc, r.0, palette.text_secondary, scale(6));
                }
            }
            for (id, label, unit) in [
                (ControlId::Inset, "边距", "%"),
                (ControlId::Radius, "圆角", "px"),
                (ControlId::Shadow, "阴影", "%"),
            ] {
                let r = controls.raw_control_rect(id).0;
                let (value, max) = state.widget_range(id).unwrap_or((0.0, 1.0));
                text(
                    hdc,
                    RECT {
                        bottom: r.top + scale(24),
                        right: r.right - scale(65),
                        ..r
                    },
                    label,
                    palette.text_secondary,
                    TextStyle::Body,
                );
                pill(
                    hdc,
                    value_rect(r, theme, id),
                    &format!("{value:.0} {unit}"),
                    state,
                    id,
                );
                slider(
                    hdc,
                    RECT {
                        top: r.top + scale(28),
                        bottom: r.bottom - scale(2),
                        ..r
                    },
                    value / max,
                    theme,
                );
            }
            toggle(
                hdc,
                controls.cursor.0,
                "显示光标",
                state.editor.settings().cursor.visible,
                state,
                ControlId::Cursor,
            );
            choice_button(
                hdc,
                controls.raw_control_rect(ControlId::CursorProperties).0,
                &format!("光标大小  {:.2}×", state.editor.settings().cursor.scale),
                state,
                ControlId::CursorProperties,
                false,
            );
            line(hdc, row(560, 1), theme);
            text(
                hdc,
                row(576, 24),
                "画布尺寸",
                palette.text,
                TextStyle::Heading,
            );
            let size = state.editor.settings().canvas.size;
            for (id, label, ratio) in [
                (ControlId::CanvasOriginal, "原始", None),
                (ControlId::CanvasWide, "16:9", Some((16, 9))),
                (ControlId::CanvasPortrait, "9:16", Some((9, 16))),
                (ControlId::CanvasSquare, "1:1", Some((1, 1))),
            ] {
                let selected = size
                    == ratio
                        .map(|r| panzo_core::CanvasSize::from_ratio(state.frame.source_size(), r));
                choice_button(
                    hdc,
                    controls.raw_control_rect(id).0,
                    label,
                    state,
                    id,
                    selected,
                );
            }
            for (id, label) in [
                (ControlId::CanvasWidth, "宽度"),
                (ControlId::CanvasHeight, "高度"),
            ] {
                let r = controls.raw_control_rect(id).0;
                text(
                    hdc,
                    RECT {
                        bottom: r.top + scale(22),
                        ..r
                    },
                    label,
                    palette.text_secondary,
                    TextStyle::Body,
                );
                pill(
                    hdc,
                    value_rect(r, theme, id),
                    &format!("{:.0} px", state.widget_value(id).unwrap_or(0.0)),
                    state,
                    id,
                );
            }
        }
        unsafe {
            let _ = RestoreDC(hdc, saved);
        }
        for (id, label, selected) in [
            (ControlId::CanvasTab, "画布", !state.inspector_video_tab),
            (ControlId::VideoTab, "视频", state.inspector_video_tab),
        ] {
            choice_button(hdc, controls.control_rect(id).0, label, state, id, selected);
        }
        if controls.inspector_scroll_max > 0 {
            let track = controls.inspector_scrollbar.0;
            let height = track.bottom - track.top;
            let thumb = (height * height / (height + controls.inspector_scroll_max)).max(scale(28));
            let offset =
                (height - thumb) * controls.inspector_scroll / controls.inspector_scroll_max;
            rounded_surface(
                hdc,
                RECT {
                    left: track.left + scale(3),
                    top: track.top + offset,
                    right: track.right - scale(2),
                    bottom: track.top + offset + thumb,
                },
                palette.border,
                palette.border,
                scale(3),
            );
        }
    }
    text(
        hdc,
        controls.status,
        state.status_message().0,
        state.status_message().1.color(theme),
        TextStyle::Caption,
    );
    if let (Some(id), Some(message)) = (state.inline_control, state.inline_validation.as_deref()) {
        draw_validation_hint(hdc, controls, state, id, message);
    } else if state.tooltip_visible
        && let Some(control) = state.hot_control
    {
        draw_control_tooltip(hdc, controls, control, theme);
    }
}
fn draw_validation_hint(
    hdc: DC,
    controls: &ControlLayout,
    state: &WindowState,
    id: ControlId,
    message: &str,
) {
    use windows::Win32::Graphics::Gdi::{DT_NOPREFIX, DT_WORDBREAK};
    let theme = state.theme;
    let anchor = controls.control_rect(id);
    let edge = theme.scale(8);
    let bounds = if ControlLayout::is_inspector_control(id) {
        controls.panel
    } else {
        controls.command_bar
    };
    let width = theme
        .scale(360)
        .min(bounds.right - bounds.left - edge * 2)
        .max(1);
    let height = theme.scale(76);
    let left = anchor.left.clamp(
        bounds.left + edge,
        (bounds.right - edge - width).max(bounds.left + edge),
    );
    let top = if anchor.bottom + height + edge < controls.status.top {
        anchor.bottom + edge
    } else {
        (anchor.top - height - edge).max(theme.metrics.toolbar_height)
    };
    let rect = RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    };
    rounded_surface(
        hdc,
        rect,
        theme.palette.raised,
        theme.palette.border,
        theme.scale(6),
    );
    draw_text_styled(
        hdc,
        RECT {
            left: left + edge,
            top: top + edge,
            right: rect.right - edge,
            bottom: rect.bottom - edge,
        },
        message,
        theme.palette.text,
        DT_LEFT | DT_WORDBREAK | DT_NOPREFIX,
        TextStyle::Caption,
    );
}
fn choice_button(
    hdc: DC,
    rect: RECT,
    label: &str,
    state: &WindowState,
    id: ControlId,
    selected: bool,
) {
    let palette = state.theme.palette;
    let enabled = state.control_enabled(id);
    let hovered = enabled && state.hot_control == Some(id);
    let focused = enabled && state.keyboard_focus && state.focused_control == Some(id);
    let color = if selected || hovered {
        palette.raised
    } else {
        palette.panel
    };
    rounded_surface(hdc, rect, color, color, state.theme.scale(5));
    if hovered || focused {
        focus_ring(
            hdc,
            rect,
            if focused {
                palette.accent
            } else {
                palette.border
            },
            state.theme.scale(5),
        );
    }
    draw_text_styled(
        hdc,
        rect,
        label,
        if !enabled {
            palette.text_muted
        } else if selected {
            palette.accent
        } else {
            palette.text_secondary
        },
        DT_CENTER | DT_SINGLELINE | DT_VCENTER,
        TextStyle::Button,
    );
}

fn section_heading(
    hdc: DC,
    rect: RECT,
    state: &WindowState,
    id: ControlId,
    label: &str,
    collapsed: bool,
) {
    let theme = state.theme;
    if state.hot_control == Some(id) || state.focused_control == Some(id) && state.keyboard_focus {
        rounded_surface(
            hdc,
            rect,
            theme.palette.raised,
            theme.palette.border,
            theme.scale(4),
        );
    }
    text(
        hdc,
        RECT {
            right: rect.right - theme.scale(20),
            ..rect
        },
        label,
        theme.palette.text,
        TextStyle::Heading,
    );
    draw_icon(
        hdc,
        RECT {
            left: rect.right - theme.scale(18),
            top: rect.top + theme.scale(8),
            bottom: rect.bottom - theme.scale(8),
            ..rect
        },
        if collapsed {
            IconKind::ChevronRight
        } else {
            IconKind::ChevronDown
        },
        theme.palette.text_muted,
    );
}

#[allow(clippy::too_many_lines)] // Ordered property groups share layout and hit rectangles.
fn draw_video_properties(hdc: DC, controls: &ControlLayout, state: &WindowState) {
    let theme = state.theme;
    let scale = |n| theme.scale(n);
    let palette = theme.palette;
    for (index, id, label) in [
        (0, ControlId::SpeedSection, "播放速度"),
        (1, ControlId::CropSection, "画面裁剪"),
    ] {
        section_heading(
            hdc,
            controls.video_sections[index].0,
            state,
            id,
            label,
            state.video_collapsed[index],
        );
    }
    let video = state.editor.video_edit();
    let clip = video
        .clips
        .iter()
        .find(|clip| Some(&clip.id) == state.selected_video.as_ref());
    button(
        hdc,
        controls.speed_reset.0,
        state,
        ControlId::SpeedReset,
        false,
        clip.is_some(),
    );
    for (id, selected) in [
        (ControlId::CropReset, false),
        (ControlId::CropEdit, state.crop_editing),
        (ControlId::CropLock, state.crop_locked),
    ] {
        button(
            hdc,
            controls.raw_control_rect(id).0,
            state,
            id,
            selected,
            clip.is_some(),
        );
    }
    let Some(clip) = clip else {
        return;
    };
    let speed = f64::from(clip.speed_percent) / 100.0;
    if !state.video_collapsed[0] {
        let r = controls.video_speed.0;
        text(
            hdc,
            RECT {
                bottom: r.top + scale(24),
                right: r.right - scale(65),
                ..r
            },
            "速度",
            palette.text_secondary,
            TextStyle::Body,
        );
        pill(
            hdc,
            value_rect(r, theme, ControlId::VideoSpeed),
            &format!("{speed:.2}×"),
            state,
            ControlId::VideoSpeed,
        );
        slider(
            hdc,
            RECT {
                top: r.top + scale(28),
                bottom: r.bottom - scale(2),
                ..r
            },
            (speed.log2() + 2.0) / 4.0,
            theme,
        );
        for (id, preset) in super::editor_widgets::SPEED_PRESETS {
            choice_button(
                hdc,
                controls.raw_control_rect(id).0,
                &format!("{preset}×"),
                state,
                id,
                speed == preset,
            );
        }
        for (offset, label, duration) in [
            (
                132,
                "原始时长",
                clip.source_out_tick.0 - clip.source_in_tick.0,
            ),
            (162, "调整后时长", clip.duration().0),
        ] {
            let rect = RECT {
                top: r.top + scale(offset),
                bottom: r.top + scale(offset + 24),
                ..r
            };
            text(hdc, rect, label, palette.text_muted, TextStyle::Caption);
            draw_text_styled(
                hdc,
                rect,
                &format!("{:.3} 秒", duration as f64 / TICKS_PER_SECOND as f64),
                palette.text_secondary,
                windows::Win32::Graphics::Gdi::DT_RIGHT | DT_SINGLELINE | DT_VCENTER,
                TextStyle::Body,
            );
        }
    }
    let header = controls.video_sections[1].0;
    line(
        hdc,
        RECT {
            left: controls.panel.left + scale(20),
            right: controls.panel.right - scale(20),
            top: header.top - scale(12),
            bottom: header.top - scale(11),
        },
        theme,
    );
    if state.video_collapsed[1] {
        return;
    }
    let size = state.frame.source_size();
    for (id, label, ratio) in super::editor_widgets::CROP_PRESETS {
        choice_button(
            hdc,
            controls.raw_control_rect(id).0,
            label,
            state,
            id,
            clip.crop == panzo_core::VideoCrop::centered_aspect(size.0, size.1, ratio),
        );
    }
    for (id, label) in super::editor_widgets::CROP_EDGES {
        let rect = controls.raw_control_rect(id).0;
        let (value, max) = state.widget_range(id).unwrap_or((0.0, 95.0));
        text(
            hdc,
            RECT {
                bottom: rect.top + scale(24),
                right: rect.right - scale(65),
                ..rect
            },
            label,
            palette.text_secondary,
            TextStyle::Body,
        );
        pill(
            hdc,
            value_rect(rect, theme, id),
            &format!("{value:.1}%"),
            state,
            id,
        );
        slider(
            hdc,
            RECT {
                top: rect.top + scale(28),
                bottom: rect.bottom - scale(2),
                ..rect
            },
            value / max.max(1.0),
            theme,
        );
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn draw_selection(hdc: DC, controls: &ControlLayout, state: &WindowState) {
    let theme = state.theme;
    let palette = theme.palette;
    let scale = |n| theme.scale(n);
    let camera = state.selected_segment.is_some();
    if !camera && state.selected_video.is_none() {
        return;
    }
    text(
        hdc,
        controls.selected_info,
        if camera { "镜头" } else { "视频片段" },
        palette.text,
        TextStyle::Body,
    );
    for (id, label) in [
        (ControlId::StartValue, if camera { "开始" } else { "入点" }),
        (ControlId::EndValue, if camera { "结束" } else { "出点" }),
        (ControlId::ScaleValue, "倍率"),
    ] {
        let r = controls.control_rect(id).0;
        if r.right <= r.left {
            continue;
        }
        text(
            hdc,
            RECT {
                left: r.left - scale(36),
                right: r.left - scale(6),
                ..r
            },
            label,
            palette.text_secondary,
            TextStyle::Caption,
        );
        let value = state.widget_value(id).map_or("—".into(), |v| {
            if id == ControlId::ScaleValue {
                format!("{v:.2}×")
            } else {
                format!("{v:.3}")
            }
        });
        pill(hdc, r, &value, state, id);
    }
    if camera {
        button(
            hdc,
            controls.center_focus.0,
            state,
            ControlId::CenterFocus,
            false,
            true,
        );
    }
    if controls.context_bar.right >= scale(760) {
        let start = state.widget_value(ControlId::StartValue).unwrap_or(0.0);
        let end = state.widget_value(ControlId::EndValue).unwrap_or(0.0);
        text(
            hdc,
            RECT {
                left: controls.context_bar.right - scale(190),
                right: controls.context_bar.right - scale(18),
                ..controls.context_bar
            },
            &format!(
                "时长 {:.3} 秒",
                (end - start).max(0.0) / state.widget_value(ControlId::VideoSpeed).unwrap_or(1.0)
            ),
            palette.text_muted,
            TextStyle::Caption,
        );
    }
}

#[allow(clippy::too_many_lines)] // Keep track layers and clip selection in drawing order.
pub(super) fn draw_tracks(hdc: DC, controls: &ControlLayout, state: &WindowState) {
    let theme = state.theme;
    let palette = theme.palette;
    let scale = |n| theme.scale(n);
    let snapshot = state.session.snapshot();
    let display_tick = state
        .timeline_scrub
        .map_or(snapshot.project_tick, |scrub| scrub.desired_tick);
    fill(
        hdc,
        &RECT {
            left: controls.timeline_panel.left,
            top: controls.ruler.top,
            right: controls.timeline_panel.right,
            bottom: controls.scrollbar.bottom,
        },
        palette.panel,
    );
    text(
        hdc,
        RECT {
            left: controls.track_labels.left,
            right: controls.track_labels.right,
            ..controls.ruler.0
        },
        "时间",
        palette.text_muted,
        TextStyle::Caption,
    );
    for (rect, icon) in [
        (controls.video_track.0, IconKind::Monitor),
        (controls.camera_track.0, IconKind::Camera),
    ] {
        draw_icon(
            hdc,
            RECT {
                left: controls.track_labels.left,
                right: controls.track_labels.left + scale(13),
                top: (rect.top + rect.bottom - scale(13)) / 2,
                bottom: (rect.top + rect.bottom + scale(13)) / 2,
            },
            icon,
            palette.text_muted,
        );
    }
    draw_timeline_ruler(hdc, controls.ruler, state.timeline, theme);
    rounded_surface(
        hdc,
        controls.click_track.0,
        palette.track,
        palette.track,
        scale(2),
    );
    rounded_surface(
        hdc,
        controls.camera_track.0,
        palette.track_alt,
        palette.track_alt,
        scale(4),
    );
    let video = state.editor.video_edit();
    let mapped_clicks: Vec<_> = state
        .editor
        .click_ticks()
        .iter()
        .filter_map(|tick| panzo_core::TimeMapping::project_time(&video, *tick).ok())
        .collect();
    let click_ticks = &mapped_clicks;
    let first_visible_click =
        click_ticks.partition_point(|tick| *tick < state.timeline.visible_start());
    for tick in &click_ticks[first_visible_click..] {
        if *tick > state.timeline.visible_end() {
            break;
        }
        let x = timeline_x(controls.click_track, *tick, state.timeline);
        fill(
            hdc,
            &RECT {
                left: x - 1,
                top: controls.click_track.top,
                right: x + 1,
                bottom: controls.click_track.bottom,
            },
            palette.warning,
        );
    }
    let displayed_segments = state.display_segments();
    let segments = &displayed_segments;
    state.draw_video_track(hdc, controls.video_track);
    state.draw_scrollbar(hdc, controls.scrollbar);
    let first_visible_segment =
        segments.partition_point(|segment| segment.end_tick < state.timeline.visible_start());
    for segment in &segments[first_visible_segment..] {
        if segment.start_tick > state.timeline.visible_end() {
            break;
        }
        let rect = segment_rect(controls.camera_track, segment, state.timeline);
        if rect.right <= controls.camera_track.left || rect.left >= controls.camera_track.right {
            continue;
        }
        let rect = RECT {
            left: rect.left.max(controls.camera_track.left),
            right: rect.right.min(controls.camera_track.right),
            ..rect
        };
        let selected = state.selected_segment.as_deref() == Some(segment.id.as_str());
        let accent = palette.camera_border;
        rounded_surface(
            hdc,
            rect,
            if selected {
                palette.camera_selected
            } else {
                palette.camera_clip
            },
            if selected {
                accent
            } else {
                palette.camera_clip
            },
            scale(5),
        );
        // Short edge grips stay discreet when the timeline grows. Narrow clips use
        // their selection outline alone; no pair of bars can crowd the centre.
        if selected && rect.right - rect.left >= scale(28) {
            let half = scale(6).min((rect.bottom - rect.top - scale(8)).max(0) / 2);
            let y = i32::midpoint(rect.top, rect.bottom);
            for (x, visible) in [
                (
                    rect.left + scale(4),
                    segment.start_tick >= state.timeline.visible_start(),
                ),
                (
                    rect.right - scale(6),
                    segment.end_tick <= state.timeline.visible_end(),
                ),
            ] {
                if visible && half > 0 {
                    rounded_surface(
                        hdc,
                        RECT {
                            left: x,
                            right: x + scale(2),
                            top: y - half,
                            bottom: y + half,
                        },
                        accent,
                        accent,
                        scale(1),
                    );
                }
            }
        }
    }
    state.draw_camera_drag(hdc, controls.camera_track);

    if let Some(tick) = state
        .snap_guide
        .or_else(|| state.camera_snap_guide(controls.camera_track))
        .filter(|tick| state.timeline.contains(*tick))
    {
        let x = timeline_x(controls.camera_track, tick, state.timeline);
        for top in (controls.ruler.top..controls.camera_track.bottom)
            .step_by(usize::try_from(scale(8)).unwrap_or(8))
        {
            fill(
                hdc,
                &RECT {
                    left: x - 1,
                    right: x + 1,
                    top,
                    bottom: (top + scale(4)).min(controls.camera_track.bottom),
                },
                COLORREF(0x0058_C7FF),
            );
        }
    }
    if state.timeline.contains(display_tick) {
        let playhead_x = timeline_x(controls.camera_track, display_tick, state.timeline);
        fill(
            hdc,
            &RECT {
                left: playhead_x - 1,
                top: controls.ruler.top,
                right: playhead_x + 1,
                bottom: controls.camera_track.bottom + 2,
            },
            palette.accent,
        );
    }
}
