//! Shared pane divider geometry, painting and pointer drag state.
//! Pages own sizing policy and schedule layout updates; this component owns the
//! visible boundary and the interaction band, in the same coordinate system.
use super::editor_ui::{EditorTheme, fill_rect, rounded_surface};
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::HDC,
        UI::{
            Input::KeyboardAndMouse::SetCapture,
            WindowsAndMessaging::{IDC_SIZENS, IDC_SIZEWE},
        },
    },
    core::PCWSTR,
};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Axis {
    Vertical,
    Horizontal,
}

/// Location of the resized pane relative to the divider.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Side {
    Before,
    After,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Splitter {
    pub(crate) bounds: RECT,
    axis: Axis,
    side: Side,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Drag {
    axis: Axis,
    side: Side,
    origin: i32,
    initial_size: i32,
    pub(crate) restore: i32,
}

impl Splitter {
    pub(crate) fn new(bounds: RECT, axis: Axis, side: Side) -> Self {
        Self { bounds, axis, side }
    }
    pub(crate) fn contains(self, x: i32, y: i32) -> bool {
        let r = self.bounds;
        x >= r.left && x < r.right && y >= r.top && y < r.bottom
    }
    pub(crate) fn cursor(self) -> PCWSTR {
        cursor(self.axis)
    }
    fn drag(self, x: i32, y: i32, size: i32, restore: i32) -> Drag {
        Drag {
            axis: self.axis,
            side: self.side,
            origin: coordinate(self.axis, x, y),
            initial_size: size,
            restore,
        }
    }
    pub(crate) fn begin(self, hwnd: HWND, x: i32, y: i32, size: i32, restore: i32) -> Drag {
        unsafe {
            SetCapture(hwnd);
        }
        self.drag(x, y, size, restore)
    }
    /// Put the stroke at the edge adjoining the preview. A center stroke plus
    /// the preview background's edge otherwise reads as two parallel dividers.
    fn stroke(self, thickness: i32) -> RECT {
        let mut r = self.bounds;
        match (self.axis, self.side) {
            (Axis::Vertical, Side::Before) => r.left = (r.right - thickness).max(r.left),
            (Axis::Vertical, Side::After) => r.right = (r.left + thickness).min(r.right),
            (Axis::Horizontal, Side::Before) => r.top = (r.bottom - thickness).max(r.top),
            (Axis::Horizontal, Side::After) => r.bottom = (r.top + thickness).min(r.bottom),
        }
        r
    }
    pub(crate) fn paint(self, dc: HDC, theme: EditorTheme, hovered: bool, dragging: bool) {
        let r = self.bounds;
        if r.right <= r.left || r.bottom <= r.top {
            return;
        }
        let active = hovered || dragging;
        let p = theme.palette;
        fill_rect(dc, &r, p.panel);
        fill_rect(
            dc,
            &self.stroke(theme.scale(1).max(1)),
            if active { p.accent } else { p.divider },
        );
        // The grip stays inside the hit band, including fractional DPI scales.
        let (w, h) = match self.axis {
            Axis::Vertical => (theme.scale(3), theme.scale(36)),
            Axis::Horizontal => (theme.scale(36), theme.scale(3)),
        };
        let w = w.min(r.right - r.left).max(1);
        let h = h.min(r.bottom - r.top).max(1);
        let left = r.left + (r.right - r.left - w) / 2;
        let top = r.top + (r.bottom - r.top - h) / 2;
        let color = if active { p.accent } else { p.text_muted };
        rounded_surface(
            dc,
            RECT {
                left,
                top,
                right: left + w,
                bottom: top + h,
            },
            color,
            color,
            theme.scale(2),
        );
    }
}
impl Drag {
    pub(crate) fn size_at(self, x: i32, y: i32) -> i32 {
        let delta = coordinate(self.axis, x, y) - self.origin;
        self.initial_size
            + match self.side {
                Side::Before => delta,
                Side::After => -delta,
            }
    }
    pub(crate) fn cursor(self) -> PCWSTR {
        cursor(self.axis)
    }
    /// The caller must take the drag out of its state before releasing capture.
    pub(crate) fn finish(self, hwnd: HWND) {
        super::desktop_ui::release_capture_owned_by(hwnd);
    }
}
fn coordinate(axis: Axis, x: i32, y: i32) -> i32 {
    match axis {
        Axis::Vertical => x,
        Axis::Horizontal => y,
    }
}
fn cursor(axis: Axis) -> PCWSTR {
    match axis {
        Axis::Vertical => IDC_SIZEWE,
        Axis::Horizontal => IDC_SIZENS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn divider_drag_follows_pane_side_and_keeps_restore_value() {
        for (axis, side, x, y, expected) in [
            (Axis::Vertical, Side::Before, 130, 900, 230),
            (Axis::Vertical, Side::After, 130, 900, 170),
            (Axis::Horizontal, Side::After, 900, 130, 170),
        ] {
            let d = Splitter::new(RECT::default(), axis, side).drag(100, 100, 200, 160);
            assert_eq!(d.size_at(x, y), expected);
            assert_eq!(d.size_at(100, 100), 200);
            assert_eq!(d.restore, 160);
        }
    }
    #[test]
    fn divider_stroke_and_hit_band_stay_inside_bounds_at_all_dpi() {
        for dpi in [96, 120, 144, 192] {
            let scale = |n: i32| (n * dpi + 48) / 96;
            for axis in [Axis::Vertical, Axis::Horizontal] {
                for side in [Side::Before, Side::After] {
                    let (w, h) = match axis {
                        Axis::Vertical => (scale(6), scale(400)),
                        Axis::Horizontal => (scale(400), scale(6)),
                    };
                    let r = RECT {
                        left: 20,
                        top: 30,
                        right: 20 + w,
                        bottom: 30 + h,
                    };
                    let s = Splitter::new(r, axis, side);
                    let line = s.stroke(scale(1));
                    assert!(s.contains(r.left, r.top));
                    assert!(!s.contains(r.right, r.top));
                    assert!(!s.contains(r.left, r.bottom));
                    assert!(
                        line.left >= r.left
                            && line.right <= r.right
                            && line.top >= r.top
                            && line.bottom <= r.bottom
                    );
                    assert_eq!(
                        match axis {
                            Axis::Vertical => line.right - line.left,
                            Axis::Horizontal => line.bottom - line.top,
                        },
                        scale(1)
                    );
                }
            }
        }
        assert!(!Splitter::new(RECT::default(), Axis::Vertical, Side::Before).contains(0, 0));
    }
}
