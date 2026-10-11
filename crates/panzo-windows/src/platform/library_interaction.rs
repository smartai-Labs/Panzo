//! Asset-library-only scrolling and card reflow. Native list data/keyboard behavior is retained.
use super::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};

pub(super) struct Reflow {
    from: HashMap<usize, RECT>,
    start: Instant,
    progress: f64,
}
struct Scrollbar {
    track: RECT,
    thumb: RECT,
    maximum: i32,
    position: i32,
    page: i32,
}
impl History {
    pub(super) fn move_inspector_drag(&mut self, parent: HWND, point: POINT) {
        let Some((drag, _)) = self.splitter_drag else {
            return;
        };
        let width = (drag.size_at(point.x, point.y) * 96 / self.dpi as i32).clamp(264, 640);
        if self.inspector_width == Some(width) {
            return;
        }
        self.inspector_width = Some(width);
        self.layout(parent);
        // Finish this local layout before another pointer event moves the native
        // surfaces; card reflow still uses the existing animation timer.
        unsafe {
            let _ = RedrawWindow(Some(parent), None, None, RDW_UPDATENOW | RDW_ALLCHILDREN);
        }
    }
    pub(super) fn finish_inspector_drag(&mut self, parent: HWND, canceled: bool) {
        let Some((drag, original_width)) = self.splitter_drag.take() else {
            return;
        };
        if canceled {
            self.stop_reflow();
            self.inspector_width = original_width;
            self.layout(parent);
            unsafe {
                let _ = RedrawWindow(Some(parent), None, None, RDW_UPDATENOW | RDW_ALLCHILDREN);
            }
        }
        // Clear the state before releasing capture (WM_CAPTURECHANGED is synchronous).
        drag.finish(self.splitter);
        ui::invalidate(self.splitter);
    }

