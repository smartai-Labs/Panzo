//! Pure DPI-aware editor layout and hit testing; no media or file I/O.
use super::{ControlId, EditorTheme, HitRect, RECT};
use crate::platform::editor_ui::{HeaderAction, HeaderLayout};

#[derive(Clone, Copy)]
pub(super) struct ControlLayout {
    pub(super) header: HeaderLayout,
    pub(super) new_recording: HitRect,
    pub(super) open_project: HitRect,
    pub(super) video_sections: [HitRect; 2],
    pub(super) crop_edit: HitRect,
    pub(super) crop_lock: HitRect,
    pub(super) canvas_presets: [HitRect; 4],
    pub(super) canvas_fields: [HitRect; 2],
    pub(super) frame_buttons: [HitRect; 2],
    pub(super) window_buttons: [HitRect; 3],
    pub(super) crop_edges: [HitRect; 4],
    pub(super) crop_reset: HitRect,
    pub(super) crop_presets: [HitRect; 4],
    pub(super) inspector_tabs: [HitRect; 2],
    pub(super) inspector_body_top: i32,
    pub(super) video_speed: HitRect,
    pub(super) speed_reset: HitRect,
    pub(super) speed_presets: [HitRect; 5],
    pub(super) inspector_content_top: i32,
    pub(super) inspector_scroll: i32,
    pub(super) inspector_scroll_max: i32,
    pub(super) inspector_scrollbar: HitRect,
    pub(super) inspector_splitter: HitRect,
    pub(super) transport: RECT,
    pub(super) context_bar: RECT,
    pub(super) track_labels: RECT,
    pub(super) preview_meta: RECT,
    pub(super) back: HitRect,
    pub(super) inspector_toggle: HitRect,
    pub(super) start_value: HitRect,
    pub(super) end_value: HitRect,
    pub(super) scale_value: HitRect,
    pub(super) center_focus: HitRect,
    pub(super) zoom_slider: HitRect,
    pub(super) swatches: [HitRect; 6],
    pub(super) background_file: HitRect,
    pub(super) background_color: HitRect,
    pub(super) cursor_properties: HitRect,
    pub(super) splitter: HitRect,
    pub(super) scrollbar: HitRect,
    pub(super) video_track: HitRect,
    pub(super) split: HitRect,
    pub(super) video_trim: HitRect,
    pub(super) camera_properties: HitRect,
    pub(super) camera_enabled: HitRect,
    pub(super) cursor: HitRect,
    pub(super) export: HitRect,
    pub(super) location: HitRect,
    pub(super) command_bar: RECT,
    pub(super) command_title: RECT,
    pub(super) preview_card: RECT,
    pub(super) preview_well: RECT,
    pub(super) panel: RECT,
    pub(super) timeline_panel: RECT,
    pub(super) timeline_header: RECT,
    pub(super) click_track: HitRect,
    pub(super) camera_track: HitRect,
    pub(super) home: HitRect,
    pub(super) play: HitRect,
    pub(super) save: HitRect,
    pub(super) undo: HitRect,
    pub(super) redo: HitRect,
    pub(super) add: HitRect,
    pub(super) delete: HitRect,
    pub(super) reset: HitRect,
    pub(super) diagnostics: HitRect,
    pub(super) timeline_zoom_out: HitRect,
    pub(super) timeline_fit: HitRect,
    pub(super) timeline_zoom_in: HitRect,
    pub(super) scale_minus: HitRect,
    pub(super) scale_plus: HitRect,
    pub(super) focus_left: HitRect,
    pub(super) focus_right: HitRect,
    pub(super) focus_up: HitRect,
    pub(super) focus_down: HitRect,
    pub(super) start_minus: HitRect,
    pub(super) start_plus: HitRect,
    pub(super) end_minus: HitRect,
    pub(super) end_plus: HitRect,
    pub(super) background: HitRect,
    pub(super) background_image: HitRect,
    pub(super) inset: HitRect,
    pub(super) radius: HitRect,
    pub(super) shadow: HitRect,
    pub(super) selected_info: RECT,
    pub(super) ruler: HitRect,
    pub(super) time: RECT,
    pub(super) status: RECT,
}

