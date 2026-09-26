use super::{
    ControlId, ControlLayout, HWND, OutputDescriptor, PreviewSessionError, PreviewWindowError,
    TimeTick, WindowState, client_rect,
};
use panzo_core::CanvasSize;

impl WindowState {
    pub(super) fn canvas_size(&self) -> CanvasSize {
        self.editor.settings().canvas.size.unwrap_or_else(|| {
            let (width, height) = self.frame.source_size();
            CanvasSize { width, height }
        })
    }

    pub(super) fn set_canvas_preset(&mut self, id: ControlId) {
        let ratio = match id {
            ControlId::CanvasWide => Some((16, 9)),
            ControlId::CanvasPortrait => Some((9, 16)),
            ControlId::CanvasSquare => Some((1, 1)),
            _ => None,
        };
        let size = ratio.map(|ratio| CanvasSize::from_ratio(self.frame.source_size(), ratio));
        let result = self
            .editor
            .set_canvas_size(size)
            .map_err(PreviewWindowError::from)
            .and_then(|()| self.apply_editor_state());
        self.record_editor_result(result, "画布尺寸已更新");
    }

    pub(super) fn update_canvas_output(&mut self) -> Result<(), PreviewWindowError> {
        let size = self.canvas_size();
        self.session.set_output(
            OutputDescriptor::bgra8(size.width, size.height).map_err(PreviewSessionError::from)?,
        );
        Ok(())
    }

    pub(super) fn step_frame(&mut self, forward: bool) {
        if self.crop_editing {
            self.toggle_crop_editing();
        }
        self.seek_absolute(self.session.frame_step_tick(forward));
    }

    pub(super) fn seek_clip_boundary(&mut self, forward: bool) {
        if self.crop_editing {
            self.toggle_crop_editing();
        }
        let tick = self.session.snapshot().project_tick;
        let video = self.editor.video_edit();
        let target = if forward {
            video
                .spans()
                .map(|span| span.project_out)
                .find(|at| *at > tick)
                .unwrap_or(video.duration())
        } else {
            video
                .spans()
                .map(|span| span.project_in)
                .filter(|at| *at < tick)
                .last()
                .unwrap_or(TimeTick::ZERO)
        };
        self.seek_absolute(target);
    }

    pub(super) fn persist_layout(&mut self) {
        if self.close_at.is_some() {
            return;
        }
        let dpi = i32::try_from(self.theme.dpi).unwrap_or(96).max(1);
        let layout = crate::platform::ui_preferences::LayoutPreferences {
            inspector_width: self.inspector_width_dip,
            inspector_visible: self.theme.metrics.inspector_width > 0,
            timeline_height: self.theme.metrics.timeline_height * 96 / dpi,
            video_collapsed: self.video_collapsed,
        };
        if let Err(error) = crate::platform::ui_preferences::set_layout(layout) {
            self.note_problem(
                "当前布局可继续使用，但未记住；下次启动可能恢复旧布局",
                format!("布局暂未保存：{error}"),
            );
        }
    }

    pub(super) fn restore_layout(&mut self) {
        if self.close_at.is_some() {
            return;
        }
        let layout = crate::platform::ui_preferences::layout();
        self.inspector_width_dip = layout.inspector_width;
        self.theme.metrics.inspector_width = if layout.inspector_visible {
            self.theme.scale(layout.inspector_width)
        } else {
            0
        };
        self.theme.metrics.timeline_height = self.theme.scale(layout.timeline_height);
        self.video_collapsed = layout.video_collapsed;
    }

    pub(super) fn reveal_inspector_control(&mut self, hwnd: HWND, id: ControlId) {
        if !ControlLayout::is_inspector_control(id) {
            return;
        }
        let layout = self.control_layout(client_rect(hwnd));
        let rect = layout.raw_control_rect(id);
        let shift = if rect.top < layout.inspector_body_top {
            rect.top - layout.inspector_body_top - self.theme.scale(8)
        } else if rect.bottom > layout.panel.bottom - self.theme.scale(8) {
            rect.bottom - layout.panel.bottom + self.theme.scale(8)
        } else {
            0
        };
        self.inspector_scroll =
            (layout.inspector_scroll + shift).clamp(0, layout.inspector_scroll_max);
    }
}