    pub(super) fn grid_width(&self) -> i32 {
        (controls::rect(self.grid).right - self.dip(14)).max(1)
    }
    pub(super) fn visual_grid_rect(&self, index: usize) -> RECT {
        let target = self.grid_rect(index);
        let Some(animation) = &self.reflow else {
            return target;
        };
        let Some(from) = animation.from.get(&index) else {
            return target;
        };
        let mix = |a: i32, b: i32| (a as f64 + (b - a) as f64 * animation.progress).round() as i32;
        RECT {
            left: mix(from.left, target.left),
            top: mix(from.top, target.top),
            right: mix(from.right, target.right),
            bottom: mix(from.bottom, target.bottom),
        }
    }
    pub(super) fn grid_paint_range(&self) -> std::ops::Range<usize> {
        let columns = self.grid_columns();
        let first = (self.grid_scroll / self.grid_row()).max(0) as usize * columns;
        let count =
            (controls::rect(self.grid).bottom / self.grid_row() + 3).max(1) as usize * columns;
        let mut range = first.min(self.visible.len())..(first + count).min(self.visible.len());
        if let Some(animation) = &self.reflow {
            for &i in animation.from.keys() {
                range.start = range.start.min(i);
                range.end = range.end.max(i + 1).min(self.visible.len());
            }
        }
        range
    }
    pub(super) fn capture_grid_cells(&self) -> HashMap<usize, RECT> {
        if self.splitter_drag.is_none() || !self.grid_mode {
            return HashMap::new();
        }
        let height = controls::rect(self.grid).bottom;
        self.grid_paint_range()
            .filter_map(|i| {
                let r = self.visual_grid_rect(i);
                (r.bottom >= -self.grid_row() && r.top <= height + self.grid_row())
                    .then_some((i, r))
            })
            .collect()
    }
    pub(super) fn start_reflow(&mut self, previous_columns: usize, from: HashMap<usize, RECT>) {
        if self.grid_mode
            && self.splitter_drag.is_some()
            && previous_columns != self.grid_columns()
            && !from.is_empty()
            && crate::platform::system_preferences::animations()
        {
            self.reflow = Some(Reflow {
                from,
                start: Instant::now(),
                progress: 0.,
            });
            unsafe {
                SetTimer(Some(self.grid), 82, 16, None);
            }
        }
    }
    pub(super) fn advance_reflow(&mut self) {
        if let Some(animation) = &mut self.reflow {
            let t = (animation.start.elapsed().as_secs_f64() / 0.16).clamp(0., 1.);
            animation.progress = 1. - (1. - t).powi(3);
            if t < 1. && crate::platform::system_preferences::animations() {
                return;
            }
        }
        self.stop_reflow();
    }
    pub(super) fn stop_reflow(&mut self) {
        self.reflow = None;
        unsafe {
            let _ = KillTimer(Some(self.grid), 82);
        }
    }
    fn scrollbar(&self, surface: HWND) -> Scrollbar {
        let r = controls::rect(surface);
        let (maximum, position, page) = if surface == self.grid {
            (self.grid_max(), self.grid_scroll, r.bottom)
        } else {
            let page = (r.bottom / self.item_dip(58)).max(1);
            let top = unsafe { SendMessageW(self.list, LB_GETTOPINDEX, None, None) }
                .0
                .max(0) as i32;
            ((self.visible.len() as i32 - page).max(0), top, page)
        };
        let track = ui::rect(
            r.right - self.dip(12),
            self.dip(4),
            self.dip(12),
            (r.bottom - self.dip(8)).max(1),
        );
        let height = track.bottom - track.top;
        let thumb_height = ((height as i64 * page.max(1) as i64 / (maximum + page).max(1) as i64)
            as i32)
            .clamp(self.dip(28).min(height), height);
        let y = track.top
            + ((height - thumb_height) as i64 * position as i64 / maximum.max(1) as i64) as i32;
        Scrollbar {
            track,
            thumb: ui::rect(
                track.left + self.dip(3),
                y,
                self.dip(5).max(3),
                thumb_height,
            ),
            maximum,
            position,
            page,
        }
    }
    pub(super) fn paint_scrollbar(&self, surface: HWND, dc: HDC) {
        let bar = self.scrollbar(surface);
        if bar.maximum == 0 {
            return;
        }
        let t = editor_ui::EditorTheme::current(self.dpi);
        super::super::fill(dc, bar.track, t.palette.panel);
        let active = self.scroll_drag.is_some_and(|(h, _)| h == surface);
        let ink = if active {
            t.palette.accent
        } else {
            t.palette.text_muted
        };
        editor_ui::rounded_surface(dc, bar.thumb, ink, ink, t.scale(3));
    }
    fn scroll_to(&mut self, surface: HWND, position: i32) {
        let bar = self.scrollbar(surface);
        let next = position.clamp(0, bar.maximum);
        if next == bar.position {
            return;
        }
        self.stop_reflow();
        if surface == self.grid {
            self.grid_scroll = next;
        } else {
            unsafe {
                // LB_SETTOPINDEX otherwise uses immediate ScrollWindow copies, which
                // move the custom thumb with the rows before its buffered repaint.
                SendMessageW(surface, WM_SETREDRAW, Some(WPARAM(0)), None);
                SendMessageW(surface, LB_SETTOPINDEX, Some(WPARAM(next as usize)), None);
                SendMessageW(surface, WM_SETREDRAW, Some(WPARAM(1)), None);
            }
        }
        self.request_thumbs();
        ui::invalidate(surface);
        unsafe {
            let _ = RedrawWindow(Some(surface), None, None, RDW_UPDATENOW | RDW_NOERASE);
        }
    }
    pub(super) fn handle_scroll_input(&mut self, h: HWND, m: u32, w: WPARAM, l: LPARAM) -> bool {
        if !matches!(
            m,
            WM_MOUSEWHEEL
                | WM_LBUTTONDOWN
                | WM_MOUSEMOVE
                | WM_LBUTTONUP
                | WM_CAPTURECHANGED
                | WM_VSCROLL
        ) {
            return false;
        }
        let bar = self.scrollbar(h);
        let x = l.0 as u16 as i16 as i32;
        let y = (l.0 >> 16) as u16 as i16 as i32;
        match m {
            WM_MOUSEWHEEL => {
                let delta = (w.0 >> 16) as u16 as i16 as i32;
                if w.0 & 8 != 0 {
                    if let Ok(parent) = unsafe { GetParent(h) } {
                        self.stop_reflow();
                        self.zoom_items(parent, delta);
                    }
                } else {
                    self.wheel_remainder += delta;
                    let steps = self.wheel_remainder / 120;
                    self.wheel_remainder %= 120;
                    if steps != 0 {
                        self.scroll_to(
                            h,
                            bar.position - steps * if h == self.grid { self.dip(90) } else { 3 },
                        );
                    }
                }
                true
            }
            WM_LBUTTONDOWN if bar.maximum > 0 && x >= bar.track.left => {
                if y >= bar.thumb.top && y < bar.thumb.bottom {
                    self.scroll_drag = Some((h, y - bar.thumb.top));
                    unsafe {
                        let _ = SetCapture(h);
                    }
                    ui::invalidate(h);
                } else {
                    self.scroll_to(
                        h,
                        bar.position
                            + if y < bar.thumb.top {
                                -bar.page
                            } else {
                                bar.page
                            },
                    );
                }
                true
            }
            WM_MOUSEMOVE if self.scroll_drag.is_some_and(|(surface, _)| surface == h) => {
                let offset = self.scroll_drag.unwrap().1;
                let travel =
                    (bar.track.bottom - bar.track.top - (bar.thumb.bottom - bar.thumb.top)).max(1);
                let position = ((y - bar.track.top - offset) as i64 * bar.maximum as i64
                    / travel as i64) as i32;
                self.scroll_to(h, position);
                true
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED
                if self.scroll_drag.is_some_and(|(surface, _)| surface == h) =>
            {
                self.scroll_drag = None;
                if m == WM_LBUTTONUP {
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                }
                ui::invalidate(h);
                true
            }
            WM_VSCROLL => {
                let next = match SCROLLBAR_COMMAND(w.0 as u16 as i32) {
                    SB_LINEUP => bar.position - 1,
                    SB_LINEDOWN => bar.position + 1,
                    SB_PAGEUP => bar.position - bar.page,
                    SB_PAGEDOWN => bar.position + bar.page,
                    SB_TOP => 0,
                    SB_BOTTOM => bar.maximum,
                    SB_THUMBTRACK | SB_THUMBPOSITION => (w.0 >> 16) as i32,
                    _ => bar.position,
                };
                self.scroll_to(h, next);
                true
            }
            _ => false,
        }
    }
    pub(super) fn paint_list(&self, dc: HDC) {
        let r = controls::rect(self.list);
        let theme = editor_ui::EditorTheme::current(self.dpi);
        super::super::fill(dc, r, theme.palette.panel);
        let top = unsafe { SendMessageW(self.list, LB_GETTOPINDEX, None, None) }
            .0
            .max(0) as usize;
        let selected = unsafe { SendMessageW(self.list, LB_GETCURSEL, None, None) }.0;
        let row = self.item_dip(58);
        let count = (r.bottom / row + 1).max(0) as usize;
        for i in top..(top + count).min(self.visible.len()) {
            let item = windows::Win32::UI::Controls::DRAWITEMSTRUCT {
                itemID: i as u32,
                itemState: if i as isize == selected {
                    windows::Win32::UI::Controls::ODS_SELECTED
                } else {
                    Default::default()
                },
                hwndItem: self.list,
                hDC: dc,
                rcItem: ui::rect(0, (i - top) as i32 * row, r.right - self.dip(14), row),
                ..Default::default()
            };
            self.draw_item(&item);
        }
        self.paint_scrollbar(self.list, dc);
    }
}

#[cfg(test)]
impl History {
    pub(super) fn review_scrollbar(&self, h: HWND) -> (RECT, i32, i32) {
        let bar = self.scrollbar(h);
        (bar.thumb, bar.maximum, bar.position)
    }
}
