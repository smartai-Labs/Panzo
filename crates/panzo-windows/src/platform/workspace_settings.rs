//! Persistent settings and inline provider test feedback.
use super::{
    image_tasks::Output,
    vision_settings::{self, KeyChange},
    workspace, workspace_form,
};
use std::{cell::Cell, rc::Rc};
use windows::Win32::Foundation::HWND;
#[allow(clippy::too_many_lines)] // Keep settings controls and their action callbacks together.
pub(crate) fn open() -> Result<(), String> {
    let previous = vision_settings::load()?;
    let holder = Rc::new(Cell::new(HWND::default()));
    let target = holder.clone();
    let fields = [
        ("API 地址", previous.base_url.clone()),
        ("视觉模型名称", previous.model.clone()),
        ("API Key（留空保留）", String::new()),
        ("密钥操作", "保留或更新".into()),
        (
            "默认识别方式",
            if previous.default_online {
                "多模态识图"
            } else {
                "离线 OCR"
            }
            .into(),
        ),
        ("请求超时（秒）", previous.timeout_seconds.to_string()),
    ];
    let h = workspace_form::open(
        3,
        crate::platform::workspace_form::FormId::Vision,
        "设置 · 多模态识图",
        &format!(
            "OpenAI 兼容接口。密钥{}，使用当前 Windows 账户加密长期保存。测试只发送应用生成的色块。",
            if previous.has_key() {
                "已保存"
            } else {
                "未保存"
            }
        ),
        &fields,
        &[
            (3, &["保留或更新", "删除已存密钥"]),
            (4, &["离线 OCR", "多模态识图"]),
        ],
        &[2],
        &["保存配置", "测试已保存配置", "OCR 语言", "外观与行为"],
        Box::new(move |action, v| match action {
            1 => {
                let mut next = vision_settings::load()?;
                next.base_url.clone_from(&v[0]);
                next.model.clone_from(&v[1]);
                next.default_online = v[4] == "多模态识图";
                next.timeout_seconds = v[5].trim().parse().map_err(|_| "超时须为 10–300 秒")?;
                let change = if v[3] == "删除已存密钥" {
                    KeyChange::Clear
                } else if v[2].trim().is_empty() {
                    KeyChange::Keep
                } else {
                    KeyChange::Replace(v[2].clone())
                };
                vision_settings::save(next, change)?;
                if let Some(edit)=workspace_form::field(target.get(),"api-key") {super::workspace_ui::text(edit,"");}
                Ok("配置已保存，重启后仍会保留。可返回图片工具开始识图。".into())
            }
            2 => {
                let lease = super::plugins::acquire(super::plugins::VISION)?;
                let settings = vision_settings::load()?;
                settings.validate()?;
                if !settings.has_key() {
                    return Err("请先保存 API Key".into());
                }
                workspace::start_job(
                    workspace::Target::Form(target.get()),
                    "连接测试",
                    move |cancel, _| {
                        let _lease = lease;
                        let image = super::capture_tools::Raster {
                            width: 8,
                            height: 8,
                            pixels: [40, 90, 220, 255].repeat(64),
                        };
                        let response = super::vision::recognize(
                            &settings,
                            &image,
                            "这是应用生成的连接测试色块，请只回复 PANZO。",
                            &cancel,
                        )?;
                        Ok(Output::Text(format!(
                            "连接成功 · {}\r\n{response}",
                            settings.model
                        )))
                    },
                );
                Ok("正在测试已保存的服务配置…".into())
            }
            3 => {
                super::image_workflows::manage_ocr_async()?;
                Ok(String::new())
            }
            _ => {
                general()?;
                Ok(String::new())
            }
        }),
    )?;
    holder.set(h);
    Ok(())
}
pub(crate) fn general() -> Result<(), String> {
    use super::ui_preferences as prefs;
    let form = workspace_form::open(
        3,
        crate::platform::workspace_form::FormId::General,
        "设置 · 外观与行为",
        "素材位置重启后生效；输出位置用于下次图片保存和视频导出。已有文件不会搬移。",
        &[
            (
                "关闭主窗口",
                if prefs::close_to_tray() {
                    "留在托盘"
                } else {
                    "退出应用"
                }
                .into(),
            ),
            (
                "界面主题",
                match prefs::theme() {
                    super::ui_preferences::ThemeMode::Light => "浅色",
                    super::ui_preferences::ThemeMode::Dark => "深色",
                }
                .into(),
            ),
            (
                "默认素材位置",
                prefs::library_directory()
                    .unwrap_or_else(|| {
                        std::env::var_os("USERPROFILE").map_or_else(
                            || std::path::PathBuf::from("."),
                            |p| std::path::PathBuf::from(p).join("Videos/Panzo"),
                        )
                    })
                    .display()
                    .to_string(),
            ),
            (
                "界面动画",
                if prefs::animations() {
                    "跟随系统"
                } else {
                    "关闭"
                }
                .into(),
            ),
            ("默认输出位置",prefs::output_directory().unwrap_or_else(super::workspace::library_path).display().to_string()),
        ],
        &[
            (0, &["留在托盘", "退出应用"]),
            (1, &["浅色", "深色"]),
            (3, &["跟随系统", "关闭"]),
        ],
        &[],
        &["保存", "多模态设置"],
        Box::new(|action, v| {
            if action == 2 {
                open()?;
                return Ok(String::new());
            }
            let directory = super::path_field::validate(&v[2])?;
            let output=super::path_field::validate(&v[4])?;
            prefs::set_directories(directory,output)?;
            prefs::set_close_to_tray(v[0] == "留在托盘")?;
            prefs::set_animations(v[3] != "关闭")?;
            prefs::set_theme(match v[1].as_str() {
                "深色" => super::ui_preferences::ThemeMode::Dark,
                _ => super::ui_preferences::ThemeMode::Light,
            })?;
            Ok("已保存。素材位置重启后生效；默认输出位置在下次保存图片或导出视频时使用。".into())
        }),
    )?;
    sync_theme(form);
    Ok(())
}

pub(crate) fn sync_theme(form: HWND) {
    use windows::Win32::{
        Foundation::WPARAM,
        UI::WindowsAndMessaging::{CB_SETCURSEL, SendMessageW},
    };
    if workspace_form::identity(form) == Some(workspace_form::FormId::General)
        && let Some(combo) = workspace_form::field(form, "theme")
    {
        unsafe {
            SendMessageW(
                combo,
                CB_SETCURSEL,
                Some(WPARAM(usize::from(
                    super::ui_preferences::theme() == super::ui_preferences::ThemeMode::Dark,
                ))),
                None,
            );
        }
    }
}
