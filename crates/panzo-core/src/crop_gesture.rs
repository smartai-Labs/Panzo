use crate::VideoCrop;

/// Horizontal/vertical edge: -1 is leading, 1 trailing, 0 unchanged. (0,0) moves the box.
#[allow(clippy::many_single_char_names, clippy::cast_sign_loss)] // Conventional rectangle coordinates; quantization clamps to a nonnegative range.
pub fn drag_crop(crop: VideoCrop, edge: (i8, i8), delta: (f64, f64), locked: bool) -> VideoCrop {
    if !delta.0.is_finite() || !delta.1.is_finite() {
        return crop;
    }
    let mut l = f64::from(crop.left) / 1000.0;
    let mut t = f64::from(crop.top) / 1000.0;
    let mut r = 1.0 - f64::from(crop.right) / 1000.0;
    let mut b = 1.0 - f64::from(crop.bottom) / 1000.0;
    if edge == (0, 0) {
        let dx = delta.0.clamp(-l, 1.0 - r);
        let dy = delta.1.clamp(-t, 1.0 - b);
        l += dx;
        r += dx;
        t += dy;
        b += dy;
    } else if locked {
        let w = r - l;
        let h = b - t;
        let anchor = |a, z, e: i8| match e.cmp(&0) {
            std::cmp::Ordering::Less => z,
            std::cmp::Ordering::Greater => a,
            std::cmp::Ordering::Equal => f64::midpoint(a, z),
        };
        let ax = anchor(l, r, edge.0);
        let ay = anchor(t, b, edge.1);
        let fx = 1.0 + delta.0 * f64::from(edge.0) / w;
        let fy = 1.0 + delta.1 * f64::from(edge.1) / h;
        let wanted = if edge.0 == 0 {
            fy
        } else if edge.1 == 0 || (fx - 1.0).abs() >= (fy - 1.0).abs() {
            fx
        } else {
            fy
        };
        let available = |a: f64, e: i8| match e.cmp(&0) {
            std::cmp::Ordering::Less => a,
            std::cmp::Ordering::Greater => 1.0 - a,
            std::cmp::Ordering::Equal => 2.0 * a.min(1.0 - a),
        };
        let minimum = (0.05 / w).max(0.05 / h);
        let maximum = (available(ax, edge.0) / w)
            .min(available(ay, edge.1) / h)
            .max(minimum);
        let factor = wanted.clamp(minimum, maximum);
        let origin = |a, size, e: i8| match e.cmp(&0) {
            std::cmp::Ordering::Less => a - size,
            std::cmp::Ordering::Greater => a,
            std::cmp::Ordering::Equal => a - size / 2.0,
        };
        l = origin(ax, w * factor, edge.0);
        r = l + w * factor;
        t = origin(ay, h * factor, edge.1);
        b = t + h * factor;
    } else {
        if edge.0 < 0 {
            l = (l + delta.0).clamp(0.0, (r - 0.05).max(0.0));
        }
        if edge.0 > 0 {
            r = (r + delta.0).clamp((l + 0.05).min(1.0), 1.0);
        }
        if edge.1 < 0 {
            t = (t + delta.1).clamp(0.0, (b - 0.05).max(0.0));
        }
        if edge.1 > 0 {
            b = (b + delta.1).clamp((t + 0.05).min(1.0), 1.0);
        }
    }
    let quantize = |v: f64| (v * 1000.0).round().clamp(0.0, 950.0) as u16;
    let left = quantize(l);
    let top = quantize(t);
    VideoCrop {
        left,
        top,
        right: quantize(1.0 - r).min(950 - left),
        bottom: quantize(1.0 - b).min(950 - top),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moving_and_resizing_crop_keeps_bounds_and_locked_aspect() {
        let crop = VideoCrop {
            left: 200,
            top: 100,
            right: 200,
            bottom: 100,
        };
        let moved = drag_crop(crop, (0, 0), (2.0, -2.0), false);
        assert_eq!(
            moved,
            VideoCrop {
                left: 400,
                top: 0,
                right: 0,
                bottom: 200
            }
        );
        for x in -1..=1 {
            for y in -1..=1 {
                for delta in [(-4.0, 3.0), (0.17, 0.08), (4.0, -3.0)] {
                    for locked in [false, true] {
                        let actual = drag_crop(crop, (x, y), delta, locked);
                        assert!(actual.validate().is_ok());
                        if locked {
                            assert!(
                                (actual.width() / actual.height() - crop.width() / crop.height())
                                    .abs()
                                    < 0.025
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(drag_crop(crop, (1, 1), (f64::NAN, 0.0), true), crop);
    }
}
