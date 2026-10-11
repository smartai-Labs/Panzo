// Sample positions and validated form values are clamped to nonnegative ranges before conversion.
#![allow(clippy::cast_sign_loss)]
//! Shared native forms for recording and post-production settings.
use super::{editor_dialog, recorder::RecordingOptions};
use panzo_core::{
    TimeTick, WorkbenchSettings,
    camera_policy::{AutoCameraConfig, AutoCameraSettings, FollowMode},
};
use windows::Win32::Foundation::HWND;
const SWITCH: &[&str] = &["关闭", "开启"];
fn on(value: bool) -> String {
    if value { "开启" } else { "关闭" }.into()
}
fn number(s: &str, min: f64, max: f64) -> Result<f64, String> {
    let n = s.trim().parse::<f64>().map_err(|_| "请输入有效数字")?;
    if !n.is_finite() || n < min || n > max {
        return Err(format!("请输入 {min} 至 {max} 之间的数字"));
    }
    Ok(n)
}
fn camera_fields(camera: AutoCameraConfig) -> Vec<(&'static str, String)> {
    vec![
        ("自动跟随缩放", on(camera.enabled)),
        (
            "跟随方式",
            if camera.mode == FollowMode::Smooth {
                "连续跟随"
            } else {
                "点击聚焦"
            }
            .into(),
        ),
        ("缩放倍率（1–3）", format!("{}", camera.zoom)),
        ("响应时间（毫秒）", camera.response_ms.to_string()),
    ]
}
fn camera_from(values: &[String]) -> Result<AutoCameraConfig, String> {
    Ok(AutoCameraConfig {
        enabled: values[0] == "开启",
        mode: if values[1] == "连续跟随" {
            FollowMode::Smooth
        } else {
            FollowMode::Click
        },
        zoom: number(&values[2], 1.0, 3.0)?,
        response_ms: number(&values[3], 80.0, 1000.0)?.round() as u32,
    })
}
pub fn record_options(hwnd: HWND, options: &mut RecordingOptions) -> Result<bool, String> {
    let mut fields = camera_fields(options.camera);
    fields.extend([
        ("录制帧率", options.fps.to_string()),
        ("倒计时（秒）", options.countdown_seconds.to_string()),
        ("显示鼠标", on(options.cursor_visible)),
    ]);
    let Some(values) = editor_dialog::form_with_description(
        hwnd,
        "录制设置",
        "普通录屏：关闭跟随；演示录屏：开启连续跟随。录制中仍可切换，录后可修改整段或指定区间。",
        &fields,
        &[
            (0, SWITCH),
            (1, &["连续跟随", "点击聚焦"]),
            (4, &["30", "60"]),
            (5, &["0", "3", "5", "10"]),
            (6, SWITCH),
        ],
        "保存设置",
    )
    .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    options.camera = camera_from(&values)?;
    options.fps = number(&values[4], 30.0, 60.0)? as u32;
    options.countdown_seconds = number(&values[5], 0.0, 10.0)? as u32;
    options.cursor_visible = values[6] == "开启";
    Ok(true)
}
pub fn record_audio(hwnd: HWND, options: &mut RecordingOptions) -> Result<bool, String> {
    let systems = super::audio_capture::devices(false)?;
    let microphones = super::audio_capture::devices(true)?;
    let names = |devices: &[super::audio_capture::AudioDevice]| {
        std::iter::once("默认设备".to_owned())
            .chain(
                devices
                    .iter()
                    .enumerate()
                    .map(|(i, device)| format!("{} · {}", i + 1, device.name)),
            )
            .collect::<Vec<_>>()
    };
    let sn = names(&systems);
    let mn = names(&microphones);
    let sr = sn.iter().map(String::as_str).collect::<Vec<_>>();
    let mr = mn.iter().map(String::as_str).collect::<Vec<_>>();
    let selected = |devices: &[super::audio_capture::AudioDevice], id: &Option<String>| {
        devices
            .iter()
            .enumerate()
            .find(|(_, device)| Some(&device.id) == id.as_ref())
            .map_or("默认设备".into(), |(i, device)| {
                format!("{} · {}", i + 1, device.name)
            })
    };
    let Some(v)=editor_dialog::form_with_description(hwnd,"录音设备与电平","选择要录制的声音。开启电平检测后将采样 3 秒，只显示峰值，不保存声音。录制中悬浮条会显示实时电平。",&[("系统声音",on(options.audio.system)),("播放设备",selected(&systems,&options.audio.system_device)),("麦克风",on(options.audio.microphone)),("输入设备",selected(&microphones,&options.audio.microphone_device)),("检测 3 秒电平",on(false))],&[(0,SWITCH),(1,&sr),(2,SWITCH),(3,&mr),(4,SWITCH)],"应用").map_err(|e|e.to_string())?else{return Ok(false);};
    let mut audio = options.audio.clone();
    audio.system = v[0] == "开启";
    audio.microphone = v[2] == "开启";
    audio.system_device = systems
        .iter()
        .enumerate()
        .find(|(i, device)| format!("{} · {}", i + 1, device.name) == v[1])
        .map(|(_, device)| device.id.clone());
    audio.microphone_device = microphones
        .iter()
        .enumerate()
        .find(|(i, device)| format!("{} · {}", i + 1, device.name) == v[3])
        .map(|(_, device)| device.id.clone());
    let result = if v[4] == "开启" {
        super::audio_capture::measure(&audio)?
    } else {
        super::audio_capture::probe(&audio)?
    };
    if v[4] == "开启" {
        let text = result.encode_utf16().chain([0]).collect::<Vec<_>>();
        unsafe {
            crate::platform::app_dialog::message(
                Some(hwnd),
                windows::core::PCWSTR(text.as_ptr()),
                windows::core::w!("录音电平"),
                windows::Win32::UI::WindowsAndMessaging::MB_OK,
            );
        }
    }
    options.audio = audio;
    Ok(true)
}
pub fn edit_camera(
    hwnd: HWND,
    settings: &mut WorkbenchSettings,
    duration: TimeTick,
    range: bool,
) -> Result<bool, String> {
    let mut policy = settings
        .automatic_camera
        .clone()
        .unwrap_or(AutoCameraSettings {
            default: AutoCameraConfig {
                enabled: settings.camera_enabled,
                mode: FollowMode::Click,
                ..Default::default()
            },
            ranges: Vec::new(),
        });
    let mut fields = camera_fields(policy.default);
    if range {
        fields.extend([
            ("素材起点（秒）", "0".into()),
            (
                "素材终点（秒）",
                format!("{:.3}", duration.0 as f64 / 10_000_000.0),
            ),
        ]);
    } else {
        fields.push(("手工镜头", on(settings.manual_camera_enabled)));
    }
    let Some(v)=editor_dialog::form_with_description(hwnd,if range{"设置跟随区间"}else{"整段鼠标跟随"},if range{"区间使用原始素材时间；剪切、删除和变速后仍跟随对应素材。边界平滑切换。"}else{"整段设置会清除已有开关区间。手工镜头开启时优先于自动跟随。仅 Panzo 录屏具有可编辑的鼠标轨迹。"},&fields,&[(0,SWITCH),(1,&["连续跟随","点击聚焦"]),(if range{99}else{4},SWITCH)],"应用").map_err(|e|e.to_string())?else{return Ok(false);};
    let config = camera_from(&v)?;
    if range {
        let a = number(&v[4], 0.0, duration.0 as f64 / 10_000_000.0)?;
        let b = number(&v[5], 0.0, duration.0 as f64 / 10_000_000.0)?;
        if a >= b {
            return Err("终点必须晚于起点".into());
        }
        policy.set_range(
            TimeTick((a * 10_000_000.0).round() as i64),
            TimeTick((b * 10_000_000.0).round() as i64),
            config,
        );
    } else {
        policy.default = config;
        policy.ranges.clear();
        settings.manual_camera_enabled = v[4] == "开启";
    }
    settings.camera_enabled = true;
    settings.automatic_camera = Some(policy);
    Ok(true)
}
pub fn edit_audio(hwnd: HWND, settings: &mut WorkbenchSettings) -> Result<bool, String> {
    let a = settings.audio;
    let Some(v)=editor_dialog::form_with_description(hwnd,"声音与音量","只有工程内已有的音轨会输出。0% 静音，100% 原音量，最高 200%。变速会同时改变音高；GIF 始终无声音。",&[("输出声音",on(a.enabled)),("系统声音（%）",format!("{}",a.system_gain*100.0)),("麦克风（%）",format!("{}",a.microphone_gain*100.0)),("导入视频声音（%）",format!("{}",a.imported_gain*100.0))],&[(0,SWITCH)],"应用").map_err(|e|e.to_string())?else{return Ok(false);};
    settings.audio = panzo_core::camera_policy::AudioSettings {
        enabled: v[0] == "开启",
        source_muted: settings.audio.source_muted,
        system_gain: number(&v[1], 0.0, 200.0)? / 100.0,
        microphone_gain: number(&v[2], 0.0, 200.0)? / 100.0,
        imported_gain: number(&v[3], 0.0, 200.0)? / 100.0,
    };
    Ok(true)
}

