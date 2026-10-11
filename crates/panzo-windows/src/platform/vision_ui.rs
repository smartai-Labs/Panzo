//! Native configuration and explicit image-upload actions for multimodal recognition.
use super::{
    capture_tools::Raster,
    image_tasks::{self, Output, Update},
    vision,
    vision_settings::{self, KeyChange},
};
use std::path::Path;
use windows::{
    Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK},
    },
    core::{PCWSTR, w},
};

fn message(hwnd: HWND, value: &str) {
    let value = value.encode_utf16().chain([0]).collect::<Vec<_>>();
    unsafe {
        let _ = crate::platform::app_dialog::message(
            Some(hwnd),
            PCWSTR(value.as_ptr()),
            w!("Panzo · 多模态识图"),
            MB_OK | MB_ICONERROR,
        );
    }
}
pub(crate) fn configure(hwnd: HWND) -> Result<bool, String> {
    configure_with(hwnd, &vision_settings::load()?, vision_settings::save)
}
fn configure_with(
    hwnd: HWND,
    previous: &vision_settings::Settings,
    save: impl Fn(vision_settings::Settings, KeyChange) -> Result<(), String>,
) -> Result<bool, String> {
    let mut fields = vec![
        ("API 地址", previous.base_url.clone()),
        ("视觉模型名称", previous.model.clone()),
        ("API Key（留空保留）", String::new()),
        ("密钥操作", "保留或更新".into()),
        (
            "默认识别方式",
            if previous.has_key() && !previous.default_online {
                "离线 OCR"
            } else {
                "多模态识图"
            }
            .into(),
        ),
        ("请求超时（秒）", previous.timeout_seconds.to_string()),
    ];
    let description = format!(
        "OpenAI 兼容接口，地址可填 /v1 或完整 /chat/completions。密钥{}，按当前 Windows 账户加密保存；留空保留已有密钥。",
        if previous.has_key() {
            "已保存"
        } else {
            "未保存"
        }
    );
    loop {
        let Some(values) = super::editor_dialog::form_with_password_fields(
            hwnd,
            "多模态识图设置",
            &description,
            &fields,
            &[
                (3, &["保留或更新", "删除已存密钥"]),
                (4, &["多模态识图", "离线 OCR"]),
            ],
            &[2],
            "保存配置",
        )
        .map_err(|e| e.to_string())?
        else {
            return Ok(false);
        };
        let result = (|| {
            let mut next = previous.clone();
            next.base_url.clone_from(&values[0]);
            next.model.clone_from(&values[1]);
            next.timeout_seconds = values[5]
                .trim()
                .parse()
                .map_err(|_| "请求超时须为 10–300 秒")?;
            next.default_online = values[4] == "多模态识图";
            let change = if values[3] == "删除已存密钥" {
                KeyChange::Clear
            } else if values[2].trim().is_empty() {
                KeyChange::Keep
            } else {
                KeyChange::Replace(values[2].clone())
            };
            save(next, change)
        })();
        match result {
            Ok(()) => return Ok(true),
            Err(error) => {
                message(hwnd, &error);
                for ((_, field), value) in fields.iter_mut().zip(values) {
                    *field = value;
                }
            }
        }
    }
}
fn ready_settings(hwnd: HWND) -> Result<Option<vision_settings::Settings>, String> {
    let mut settings = vision_settings::load()?;
    if !settings.has_key() || settings.model.trim().is_empty() {
        if !configure(hwnd)? {
            return Ok(None);
        }
        settings = vision_settings::load()?;
    }
    settings.validate()?;
    if !settings.has_key() {
        return Err("请先在多模态识图设置中填写 API Key".into());
    }
    Ok(Some(settings))
}
pub(crate) fn recognize(hwnd: HWND, image: Raster, library: &Path) -> Result<(), String> {
    super::plugins::require(super::plugins::VISION)?;
    let Some(settings) = ready_settings(hwnd)? else {
        return Ok(());
    };
    let host = vision::endpoint(&settings.base_url)?
        .host_str()
        .unwrap_or_default()
        .to_owned();
    let Some(values) = super::editor_dialog::form_with_description(
        hwnd, "多模态识图",
        &format!("将当前图片发送到 {host}，模型：{}。服务可能按用量计费。已添加的遮挡会保留；结果可校对、编辑和复制。", settings.model),
        &[("识图任务", "提取文字".into()), ("补充要求 / 提问", String::new())],
        &[(0, &["提取文字", "提取表格", "描述图片", "自定义提问"])], "发送识图",
    ).map_err(|e| e.to_string())? else { return Ok(()); };
    let preset = match values[0].as_str() {
        "提取表格" => vision::TABLE_PROMPT,
        "描述图片" => vision::DESCRIBE_PROMPT,
        "自定义提问" => "",
        _ => vision::OCR_PROMPT,
    };
    let prompt = [preset, values[1].trim()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if prompt.is_empty() {
        return Err("自定义提问不能为空，请填写要询问的内容".into());
    }
    image_tasks::start(
        "Panzo · 多模态识图",
        library,
        move |cancel, progress| {
            let _ = progress.send(Update::Progress(
                "正在发送图片并等待模型结果，可取消…".into(),
            ));
            vision::recognize(&settings, &image, &prompt, &cancel).map(Output::Text)
        },
    )
}
pub(crate) fn test_connection(hwnd: HWND, library: &Path) -> Result<(), String> {
    super::plugins::require(super::plugins::VISION)?;
    let Some(settings) = ready_settings(hwnd)? else {
        return Ok(());
    };
    image_tasks::start(
        "Panzo · 测试多模态连接",
        library,
        move |cancel, progress| {
            let _ = progress.send(Update::Progress(
                "正在发送应用生成的测试色块（不读取屏幕）…".into(),
            ));
            let image = Raster {
                width: 8,
                height: 8,
                pixels: [40, 90, 220, 255].repeat(64),
            };
            let text = vision::recognize(
                &settings,
                &image,
                "这是应用生成的连接测试色块，请只回复 PANZO。",
                &cancel,
            )?;
            Ok(Output::Text(format!(
                "连接成功，模型已返回图片请求结果。\r\n模型：{}\r\n\r\n{text}",
                settings.model
            )))
        },
    )
}

#[cfg(test)]
#[path = "vision_review.rs"]
mod review_tests;
