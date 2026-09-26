use super::*;
use crate::platform::{
    fmp4_writer::{Nv12Fmp4Writer, Nv12VideoConfig},
    media_decoder::MfBgraDecoder,
};
use std::{fs, path::PathBuf};

const COLORS: [[u8; 3]; 12] = [
    [0, 0, 0],
    [255, 255, 255],
    [128, 128, 128],
    [32, 32, 32],
    [255, 0, 0],
    [0, 255, 0],
    [0, 0, 255],
    [255, 255, 0],
    [0, 255, 255],
    [255, 0, 255],
    [38, 103, 181],
    [221, 137, 75],
];

fn color_chart(width: u32, height: u32) -> Vec<u8> {
    (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                let [r, g, b] = COLORS[((y * 3 / height) * 4 + x * 4 / width) as usize];
                [b, g, r, 255]
            })
        })
        .collect()
}

// Independent BT.709 limited-range reference, evaluated away from subsampled edges.
fn reference_yuv([r, g, b]: [u8; 3]) -> [f64; 3] {
    let (r, g, b) = (f64::from(r), f64::from(g), f64::from(b));
    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    [
        16.0 + 219.0 * luma / 255.0,
        128.0 + 224.0 * (b - luma) / (2.0 * (1.0 - 0.0722) * 255.0),
        128.0 + 224.0 * (r - luma) / (2.0 * (1.0 - 0.2126) * 255.0),
    ]
}

#[test]
fn nv12_conversion_matches_bt709_at_native_and_scaled_sizes() {
    let device = D3d11Device::create_hardware().unwrap();
    let pixels = color_chart(1280, 720);
    for (width, height) in [(1280, 720), (640, 360)] {
        let converter =
            Nv12VideoProcessor::create_with_output(&device, 1280, 720, width, height).unwrap();
        let nv12 = converter.convert_bgra_bytes(&pixels).unwrap();
        for (index, color) in COLORS.into_iter().enumerate() {
            let x = (index % 4) * width as usize / 4 + width as usize / 8;
            let y = (index / 4) * height as usize / 3 + height as usize / 6;
            let uv = width as usize * height as usize + (y / 2) * width as usize + (x / 2) * 2;
            let actual = [nv12[y * width as usize + x], nv12[uv], nv12[uv + 1]];
            let expected = reference_yuv(color);
            for channel in 0..3 {
                assert!(
                    (f64::from(actual[channel]) - expected[channel]).abs() <= 1.5,
                    "{width}x{height} RGB {color:?}: YUV {actual:?}, expected {expected:?}"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires GPU, hardware encoder and explicit output directory"]
fn color_roundtrip_ui_review() {
    let output = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
    let device = D3d11Device::create_hardware().unwrap();
    let pixels = color_chart(1280, 720);
    let converter = Nv12VideoProcessor::create_with_output(&device, 1280, 720, 1280, 720).unwrap();
    let nv12 = converter.convert_bgra_bytes(&pixels).unwrap();
    let path = output.join(format!("color-chart-{}.mp4", uuid::Uuid::new_v4()));
    let mut writer = Nv12Fmp4Writer::create_with_config(
        &path,
        Nv12VideoConfig::native_screen_60(1280, 720).unwrap(),
    )
    .unwrap();
    for index in 0..60 {
        let start = index * panzo_core::TICKS_PER_SECOND / 60;
        let end = (index + 1) * panzo_core::TICKS_PER_SECOND / 60;
        writer.write_nv12(&nv12, start, end - start).unwrap();
    }
    let encoder = writer.encoder_name().to_owned();
    writer.finish().unwrap();
    let mut decoder = MfBgraDecoder::open(&path).unwrap();
    let frame = decoder.read_next().unwrap().unwrap();
    let mut errors = Vec::new();
    for (index, expected) in COLORS.into_iter().enumerate() {
        let x = (index % 4) * 320 + 160;
        let y = (index / 4) * 240 + 120;
        let mut sums = [0_u64; 3];
        for row in y - 16..y + 16 {
            for col in x - 16..x + 16 {
                let at = (row * 1280 + col) * 4;
                for (channel, sum) in sums.iter_mut().enumerate() {
                    *sum += u64::from(frame.pixels[at + 2 - channel]);
                }
            }
        }
        let actual = sums.map(|sum| sum as f64 / 1024.0);
        let max_error = (0..3)
            .map(|c| (actual[c] - f64::from(expected[c])).abs())
            .fold(0.0, f64::max);
        errors.push(serde_json::json!({"expectedRgb":expected,"decodedRgb":actual,"maxChannelError":max_error}));
    }
    for (name, bgra) in [
        ("color-original.png", &pixels),
        ("color-decoded.png", &frame.pixels),
    ] {
        let rgba: Vec<_> = bgra
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        image::save_buffer(output.join(name), &rgba, 1280, 720, image::ColorType::Rgba8).unwrap();
    }
    fs::write(
        output.join("color-roundtrip.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "mp4":path,"encoder":encoder,"patches":errors
        }))
        .unwrap(),
    )
    .unwrap();
    for error in errors {
        assert!(error["maxChannelError"].as_f64().unwrap() <= 3.0, "{error}");
    }
}
