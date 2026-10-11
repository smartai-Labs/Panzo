//! Prototype controls wired to existing edit transactions; no new media pipeline.
use super::{
    CameraState, ControlId, DRAG_REFRESH_INTERVAL, EditorTheme, HWND, Instant, RECT, SetCapture,
    SetFocus, TICKS_PER_SECOND, TimeTick, WindowState,
};
#[cfg(test)]
pub(super) const SWATCHES: [ControlId; 6] = [
    ControlId::SwatchMist,
    ControlId::SwatchPeach,
    ControlId::SwatchLilac,
    ControlId::SwatchCharcoal,
    ControlId::SwatchWhite,
    ControlId::SwatchSage,
];
pub(super) const SPEED_PRESETS: [(ControlId, f64); 5] = [
    (ControlId::SpeedHalf, 0.5),
    (ControlId::SpeedNormal, 1.0),
    (ControlId::SpeedOneHalf, 1.5),
    (ControlId::SpeedDouble, 2.0),
    (ControlId::SpeedQuadruple, 4.0),
];
pub(super) const CROP_EDGES: [(ControlId, &str); 4] = [
    (ControlId::CropLeft, "左侧"),
    (ControlId::CropTop, "顶部"),
    (ControlId::CropRight, "右侧"),
    (ControlId::CropBottom, "底部"),
];
pub(super) const CROP_PRESETS: [(ControlId, &str, f64); 4] = [
    (ControlId::CropWide, "16:9", 16.0 / 9.0),
    (ControlId::CropStandard, "4:3", 4.0 / 3.0),
    (ControlId::CropSquare, "1:1", 1.0),
    (ControlId::CropPortrait, "9:16", 9.0 / 16.0),
];
impl ControlId {
    pub(super) const fn is_crop_edge(self) -> bool {
        matches!(
            self,
            Self::CropLeft | Self::CropTop | Self::CropRight | Self::CropBottom
        )
    }
}
pub(super) fn swatch_color(id: ControlId) -> &'static str {
    match id {
        ControlId::SwatchPeach => "#f0d7c3",
        ControlId::SwatchLilac => "#d8d1ef",
        ControlId::SwatchCharcoal => "#383a46",
        ControlId::SwatchWhite => "#f8f8f8",
        ControlId::SwatchSage => "#ccdace",
        _ => "#c5d5e7",
    }
}
pub(super) fn value_rect(mut r: RECT, theme: EditorTheme, id: ControlId) -> RECT {
    if matches!(id, ControlId::CanvasWidth | ControlId::CanvasHeight) {
        return RECT {
            top: r.top + theme.scale(24),
            ..r
        };
    }

    if matches!(
        id,
        ControlId::Inset | ControlId::Radius | ControlId::Shadow | ControlId::VideoScale | ControlId::VideoSpeed
    ) || id.is_crop_edge()
    {
        r.left = r.right - theme.scale(100);
        r.bottom = r.top + theme.scale(28);
    }
    r
}
impl WindowState {
    fn canvas_pixel_basis(&self) -> f64 {
        let size=self.canvas_size(); f64::from(size.width.min(size.height))
    }
    pub(super) fn selected_video_clip(&self) -> Option<panzo_core::VideoClip> {
        let id = self.selected_video.as_ref()?;
        if let Some(c)=self.editor.settings().visual_clips.iter().find(|c| &c.id==id) {
            return Some(panzo_core::VideoClip { id:c.id.clone(), source_in_tick:c.source_in, source_out_tick:c.source_out, speed_percent:c.speed_percent, crop:c.crop, scale_per_mille:(c.scale*1000.0).round() as u16 });
        }
        self.editor.video_edit().clips.iter().find(|c| &c.id==id).cloned()
    }
    pub(super) fn selected_video_size(&self)->(u32,u32) {
        self.selected_video.as_ref().and_then(|id|self.editor.settings().visual_clips.iter().find(|c|&c.id==id)).and_then(|c|self.editor.settings().visual_assets.iter().find(|a|a.id==c.asset_id)).map_or(self.frame.source_size(),|a|(a.width,a.height))
    }
    fn selected_video_duration(&self)->TimeTick {
        self.selected_video.as_ref().and_then(|id|self.editor.settings().visual_clips.iter().find(|c|&c.id==id)).and_then(|c|self.editor.settings().visual_assets.iter().find(|a|a.id==c.asset_id)).map_or(self.editor.duration(),|a|a.duration)
    }
    pub(super) fn selected_crop(&self) -> Option<panzo_core::VideoCrop> {
        self.selected_video_clip().map(|c|c.crop)
    }
    pub(super) fn set_selected_crop(&mut self, crop: panzo_core::VideoCrop) -> Result<(), String> {
        self.select_preview_source(false).map_err(|e|e.to_string())?;
        let id = self.selected_video.clone().ok_or("请先选择视频片段")?;
        self.editor
            .set_video_crop(&id, crop)
            .map_err(|error| self.edit_error(&error))?;
        let video = self.editor.video_edit();
        let range = self.editor.settings().visual_clips.iter().find(|c|c.id==id).map(|c|(c.start,c.end())).or_else(||video.spans().find(|span|span.clip.id==id).map(|s|(s.project_in,s.project_out)));
        if let Some((a,b))=range {let tick=self.session.snapshot().project_tick;if tick<a||tick>=b {self.session.seek(a).map_err(|e|e.to_string())?;}}
        Ok(())
    }

