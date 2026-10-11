//! Regenerate the checked-in Windows application / MSIX icons without extra tools.
//! cargo run -p panzo-windows --example generate_app_icon --offline
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
    let svg = fs::read_to_string(directory.join("panzo.svg"))?;
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default())?;
    let icon_sizes = [16_u32, 20, 24, 32, 40, 48, 64, 96, 128, 256];
    let mut images = Vec::new();
    for size in icon_sizes.into_iter().chain([44, 50, 150]) {
        let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or("Invalid icon size")?;
        let scale = size as f32 / tree.size().width();
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        let png = pixmap.encode_png()?;
        fs::write(directory.join(format!("panzo-{size}.png")), &png)?;
        if icon_sizes.contains(&size) {
            images.push((size, png));
        }
    }
    // Windows Vista+ supports lossless RGBA PNG entries in an ICO container.
    let mut ico = vec![0, 0, 1, 0];
    ico.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, png) in &images {
        ico.extend_from_slice(&[*size as u8, *size as u8, 0, 0]); // 0 means 256 px.
        ico.extend_from_slice(&1_u16.to_le_bytes());
        ico.extend_from_slice(&32_u16.to_le_bytes());
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in images {
        ico.extend_from_slice(&png);
    }
    fs::write(directory.join("panzo.ico"), ico)?;
    println!("Generated application icons in {}", directory.display());
    Ok(())
}
