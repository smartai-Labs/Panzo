use super::{
    CameraState, HWND, PreparedPreviewFrame, RECT, ReleaseCapture, SetCapture, WindowState,
    client_rect,
};

#[derive(Clone, Copy)]
pub(super) struct CropDrag {
    original: panzo_core::VideoCrop,
    edge: (i8, i8),
    start: (i32, i32),
    source_rect: RECT,
}

impl WindowState {
    pub(super) fn toggle_crop_editing(&mut self) {
        self.set_crop_editing(!self.crop_editing);
    }

    pub(super) fn set_crop_editing(&mut self, enabled: bool) {
        if enabled && self.session.is_asset_preview() {
            if let Err(error)=self.select_preview_source(false) { self.record_result(Err(error));return; }
        }
        if enabled && self.selected_crop().is_none() {
            return;
        }
        self.crop_editing = enabled;
        self.session.set_crop_editing(self.crop_editing);
        let target=self.selected_video.as_ref().filter(|id|self.crop_editing&&self.editor.settings().visual_clips.iter().any(|c|&c.id==*id)).cloned();
        self.session.set_crop_target(target);
        if self.crop_editing {
            if let Some(bin) = &self.project_bin {
                super::project_bin::select_preview(bin, false);
            }
            self.deferred_playback.cancel();
            self.session.pause();
            self.selected_segment = None;
            if let Some(crop) = self.selected_crop() {
                let _ = self.set_selected_crop(crop);
            }
        }
        let result = self.apply_editor_state();
        self.record_editor_result(
            result,
            if self.crop_editing {
                "拖动边角或选区调整裁剪 · Enter 完成"
            } else {
                "裁剪调整完成"
            },
        );
    }

    fn crop_content_rect(&self,frame:&PreparedPreviewFrame)->panzo_core::SourceRect {
        let mut evaluation=frame.evaluation;let content_scale=evaluation.camera_transform.content_scale;evaluation.camera_transform=CameraState::BASE.into();evaluation.camera_transform.content_scale=content_scale;
        if let Some(clip)=self.selected_video.as_ref().and_then(|id|self.editor.settings().visual_clips.iter().find(|c|&c.id==id)) {
            let size=self.editor.settings().visual_assets.iter().find(|a|a.id==clip.asset_id)
                .map_or(frame.source_size(),|a|(a.width,a.height));
            evaluation.camera_transform.content_scale=1.0;
            let r=evaluation.content_rect(size,self.editor.settings().canvas.inset);
            let w=(r.right-r.left)*clip.scale;let h=(r.bottom-r.top)*clip.scale;
            return panzo_core::SourceRect{left:clip.x-w/2.,right:clip.x+w/2.,top:clip.y-h/2.,bottom:clip.y+h/2.};
        }
        evaluation.content_rect(frame.source_size(),self.editor.settings().canvas.inset)
    }

    pub(super) fn crop_overlay(&self, frame: &PreparedPreviewFrame) -> Option<[f64; 4]> {
        if !self.crop_editing {
            return None;
        }
        let crop = self.selected_crop()?;
        let rect = self.crop_content_rect(frame);
        let w = rect.right - rect.left;
        let h = rect.bottom - rect.top;
        Some([
            rect.left + w * f64::from(crop.left) / 1000.0,
            rect.top + h * f64::from(crop.top) / 1000.0,
            rect.right - w * f64::from(crop.right) / 1000.0,
            rect.bottom - h * f64::from(crop.bottom) / 1000.0,
        ])
    }

    fn crop_source_rect(&self, client: RECT) -> RECT {
        let video = self.preview_rect(client);
        let content = self.crop_content_rect(&self.frame);
        let w = f64::from(video.right - video.left);
        let h = f64::from(video.bottom - video.top);
        RECT {
            left: video.left + (w * content.left).round() as i32,
            top: video.top + (h * content.top).round() as i32,
            right: video.left + (w * content.right).round() as i32,
            bottom: video.top + (h * content.bottom).round() as i32,
        }
    }