    pub(super) fn apply_crop_preset(&mut self, hwnd: HWND, id: ControlId) {
        self.crop_locked = id != ControlId::CropReset;
        let size = self.selected_video_size();
        let crop = CROP_PRESETS
            .iter()
            .find(|(control, _, _)| *control == id)
            .map_or_else(panzo_core::VideoCrop::default, |(_, _, ratio)| {
                panzo_core::VideoCrop::centered_aspect(size.0, size.1, *ratio)
            });
        match self.set_selected_crop(crop) {
            Ok(()) => {
                if id != ControlId::CropReset {
                    self.set_crop_editing(true);
                } else {
                    let result = self.apply_editor_state();
                    self.record_editor_result(result, "画面裁剪已重置 · 可撤销");
                }
            }
            Err(error) => {
                self.set_status(crate::platform::editor_ui::StatusSeverity::Warning, error);
            }
        }
        self.refresh(hwnd, true);
    }
    pub(super) fn select_video_properties(&mut self) {
        let visual = self.media_selected.as_ref().filter(|(sub,id)| !sub && self.editor.settings().visual_clips.iter().any(|c|&c.id==id)).map(|(_,id)|id.clone());
        if let Some(id) = visual {
            self.clear_inspector_panel(); self.selected_video=Some(id); self.selected_segment=None;
            self.inspector_video_tab=true; self.inspector_scroll=0; return;
        }

        self.clear_inspector_panel();
        self.media_selected = None;
        if !self.editor.settings().source_video_enabled{self.selected_video=None;self.selected_segment=None;self.inspector_video_tab=true;return;}
        if self.crop_editing {
            self.toggle_crop_editing();
        }
        let video = self.editor.video_edit();
        if !video
            .clips
            .iter()
            .any(|clip| Some(&clip.id) == self.selected_video.as_ref())
        {
            let tick = self
                .session
                .snapshot()
                .project_tick
                .min(TimeTick(video.duration().0 - 1));
            self.selected_video = video.clip_at(tick).map(|span| span.clip.id.clone());
        }
        self.selected_segment = None;
        self.inspector_video_tab = true;
        self.inspector_scroll = 0;
    }

