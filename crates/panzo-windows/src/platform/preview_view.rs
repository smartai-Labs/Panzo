//! View-only zoom/pan. Never changes crop, camera, project history or export geometry.
use super::{HWND, HitRect, RECT, WindowState, client_rect, preview_video_rect};
use windows::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject, InvalidateRect, SetWindowRgn};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_SPACE,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PreviewView {
    pub zoom: f64,
    pan: [f64; 2],
}
impl Default for PreviewView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: [0.0; 2],
        }
    }
}
impl PreviewView {
    pub fn rect(self, base: RECT, well: RECT) -> RECT {
        let width = (f64::from(base.right - base.left) * self.zoom)
            .round()
            .max(1.0) as i32;
        let height = (f64::from(base.bottom - base.top) * self.zoom)
            .round()
            .max(1.0) as i32;
        let cx = f64::from(base.left + base.right) / 2.0
            + self.pan[0] * f64::from(well.right - well.left);
        let cy = f64::from(base.top + base.bottom) / 2.0
            + self.pan[1] * f64::from(well.bottom - well.top);
        let left = (cx - f64::from(width) / 2.0).round() as i32;
        let top = (cy - f64::from(height) / 2.0).round() as i32;
        RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }
    pub fn zoom_at(&mut self, base: RECT, well: RECT, x: i32, y: i32, factor: f64) {
        let old = self.rect(base, well);
        self.zoom = (self.zoom * factor).clamp(0.1, 8.0);
        let new = self.rect(base, well);
        let u = f64::from(x - old.left) / f64::from(old.right - old.left);
        let v = f64::from(y - old.top) / f64::from(old.bottom - old.top);
        self.pan[0] += (f64::from(x - new.left) - u * f64::from(new.right - new.left))
            / f64::from((well.right - well.left).max(1));
        self.pan[1] += (f64::from(y - new.top) - v * f64::from(new.bottom - new.top))
            / f64::from((well.bottom - well.top).max(1));
        self.constrain(base, well);
    }
    pub fn translate(&mut self, base: RECT, well: RECT, dx: i32, dy: i32) {
        self.pan[0] += f64::from(dx) / f64::from((well.right - well.left).max(1));
        self.pan[1] += f64::from(dy) / f64::from((well.bottom - well.top).max(1));
        self.constrain(base, well);
    }
    fn constrain(&mut self, base: RECT, well: RECT) {
        let view = self.rect(base, well);
        for (i, start, end, low, high) in [
            (0, view.left, view.right, well.left, well.right),
            (1, view.top, view.bottom, well.top, well.bottom),
        ] {
            // Keep a visible, grabbable portion inside the well at every zoom.
            let visible = 24.min((end - start) / 2).min((high - low) / 2).max(1);
            let shift = if end < low + visible {
                low + visible - end
            } else if start > high - visible {
                high - visible - start
            } else {
                0
            };
            self.pan[i] += f64::from(shift) / f64::from((high - low).max(1));
        }
    }
}

