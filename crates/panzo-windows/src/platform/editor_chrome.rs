//! Approved prototype chrome. Layout, drawing and interaction share `ControlLayout`.
use super::editor_widgets::value_rect;
use windows::Win32::Graphics::Gdi::DT_RIGHT;
use super::{
    COLORREF, ControlId, ControlLayout, DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER, EditorTheme,
    IconKind, RECT, TextStyle, WindowState, button,
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
        state.theme.palette.raised,
        state.theme.scale(3),
    );
    let d=|n|state.theme.scale(n);
    let unit=match id {
        ControlId::Inset|ControlId::Radius|ControlId::CanvasWidth|ControlId::CanvasHeight=>"px",
        ControlId::VideoScale|ControlId::Shadow|ControlId::CropLeft|ControlId::CropTop|ControlId::CropRight|ControlId::CropBottom=>"%",
        ControlId::VideoSpeed|ControlId::ScaleValue|ControlId::FollowZoom=>"×",
        ControlId::StartValue|ControlId::EndValue=>"s", _=>"",
    };
    let value=value.trim().trim_end_matches("px").trim_end_matches('%').trim_end_matches('×').trim();
    let ink=if state.control_enabled(id){state.theme.palette.text}else{state.theme.palette.text_muted};
    draw_text_styled(hdc,RECT{left:r.left+d(6),right:r.right-d(40),..r},value,ink,DT_RIGHT|DT_SINGLELINE|DT_VCENTER,TextStyle::Caption);
    draw_text_styled(hdc,RECT{left:r.right-d(24),right:r.right-d(5),..r},unit,state.theme.palette.text_muted,DT_RIGHT|DT_SINGLELINE|DT_VCENTER,TextStyle::Micro);
    if state.hot_control==Some(id) || state.focused_control==Some(id) && state.keyboard_focus {
        let middle=(r.top+r.bottom)/2;
        for (icon,top) in [(IconKind::ChevronUp,middle-d(10)),(IconKind::ChevronDown,middle+d(1))] {
            draw_icon(hdc,RECT{left:r.right-d(39),right:r.right-d(28),top,bottom:top+d(9)},icon,state.theme.palette.text_secondary);
        }
    }
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
    let trailing = matches!(id,ControlId::Cursor|ControlId::CameraEnabled);
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
            left: r.left + scale(if id == ControlId::Cursor { 24 } else if trailing { 0 } else { 36 }),
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
    if id == ControlId::Cursor {
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
    let progress=RECT {top:controls.transport.top,bottom:controls.transport.top+scale(2),..controls.transport};
    fill(hdc,&progress,palette.border);
    let fraction=snapshot.project_tick.0.max(0) as f64/snapshot.duration_tick.0.max(1) as f64;
    fill(hdc,&RECT{right:progress.left+((progress.right-progress.left) as f64*fraction.clamp(0.0,1.0)) as i32,..progress},palette.accent);
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
    for x in [80, 157] {
        fill(
            hdc,
            &RECT {
                left: scale(x),
                right: scale(x) + 1,
                top: controls.timeline_header.top + scale(10),
                bottom: controls.timeline_header.bottom - scale(10),
            },
            palette.border,
        );
    }
    text(
        hdc,
        RECT {
            left: controls.preview_fit.left - scale(64),
            right: controls.preview_fit.left - scale(8),
            top: controls.preview_fit.top,
            bottom: controls.preview_fit.bottom,
        },
        &format!("{:.0}%", state.preview_view.zoom * 100.0),
        palette.text_muted,
        TextStyle::Caption,
    );
    if controls.context_bar.bottom > controls.context_bar.top {
        line(hdc, controls.context_bar, theme);
    }
    for id in [
        ControlId::NewRecording,
        ControlId::OpenProject,
        ControlId::Undo,
        ControlId::Redo,
        ControlId::Last,
        ControlId::PreviewMute,
        ControlId::Home,
        ControlId::Play,
        ControlId::InspectorToggle,
        ControlId::PreviewFit,
        ControlId::AudioPanel,
        ControlId::SubtitlePanel,
        ControlId::Split,
        ControlId::Delete,
        ControlId::Add,
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
    line(hdc, RECT { bottom: controls.transport.top + 1, ..controls.transport }, theme);
    text(hdc, RECT { left: controls.timeline_zoom_out.left-scale(110), right: controls.timeline_zoom_out.left-scale(8), ..controls.timeline_header }, &format!("时长 {}", format_tick(snapshot.duration_tick)), palette.text_muted, TextStyle::Caption);
    let zoom = state.timeline.zoom_fraction();
    slider(hdc, controls.zoom_slider.0, zoom, theme);
    if unsafe { windows::Win32::Graphics::Gdi::RectVisible(hdc, &controls.timeline_panel) }.as_bool() {
        draw_tracks(hdc, controls, state);
    }
    if controls.panel.right > controls.panel.left && unsafe { windows::Win32::Graphics::Gdi::RectVisible(hdc, &controls.panel) }.as_bool() {
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
        let x = controls.panel.left + scale(16);
        let right = controls.panel.right - scale(16);
        let y = controls.inspector_content_top;
        let row = |top, height| RECT {
            left: x,
            top: y + scale(top),
            right,
            bottom: y + scale(top + height),
        };
        if state.inspector_panel.is_some() {
            // A child page occupies the shared content rectangle.
        } else if state.inspector_video_tab {
            draw_selection(hdc, controls, state);
            draw_video_properties(hdc, controls, state);
        } else {
            line(hdc,row(178,1),theme);
            let advanced=controls.raw_control_rect(ControlId::CanvasAdvanced).0;
            line(hdc,RECT{top:advanced.top-scale(12),..advanced},theme);
            text(hdc,row(58,24),"项目画面",palette.text,TextStyle::Heading);
            let size=state.canvas_size();
            segmented_background(hdc,controls,state,&[ControlId::CanvasWide,ControlId::CanvasStandard,ControlId::CanvasSquare,ControlId::CanvasPortrait]);
            for (id,label,(w,h)) in [(ControlId::CanvasWide,"16:9",(16,9)),(ControlId::CanvasStandard,"4:3",(4,3)),(ControlId::CanvasSquare,"1:1",(1,1)),(ControlId::CanvasPortrait,"9:16",(9,16))] {
                choice_button(hdc,controls.raw_control_rect(id).0,label,state,id,(i64::from(size.width)*h-i64::from(size.height)*w).abs()<=w.max(h));
            }
            text(hdc,row(134,28),"分辨率",palette.text_secondary,TextStyle::Body);
            let r=controls.resolution.0;
            choice_button(hdc,r,&format!("{} × {}",size.width,size.height),state,ControlId::CanvasResolution,false);
            draw_icon(hdc,RECT{left:r.right-scale(20),right:r.right-scale(8),top:r.top+scale(8),bottom:r.bottom-scale(8)},IconKind::ChevronDown,palette.text_muted);
            text(hdc,row(192,22),"背景",palette.text,TextStyle::Heading);
            let image=state.editor.settings().background.kind==panzo_core::BackgroundKind::Image;
            segmented_background(hdc,controls,state,&[ControlId::Background,ControlId::BackgroundImage]);
            for (id,label,on) in [(ControlId::Background,"纯色背景",!image),(ControlId::BackgroundImage,"图片背景",image)] {choice_button(hdc,controls.raw_control_rect(id).0,label,state,id,on);}
            if image {draw_background_preview(hdc,controls.background_file.0,state);} else {
                text(hdc,row(262,28),"颜色",palette.text_secondary,TextStyle::Body);
                let r=controls.background_color.0;
                let color=u32::from_str_radix(state.editor.settings().background.color.trim_start_matches('#'),16).unwrap_or(0);
                rounded_surface(hdc,r,palette.raised,palette.raised,scale(2));
                fill(hdc,&RECT{left:r.left+scale(5),top:r.top+scale(5),right:r.right-scale(5),bottom:r.bottom-scale(5)},rgb((color>>16) as u8,(color>>8) as u8,color as u8));
            }
            section_heading(hdc,controls.raw_control_rect(ControlId::CanvasAdvanced).0,state,ControlId::CanvasAdvanced,"圆角与阴影",!state.canvas_advanced);
            for (id,label) in [(ControlId::Inset,"边距"),(ControlId::Radius,"圆角"),(ControlId::Shadow,"阴影")] {
                let r=controls.raw_control_rect(id).0;
                if r.right<=r.left {continue;}
                text(hdc,RECT{right:r.right-scale(110),..r},label,palette.text_secondary,TextStyle::Body);
                pill(hdc,value_rect(r,theme,id),&super::editor_widgets::format_widget_value(id,state.widget_value(id).unwrap_or(0.)),state,id);
            }
            if state.canvas_advanced {
                toggle(hdc,controls.cursor.0,"显示光标",state.editor.settings().cursor.visible,state,ControlId::Cursor);
                choice_button(hdc,controls.cursor_properties.0,&format!("光标大小  {:.1}×",state.editor.settings().cursor.scale),state,ControlId::CursorProperties,false);
                for (id,label) in [(ControlId::CanvasWidth,"自定义宽度"),(ControlId::CanvasHeight,"自定义高度")] {
                    let r=controls.raw_control_rect(id).0;text(hdc,RECT{bottom:r.top+scale(22),..r},label,palette.text_secondary,TextStyle::Caption);
                    pill(hdc,value_rect(r,theme,id),&super::editor_widgets::format_widget_value(id,state.widget_value(id).unwrap_or(0.)),state,id);
                }

            }
        }
        unsafe {
            let _ = RestoreDC(hdc, saved);
        }
        line(hdc, RECT { top: controls.inspector_body_top-1, bottom: controls.inspector_body_top, ..controls.panel }, theme);
        for (id, label) in [
            (ControlId::CanvasTab, "画面"),
            (ControlId::VideoTab, "视频"),
            (ControlId::AudioTab, "声音"),
            (ControlId::SubtitleTab, "字幕"),
        ] {
            let selected = state.inspector_tab_selected(id);
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
    if controls.panel.right > controls.panel.left {
        text(
            hdc,
            crate::platform::workbench_layout::panel_title(controls.panel, theme),
            "项目与片段",
            palette.text_secondary,
            TextStyle::Caption,
        );
    }
    draw_document_status(hdc, controls.status, state);
    if let (Some(id), Some(message)) = (state.inline_control, state.inline_validation.as_deref()) {
        draw_validation_hint(hdc, controls, state, id, message);
    }
}
/// Save state sits beside the page menu. Routine editing hints stay off the canvas.
fn draw_document_status(hdc: DC, row: RECT, state: &WindowState) {
    use crate::platform::editor_ui::StatusSeverity;
    use windows::Win32::Graphics::Gdi::DT_END_ELLIPSIS;
    let theme = state.theme;
    let badge = RECT {
        right: (row.left + theme.scale(64)).min(row.right),
        ..row
    };
    text(
        hdc,
        badge,
        if state.save_task.is_some() {
            "保存中…"
        } else if state.editor.is_dirty() {
            "未保存"
        } else {
            "已保存"
        },
        if state.editor.is_dirty() {
            theme.palette.accent
        } else {
            theme.palette.text_muted
        },
        TextStyle::Caption,
    );
    let (message, severity) = state.status_message();
    if matches!(severity, StatusSeverity::Warning | StatusSeverity::Error) && !message.is_empty() {
        let feedback = RECT {
            left: badge.right + theme.scale(12),
            ..row
        };
        if feedback.left < feedback.right {
            draw_text_styled(
                hdc,
                feedback,
                message,
                severity.color(theme),
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
                TextStyle::Caption,
            );
        }
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
    let top = if anchor.bottom + height + edge < controls.timeline_panel.bottom {
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
fn segmented_background(hdc: DC, controls: &ControlLayout, state: &WindowState, ids: &[ControlId]) {
    let mut rect=None::<RECT>;
    for id in ids {
        let r=controls.raw_control_rect(*id).0;
        if r.right<=r.left || r.bottom<=r.top {continue;}
        rect=Some(rect.map_or(r,|a|RECT{left:a.left.min(r.left),right:a.right.max(r.right),top:a.top.min(r.top),bottom:a.bottom.max(r.bottom)}));
    }
    if let Some(r)=rect {rounded_surface(hdc,r,state.theme.palette.raised,state.theme.palette.raised,state.theme.scale(4));}
}
fn is_segment_choice(id:ControlId)->bool {
    matches!(id,ControlId::CanvasWide|ControlId::CanvasStandard|ControlId::CanvasSquare|ControlId::CanvasPortrait|ControlId::CropLock|ControlId::CropWide|ControlId::CropStandard|ControlId::CropSquare|ControlId::SpeedHalf|ControlId::SpeedNormal|ControlId::SpeedOneHalf|ControlId::SpeedDouble|ControlId::Background|ControlId::BackgroundImage)
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
    if rect.right <= rect.left || rect.bottom <= rect.top { return; }
    let rect=if is_segment_choice(id) { let d=state.theme.scale(2); RECT{left:rect.left+d,right:rect.right-d,top:rect.top+d,bottom:rect.bottom-d} } else {rect};
    let color = if selected || hovered { palette.hover } else if matches!(id,ControlId::CanvasTab|ControlId::VideoTab|ControlId::AudioTab|ControlId::SubtitleTab) {palette.panel} else { palette.raised };
    rounded_surface(hdc, rect, color, color, state.theme.scale(5));
    if focused {
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
            palette.text
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
    if state.selected_video.is_none() {
        return;
    }
    let theme = state.theme;
    let scale = |n| theme.scale(n);
    let palette = theme.palette;
    for (index, id, label) in [(1, ControlId::CropSection, "画面裁剪")] {
        section_heading(
            hdc,
            controls.video_sections[index].0,
            state,
            id,
            label,
            state.video_collapsed[index],
        );
    }
    section_heading(hdc,controls.video_sections[0].0,state,ControlId::SpeedSection,"鼠标跟随与缩放",!state.video_collapsed[0]);
    if state.video_collapsed[0] {
        toggle(hdc,controls.camera_enabled.0,"启用",state.editor.settings().camera_enabled,state,ControlId::CameraEnabled);
        text(hdc,RECT{left:controls.camera_enabled.left,right:controls.follow_zoom.left-scale(8),..controls.follow_zoom.0},"放大倍数",palette.text_secondary,TextStyle::Body);
        pill(hdc,controls.follow_zoom.0,&format!("{:.1}",state.widget_value(ControlId::FollowZoom).unwrap_or(1.5)),state,ControlId::FollowZoom);
    }
    let clip = state.selected_video_clip();
    button(
        hdc,
        controls.speed_reset.0,
        state,
        ControlId::SpeedReset,
        false,
        clip.is_some(),
    );
    for (id, selected) in [(ControlId::CropReset, false)] {
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
    {
        let r = controls.video_speed.0;
        text(
            hdc,
            RECT {
                bottom: r.top + scale(24),
                right: r.right - scale(65),
                ..r
            },
            "播放速度",
            palette.text_secondary,
            TextStyle::Body,
        );
        pill(
            hdc,
            value_rect(r, theme, ControlId::VideoSpeed),
            &format!("{speed:.1}"),
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
        segmented_background(hdc,controls,state,&[ControlId::SpeedHalf,ControlId::SpeedNormal,ControlId::SpeedOneHalf,ControlId::SpeedDouble]);
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
    }
    let r=controls.video_scale.0;
    text(hdc,RECT{bottom:r.top+scale(24),right:r.right-scale(110),..r},"画面缩放",palette.text_secondary,TextStyle::Body);
    let zoom=state.widget_value(ControlId::VideoScale).unwrap_or(100.0);
    pill(hdc,value_rect(r,theme,ControlId::VideoScale),&format!("{zoom:.1}"),state,ControlId::VideoScale);
    slider(hdc,RECT{top:r.top+scale(28),bottom:r.bottom-scale(2),..r},(zoom-5.0)/395.0,theme);
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
    choice_button(
        hdc,
        controls.crop_edit.0,
        if state.crop_editing {
            "结束画面裁剪"
        } else {
            "在预览中裁剪"
        },
        state,
        ControlId::CropEdit,
        state.crop_editing,
    );
    segmented_background(hdc,controls,state,&[ControlId::CropLock,ControlId::CropWide,ControlId::CropStandard,ControlId::CropSquare]);
    choice_button(hdc, controls.crop_lock.0, "自由", state, ControlId::CropLock, !state.crop_locked);
    let size = state.selected_video_size();
    for (id, label, ratio) in super::editor_widgets::CROP_PRESETS {
        choice_button(
            hdc,
            controls.raw_control_rect(id).0,
            label,
            state,
            id,
            state.crop_locked && ((f64::from(size.0) * clip.crop.width())
                / (f64::from(size.1) * clip.crop.height()) - ratio).abs() < 0.015,
        );
    }
    for (id, label) in super::editor_widgets::CROP_EDGES {
        let rect = controls.raw_control_rect(id).0;
        if rect.right <= rect.left {
            continue;
        }
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
    let scale = |n| theme.scale(n);
    let palette = theme.palette;
    if state.media_selected.is_some() && state.selected_video.is_none() {
        return;
    }
    let camera = state.selected_segment.is_some();
    if !camera && state.selected_video.is_none() {
        text(
            hdc,
            RECT {
                left: controls.panel.left + scale(16),
                right: controls.panel.right - scale(16),
                top: controls.inspector_body_top + scale(16),
                bottom: controls.inspector_body_top + scale(44),
            },
            "在轨道中选择片段",
            palette.text_muted,
            TextStyle::Body,
        );
        return;
    }
    if !camera { return; }
    draw_icon(hdc, RECT{right:controls.selected_info.left+scale(16),..controls.selected_info},IconKind::Camera,palette.accent);
    text(hdc,RECT{left:controls.selected_info.left+scale(25),..controls.selected_info},"鼠标跟踪",palette.text,TextStyle::Heading);
    let duration=state.widget_value(ControlId::EndValue).unwrap_or(0.)-state.widget_value(ControlId::StartValue).unwrap_or(0.);
    text(hdc, RECT{left:controls.selected_info.left+scale(25),top:controls.selected_info.top+scale(24),bottom:controls.selected_info.top+scale(44),..controls.selected_info}, &format!("镜头 · {duration:.2} 秒"),palette.text_secondary,TextStyle::Caption);
    for y in [58,157] { line(hdc, RECT{top:controls.selected_info.top+scale(y),bottom:controls.selected_info.top+scale(y)+1,..controls.selected_info},theme); }
    text(hdc, RECT{top:controls.selected_info.top+scale(76),bottom:controls.selected_info.top+scale(96),..controls.selected_info},"时间范围",palette.text,TextStyle::Heading);
    for (id, label) in [
        (ControlId::StartValue, "开始"),
        (ControlId::EndValue, "结束"),
        (ControlId::ScaleValue, "倍率"),
    ] {
        let r = controls.control_rect(id).0;
        if r.right <= r.left {
            continue;
        }
        text(
            hdc,
            RECT {
                left: r.left - scale(32),
                right: r.left - scale(4),
                ..r
            },
            label,
            palette.text_secondary,
            TextStyle::Caption,
        );
        let value = state.widget_value(id).map_or("—".into(), |v| {
            if id == ControlId::ScaleValue {
                format!("{v:.1}×")
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
}

#[allow(clippy::too_many_lines)] // Keep track layers and clip selection in drawing order.
pub(super) fn draw_tracks(hdc: DC, controls: &ControlLayout, state: &WindowState) {
    let theme = state.theme;
    let palette = theme.palette;
    let scale = |n| theme.scale(n);
    let display_tick = state
        .timeline_scrub
        .map_or(state.session.project_playhead(), |scrub| scrub.desired_tick);
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
            right: controls.ruler.left - scale(28),
            ..controls.ruler.0
        },
        "时间",
        palette.text_muted,
        TextStyle::Caption,
    );
    draw_timeline_ruler(hdc, controls.ruler, state.timeline, theme);
    let saved = unsafe { windows::Win32::Graphics::Gdi::SaveDC(hdc) };
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::IntersectClipRect(
            hdc,
            controls.timeline_panel.left,
            controls.track_area.top,
            controls.timeline_panel.right,
            controls.track_area.bottom,
        );
    }
    state.draw_track_switches(hdc,controls);
    for (rect, icon) in [
        (controls.video_track.0, IconKind::Monitor),
        (controls.camera_track.0, IconKind::Camera),
    ] {
        if rect.bottom <= rect.top {
            continue;
        }
        text(
            hdc,
            RECT {
                left: controls.track_labels.left + scale(22),
                right: controls.track_labels.right,
                top: rect.top,
                bottom: rect.bottom,
            },
            if rect == controls.video_track.0 {
                "视频"
            } else {
                "跟随"
            },
            palette.text_secondary,
            TextStyle::Caption,
        );
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
    if state.editor.settings().source_video_enabled && controls.video_track.bottom>controls.video_track.top {state.draw_video_track(hdc, controls.video_track);}

    let first_visible_segment =
        segments.partition_point(|segment| segment.end_tick < state.timeline.visible_start());
    for segment in segments[first_visible_segment..].iter().filter(|s|controls.camera_track.bottom>controls.camera_track.top&&state.editor.settings().clip_tracks.get(&s.id).map_or("follow",String::as_str)=="follow") {
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
        crate::platform::editor_ui::track_clip(
            hdc,
            rect,
            palette.camera_clip,
            accent,
            selected,
            theme,
        );
        crate::platform::editor_ui::track_grips(hdc, rect, accent, selected, theme);
    }
    state.draw_camera_drag(hdc, controls.camera_track);
    state.draw_media_tracks(hdc, controls);

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
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::RestoreDC(hdc, saved);
    }
    if controls.track_scroll_max > 0 {
        let a = controls.track_area;
        let height = a.bottom - a.top;
        let total = height + controls.track_scroll_max;
        let bar = (height * height / total).max(scale(24));
        let y = a.top
            + state.track_scroll.clamp(0, controls.track_scroll_max) * (height - bar)
                / controls.track_scroll_max;
        fill(
            hdc,
            &RECT {
                left: controls.timeline_panel.right - scale(5),
                right: controls.timeline_panel.right - scale(2),
                top: y,
                bottom: y + bar,
            },
            palette.border,
        );
    }

    state.draw_scrollbar(hdc, controls.scrollbar);
    if state.timeline.contains(display_tick) {
        let playhead_x = timeline_x(controls.camera_track, display_tick, state.timeline);
        for y in 0..scale(8) {
            let half = if y < scale(4) { scale(4) } else { (scale(8)-y).max(1) };
            fill(hdc, &RECT { left: playhead_x-half, right: playhead_x+half+1, top: controls.ruler.top+y, bottom: controls.ruler.top+y+1 }, palette.accent);
        }
        fill(
            hdc,
            &RECT {
                left: playhead_x,
                top: controls.ruler.top,
                right: playhead_x + 1,
                bottom: controls.track_area.bottom,
            },
            palette.accent,
        );
    }
}

/// Every pane boundary uses the same paint and interaction geometry.
pub(super) fn draw_splitter(hdc: DC, controls: &ControlLayout, state: &WindowState) {
    use super::editor_layout::SplitterId;
    for (id, splitter) in controls.splitters() {
        let dragging = match id {
            SplitterId::ProjectBin => state.project_bin_resize.is_some(),
            SplitterId::Inspector => state.inspector_resize.is_some(),
            SplitterId::Timeline => state.timeline_resize.is_some(),
        };
        splitter.paint(hdc, state.theme, state.splitter_hover == Some(id), dragging);
    }
}