impl ControlLayout {
    #[allow(clippy::too_many_lines)]
    pub(super) fn new(client: RECT, theme: EditorTheme) -> Self {
        let s = |n| theme.scale(n);
        let w = client.right.max(s(1));
        let h = client.bottom.max(s(1));
        let toolbar = theme.metrics.toolbar_height + theme.metrics.outer_margin;
        let margin = theme.metrics.outer_margin;
        let gap = theme.metrics.gap;
        let height = Self::clamp_timeline_height(h, theme.metrics.timeline_height, theme);
        let bottom = h - margin;
        let top = bottom - height;
        let upper_bottom = top - gap;
        // Short windows scroll the inspector instead of silently removing it.
        let inspector = Self::clamp_inspector_width(w, theme.metrics.inspector_width, theme);
        let edge = w - margin - inspector;
        let preview_right = if inspector > 0 { edge - gap } else { edge };
        let r = |x, y, width, height| {
            HitRect(RECT {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            })
        };
        let empty = r(0, 0, 0, 0);
        let panel_x = edge + s(20);
        let panel_width = (inspector - s(40)).max(0);
        let prop = |y, height| {
            if inspector > 0 {
                r(panel_x, toolbar + s(y), panel_width, s(height))
            } else {
                empty
            }
        };
        let left = margin + s(72);
        let right = w - margin - s(16);
        let ruler_top = top + s(84);
        let context_top = bottom - s(26);
        let header = HeaderLayout::new(client, theme, None);
        let video_top = ruler_top + s(24);
        let track_bottom = context_top - s(24);
        let track_space = (track_bottom - video_top - s(8)).max(1);
        let video_height = track_space * 3 / 5;
        let camera_top = video_top + video_height + s(8);
        // Keep four frame/playback buttons together, then give time text only the
        // remaining space before the inspector toggle. Long minute counts must
        // never paint across that button on a narrow physical high-DPI window.
        let transport_right = preview_right - s(52);
        let transport_step = s(36);
        let button_group_width = transport_step * 3 + s(32);
        let transport_left = (margin + (transport_right - margin - s(330)) / 2).max(margin);
        let time_left = transport_left + button_group_width + s(6);
        let time_width = (transport_right - time_left).clamp(0, s(184));
        Self {
            header,
            new_recording: HitRect(header.rect(HeaderAction::NewRecording)),
            open_project: HitRect(header.rect(HeaderAction::OpenProject)),
            video_sections: [empty; 2],
            crop_edit: empty,
            crop_lock: empty,
            canvas_presets: [empty; 4],
            canvas_fields: [empty; 2],
            frame_buttons: [
                r(
                    transport_left + transport_step,
                    upper_bottom - s(40),
                    s(32),
                    s(32),
                ),
                r(
                    transport_left + transport_step * 3,
                    upper_bottom - s(40),
                    s(32),
                    s(32),
                ),
            ],
            window_buttons: [
                HeaderAction::Minimize,
                HeaderAction::Maximize,
                HeaderAction::Close,
            ]
            .map(|action| HitRect(header.rect(action))),
            crop_edges: [empty; 4],
            crop_reset: empty,
            crop_presets: [empty; 4],
            inspector_tabs: std::array::from_fn(|i| {
                if inspector > 0 {
                    let width = (panel_width - s(4)) / 2;
                    r(
                        panel_x + (width + s(4)) * i32::try_from(i).unwrap_or(0),
                        toolbar + s(12),
                        width,
                        s(32),
                    )
                } else {
                    empty
                }
            }),
            inspector_body_top: toolbar + s(58),
            video_speed: empty,
            speed_reset: empty,
            speed_presets: [empty; 5],
            inspector_content_top: toolbar,
            inspector_scroll: 0,
            inspector_splitter: if inspector > 0 {
                r(preview_right, toolbar, gap, upper_bottom - toolbar)
            } else {
                empty
            },
            inspector_scroll_max: (s(536) - (upper_bottom - toolbar)).max(0),
            inspector_scrollbar: if inspector > 0 {
                r(
                    w - margin - s(12),
                    toolbar + s(60),
                    s(10),
                    (upper_bottom - toolbar - s(68)).max(0),
                )
            } else {
                empty
            },
            transport: r(margin, upper_bottom - s(48), preview_right - margin, s(48)).0,
            context_bar: r(margin, top + s(44), w - margin * 2, s(40)).0,
            track_labels: r(margin + s(12), ruler_top, s(52), context_top - ruler_top).0,
            preview_meta: r(
                margin + s(16),
                toolbar + s(8),
                (preview_right - margin - s(32)).max(0),
                s(24),
            )
            .0,
            back: empty,
            inspector_toggle: r(preview_right - s(44), upper_bottom - s(40), s(32), s(32)),
            start_value: empty,
            end_value: empty,
            scale_value: empty,
            center_focus: empty,
            zoom_slider: r(w - margin - s(154), top + s(6), s(64), s(32)),
            background_file: empty,
            background_color: prop(176, 32),
            cursor_properties: prop(506, 32),
            swatches: std::array::from_fn(|i| {
                if inspector > 0 {
                    let cell = ((panel_width - s(40)) / 6).min(s(36));
                    let gap = (panel_width - cell * 6) / 5;
                    r(
                        panel_x + (cell + gap) * i32::try_from(i).unwrap_or(0),
                        toolbar + s(126),
                        cell,
                        cell,
                    )
                } else {
                    empty
                }
            }),
            splitter: r(margin, upper_bottom, w - margin * 2, gap),
            scrollbar: r(left, context_top - s(16), right - left, s(12)),
            video_track: r(left, video_top, right - left, video_height),
            split: r(s(108), top + s(6), s(32), s(32)),
            video_trim: empty,
            camera_properties: empty,
            camera_enabled: r(s(236), top + s(6), s(32), s(32)),
            cursor: prop(470, 32),
            export: HitRect(header.rect(HeaderAction::Export)),
            location: empty,
            command_bar: header.bar,
            command_title: header.title,
            preview_card: r(
                margin,
                toolbar,
                preview_right - margin,
                upper_bottom - toolbar,
            )
            .0,
            preview_well: r(
                margin,
                toolbar,
                preview_right - margin,
                (upper_bottom - toolbar - s(48)).max(1),
            )
            .0,
            panel: r(edge, toolbar, inspector, upper_bottom - toolbar).0,
            timeline_panel: r(margin, top, w - margin * 2, height).0,
            timeline_header: r(margin, top, w - margin * 2, s(44)).0,
            click_track: r(left, video_top + video_height + s(2), right - left, s(3)),
            camera_track: r(left, camera_top, right - left, track_space - video_height),
            home: r(transport_left, upper_bottom - s(40), s(32), s(32)),
            play: r(
                transport_left + transport_step * 2,
                upper_bottom - s(40),
                s(32),
                s(32),
            ),
            save: HitRect(header.rect(HeaderAction::Save)),
            undo: r(s(18), top + s(6), s(32), s(32)),
            redo: r(s(58), top + s(6), s(32), s(32)),
            add: r(s(148), top + s(6), s(32), s(32)),
            delete: r(s(188), top + s(6), s(32), s(32)),
            reset: r(s(276), top + s(6), s(32), s(32)),
            diagnostics: HitRect(header.rect(HeaderAction::Settings)),
            timeline_zoom_out: r(w - margin - s(194), top + s(6), s(32), s(32)),
            timeline_zoom_in: r(w - margin - s(82), top + s(6), s(32), s(32)),
            timeline_fit: r(w - margin - s(42), top + s(6), s(32), s(32)),
            scale_minus: empty,
            scale_plus: empty,
            focus_left: empty,
            focus_right: empty,
            focus_up: empty,
            focus_down: empty,
            start_minus: empty,
            start_plus: empty,
            end_minus: empty,
            end_plus: empty,
            background: if inspector > 0 {
                r(panel_x + panel_width - s(72), toolbar + s(70), s(32), s(32))
            } else {
                empty
            },
            background_image: if inspector > 0 {
                r(panel_x + panel_width - s(32), toolbar + s(70), s(32), s(32))
            } else {
                empty
            },
            inset: prop(268, 52),
            radius: prop(326, 52),
            shadow: prop(384, 52),
            selected_info: RECT::default(),
            ruler: r(left, ruler_top, right - left, s(24)),
            time: r(time_left, upper_bottom - s(48), time_width, s(48)).0,
            status: r(
                margin + s(16),
                bottom - s(26),
                w - margin * 2 - s(32),
                s(26),
            )
            .0,
        }
    }

