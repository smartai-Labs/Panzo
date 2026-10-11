use super::{Raster, Surface};
use panzo_core::media_edit::{SubtitleCue, SubtitleStyle};
use windows::Win32::{
    Foundation::{COLORREF, RECT},
    Graphics::Gdi::*,
};
pub(super) fn render(
    out: &mut Raster,
    cues: &[&SubtitleCue],
    style: &SubtitleStyle,
) -> Result<(), String> {
    if cues.is_empty() || !style.enabled {
        return Ok(());
    }
    let text = cues
        .iter()
        .filter(|c| !c.text.trim().is_empty())
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() { return Ok(()); }
    let width = (out.width as f64 * 0.88).round().max(1.0) as u32;
    let max_height = (out.height as f64 * 0.5).round().max(1.0) as u32;
    let font_size = (out.height as f64 * style.font_size)
        .round()
        .clamp(1.0, 1024.0) as i32;
    let surface = Surface::new(width, max_height)?;
    unsafe {
        std::ptr::write_bytes(surface.bits, 0, surface.bytes);
    }
    let font = crate::platform::font_catalog::create_font(&style.font, font_size);
    if font.is_invalid() {
        return Err("无法创建字幕字体".into());
    }
    let old = unsafe { SelectObject(surface.dc, font.into()) };
    let mut text = text.encode_utf16().collect::<Vec<_>>();
    let mut r = RECT {
        left: 8,
        top: 0,
        right: width as i32 - 8,
        bottom: max_height as i32,
    };
    unsafe {
        SetBkMode(surface.dc, TRANSPARENT);
        SetTextColor(surface.dc, COLORREF(0xffffff));
        DrawTextW(
            surface.dc,
            &mut text,
            &raw mut r,
            crate::platform::font_catalog::alignment(style.font.alignment)
                | DT_WORDBREAK
                | DT_NOPREFIX
                | DT_CALCRECT,
        );
    }
    let height = (r.bottom + 12).clamp(1, max_height as i32);
    r.top = 6;
    r.left = 8;
    r.right = width as i32 - 8;
    r.bottom = height;
    unsafe {
        DrawTextW(
            surface.dc,
            &mut text,
            &raw mut r,
            crate::platform::font_catalog::alignment(style.font.alignment)
                | DT_WORDBREAK
                | DT_NOPREFIX,
        );
        SelectObject(surface.dc, old);
        let _ = DeleteObject(font.into());
    }
    let bitmap = surface.raster(width, max_height);
    let left = (out.width as i32 - width as i32) / 2;
    let top = (out.height as f64 * (1.0 - style.bottom)).round() as i32 - height;
    let rgb =
        u32::from_str_radix(style.color.trim_start_matches('#'), 16).map_err(|_| "字幕颜色无效")?;
    let color = [rgb as u8, (rgb >> 8) as u8, (rgb >> 16) as u8];
    if style.background {
        for y in 0..height {
            for x in 0..width as i32 {
                blend(out, left + x, top + y, [16, 16, 16], 190);
            }
        }
    }
    let stroke = (font_size / 20).clamp(1, 3);
    for y in 0..height {
        for x in 0..width as i32 {
            let a = ((u16::from(bitmap.pixels[(y as usize * width as usize + x as usize) * 4])
                * u16::from(style.opacity))
                / 255) as u8;
            if a == 0 {
                continue;
            }
            for (dx, dy) in [(-stroke, 0), (stroke, 0), (0, -stroke), (0, stroke)] {
                blend(out, left + x + dx, top + y + dy, [0, 0, 0], a);
            }
        }
    }
    for y in 0..height {
        for x in 0..width as i32 {
            let a = ((u16::from(bitmap.pixels[(y as usize * width as usize + x as usize) * 4])
                * u16::from(style.opacity))
                / 255) as u8;
            if a > 0 {
                blend(out, left + x, top + y, color, a);
            }
        }
    }
    Ok(())
}
fn blend(out: &mut Raster, x: i32, y: i32, color: [u8; 3], alpha: u8) {
    if x < 0 || y < 0 || x >= out.width as i32 || y >= out.height as i32 {
        return;
    }
    let i = (y as usize * out.width as usize + x as usize) * 4;
    let a = u32::from(alpha);
    let old = u32::from(out.pixels[i + 3]);
    let total = a * 255 + old * (255 - a);
    if total == 0 {
        return;
    }
    for c in 0..3 {
        out.pixels[i + c] = ((u32::from(color[c]) * a * 255
            + u32::from(out.pixels[i + c]) * old * (255 - a))
            / total) as u8;
    }
    out.pixels[i + 3] = (total / 255) as u8;
}
#[cfg(test)]
mod tests {
    use super::*;
    use panzo_core::{TimeTick, media_edit::TimeAnchor};
    #[test]
    fn centered_caption_is_burned_into_shared_transparent_overlay() {
        let mut image = Raster {
            width: 640,
            height: 360,
            pixels: vec![0; 640 * 360 * 4],
        };
        let cue = SubtitleCue {
            id: "s".into(),
            start: TimeTick::ZERO,
            end: TimeTick::from_millis(1000),
            text: "Panzo 字幕校对".into(),
            anchor: TimeAnchor::Project,
        };
        render(&mut image, &[&cue], &SubtitleStyle::default()).unwrap();
        assert!(
            image
                .pixels
                .chunks_exact(4)
                .any(|p| p[0] > 200 && p[1] > 200 && p[2] > 200 && p[3] > 200)
        );
        assert!(image.pixels[..640 * 180 * 4].iter().all(|p| *p == 0));
        let before = image.pixels.clone();
        render(&mut image, &[], &SubtitleStyle::default()).unwrap();
        assert_eq!(image.pixels, before);
    }
    #[test]
    fn subtitle_font_style_affects_shared_raster_and_opacity() {
        let cue=SubtitleCue{id:"font".into(),start:TimeTick::ZERO,end:TimeTick::from_millis(1000),text:"WWW iii Fonts".into(),anchor:TimeAnchor::Project};
        let image=||Raster{width:640,height:360,pixels:vec![0;640*360*4]};
        let mut style=SubtitleStyle{background:false,color:"#3280E0".into(),..Default::default()};
        style.font.family="Arial".into();style.font.weight=700;style.font.italic=true;style.font.underline=true;
        let mut a=image();render(&mut a,&[&cue],&style).unwrap();
        style.font.family="Courier New".into();style.font.weight=400;let mut b=image();render(&mut b,&[&cue],&style).unwrap();assert_ne!(a.pixels,b.pixels);
        style.opacity=0;let mut c=image();render(&mut c,&[&cue],&style).unwrap();assert!(c.pixels.iter().all(|v|*v==0));
        let decoded:SubtitleStyle=serde_json::from_str(&serde_json::to_string(&style).unwrap()).unwrap();assert_eq!(decoded,style);
    }

}