pub(crate) fn record_async(
    hwnd: HWND,
    mut options: RecordingOptions,
    audio: bool,
) -> Result<(), String> {
    if audio {
        return audio_async(hwnd, options);
    }
    let mut fields = camera_fields(options.camera);
    fields.extend([
        ("录制帧率", options.fps.to_string()),
        ("倒计时（秒）", options.countdown_seconds.to_string()),
        ("显示鼠标", on(options.cursor_visible)),
    ]);
    let form = super::workspace_form::open(
        1,
        crate::platform::workspace_form::FormId::Recording,
        "录制设置",
        "在这里调整录制帧率、鼠标和跟踪缩放；开始录制前应用。",
        &fields,
        &[
            (0, SWITCH),
            (1, &["连续跟随", "点击聚焦"]),
            (4, &["30", "60"]),
            (5, &["0", "3", "5", "10"]),
            (6, SWITCH),
        ],
        &[],
        &["应用设置"],
        Box::new(move |_, v| {
            options.camera = camera_from(&v)?;
            options.fps = number(&v[4], 30.0, 60.0)? as u32;
            options.countdown_seconds = number(&v[5], 0.0, 10.0)? as u32;
            options.cursor_visible = v[6] == "开启";
            super::recorder_window::apply_workspace_recording(hwnd, options.clone())?;
            Ok("录制设置已应用".into())
        }),
    )?;
    super::recording_setup::advanced_opened(form);
    Ok(())
}
fn audio_async(hwnd: HWND, mut options: RecordingOptions) -> Result<(), String> {
    let systems = super::audio_capture::devices(false)?;
    let microphones = super::audio_capture::devices(true)?;
    let names = |devices: &[super::audio_capture::AudioDevice]| {
        std::iter::once("默认设备".to_owned())
            .chain(
                devices
                    .iter()
                    .enumerate()
                    .map(|(i, device)| format!("{} · {}", i + 1, device.name)),
            )
            .collect::<Vec<_>>()
    };
    let sn = names(&systems);
    let mn = names(&microphones);
    let selected = |devices: &[super::audio_capture::AudioDevice], id: &Option<String>| {
        devices
            .iter()
            .enumerate()
            .find(|(_, device)| Some(&device.id) == id.as_ref())
            .map_or("默认设备".into(), |(i, device)| {
                format!("{} · {}", i + 1, device.name)
            })
    };
    let fields = [
        ("系统声音", on(options.audio.system)),
        ("播放设备", selected(&systems, &options.audio.system_device)),
        ("麦克风", on(options.audio.microphone)),
        (
            "输入设备",
            selected(&microphones, &options.audio.microphone_device),
        ),
    ];
    let holder = std::rc::Rc::new(std::cell::Cell::new(HWND::default()));
    let target = holder.clone();
    let h = super::workspace_form::open(
        1,
        crate::platform::workspace_form::FormId::Devices,
        "录音设备与电平",
        "检测会采样 3 秒并显示电平，不保存声音；检测时可继续使用其他功能。",
        &fields,
        &[
            (0, SWITCH),
            (1, &sn.iter().map(String::as_str).collect::<Vec<_>>()),
            (2, SWITCH),
            (3, &mn.iter().map(String::as_str).collect::<Vec<_>>()),
        ],
        &[],
        &["应用设备", "检测 3 秒电平"],
        Box::new(move |action, v| {
            let mut a = options.audio.clone();
            a.system = v[0] == "开启";
            a.microphone = v[2] == "开启";
            a.system_device = systems
                .iter()
                .enumerate()
                .find(|(i, device)| format!("{} · {}", i + 1, device.name) == v[1])
                .map(|(_, device)| device.id.clone());
            a.microphone_device = microphones
                .iter()
                .enumerate()
                .find(|(i, device)| format!("{} · {}", i + 1, device.name) == v[3])
                .map(|(_, device)| device.id.clone());
            if action == 2 {
                super::workspace::start_job(
                    super::workspace::Target::Form(target.get()),
                    "电平检测",
                    move |_, _| {
                        super::audio_capture::measure(&a).map(super::image_tasks::Output::Text)
                    },
                );
                return Ok("正在检测所选设备…".into());
            }
            options.audio = a;
            super::recorder_window::apply_workspace_recording(hwnd, options.clone())?;
            Ok("录音设备已应用".into())
        }),
    )?;
    holder.set(h);
    Ok(())
}
#[allow(clippy::too_many_lines)] // Keep inspector creation and its validated callback together.
pub(crate) fn effects_async(
    hwnd: HWND,
    project: std::path::PathBuf,
    mut settings: WorkbenchSettings,
    duration: TimeTick,
    source: TimeTick,
    action: i32,
) -> Result<(), String> {
    if action == 5 {
        return super::video_annotations::edit_async(hwnd, project, settings, duration, source);
    }
    if action == 3 {
        let a = settings.audio;
        super::workspace_form::open(
            1,
            crate::platform::workspace_form::FormId::AudioMix,
            "声音与音量",
            "调整工程已有音轨，应用后可预览并撤销。GIF 无声音。",
            &[
                ("输出声音", on(a.enabled)),
                ("系统声音（%）", format!("{}", a.system_gain * 100.0)),
                ("麦克风（%）", format!("{}", a.microphone_gain * 100.0)),
                ("导入视频声音（%）", format!("{}", a.imported_gain * 100.0)),
            ],
            &[(0, SWITCH)],
            &[],
            &["应用"],
            Box::new(move |_, v| {
                let mut next = settings.clone();
                next.audio = panzo_core::camera_policy::AudioSettings {
                    enabled: v[0] == "开启",
                    source_muted: settings.audio.source_muted,
                    system_gain: number(&v[1], 0.0, 200.0)? / 100.0,
                    microphone_gain: number(&v[2], 0.0, 200.0)? / 100.0,
                    imported_gain: number(&v[3], 0.0, 200.0)? / 100.0,
                };
                super::preview_window::apply_workspace_media(
                    hwnd,
                    &project,
                    &settings,
                    next.clone(),
                )?;
                settings = next;
                Ok("声音已应用 · 可撤销".into())
            }),
        )?;
        return Ok(());
    }
    let range = action == 2;
    let policy = settings
        .automatic_camera
        .clone()
        .unwrap_or(AutoCameraSettings {
            default: AutoCameraConfig {
                enabled: settings.camera_enabled,
                mode: FollowMode::Click,
                ..Default::default()
            },
            ranges: Vec::new(),
        });
    let mut fields = camera_fields(policy.default);
    if range {
        fields.extend([
            ("素材起点（秒）", "0".into()),
            ("素材终点（秒）", format!("{:.3}", duration.0 as f64 / 1e7)),
        ]);
    } else {
        fields.push(("手工镜头", on(settings.manual_camera_enabled)));
    }
    super::workspace_form::open(
        1,
        if range {
            crate::platform::workspace_form::FormId::CameraRange
        } else {
            crate::platform::workspace_form::FormId::Camera
        },
        if range {
            "鼠标跟随区间"
        } else {
            "整段鼠标跟随"
        },
        "调整开关、跟随方式和缩放倍率。整段应用会清除原有跟随区间；区间采用原始素材时间。",
        &fields,
        &[
            (0, SWITCH),
            (1, &["连续跟随", "点击聚焦"]),
            (if range { 99 } else { 4 }, SWITCH),
        ],
        &[],
        &["应用"],
        Box::new(move |_, v| {
            let mut next = settings.clone();
            let mut policy = next
                .automatic_camera
                .clone()
                .unwrap_or_else(|| policy.clone());
            let config = camera_from(&v)?;
            if range {
                let a = number(&v[4], 0.0, duration.0 as f64 / 1e7)?;
                let b = number(&v[5], 0.0, duration.0 as f64 / 1e7)?;
                if a >= b {
                    return Err("终点必须晚于起点".into());
                }
                policy.set_range(
                    TimeTick((a * 1e7).round() as i64),
                    TimeTick((b * 1e7).round() as i64),
                    config,
                );
            } else {
                policy.default = config;
                policy.ranges.clear();
                next.manual_camera_enabled = v[4] == "开启";
            }
            next.camera_enabled = true;
            next.automatic_camera = Some(policy);
            super::preview_window::apply_workspace_media(hwnd, &project, &settings, next.clone())?;
            settings = next;
            Ok("跟随设置已应用 · 可撤销".into())
        }),
    )?;
    Ok(())
}
