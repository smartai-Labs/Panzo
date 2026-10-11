//! Collage geometry: regular grids, with natural proportions only for two/three-photo strips.
use super::capture_tools::Raster;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Fit {
    Natural,
    Contain,
    Cover,
}
#[derive(Clone, Debug)]
pub(crate) struct Photo {
    pub path: PathBuf,
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}
impl Photo {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            x: 0.5,
            y: 0.5,
            zoom: 1.,
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Options {
    pub count: usize,
    pub vertical: bool,
    pub ratio: f64,
    pub fit: Fit,
    pub edge: u32,
    pub gap: u32,
    pub background: [u8; 4],
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cell {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Debug)]
pub(crate) struct Geometry {
    pub width: u32,
    pub height: u32,
    pub cells: Vec<Cell>,
}
fn rounded(v: f64) -> u32 {
    v.round().max(1.) as u32
}
pub(crate) fn geometry(images: &[(u32, u32)], options: Options) -> Result<Geometry, String> {
    let o = options;
    if ![2, 3, 4, 9].contains(&o.count)
        || images.len() < o.count
        || !(128..=16000).contains(&o.edge)
        || o.gap > 100
        || !o.ratio.is_finite()
        || !(0.25..=4.).contains(&o.ratio)
        || images.iter().any(|&(w, h)| w == 0 || h == 0)
    {
        return Err("参数无效：长边 128–16000 px，间距 0–100 px".into());
    }
    let (cols, rows) = if o.count < 4 {
        if o.vertical {
            (1, o.count)
        } else {
            (o.count, 1)
        }
    } else {
        let n = if o.count == 4 { 2 } else { 3 };
        (n, n)
    };
    let gx = o.gap * (cols as u32 - 1);
    let gy = o.gap * (rows as u32 - 1);
    if o.edge <= gx + cols as u32 || o.edge <= gy + rows as u32 {
        return Err("间距相对输出尺寸过大，请减小间距或增大长边".into());
    }
    let ratios: Vec<f64> = images[..o.count]
        .iter()
        .map(|&(w, h)| f64::from(w) / f64::from(h))
        .collect();
    let sums: Vec<f64> = ratios.chunks(cols).map(|v| v.iter().sum()).collect();
    // A grid owns its row/column boundaries; image proportions must never move them.
    let natural = o.count < 4 && o.fit == Fit::Natural;
    let grid = o.count >= 4;
    let (raw_width, raw_height) = if grid {
        // The canvas belongs to the grid settings, never to photo sizes or display mode.
        (cols as f64 * o.ratio, rows as f64)
    } else if natural {
        let width = (0..rows)
            .map(|r| {
                sums[r]
                    * f64::from(
                        images[r * cols..(r + 1) * cols]
                            .iter()
                            .map(|p| p.1)
                            .max()
                            .unwrap(),
                    )
            })
            .fold(1., f64::max);
        (width, sums.iter().map(|sum| width / sum).sum())
    } else {
        let cell = images[..o.count]
            .iter()
            .map(|&(w, h)| {
                if o.fit == Fit::Contain {
                    f64::from(w).max(f64::from(h) * o.ratio)
                } else {
                    f64::from(w).min(f64::from(h) * o.ratio)
                }
            })
            .fold(1., f64::max);
        (cell * cols as f64, cell / o.ratio * rows as f64)
    };
    let scale = (f64::from(o.edge - gx) / raw_width).min(f64::from(o.edge - gy) / raw_height);
    let scale = if grid { scale } else { scale.min(1.) };
    let content_w = rounded(raw_width * scale).max(cols as u32);
    let content_h = rounded(raw_height * scale).max(rows as u32);
    let width = content_w + gx;
    let height = content_h + gy;
    if u64::from(width) * u64::from(height) > 64_000_000 {
        return Err("结果超过 6400 万像素，请减小输出长边".into());
    }
    let mut cells = Vec::new();
    let mut y = 0;
    let mut cumulative_y = 0.;
    for r in 0..rows {
        cumulative_y += if natural {
            raw_width / sums[r] / raw_height
        } else {
            1. / rows as f64
        };
        let bottom = if r + 1 == rows {
            content_h
        } else {
            rounded(cumulative_y * f64::from(content_h))
                .clamp(y + 1, content_h - (rows - r - 1) as u32)
        };
        let mut x = 0;
        let mut cumulative_x = 0.;
        for c in 0..cols {
            cumulative_x += if natural {
                ratios[r * cols + c] / sums[r]
            } else {
                1. / cols as f64
            };
            let right = if c + 1 == cols {
                content_w
            } else {
                rounded(cumulative_x * f64::from(content_w))
                    .clamp(x + 1, content_w - (cols - c - 1) as u32)
            };
            cells.push(Cell {
                x: x + o.gap * c as u32,
                y: y + o.gap * r as u32,
                width: right - x,
                height: bottom - y,
            });
            x = right;
        }
        y = bottom;
    }
    Ok(Geometry {
        width,
        height,
        cells,
    })
}
/// Image rectangle in cell coordinates. Both preview gestures and export use this transform.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
pub(crate) fn placement(size: (u32, u32), cell: Cell, fit: Fit, photo: &Photo) -> Placement {
    let sx = f64::from(cell.width) / f64::from(size.0);
    let sy = f64::from(cell.height) / f64::from(size.1);
    let scale = if fit == Fit::Contain {
        sx.min(sy)
    } else {
        sx.max(sy)
    };
    let zoom = photo.zoom.clamp(1., 3.);
    // Keep source proportions in every mode, including natural photos inside a fixed grid.
    let (width, height) = (
        f64::from(size.0) * scale * zoom,
        f64::from(size.1) * scale * zoom,
    );
    Placement {
        x: (f64::from(cell.width) - width) * photo.x,
        y: (f64::from(cell.height) - height) * photo.y,
        width,
        height,
    }
}
fn position(offset: f64, room: f64) -> f64 {
    if room.abs() < 0.00001 {
        0.5
    } else {
        (offset / room).clamp(0., 1.)
    }
}
pub(crate) fn pan(
    photo: &mut Photo,
    origin: &Photo,
    size: (u32, u32),
    cell: Cell,
    fit: Fit,
    delta: (f64, f64),
) {
    let r = placement(size, cell, fit, origin);
    photo.x = position(r.x + delta.0, f64::from(cell.width) - r.width);
    photo.y = position(r.y + delta.1, f64::from(cell.height) - r.height);
}
pub(crate) fn zoom_at(
    photo: &mut Photo,
    size: (u32, u32),
    cell: Cell,
    fit: Fit,
    factor: f64,
    anchor: (f64, f64),
) {
    let before = placement(size, cell, fit, photo);
    photo.zoom = (photo.zoom * factor).clamp(1., 3.);
    let after = placement(size, cell, fit, photo);
    photo.x = position(
        anchor.0 - (anchor.0 - before.x) * after.width / before.width,
        f64::from(cell.width) - after.width,
    );
    photo.y = position(
        anchor.1 - (anchor.1 - before.y) * after.height / before.height,
        f64::from(cell.height) - after.height,
    );
}
pub(crate) struct Rendered {
    pub image: Raster,
    pub preview: Raster,
    pub geometry: Geometry,
    pub thumbnails: Vec<Raster>,
    pub sources: Vec<std::sync::Arc<Raster>>,
    pub sizes: Vec<(u32, u32)>,
}
pub(crate) fn render(
    photos: &[Photo],
    o: Options,
    cancel: &AtomicBool,
) -> Result<Rendered, String> {
    let check = || {
        if cancel.load(Ordering::Acquire) {
            Err("已取消".to_string())
        } else {
            Ok(())
        }
    };
    let mut dimensions = Vec::new();
    for p in photos.iter().take(o.count) {
        check()?;
        dimensions.push(
            image::image_dimensions(&p.path).map_err(|e| format!("{}：{e}", p.path.display()))?,
        );
    }
    let fallback = dimensions.first().copied().unwrap_or((800, 800));
    dimensions.resize(o.count, fallback);
    let g = geometry(&dimensions, o)?;
    let mut output = Raster {
        width: g.width,
        height: g.height,
        pixels: o.background.repeat(g.width as usize * g.height as usize),
    };
    let mut thumbnails = Vec::new();
    let mut sources = Vec::new();
    let mut sizes = Vec::new();
    for (i, p) in photos.iter().enumerate() {
        check()?;
        let source = Raster::load(&p.path)?;
        thumbnails.push(super::image_ops::resize(&source, 96)?);
        sizes.push((source.width, source.height));
        let proxy = super::image_ops::resize(&source, 1024)?;
        let mut flat = Raster {
            width: proxy.width,
            height: proxy.height,
            pixels: o
                .background
                .repeat(proxy.width as usize * proxy.height as usize),
        };
        super::image_ops::paste(&mut flat, &proxy, 0, 0);
        sources.push(std::sync::Arc::new(flat));
        let Some(cell) = g.cells.get(i) else { continue };
        let rgba = image::RgbaImage::from_raw(source.width, source.height, source.pixels)
            .ok_or("图片数据无效")?;
        let (w, h) = (rgba.width(), rgba.height());
        let r = placement((w, h), *cell, o.fit, p);
        let left = r.x.max(0.).round() as u32;
        let top = r.y.max(0.).round() as u32;
        let right = (r.x + r.width).min(f64::from(cell.width)).round() as u32;
        let bottom = (r.y + r.height).min(f64::from(cell.height)).round() as u32;
        let sx = ((f64::from(left) - r.x) / r.width * f64::from(w))
            .round()
            .clamp(0., f64::from(w - 1)) as u32;
        let sy = ((f64::from(top) - r.y) / r.height * f64::from(h))
            .round()
            .clamp(0., f64::from(h - 1)) as u32;
        let sw = rounded(f64::from(right - left) / r.width * f64::from(w)).min(w - sx);
        let sh = rounded(f64::from(bottom - top) / r.height * f64::from(h)).min(h - sy);
        let image = image::imageops::resize(
            &image::imageops::crop_imm(&rgba, sx, sy, sw, sh).to_image(),
            (right - left).max(1),
            (bottom - top).max(1),
            image::imageops::FilterType::Lanczos3,
        );
        let (left, top) = (cell.x + left, cell.y + top);
        check()?;
        super::image_ops::paste(
            &mut output,
            &Raster {
                width: image.width(),
                height: image.height(),
                pixels: image.into_raw(),
            },
            left,
            top,
        );
    }
    let preview = super::image_ops::resize(&output, 1600)?;
    check()?;
    Ok(Rendered {
        image: output,
        preview,
        geometry: g,
        thumbnails,
        sources,
        sizes,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn options(fit: Fit) -> Options {
        Options {
            count: 4,
            vertical: false,
            ratio: 1.,
            fit,
            edge: 2048,
            gap: 0,
            background: [255; 4],
        }
    }
    #[test]
    fn mixed_photo_grids_keep_shared_boundaries_in_every_display_mode() {
        let images = [
            (1800, 600),
            (600, 1800),
            (900, 600),
            (600, 900),
            (2200, 700),
            (700, 2200),
            (1200, 1200),
            (1900, 1100),
            (1100, 1900),
        ];
        for count in [4, 9] {
            let n = if count == 4 { 2 } else { 3 };
            for fit in [Fit::Natural, Fit::Contain, Fit::Cover] {
                for ratio in [1., 4. / 3., 3. / 4., 16. / 9., 9. / 16.] {
                    for gap in [0, 13, 64] {
                        let mut o = options(fit);
                        o.count = count;
                        o.ratio = ratio;
                        o.gap = gap;
                        o.edge = 1377;
                        let g = geometry(&images[..count], o).unwrap();
                        for (i, cell) in g.cells.iter().enumerate() {
                            let column = g.cells[i % n];
                            let row = g.cells[i / n * n];
                            assert_eq!((cell.x, cell.width), (column.x, column.width));
                            assert_eq!((cell.y, cell.height), (row.y, row.height));
                            assert!(
                                (cell.width as f64 - cell.height as f64 * ratio).abs()
                                    <= 1. + ratio
                            );
                            if i % n > 0 {
                                let left = g.cells[i - 1];
                                assert_eq!(cell.x - (left.x + left.width), gap);
                            }
                            if i >= n {
                                let top = g.cells[i - n];
                                assert_eq!(cell.y - (top.y + top.height), gap);
                            }
                        }
                        let last = g.cells.last().unwrap();
                        assert_eq!(
                            (last.x + last.width, last.y + last.height),
                            (g.width, g.height)
                        );
                        let mut reordered = images[..count].to_vec();
                        reordered.reverse();
                        o.vertical = true;
                        let other = geometry(&reordered, o).unwrap();
                        let replacement = geometry(&vec![(64, 32); count], o).unwrap();
                        assert_eq!((replacement.width, replacement.height), (g.width, g.height));
                        assert_eq!(g.width.max(g.height), o.edge);
                        assert_eq!((other.width, other.height), (g.width, g.height));
                        for (a, b) in g.cells.iter().zip(&other.cells) {
                            assert_eq!(
                                (a.x, a.y, a.width, a.height),
                                (b.x, b.y, b.width, b.height)
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn natural_placement_preserves_photo_proportions_in_a_grid() {
        let g = geometry(
            &[(1200, 400), (400, 1200), (900, 600), (600, 900)],
            options(Fit::Natural),
        )
        .unwrap();
        for (size, cell) in [(1200, 400), (400, 1200), (900, 600), (600, 900)]
            .into_iter()
            .zip(g.cells)
        {
            let photo = Photo::new(PathBuf::new());
            let p = placement(size, cell, Fit::Natural, &photo);
            assert!((p.width / p.height - size.0 as f64 / size.1 as f64).abs() < 1e-9);
            assert!(p.width >= cell.width as f64 && p.height >= cell.height as f64);
        }
    }
    #[test]
    fn rendered_grids_have_continuous_gutters_and_keep_transforms_inside_cells() {
        let dir = std::env::temp_dir().join(format!("panzo-grid-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let sizes = [
            (180, 60),
            (60, 180),
            (90, 60),
            (60, 90),
            (140, 50),
            (50, 140),
            (120, 120),
            (170, 90),
            (90, 170),
        ];
        let photos: Vec<_> = sizes
            .into_iter()
            .enumerate()
            .map(|(i, (w, h))| {
                let path = dir.join(format!("{i}.png"));
                image::RgbaImage::from_pixel(w, h, image::Rgba([20 + i as u8 * 20, 90, 180, 255]))
                    .save(&path)
                    .unwrap();
                let mut p = Photo::new(path);
                p.zoom = 1. + (i % 3) as f64;
                p.x = (i % 2) as f64;
                p.y = 1. - p.x;
                p
            })
            .collect();
        for count in [4, 9] {
            let n = if count == 4 { 2 } else { 3 };
            for fit in [Fit::Natural, Fit::Contain, Fit::Cover] {
                let mut o = options(fit);
                o.count = count;
                o.ratio = 4. / 3.;
                o.edge = 257;
                o.gap = 7;
                o.background = [7, 11, 19, 255];
                let r = render(&photos[..count], o, &AtomicBool::new(false)).unwrap();
                let pixel = |x, y| {
                    let offset = ((y * r.image.width + x) * 4) as usize;
                    &r.image.pixels[offset..offset + 4]
                };
                for col in 1..n {
                    for x in r.geometry.cells[col].x - o.gap..r.geometry.cells[col].x {
                        for y in 0..r.image.height {
                            assert_eq!(pixel(x, y), o.background);
                        }
                    }
                }
                for row in 1..n {
                    for y in r.geometry.cells[row * n].y - o.gap..r.geometry.cells[row * n].y {
                        for x in 0..r.image.width {
                            assert_eq!(pixel(x, y), o.background);
                        }
                    }
                }
                // With no padding, every cell edge contains its own photo, not a neighbor's.
                if fit != Fit::Contain {
                    for (i, cell) in r.geometry.cells.iter().enumerate() {
                        let expected = [180, 90, 20 + i as u8 * 20, 255];
                        assert_eq!(pixel(cell.x, cell.y), expected);
                        assert_eq!(
                            pixel(cell.x + cell.width - 1, cell.y + cell.height - 1),
                            expected
                        );
                    }
                }
                assert_eq!(
                    (r.preview.width, r.preview.height),
                    (r.image.width, r.image.height)
                );
                assert_eq!(r.preview.pixels, r.image.pixels);
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn grid_gap_and_edge_are_output_pixels() {
        let mut o = options(Fit::Cover);
        o.gap = 13;
        o.edge = 1376;
        let g = geometry(&[(2000, 3000); 4], o).unwrap();
        assert_eq!((g.width, g.height), (1376, 1376));
        assert_eq!(g.cells[1].x - g.cells[0].width, 13);
        assert_eq!(g.cells[2].y - g.cells[0].height, 13);
    }
    #[test]
    fn reject_invalid_and_oversize_geometry() {
        let mut o = options(Fit::Cover);
        o.edge = 16000;
        assert!(geometry(&[(12000, 12000); 4], o).is_err());
        o.edge = 128;
        o.gap = 100;
        assert!(geometry(&[(200, 200); 4], o).is_ok());
        o.count = 9;
        assert!(geometry(&[(200, 200); 9], o).is_err());
    }
    #[test]
    fn vertical_two_retains_whole_images() {
        let mut o = options(Fit::Natural);
        o.count = 2;
        o.vertical = true;
        let g = geometry(&[(800, 400), (400, 800)], o).unwrap();
        assert_eq!(g.cells[0].width, g.cells[1].width);
        assert!((f64::from(g.cells[1].height) / f64::from(g.cells[0].height) - 4.).abs() < 0.01);
    }
    #[test]
    fn rendered_modes_crop_position_and_cancellation_preserve_sources() {
        let dir = std::env::temp_dir().join(format!("panzo-collage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("strip.png");
        let mut rgba = image::RgbaImage::from_pixel(240, 120, image::Rgba([0, 0, 255, 255]));
        for y in 0..120 {
            for x in 0..120 {
                rgba.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        rgba.save(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let mut photos = vec![Photo::new(path.clone()), Photo::new(path.clone())];
        let mut o = options(Fit::Contain);
        o.count = 2;
        o.edge = 480;
        let cancel = AtomicBool::new(false);
        let contained = render(&photos, o, &cancel).unwrap();
        assert_eq!(&contained.image.pixels[..4], &[255; 4]);
        o.fit = Fit::Natural;
        let natural = render(&photos, o, &cancel).unwrap();
        assert_eq!(&natural.image.pixels[..4], &[0, 0, 255, 255]);
        o.fit = Fit::Cover;
        photos[0].x = 0.;
        photos[1].x = 1.;
        let covered = render(&photos, o, &cancel).unwrap();
        let cell = covered.geometry.cells[1];
        let offset = ((cell.y * covered.image.width + cell.x) * 4) as usize;
        assert_eq!(&covered.image.pixels[..4], &[0, 0, 255, 255]);
        assert_eq!(&covered.image.pixels[offset..offset + 4], &[255, 0, 0, 255]);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        cancel.store(true, Ordering::Release);
        assert!(render(&photos, o, &cancel).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn direct_zoom_preserves_cursor_anchor_and_pan_tracks_pointer() {
        let cell = Cell {
            x: 0,
            y: 0,
            width: 400,
            height: 300,
        };
        for fit in [Fit::Natural, Fit::Contain, Fit::Cover] {
            let size = (1200, 800);
            let mut photo = Photo::new(PathBuf::new());
            photo.zoom = 1.5;
            let before = placement(size, cell, fit, &photo);
            let anchor = (180., 140.);
            let source = (
                (anchor.0 - before.x) / before.width,
                (anchor.1 - before.y) / before.height,
            );
            zoom_at(&mut photo, size, cell, fit, 1.2, anchor);
            let after = placement(size, cell, fit, &photo);
            assert!(((anchor.0 - after.x) / after.width - source.0).abs() < 1e-9);
            assert!(((anchor.1 - after.y) / after.height - source.1).abs() < 1e-9);
            let origin = photo.clone();
            pan(&mut photo, &origin, size, cell, fit, (12., -9.));
            let moved = placement(size, cell, fit, &photo);
            assert!((moved.x - after.x - 12.).abs() < 1e-9);
            assert!((moved.y - after.y + 9.).abs() < 1e-9);
            pan(&mut photo, &origin, size, cell, fit, (1e8, -1e8));
            assert!((0. ..=1.).contains(&photo.x) && (0. ..=1.).contains(&photo.y));
            zoom_at(&mut photo, size, cell, fit, 100., anchor);
            assert_eq!(photo.zoom, 3.);
            zoom_at(&mut photo, size, cell, fit, 0.001, anchor);
            assert_eq!(photo.zoom, 1.);
            assert!(photo.x.is_finite() && photo.y.is_finite());
        }
    }
    #[test]
    fn photo_transforms_apply_to_every_display_mode_and_export_has_no_selection_marks() {
        let dir =
            std::env::temp_dir().join(format!("panzo-collage-gesture-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("split.png");
        let mut image = image::RgbaImage::from_pixel(240, 120, image::Rgba([20, 80, 160, 255]));
        for y in 0..120 {
            for x in 120..240 {
                image.put_pixel(x, y, image::Rgba([180, 210, 60, 255]));
            }
        }
        image.save(&path).unwrap();
        let cancel = AtomicBool::new(false);
        for fit in [Fit::Natural, Fit::Contain, Fit::Cover] {
            let mut photos = vec![Photo::new(path.clone()); 2];
            photos[0].zoom = 3.;
            photos[0].x = 0.;
            photos[1].zoom = 3.;
            photos[1].x = 1.;
            let mut o = options(fit);
            o.count = 2;
            o.edge = 480;
            o.background = [33, 22, 11, 255];
            let rendered = render(&photos, o, &cancel).unwrap();
            for (i, expected) in [[160, 80, 20, 255], [60, 210, 180, 255]].iter().enumerate() {
                let cell = rendered.geometry.cells[i];
                let x = cell.x + cell.width / 2;
                let y = cell.y + cell.height / 2;
                let offset = ((y * rendered.image.width + x) * 4) as usize;
                assert_eq!(
                    &rendered.image.pixels[offset..offset + 4],
                    expected,
                    "{fit:?}"
                );
            }
            assert!(
                !rendered
                    .image
                    .pixels
                    .chunks_exact(4)
                    .any(|p| p == [70, 96, 242, 255])
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
