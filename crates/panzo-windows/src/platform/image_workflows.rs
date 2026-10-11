//! Entry points for image tools; results stay inside the same image editor and history.
use super::{
    capture_tools::Raster,
    image_ops::{Layout, OutputFormat},
    image_tasks::{self, Output, Update},
};
use std::{
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};
use windows::{
    Win32::{
        Foundation::HWND,
        UI::Controls::Dialogs::{
            CommDlgExtendedError, GetOpenFileNameW, OFN_ALLOWMULTISELECT, OFN_EXPLORER,
            OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPEN_FILENAME_FLAGS,
            OPENFILENAMEW,
        },
    },
    core::{PCWSTR, w},
};
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}
pub(crate) fn choose_images(
    owner: HWND,
    multiple: bool,
    gif: bool,
) -> Result<Vec<PathBuf>, String> {
    let mut names = vec![0u16; 65536];
    let filter = wide(if gif {
        "图片 / GIF\0*.png;*.jpg;*.jpeg;*.bmp;*.gif\0\0"
    } else {
        "静态图片\0*.png;*.jpg;*.jpeg;*.bmp\0\0"
    });
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(names.as_mut_ptr()),
        nMaxFile: names.len() as u32,
        Flags: OFN_FILEMUSTEXIST
            | OFN_PATHMUSTEXIST
            | OFN_NOCHANGEDIR
            | OFN_EXPLORER
            | if multiple {
                OFN_ALLOWMULTISELECT
            } else {
                OPEN_FILENAME_FLAGS(0)
            },
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&raw mut dialog) }.as_bool() {
        let error = unsafe { CommDlgExtendedError() };
        if error.0 != 0 {
            return Err(format!(
                "文件选择失败：{}，请减少一次选择的文件数量",
                error.0
            ));
        }
        return Ok(Vec::new());
    }
    let parts = names
        .split(|v| *v == 0)
        .take_while(|v| !v.is_empty())
        .map(|part| PathBuf::from(std::ffi::OsString::from_wide(part)))
        .collect::<Vec<_>>();
    if parts.len() == 1 {
        Ok(parts)
    } else {
        Ok(parts[1..].iter().map(|p| parts[0].join(p)).collect())
    }
}
fn integer(value: &str, min: u32, max: u32) -> Result<u32, String> {
    value
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|v| (min..=max).contains(v))
        .ok_or_else(|| format!("请输入 {min}–{max} 之间的整数"))
}
pub(crate) fn ocr(hwnd: HWND, image: Raster, library: &Path) -> Result<(), String> {
    loop {
        let online = super::vision_settings::load().is_ok_and(|s| s.default_online && s.has_key());
        let Some(values) = super::editor_dialog::form_with_description(
            hwnd, "文字识别与识图", "离线 OCR 在本机处理。多模态识图可提取文字、表格或回答图片问题；发送前可查看服务和模型。",
            &[("识别方式", if online { "多模态识图" } else { "离线 OCR" }.into())],
            &[(0, &["离线 OCR", "多模态识图", "配置多模态服务"])], "继续",
        ).map_err(|e| e.to_string())? else { return Ok(()); };
        match values[0].as_str() {
            "配置多模态服务" => {
                if !super::vision_ui::configure(hwnd)? {
                    return Ok(());
                }
            }
            "多模态识图" => return super::vision_ui::recognize(hwnd, image, library),
            _ => return offline_ocr(hwnd, image, library),
        }
    }
}
fn offline_ocr(hwnd: HWND, image: Raster, library: &Path) -> Result<(), String> {
    super::plugins::require(super::plugins::OCR)?;
    let languages = super::ocr::languages()?;
    if languages.is_empty() {
        return manage_ocr(hwnd);
    }
    let names = std::iter::once("自动（用户语言）".to_owned())
        .chain(languages.iter().map(|l| format!("{} ({})", l.name, l.tag)))
        .collect::<Vec<_>>();
    let options = names.iter().map(String::as_str).collect::<Vec<_>>();
    let Some(values) = super::editor_dialog::form_with_description(
        hwnd,
        "识别文字",
        "在本机离线识别。已添加的遮挡会保持遮挡；识别完成后可校对、编辑并复制文字。",
        &[("识别语言", names[0].clone())],
        &[(0, &options)],
        "开始识别",
    )
    .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    let language = names
        .iter()
        .position(|v| v == &values[0])
        .and_then(|i| i.checked_sub(1))
        .map(|i| languages[i].tag.clone());
    image_tasks::start("Panzo · 识别文字", library, move |cancel, _| {
        let result = super::ocr::recognize(&image, language.as_deref())?;
        if cancel.load(Ordering::Acquire) {
            Err("识别已取消".into())
        } else {
            Ok(Output::Text(result))
        }
    })
}
pub(crate) fn collage(hwnd: HWND, library: &Path) -> Result<(), String> {
    super::plugins::require(super::plugins::IMAGES)?;
    let mut paths = choose_images(hwnd, true, false)?;
    if paths.is_empty() {
        return Ok(());
    }
    let Some(v) = super::editor_dialog::form_with_description(
        hwnd,
        "图片拼图",
        "按文件选择顺序排列，可选择反序。拼图完成后继续标注、裁剪、保存或贴到桌面。",
        &[
            ("布局", "纵向".into()),
            ("每张最长边（像素）", "1200".into()),
            ("间距（像素）", "16".into()),
            ("排序", "原顺序".into()),
            ("背景", "白色".into()),
        ],
        &[
            (0, &["纵向", "横向", "2 列", "3 列", "4 列"]),
            (3, &["原顺序", "反序"]),
            (4, &["白色", "黑色", "浅灰"]),
        ],
        "生成拼图",
    )
    .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    let layout = match v[0].as_str() {
        "横向" => Layout::Horizontal,
        "2 列" => Layout::Grid(2),
        "3 列" => Layout::Grid(3),
        "4 列" => Layout::Grid(4),
        _ => Layout::Vertical,
    };
    let cell = integer(&v[1], 64, 4096)?;
    let gap = integer(&v[2], 0, 100)?;
    if v[3] == "反序" {
        paths.reverse();
    }
    let bg = match v[4].as_str() {
        "黑色" => [0, 0, 0, 255],
        "浅灰" => [235, 235, 235, 255],
        _ => [255; 4],
    };
    image_tasks::start("Panzo · 拼图", library, move |cancel, _| {
        super::image_ops::collage(&paths, layout, cell, gap, bg, &cancel).map(Output::Image)
    })
}
pub(crate) fn batch(hwnd: HWND, library: &Path) -> Result<(), String> {
    super::plugins::require(super::plugins::IMAGES)?;
    let paths = choose_images(hwnd, true, false)?;
    if paths.is_empty() {
        return Ok(());
    }
    let directory = library.join(format!("图片批处理-{}", uuid::Uuid::new_v4()));
    let Some(v)=super::editor_dialog::form_with_description(hwnd,"批量压缩与格式转换","原文件保留。PNG 无损压缩；JPEG 会以白色填充透明区。最长边为 0 时保持尺寸，不放大小图。输出已存在时跳过并报告。",&[("输出格式","JPEG".into()),("JPEG 质量（10–100）","85".into()),("最长边（0–8192）","1920".into()),("输出文件夹",directory.display().to_string())],&[(0,&["JPEG","PNG","BMP"])],"开始处理").map_err(|e|e.to_string())?else{return Ok(());};
    let format = match v[0].as_str() {
        "PNG" => OutputFormat::Png,
        "BMP" => OutputFormat::Bmp,
        _ => OutputFormat::Jpeg,
    };
    let quality = integer(&v[1], 10, 100)? as u8;
    let longest = integer(&v[2], 0, 8192)?;
    let directory = PathBuf::from(v[3].trim());
    if !directory.is_absolute() {
        return Err("请选择绝对路径作为输出文件夹".into());
    }
    let registry = library.to_path_buf();
    image_tasks::start(
        "Panzo · 批量图片处理",
        library,
        move |cancel, updates| {
            super::image_ops::batch_registered(
                &paths,
                &directory,
                format,
                quality,
                longest,
                &cancel,
                Some(&registry),
                |text| {
                    let _ = updates.send(Update::Progress(text));
                },
            )
            .map(Output::Text)
        },
    )
}
pub(crate) fn manage_ocr(hwnd: HWND) -> Result<(), String> {
    let installed = super::ocr::languages()?
        .iter()
        .map(|l| l.tag.clone())
        .collect::<Vec<_>>()
        .join("、");
    let Some(v)=super::editor_dialog::form_with_description(hwnd,"OCR 语言管理",&format!("已安装：{}。安装调用 Windows 可选能力，需要联网和管理员同意；完成后重新打开此窗口检测。",if installed.is_empty(){"无"}else{&installed}),&[("语言","简体中文".into()),("操作","仅检测".into())],&[(0,&["简体中文","繁体中文","英语"]),(1,&["仅检测","安装离线识别能力"])],"确定").map_err(|e|e.to_string())?else{return Ok(());};
    if v[1] != "安装离线识别能力" {
        return Ok(());
    }
    install_ocr(super::workspace::root().unwrap_or_default(), &v[0])
}
fn install_ocr(hwnd: HWND, language: &str) -> Result<(), String> {
    let tag = match language {
        "繁体中文" => "zh-TW",
        "英语" => "en-US",
        _ => "zh-CN",
    };
    let command = format!(
        "-NoProfile -WindowStyle Hidden -Command \"$ErrorActionPreference='Stop'; $ocrDir=Join-Path $env:LOCALAPPDATA 'Panzo'; New-Item -ItemType Directory -Force $ocrDir | Out-Null; try {{ Add-WindowsCapability -Online -Name 'Language.Basic~~~{tag}~0.0.1.0'; Add-WindowsCapability -Online -Name 'Language.OCR~~~{tag}~0.0.1.0' }} catch {{ $_ | Out-File -LiteralPath (Join-Path $ocrDir 'ocr-language-install.log'); exit 1 }}\""
    );
    let exe = std::env::var_os("SystemRoot")
        .map_or_else(|| PathBuf::from("C:/Windows"), PathBuf::from)
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let exe = wide(&exe.to_string_lossy());
    let command = wide(&command);
    let result = unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(
            Some(hwnd),
            w!("runas"),
            PCWSTR(exe.as_ptr()),
            PCWSTR(command.as_ptr()),
            PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::SW_HIDE,
        )
    };
    if result.0 as isize <= 32 {
        return Err("系统安装未启动或授权已取消".into());
    }
    Ok(())
}

pub(crate) fn manage_ocr_async() -> Result<(), String> {
    let installed = super::ocr::languages()?
        .iter()
        .map(|l| l.tag.clone())
        .collect::<Vec<_>>()
        .join("、");
    super::workspace_form::open(
        3,
        crate::platform::workspace_form::FormId::Ocr,
        "设置 · OCR 语言",
        &format!("已安装：{installed}。安装 Windows 离线识别能力需要联网及管理员同意。"),
        &[("语言", "简体中文".into())],
        &[(0, &["简体中文", "繁体中文", "英语"])],
        &[],
        &["安装离线识别能力", "刷新检测", "多模态设置"],
        Box::new(|action, v| match action {
            1 => {
                install_ocr(super::workspace::root().unwrap_or_default(), &v[0])?;
                Ok("已启动 Windows 安装，完成后点击刷新检测。".into())
            }
            2 => Ok(super::ocr::languages()?
                .iter()
                .map(|l| format!("{} ({})", l.name, l.tag))
                .collect::<Vec<_>>()
                .join("\r\n")),
            _ => {
                super::workspace_settings::open()?;
                Ok(String::new())
            }
        }),
    )?;
    Ok(())
}
