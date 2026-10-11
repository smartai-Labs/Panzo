//! Bounded image composition, overlap matching and static batch conversion.
use super::capture_tools::Raster;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
const PIXELS: u64 = 64_000_000;
fn size(width: u32, height: u32) -> Result<usize, String> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 || pixels > PIXELS {
        return Err("结果超出 6400 万像素上限，请减少图片或缩小尺寸".into());
    }
    usize::try_from(pixels * 4).map_err(|e| e.to_string())
}
fn rgba(image: &Raster) -> Result<image::RgbaImage, String> {
    image::RgbaImage::from_raw(image.width, image.height, image.pixels.clone())
        .ok_or("图片数据无效".into())
}
#[allow(clippy::cast_sign_loss)] // Dimensions are clamped positive before conversion.
pub(crate) fn resize(image: &Raster, longest: u32) -> Result<Raster, String> {
    if longest == 0 || image.width.max(image.height) <= longest {
        return Ok(image.clone());
    }
    let scale = f64::from(longest) / f64::from(image.width.max(image.height));
    let image = image::imageops::resize(
        &rgba(image)?,
        (f64::from(image.width) * scale).round().max(1.0) as u32,
        (f64::from(image.height) * scale).round().max(1.0) as u32,
        image::imageops::FilterType::Lanczos3,
    );
    Ok(Raster {
        width: image.width(),
        height: image.height(),
        pixels: image.into_raw(),
    })
}
pub(super) fn paste(output: &mut Raster, source: &Raster, left: u32, top: u32) {
    for y in 0..source.height {
        for x in 0..source.width {
            let src = ((y * source.width + x) * 4) as usize;
            let dst = (((y + top) * output.width + x + left) * 4) as usize;
            let alpha = u32::from(source.pixels[src + 3]);
            for c in 0..3 {
                output.pixels[dst + c] = ((u32::from(source.pixels[src + c]) * alpha
                    + u32::from(output.pixels[dst + c]) * (255 - alpha))
                    / 255) as u8;
            }
            output.pixels[dst + 3] = 255;
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) enum Layout {
    Vertical,
    Horizontal,
    Grid(u32),
}
pub(crate) fn collage(
    paths: &[PathBuf],
    layout: Layout,
    cell: u32,
    gap: u32,
    background: [u8; 4],
    cancel: &AtomicBool,
) -> Result<Raster, String> {
    if paths.is_empty() || paths.len() > 64 || !(64..=4096).contains(&cell) || gap > 100 {
        return Err("拼图参数无效（最多 64 张，尺寸 64–4096，间距 0–100）".into());
    }
    let columns = match layout {
        Layout::Vertical => 1,
        Layout::Horizontal => paths.len() as u32,
        Layout::Grid(n) => n.clamp(1, 8),
    };
    let mut images = Vec::new();
    let mut bytes = 0;
    for path in paths {
        if cancel.load(Ordering::Acquire) {
            return Err("已取消".into());
        }
        let source = Raster::load(path)?;
        let image = resize(&source, cell)?;
        bytes += image.pixels.len();
        if bytes > 256 * 1024 * 1024 {
            return Err("拼图素材超过 256 MB，请减小尺寸".into());
        }
        images.push(image);
    }
    let rows = (images.len() as u32).div_ceil(columns);
    let mut widths = vec![0; columns as usize];
    let mut heights = vec![0; rows as usize];
    for (i, img) in images.iter().enumerate() {
        widths[i % columns as usize] = widths[i % columns as usize].max(img.width);
        heights[i / columns as usize] = heights[i / columns as usize].max(img.height);
    }
    let width = widths.iter().sum::<u32>() + gap * (columns + 1);
    let height = heights.iter().sum::<u32>() + gap * (rows + 1);
    let mut output = Raster {
        width,
        height,
        pixels: background.repeat(size(width, height)? / 4),
    };
    for (i, img) in images.iter().enumerate() {
        if cancel.load(Ordering::Acquire) {
            return Err("已取消".into());
        }
        let col = i % columns as usize;
        let row = i / columns as usize;
        let x = gap * (col as u32 + 1)
            + widths[..col].iter().sum::<u32>()
            + (widths[col] - img.width) / 2;
        let y = gap * (row as u32 + 1)
            + heights[..row].iter().sum::<u32>()
            + (heights[row] - img.height) / 2;
        paste(&mut output, img, x, y);
    }
    Ok(output)
}
fn cost(previous: &Raster, next: &Raster, step: u32) -> f64 {
    let overlap = previous.height - step;
    let mut sum = 0u64;
    let mut count = 0u64;
    let xs = (previous.width / 48).max(1);
    let ys = (overlap / 40).max(1);
    for y in (0..overlap).step_by(ys as usize) {
        for x in (0..previous.width).step_by(xs as usize) {
            let a = (((y + step) * previous.width + x) * 4) as usize;
            let b = ((y * next.width + x) * 4) as usize;
            for c in 0..3 {
                sum += u64::from(previous.pixels[a + c].abs_diff(next.pixels[b + c]));
                count += 1;
            }
        }
    }
    sum as f64 / count.max(1) as f64
}
pub(crate) fn estimate_scroll(previous: &Raster, next: &Raster) -> Result<u32, String> {
    if previous.width != next.width || previous.height != next.height || previous.height < 48 {
        return Err("截图尺寸改变或区域太小，请重新选择滚动区域".into());
    }
    if cost(previous, next, 0) < 0.7 {
        return Err("画面没有变化，可能已到底部；可结束或继续手动滚动".into());
    }
    let mut scores = (1..=previous.height - 32)
        .map(|step| (step, cost(previous, next, step)))
        .collect::<Vec<_>>();
    scores.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (step, best) = scores[0];
    let alternative = scores
        .iter()
        .find(|(s, _)| s.abs_diff(step) > 4)
        .map_or(255.0, |(_, c)| *c);
    if best > 16.0 || alternative - best < 0.45 {
        return Err("无法确定重叠位置。请减少滚动幅度，或填写重叠高度后手动拼接".into());
    }
    Ok(step)
}
#[derive(Default)]
pub(crate) struct ScrollDocument {
    pub frames: Vec<Raster>,
    pub steps: Vec<u32>,
}
impl ScrollDocument {
    pub fn append(&mut self, image: Raster, manual_overlap: Option<u32>) -> Result<(), String> {
        if self.frames.len() >= 64 {
            return Err("最多采集 64 屏，请先完成本次长截图".into());
        }
        let step = if let Some(last) = self.frames.last() {
            if last.width != image.width || last.height != image.height {
                return Err("尺寸改变，请重新开始".into());
            }
            if let Some(overlap) = manual_overlap {
                if overlap == 0 || overlap >= image.height {
                    return Err("重叠高度须大于 0 且小于选区高度".into());
                }
                image.height - overlap
            } else {
                estimate_scroll(last, &image)?
            }
        } else {
            image.height
        };
        size(
            image.width,
            self.steps.iter().sum::<u32>().saturating_add(step),
        )?;
        if self
            .frames
            .iter()
            .map(|f| f.pixels.len())
            .sum::<usize>()
            .saturating_add(image.pixels.len())
            > 256 * 1024 * 1024
        {
            return Err("采集缓存达到 256 MB，请结束本次长截图".into());
        }
        self.steps.push(step);
        self.frames.push(image);
        Ok(())
    }
    pub fn undo(&mut self) {
        self.frames.pop();
        self.steps.pop();
    }
    pub fn compose(&self) -> Result<Raster, String> {
        let width = self.frames.first().ok_or("尚未采集画面")?.width;
        let height = self.steps.iter().sum::<u32>();
        let mut result = Raster {
            width,
            height,
            pixels: vec![255; size(width, height)?],
        };
        let mut y = 0;
        for (frame, step) in self.frames.iter().zip(&self.steps) {
            let from = (frame.height - step) * width * 4;
            let to = y * width * 4;
            let count = step * width * 4;
            result.pixels[to as usize..(to + count) as usize]
                .copy_from_slice(&frame.pixels[from as usize..(from + count) as usize]);
            y += step;
        }
        Ok(result)
    }
}
#[derive(Clone, Copy)]
pub(crate) enum OutputFormat {
    Png,
    Jpeg,
    Bmp,
}
impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Bmp => "bmp",
        }
    }
}
pub(crate) fn encode(image: &Raster, format: OutputFormat, quality: u8) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let mut rgba = rgba(image)?;
    for p in rgba.pixels_mut() {
        p.0.swap(0, 2);
    }
    let mut bytes = Vec::new();
    match format {
        OutputFormat::Png => image::codecs::png::PngEncoder::new_with_quality(
            &mut bytes,
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::Adaptive,
        )
        .write_image(
            &rgba,
            image.width,
            image.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| e.to_string())?,
        OutputFormat::Jpeg => {
            let rgb = rgba
                .pixels()
                .flat_map(|p| {
                    let alpha = u32::from(p[3]);
                    [0, 1, 2].map(|c| ((u32::from(p[c]) * alpha + 255 * (255 - alpha)) / 255) as u8)
                })
                .collect::<Vec<_>>();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality.clamp(10, 100))
                .encode(
                    &rgb,
                    image.width,
                    image.height,
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|e| e.to_string())?;
        }
        OutputFormat::Bmp => image::codecs::bmp::BmpEncoder::new(&mut bytes)
            .encode(
                &rgba,
                image.width,
                image.height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?,
    }
    Ok(bytes)
}
#[cfg(test)]
pub(crate) fn batch(
    paths: &[PathBuf],
    directory: &Path,
    format: OutputFormat,
    quality: u8,
    longest: u32,
    cancel: &AtomicBool,
    progress: impl Fn(String),
) -> Result<String, String> {
    batch_registered(
        paths, directory, format, quality, longest, cancel, None, progress,
    )
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn batch_registered(
    paths: &[PathBuf],
    directory: &Path,
    format: OutputFormat,
    quality: u8,
    longest: u32,
    cancel: &AtomicBool,
    library: Option<&Path>,
    progress: impl Fn(String),
) -> Result<String, String> {
    use std::io::Write;
    if paths.is_empty() || paths.len() > 500 {
        return Err("请选择 1–500 张静态图片".into());
    }
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let mut lines = Vec::new();
    let mut success = 0;
    let mut total = 0;
    for (i, path) in paths.iter().enumerate() {
        if cancel.load(Ordering::Acquire) {
            lines.push("已取消；已完成文件保留。".into());
            break;
        }
        progress(format!(
            "正在处理 {}/{}：{}",
            i + 1,
            paths.len(),
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        let result = (|| {
            let image = resize(&Raster::load(path)?, longest)?;
            let bytes = encode(&image, format, quality)?;
            if cancel.load(Ordering::Acquire) {
                return Err("已取消".into());
            }
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            let output = directory.join(format!("{:03}-{}.{}", i + 1, name, format.extension()));
            let temporary = directory.join(format!(".panzo-{}.part", uuid::Uuid::new_v4()));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|e| e.to_string())?;
            if let Err(e) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
                drop(file);
                let _ = std::fs::remove_file(&temporary);
                return Err(e.to_string());
            }
            drop(file);
            if let Err(e) =
                crate::platform::manifest_store::move_file_new_atomically(&temporary, &output)
            {
                let _ = std::fs::remove_file(&temporary);
                return Err(e.to_string());
            }
            if let Some(library) = library {
                super::asset_index::register(library, &output)?;
            }
            Ok::<_, String>(bytes.len())
        })();
        match result {
            Ok(bytes) => {
                success += 1;
                total += bytes;
                lines.push(format!("完成：{}（{} KB）", path.display(), bytes / 1024));
            }
            Err(e) => lines.push(format!("失败：{}：{e}", path.display())),
        }
    }
    Ok(format!(
        "已完成 {success}/{} 张，共 {} KB\r\n输出文件夹见上方路径，可点击文件夹图标重新选择。\r\n\r\n{}",
        paths.len(),
        total / 1024,
        lines.join("\r\n")
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn scene(width: u32, height: u32) -> Raster {
        let mut image = Raster {
            width,
            height,
            pixels: vec![255; (width * height * 4) as usize],
        };
        for y in 0..height {
            for x in 0..width {
                let value = ((y * 31 + x * 17 + (x * y) % 83) % 255) as u8;
                let i = ((y * width + x) * 4) as usize;
                image.pixels[i..i + 3].copy_from_slice(&[value, value / 2, 255 - value]);
            }
        }
        image
    }
    #[test]
    fn stitch_recovers_overlap_and_rejects_ambiguous_frames() {
        let full = scene(160, 500);
        let a = full
            .crop(panzo_core::PhysicalRect {
                x: 0,
                y: 0,
                width: 160,
                height: 300,
            })
            .unwrap();
        let b = full
            .crop(panzo_core::PhysicalRect {
                x: 0,
                y: 200,
                width: 160,
                height: 300,
            })
            .unwrap();
        assert_eq!(estimate_scroll(&a, &b).unwrap(), 200);
        let mut doc = ScrollDocument::default();
        doc.append(a.clone(), None).unwrap();
        assert!(doc.append(a, None).is_err());
        doc.append(b, None).unwrap();
        assert_eq!(doc.compose().unwrap().pixels, full.pixels);
        doc.undo();
        assert_eq!(doc.compose().unwrap().height, 300);
    }
    #[test]
    fn jpeg_flattens_transparency_and_png_keeps_it() {
        let image = Raster {
            width: 8,
            height: 8,
            pixels: vec![0; 8 * 8 * 4],
        };
        let jpg = image::load_from_memory(&encode(&image, OutputFormat::Jpeg, 95).unwrap())
            .unwrap()
            .to_rgb8();
        assert!(jpg.pixels().all(|p| p.0.iter().all(|v| *v > 248)));
        let png = image::load_from_memory(&encode(&image, OutputFormat::Png, 95).unwrap())
            .unwrap()
            .to_rgba8();
        assert!(png.pixels().all(|p| p[3] == 0));
    }
    #[test]
    fn batch_and_collage_preserve_sources_and_refuse_collisions() {
        let root = std::env::temp_dir().join(format!("panzo-image-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let a = root.join("红.png");
        let b = root.join("blue.png");
        Raster {
            width: 80,
            height: 40,
            pixels: [0, 0, 255, 255].repeat(80 * 40),
        }
        .save(&a)
        .unwrap();
        Raster {
            width: 40,
            height: 80,
            pixels: [255, 0, 0, 255].repeat(40 * 80),
        }
        .save(&b)
        .unwrap();
        let originals = [std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap()];
        let files = vec![a, b];
        let cancel = AtomicBool::new(false);
        let merged = collage(&files, Layout::Horizontal, 64, 8, [255; 4], &cancel).unwrap();
        assert_eq!((merged.width, merged.height), (120, 80));
        assert_eq!(
            &merged.pixels[((40 * 120 + 40) * 4)..((40 * 120 + 40) * 4 + 4)],
            &[0, 0, 255, 255]
        );
        assert_eq!(
            &merged.pixels[((40 * 120 + 90) * 4)..((40 * 120 + 90) * 4 + 4)],
            &[255, 0, 0, 255]
        );
        let output = root.join("out");
        let result = batch(&files, &output, OutputFormat::Jpeg, 85, 32, &cancel, |_| {}).unwrap();
        assert!(result.contains("2/2"));
        let converted = output.join("001-红.jpg");
        let first = std::fs::read(&converted).unwrap();
        assert_eq!(
            (
                Raster::load(&converted).unwrap().width,
                Raster::load(&converted).unwrap().height
            ),
            (32, 16)
        );
        assert!(
            batch(&files, &output, OutputFormat::Jpeg, 50, 16, &cancel, |_| {})
                .unwrap()
                .contains("0/2")
        );
        assert_eq!(std::fs::read(&converted).unwrap(), first);
        cancel.store(true, Ordering::Release);
        assert!(
            batch(
                &files,
                &root.join("cancel"),
                OutputFormat::Png,
                100,
                0,
                &cancel,
                |_| {}
            )
            .unwrap()
            .contains("0/2")
        );
        assert!(collage(&files, Layout::Vertical, 64, 8, [255; 4], &cancel).is_err());
        for (path, data) in files.iter().zip(originals) {
            assert_eq!(std::fs::read(path).unwrap(), data);
        }
        assert!(
            std::fs::read_dir(&output)
                .unwrap()
                .all(|e| e.unwrap().path().extension().unwrap() == "jpg")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn manual_overlap_repairs_repeated_content() {
        let frame = Raster {
            width: 100,
            height: 100,
            pixels: vec![255; 40_000],
        };
        let mut doc = ScrollDocument::default();
        doc.append(frame.clone(), None).unwrap();
        assert!(doc.append(frame.clone(), None).is_err());
        assert_eq!(doc.frames.len(), 1);
        doc.append(frame, Some(20)).unwrap();
        assert_eq!(doc.compose().unwrap().height, 180);
    }
}