    pub(super) fn with_caption_bounds(mut self, end: Option<RECT>, theme: EditorTheme) -> Self {
        if let Some(end) = end {
            self.header = HeaderLayout::new(self.command_bar, theme, Some(end));
            self.window_buttons = [
                HeaderAction::Minimize,
                HeaderAction::Maximize,
                HeaderAction::Close,
            ]
            .map(|action| HitRect(self.header.rect(action)));
            self.export = HitRect(self.header.rect(HeaderAction::Export));
            self.save = HitRect(self.header.rect(HeaderAction::Save));
            self.command_title = self.header.title;
            self.diagnostics = HitRect(self.header.rect(HeaderAction::Settings));
        }
        self
    }

    pub(super) fn clamp_timeline_height(height: i32, requested: i32, theme: EditorTheme) -> i32 {
        // Match the compact reference layout while keeping controls usable on small screens.
        let minimum = theme.scale(216);
        let maximum = (height * 22 / 100).max(theme.scale(244));
        requested.clamp(minimum, maximum)
    }

    pub(super) fn clamp_inspector_width(width: i32, requested: i32, theme: EditorTheme) -> i32 {
        if requested == 0 {
            return 0;
        }
        let minimum = theme.scale(240);
        let maximum =
            (width - theme.metrics.outer_margin * 2 - theme.metrics.gap - theme.scale(380))
                .clamp(minimum, theme.scale(480));
        requested.clamp(minimum, maximum)
    }

    pub(super) fn with_inspector(
        mut self,
        camera: bool,
        video: bool,
        scroll: i32,
        theme: EditorTheme,
    ) -> Self {
        let s = |n| theme.scale(n);
        // Selection belongs to the timeline and remains usable with the inspector hidden.
        if camera || video {
            let y = self.context_bar.top + s(4);
            self.selected_info = RECT {
                left: s(18),
                right: s(100),
                top: y,
                bottom: y + s(32),
            };
            let field = |left, width| {
                HitRect(RECT {
                    left: s(left),
                    right: s(left + width),
                    top: y,
                    bottom: y + s(32),
                })
            };
            self.start_value = field(142, 78);
            self.end_value = field(268, 78);
            if camera {
                self.scale_value = field(394, 62);
                self.center_focus = field(472, 32);
                self.camera_properties = field(520, 32);
            } else {
                self.video_trim = field(362, 32);
            }
        }
        if self.panel.right <= self.panel.left {
            return self;
        }
        self.inspector_scroll_max = (s(720) - (self.panel.bottom - self.panel.top)).max(0);
        self.inspector_scroll = scroll.clamp(0, self.inspector_scroll_max);
        self.inspector_content_top = self.panel.top - self.inspector_scroll;
        let x = self.panel.left + s(20);
        let width = self.panel.right - s(20) - x;
        self.canvas_presets = std::array::from_fn(|i| {
            let step = (width + s(4)) / 4;
            let left = x + step * i32::try_from(i).unwrap_or(0);
            HitRect(RECT {
                left,
                right: left + step - s(4),
                top: self.inspector_content_top + s(608),
                bottom: self.inspector_content_top + s(640),
            })
        });
        self.canvas_fields = std::array::from_fn(|i| {
            let left = x + i32::midpoint(width, s(12)) * i32::try_from(i).unwrap_or(0);
            HitRect(RECT {
                left,
                right: left + (width - s(12)) / 2,
                top: self.inspector_content_top + s(650),
                bottom: self.inspector_content_top + s(698),
            })
        });

        for rect in [
            &mut self.background,
            &mut self.background_image,
            &mut self.background_file,
            &mut self.background_color,
            &mut self.cursor_properties,
            &mut self.inset,
            &mut self.radius,
            &mut self.shadow,
            &mut self.cursor,
        ] {
            if rect.right > rect.left {
                rect.0.top -= self.inspector_scroll;
                rect.0.bottom -= self.inspector_scroll;
            }
        }
        for rect in &mut self.swatches {
            rect.0.top -= self.inspector_scroll;
            rect.0.bottom -= self.inspector_scroll;
        }
        self
    }

