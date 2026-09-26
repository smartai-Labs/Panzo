use image::ImageReader;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const SUPPORTED_BACKGROUND_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "bmp"];
const MAX_BACKGROUND_DIMENSION: u32 = 16_384;
const MAX_BACKGROUND_PIXELS: u64 = 64 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedBackgroundImage {
    pub width: u32,
    pub height: u32,
    /// Tightly packed, top-down BGRA8 pixels.
    pub pixels: Vec<u8>,
    /// Bounded BGRA preview, prepared by the background loader, never during painting.
    pub preview: std::sync::Arc<image::RgbaImage>,
}

impl DecodedBackgroundImage {
    pub(crate) fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        let view =
            image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(width, height, pixels.as_slice())
                .expect("validated BGRA image dimensions");
        let preview = std::sync::Arc::new(image::imageops::thumbnail(
            &view,
            width.min(1024),
            height.min(384),
        ));
        Self {
            width,
            height,
            pixels,
            preview,
        }
    }
}

pub fn decode_background_bytes(
    bytes: &[u8],
    source: impl Into<PathBuf>,
) -> Result<DecodedBackgroundImage, BackgroundImageError> {
    let source = source.into();
    let header = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| BackgroundImageError::Decode {
            path: source.clone(),
            message: error.to_string(),
        })?;
    let (width, height) =
        header
            .into_dimensions()
            .map_err(|error| BackgroundImageError::Decode {
                path: source.clone(),
                message: error.to_string(),
            })?;
    validate_geometry(width, height, &source)?;
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| BackgroundImageError::Decode {
            path: source.clone(),
            message: error.to_string(),
        })?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_BACKGROUND_DIMENSION);
    limits.max_image_height = Some(MAX_BACKGROUND_DIMENSION);
    limits.max_alloc = Some(MAX_BACKGROUND_PIXELS * 8);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|error| BackgroundImageError::Decode {
            path: source.clone(),
            message: error.to_string(),
        })?;
    let width = decoded.width();
    let height = decoded.height();
    validate_geometry(width, height, &source)?;
    let mut pixels = decoded.into_rgba8().into_raw();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(DecodedBackgroundImage::new(width, height, pixels))
}

pub fn decode_background_file(
    path: impl AsRef<Path>,
) -> Result<DecodedBackgroundImage, BackgroundImageError> {
    let path = path.as_ref();
    let bytes = read_background_bytes(path)?;
    decode_background_bytes(&bytes, path)
}

pub fn read_background_bytes(path: &Path) -> Result<Vec<u8>, BackgroundImageError> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|source| BackgroundImageError::Read {
            path: path.into(),
            source,
        })?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(BackgroundImageError::Decode {
            path: path.into(),
            message: "背景文件不能超过 64 MiB".into(),
        });
    }
    Ok(bytes)
}

pub fn normalized_extension(path: &Path) -> Result<&'static str, BackgroundImageError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    SUPPORTED_BACKGROUND_EXTENSIONS
        .iter()
        .copied()
        .find(|candidate| extension.eq_ignore_ascii_case(candidate))
        .ok_or_else(|| BackgroundImageError::UnsupportedExtension(path.into()))
}

fn validate_geometry(width: u32, height: u32, path: &Path) -> Result<(), BackgroundImageError> {
    let pixels = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || width > MAX_BACKGROUND_DIMENSION
        || height > MAX_BACKGROUND_DIMENSION
        || pixels > MAX_BACKGROUND_PIXELS
    {
        return Err(BackgroundImageError::InvalidGeometry {
            path: path.into(),
            width,
            height,
        });
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum BackgroundImageError {
    #[error("unsupported background image extension: {0}")]
    UnsupportedExtension(PathBuf),
    #[error("failed to read background image {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to decode background image {path}: {message}")]
    Decode { path: PathBuf, message: String },
    #[error("background image {path} has unsupported geometry {width}x{height}")]
    InvalidGeometry {
        path: PathBuf,
        width: u32,
        height: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_png_to_bgra() {
        let rgba = image::RgbaImage::from_pixel(2, 1, image::Rgba([10, 20, 30, 255]));
        let mut bytes = Vec::new();
        rgba.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        let decoded = decode_background_bytes(&bytes, "test.png").unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 1));
        assert_eq!(decoded.pixels, [30, 20, 10, 255, 30, 20, 10, 255]);
    }

    #[test]
    fn rejects_unknown_extension() {
        assert!(matches!(
            normalized_extension(Path::new("background.gif")),
            Err(BackgroundImageError::UnsupportedExtension(_))
        ));
    }

    #[test]
    fn oversized_bmp_header_is_rejected_before_pixel_allocation() {
        // A tiny BMP declares a huge bitmap but contains no pixel buffer.
        let mut header = vec![0_u8; 54];
        header[..2].copy_from_slice(b"BM");
        header[2..6].copy_from_slice(&54_u32.to_le_bytes());
        header[10..14].copy_from_slice(&54_u32.to_le_bytes());
        header[14..18].copy_from_slice(&40_u32.to_le_bytes());
        header[18..22].copy_from_slice(&100_000_i32.to_le_bytes());
        header[22..26].copy_from_slice(&100_000_i32.to_le_bytes());
        header[26..28].copy_from_slice(&1_u16.to_le_bytes());
        header[28..30].copy_from_slice(&24_u16.to_le_bytes());
        let result = decode_background_bytes(&header, "huge.bmp");
        assert!(result.is_err(), "oversized header was accepted");
        // Some image decoders enforce their own header limits even before ours.
        assert!(matches!(
            result,
            Err(BackgroundImageError::InvalidGeometry { .. } | BackgroundImageError::Decode { .. })
        ));
    }
}