    pub(super) fn widget_range(&self, id: ControlId) -> Option<(f64, f64)> {
        if id.is_crop_edge() {
            let crop = self.selected_crop()?;
            let opposite = match id {
                ControlId::CropLeft => crop.right,
                ControlId::CropRight => crop.left,
                ControlId::CropTop => crop.bottom,
                _ => crop.top,
            };
            return Some((self.widget_value(id)?, f64::from(950 - opposite) / 10.0));
        }
        if id == ControlId::ZoomSlider {
            let value = self.timeline.zoom_fraction() * 100.0;
            return Some((value, 100.0));
        }
        let maximum = match id {
            ControlId::VideoSpeed => 4.0,
            ControlId::VideoScale => 400.0,
            ControlId::Inset => self.canvas_pixel_basis() * 0.25,
            ControlId::FollowZoom => 3.0,
            ControlId::Radius => {
                f64::from(
                    self.frame
                        .evaluation
                        .output
                        .width
                        .min(self.frame.evaluation.output.height),
                ) * 0.1
            }
            ControlId::Shadow => 100.0,
            _ => return None,
        };
        self.widget_value(id).map(|value| (value, maximum))
    }
    pub(super) fn apply_widget_range(&mut self, hwnd: HWND, id: ControlId, value: f64) {
        if !self.control_enabled(id) {
            return;
        }
        let Some((_, maximum)) = self.widget_range(id) else {
            return;
        };
        let minimum = if id == ControlId::VideoSpeed {
            0.25
        } else if id == ControlId::VideoScale {
            5.0
        } else {
            0.0
        };
        if !value.is_finite() || !(minimum..=maximum).contains(&value) {
            return;
        }
        if id == ControlId::ZoomSlider {
            self.timeline.set_zoom_fraction(self.session.project_playhead(), value / 100.0);
            self.follow_playhead = false;
        } else {
            match self.apply_widget_value(id, value) {
                Ok(()) => {
                    let result = self.apply_editor_state();
                    self.record_editor_result(result, "参数已更新 · 可撤销");
                }
                Err(error) => self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Warning,
                    format!("参数未应用：{error}"),
                ),
            }
        }
        self.refresh(hwnd, true);
    }

    pub(super) fn widget_value(&self, id: ControlId) -> Option<f64> {
        let settings = self.editor.settings();
        match id {
            ControlId::CanvasWidth => Some(f64::from(self.canvas_size().width)),
            ControlId::CanvasHeight => Some(f64::from(self.canvas_size().height)),
            ControlId::CropLeft
            | ControlId::CropTop
            | ControlId::CropRight
            | ControlId::CropBottom => self.selected_crop().map(|crop| {
                f64::from(match id {
                    ControlId::CropLeft => crop.left,
                    ControlId::CropTop => crop.top,
                    ControlId::CropRight => crop.right,
                    _ => crop.bottom,
                }) / 10.0
            }),
            ControlId::VideoScale => self.selected_video_clip().map(|c| f64::from(c.scale_per_mille)/10.0),
            ControlId::VideoSpeed => self.selected_video_clip().map(|c| f64::from(c.speed_percent)/100.0),
            ControlId::Inset => Some(settings.canvas.inset * self.canvas_pixel_basis()),
            ControlId::FollowZoom => Some(settings.automatic_camera.as_ref().map_or(1.5, |c| c.default.zoom)),
            // Display radius in source pixels without changing the normalized saved schema.
            ControlId::Radius => Some(
                settings.canvas.corner_radius
                    * f64::from(
                        self.frame
                            .evaluation
                            .output
                            .width
                            .min(self.frame.evaluation.output.height),
                    ),
            ),
            ControlId::Shadow => Some(settings.canvas.shadow * 100.0),
            ControlId::ScaleValue => self.selected_segment().map(|s| s.to.scale),
            ControlId::StartValue | ControlId::EndValue => {
                let start = id == ControlId::StartValue;
                self.selected_segment()
                    .map(|s| {let (a,b)=settings.follow_positions.get(&s.id).copied().unwrap_or((s.start_tick,s.end_tick));if start {a}else{b}})
                    .or_else(|| {
                        self.selected_video_clip()
                            .map(|c| {
                                if start {
                                    c.source_in_tick
                                } else {
                                    c.source_out_tick
                                }
                            })
                    })
                    .map(|t| t.0 as f64 / TICKS_PER_SECOND as f64)
            }
            _ => None,
        }
    }
    pub(super) fn step_widget_at(&mut self, hwnd: HWND, x:i32,y:i32)->bool {
        let c=self.control_layout(super::client_rect(hwnd));
        let Some(id)=c.hit_control(x,y) else {return false;};
        let Some(value)=self.widget_value(id) else {return false;};
        let r=value_rect(c.raw_control_rect(id).0,self.theme,id);
        if x<r.right-self.theme.scale(40)||x>=r.right-self.theme.scale(26)||y<r.top||y>=r.bottom {return false;}
        let step=if matches!(id,ControlId::Inset|ControlId::Radius|ControlId::CanvasWidth|ControlId::CanvasHeight){1.0}else{0.1};
        let next=value+if y<(r.top+r.bottom)/2 {step}else{-step};
        if self.apply_widget_value(id,next).is_ok() {let result=self.apply_editor_state();self.record_editor_result(result,"参数已更新 · 可撤销");}
        self.refresh(hwnd,true); true
    }
    pub(super) fn begin_inline_input(&mut self, hwnd: HWND, id: ControlId) {
        if !self.finish_inline_input(hwnd, true) {
            return;
        }
        self.reveal_inspector_control(hwnd, id);
        let Some(value) = self.widget_value(id) else {
            return;
        };
        self.finish_wheel_edit();
        self.session.pause();
        self.deferred_playback.cancel();
        let layout = self.control_layout(super::client_rect(hwnd));
        let mut rect = value_rect(layout.raw_control_rect(id).0, self.theme, id);
        rect.right -= self.theme.scale(26);
        if rect.right <= rect.left {
            return;
        }
        match crate::platform::inline_input::InlineInput::open(hwnd, rect, &format_widget_value(id,value)) {
            Ok(input) => {
                let (min,max,step)=match id {
                    ControlId::VideoSpeed => (0.25,4.0,0.1),
                    ControlId::VideoScale => (5.0,400.0,0.1),
                    ControlId::Shadow | ControlId::CropLeft | ControlId::CropRight | ControlId::CropTop | ControlId::CropBottom => (0.0,self.widget_range(id).map_or(100.0,|(_,m)|m),0.1),
                    ControlId::FollowZoom => (1.0,3.0,0.1),
                    ControlId::ScaleValue => (1.0,8.0,0.1),
                    ControlId::CanvasWidth | ControlId::CanvasHeight => (64.0,8192.0,2.0),
                    ControlId::StartValue | ControlId::EndValue => (0.0,self.selected_video_duration().0 as f64 / TICKS_PER_SECOND as f64,0.1),
                    _ => (0.0,self.widget_range(id).map_or(8192.0,|(_,m)|m),1.0),
                };
                input.numeric(min,max,step);
                self.inline_control = Some(id);
                self.inline_input = Some(input);
            }
            Err(error) => self.note_problem(
                "输入框未打开，请重新点击参数数值",
                format!("无法打开输入框：{error}"),
            ),
        }
    }
    pub(super) fn finish_inline_input(&mut self, hwnd: HWND, commit: bool) -> bool {
        let Some(input) = self.inline_input.as_ref() else {
            return true;
        };
        // Merely opening a rounded display value must not quantize the original
        // source timestamp, dirty the project or queue an unnecessary media seek.
        let apply = commit && input.is_changed();
        if apply {
            let value = input
                .value()
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite());
            let result = value
                .ok_or_else(|| "请输入有效数字".to_string())
                .and_then(|v| self.apply_widget_value(self.inline_control.unwrap(), v));
            if let Err(error) = result {
                self.inline_validation = Some(format!("{error}；按 Esc 可取消本次输入"));
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Warning,
                    format!("输入未应用：{error}"),
                );
                if let Some(input) = &self.inline_input {
                    unsafe {
                        let _ = SetFocus(Some(input.hwnd));
                    }
                }
                self.refresh(hwnd, true);
                return false;
            }
        }
        if !commit && self.inline_validation.is_some() {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "已取消输入，原值未改变".into(),
            );
        }
        self.inline_input = None;
        self.inline_control = None;
        self.inline_validation = None;
        if apply {
            let result = self.apply_editor_state();
            self.record_editor_result(result, "参数已更新 · 可撤销");
        }
        self.refresh(hwnd, true);
        true
    }
    #[allow(clippy::too_many_lines)] // Validated property dispatch; mutations all enter the shared editor history.
    fn apply_widget_value(&mut self, id: ControlId, v: f64) -> Result<(), String> {
        if !v.is_finite() {
            return Err("请输入有限数值".into());
        }
        let result = match id {
            ControlId::CanvasWidth | ControlId::CanvasHeight => {
                if !(64.0..=8192.0).contains(&v) || v.fract() != 0.0 || v % 2.0 != 0.0 {
                    return Err("请输入 64–8192 之间的偶数像素尺寸".into());
                }
                let mut size = self.canvas_size();
                if id == ControlId::CanvasWidth {
                    size.width = u32::try_from(v as i32).map_err(|e| e.to_string())?;
                } else {
                    size.height = u32::try_from(v as i32).map_err(|e| e.to_string())?;
                }
                self.editor.set_canvas_size(Some(size))
            }

            ControlId::CropLeft
            | ControlId::CropTop
            | ControlId::CropRight
            | ControlId::CropBottom => {
                let mut crop = self.selected_crop().ok_or("请先选择视频片段")?;
                let original = crop;
                let (_, max) = self.widget_range(id).ok_or("请先选择视频片段")?;
                if !(0.0..=max).contains(&v) {
                    return Err(format!("裁剪范围为 0–{max:.1}%"));
                }
                let edge = u16::try_from((v * 10.0).round() as i32).map_err(|e| e.to_string())?;
                match id {
                    ControlId::CropLeft => crop.left = edge,
                    ControlId::CropTop => crop.top = edge,
                    ControlId::CropRight => crop.right = edge,
                    _ => crop.bottom = edge,
                }
                if self.crop_locked {
                    let (handle, delta) = match id {
                        ControlId::CropLeft => (
                            (-1, 0),
                            (f64::from(crop.left) - f64::from(original.left), 0.0),
                        ),
                        ControlId::CropTop => (
                            (0, -1),
                            (0.0, f64::from(crop.top) - f64::from(original.top)),
                        ),
                        ControlId::CropRight => (
                            (1, 0),
                            (f64::from(original.right) - f64::from(crop.right), 0.0),
                        ),
                        _ => (
                            (0, 1),
                            (0.0, f64::from(original.bottom) - f64::from(crop.bottom)),
                        ),
                    };
                    crop = panzo_core::crop_gesture::drag_crop(
                        original,
                        handle,
                        (delta.0 / 1000.0, delta.1 / 1000.0),
                        true,
                    );
                }
                return self.set_selected_crop(crop);
            }
            ControlId::VideoSpeed => {
                if !(0.25..=4.0).contains(&v) {
                    return Err("视频速度范围为 0.25–4 倍".into());
                }
                let id = self.selected_video.as_ref().ok_or("请先选择视频片段")?;
                self.editor.set_video_speed(
                    id,
                    u16::try_from((v * 100.0).round() as i32).map_err(|e| e.to_string())?,
                )
            }
            ControlId::VideoScale => {
                if !(5.0..=400.0).contains(&v) { return Err("画面缩放范围为 5.0–400.0%".into()); }
                let id=self.selected_video.as_ref().ok_or("请先选择视频片段")?;
                self.editor.set_video_scale(id,(v*10.0).round() as u16)
            }
            ControlId::Inset => self.editor.set_canvas_inset(v.round() / self.canvas_pixel_basis()),
            ControlId::FollowZoom => {
                if !(1.0..=3.0).contains(&v) {return Err("放大倍数范围为 1–3".into());}
                let mut settings = self.editor.settings().clone();
                settings.automatic_camera.get_or_insert_with(Default::default).default.zoom = v;
                self.editor.replace_media_settings(settings)
            },
            ControlId::Radius => self.editor.set_corner_radius(
                v.round() / f64::from(
                    self.frame
                        .evaluation
                        .output
                        .width
                        .min(self.frame.evaluation.output.height),
                ),
            ),
            ControlId::Shadow => self.editor.set_shadow(v / 100.0),
            ControlId::StartValue | ControlId::EndValue | ControlId::ScaleValue => {
                if let Some(s) = self.selected_segment() {
                    if id != ControlId::ScaleValue {
                        if let Some((a,b))=self.editor.settings().follow_positions.get(&s.id).copied() {
                            let mut next=self.editor.settings().clone();let tick=checked_seconds(v)?;
                            next.follow_positions.insert(s.id.clone(),if id==ControlId::StartValue{(tick,b)}else{(a,tick)});
                            return self.editor.replace_media_settings(next).map_err(|e|e.to_string());
                        }
                    }
                    let tick = checked_seconds(v)?;
                    let start = if id == ControlId::StartValue {
                        tick
                    } else {
                        s.start_tick
                    };
                    let end = if id == ControlId::EndValue {
                        tick
                    } else {
                        s.end_tick
                    };
                    let target = if id == ControlId::ScaleValue {
                        if !(1.0..=8.0).contains(&v) {
                            return Err("倍率范围为 1–8".into());
                        }
                        CameraState::focused(s.focus_point(), v).map_err(|e| e.to_string())?
                    } else {
                        s.to
                    };
                    self.editor.update_segment(&s.id, start, end, target)
                } else {
                    let clip = self.selected_video_clip().ok_or("请先选择片段")?;
                    self.editor.trim_video(
                        &clip.id,
                        if id == ControlId::StartValue {
                            checked_seconds(v)?
                        } else {
                            clip.source_in_tick
                        },
                        if id == ControlId::EndValue {
                            checked_seconds(v)?
                        } else {
                            clip.source_out_tick
                        },
                    )
                }
            }
            _ => return Ok(()),
        };
        result.map_err(|error| self.edit_error(&error))
    }
    pub(super) fn begin_property_drag(
        &mut self,
        hwnd: HWND,
        id: ControlId,
        x: i32,
        y: i32,
    ) -> bool {
        if !matches!(
            id,
            ControlId::Inset
                | ControlId::Radius
                | ControlId::Shadow
                | ControlId::ZoomSlider
                | ControlId::VideoScale | ControlId::VideoSpeed
        ) && !id.is_crop_edge()
            || !self.control_enabled(id)
        {
            return false;
        }
        let rect = self
            .control_layout(super::client_rect(hwnd))
            .raw_control_rect(id);
        if id != ControlId::ZoomSlider && y < rect.top + self.theme.scale(27) {
            return false;
        }
        self.session.pause();
        self.deferred_playback.cancel();
        self.finish_wheel_edit();
        self.editor.begin_edit_group();
        self.property_drag = Some(id);
        unsafe {
            SetCapture(hwnd);
        }
        self.update_property_drag(hwnd, id, x);
        true
    }
    pub(super) fn update_property_drag(&mut self, hwnd: HWND, id: ControlId, x: i32) {
        let rect = self
            .control_layout(super::client_rect(hwnd))
            .raw_control_rect(id);
        let t = f64::from(x - rect.left - self.theme.scale(5))
            / f64::from((rect.right - rect.left - self.theme.scale(10)).max(1));
        let t = t.clamp(0.0, 1.0);
        if id == ControlId::ZoomSlider {
            self.timeline.set_zoom_fraction(self.session.project_playhead(), t);
            self.follow_playhead = false;
        } else {
            let Some((_, max)) = self.widget_range(id) else {
                return;
            };
            let value = if id == ControlId::VideoSpeed {
                (2.0_f64.powf(t * 4.0 - 2.0)*10.0).round().clamp(2.5,40.0)/10.0
            } else if id == ControlId::VideoScale {
                ( (5.0+t*395.0)*10.0).round()/10.0
            } else if id.is_crop_edge() || id==ControlId::Shadow {
                (t * max * 10.0).round() / 10.0
            } else {
                (t * max).round()
            };
            if let Err(error) = self.apply_widget_value(id, value) {
                self.set_status(crate::platform::editor_ui::StatusSeverity::Warning, error);
            }
            if self.last_drag_refresh.elapsed() >= DRAG_REFRESH_INTERVAL {
                let result = self.apply_editor_state();
                self.record_result(result);
            }
        }
        if self.last_drag_refresh.elapsed() >= DRAG_REFRESH_INTERVAL || self.property_drag.is_none()
        {
            self.refresh(hwnd, true);
            self.last_drag_refresh = Instant::now();
        }
    }
}
pub(super) fn format_widget_value(id: ControlId, value: f64) -> String {
    match id {
        ControlId::Inset | ControlId::Radius | ControlId::CanvasWidth | ControlId::CanvasHeight => format!("{value:.0}"),
        ControlId::StartValue | ControlId::EndValue => format!("{value:.3}"),
        _ => format!("{value:.1}"),
    }
}
fn checked_seconds(v: f64) -> Result<TimeTick, String> {
    if !v.is_finite() || v < 0.0 || v > (i64::MAX / TICKS_PER_SECOND) as f64 {
        return Err("时间必须为有效非负秒数".into());
    }
    Ok(TimeTick((v * TICKS_PER_SECOND as f64).round() as i64))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inline_time_rejects_nonfinite_and_negative() {
        for v in [f64::NAN, f64::INFINITY, -1.0] {
            assert!(checked_seconds(v).is_err());
        }
        assert_eq!(checked_seconds(1.25).unwrap().0, 12_500_000);
    }
    #[test]
    fn background_presets_match_approved_prototype() {
        assert_eq!(
            SWATCHES.map(swatch_color),
            [
                "#c5d5e7", "#f0d7c3", "#d8d1ef", "#383a46", "#f8f8f8", "#ccdace"
            ]
        );
    }
}