impl WindowState {
    pub(super) fn preview_rect(&self, client: RECT) -> RECT {
        let mut layout=self.control_layout(client);layout.preview_well=self.preview_clip_rect(client);
        self.preview_view.rect(preview_video_rect(layout, &self.frame, self.theme),self.preview_clip_rect(client))
    }
    pub(super) fn refresh_preview_view(&mut self, hwnd: HWND) {
        self.resize_preview_surface(hwnd);
        let c = self.control_layout(client_rect(hwnd));
        unsafe {
            let _ = InvalidateRect(Some(hwnd), Some(&c.preview_well), false);
            let _ = InvalidateRect(Some(hwnd), Some(&c.preview_meta), false);
        }
    }
    pub(super) fn can_pan_preview(&self, client: RECT, x: i32, y: i32) -> bool {
        HitRect(self.control_layout(client).preview_well).contains(x, y)
            && ((!self.crop_editing
                && (self.selected_segment.is_none()
                    || self.preview_view != PreviewView::default()))
                || unsafe {
                    GetKeyState(i32::from(VK_CONTROL.0)) < 0
                        || GetKeyState(i32::from(VK_SPACE.0)) < 0
                })
    }
    pub(super) fn begin_preview_pan(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        if self.navigation_authorized
            || self.crop_drag.is_some()
            || self.canvas_drag.is_some()
            || !HitRect(self.control_layout(client_rect(hwnd)).preview_well).contains(x, y)
        {
            return false;
        }
        self.preview_pan = Some((x, y, self.preview_view));
        self.tooltip_visible = false;
        self.tooltip_deadline = None;
        unsafe {
            SetCapture(hwnd);
        }
        self.update_cursor(hwnd, x, y);
        true
    }
    pub(super) fn move_preview_pan(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some((start_x, start_y, original)) = self.preview_pan else {
            return false;
        };
        let client = client_rect(hwnd);
        self.preview_view = original;
        self.preview_view.translate(
            preview_video_rect(self.control_layout(client), &self.frame, self.theme),
            self.control_layout(client).preview_well,
            x - start_x,
            y - start_y,
        );
        self.refresh_preview_view(hwnd);
        self.update_cursor(hwnd, x, y);
        true
    }
    pub(super) fn end_preview_pan(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        if !self.move_preview_pan(hwnd, x, y) {
            return false;
        }
        self.preview_pan = None;
        unsafe {
            let _ = ReleaseCapture();
        }
        self.update_cursor(hwnd, x, y);
        true
    }
    pub(super) fn update_splitter_hover(&mut self, hwnd: HWND, x: i32, y: i32) {
        let c = self.control_layout(client_rect(hwnd));
        let hover = c.splitter_at(x, y).map(|(id, _)| id);
        if hover != self.splitter_hover {
            let previous = self.splitter_hover;
            self.splitter_hover = hover;
            for (id, splitter) in c.splitters() {
                if Some(id) == previous || Some(id) == hover {
                    unsafe { let _ = InvalidateRect(Some(hwnd), Some(&splitter.bounds), false); }
                }
            }
        }
    }
}

pub(super) fn clip_surface(hwnd: HWND, video: RECT, well: RECT) {
    let left = video.left.max(well.left);
    let top = video.top.max(well.top);
    let right = video.right.min(well.right).max(left);
    let bottom = video.bottom.min(well.bottom).max(top);
    unsafe {
        let region = CreateRectRgn(
            left - video.left,
            top - video.top,
            right - video.left,
            bottom - video.top,
        );
        if !region.is_invalid() && SetWindowRgn(hwnd, Some(region), false) == 0 {
            let _ = DeleteObject(region.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zoom_preserves_pointer_anchor_pan_scales_with_window_and_reset_fits() {
        for scale in [1, 2, 3] {
            let base = RECT {
                left: 100 * scale,
                top: 100 * scale,
                right: 900 * scale,
                bottom: 550 * scale,
            };
            let well = RECT {
                left: 0,
                top: 0,
                right: 1000 * scale,
                bottom: 650 * scale,
            };
            let mut view = PreviewView::default();
            let x = 650 * scale;
            let y = 270 * scale;
            view.zoom_at(base, well, x, y, 2.0);
            let r = view.rect(base, well);
            assert!((f64::from(x - r.left) / f64::from(r.right - r.left) - 0.6875).abs() < 0.002);
            assert!(
                (f64::from(y - r.top) / f64::from(r.bottom - r.top) - 170.0 / 450.0).abs() < 0.002
            );
            view.translate(base, well, 50 * scale, 20 * scale);
            let moved = view.rect(base, well);
            assert_eq!(
                (moved.left - r.left, moved.top - r.top),
                (50 * scale, 20 * scale)
            );
            view.zoom_at(base, well, x, y, 1e9);
            assert_eq!(view.zoom, 8.0);
            view.translate(base, well, 1000000, -1000000);
            let r = view.rect(base, well);
            assert!(r.left < well.right && r.bottom > well.top);
            view.zoom_at(base, well, x, y, 1e-9);
            assert_eq!(view.zoom, 0.1);
            view = PreviewView::default();
            assert_eq!(view.rect(base, well), base);
        }
    }
}
