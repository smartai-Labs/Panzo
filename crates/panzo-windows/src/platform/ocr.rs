//! Offline Windows OCR. The application checks available language capabilities at runtime.
use crate::platform::{apartment::WinRtApartment, capture_tools::Raster};
use windows::{
    Globalization::Language,
    Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::OcrEngine,
    Storage::Streams::DataWriter,
    core::HSTRING,
};
#[derive(Debug, Clone, serde::Serialize)]
pub struct LanguageChoice {
    pub tag: String,
    pub name: String,
}
pub fn languages() -> Result<Vec<LanguageChoice>, String> {
    keep_mta_alive()?;
    let _apartment = WinRtApartment::multi_threaded().map_err(|e| e.to_string())?;
    let available = OcrEngine::AvailableRecognizerLanguages()
        .map_err(|e| format!("本地文字识别不可用：{e}"))?;
    available
        .into_iter()
        .map(|language| {
            Ok(LanguageChoice {
                tag: language
                    .LanguageTag()
                    .map_err(|e| e.to_string())?
                    .to_string(),
                name: language
                    .NativeName()
                    .map_err(|e| e.to_string())?
                    .to_string(),
            })
        })
        .collect()
}
pub(crate) fn recognize(image: &Raster, language: Option<&str>) -> Result<String, String> {
    keep_mta_alive()?;
    let _apartment = WinRtApartment::multi_threaded().map_err(|e| e.to_string())?;
    let engine = if let Some(tag) = language {
        let language = Language::CreateLanguage(&HSTRING::from(tag)).map_err(|e| e.to_string())?;
        OcrEngine::TryCreateFromLanguage(&language)
    } else {
        OcrEngine::TryCreateFromUserProfileLanguages()
    }
    .map_err(|_| "未安装所选语言的离线识别能力。请在图片工具的「OCR 语言管理」中安装后重试。")?;
    let limit = OcrEngine::MaxImageDimension().map_err(|e| e.to_string())?;
    if image.width > limit || image.height > limit {
        return Err(format!(
            "图片超出本地识别单次上限 {limit} 像素，请先裁剪需要识别的区域。"
        ));
    }
    let writer = DataWriter::new().map_err(|e| e.to_string())?;
    writer
        .WriteBytes(&image.pixels)
        .map_err(|e| e.to_string())?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
        &writer.DetachBuffer().map_err(|e| e.to_string())?,
        BitmapPixelFormat::Bgra8,
        i32::try_from(image.width).map_err(|e| e.to_string())?,
        i32::try_from(image.height).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let operation = engine.RecognizeAsync(&bitmap).map_err(|e| e.to_string())?;
    let result = operation.join().map_err(|e| format!("文字识别失败：{e}"))?;
    let lines = result.Lines().map_err(|e| e.to_string())?;
    let text = lines
        .into_iter()
        .map(|line| {
            line.Text()
                .map(|s| s.to_string())
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?
        .join("\r\n");
    let _ = bitmap.Close();
    if text.trim().is_empty() {
        Err("没有识别到文字，请尝试裁剪、更大的字号或切换识别语言。".into())
    } else {
        Ok(join_cjk(&text))
    }
}
// Windows OCR inserts word spaces inside CJK runs. Preserve Latin word spacing and line breaks.
fn join_cjk(text: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let cjk = |c: char| matches!(c,'\u{3400}'..='\u{9fff}'|'\u{f900}'..='\u{faff}');
    chars
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            if *c == ' ' && i > 0 && i + 1 < chars.len() && cjk(chars[i - 1]) && cjk(chars[i + 1]) {
                None
            } else {
                Some(*c)
            }
        })
        .collect()
}

// OCR activation factories outlive individual calls. Keep the process MTA alive while those
// Windows factories are cached; otherwise a later activation may dereference unloaded state.
fn keep_mta_alive() -> Result<(), String> {
    static COOKIE: std::sync::OnceLock<Result<usize, String>> = std::sync::OnceLock::new();
    COOKIE
        .get_or_init(|| {
            unsafe { windows::Win32::System::Com::CoIncrementMTAUsage() }
                .map(|cookie| cookie.0 as usize)
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map(|_| ())
        .map_err(Clone::clone)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cjk_spacing_keeps_english_and_newlines() {
        assert_eq!(
            join_cjk("截 图 文 字\r\nPanzo OCR 2026"),
            "截图文字\r\nPanzo OCR 2026"
        );
    }
    #[test]
    #[ignore = "requires installed Windows OCR capability and PANZO_STAGE_D_DIR; generated text only"]
    fn stage_d_native_ocr() {
        let root = std::path::PathBuf::from(
            std::env::var_os("PANZO_STAGE_D_DIR").expect("isolated output"),
        );
        std::fs::create_dir_all(&root).unwrap();
        let available = languages().unwrap();
        assert!(!available.is_empty());
        let image = crate::platform::capture_tools::review_ocr_image();
        image.save(&root.join("ocr-source.png")).unwrap();
        let language = available
            .iter()
            .find(|l| l.tag.starts_with("en"))
            .unwrap_or(&available[0]);
        let mut results = Vec::new();
        for _ in 0..3 {
            let image = image.clone();
            let tag = language.tag.clone();
            results.push(
                std::thread::spawn(move || recognize(&image, Some(&tag)).unwrap())
                    .join()
                    .unwrap(),
            );
        }
        let result = &results[0];
        assert!(results.iter().all(|text| text == result));
        assert!(result.to_ascii_lowercase().contains("panzo"), "{result}");
        if language.tag.starts_with("zh") {
            assert!(
                result.contains("截图") && result.contains("文字"),
                "{result}"
            );
        }
        std::fs::write(
            root.join("ocr-proof.json"),
            serde_json::to_vec_pretty(&serde_json::json!({"languages":available,"text":result,"expectedChinese":"截图识别文字","chineseExactMatch":result.contains("截图识别文字"),"repeatCount":3,"workerThreads":true}))
                .unwrap(),
        )
        .unwrap();
    }
}
