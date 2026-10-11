//! Pure DPI-aware editor layout and hit testing; no media or file I/O.
use super::{ControlId, EditorTheme, HitRect, RECT};
use crate::platform::editor_ui::{HeaderAction, HeaderLayout};
use crate::platform::{pane_splitter::{Splitter, Axis, Side}, workbench_layout::PANEL_HEADER};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SplitterId { ProjectBin, Inspector, Timeline }


#[derive(Clone, Copy)]
pub(super) struct ControlLayout {
    pub(super) header: HeaderLayout,
    pub(super) new_recording: HitRect,
    pub(super) open_project: HitRect,
    pub(super) video_sections: [HitRect; 2],
    pub(super) crop_edit: HitRect,
    pub(super) crop_lock: HitRect,
    pub(super) resolution: HitRect,
    pub(super) follow_zoom: HitRect,
    pub(super) canvas_presets: [HitRect; 4],
    pub(super) canvas_fields: [HitRect; 2],
    pub(super) frame_buttons: [HitRect; 2],
    pub(super) window_buttons: [HitRect; 3],
    pub(super) crop_edges: [HitRect; 4],
    pub(super) crop_reset: HitRect,
    pub(super) crop_presets: [HitRect; 4],
    pub(super) inspector_tabs: [HitRect; 4],
    pub(super) inspector_body_top: i32,
    pub(super) video_speed: HitRect,
    pub(super) video_scale: HitRect,
    pub(super) speed_reset: HitRect,
    pub(super) speed_presets: [HitRect; 5],
    pub(super) inspector_content_top: i32,
    pub(super) inspector_scroll: i32,
    pub(super) inspector_scroll_max: i32,
    pub(super) inspector_scrollbar: HitRect,
    pub(super) inspector_splitter: HitRect,
    pub(super) external_inspector: bool,
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
    pub(super) track_area: HitRect,
    pub(super) track_scroll_max: i32,
    pub(super) project_bin: RECT,
    pub(super) project_bin_splitter: HitRect,
    canvas_advanced: HitRect,
    pub(super) video_track: HitRect,
    pub(super) audio_panel: HitRect,
    pub(super) subtitle_panel: HitRect,
    pub(super) preview_fit: HitRect,
    pub(super) preview_mute: HitRect,
    pub(super) last: HitRect,
    pub(super) media_tracks: [HitRect; 35],
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
    pub(super) preview_quality: HitRect,
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
    pub(super) fn splitters(&self) -> [(SplitterId, Splitter); 3] {
        [(SplitterId::ProjectBin, Splitter::new(self.project_bin_splitter.0, Axis::Vertical, Side::Before)),
         (SplitterId::Inspector, Splitter::new(self.inspector_splitter.0, Axis::Vertical, Side::After)),
         (SplitterId::Timeline, Splitter::new(self.splitter.0, Axis::Horizontal, Side::After))]
    }
    pub(super) fn splitter_at(&self,x:i32,y:i32) -> Option<(SplitterId,Splitter)> {
        self.splitters().into_iter().find(|(_,s)|s.contains(x,y))
    }

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
        let panel_x = edge + s(16);
        let panel_width = (inspector - s(32)).max(0);
        let prop = |y, height| {
            if inspector > 0 {
                r(panel_x, toolbar + s(PANEL_HEADER + y), panel_width, s(height))
            } else {
                empty
            }
        };
        let left = margin + s(112);
        let right = w - margin - s(12);
        let ruler_top = top + s(36);
        let context_top = bottom;
        let header = HeaderLayout::new(client, theme, None);
        let menu = header.rect(HeaderAction::Menu);
        let video_top = ruler_top + s(32);
        let track_bottom = context_top - s(16);
        // Keep four frame/playback buttons together, then give time text only the
        // remaining space before the inspector toggle. Long minute counts must
        // never paint across that button on a narrow physical high-DPI window.
        let transport_step = s(34);
        let transport_left = margin + (preview_right-margin-s(286))/2;
        let time_left = transport_left + transport_step * 3;
        let time_width = s(184);
        Self {
            header,
            new_recording: HitRect(header.rect(HeaderAction::NewRecording)),
            open_project: HitRect(header.rect(HeaderAction::OpenProject)),
            video_sections: [empty; 2],
            crop_edit: empty,
            crop_lock: empty,
            resolution: empty, follow_zoom: empty,
            canvas_presets: [empty; 4],
            canvas_fields: [empty; 2],
            frame_buttons: [empty; 2],
            preview_mute: r(preview_right-s(38), upper_bottom-s(32), s(26), s(26)),
            last: r(transport_left+transport_step*2, upper_bottom-s(32), s(26), s(26)),
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
                    let width = (panel_width - s(4) * 3) / 4;
                    r(
                        panel_x + (width + s(4)) * i32::try_from(i).unwrap_or(0),
                        toolbar + s(PANEL_HEADER + 9),
                        width,
                        s(26),
                    )
                } else {
                    empty
                }
            }),
            external_inspector: false,
            inspector_body_top: toolbar + s(PANEL_HEADER + 44),
            video_speed: empty,
            video_scale: empty,
            speed_reset: empty,
            speed_presets: [empty; 5],
            inspector_content_top: toolbar + s(PANEL_HEADER),
            inspector_scroll: 0,
            inspector_splitter: if inspector > 0 {
                r(preview_right, toolbar, gap, upper_bottom - toolbar)
            } else {
                empty
            },
            inspector_scroll_max: (s(PANEL_HEADER + 536) - (upper_bottom - toolbar)).max(0),
            inspector_scrollbar: if inspector > 0 {
                r(
                    w - margin - s(12),
                    toolbar + s(PANEL_HEADER + 60),
                    s(10),
                    (upper_bottom - toolbar - s(PANEL_HEADER + 68)).max(0),
                )
            } else {
                empty
            },
            transport: r(margin, upper_bottom - s(38), preview_right - margin, s(38)).0,
            context_bar: RECT::default(),
            track_labels: r(margin + s(12), ruler_top, s(88), context_top - ruler_top).0,
            preview_meta: header.bar,
            back: empty,
            preview_fit: r(
                (preview_right - s(44)).min(w - s(80)),
                menu.top,
                s(32),
                menu.bottom - menu.top,
            ),
            audio_panel: r(s(197), top + s(4), s(28), s(27)),
            subtitle_panel: r(s(228), top + s(4), s(28), s(27)),
            inspector_toggle: r(w - s(40), if inspector > 0 { toolbar + s(13) } else { s(6) }, s(28), s(28)),
            start_value: empty,
            end_value: empty,
            scale_value: empty,
            center_focus: empty,
            zoom_slider: r(w - margin - s(164), top + s(4), s(80), s(27)),
            background_file: empty,
            background_color: prop(176, 32),
            cursor_properties: prop(506, 32),
            swatches: std::array::from_fn(|i| {
                if inspector > 0 {
                    let cell = ((panel_width - s(40)) / 6).min(s(36));
                    let gap = (panel_width - cell * 6) / 5;
                    r(
                        panel_x + (cell + gap) * i32::try_from(i).unwrap_or(0),
                        toolbar + s(PANEL_HEADER + 126),
                        cell,
                        cell,
                    )
                } else {
                    empty
                }
            }),
            splitter: r(margin, upper_bottom, w - margin * 2, gap),
            scrollbar: r(left, context_top - s(16), right - left, s(12)),
            track_area: r(left, video_top, right - left, track_bottom - video_top),
            track_scroll_max: 0,
            project_bin: RECT::default(),
            project_bin_splitter: empty,
            canvas_advanced: empty,
            video_track: r(left, video_top, right - left, 0),
            media_tracks: [empty; 35],
            split: r(s(89), top + s(4), s(28), s(27)),
            video_trim: empty,
            camera_properties: empty,
            camera_enabled: empty,
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
                (upper_bottom - toolbar - s(38)).max(1),
            )
            .0,
            panel: r(edge, toolbar, inspector, upper_bottom - toolbar).0,
            timeline_panel: r(margin, top, w - margin * 2, height).0,
            timeline_header: r(margin, top, w - margin * 2, s(36)).0,
            click_track: r(left, video_top, right - left, 0),
            camera_track: r(left, video_top, right - left, 0),
            preview_quality: r(transport_left-s(94), upper_bottom-s(32), s(82), s(26)),
            home: r(transport_left, upper_bottom - s(32), s(26), s(26)),
            play: r(
                transport_left + transport_step,
                upper_bottom - s(32),
                s(26),
                s(26),
            ),
            save: HitRect(header.rect(HeaderAction::Save)),
            undo: r(s(12), top + s(4), s(28), s(27)),
            redo: r(s(43), top + s(4), s(28), s(27)),
            add: r(s(166), top + s(4), s(28), s(27)),
            delete: r(s(120), top + s(4), s(28), s(27)),
            reset: empty,
            diagnostics: HitRect(header.rect(HeaderAction::Settings)),
            timeline_zoom_out: r(w - margin - s(202), top + s(4), s(28), s(27)),
            timeline_zoom_in: r(w - margin - s(70), top + s(4), s(28), s(27)),
            timeline_fit: r(w - margin - s(32), top + s(4), s(28), s(27)),
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
            time: r(time_left, upper_bottom - s(38), time_width, s(38)).0,
            status: RECT {
                left: menu.right + s(8),
                right: (preview_right - s(116)).max(menu.right + s(8)),
                top: menu.top,
                bottom: menu.bottom,
            },
        }
        .with_canvas_details(false, false, false, 0, theme)
        .with_media_tracks(false, theme)
    }

    pub(super) fn with_external_inspector(mut self, external: bool) -> Self {
        self.external_inspector = external;
        if external {
            self.inspector_scroll_max = 0;
            self.inspector_scrollbar = HitRect(RECT::default());
        }
        self
    }

    pub(super) fn inspector_content(self) -> RECT {
        RECT {
            top: self.inspector_body_top.min(self.panel.bottom),
            ..self.panel
        }
    }

    pub(super) fn with_timeline_context(self, _visible: bool) -> Self {
        self
    }

    #[cfg(test)]
    pub(super) fn minimum_media_timeline_height(theme: EditorTheme) -> i32 {
        theme.scale(150)
    }

    pub(super) fn with_media_tracks(self, media: bool, theme: EditorTheme) -> Self {
        self.with_tracks(media, true, 0, theme)
    }

    /// Row heights are independent of dock height. Scroll the viewport when rows do not fit.
    pub(super) fn with_tracks(
        mut self,
        media: bool,
        camera: bool,
        scroll: i32,
        theme: EditorTheme,
    ) -> Self {
        let s = |n| theme.scale(n);
        let compact_window = self.timeline_panel.bottom <= s(684);
        let compact = self.timeline_panel.bottom - self.timeline_panel.top < s(200);
        let video = s(if compact {
            32
        } else if compact_window {
            48
        } else {
            62
        });
        let row = s(if compact || compact_window { 32 } else { 38 });
        let gap = s(5);
        let rows = i32::from(camera) + if media { 3 } else { 0 };
        let total = video + rows * (row + gap);
        let area = self.track_area;
        self.track_scroll_max = (total - (area.bottom - area.top)).max(0);
        let offset = scroll.clamp(0, self.track_scroll_max);
        let mut top = area.top - offset;
        self.video_track.0.top = top;
        self.video_track.0.bottom = top + video;
        self.click_track.0.top = top + video - 2;
        self.click_track.0.bottom = top + video;
        top += video + gap;
        self.camera_track.0.top = top;
        self.camera_track.0.bottom = top + if camera { row } else { 0 };
        if camera {
            top += row + gap;
        }
        self.media_tracks = [HitRect(RECT::default()); 35];
        if media {
            for track in self.media_tracks.iter_mut().take(3) {
                *track = HitRect(RECT {
                    left: area.left,
                    right: area.right,
                    top,
                    bottom: top + row,
                });
                top += row + gap;
            }
        }
        self
    }
    pub(super) fn with_project_bin(
        mut self,
        width: i32,
        overlay: bool,
        theme: EditorTheme,
    ) -> Self {
        self.project_bin = RECT {
            left: 0,
            top: self.preview_well.top,
            right: width,
            bottom: self.panel.bottom,
        };
        let edge = width + if width > 0 { theme.scale(6) } else { 0 };
        self.project_bin_splitter = HitRect(RECT {
            left: width, right: edge, top: self.project_bin.top, bottom: self.project_bin.bottom,
        });
        if width > 0 && !overlay {
            self.preview_card.left = edge;
            self.preview_well.left = edge;
            self.transport.left = edge;
            let delta = width / 2;
            for r in [&mut self.home.0, &mut self.play.0, &mut self.last.0, &mut self.time] {
                r.left += delta;
                r.right += delta;
            }
            for r in &mut self.frame_buttons {
                r.0.left += delta;
                r.0.right += delta;
            }
        }
        let available=(self.transport.right-self.transport.left).max(1);
        let tabs=theme.scale(184).min(available/3);
        let left=self.transport.left+theme.scale(10)+tabs+theme.scale(12);
        let right=self.preview_mute.left-theme.scale(10);
        let group=theme.scale(380).min((right-left).max(0));
        let x=(self.transport.left+(available-group)/2).max(left);
        let quality=theme.scale(82).min((group/4).max(0));
        self.preview_quality=HitRect(RECT{left:x,right:x+quality,top:self.home.top,bottom:self.home.bottom});
        let x=x+quality+theme.scale(10);
        let step=theme.scale(30).min(((group-quality-theme.scale(10))/4).max(1));
        for (i,r) in [&mut self.home.0,&mut self.play.0,&mut self.last.0].into_iter().enumerate() {
            r.left=x+i as i32*step;r.right=r.left+step.min(theme.scale(26));
        }
        self.time.left=x+step*3+theme.scale(4);
        self.time.right=right.max(self.time.left);
        let y = theme.scale(6);
        self.new_recording = HitRect(RECT {
            left: theme.scale(114),
            right: theme.scale(144),
            top: y,
            bottom: y + theme.scale(28),
        });
        self.open_project = HitRect(RECT {
            left: theme.scale(150),
            right: theme.scale(180),
            top: y,
            bottom: y + theme.scale(28),
        });
        self.status.right = theme.scale(106);
        self
    }

    pub(super) fn video_thumbnails_visible(track: HitRect, theme: EditorTheme) -> bool {
        track.bottom - track.top >= theme.scale(44)
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
        // Leave half the window for preview while allowing readable audio and subtitle tracks.
        let minimum = theme.scale(150);
        let maximum = (height * 55 / 100).max(minimum);
        requested.clamp(minimum, maximum)
    }

    pub(super) fn clamp_inspector_width(width: i32, requested: i32, theme: EditorTheme) -> i32 {
        if requested == 0 {
            return 0;
        }
        let minimum = theme.scale(crate::platform::workbench_layout::INSPECTOR_MIN);
        let maximum =
            (width - theme.metrics.outer_margin * 2 - theme.metrics.gap - theme.scale(380)).clamp(
                minimum,
                theme.scale(crate::platform::workbench_layout::INSPECTOR_MAX),
            );
        requested.clamp(minimum, maximum)
    }

    pub(super) fn with_inspector(
        mut self,
        camera: bool,
        _video: bool,
        scroll: i32,
        theme: EditorTheme,
    ) -> Self {
        let s = |n| theme.scale(n);
        let scroll = scroll.clamp(
            0,
            (s(PANEL_HEADER + if camera { 270 } else { 739 }) - (self.panel.bottom - self.panel.top)).max(0),
        );
        self.inspector_scroll=scroll;
        if self.panel.right > self.panel.left && camera {
            let left = self.panel.left + s(16);
            let width = self.panel.right - self.panel.left - s(32);
            let top = self.panel.top + s(PANEL_HEADER + 60) - scroll;
            self.selected_info = RECT {
                left,
                right: left + width,
                top,
                bottom: top + s(22),
            };
            let field_width = (width - s(12)) / 2 - s(32);
            self.start_value = HitRect(RECT {
                left: left + s(32),
                right: left + s(32) + field_width,
                top: top + s(109),
                bottom: top + s(137),
            });
            self.end_value = HitRect(RECT {
                left: left + (width + s(12)) / 2 + s(32),
                right: left + width,
                top: top + s(109),
                bottom: top + s(137),
            });
            if camera {
                self.scale_value = HitRect(RECT {
                    left: left + s(32),
                    right: left + s(112),
                    top: top + s(151),
                    bottom: top + s(179),
                });
                self.center_focus = HitRect(RECT {
                    left: left + s(126),
                    right: left + s(156),
                    top: top + s(151),
                    bottom: top + s(179),
                });
                self.camera_properties = HitRect(RECT {
                    left: left + s(170),
                    right: left + s(200),
                    top: top + s(151),
                    bottom: top + s(179),
                });
            }
        }
        if self.panel.right <= self.panel.left {
            return self;
        }
        self
    }

    pub(super) fn with_canvas_details(
        mut self, advanced: bool, video: bool, image: bool, scroll: i32, theme: EditorTheme,
    ) -> Self {
        if video || self.panel.right <= self.panel.left { return self; }
        let d = |n| theme.scale(n);
        let extra = if image { 60 } else { 0 };
        let height = if advanced { 636 + extra } else { 390 + extra };
        self.inspector_scroll_max=(d(PANEL_HEADER+height)-(self.panel.bottom-self.panel.top)).max(0);
        self.inspector_scroll=scroll.clamp(0,self.inspector_scroll_max);
        self.inspector_content_top=self.panel.top+d(PANEL_HEADER)-self.inspector_scroll;
        let left=self.panel.left+d(16); let right=self.panel.right-d(16); let width=right-left;
        let empty=HitRect(RECT::default());
        let row=|y,h|HitRect(RECT{left,right,top:self.inspector_content_top+d(y),bottom:self.inspector_content_top+d(y+h)});
        self.canvas_presets=std::array::from_fn(|i| HitRect(RECT{left:left+width*i as i32/4,right:left+width*(i as i32+1)/4,..row(92,30).0}));
        self.resolution=HitRect(RECT{left:left+d(76),..row(134,28).0});
        self.background=HitRect(RECT{right:left+width/2,..row(220,30).0});
        self.background_image=HitRect(RECT{left:left+width/2,..row(220,30).0});
        self.background_color=if image {empty}else{HitRect(RECT{left:right-d(44),..row(262,28).0})};
        self.background_file=if image {row(262,88)}else{empty};
        self.inset=row(304+extra,28);
        self.canvas_advanced=row(360+extra,28);
        self.radius=if advanced {row(404+extra,28)}else{empty};
        self.shadow=if advanced {row(442+extra,28)}else{empty};
        self.cursor=if advanced {row(486+extra,28)}else{empty};
        self.cursor_properties=if advanced {row(524+extra,28)}else{empty};
        self.canvas_fields=std::array::from_fn(|i|if advanced {let w=(width-d(12))/2;HitRect(RECT{left:left+(w+d(12))*i as i32,right:left+(w+d(12))*i as i32+w,..row(574+extra,52).0})}else{empty});
        self.swatches=[empty;6];
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
            &mut self.canvas_advanced,
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
        self.resolution=empty;
        let s = |n| theme.scale(n);
        if self.scale_value.right > self.scale_value.left {
            self.inspector_scroll_max = (s(PANEL_HEADER + 270) - (self.panel.bottom - self.panel.top)).max(0);
            return self;
        }
        let crop_top = 250;
        let height = crop_top + if collapsed[1] { 48 } else { 358 };
        self.inspector_scroll_max = (s(PANEL_HEADER + height) - (self.panel.bottom - self.panel.top)).max(0);
        let previous_scroll=self.inspector_scroll;
        self.inspector_scroll = scroll.clamp(0, self.inspector_scroll_max);
        let correction=previous_scroll-self.inspector_scroll;
        for r in [&mut self.selected_info,&mut self.start_value.0,&mut self.end_value.0] {r.top+=correction;r.bottom+=correction;}
        self.inspector_content_top = self.panel.top + s(PANEL_HEADER) - self.inspector_scroll;
        let left = self.panel.left + s(16);
        let right = self.panel.right - s(16);
        let row = |top, height| HitRect(RECT {left, right, top:self.inspector_content_top+s(top), bottom:self.inspector_content_top+s(top+height)});
        // Speed is always available; the original prototype has no redundant section heading.
        self.video_sections = [empty, row(crop_top, 22)];
        self.speed_reset = empty;
        self.video_speed = row(58, 54);
        self.video_scale = row(164, 54);
        let width = right-left;
        self.speed_presets = std::array::from_fn(|i| {
            if i==4 {return empty;}
            HitRect(RECT {left:left+width*i as i32/4,right:left+width*(i as i32+1)/4,..row(120,30).0})
        });
        self.crop_reset = if collapsed[1] {empty}else{HitRect(RECT { left:right-s(28),..row(crop_top+70,30).0 })};
        self.crop_lock = if collapsed[1] {empty}else{HitRect(RECT {right:left+width/4,..row(crop_top+28,30).0})};
        self.crop_edit = if collapsed[1] {empty}else{HitRect(RECT {right:right-s(38),..row(crop_top+70,30).0})};
        if !collapsed[1] {
            self.crop_presets=std::array::from_fn(|i| {
                if i==3 {return empty;}
                HitRect(RECT {left:left+width*(i as i32+1)/4,right:left+width*(i as i32+2)/4,..row(crop_top+28,30).0})
            });
            self.crop_edges=std::array::from_fn(|i| row(crop_top+118+i as i32*52,48));
        }

        self
    }

    pub(super) fn with_crop_details(mut self, editing: bool, theme: EditorTheme) -> Self {
        if !editing && self.crop_edit.right > self.crop_edit.left {
            self.crop_edges = [HitRect(RECT::default()); 4];
            let content =
                self.crop_edit.bottom + self.inspector_scroll + theme.scale(24) - self.panel.top;
            self.inspector_scroll_max = (content - (self.panel.bottom - self.panel.top)).max(0);
        }
        self
    }

    pub(super) fn with_follow_details(mut self,video:bool,expanded:bool,theme:EditorTheme)->Self {
        if !video || self.panel.right<=self.panel.left || self.scale_value.right>self.scale_value.left {return self;}
        let d=|n|theme.scale(n);let left=self.panel.left+d(16);let right=self.panel.right-d(16);
        let bottom=if self.crop_edges[3].bottom>0 {self.crop_edges[3].bottom}else if self.crop_edit.bottom>0 {self.crop_edit.bottom}else{self.video_sections[1].bottom};
        let top=bottom+d(24);
        self.video_sections[0]=HitRect(RECT{left,right,top,bottom:top+d(26)});
        if expanded {
            self.camera_enabled=HitRect(RECT{left,right,top:top+d(38),bottom:top+d(68)});
            self.follow_zoom=HitRect(RECT{left:right-d(100),right,top:top+d(76),bottom:top+d(104)});
            self.reset=HitRect(RECT::default());
        }
        let end=top+d(if expanded {122}else{42});
        self.inspector_scroll_max=(end+self.inspector_scroll-self.panel.bottom).max(0);
        self
    }

    pub(super) fn is_inspector_control(id: ControlId) -> bool {
        matches!(
            id,
            ControlId::CanvasAdvanced
                | ControlId::CameraEnabled
                | ControlId::Reset
                | ControlId::StartValue
                | ControlId::EndValue
                | ControlId::ScaleValue
                | ControlId::CenterFocus
                | ControlId::CameraProperties
                | ControlId::VideoTrim
                | ControlId::SpeedSection
                | ControlId::CropSection
                | ControlId::CropEdit
                | ControlId::CropLock
                | ControlId::CanvasStandard | ControlId::CanvasResolution | ControlId::FollowZoom
                | ControlId::CanvasOriginal
                | ControlId::CanvasWide
                | ControlId::CanvasPortrait
                | ControlId::CanvasSquare
                | ControlId::CanvasWidth
                | ControlId::CanvasHeight
                | ControlId::VideoScale
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

    pub(super) fn timeline_area(self) -> HitRect {
        HitRect(RECT {
            left: self.camera_track.left,
            top: self.ruler.top,
            right: self.camera_track.right,
            bottom: self.track_area.bottom,
        })
    }

    pub(super) fn hit_control(self, x: i32, y: i32) -> Option<ControlId> {
        ControlId::ORDER
            .into_iter()
            .find(|id| self.control_rect(*id).contains(x, y))
    }

    pub(super) fn control_rect(self, id: ControlId) -> HitRect {
        if self.external_inspector && Self::is_inspector_control(id) {
            return HitRect(RECT::default());
        }
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
            ControlId::CanvasAdvanced => self.canvas_advanced,
            ControlId::CanvasTab => self.inspector_tabs[0],
            ControlId::VideoTab => self.inspector_tabs[1],
            ControlId::AudioTab => self.inspector_tabs[2],
            ControlId::SubtitleTab => self.inspector_tabs[3],
            ControlId::SpeedSection => self.video_sections[0],
            ControlId::CropSection => self.video_sections[1],
            ControlId::CropEdit => self.crop_edit,
            ControlId::CropLock => self.crop_lock,
            ControlId::CanvasOriginal => HitRect(RECT::default()),
            ControlId::CanvasWide => self.canvas_presets[0],
            ControlId::CanvasStandard => self.canvas_presets[1],
            ControlId::CanvasSquare => self.canvas_presets[2],
            ControlId::CanvasPortrait => self.canvas_presets[3],
            ControlId::CanvasResolution => self.resolution,
            ControlId::FollowZoom => self.follow_zoom,
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
            ControlId::VideoScale => self.video_scale,
            ControlId::SpeedReset => self.speed_reset,
            ControlId::SpeedHalf => self.speed_presets[0],
            ControlId::SpeedNormal => self.speed_presets[1],
            ControlId::SpeedOneHalf => self.speed_presets[2],
            ControlId::SpeedDouble => self.speed_presets[3],
            ControlId::SpeedQuadruple => self.speed_presets[4],
            ControlId::BackgroundFile => self.background_file,
            ControlId::BackgroundColor => self.background_color,
            ControlId::AudioPanel => self.audio_panel,
            ControlId::SubtitlePanel => self.subtitle_panel,
            ControlId::PreviewFit => self.preview_fit,
            ControlId::PreviewMute => self.preview_mute,
            ControlId::Last => self.last,
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
    fn pane_titles_align_and_stay_above_scrolling_content() {
        use crate::platform::workbench_layout::panel_title;
        for dpi in [96, 120, 144, 192] {
            let theme=EditorTheme::for_dpi(dpi);
            for (width,height) in [(1100,680),(1440,900),(2560,1440)] {
                let client=RECT{right:theme.scale(width),bottom:theme.scale(height),..RECT::default()};
                for scroll in [0,10000] {
                    let c=ControlLayout::new(client,theme)
                        .with_canvas_details(true,false,true,scroll,theme)
                        .with_project_bin(theme.scale(212),false,theme);
                    let left=panel_title(c.project_bin,theme);let right=panel_title(c.panel,theme);
                    assert_eq!((left.top,left.bottom),(right.top,right.bottom));
                    assert_eq!(c.project_bin.top,c.panel.top);
                    assert_eq!(c.project_bin.bottom,c.panel.bottom);
                    assert!(right.bottom<c.inspector_tabs[0].top);
                    assert!(c.inspector_tabs[0].bottom<c.inspector_body_top);
                    assert_eq!((c.inspector_toggle.top,c.inspector_toggle.bottom),(right.top,right.bottom));
                    for (id,splitter) in c.splitters() {
                        let r=splitter.bounds;let x=(r.left+r.right)/2;let y=(r.top+r.bottom)/2;
                        assert_eq!(c.splitter_at(x,y).map(|(id,_)|id),Some(id));
                        if id!=SplitterId::Timeline {
                            assert_eq!((r.top,r.bottom),(c.panel.top,c.panel.bottom));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn all_inspector_pages_use_the_same_boundary_above_full_width_tracks() {
        for dpi in [96, 120, 144, 192] {
            for width in [960, 1440, 1920] {
                for dock in [216, 350, 480] {
                    let mut theme = EditorTheme::for_dpi(dpi);
                    theme.metrics.timeline_height = theme.scale(dock);
                    let client = RECT {
                        right: theme.scale(width),
                        bottom: theme.scale(900),
                        ..RECT::default()
                    };
                    let builtin =
                        ControlLayout::new(client, theme).with_inspector(false, true, 0, theme);
                    let external = builtin.with_external_inspector(true);
                    assert_eq!(builtin.panel, external.panel);
                    assert_eq!(builtin.timeline_panel, external.timeline_panel);
                    assert_eq!(external.inspector_content().bottom, external.splitter.top);
                    assert!(external.inspector_content().top >= external.inspector_tabs[0].bottom);
                    assert!(external.timeline_panel.right > external.panel.left);
                    for (i, id) in [
                        ControlId::CanvasTab,
                        ControlId::VideoTab,
                        ControlId::AudioTab,
                        ControlId::SubtitleTab,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let r = external.control_rect(id);
                        assert_eq!(
                            external.hit_control((r.left + r.right) / 2, (r.top + r.bottom) / 2),
                            Some(id)
                        );
                        if i > 0 {
                            assert!(external.inspector_tabs[i - 1].right < r.left);
                        }
                    }
                    assert_eq!(
                        external.control_rect(ControlId::CropLeft).0,
                        RECT::default()
                    );
                    assert_eq!(external.inspector_scroll_max, 0);
                }
            }
        }
    }

    #[test]
    fn preview_fit_and_inspector_toggle_have_distinct_hit_targets_when_closed() {
        for dpi in [96,120,144,192,288] { for width in [640,960,1440,2560] { for inspector in [0,304] {
            let mut t=EditorTheme::for_dpi(dpi);t.metrics.inspector_width=t.scale(inspector);
            let c=ControlLayout::new(RECT{right:t.scale(width),bottom:t.scale(900),..Default::default()},t);
            assert!(c.preview_fit.right+t.scale(8)<=c.inspector_toggle.left);
            for id in [ControlId::PreviewFit,ControlId::InspectorToggle] {let r=c.control_rect(id);assert_eq!(c.hit_control((r.left+r.right)/2,(r.top+r.bottom)/2),Some(id));}
        }}}
    }
    #[test]
    fn preview_tools_share_one_row_with_saved_state() {
        for dpi in [96, 120, 144, 192] {
            for width in [960, 1280, 1920] {
                let theme = EditorTheme::for_dpi(dpi);
                let c = ControlLayout::new(
                    RECT {
                        right: theme.scale(width),
                        bottom: theme.scale(900),
                        ..RECT::default()
                    },
                    theme,
                );
                let menu = c.header.rect(HeaderAction::Menu);
                assert_eq!((c.status.top, c.status.bottom), (menu.top, menu.bottom));
                assert_eq!(
                    (c.preview_fit.top, c.preview_fit.bottom),
                    (menu.top, menu.bottom)
                );
                assert_eq!(c.status.left, menu.right + theme.scale(8));
                assert!(c.status.right < c.preview_fit.left - theme.scale(64));
                assert_eq!(c.preview_well.top, c.preview_card.top);
                assert_eq!(c.diagnostics.0, RECT::default());
                assert!(c.preview_meta.bottom <= c.preview_well.top);
            }
        }
    }

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
                for pair in layout.speed_presets[..4].windows(2) {
                    assert!(pair[0].right <= pair[1].left);
                }
                assert_eq!(layout.speed_presets[4].0,RECT::default());
                assert!(layout.speed_presets[3].right < layout.panel.right);
            }
        }
    }
    #[test]
    fn timeline_expands_for_media_tracks_and_reclamps_after_window_resize() {
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
                let expected = (client.bottom * 55 / 100)
                    .max(ControlLayout::minimum_media_timeline_height(theme));
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
                    theme.scale(150)
                );
                assert!(layout.scrollbar.top - layout.camera_track.bottom >= theme.scale(8));
            }
        }
    }

    #[test]
    fn taller_timeline_caps_video_and_keeps_empty_room_below_tracks() {
        for dpi in [96, 120, 144, 192] {
            let mut theme = EditorTheme::for_dpi(dpi);
            let client = RECT {
                right: theme.scale(1440),
                bottom: theme.scale(1417),
                ..RECT::default()
            };
            for media in [false, true] {
                let mut previous = 0;
                for height in [216, 244, 264, 350, 440, 700] {
                    theme.metrics.timeline_height = theme.scale(height).max(if media {
                        ControlLayout::minimum_media_timeline_height(theme)
                    } else {
                        theme.scale(216)
                    });
                    let c = ControlLayout::new(client, theme).with_media_tracks(media, theme);
                    let video_height = c.video_track.bottom - c.video_track.top;
                    assert!(video_height <= theme.scale(72));
                    assert!(video_height >= previous);
                    previous = video_height;
                    assert!(c.video_track.bottom < c.camera_track.top);
                    assert!(c.camera_track.bottom <= c.track_area.bottom);
                    if media {
                        assert!(c.camera_track.bottom < c.media_tracks[0].top);
                        for pair in c.media_tracks[..3].windows(2) {
                            assert!(pair[0].bottom < pair[1].top);
                        }
                        for track in &c.media_tracks[..3] {
                            assert!(
                                track.bottom > track.top
                                    && track.bottom <= c.track_area.bottom + c.track_scroll_max
                            );
                        }
                    }
                    assert_eq!(c.ruler.left, c.video_track.left);
                    assert_eq!(c.ruler.right, c.video_track.right);
                    assert!(c.video_track.contains(
                        c.video_track.left + 4,
                        (c.video_track.top + c.video_track.bottom) / 2
                    ));
                    assert!(c.split.bottom <= c.ruler.top);
                    if height < 200 && media {
                        assert!(!ControlLayout::video_thumbnails_visible(
                            c.video_track,
                            theme
                        ));
                    }
                    if height >= 440 {
                        assert_eq!(video_height, theme.scale(62));
                        assert!(ControlLayout::video_thumbnails_visible(
                            c.video_track,
                            theme
                        ));
                    }
                    if height == 700 {
                        assert!(
                            c.track_area.bottom
                                - if media {
                                    c.media_tracks[2].bottom
                                } else {
                                    c.camera_track.bottom
                                }
                                > theme.scale(100)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn occupied_lanes_stay_readable_and_use_the_reclaimed_space() {
        for dpi in [96, 120, 144, 192] {
            for selected in [false, true] {
                let mut theme = EditorTheme::for_dpi(dpi);
                for height in [272, 304, 350, 440] {
                    theme.metrics.timeline_height = theme.scale(height);
                    let c = ControlLayout::new(
                        RECT {
                            right: theme.scale(1440),
                            bottom: theme.scale(1000),
                            ..RECT::default()
                        },
                        theme,
                    )
                    .with_timeline_context(selected)
                    .with_media_tracks(true, theme);
                    for row in [
                        c.video_track,
                        c.camera_track,
                        c.media_tracks[0],
                        c.media_tracks[1],
                        c.media_tracks[2],
                    ] {
                        assert!(
                            row.bottom - row.top >= theme.scale(24),
                            "DPI {dpi}, dock {height}, selection {selected}: unreadable row"
                        );
                        assert!(row.bottom <= c.track_area.bottom + c.track_scroll_max);
                    }
                    assert!(c.status.bottom <= c.preview_well.top);
                    assert!(c.status.right < c.preview_fit.left - theme.scale(64));
                    assert!(c.media_tracks[2].bottom <= c.track_area.bottom + c.track_scroll_max);
                    assert_eq!(c.context_bar.bottom - c.context_bar.top, 0);
                    if height >= 350 {
                        assert!(c.video_track.bottom - c.video_track.top >= theme.scale(44));
                    }
                }
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
                let c = ControlLayout::new(client, theme)
                    .with_inspector(false, false, 0, theme)
                    .with_canvas_details(true, false, false, 0, theme);
                assert_eq!(
                    c.panel.right - c.panel.left,
                    theme.scale(width.clamp(264, 420))
                );
                assert!(c.swatches[5].right <= c.panel.right - theme.scale(16));
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
        assert_eq!(layout.command_bar.bottom, 40);
        assert_eq!(layout.preview_card.top, 40);
        assert_eq!(layout.panel.right - layout.panel.left, 304);
        assert_eq!(
            layout.timeline_panel.bottom - layout.timeline_panel.top,
            282
        );
        assert_eq!(layout.transport.bottom - layout.transport.top, 38);
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
            let t = EditorTheme::for_dpi(dpi);
            for (w, h) in [(960, 720), (1440, 900)] {
                let client = RECT {
                    right: t.scale(w),
                    bottom: t.scale(h),
                    ..RECT::default()
                };
                for video in [false, true] {
                    let build = |scroll| {
                        ControlLayout::new(client, t)
                            .with_inspector(false, video, scroll, t)
                            .with_canvas_details(true, video, false, scroll, t)
                            .with_video_groups(video, scroll, t, [false; 2])
                    };
                    let base = build(0);
                    let mut reachable = std::collections::HashSet::new();
                    for scroll in (0..=base.inspector_scroll_max + t.scale(10)).step_by(8) {
                        let c = build(scroll);
                        let visible: Vec<_> = ControlId::ORDER
                            .into_iter()
                            .filter_map(|id| {
                                let r = c.control_rect(id);
                                (r.right > r.left && r.bottom > r.top).then_some((id, r))
                            })
                            .collect();
                        for (i, (id, a)) in visible.iter().enumerate() {
                            reachable.insert(*id as u32);
                            assert!(
                                a.left >= 0
                                    && a.top >= 0
                                    && a.right <= client.right
                                    && a.bottom <= client.bottom,
                                "{dpi} {id:?}"
                            );
                            for (other, b) in visible.iter().skip(i + 1) {
                                assert!(
                                    !(a.left < b.right
                                        && b.left < a.right
                                        && a.top < b.bottom
                                        && b.top < a.bottom),
                                    "{dpi}: {id:?} overlaps {other:?}"
                                );
                            }
                        }
                    }
                    let required = if video {
                        vec![
                            ControlId::VideoScale,
                            ControlId::VideoSpeed,
                            ControlId::CropBottom,
                        ]
                    } else {
                        vec![
                            ControlId::CanvasWidth,
                            ControlId::CanvasHeight,
                            ControlId::BackgroundColor,
                            ControlId::Inset,
                            ControlId::Radius,
                            ControlId::Shadow,
                            ControlId::CursorProperties,
                        ]
                    };
                    for id in required {
                        assert!(
                            reachable.contains(&(id as u32)),
                            "{dpi} {w}: {id:?} unreachable"
                        );
                    }
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
            let image = preset.with_canvas_details(false, false, true, 0, theme);
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
                    assert!(
                        layout.click_track.top >= layout.video_track.top
                            && layout.click_track.bottom <= layout.video_track.bottom
                    );
                    assert!(layout.click_track.bottom <= layout.camera_track.top);
                    assert!(layout.camera_track.bottom <= layout.scrollbar.top);
                    assert!(layout.context_bar.bottom <= layout.ruler.top);
                    assert!(layout.scrollbar.bottom <= layout.timeline_panel.bottom);
                    assert!(layout.status.bottom <= layout.preview_well.top);
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
fn compact_track_viewport_scrolls_without_squeezing_or_overlap() {
    for dpi in [96, 120, 144, 192] {
        let mut t = EditorTheme::for_dpi(dpi);
        t.metrics.timeline_height = t.scale(150);
        let client = RECT {
            right: t.scale(960),
            bottom: t.scale(720),
            ..RECT::default()
        };
        let base = ControlLayout::new(client, t).with_tracks(true, true, 0, t);
        assert_eq!(base.video_track.bottom - base.video_track.top, t.scale(32));
        assert!(base.track_scroll_max > 0);
        let last = ControlLayout::new(client, t).with_tracks(true, true, base.track_scroll_max, t);
        assert_eq!(last.media_tracks[2].bottom, last.track_area.bottom);
        assert!(last.media_tracks[2].bottom - last.media_tracks[2].top >= t.scale(32));
        assert_eq!(base.ruler.0, last.ruler.0);
        assert!(base.split.bottom <= base.ruler.top);
    }
}

#[test]
fn selection_parameters_hide_with_inspector_and_reappear_in_selected_clip_page() {
    for dpi in [96, 120, 144, 192] {
        let mut t = EditorTheme::for_dpi(dpi);
        let client = RECT {
            right: t.scale(960),
            bottom: t.scale(720),
            ..RECT::default()
        };
        t.metrics.inspector_width = 0;
        let hidden = ControlLayout::new(client, t)
            .with_inspector(false, true, 0, t)
            .with_video_groups(true, 0, t, [false; 2]);
        for id in [ControlId::StartValue, ControlId::EndValue] {
            assert_eq!(hidden.control_rect(id).0, RECT::default());
        }
        assert_eq!(hidden.context_bar, RECT::default());
        t.metrics.inspector_width = t.scale(284);
        let shown = ControlLayout::new(client, t)
            .with_inspector(false, true, 0, t)
            .with_video_groups(true, 0, t, [false; 2]);
        for id in [ControlId::StartValue, ControlId::EndValue] { assert_eq!(shown.control_rect(id).0,RECT::default()); }
        let camera=ControlLayout::new(client,t).with_inspector(true,false,0,t);
        for id in [ControlId::StartValue,ControlId::EndValue] {assert!(camera.control_rect(id).right>camera.control_rect(id).left);}
        for id in [ControlId::VideoSpeed, ControlId::VideoScale] {
            let r = shown.control_rect(id);
            assert!(r.left >= shown.panel.left && r.right <= shown.panel.right);
            assert!(r.top >= shown.inspector_body_top && r.bottom <= shown.panel.bottom);
            assert_eq!(
                shown.hit_control((r.left + r.right) / 2, (r.top + r.bottom) / 2),
                Some(id)
            );
        }
    }
}
