//! Native annotation manager. All edits flow through the shared workbench undo/save pipeline.
use super::editor_dialog::form_with_description;
use panzo_core::{AnnotationAnchor, AnnotationKind, TimeTick, VideoAnnotation, WorkbenchSettings};
use windows::Win32::Foundation::HWND;
const KINDS: &[&str] = &["文字", "箭头", "矩形", "椭圆", "荧光", "遮挡"];
fn kind_name(kind: AnnotationKind) -> &'static str {
    match kind {
        AnnotationKind::Text => KINDS[0],
        AnnotationKind::Arrow => KINDS[1],
        AnnotationKind::Rectangle => KINDS[2],
        AnnotationKind::Ellipse => KINDS[3],
        AnnotationKind::Highlight => KINDS[4],
        AnnotationKind::Cover => KINDS[5],
    }
}
fn numbers<const N: usize>(value: &str) -> Result<[f64; N], String> {
    let v = value
        .split([',', '，', ' ', ';'])
        .filter(|v| !v.is_empty())
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "请输入数字，以逗号分隔")?;
    if v.len() != N || v.iter().any(|n| !n.is_finite()) {
        return Err(format!("请输入 {N} 个有限数字，以逗号分隔"));
    }
    Ok(std::array::from_fn(|i| v[i]))
}
pub(crate) fn edit(
    hwnd: HWND,
    settings: &mut WorkbenchSettings,
    duration: TimeTick,
    source: TimeTick,
) -> Result<bool, String> {
    let mut names = vec!["＋ 新增标注".to_owned()];
    names.extend(settings.annotations.iter().enumerate().map(|(i, a)| {
        format!(
            "{} · {} · {:.2}–{:.2}s · {}",
            i + 1,
            kind_name(a.kind),
            a.start.0 as f64 / 1e7,
            a.end.0 as f64 / 1e7,
            a.text.chars().take(16).collect::<String>()
        )
    }));
    let choices = names.iter().map(String::as_str).collect::<Vec<_>>();
    let Some(v)=form_with_description(hwnd,"视频标注","文字、箭头、形状与遮挡会一并导出到 MP4 / GIF。按素材时间定位，剪辑和变速后仍与原画面对齐。",&[("标注",names[0].clone()),("操作","编辑".into())],&[(0,&choices),(1,&["编辑","删除"])],"继续").map_err(|e|e.to_string())?else{return Ok(false);};
    let index = names.iter().position(|n| n == &v[0]).ok_or("请选择标注")?;
    if v[1] == "删除" {
        if index == 0 {
            return Ok(false);
        }
        settings.annotations.remove(index - 1);
        return Ok(true);
    }
    if index == 0 && settings.annotations.len() >= 200 {
        return Err("每个工程最多 200 个标注".into());
    }
    let start = source.0.clamp(0, (duration.0 - 1).max(0));
    let mut a = if index == 0 {
        VideoAnnotation {
            id: uuid::Uuid::new_v4().to_string(),
            start: TimeTick(start),
            end: TimeTick((start + 20_000_000).min(duration.0)),
            kind: AnnotationKind::Text,
            anchor: AnnotationAnchor::Canvas,
            bounds: [0.1, 0.1, 0.8, 0.3],
            text: "说明文字".into(),
            color: "#EF4444".into(),
            stroke: 0.004,
            font_size: 0.05,
        }
    } else {
        settings.annotations[index - 1].clone()
    };
    let bounds = a.bounds.map(|n| format!("{:.2}", n * 100.0)).join(", ");
    let fields = [
        ("类型", kind_name(a.kind).into()),
        ("文字内容", a.text.clone()),
        (
            "素材起止秒（逗号）",
            format!("{:.3}, {:.3}", a.start.0 as f64 / 1e7, a.end.0 as f64 / 1e7),
        ),
        ("两点 x,y,x,y（%）", bounds),
        ("颜色 #RRGGBB", a.color.clone()),
        (
            "线宽、字号（高度%）",
            format!("{}, {}", a.stroke * 100.0, a.font_size * 100.0),
        ),
        (
            "位置参照",
            if a.anchor == AnnotationAnchor::Canvas {
                "固定画布"
            } else {
                "跟随素材"
            }
            .into(),
        ),
    ];
    let Some(v)=form_with_description(hwnd,"编辑视频标注","两点定义文字框/形状或箭头起终点（0–100%）。固定画布适合标题；跟随素材会随镜头和裁剪移动。应用后可预览，可撤销。",&fields,&[(0,KINDS),(6,&["固定画布","跟随素材"])],"应用标注").map_err(|e|e.to_string())?else{return Ok(false);};
    a.kind = match v[0].as_str() {
        "箭头" => AnnotationKind::Arrow,
        "矩形" => AnnotationKind::Rectangle,
        "椭圆" => AnnotationKind::Ellipse,
        "荧光" => AnnotationKind::Highlight,
        "遮挡" => AnnotationKind::Cover,
        _ => AnnotationKind::Text,
    };
    a.text.clone_from(&v[1]);
    let times = numbers::<2>(&v[2])?;
    if times[0] < 0.0 || times[1] > duration.0 as f64 / 1e7 || times[1] <= times[0] {
        return Err("素材起止时间超出范围".into());
    }
    a.start = TimeTick((times[0] * 1e7).round() as i64);
    a.end = TimeTick((times[1] * 1e7).round() as i64);
    a.bounds = numbers::<4>(&v[3])?.map(|n| n / 100.0);
    a.color = v[4].trim().into();
    let style = numbers::<2>(&v[5])?;
    a.stroke = style[0] / 100.0;
    a.font_size = style[1] / 100.0;
    a.anchor = if v[6] == "跟随素材" {
        AnnotationAnchor::Source
    } else {
        AnnotationAnchor::Canvas
    };
    if !a.valid() {
        return Err("请检查位置、颜色、线宽（0.05–3%）和字号（0.5–20%）".into());
    }
    if index == 0 {
        settings.annotations.push(a);
    } else {
        settings.annotations[index - 1] = a;
    }
    Ok(true)
}