    #[cfg(test)]
    pub(super) fn with_video_tab(self, video: bool, scroll: i32, theme: EditorTheme) -> Self {
        self.with_video_groups(video, scroll, theme, [false; 2])
    }

    pub(super) fn with_video_groups(
        mut self,
        video: bool,
        scroll: i32,
        theme: EditorTheme,
        collapsed: [bool; 2],
    ) -> Self {
        if !video || self.panel.right <= self.panel.left {
            return self;
        }
        let empty = HitRect(RECT::default());
        for rect in [
            &mut self.background,
            &mut self.background_image,
            &mut self.background_file,
            &mut self.background_color,
            &mut self.cursor_properties,
            &mut self.inset,
            &mut self.radius,
            &mut self.shadow,
            &mut self.cursor,
        ] {
            *rect = empty;
        }
        self.swatches = [empty; 6];
        self.canvas_presets = [empty; 4];
        self.canvas_fields = [empty; 2];
        let s = |n| theme.scale(n);
        let crop_top = if collapsed[0] { 106 } else { 324 };
        let height = crop_top + if collapsed[1] { 48 } else { 312 };
        self.inspector_scroll_max = (s(height) - (self.panel.bottom - self.panel.top)).max(0);
        self.inspector_scroll = scroll.clamp(0, self.inspector_scroll_max);
        self.inspector_content_top = self.panel.top - self.inspector_scroll;
        let left = self.panel.left + s(20);
        let right = self.panel.right - s(20);
        let row = |top, height| {
            HitRect(RECT {
                left,
                right,
                top: self.inspector_content_top + s(top),
                bottom: self.inspector_content_top + s(top + height),
            })
        };
        self.video_sections = [
            HitRect(RECT {
                right: right - s(40),
                ..row(66, 32).0
            }),
            HitRect(RECT {
                right: right - s(112),
                ..row(crop_top, 32).0
            }),
        ];
        self.speed_reset = HitRect(RECT {
            left: right - s(32),
            ..row(66, 32).0
        });
        if !collapsed[0] {
            self.video_speed = row(112, 52);
            let width = (right - left - s(16)) / 5;
            self.speed_presets = std::array::from_fn(|i| {
                let x = left + (width + s(4)) * i32::try_from(i).unwrap_or(0);
                HitRect(RECT {
                    left: x,
                    right: x + width,
                    ..row(180, 32).0
                })
            });
        }
        self.crop_reset = HitRect(RECT {
            left: right - s(32),
            ..row(crop_top, 32).0
        });
        self.crop_lock = HitRect(RECT {
            left: right - s(68),
            right: right - s(36),
            ..row(crop_top, 32).0
        });
        self.crop_edit = HitRect(RECT {
            left: right - s(104),
            right: right - s(72),
            ..row(crop_top, 32).0
        });
        if !collapsed[1] {
            self.crop_presets = std::array::from_fn(|i| {
                let width = (right - left - s(12)) / 4;
                let x = left + (width + s(4)) * i32::try_from(i).unwrap_or(0);
                HitRect(RECT {
                    left: x,
                    right: x + width,
                    ..row(crop_top + 42, 32).0
                })
            });
            self.crop_edges = std::array::from_fn(|i| {
                row(crop_top + 86 + i32::try_from(i).unwrap_or(0) * 52, 48)
            });
        }
        self
    }

    pub(super) fn is_inspector_control(id: ControlId) -> bool {
        matches!(
            id,
            ControlId::SpeedSection
                | ControlId::CropSection
                | ControlId::CropEdit
                | ControlId::CropLock
                | ControlId::CanvasOriginal
                | ControlId::CanvasWide
                | ControlId::CanvasPortrait
                | ControlId::CanvasSquare
                | ControlId::CanvasWidth
                | ControlId::CanvasHeight
                | ControlId::VideoSpeed
                | ControlId::CropLeft
                | ControlId::CropTop
                | ControlId::CropRight
                | ControlId::CropBottom
                | ControlId::CropReset
                | ControlId::CropWide
                | ControlId::CropStandard
                | ControlId::CropSquare
                | ControlId::CropPortrait
                | ControlId::SpeedReset
                | ControlId::SpeedHalf
                | ControlId::SpeedNormal
                | ControlId::SpeedOneHalf
                | ControlId::SpeedDouble
                | ControlId::SpeedQuadruple
                | ControlId::Background
                | ControlId::BackgroundImage
                | ControlId::BackgroundFile
                | ControlId::BackgroundColor
                | ControlId::CursorProperties
                | ControlId::Inset
                | ControlId::Radius
                | ControlId::Shadow
                | ControlId::Cursor
                | ControlId::SwatchMist
                | ControlId::SwatchPeach
                | ControlId::SwatchLilac
                | ControlId::SwatchCharcoal
                | ControlId::SwatchWhite
                | ControlId::SwatchSage
        )
    }

    pub(super) fn with_background_tab(mut self, image: bool) -> Self {
        if image && self.panel.right > self.panel.left {
            self.background_color = HitRect(RECT::default());
            self.background_file = HitRect(RECT {
                left: self.swatches[0].left,
                right: self.background_image.right,
                top: self.background_image.bottom
                    + (self.background.bottom - self.background.top) / 4,
                bottom: self.background_image.bottom
                    + (self.background.bottom - self.background.top) * 3,
            });
            self.swatches = [HitRect(RECT::default()); 6];
        }
        self
    }

