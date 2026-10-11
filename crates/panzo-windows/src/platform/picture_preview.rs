//! Cached source-space drafts with region updates for immutable preview levels.
use super::*;

pub(super) struct DraftCache {
    revision: u64,
    excluded: Option<usize>,
    mark: Mark,
    raster: Raster,
    levels: Vec<Raster>,
}
impl DraftCache {
    pub fn update(
        slot: &mut Option<Self>,
        document: &Document,
        mark: &Mark,
        excluded: Option<usize>,
    ) {
        let reusable = slot
            .as_ref()
            .is_some_and(|c| c.revision == document.revision && c.excluded == excluded);
        if reusable && slot.as_ref().unwrap().mark.same(mark) {
            return;
        }
        let Ok(raster) = document.render_replacement(mark, excluded) else {
            *slot = None;
            return;
        };
        if reusable {
            let cache = slot.as_mut().unwrap();
            let mut dirty = union(cache.mark.bounds(), mark.bounds());
            // These effects sample neighboring pixels; their output can change
            // outside the text bounds. Ordinary layers preserve the dirty area.
            if let Some(index) = excluded {
                for later in document.state.marks.iter().skip(index + 1) {
                    if matches!(later.tool, Tool::Blur | Tool::Mosaic) {
                        dirty = union(dirty, later.bounds());
                    }
                }
            }
            let corners = [
                POINT {
                    x: dirty.left,
                    y: dirty.top,
                },
                POINT {
                    x: dirty.right,
                    y: dirty.top,
                },
                POINT {
                    x: dirty.left,
                    y: dirty.bottom,
                },
                POINT {
                    x: dirty.right,
                    y: dirty.bottom,
                },
            ]
            .map(|p| document.display_edge(p));
            let dirty = RECT {
                left: corners.iter().map(|p| p.x).min().unwrap().max(0),
                top: corners.iter().map(|p| p.y).min().unwrap().max(0),
                right: corners
                    .iter()
                    .map(|p| p.x)
                    .max()
                    .unwrap()
                    .min(raster.width as i32),
                bottom: corners
                    .iter()
                    .map(|p| p.y)
                    .max()
                    .unwrap()
                    .min(raster.height as i32),
            };
            for level in &mut cache.levels {
                update_level(&raster, level, dirty);
            }
            cache.mark = mark.clone();
            cache.raster = raster;
        } else {
            let levels = preview_levels(&raster);
            *slot = Some(Self {
                revision: document.revision,
                excluded,
                mark: mark.clone(),
                raster,
                levels,
            });
        }
    }
    pub fn image(&self, view: RECT) -> &Raster {
        self.levels
            .iter()
            .find(|level| {
                level.width >= (view.right - view.left).max(1) as u32
                    && level.height >= (view.bottom - view.top).max(1) as u32
            })
            .unwrap_or(&self.raster)
    }
}
fn union(a: RECT, b: RECT) -> RECT {
    RECT {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}
// Match image::imageops::thumbnail's downscale cell boundaries and integer
// rounding. Recompute only cells intersecting changed source pixels. The extra
// cell on each edge covers floating-point boundary rounding at odd dimensions.
fn update_level(source: &Raster, level: &mut Raster, dirty: RECT) {
    if dirty.right <= dirty.left || dirty.bottom <= dirty.top {
        return;
    }
    let rx = source.width as f32 / level.width as f32;
    let ry = source.height as f32 / level.height as f32;
    debug_assert!(rx >= 1. && ry >= 1.);
    let left = ((dirty.left as f32 / rx).floor() as i32 - 1).max(0) as u32;
    let top = ((dirty.top as f32 / ry).floor() as i32 - 1).max(0) as u32;
    let right = ((dirty.right as f32 / rx).ceil() as u32 + 1).min(level.width);
    let bottom = ((dirty.bottom as f32 / ry).ceil() as u32 + 1).min(level.height);
    for y in top..bottom {
        let yf = y as f32 * ry;
        let sy = (yf.ceil() as u32).min(source.height - 1);
        let ey = ((yf + ry).ceil() as u32).min(source.height);
        for x in left..right {
            let xf = x as f32 * rx;
            let sx = (xf.ceil() as u32).min(source.width - 1);
            let ex = ((xf + rx).ceil() as u32).min(source.width);
            let mut sum = [0_u64; 4];
            for py in sy..ey {
                let start = (py as usize * source.width as usize + sx as usize) * 4;
                let end = (py as usize * source.width as usize + ex as usize) * 4;
                for pixel in source.pixels[start..end].chunks_exact(4) {
                    for c in 0..4 {
                        sum[c] += pixel[c] as u64;
                    }
                }
            }
            let count = u64::from((ex - sx) * (ey - sy));
            let offset = (y as usize * level.width as usize + x as usize) * 4;
            for c in 0..4 {
                level.pixels[offset + c] = ((sum[c] + count / 2) / count) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regional_preview_matches_full_thumbnail_at_odd_dimensions_and_edges() {
        for (w, h) in [(2167, 1231), (4096, 2160), (1231, 2167), (3001, 1)] {
            let mut source = Raster {
                width: w,
                height: h,
                pixels: (0..w as usize * h as usize * 4)
                    .map(|i| (i.wrapping_mul(37) % 256) as u8)
                    .collect(),
            };
            let mut levels = preview_levels(&source);
            for r in [
                RECT {
                    left: 0,
                    top: 0,
                    right: 43,
                    bottom: 1,
                },
                RECT {
                    left: 71,
                    top: 0,
                    right: 441,
                    bottom: 1,
                },
                RECT {
                    left: w as i32 - 27,
                    top: h as i32 - 1,
                    right: w as i32,
                    bottom: h as i32,
                },
                RECT {
                    left: 100,
                    top: 0,
                    right: 350,
                    bottom: h.min(270) as i32,
                },
            ] {
                for y in r.top..r.bottom {
                    for x in r.left..r.right {
                        let i = (y as usize * w as usize + x as usize) * 4;
                        source.pixels[i..i + 4].copy_from_slice(&[31, 73, 159, 255]);
                    }
                }
                for level in &mut levels {
                    update_level(&source, level, r);
                }
                for (actual, expected) in levels.iter().zip(preview_levels(&source)) {
                    assert_eq!(actual.pixels, expected.pixels, "{w}x{h} region {r:?}");
                }
            }
        }
    }
}
