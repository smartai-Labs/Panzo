//! Stateful source list and parameters for collage and batch conversion.
use super::{
    image_ops::{Layout, OutputFormat},
    image_tasks::{Output, Update},
    workspace, workspace_form, workspace_ui as ui,
};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
};
use windows::Win32::Foundation::HWND;
fn integer(v: &str, min: u32, max: u32) -> Result<u32, String> {
    v.trim()
        .parse::<u32>()
        .ok()
        .filter(|n| (min..=max).contains(n))
        .ok_or_else(|| format!("请输入 {min}–{max} 之间的整数"))
}
#[allow(clippy::too_many_lines)] // Keep inspector creation and its validated callback together.
pub(crate) fn open(batch: bool, library: &Path) -> Result<(), String> {
    super::plugins::require(super::plugins::IMAGES)?;
    if !batch {
        return super::collage_workspace::open(library);
    }
    let directory = library.join(format!("图片批处理-{}", uuid::Uuid::new_v4()));
    let fields = if batch {
        vec![
            ("输出格式", "JPEG".into()),
            ("JPEG 质量（10–100）", "85".into()),
            ("最长边（0 保持原尺寸）", "1920".into()),
            ("输出文件夹", directory.display().to_string()),
        ]
    } else {
        vec![
            ("布局", "纵向".into()),
            ("每张最长边（像素）", "1200".into()),
            ("间距（像素）", "16".into()),
            ("排序", "原顺序".into()),
            ("背景", "白色".into()),
        ]
    };
    let registry_library = library.to_path_buf();
    let mut paths: Vec<PathBuf> = Vec::new();
    let holder = Rc::new(Cell::new(HWND::default()));
    let target = holder.clone();
    let mut running: Option<std::sync::Arc<std::sync::atomic::AtomicBool>> = None;
    let h = workspace_form::open(
        0,
        crate::platform::workspace_form::FormId::Batch,
        if batch {
            "批量压缩与格式转换"
        } else {
            "图片拼图"
        },
        "添加素材后，在下方列表确认文件。参数和列表会保留；切换功能不会取消处理。原文件保留。",
        &fields,
        if batch {
            &[(0, &["JPEG", "PNG", "BMP"])]
        } else {
            &[
                (0, &["纵向", "横向", "2 列", "3 列", "4 列"]),
                (3, &["原顺序", "反序"]),
                (4, &["白色", "黑色", "浅灰"]),
            ]
        },
        &[],
        &["添加素材", "开始处理", "清空列表", "取消任务"],
        Box::new(move |action, v| match action {
            1 => {
                for path in super::image_workflows::choose_images(target.get(), true, false)? {
                    if !paths.contains(&path) {
                        paths.push(path);
                    }
                }
                Ok(paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\r\n"))
            }
            3 => {
                paths.clear();
                Ok("素材列表已清空".into())
            }
            4 => {
                if let Some(c) = &running {
                    c.store(true, std::sync::atomic::Ordering::Release);
                }
                Ok("已请求取消任务".into())
            }
            _ => {
                if paths.is_empty() {
                    return Err("请先添加素材".into());
                }
                let lease = super::plugins::acquire(super::plugins::IMAGES)?;
                let mut input = paths.clone();
                let token = if batch {
                    let format = match v[0].as_str() {
                        "PNG" => OutputFormat::Png,
                        "BMP" => OutputFormat::Bmp,
                        _ => OutputFormat::Jpeg,
                    };
                    let quality = integer(&v[1], 10, 100)? as u8;
                    let longest = integer(&v[2], 0, 8192)?;
                    let directory = PathBuf::from(v[3].trim());
                    if !directory.is_absolute() {
                        return Err("请输入绝对路径作为输出文件夹".into());
                    }
                    let registry = registry_library.clone();
                    workspace::start_job(
                        workspace::Target::Form(target.get()),
                        "批处理",
                        move |cancel, tx| {
                            let _lease = lease;
                            super::image_ops::batch_registered(
                                &input,
                                &directory,
                                format,
                                quality,
                                longest,
                                &cancel,
                                Some(&registry),
                                |s| {
                                    let _ = tx.send(Update::Progress(s));
                                },
                            )
                            .map(Output::Text)
                        },
                    )
                } else {
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
                        input.reverse();
                    }
                    let bg = match v[4].as_str() {
                        "黑色" => [0, 0, 0, 255],
                        "浅灰" => [235, 235, 235, 255],
                        _ => [255; 4],
                    };
                    workspace::start_job(
                        workspace::Target::Form(target.get()),
                        "拼图",
                        move |cancel, _| {
                            let _lease = lease;
                            super::image_ops::collage(&input, layout, cell, gap, bg, &cancel)
                                .map(Output::Image)
                        },
                    )
                };
                running = Some(token);
                Ok(format!(
                    "已开始处理 {} 张图片。结果会回到本工作区。",
                    paths.len()
                ))
            }
        }),
    )?;
    holder.set(h);
    ui::text(h, "图片处理");
    Ok(())
}