    pub(super) fn crop_hit(&self, client: RECT, x: i32, y: i32) -> Option<(i8, i8)> {
        if !self.crop_editing
            || !super::HitRect(self.control_layout(client).preview_well).contains(x, y)
        {
            return None;
        }
        let crop = self.selected_crop()?;
        let source = self.crop_source_rect(client);
        let w = source.right - source.left;
        let h = source.bottom - source.top;
        let left = source.left + (f64::from(w) * f64::from(crop.left) / 1000.0).round() as i32;
        let right = source.right - (f64::from(w) * f64::from(crop.right) / 1000.0).round() as i32;
        let top = source.top + (f64::from(h) * f64::from(crop.top) / 1000.0).round() as i32;
        let bottom =
            source.bottom - (f64::from(h) * f64::from(crop.bottom) / 1000.0).round() as i32;
        let margin = self.theme.scale(9);
        if x < left - margin || x > right + margin || y < top - margin || y > bottom + margin {
            return None;
        }
        let edge = |v: i32, a: i32, b: i32| {
            if (v - a).abs() <= margin {
                -1
            } else {
                i8::from((v - b).abs() <= margin)
            }
        };
        Some((edge(x, left, right), edge(y, top, bottom)))
    }

    pub(super) fn begin_crop_drag(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some(edge) = self.crop_hit(client_rect(hwnd), x, y) else {
            return false;
        };
        let Some(original) = self.selected_crop() else {
            return false;
        };
        self.session.pause();
        self.deferred_playback.cancel();
        self.editor.begin_edit_group();
        self.crop_drag = Some(CropDrag {
            original,
            edge,
            start: (x, y),
            source_rect: self.crop_source_rect(client_rect(hwnd)),
        });
        unsafe {
            SetCapture(hwnd);
        }
        true
    }

    pub(super) fn update_crop_drag(&mut self, hwnd: HWND, x: i32, y: i32) {
        let Some(drag) = self.crop_drag else {
            return;
        };
        let dx = f64::from(x - drag.start.0)
            / f64::from((drag.source_rect.right - drag.source_rect.left).max(1));
        let dy = f64::from(y - drag.start.1)
            / f64::from((drag.source_rect.bottom - drag.source_rect.top).max(1));
        let crop = panzo_core::crop_gesture::drag_crop(
            drag.original,
            drag.edge,
            (dx, dy),
            self.crop_locked,
        );
        if self.selected_crop() == Some(crop) {
            return;
        }
        if let Err(error) = self.set_selected_crop(crop) {
            self.set_status(crate::platform::editor_ui::StatusSeverity::Warning, error);
            return;
        }
        // Only the editing overlay changes while dragging; reuse the already decoded full frame.
        self.session
            .set_crop_overlay(self.crop_overlay(&self.frame));
        if let Some(surface) = &self.preview_surface {
            let result = self
                .session
                .present_prepared(&surface.presenter, &self.frame, None);
            self.record_session_result(result);
        } else if let Ok(frame) = self.session.compose_prepared(&self.frame) {
            self.fallback_frame = Some(frame);
        }
        // Geometry is stable during a drag. Do not refresh the project bin, child
        // layout, accessibility tree or the rest of the inspector on every move.
        let layout = self.control_layout(client_rect(hwnd));
        for rect in layout.crop_edges.iter().map(|r| r.0)
            .chain(self.preview_surface.is_none().then_some(layout.preview_well))
        {
            if rect.right > rect.left && rect.bottom > rect.top {
                unsafe { let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), Some(&rect), false); }
            }
        }
    }

    pub(super) fn finish_crop_drag(&mut self, hwnd: HWND, cancel: bool) {
        if self.crop_drag.take().is_none() {
            return;
        }
        unsafe {
            let _ = ReleaseCapture();
        }
        if cancel {
            self.editor.cancel_edit_group();
        } else {
            self.editor.finish_edit_group();
        }
        let result = self.apply_editor_state();
        self.record_result(result);
        self.refresh(hwnd, true);
    }
}