pub(crate) fn edit_async(
    hwnd: HWND,
    project: std::path::PathBuf,
    mut settings: WorkbenchSettings,
    duration: TimeTick,
    source: TimeTick,
) -> Result<(), String> {
    let names = std::iter::once("＋ 新增标注".to_owned())
        .chain(settings.annotations.iter().enumerate().map(|(i, a)| {
            format!(
                "{} · {} · {}",
                i + 1,
                kind_name(a.kind),
                a.text.chars().take(16).collect::<String>()
            )
        }))
        .collect::<Vec<_>>();
    let choices = names.iter().map(String::as_str).collect::<Vec<_>>();
    let lookup = names.clone();
    super::workspace_form::open(
        1,
        crate::platform::workspace_form::FormId::AnnotationList,
        "视频标注",
        "选择或新增标注，在工作区内编辑。标注随 MP4 / GIF 一起导出。",
        &[("标注", names[0].clone())],
        &[(0, &choices)],
        &[],
        &["编辑 / 新增", "删除"],
        Box::new(move |action, v| {
            let index = lookup.iter().position(|n| n == &v[0]).ok_or("请选择标注")?;
            if action == 2 {
                if index == 0 {
                    return Err("请选择已有标注".into());
                }
                let mut next = settings.clone();
                next.annotations.remove(index - 1);
                super::preview_window::apply_workspace_media(
                    hwnd,
                    &project,
                    &settings,
                    next.clone(),
                )?;
                settings = next;
                edit_async(hwnd, project.clone(), settings.clone(), duration, source)?;
                return Ok("已删除标注".into());
            }
            annotation_async(
                hwnd,
                project.clone(),
                settings.clone(),
                duration,
                source,
                index,
            )?;
            Ok(String::new())
        }),
    )?;
    Ok(())
}
fn annotation_async(
    hwnd: HWND,
    project: std::path::PathBuf,
    mut settings: WorkbenchSettings,
    duration: TimeTick,
    source: TimeTick,
    mut index: usize,
) -> Result<(), String> {
    if index == 0 && settings.annotations.len() >= 200 {
        return Err("每个工程最多 200 个标注".into());
    }
    let start = source.0.clamp(0, (duration.0 - 1).max(0));
    let mut a = if index == 0 {
        VideoAnnotation {
            id: uuid::Uuid::new_v4().to_string(),
            start: TimeTick(start),
            end: TimeTick((start + 20_000_000).min(duration.0)),
            kind: AnnotationKind::Text,
            anchor: AnnotationAnchor::Canvas,
            bounds: [0.1, 0.1, 0.8, 0.3],
            text: "说明文字".into(),
            color: "#EF4444".into(),
            stroke: 0.004,
            font_size: 0.05,
        }
    } else {
        settings.annotations[index - 1].clone()
    };
    let bounds = a.bounds.map(|n| format!("{:.2}", n * 100.0)).join(", ");
    let fields = [
        ("类型", kind_name(a.kind).into()),
        ("文字内容", a.text.clone()),
        (
            "素材起止秒（逗号）",
            format!("{:.3}, {:.3}", a.start.0 as f64 / 1e7, a.end.0 as f64 / 1e7),
        ),
        ("两点 x,y,x,y（%）", bounds),
        ("颜色 #RRGGBB", a.color.clone()),
        (
            "线宽、字号（高度%）",
            format!("{}, {}", a.stroke * 100.0, a.font_size * 100.0),
        ),
        (
            "位置参照",
            if a.anchor == AnnotationAnchor::Canvas {
                "固定画布"
            } else {
                "跟随素材"
            }
            .into(),
        ),
    ];
    let context = a.id.clone();
    super::workspace_form::open_scoped(
        1,
        crate::platform::workspace_form::FormId::Annotation,
        "编辑视频标注",
        &context,
        "设置位置、内容与显示时间。应用后直接预览；可撤销修改。",
        &fields,
        &[(0, KINDS), (6, &["固定画布", "跟随素材"])],
        &[],
        &["应用标注", "返回列表"],
        Box::new(move |action, v| {
            if action == 2 {
                edit_async(hwnd, project.clone(), settings.clone(), duration, source)?;
                return Ok(String::new());
            }
            a.kind = match v[0].as_str() {
                "箭头" => AnnotationKind::Arrow,
                "矩形" => AnnotationKind::Rectangle,
                "椭圆" => AnnotationKind::Ellipse,
                "荧光" => AnnotationKind::Highlight,
                "遮挡" => AnnotationKind::Cover,
                _ => AnnotationKind::Text,
            };
            a.text.clone_from(&v[1]);
            let times = numbers::<2>(&v[2])?;
            if times[0] < 0.0 || times[1] > duration.0 as f64 / 1e7 || times[1] <= times[0] {
                return Err("素材起止时间超出范围".into());
            }
            a.start = TimeTick((times[0] * 1e7).round() as i64);
            a.end = TimeTick((times[1] * 1e7).round() as i64);
            a.bounds = numbers::<4>(&v[3])?.map(|n| n / 100.0);
            a.color = v[4].trim().into();
            let style = numbers::<2>(&v[5])?;
            a.stroke = style[0] / 100.0;
            a.font_size = style[1] / 100.0;
            a.anchor = if v[6] == "跟随素材" {
                AnnotationAnchor::Source
            } else {
                AnnotationAnchor::Canvas
            };
            if !a.valid() {
                return Err("请检查位置、颜色、线宽（0.05–3%）和字号（0.5–20%）".into());
            }
            let mut next = settings.clone();
            if index == 0 {
                next.annotations.push(a.clone());
            } else {
                next.annotations[index - 1] = a.clone();
            }
            super::preview_window::apply_workspace_media(hwnd, &project, &settings, next.clone())?;
            if index == 0 {
                index = next.annotations.len();
            }
            settings = next;
            Ok("标注已应用 · 可撤销".into())
        }),
    )?;
    Ok(())
}