    pub(super) fn timeline_area(self) -> HitRect {
        HitRect(RECT {
            left: self.camera_track.left,
            top: self.ruler.top,
            right: self.camera_track.right,
            bottom: self.camera_track.bottom,
        })
    }

    pub(super) fn hit_control(self, x: i32, y: i32) -> Option<ControlId> {
        ControlId::ORDER
            .into_iter()
            .find(|id| self.control_rect(*id).contains(x, y))
    }

    pub(super) fn control_rect(self, id: ControlId) -> HitRect {
        let mut rect = self.raw_control_rect(id);
        if Self::is_inspector_control(id) {
            rect.0.top = rect.top.max(self.inspector_body_top);
            rect.0.bottom = rect.bottom.min(self.panel.bottom);
            if rect.bottom <= rect.top {
                return HitRect(RECT::default());
            }
        }
        rect
    }

    pub(super) fn raw_control_rect(self, id: ControlId) -> HitRect {
        match id {
            ControlId::Menu => HitRect(self.header.rect(HeaderAction::Menu)),
            ControlId::NewRecording => self.new_recording,
            ControlId::OpenProject => self.open_project,
            ControlId::CanvasTab => self.inspector_tabs[0],
            ControlId::VideoTab => self.inspector_tabs[1],
            ControlId::SpeedSection => self.video_sections[0],
            ControlId::CropSection => self.video_sections[1],
            ControlId::CropEdit => self.crop_edit,
            ControlId::CropLock => self.crop_lock,
            ControlId::CanvasOriginal => self.canvas_presets[0],
            ControlId::CanvasWide => self.canvas_presets[1],
            ControlId::CanvasPortrait => self.canvas_presets[2],
            ControlId::CanvasSquare => self.canvas_presets[3],
            ControlId::CanvasWidth => self.canvas_fields[0],
            ControlId::CanvasHeight => self.canvas_fields[1],
            ControlId::PreviousFrame => self.frame_buttons[0],
            ControlId::NextFrame => self.frame_buttons[1],
            ControlId::WindowMinimize => self.window_buttons[0],
            ControlId::WindowMaximize => self.window_buttons[1],
            ControlId::WindowClose => self.window_buttons[2],
            ControlId::CropLeft => self.crop_edges[0],
            ControlId::CropTop => self.crop_edges[1],
            ControlId::CropRight => self.crop_edges[2],
            ControlId::CropBottom => self.crop_edges[3],
            ControlId::CropReset => self.crop_reset,
            ControlId::CropWide => self.crop_presets[0],
            ControlId::CropStandard => self.crop_presets[1],
            ControlId::CropSquare => self.crop_presets[2],
            ControlId::CropPortrait => self.crop_presets[3],
            ControlId::VideoSpeed => self.video_speed,
            ControlId::SpeedReset => self.speed_reset,
            ControlId::SpeedHalf => self.speed_presets[0],
            ControlId::SpeedNormal => self.speed_presets[1],
            ControlId::SpeedOneHalf => self.speed_presets[2],
            ControlId::SpeedDouble => self.speed_presets[3],
            ControlId::SpeedQuadruple => self.speed_presets[4],
            ControlId::BackgroundFile => self.background_file,
            ControlId::BackgroundColor => self.background_color,
            ControlId::CursorProperties => self.cursor_properties,
            ControlId::Back => self.back,
            ControlId::InspectorToggle => self.inspector_toggle,
            ControlId::StartValue => self.start_value,
            ControlId::EndValue => self.end_value,
            ControlId::ScaleValue => self.scale_value,
            ControlId::CenterFocus => self.center_focus,
            ControlId::ZoomSlider => self.zoom_slider,
            ControlId::SwatchMist => self.swatches[0],
            ControlId::SwatchPeach => self.swatches[1],
            ControlId::SwatchLilac => self.swatches[2],
            ControlId::SwatchCharcoal => self.swatches[3],
            ControlId::SwatchWhite => self.swatches[4],
            ControlId::SwatchSage => self.swatches[5],
            ControlId::Split => self.split,
            ControlId::VideoTrim => self.video_trim,
            ControlId::CameraProperties => self.camera_properties,
            ControlId::CameraEnabled => self.camera_enabled,
            ControlId::Cursor => self.cursor,
            ControlId::Export => self.export,
            ControlId::Location => self.location,
            ControlId::Home => self.home,
            ControlId::Play => self.play,
            ControlId::Save => self.save,
            ControlId::Undo => self.undo,
            ControlId::Redo => self.redo,
            ControlId::Add => self.add,
            ControlId::Delete => self.delete,
            ControlId::Reset => self.reset,
            ControlId::Diagnostics => self.diagnostics,
            ControlId::TimelineZoomOut => self.timeline_zoom_out,
            ControlId::TimelineFit => self.timeline_fit,
            ControlId::TimelineZoomIn => self.timeline_zoom_in,
            ControlId::ScaleMinus => self.scale_minus,
            ControlId::ScalePlus => self.scale_plus,
            ControlId::FocusLeft => self.focus_left,
            ControlId::FocusRight => self.focus_right,
            ControlId::FocusUp => self.focus_up,
            ControlId::FocusDown => self.focus_down,
            ControlId::StartMinus => self.start_minus,
            ControlId::StartPlus => self.start_plus,
            ControlId::EndMinus => self.end_minus,
            ControlId::EndPlus => self.end_plus,
            ControlId::Background => self.background,
            ControlId::BackgroundImage => self.background_image,
            ControlId::Inset => self.inset,
            ControlId::Radius => self.radius,
            ControlId::Shadow => self.shadow,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_timecodes_fit_the_real_font_without_covering_transport_buttons() {
        use crate::platform::editor_ui::{DrawingScale, TextStyle, measure_text_width};
        use windows::Win32::Graphics::Gdi::{CreateCompatibleDC, DeleteDC};
        let dc = unsafe { CreateCompatibleDC(None) };
        assert!(!dc.0.is_null());
        for dpi in [96, 120, 144, 192] {
            let mut theme = EditorTheme::for_dpi(dpi);
            let _scale = DrawingScale::enter(dpi);
            for (width, height) in [(1280, 900), (1920, 1080)] {
                for inspector in [0, theme.scale(288), theme.scale(480)] {
                    theme.metrics.inspector_width = inspector;
                    let layout = ControlLayout::new(
                        RECT {
                            right: width,
                            bottom: height,
                            ..RECT::default()
                        },
                        theme,
                    );
                    assert!(layout.frame_buttons[1].right < layout.time.left);
                    assert!(layout.time.right < layout.inspector_toggle.left);
                    for label in [
                        "00:00.000  /  30:00.000",
                        "120:00.000  /  1440:00.000",
                        "1440:00.000  /  1440:00.000",
                    ] {
                        assert!(
                            measure_text_width(dc, label, TextStyle::Caption)
                                <= layout.time.right - layout.time.left,
                            "{width}x{height}, dpi={dpi}, inspector={inspector}: {label}"
                        );
                    }
                }
            }
        }
        unsafe {
            let _ = DeleteDC(dc);
        }
    }
    #[test]
    fn partially_visible_property_rows_keep_their_visible_hit_area() {
        for dpi in [96, 144, 192] {
            let theme = EditorTheme::for_dpi(dpi);
            let client = RECT {
                right: theme.scale(1100),
                bottom: theme.scale(680),
                ..RECT::default()
            };
            let mut partial = 0;
            for scroll in (0..theme.scale(600)).step_by(7) {
                let layout = ControlLayout::new(client, theme)
                    .with_inspector(false, true, scroll, theme)
                    .with_video_tab(true, scroll, theme);
                for id in [
                    ControlId::VideoSpeed,
                    ControlId::CropLeft,
                    ControlId::CropTop,
                    ControlId::CropRight,
                    ControlId::CropBottom,
                ] {
                    let raw = layout.raw_control_rect(id);
                    let hit = layout.control_rect(id);
                    if hit.bottom > hit.top && (raw.top != hit.top || raw.bottom != hit.bottom) {
                        partial += 1;
                        assert_eq!(
                            layout.hit_control(hit.right - 2, i32::midpoint(hit.top, hit.bottom)),
                            Some(id)
                        );
                    }
                }
            }
            assert!(
                partial > 0,
                "DPI {dpi}: no partially visible rows exercised"
            );
        }
    }
    #[test]
    fn video_tabs_keep_header_fixed_and_hide_canvas_hit_targets() {
        for dpi in [96, 120, 144, 192] {
            let theme = EditorTheme::for_dpi(dpi);
            let client = RECT {
                right: theme.scale(1100),
                bottom: theme.scale(680),
                ..RECT::default()
            };
            for scroll in [0, 10000] {
                let layout = ControlLayout::new(client, theme)
                    .with_inspector(false, true, scroll, theme)
                    .with_video_tab(true, scroll, theme);
                assert_eq!(layout.control_rect(ControlId::Inset).right, 0);
                assert_eq!(layout.control_rect(ControlId::BackgroundFile).right, 0);
                let tab = layout.inspector_tabs[1];
                assert_eq!(
                    layout.hit_control(tab.left + 1, tab.top + 1),
                    Some(ControlId::VideoTab)
                );
                assert!(tab.bottom < layout.inspector_body_top);
                let speed = layout.control_rect(ControlId::VideoSpeed);
                if speed.right > speed.left {
                    assert_eq!(
                        layout.hit_control(speed.right - 1, speed.top + 1),
                        Some(ControlId::VideoSpeed)
                    );
                } else {
                    assert!(scroll > 0);
                }
                if scroll > 0 {
                    let crop = layout.control_rect(ControlId::CropBottom);
                    assert!(crop.right > crop.left);
                    assert_eq!(
                        layout.hit_control(crop.right - 1, crop.bottom - 1),
                        Some(ControlId::CropBottom)
                    );
                }
                for pair in layout.speed_presets.windows(2) {
                    assert!(pair[0].right < pair[1].left);
                }
                assert!(layout.speed_presets[4].right < layout.panel.right);
            }
        }
    }
    #[test]
    fn timeline_maximum_matches_reference_and_reclamps_after_window_resize() {
        for dpi in [96, 120, 144, 192] {
            let mut theme = EditorTheme::for_dpi(dpi);
            theme.metrics.timeline_height = theme.scale(2000);
            for height in [680, 900, 1080, 1417, 2160] {
                let client = RECT {
                    left: 0,
                    top: 0,
                    right: theme.scale(2560),
                    bottom: theme.scale(height),
                };
                let layout = ControlLayout::new(client, theme);
                let expected = (client.bottom * 22 / 100).max(theme.scale(244));
                assert_eq!(
                    layout.timeline_panel.bottom - layout.timeline_panel.top,
                    expected
                );
                assert_eq!(
                    ControlLayout::clamp_timeline_height(client.bottom, i32::MAX, theme),
                    expected
                );
                assert_eq!(
                    ControlLayout::clamp_timeline_height(client.bottom, 0, theme),
                    theme.scale(216)
                );
                assert_eq!(
                    layout.scrollbar.top - layout.camera_track.bottom,
                    theme.scale(8)
                );
            }
        }
    }

    #[test]
    fn taller_timeline_expands_both_tracks_and_keeps_the_bottom_gap_constant() {
        for dpi in [96, 120, 144, 192] {
            let mut theme = EditorTheme::for_dpi(dpi);
            let client = RECT {
                left: 0,
                top: 0,
                right: theme.scale(1440),
                bottom: theme.scale(1417),
            };
            let short = ControlLayout::new(client, theme);
            theme.metrics.timeline_height = theme.scale(440);
            let tall = ControlLayout::new(client, theme);
            assert!(
                tall.video_track.bottom - tall.video_track.top
                    > short.video_track.bottom - short.video_track.top
            );
            assert!(
                tall.camera_track.bottom - tall.camera_track.top
                    > short.camera_track.bottom - short.camera_track.top
            );
            assert_eq!(
                tall.scrollbar.top - tall.camera_track.bottom,
                theme.scale(8)
            );
            assert_eq!(
                short.scrollbar.top - short.camera_track.bottom,
                theme.scale(8)
            );
            for id in [ControlId::Undo, ControlId::Redo] {
                let r = tall.control_rect(id);
                assert!(
                    r.top >= tall.timeline_header.top && r.bottom <= tall.timeline_header.bottom
                );
                assert!(r.bottom < tall.ruler.top);
            }
        }
    }

    #[test]
    fn inspector_width_changes_layout_without_overlapping_properties_or_transport() {
        for dpi in [96, 120, 144, 192] {
            let mut theme = EditorTheme::for_dpi(dpi);
            let client = RECT {
                left: 0,
                top: 0,
                right: theme.scale(1000),
                bottom: theme.scale(900),
            };
            for width in [200, 240, 264, 360, 480, 700] {
                theme.metrics.inspector_width = theme.scale(width);
                let c = ControlLayout::new(client, theme).with_inspector(true, false, 0, theme);
                assert_eq!(
                    c.panel.right - c.panel.left,
                    theme.scale(width.clamp(240, 480))
                );
                assert!(c.swatches[5].right <= c.panel.right - theme.scale(20));
                assert!(c.swatches[0].bottom < c.inset.top);
                assert!(c.time.right < c.inspector_toggle.left);
                assert!(
                    c.inspector_splitter
                        .contains(c.panel.left - 1, c.panel.top + 10)
                );
            }
            theme.metrics.inspector_width = 0;
            let c = ControlLayout::new(client, theme);
            assert_eq!(c.inspector_splitter.right, c.inspector_splitter.left);
        }
    }
    #[test]
    fn editor_has_separate_transport_and_timeline_context() {
        let layout = ControlLayout::new(
            RECT {
                left: 0,
                top: 0,
                right: 1440,
                bottom: 900,
            },
            EditorTheme::for_dpi(96),
        );
        assert_eq!(layout.command_bar.bottom, 32);
        assert_eq!(layout.preview_card.top, 40);
        assert_eq!(layout.panel.right - layout.panel.left, 288);
        assert_eq!(
            layout.timeline_panel.bottom - layout.timeline_panel.top,
            244
        );
        assert_eq!(layout.transport.bottom - layout.transport.top, 48);
        assert_eq!(layout.preview_well.bottom, layout.transport.top);
        assert_eq!(
            layout.transport.bottom + EditorTheme::light().metrics.gap,
            layout.timeline_panel.top
        );
        assert_eq!(layout.start_value.right, layout.start_value.left);
        assert_eq!(layout.scale_value.right, layout.scale_value.left);
        assert!(layout.time.bottom <= layout.timeline_panel.top);
    }
    #[test]
    fn short_physical_windows_keep_all_properties_reachable_at_high_dpi() {
        for dpi in [96, 120, 144, 192] {
            let theme = EditorTheme::for_dpi(dpi);
            for (width, height) in [(1280, 900), (1920, 1080)] {
                let client = RECT {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                };
                for (camera, video) in [(false, false), (true, false), (false, true)] {
                    let mut reachable = std::collections::HashSet::new();
                    let base =
                        ControlLayout::new(client, theme).with_inspector(camera, video, 0, theme);
                    assert!(base.panel.right > base.panel.left);
                    for scroll in (0..=base.inspector_scroll_max + theme.scale(10)).step_by(8) {
                        let layout = ControlLayout::new(client, theme)
                            .with_inspector(camera, video, scroll, theme);
                        let visible: Vec<_> = ControlId::ORDER
                            .into_iter()
                            .filter_map(|id| {
                                let rect = layout.control_rect(id);
                                (rect.right > rect.left && rect.bottom > rect.top)
                                    .then_some((id, rect))
                            })
                            .collect();
                        for (index, (id, rect)) in visible.iter().enumerate() {
                            reachable.insert(*id as u32);
                            assert!(
                                rect.left >= 0
                                    && rect.top >= 0
                                    && rect.right <= width
                                    && rect.bottom <= height,
                                "{dpi}: {id:?}"
                            );
                            for (other, candidate) in visible.iter().skip(index + 1) {
                                assert!(
                                    !(rect.left < candidate.right
                                        && candidate.left < rect.right
                                        && rect.top < candidate.bottom
                                        && candidate.top < rect.bottom),
                                    "{dpi}: {id:?} overlaps {other:?}"
                                );
                            }
                        }
                    }
                    for id in [
                        ControlId::Background,
                        ControlId::BackgroundColor,
                        ControlId::CursorProperties,
                        ControlId::Inset,
                        ControlId::Radius,
                        ControlId::Shadow,
                        ControlId::Cursor,
                    ] {
                        assert!(
                            reachable.contains(&(id as u32)),
                            "{dpi}: {id:?} unreachable"
                        );
                    }
                    assert_eq!(
                        reachable.contains(&(ControlId::StartValue as u32)),
                        camera || video
                    );
                    assert_eq!(reachable.contains(&(ControlId::ScaleValue as u32)), camera);
                    assert_eq!(
                        reachable.contains(&(ControlId::CameraProperties as u32)),
                        camera
                    );
                    assert_eq!(reachable.contains(&(ControlId::VideoTrim as u32)), video);
                    assert!(reachable.contains(&(ControlId::Reset as u32)));
                }
            }
        }
    }
    #[test]
    fn background_tabs_never_leave_invisible_swatch_hit_targets() {
        for dpi in [96, 120, 144, 192] {
            let theme = EditorTheme::for_dpi(dpi);
            let preset = ControlLayout::new(
                RECT {
                    left: 0,
                    top: 0,
                    right: theme.scale(1440),
                    bottom: theme.scale(900),
                },
                theme,
            );
            let image = preset.with_background_tab(true);
            assert_eq!(
                image.control_rect(ControlId::BackgroundColor).0,
                RECT::default()
            );
            assert_eq!(preset.background_file.right, preset.background_file.left);
            for swatch in image.swatches {
                assert_eq!(swatch.right, swatch.left);
            }
            let file = image.background_file;
            assert_eq!(
                image.hit_control(
                    i32::midpoint(file.left, file.right),
                    i32::midpoint(file.top, file.bottom)
                ),
                Some(ControlId::BackgroundFile)
            );
            assert!(file.bottom < image.inset.top);
        }
    }
    #[test]
    fn visible_controls_fit_and_never_overlap_across_dpi_and_collapsed_layouts() {
        for dpi in [96, 120, 144, 192] {
            for (width, height) in [(1024, 720), (1440, 900), (1280, 650), (820, 600)] {
                for timeline_height in [252, 304, 440] {
                    let mut theme = EditorTheme::for_dpi(dpi);
                    theme.metrics.timeline_height = theme.scale(timeline_height);
                    let client = RECT {
                        left: 0,
                        top: 0,
                        right: theme.scale(width),
                        bottom: theme.scale(height),
                    };
                    let layout = ControlLayout::new(client, theme);
                    let visible: Vec<_> = ControlId::ORDER
                        .iter()
                        .map(|id| (*id, layout.control_rect(*id)))
                        .filter(|(_, rect)| rect.right > rect.left && rect.bottom > rect.top)
                        .collect();
                    for (i, (id, rect)) in visible.iter().enumerate() {
                        assert!(
                            rect.left >= 0
                                && rect.top >= 0
                                && rect.right <= client.right
                                && rect.bottom <= client.bottom,
                            "{dpi} {width}x{height} {timeline_height}: {id:?} outside client"
                        );
                        for (other, candidate) in visible.iter().skip(i + 1) {
                            let overlap = rect.left < candidate.right
                                && candidate.left < rect.right
                                && rect.top < candidate.bottom
                                && candidate.top < rect.bottom;
                            assert!(
                                !overlap,
                                "{dpi} {width}x{height} {timeline_height}: {id:?} overlaps {other:?}"
                            );
                        }
                    }
                    assert!(layout.ruler.bottom <= layout.video_track.top);
                    assert!(layout.video_track.bottom <= layout.click_track.top);
                    assert!(layout.click_track.bottom <= layout.camera_track.top);
                    assert!(layout.camera_track.bottom <= layout.scrollbar.top);
                    assert!(layout.context_bar.bottom <= layout.ruler.top);
                    assert!(layout.scrollbar.bottom <= layout.status.top);
                    assert!(layout.status.right < layout.timeline_panel.right);
                    if layout.panel.right > layout.panel.left {
                        let visible = layout.control_rect(ControlId::Cursor);
                        assert!(visible.bottom <= layout.panel.bottom);
                    }
                }
            }
        }
    }
}
#[test]
fn selection_parameters_stay_above_tracks_with_inspector_hidden() {
    for dpi in [96, 120, 144, 192] {
        for width in [640, 820, 1440] {
            let mut theme = EditorTheme::for_dpi(dpi);
            theme.metrics.inspector_width = 0;
            let client = RECT {
                right: theme.scale(width),
                bottom: theme.scale(800),
                ..RECT::default()
            };
            for camera in [false, true] {
                let c =
                    ControlLayout::new(client, theme).with_inspector(camera, !camera, 9999, theme);
                for id in [
                    ControlId::StartValue,
                    ControlId::EndValue,
                    ControlId::ScaleValue,
                    ControlId::CenterFocus,
                ] {
                    let rect = c.control_rect(id);
                    if !camera && matches!(id, ControlId::ScaleValue | ControlId::CenterFocus) {
                        continue;
                    }
                    assert!(rect.right > rect.left);
                    assert!(rect.top >= c.context_bar.top && rect.bottom <= c.ruler.top);
                    assert!(rect.right <= client.right);
                    assert_eq!(
                        c.hit_control(
                            i32::midpoint(rect.left, rect.right),
                            i32::midpoint(rect.top, rect.bottom)
                        ),
                        Some(id)
                    );
                }
            }
        }
    }
}
