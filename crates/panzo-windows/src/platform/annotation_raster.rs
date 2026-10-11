//! Shared annotation rasterization used by preview, still-frame capture and both export formats.
use super::{Mark, POINT, RECT, Raster, Tool, image_document};
use panzo_core::{
    AnnotationAnchor, AnnotationKind, FrameEvaluation, VideoAnnotation, WorkbenchSettings,
};
pub(super) fn render(
    items: &[&VideoAnnotation],
    evaluation: &FrameEvaluation,
    settings: &WorkbenchSettings,
    source_size: (u32, u32),
) -> Result<Raster, String> {
    let width = evaluation.output.width;
    let height = evaluation.output.height;
    let mut output = Raster {
        width,
        height,
        pixels: vec![0; Raster::bytes(width, height)?],
    };
    let content = evaluation.content_rect(source_size, settings.canvas.inset);
    for item in items {
        let source = item.anchor == AnnotationAnchor::Source;
        let point = |x: f64, y: f64| {
            let (x, y) = if source {
                let p = evaluation.camera_transform.source_to_output(x, y);
                (
                    content.left + p.x * (content.right - content.left),
                    content.top + p.y * (content.bottom - content.top),
                )
            } else {
                (x, y)
            };
            POINT {
                x: (x * f64::from(width)).round() as i32,
                y: (y * f64::from(height)).round() as i32,
            }
        };
        let a = point(item.bounds[0], item.bounds[1]);
        let b = point(item.bounds[2], item.bounds[3]);
        let scale = if source {
            let r = evaluation.camera_transform.source_rect();
            f64::from(height) * (content.bottom - content.top) / (r.bottom - r.top)
        } else {
            f64::from(height)
        };
        let stroke = (item.stroke * scale).round().clamp(1.0, 24.0) as u32;
        let font = (item.font_size * scale).round().clamp(6.0, 512.0) as u32;
        let padding = stroke as i32 * 4 + 16;
        let clip = if source {
            RECT {
                left: (content.left * f64::from(width)).ceil() as i32,
                top: (content.top * f64::from(height)).ceil() as i32,
                right: (content.right * f64::from(width)).floor() as i32,
                bottom: (content.bottom * f64::from(height)).floor() as i32,
            }
        } else {
            RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            }
        };
        let rect = RECT {
            left: (a.x.min(b.x) - padding).max(clip.left).max(0),
            top: (a.y.min(b.y) - padding).max(clip.top).max(0),
            right: (a.x.max(b.x) + padding).min(clip.right).min(width as i32),
            bottom: (a.y.max(b.y) + padding).min(clip.bottom).min(height as i32),
        };
        if rect.right <= rect.left || rect.bottom <= rect.top {
            continue;
        }
        let mut stencil = Raster {
            width: (rect.right - rect.left) as u32,
            height: (rect.bottom - rect.top) as u32,
            pixels: vec![
                0;
                Raster::bytes(
                    (rect.right - rect.left) as u32,
                    (rect.bottom - rect.top) as u32
                )?
            ],
        };
        let mark = Mark {
            text_box: None,
            tool: match item.kind {
                AnnotationKind::Text => Tool::Text,
                AnnotationKind::Arrow => Tool::Arrow,
                AnnotationKind::Rectangle => Tool::Rectangle,
                AnnotationKind::Ellipse => Tool::Ellipse,
                AnnotationKind::Highlight => Tool::Highlight,
                AnnotationKind::Cover => Tool::Cover,
            },
            points: vec![
                POINT {
                    x: a.x - rect.left,
                    y: a.y - rect.top,
                },
                POINT {
                    x: b.x - rect.left,
                    y: b.y - rect.top,
                },
            ],
            color: [255; 4],
            text: item.text.clone(),
            stroke,
            filled: false,
            font_size: font,
            font: Default::default(),
        };
        if item.kind == AnnotationKind::Text {
            image_document::render_text_in_rect(
                &mut stencil,
                &mark,
                RECT {
                    left: a.x.min(b.x) - rect.left,
                    top: a.y.min(b.y) - rect.top,
                    right: a.x.max(b.x) - rect.left,
                    bottom: a.y.max(b.y) - rect.top,
                },
            )?;
        } else {
            image_document::render_mark(&mut stencil, &mark)?;
        }
        let rgb = u32::from_str_radix(item.color.trim_start_matches('#'), 16)
            .map_err(|_| "标注颜色无效")?;
        let color = [
            (rgb & 255) as u8,
            ((rgb >> 8) & 255) as u8,
            ((rgb >> 16) & 255) as u8,
        ];
        for y in 0..stencil.height {
            for x in 0..stencil.width {
                let alpha = u32::from(
                    stencil.pixels[(y as usize * stencil.width as usize + x as usize) * 4],
                );
                if alpha == 0 {
                    continue;
                }
                let i = ((y as i32 + rect.top) as usize * width as usize
                    + (x as i32 + rect.left) as usize)
                    * 4;
                let old = u32::from(output.pixels[i + 3]);
                let total = alpha * 255 + old * (255 - alpha);
                for (c, value) in color.iter().enumerate() {
                    output.pixels[i + c] = ((u32::from(*value) * alpha * 255
                        + u32::from(output.pixels[i + c]) * old * (255 - alpha))
                        / total) as u8;
                }
                output.pixels[i + 3] = (total / 255) as u8;
            }
        }
    }
    Ok(output)
}
