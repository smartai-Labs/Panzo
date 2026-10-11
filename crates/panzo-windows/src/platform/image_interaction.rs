//! Per-document viewport and non-destructive crop gestures. Rectangles use exclusive right/bottom.
use super::{POINT, RECT};
#[derive(Clone, Copy, Default)]
pub(super) struct Viewport {
    pub zoom: Option<f64>,
    pub dpi: u32,
    pub pan: (f64, f64),
}
impl Viewport {
    pub fn scale(self, area: RECT, size: (u32, u32)) -> f64 {
        self.zoom.unwrap_or_else(|| {
            ((area.right - area.left - (50 * self.dpi.max(96) / 96) as i32).max(1) as f64
                / size.0.max(1) as f64)
                .min(
                    (area.bottom - area.top - (50 * self.dpi.max(96) / 96) as i32).max(1) as f64
                        / size.1.max(1) as f64,
                )
                .min(1100.0 * f64::from(self.dpi.max(96)) / 96.0 / size.0.max(1) as f64)
        })
    }
    pub fn rect(self, area: RECT, size: (u32, u32)) -> RECT {
        let scale = self.scale(area, size);
        let w = (size.0 as f64 * scale).round().max(1.0) as i32;
        let h = (size.1 as f64 * scale).round().max(1.0) as i32;
        let x = area.left + (area.right - area.left - w) / 2 + self.pan.0.round() as i32;
        let y = area.top + (area.bottom - area.top - h) / 2 + self.pan.1.round() as i32;
        RECT {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        }
    }
    pub fn zoom_at(&mut self, area: RECT, size: (u32, u32), anchor: POINT, factor: f64) {
        let before = self.rect(area, size);
        let u = (anchor.x - before.left) as f64 / (before.right - before.left).max(1) as f64;
        let v = (anchor.y - before.top) as f64 / (before.bottom - before.top).max(1) as f64;
        self.zoom = Some((self.scale(area, size) * factor).clamp(0.02, 32.0));
        let after = self.rect(area, size);
        self.pan.0 += anchor.x as f64 - (after.left as f64 + u * (after.right - after.left) as f64);
        self.pan.1 += anchor.y as f64 - (after.top as f64 + v * (after.bottom - after.top) as f64);
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Crop {
    pub rect: [f64; 4],
    pub size: (u32, u32),
    pub ratio: Option<f64>,
}
impl Crop {
    pub fn new(size: (u32, u32)) -> Self {
        Self {
            rect: [0., 0., size.0 as f64, size.1 as f64],
            size,
            ratio: None,
        }
    }
    pub fn pixels(self) -> [u32; 4] {
        [
            self.rect[0].round() as u32,
            self.rect[1].round() as u32,
            self.rect[2].round() as u32,
            self.rect[3].round() as u32,
        ]
    }
    pub fn set_ratio(&mut self, ratio: Option<f64>) {
        self.ratio = ratio;
        if let Some(r) = ratio {
            let cx = (self.rect[0] + self.rect[2]) / 2.;
            let cy = (self.rect[1] + self.rect[3]) / 2.;
            let mut w = self.rect[2] - self.rect[0];
            let mut h = self.rect[3] - self.rect[1];
            if w / h > r {
                w = h * r;
            } else {
                h = w / r;
            }
            self.rect = [cx - w / 2., cy - h / 2., cx + w / 2., cy + h / 2.];
        }
    }
    // Handle order clockwise: NW, N, NE, E, SE, S, SW, W. 8 moves the whole crop.
    pub fn dragged(self, handle: usize, dx: f64, dy: f64) -> Self {
        let mut out = self;
        let [l, t, r, b] = self.rect;
        let (mw, mh) = (self.size.0 as f64, self.size.1 as f64);
        if handle == 8 {
            let dx = dx.clamp(-l, mw - r);
            let dy = dy.clamp(-t, mh - b);
            out.rect = [l + dx, t + dy, r + dx, b + dy];
            return out;
        }
        let west = matches!(handle, 0 | 6 | 7);
        let east = matches!(handle, 2 | 3 | 4);
        let north = matches!(handle, 0 | 1 | 2);
        let south = matches!(handle, 4 | 5 | 6);
        let mut nl = if west { (l + dx).clamp(0., r - 1.) } else { l };
        let mut nr = if east { (r + dx).clamp(l + 1., mw) } else { r };
        let mut nt = if north { (t + dy).clamp(0., b - 1.) } else { t };
        let mut nb = if south { (b + dy).clamp(t + 1., mh) } else { b };
        if let Some(ratio) = self.ratio {
            let mut w = nr - nl;
            let mut h = nb - nt;
            if west || east {
                h = w / ratio;
            } else {
                w = h * ratio;
            }
            if w < ratio.max(1.) {
                w = ratio.max(1.);
                h = w / ratio;
            }
            let maxw = if west {
                r
            } else if east {
                mw - l
            } else {
                2. * ((l + r) / 2.).min(mw - (l + r) / 2.)
            };
            let maxh = if north {
                b
            } else if south {
                mh - t
            } else {
                2. * ((t + b) / 2.).min(mh - (t + b) / 2.)
            };
            let factor = (maxw / w).min(maxh / h).min(1.);
            w = (w * factor).max(1.);
            h = (h * factor).max(1.);
            if west {
                nl = r - w;
                nr = r;
            } else if east {
                nl = l;
                nr = l + w;
            } else {
                nl = (l + r - w) / 2.;
                nr = nl + w;
            }
            if north {
                nt = b - h;
                nb = b;
            } else if south {
                nt = t;
                nb = t + h;
            } else {
                nt = (t + b - h) / 2.;
                nb = nt + h;
            }
        }
        out.rect = [nl, nt, nr, nb];
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fit_preserves_aspect_and_scales_the_prototype_width_limit_with_dpi() {
        for dpi in [96, 144, 192] {
            let v = Viewport {
                dpi,
                ..Viewport::default()
            };
            let area = RECT {
                left: 44,
                top: 0,
                right: 5000,
                bottom: 3000,
            };
            let r = v.rect(area, (1600, 1067));
            assert_eq!(r.right - r.left, 1100 * dpi as i32 / 96);
            assert!(
                ((r.right - r.left) as f64 / (r.bottom - r.top) as f64 - 1600.0 / 1067.0).abs()
                    < 0.002
            );
        }
    }
    #[test]
    fn zoom_keeps_cursor_over_same_image_point() {
        let area = RECT {
            left: 44,
            top: 36,
            right: 900,
            bottom: 650,
        };
        let size = (4000, 2500);
        let mut v = Viewport::default();
        let p = POINT { x: 450, y: 300 };
        let old = v.rect(area, size);
        let u = (p.x - old.left) as f64 / (old.right - old.left) as f64;
        v.zoom_at(area, size, p, 1.25);
        let new = v.rect(area, size);
        let next = (p.x - new.left) as f64 / (new.right - new.left) as f64;
        assert!((u - next).abs() < 0.002);
    }
    #[test]
    fn crop_moves_and_all_handles_stay_inside_image() {
        let mut c = Crop::new((400, 300));
        c.rect = [40., 30., 350., 270.];
        for h in 0..9 {
            for (dx, dy) in [(-1000., -1000.), (1000., 1000.), (-50., 20.)] {
                let p = c.dragged(h, dx, dy).pixels();
                assert!(p[0] < p[2] && p[1] < p[3]);
                assert!(p[2] <= 400 && p[3] <= 300);
            }
        }
    }
    #[test]
    fn crop_ratio_survives_bounds() {
        let mut c = Crop::new((400, 300));
        c.set_ratio(Some(16. / 9.));
        for h in 0..8 {
            let p = c.dragged(h, 1000., -500.);
            let [l, t, r, b] = p.rect;
            assert!(l >= 0. && t >= 0. && r <= 400.001 && b <= 300.001);
            assert!(((r - l) / (b - t) - 16. / 9.).abs() < 0.001);
        }
    }
}
