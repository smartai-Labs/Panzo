//! Persistent native media inspector; editing, progress and errors stay inside the workbench.
use super::{
    desktop_ui as desk,
    editor_ui::{self, IconKind, TextStyle},
    media_jobs::{self, Event, Job, ResultData},
    workspace_ui as ui,
};
use panzo_core::{
    TimeTick, VideoEdit, WorkbenchSettings,
    media_edit::{self, AudioRole, SubtitleCue, TimeAnchor},
};
use std::{cell::RefCell, path::PathBuf, sync::atomic::Ordering};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::*,
        UI::{Controls::Dialogs::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, PWSTR, w},
};
thread_local! {static PANELS:RefCell<Vec<(String,HWND)>>=const{RefCell::new(Vec::new())};}
struct State {
    video: HWND,
    body: HWND,
    observed_revision: Option<(usize, u64)>,
    root: PathBuf,
    settings: WorkbenchSettings,
    edit: VideoEdit,
    mode: usize,
    selected: Option<String>,
    buttons: Vec<HWND>,
    source: HWND,
    source_label: HWND,
    meter: HWND,
    sections: [HWND; 4],
    role_label: HWND,
    device_refresh: HWND,
    recording_start: Option<TimeTick>,
    live_waveform: media_jobs::LiveWaveform,
    meter_frames: u64,
    meter_peak: f64,
    fields: Vec<(HWND, HWND)>,
    text: HWND,
    apply: HWND,
    status: HWND,
    color: HWND,
    toggles: [HWND; 2],
    role: HWND,
    panel_scroll: std::cell::Cell<i32>,
    job: Option<Job>,
    expected: Option<WorkbenchSettings>,
    devices: Vec<super::audio_capture::AudioDevice>,
    draft: bool,
    loading: bool,
    feedback_bottom: bool,
    fold_list: HWND,
    fold_style: HWND,
    cue_list: HWND,
    show_list: bool,
    show_style: bool,
    fonts: super::font_catalog::FontControls,
    opacity: HWND,
    opacity_label: HWND,
    countdown: Option<super::workbench_overlays::Overlay>,
}
pub(crate) fn open(video: HWND, mode: usize) -> Result<(), String> {
    let (root, settings, edit, _) = super::preview_window::media_snapshot(video)?;
    let key = format!("{}:{mode}", root.to_string_lossy());
    if let Some(h) = PANELS.with(|v| {
        v.borrow()
            .iter()
            .find(|(k, h)| *k == key && unsafe { IsWindow(Some(*h)) }.as_bool())
            .map(|(_, h)| *h)
    }) {
        if let Some(s) = unsafe { state(h) } {
            s.video = video;
            if s.mode != mode && s.job.is_some() {
                return Err("请先结束录音或等待当前任务完成，再切换声音与字幕".into());
            }
            if s.mode != mode {
                s.mode = mode;
                s.feedback_bottom = false;
                s.selected = None;
                s.panel_scroll.set(0);
                s.draft = false;
                refresh(h, s, true);
            }
        }
        super::workspace::set_form(1, h);
        return Ok(());
    }
    ui::register(w!("PanzoMediaPanel"), Some(procedure))?;
    ui::register(w!("PanzoRecordingMeter"), Some(meter_proc))?;
    ui::register(w!("PanzoMediaPanelBody"), Some(body_proc))?;
    let h = ui::panel(video, w!("PanzoMediaPanel"))?;
    desk::install_canvas(h);
    let labels = [
        "导入音频 / SRT",
        "录制旁白 / 添加字幕",
        "停止录音 / AI 自动字幕",
        "分割所选片段",
        "删除所选片段",
        "导出 SRT",
        "服务设置",
        "取消任务",
    ];
    let icons = [
        IconKind::Folder,
        IconKind::Record,
        IconKind::Play,
        IconKind::Split,
        IconKind::Delete,
        IconKind::Export,
        IconKind::Settings,
        IconKind::Close,
    ];
    let mut buttons = Vec::new();
    for (i, t) in labels.iter().enumerate() {
        let b = ui::button(h, t, 30 + i)?;
        desk::style(b, Some(icons[i]), true, false, true);
        buttons.push(b);
    }
    let source_label = panel_label(h, "", 60)?;
    let source = ui::combo(h, &["默认设备"], 0, 61)?;
    let meter = ui::panel(h, w!("PanzoRecordingMeter"))?;
    desk::install_canvas(meter);
    let sections = [
        panel_label(h, "音频文件", 70)?,
        panel_label(h, "录制旁白", 71)?,
        panel_label(h, "当前字幕", 72)?,
        panel_label(h, "字幕来源", 73)?,
    ];
    for heading in sections { desk::heading(heading); }
    let role_label = ui::child(h, w!("STATIC"), "添加为", 74, WINDOW_STYLE(0x0200) /* SS_CENTERIMAGE */)?;
    let device_refresh = ui::button(h, "刷新录音设备", 38)?;
    desk::style(device_refresh, Some(IconKind::Restore), true, false, true);
    let mut fields = Vec::new();
    for i in 0..7 {
        fields.push((
            panel_label(h, "", 100 + i)?,
            if i == 2 && mode == 1 {
                ui::number(
                    h,
                    "",
                    &["8", "10", "12", "14", "16", "18", "20", "22", "24", "28", "32", "36", "48", "64", "72", "96"],
                    120 + i,
                    None,
                )?
            } else {
                ui::edit(h, "", 120 + i, false)?
            },
        ));
    }
    for (i,(_,field)) in fields.iter().enumerate(){if i<6 && !(mode==1 && i==2){desk::decimal_field(*field,0.0,86400.0,0.1);}}
    if mode==2 {
        desk::decimal_field(fields[3].1,0.0,200.0,0.1);
        for i in [4,5] { desk::decimal_field(fields[i].1,0.0,86400.0,0.1); }
    } else if mode==1 {
        for i in [0,1] { desk::decimal_field(fields[i].1,0.0,86400.0,0.001); }
        desk::decimal_field(fields[3].1,0.0,80.0,0.1);
    }
    let color = ui::button(h, "文字颜色", 150)?;
    let toggles = [ui::button(h, "启用", 151)?, ui::button(h, "启用", 152)?];
    let role = ui::combo(h, &["旁白", "音乐"], 0, 153)?;
    let text = ui::edit(h, "", 140, true)?;
    let apply = ui::button(h, "应用修改", 50)?;
    let status = panel_label(h, "", 51)?;
    let body = ui::panel(h, w!("PanzoMediaPanelBody"))?;
    desk::install_canvas(body);
    ui::show(body, true);
    let fold_list = ui::button(h, "全部字幕", 160)?;
    let fold_style = ui::button(h, "全局字幕样式", 161)?;
    desk::style(fold_list, Some(IconKind::ChevronRight), true, false, false);
    desk::style(fold_style, Some(IconKind::ChevronRight), true, false, false);
    let cue_list = ui::child(
        h,
        w!("LISTBOX"),
        "字幕列表",
        162,
        WS_CHILD
            | WS_TABSTOP
            | WS_VSCROLL
            | WINDOW_STYLE(
                (LBS_NOTIFY | LBS_NOINTEGRALHEIGHT | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS) as u32,
            ),
    )?;
    desk::list(cue_list);
    let s = Box::new(State {
        video,
        body,
        observed_revision: super::preview_window::media_revision(video),
        root,
        settings,
        edit,
        mode,
        selected: None,
        buttons,
        source,
        source_label,
        meter,
        sections,
        role_label,
        device_refresh,
        recording_start: None,
        live_waveform: Default::default(),
        countdown: None,
        meter_frames: 0,
        meter_peak: 0.0,
        fields,
        text,
        apply,
        status,
        color,
        toggles,
        role,
        panel_scroll: std::cell::Cell::new(0),
        job: None,
        expected: None,
        devices: Vec::new(),
        draft: false,
        loading: false,
        feedback_bottom: false,
        fold_list,
        fold_style,
        cue_list,
        show_list: false,
        show_style: false,
        fonts: super::font_catalog::FontControls::new(h, 170)?,
        opacity: ui::edit(h, "100", 178, false)?,
        opacity_label: panel_label(h, "不透明度 · %", 179)?,
    });
    unsafe {
        SetWindowLongPtrW(h, GWLP_USERDATA, Box::into_raw(s) as isize);
        SetTimer(Some(h), 1, 120, None);
    }
    PANELS.with(|v| v.borrow_mut().push((key, h)));
    if let Some(s) = unsafe { state(h) } {
        refresh(h, s, true);
    }
    super::workspace::set_form(1, h);
    Ok(())
}
pub(crate) fn page(h: HWND) -> Option<usize> {
    PANELS
        .with(|v| v.borrow().iter().any(|(_, panel)| *panel == h))
        .then(|| unsafe { state(h) }.map(|s| s.mode))
        .flatten()
}

unsafe fn state(h: HWND) -> Option<&'static mut State> {
    unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State).as_mut() }
}
fn info(s: &State, t: &str) {
    if !t.is_empty() && s.job.is_none(){super::workspace::notice(t);ui::text(s.status, "");}
    else {ui::text(s.status,t);}
}
fn snapshot(s: &mut State) -> Result<(), String> {
    let (root, next, edit, _) = super::preview_window::media_snapshot(s.video)?;
    if root != s.root {
        return Err("视频工程已切换，请重新打开声音与字幕面板".into());
    }
    s.settings = next;
    s.edit = edit;
    s.observed_revision = super::preview_window::media_revision(s.video);
    Ok(())
}
fn selected_index(s: &State) -> Option<usize> {
    if s.mode != 1 {
        s.settings
            .audio_clips
            .iter()
            .position(|c| Some(c.id.as_str()) == s.selected.as_deref())
    } else {
        s.settings
            .subtitles
            .iter()
            .position(|c| Some(c.id.as_str()) == s.selected.as_deref())
    }
}
fn fields(s: &mut State) {
    if s.mode == 0 {
        return;
    }
    s.loading = true;
    if s.mode == 2 {
        if let Some(i) = selected_index(s) {
            let c = &s.settings.audio_clips[i];
            let values = [
                secs(c.start),
                secs(c.source_in),
                secs(c.source_out),
                format!("{:.1}", c.gain * 100.0),
                secs(c.fade_in),
                secs(c.fade_out),
            ];
            for (i, ((label, field), value)) in s.fields.iter().zip(values.iter()).enumerate() {
                ui::text(
                    *label,
                    [
                        "位置 · 秒",
                        "素材入点 · 秒",
                        "素材出点 · 秒",
                        "音量 · %",
                        "淡入 · s",
                        "淡出 · s",
                    ][i],
                );
                ui::text(*field, value);
            }
            ui::text(s.sections[0], &c.name);
            unsafe {
                SendMessageW(
                    s.role,
                    CB_SETCURSEL,
                    Some(WPARAM(usize::from(c.role == AudioRole::Music))),
                    None,
                );
            }
        }
        s.loading = false;
        return;
    }
    ui::text(s.sections[2],&format!("当前字幕   {} / {}",selected_index(s).map_or(0,|i|i+1),s.settings.subtitles.len()));
    let pixel_height=subtitle_height(s);
    let st = s.settings.subtitle_style_for(s.selected.as_deref().unwrap_or(""));
    s.fonts.load(&st.font);
    ui::text(
        s.opacity,
        &format!("{:.1}",f64::from(st.opacity)*100.0/255.0),
    );
    desk::decimal_field(s.opacity, 0.0, 100.0, 0.1);
    let mut values = vec![
        String::new(),
        String::new(),
        format!("{:.0}", st.font_size * pixel_height),
        format!("{:.1}", st.bottom * 100.0),
        st.color.clone(),
        if st.background { "开" } else { "关" }.into(),
        if st.enabled { "开" } else { "关" }.into(),
    ];
    let mut content = String::new();
    if let Some(i) = selected_index(s) {
        let c = &s.settings.subtitles[i];
        if let Some((a, b)) = s.settings.cue_ranges(c,s.edit.source_duration_tick).first() {
            values[0] = secs(*a);
            values[1] = secs(*b);
        }
        content = c.text.clone();
    }
    for (i, (label, field)) in s.fields.iter().enumerate() {
        ui::text(
            *label,
            [
                "开始 · s",
                "结束 · s",
                "字号 · px",
                "底部距离 · %",
                "文字颜色",
                "字幕底色",
                "显示字幕",
            ][i],
        );
        ui::text(*field, &values[i]);
    }
    let rgb = u32::from_str_radix(values[4].trim_start_matches('#'), 16).unwrap_or(0xffffff);
    desk::choice(
        s.color,
        desk::ChoiceVisual::Swatch([
            (rgb & 255) as u8,
            ((rgb >> 8) & 255) as u8,
            ((rgb >> 16) & 255) as u8,
            255,
        ]),
    );
    for (i, b) in s.toggles.iter().enumerate() {
        let on = values[5 + i] == "开";
        ui::text(*b, if on { "开启" } else { "关闭" });
        desk::selected(*b, on);
    }
    ui::text(s.text, &content.replace("\r\n", "\n").replace('\n', "\r\n"));
    unsafe {
        SendMessageW(s.cue_list, LB_RESETCONTENT, None, None);
        for cue in &s.settings.subtitles {
            let value = ui::wide(&format!(
                "{}  {}",
                s.settings.cue_ranges(cue,s.edit.source_duration_tick)
                    .first()
                    .map_or(String::new(), |(a, _)| secs(*a)),
                cue.text.replace('\n', " ")
            ));
            SendMessageW(
                s.cue_list,
                LB_ADDSTRING,
                None,
                Some(LPARAM(value.as_ptr() as isize)),
            );
        }
        if let Some(i) = selected_index(s) {
            SendMessageW(s.cue_list, LB_SETCURSEL, Some(WPARAM(i)), None);
        }
    }
    ui::text(
        s.fold_list,
        &format!("全部字幕 · {} 条", s.settings.subtitles.len()),
    );
    s.loading = false;
}
fn refresh(h: HWND, s: &mut State, reset: bool) {
    if let Err(e) = snapshot(s) {
        info(s, &e);
        return;
    }
    if reset {
        let mut device_error = None;
        unsafe {
            SendMessageW(s.source, CB_RESETCONTENT, None, None);
        }
        let choices = if s.mode == 0 {
            match super::audio_capture::devices(true) {
                Ok(devices) => s.devices = devices,
                Err(error) => {
                    s.devices.clear();
                    device_error = Some(format!("无法读取录音设备：{error}"));
                }
            }
            let default = s
                .devices
                .iter()
                .find(|d| d.is_default)
                .map_or("系统默认麦克风".into(), |d| {
                    format!("系统默认 · {}", d.name)
                });
            std::iter::once(if s.devices.is_empty() {
                "未检测到可用麦克风".into()
            } else {
                default
            })
            .chain(s.devices.iter().map(|d| d.name.clone()))
            .collect::<Vec<_>>()
        } else {
            vec![
                "视频原声 / 系统声音".into(),
                "录屏麦克风".into(),
                "后期旁白".into(),
                "当前混音".into(),
            ]
        };
        for v in choices {
            let w = ui::wide(&v);
            unsafe {
                SendMessageW(
                    s.source,
                    CB_ADDSTRING,
                    None,
                    Some(LPARAM(w.as_ptr() as isize)),
                );
            }
        }
        unsafe {
            SendMessageW(s.source, CB_SETCURSEL, Some(WPARAM(0)), None);
        }
        ui::text(
            s.source_label,
            if s.mode == 0 {
                "录音设备"
            } else {
                "AI 识别音轨"
            },
        );
        let names = if s.mode == 0 {
            [
                "导入音频",
                "录制旁白",
                "结束录音",
                "分割音频",
                "删除音频",
                "导出 SRT",
                "插件管理",
                "取消任务",
            ]
        } else {
            [
                "导入 SRT",
                "添加字幕",
                "AI 自动字幕",
                "分割字幕",
                "删除字幕",
                "导出 SRT",
                "AI 模型设置",
                "取消任务",
            ]
        };
        for (i, b) in s.buttons.iter().enumerate() {
            ui::text(*b, names[i]);
        }
        let primary = if s.mode == 1 {
            vec![1, 2]
        } else {
            vec![0, 1, 2]
        };
        for (i, b) in s.buttons.iter().enumerate() {
            let icon = [
                IconKind::Folder,
                if s.mode == 1 {
                    IconKind::Add
                } else {
                    IconKind::Record
                },
                if s.mode == 1 {
                    IconKind::Text
                } else {
                    IconKind::Stop
                },
                IconKind::Split,
                IconKind::Delete,
                IconKind::Export,
                IconKind::Settings,
                IconKind::Close,
            ][i];
            desk::style(
                *b,
                if primary.contains(&i) {
                    None
                } else {
                    Some(icon)
                },
                !primary.contains(&i),
                false,
                !primary.contains(&i),
            );
        }
        if selected_index(s).is_none() {
            s.selected = if s.mode == 1 {
                s.settings.subtitles.first().map(|c| c.id.clone())
            } else {
                None
            };
        }
        s.draft = false;
        fields(s);
        if s.job.is_none() {
            info(
                s,
                if let Some(error) = device_error.as_deref() {
                    error
                } else if s.mode == 0 && s.devices.is_empty() {
                    "未检测到可用麦克风，请连接设备后刷新。"
                } else if s.mode == 2 || super::plugins::enabled(panzo_plugin_api::media::PLUGIN_ID)
                {
                    ""
                } else {
                    "导入音频、录音及 AI 字幕需要安装并启用媒体插件。"
                },
            );
        }
    }
    layout(h, s);
    desk::invalidate(h);
}
fn subtitle_height(s:&State)->f64 {super::preview_window::media_canvas_height(s.video).unwrap_or(1080) as f64}
fn secs(t: TimeTick) -> String {
    format!("{:.3}", t.0 as f64 / 1e7)
}
fn layout(h: HWND, s: &State) {
    let _batch = ui::layout_batch(h);
    let r = ui::rect(h);
    let d = |n| ui::scale(h, n);
    let width = (r.right - d(32)).max(d(160));
    let busy = s.job.is_some();
    let recording = s.job.as_ref().is_some_and(|j| j.stop.is_some());
    let footer = if s.mode == 2 { 0 } else { d(56).min(r.bottom / 2) };
    let body_height = (r.bottom - footer).max(1);
    ui::place(s.body, 0, 0, r.right, body_height);
    let mut content = vec![
        s.status,
        s.source,
        s.source_label,
        s.meter,
        s.role,
        s.role_label,
        s.device_refresh,
        s.color,
        s.text,
        s.toggles[0],
        s.toggles[1],
        s.fold_list,
        s.fold_style,
        s.cue_list,
    ];
    content.extend(s.fonts.handles());
    content.extend([s.opacity, s.opacity_label]);
    content.extend(s.sections);
    content.extend(s.fields.iter().flat_map(|(l, e)| [*l, *e]));
    content.extend(s.buttons.iter().copied());
    let shown = RefCell::new(std::collections::HashSet::<isize>::new());
    let show = |c: HWND| {
        shown.borrow_mut().insert(c.0 as isize);
        ui::show(c, true);
    };
    for c in &content {
        let fixed = *c == s.buttons[7]
            || (s.mode == 1 && *c == s.buttons[5])
            || (s.mode == 0 && [*c == s.buttons[1], *c == s.buttons[2]].contains(&true));
        let parent = if fixed { h } else { s.body };
        if unsafe { GetParent(*c) }.ok() != Some(parent) {
            unsafe {
                let _ = SetParent(*c, Some(parent));
            }
            ui::forget(*c);
        }
    }
    let base = if s.mode == 0 {
        358
    } else if s.mode == 2 {
        244
    } else {
        if selected_index(s).is_some() { 748 } else { 140 }
    };
    let total=d(base+if ui::value(s.status).is_empty(){0}else{44});
    let offset = s.panel_scroll.get().clamp(0, (total - body_height).max(0));
    s.panel_scroll.set(offset);
    desk::scroll_panel(h, total, body_height, offset);
    let place = |c, x, y, w, ht| {
        show(c);
        ui::place(c, x, d(y) - offset, w, d(ht));
    };
    let button = |i: usize, x, y, w| {
        place(s.buttons[i], x, y, w, 30);
        enable(s.buttons[i], !busy);
    };
    for (c, shown) in [
        (s.apply, false),
        (s.status, !ui::value(s.status).is_empty()),
    ] {
        ui::show(c, shown);
    }
    if s.mode == 0 {

        place(s.sections[1], d(16), 16, width, 22);
        place(s.source_label, d(16), 54, width - d(36), 24);
        place(s.device_refresh, r.right - d(44), 52, d(28), 28);
        place(s.source, d(16), 84, width, 28);
        place(s.meter, d(16), 130, width, 112);
        let b = s.buttons[if recording { 2 } else { 1 }];
        show(b);
        ui::place(
            b,
            d(16),
            body_height + d(12),
            width - d(if busy && !recording { 38 } else { 0 }),
            d(30),
        );
        desk::style(b, None, false, true, false);
        enable(b, recording || (!busy && !s.devices.is_empty()));
    } else if s.mode == 2 {
        place(s.sections[0], d(16), 16, width, 26);
        place(s.fields[3].0, d(16), 64, width-d(112), 28);
        place(s.fields[3].1, r.right-d(116), 64, d(100), 28);
        place(s.meter, d(16), 104, width, 28);
        for (i,col) in [(4,0),(5,1)] {
            let w=(width-d(16))/2; let x=d(16)+col*(w+d(16));
            place(s.fields[i].0,x,164,w,22);
            place(s.fields[i].1,x,194,w,28);
        }
    } else {
        place(s.sections[3],d(16),16,width,22);
        button(1,d(16),50,width/2-d(4));
        button(2,d(16)+width/2+d(4),50,width/2-d(4));
        ui::text(s.buttons[1],"添加字幕");
        ui::text(s.buttons[2],"AI 自动字幕");
        desk::style(s.buttons[1],Some(IconKind::Add),false,true,false);
        desk::style(s.buttons[2],None,false,false,false);
        place(s.source,d(16),96,width,28);
        if selected_index(s).is_some() {
            place(s.sections[2],d(16),158,width,22);
            place(s.text,d(16),190,width,88);
            enable(s.text,!busy);
            for i in 0..2 {
                let w=(width-d(16))/2; let x=d(16)+i as i32*(w+d(16));
                place(s.fields[i].0,x,296,w,22);
                place(s.fields[i].1,x,326,w,28);
                enable(s.fields[i].1,!busy);
            }
            for c in s.fonts.handles() { if c!=s.fonts.preview { show(c); } }
            s.fonts.place(s.body,d(16),d(390)-offset,width);
            for i in 2..4 {
                let w=(width-d(16))/2;let x=d(16)+(i-2) as i32*(w+d(16));
                place(s.fields[i].0,x,502,w,22);
                place(s.fields[i].1,x,532,w,28);
            }
            place(s.opacity_label,d(16),584,width-d(112),28);
            place(s.opacity,r.right-d(116),584,d(100),28);
            for i in 4..7 {
                place(s.fields[i].0,d(16),632+40*(i as i32-4),width-d(112),28);
                place(if i==4 {s.color}else{s.toggles[i-5]},r.right-d(116),632+40*(i as i32-4),d(100),28);
            }
        }
    }
    enable(s.source, !busy && (s.mode != 0 || !s.devices.is_empty()));
    enable(s.device_refresh, !busy);
    enable(s.role, !busy);
    if s.mode == 1 && !busy {
        let b = s.buttons[5];
        if unsafe { GetParent(b) }.ok() != Some(h) {
            unsafe {
                let _ = SetParent(b, Some(h));
            }
            ui::forget(b);
        }
        show(b);
        ui::text(b, "导出 SRT");
        desk::style(b, None, true, false, false);
        ui::place(b, d(16), body_height + d(12), width, d(30));
        enable(b, !s.settings.subtitles.is_empty());
    }
    if busy && !recording {
        show(s.buttons[7]);
    }
    enable(s.buttons[7], busy);
    ui::place(
        s.buttons[7],
        r.right - d(44),
        body_height + d(12),
        d(28),
        d(30),
    );
    if !ui::value(s.status).is_empty(){place(s.status,d(16),base+4,width,36);}
    for c in content {
        if !shown.borrow().contains(&(c.0 as isize)) {
            ui::show(c, false);
        }
    }
    if !ui::is_panel_resize() { desk::invalidate(h); }
}
fn commit(s: &mut State, next: WorkbenchSettings) -> Result<(), String> {
    super::preview_window::apply_workspace_media(s.video, &s.root, &s.settings, next)?;
    snapshot(s)?;
    s.draft = false;
    super::preview_window::select_workspace_media(s.video, s.mode == 1, s.selected.as_deref());
    fields(s);
    Ok(())
}
fn apply(s: &mut State) -> Result<(), String> {
    let mut values = s
        .fields
        .iter()
        .map(|(_, e)| ui::value(*e))
        .collect::<Vec<_>>();
    if s.mode == 0 {
        values[6] = if ui::selection(s.role) == 1 {
            "音乐"
        } else {
            "旁白"
        }
        .into();
    }
    let text = ui::value(s.text);
    snapshot(s)?;
    let mut next = s.settings.clone();
    if s.mode == 0 {
        return Ok(());
    } else if s.mode == 2 {
        let i = selected_index(s).ok_or("请先选择音频片段")?;
        let c = &mut next.audio_clips[i];
        c.gain = values[3]
            .trim()
            .parse::<f64>()
            .map_err(|_| "音量须为数字")?
            / 100.0;
        c.fade_in = media_edit::parse_time(&values[4])?;
        c.fade_out = media_edit::parse_time(&values[5])?;
        if !c.valid() {
            return Err("请检查片段时间、音量与淡入淡出范围".into());
        }
    } else {
        if let Some(i) = selected_index(s) {
            let mut cue = next.subtitles[i].clone();
            cue.start = media_edit::parse_time(&values[0])?;
            cue.end = media_edit::parse_time(&values[1])?;
            cue.text = text.replace("\r\n", "\n");
            if !cue.valid() {
                return Err("请检查字幕文字和开始、结束时间".into());
            }
            if cue.anchor==TimeAnchor::Source{
                let old=&next.subtitles[i];let range=s.settings.cue_ranges(old,s.edit.source_duration_tick);
                if range.first().is_some_and(|(a,b)|*a==cue.start&&*b==cue.end){cue.start=old.start;cue.end=old.end;}else{cue.anchor=TimeAnchor::Project;}
            }
            next.subtitles[i]=cue;
        }
        let id=s.selected.clone().ok_or("请先选择字幕")?;
        let fallback=next.subtitle_style.clone();
        let st = next.subtitle_styles.entry(id).or_insert(fallback);
        st.font = s.fonts.read(usize::MAX);
        let opacity=ui::value(s.opacity).parse::<f64>().map_err(|_|"不透明度须为 0–100")?;
        if !(0.0..=100.0).contains(&opacity) { return Err("不透明度须为 0–100".into()); }
        st.opacity = (opacity * 255.0 / 100.0).round() as u8;
        st.font_size = values[2]
            .trim()
            .parse::<f64>()
            .map_err(|_| "字号须为数字")?
            / subtitle_height(s);
        st.bottom = values[3]
            .trim()
            .parse::<f64>()
            .map_err(|_| "底部距离须为数字")?
            / 100.0;
        st.color = values[4].trim().into();
        st.background = on(&values[5])?;
        st.enabled = on(&values[6])?;
        if !st.valid() {
            return Err("字号范围 1–1024 px，底部距离 0–80%，颜色格式 #RRGGBB".into());
        }
    }
    commit(s, next)
}
fn on(s: &str) -> Result<bool, String> {
    match s.trim() {
        "开" => Ok(true),
        "关" => Ok(false),
        _ => Err("请输入开或关".into()),
    }
}
// Update only the edited field; do not reload controls or reset the typing caret.
fn live_subtitle_content(s: &mut State, field: usize) -> Result<(), String> {
    let text = ui::value(s.text).replace("\r\n", "\n");
    let value = if field == 140 { None } else { Some(media_edit::parse_time(&ui::value(s.fields[field-120].1))?) };
    snapshot(s)?;
    let Some(i) = selected_index(s) else { return Ok(()); };
    let mut next = s.settings.clone();
    let cue = &mut next.subtitles[i];
    if field == 140 { cue.text = text; }
    else {
        if cue.anchor == TimeAnchor::Source {
            let Some((a,b)) = s.settings.cue_ranges(cue,s.edit.source_duration_tick).first().copied() else { return Ok(()); };
            cue.start=a; cue.end=b; cue.anchor=TimeAnchor::Project;
        }
        if field == 120 { cue.start=value.unwrap(); } else { cue.end=value.unwrap(); }
    }
    if !cue.valid() || next == s.settings { return Ok(()); }
    super::preview_window::apply_workspace_media(s.video, &s.root, &s.settings, next)?;
    snapshot(s)?;
    s.draft=false;
    desk::invalidate(s.cue_list);
    Ok(())
}
fn live_subtitle_style(s: &mut State) -> Result<(), String> {
    if s.mode != 1 || s.loading {
        return Ok(());
    }
    snapshot(s)?;
    let mut next = s.settings.clone();
    let Some(id)=s.selected.clone().filter(|id| next.subtitles.iter().any(|c| &c.id==id)) else { return Ok(()); };
    let fallback=next.subtitle_style.clone();
    let st = next.subtitle_styles.entry(id.clone()).or_insert(fallback);
    st.font = s.fonts.read(usize::MAX);
    let Ok(size) = ui::value(s.fields[2].1).parse::<f64>() else {
        return Ok(());
    };
    let Ok(opacity) = ui::value(s.opacity).parse::<f64>() else {
        return Ok(());
    };
    if !(0.0..=100.0).contains(&opacity) || !(1.0..=1024.0).contains(&size) {
        return Ok(());
    }
    let Ok(bottom) = ui::value(s.fields[3].1).parse::<f64>() else { return Ok(()); };
    st.bottom = bottom / 100.0;
    st.font_size = size / subtitle_height(s);
    st.opacity = (opacity * 255.0 / 100.0).round() as u8;
    st.color = ui::value(s.fields[4].1);
    st.background = on(&ui::value(s.fields[5].1))?;
    st.enabled = on(&ui::value(s.fields[6].1))?;
    if !st.valid() || st == s.settings.subtitle_style_for(&id) {
        return Ok(());
    }
    super::preview_window::apply_workspace_media(s.video, &s.root, &s.settings, next)?;
    snapshot(s)?;
    let rgb = u32::from_str_radix(s.settings.subtitle_style_for(s.selected.as_deref().unwrap_or("")).color.trim_start_matches('#'), 16)
        .unwrap_or(0xffffff);
    desk::choice(
        s.color,
        desk::ChoiceVisual::Swatch([
            (rgb & 255) as u8,
            ((rgb >> 8) & 255) as u8,
            ((rgb >> 16) & 255) as u8,
            255,
        ]),
    );
    Ok(())
}
fn action(h: HWND, s: &mut State, id: usize) -> Result<(), String> {
    if (170..=174).contains(&id) || matches!(id, 180 | 181) {
        let font = s.fonts.read(id);
        if !font.valid() {
            return Ok(());
        }
        s.loading = true;
        s.fonts.load(&font);
        s.loading = false;
        return live_subtitle_style(s);
    }
    if id == 160 || id == 161 {
        if id == 160 {
            s.show_list = !s.show_list;
        } else {
            s.show_style = !s.show_style;
        }
        layout(h, s);
        return Ok(());
    }
    if s.mode == 1 && matches!(id, 30..=38 | 50) {
        s.feedback_bottom = id == 50;
        if s.feedback_bottom {
            s.panel_scroll.set(
                s.panel_scroll
                    .get()
                    .max(ui::scale(h, 952) - ui::rect(h).bottom),
            );
        }
        layout(h, s);
    }
    if id == 38 && s.job.is_none() {
        let selected = ui::selection(s.source)
            .checked_sub(1)
            .and_then(|i| s.devices.get(i))
            .map(|d| d.id.clone());
        refresh(h, s, true);
        if let Some(id) = selected {
            if let Some(index) = s.devices.iter().position(|d| d.id == id) {
                unsafe {
                    SendMessageW(s.source, CB_SETCURSEL, Some(WPARAM(index + 1)), None);
                }
            } else {
                info(s, "原录音设备已断开，当前使用系统默认麦克风。");
            }
        }
        return Ok(());
    }
    if id == 150 {
        let value = ui::value(s.fields[4].1);
        let rgb = u32::from_str_radix(value.trim_start_matches('#'), 16).unwrap_or(0xffffff);
        super::preview_window::workspace_media_color_group(s.video, true);
        if let Err(e) = desk::open_color(
            h,
            s.color,
            [rgb as u8, (rgb >> 8) as u8, (rgb >> 16) as u8, 255],
            true,
        ) {
            super::preview_window::workspace_media_color_group(s.video, false);
            return Err(e);
        }
        return Ok(());
    }
    if id == 151 || id == 152 {
        let i = id - 151;
        let enabled = ui::value(s.fields[5 + i].1) != "开";
        ui::text(s.fields[5 + i].1, if enabled { "开" } else { "关" });
        ui::text(s.toggles[i], if enabled { "开启" } else { "关闭" });
        desk::selected(s.toggles[i], enabled);
        s.draft = true;
        live_subtitle_style(s)?;
        return Ok(());
    }
    if id == 52 {
        super::workspace::hide_form(h);
        return Ok(());
    }
    if id == 37 {
        if let Some(job) = &s.job {
            job.cancel.store(true, Ordering::Release);
            info(s, "正在取消…");
        }
        return Ok(());
    }
    if id == 32 && s.mode == 0 {
        if let Some(stop) = s.job.as_ref().and_then(|j| j.stop.as_ref()) {
            std::fs::write(stop, b"stop").map_err(|e| e.to_string())?;
            info(s, "正在结束录音…");
        }
        return Ok(());
    }
    if s.job.is_some() {
        return Err("任务运行中，可结束录音或取消任务".into());
    }
    if id == 50 {
        apply(s)?;
        info(s, "修改已应用 · 可撤销");
        return Ok(());
    }
    snapshot(s)?;
    let (_, _, _, tick) = super::preview_window::media_snapshot(s.video)?;
    match id {
        30 => {super::preview_window::import_project_asset(s.video)?;}
        31 if s.mode == 0 => {
            if s.devices.is_empty() {
                return Err("未检测到可用麦克风，请连接设备后刷新".into());
            }
            s.meter_frames = 0;
            s.meter_peak = 0.0;
            let device = recording_device(s)?;
            super::preview_window::media_seek(s.video, tick)?;
            let job = media_jobs::audio(
                s.root.clone(),
                None,
                device,
                tick,
                AudioRole::Narration,
                TimeTick(s.edit.duration().0 - tick.0),
            )?;
            s.expected = Some(s.settings.clone());
            s.job = Some(job);
            s.recording_start = Some(tick);
            s.live_waveform = Default::default();

            info(s, "准备录制旁白…");
        }
        31 => {
            let start=s.settings.available_start("subtitle",tick,TimeTick(20_000_000),s.edit.source_duration_tick);
            let cue = SubtitleCue {
                id: uuid::Uuid::new_v4().to_string(),
                start,
                end: TimeTick((start.0 + 20_000_000).min(panzo_core::media_edit::MAX_MEDIA_TICK)),
                text: "新字幕".into(),
                anchor: TimeAnchor::Project,
            };
            if !cue.valid() {
                return Err("请将播放头移到视频范围内".into());
            }
            s.selected = Some(cue.id.clone());
            let mut next = s.settings.clone();
            next.subtitles.push(cue);
            commit(s, next)?;
            super::preview_window::select_workspace_media(s.video, true, s.selected.as_deref());
            s.panel_scroll.set(0);
            layout(h, s);
            unsafe {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(s.text));
            }
        }
        32 => {
            let config = super::speech_settings::load()?;
            config.validate().map_err(|_| "字幕模型不可用，请在应用设置中配置模型".to_owned())?;
            let job = media_jobs::subtitles(
                s.root.clone(),
                s.settings.clone(),
                s.edit.clone(),
                ui::selection(s.source),
            )?;
            s.expected = Some(s.settings.clone());
            s.job = Some(job);
            info(s, "正在生成 AI 字幕；成功后替换字幕轨，可撤销。");
        }
        35 => {
            if let Some(path) = file_dialog(h, true, true) {
                let cues=s.settings.subtitles.iter().flat_map(|c|s.settings.cue_ranges(c,s.edit.source_duration_tick).into_iter().map(|(a,b)|{let mut cue=c.clone();cue.start=a;cue.end=b;cue.anchor=TimeAnchor::Project;cue})).collect::<Vec<_>>();
                std::fs::write(path, media_edit::to_srt(&cues, &s.edit))
                    .map_err(|e| e.to_string())?;
                info(s, "SRT 已导出");
            }
        }
        36 if s.mode == 1 => super::speech_settings::open()?,
        36 => super::plugin_ui::manager()?,
        _ => {}
    }
    layout(h, s);
    desk::invalidate(h);
    Ok(())
}
fn poll(h: HWND, s: &mut State) {
    let events = s
        .job
        .as_ref()
        .map(|j| j.events.try_iter().collect::<Vec<_>>())
        .unwrap_or_default();
    for event in events {
        match event {
            Event::Countdown(n) => {
                if s.countdown.is_none() {
                    let cancel=s.job.as_ref().unwrap().cancel.clone();
                    s.countdown=super::workbench_overlays::Overlay::countdown(s.video,cancel).ok();
                }
                if let Some(overlay)=&s.countdown {overlay.count(n);}
            }
            Event::Progress(t) => {s.countdown=None;info(s, &t);},
            Event::Done(result) => {
                s.countdown=None;
                let canceled = s
                    .job
                    .as_ref()
                    .is_some_and(|j| j.cancel.load(Ordering::Acquire));
                if s.recording_start.take().is_some() {
                    super::preview_window::clear_recording_preview(s.video, &s.root);
                }
                s.job.take();
                s.meter_peak = 0.0;
                desk::invalidate(s.meter);
                let result = if canceled {
                    Err("任务已取消，工程未修改".into())
                } else {
                    result
                };
                let applied=result.and_then(|output|{
            let expected=s.expected.take().ok_or("任务上下文丢失")?;snapshot(s)?;
            match output{
                ResultData::Audio(clip)=>{let mut next=s.settings.clone();next.audio_assets.push(media_edit::AudioAsset::from_clip(&clip));commit(s,next)?;Ok("录音已加入项目素材，拖入轨道后使用 · 可撤销".into())},
                ResultData::Subtitles(cues,approximate)=>{
                    if s.settings.video!=expected.video||s.settings.subtitles!=expected.subtitles||s.settings.subtitle_styles!=expected.subtitle_styles||s.settings.subtitle_style!=expected.subtitle_style||s.settings.audio_clips!=expected.audio_clips||s.settings.audio!=expected.audio||s.settings.visual_clips!=expected.visual_clips||s.settings.visual_assets!=expected.visual_assets||s.settings.tracks!=expected.tracks||s.settings.clip_tracks!=expected.clip_tracks||s.settings.source_video_enabled!=expected.source_video_enabled{
                        let path=s.root.join("media").join(format!("generated-{}.srt",uuid::Uuid::new_v4()));let video=expected.video_edit(s.edit.source_duration_tick).map_err(|e|e.to_string())?;std::fs::write(&path,media_edit::to_srt(&cues,&video)).map_err(|e|e.to_string())?;return Err(format!("工程已变化，未覆盖修改。字幕已保留：{}",path.display()));
                    }
                    s.selected=cues.first().map(|c|c.id.clone());let mut next=s.settings.clone();next.subtitles=cues;let count=next.subtitles.len();commit(s,next)?;Ok(format!("已生成 {count} 条字幕 · 可校对、撤销{}",if approximate{"。当前模型未返回时间戳，已按短音频段定位，请校对时间。"}else{""}))
                }
            }
        });
                info(s, &applied.unwrap_or_else(|e| e));
                layout(h, s);
            }
        }
    }
    if let (Some(_start), Some(meter)) = (s.recording_start, s.job.as_ref().and_then(|j|j.meter.as_ref())) {
        if let Some(path) = meter.parent().map(|p|p.join("audio.pcm")) {
            if s.live_waveform.read_more(&path).unwrap_or(false) {
                desk::invalidate(s.meter);
            }
        }
    }
    let visible = unsafe { IsWindowVisible(h) }.as_bool();
    if visible
        && let Some(meter) = s.job.as_ref().and_then(|j| j.meter.as_ref())
        && let Ok(v) = std::fs::read_to_string(meter)
    {
        let p = v.split_whitespace().collect::<Vec<_>>();
        if p.len() == 2 {
            let frames = p[0].parse::<u64>().unwrap_or(0);
            s.meter_frames = frames;
            s.meter_peak = p[1].parse::<f64>().unwrap_or(0.0).clamp(0.0, 100.0);
            desk::invalidate(s.meter);
            info(
                s,
                &format!(
                    "正在录制 · {:.1} 秒 · 电平 {}%",
                    frames as f64 / 48000.0,
                    p[1]
                ),
            );
        }
    }
    if visible && s.mode==1 {
        let selected=super::preview_window::selected_subtitle(s.video);
        if selected!=s.selected {s.selected=selected;s.draft=false;fields(s);layout(h,s);}
    }
    let revision = super::preview_window::media_revision(s.video);
    if visible
        && s.job.is_none()
        && revision != s.observed_revision
        && let Ok((root, settings, edit, _)) = super::preview_window::media_snapshot(s.video)
        && root == s.root
        && (!s.draft
            || (s.mode == 1
                && !settings
                    .subtitles
                    .iter()
                    .any(|c| Some(&c.id) == s.selected.as_ref())))
    {
        s.observed_revision = revision;
        s.draft = false;
        s.settings = settings;
        s.edit = edit;
        if selected_index(s).is_none() {
            s.selected = None;
        }
        fields(s);
        layout(h, s);
    }
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Some(s) = (unsafe { state(h) }) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    match m {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &mut ps) };
            let r = ui::rect(h);
            let frame = desk::BufferedFrame::new(dc, r);
            let t = editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
            editor_ui::fill_rect(frame.dc, &r, t.palette.panel);
            let footer=if s.mode==2 {0}else{t.scale(56).min(r.bottom/2)};
            editor_ui::fill_rect(
                frame.dc,
                &desk::rect(t.scale(16), r.bottom - footer, r.right - t.scale(32), 1),
                t.palette.divider,
            );
            drop(frame);
            unsafe {
                let _ = EndPaint(h, &ps);
            }
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_MOUSEWHEEL | WM_VSCROLL => {
            let (total, page, pos) = desk::panel_scroll_info(h);
            let next = if m == WM_MOUSEWHEEL {
                s.panel_scroll.get() - ((w.0 >> 16) as u16 as i16 as i32 / 120) * ui::scale(h, 45)
            } else {
                match w.0 & 65535 {
                    0 => s.panel_scroll.get() - 30,
                    1 => s.panel_scroll.get() + 30,
                    2 => s.panel_scroll.get() - page,
                    3 => s.panel_scroll.get() + page,
                    4 | 5 => pos,
                    6 => 0,
                    7 => total,
                    _ => s.panel_scroll.get(),
                }
            };
            s.panel_scroll.set(next);
            layout(h, s);
        }
        WM_DRAWITEM => {
            let item = unsafe { &*(l.0 as *const windows::Win32::UI::Controls::DRAWITEMSTRUCT) };
            if item.hwndItem == s.cue_list {
                if let Some(cue) = s.settings.subtitles.get(item.itemID as usize) {
                    let t = editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
                    let d = |n| t.scale(n);
                    let r = item.rcItem;
                    let selected =
                        item.itemState.0 & windows::Win32::UI::Controls::ODS_SELECTED.0 != 0;
                    editor_ui::fill_rect(
                        item.hDC,
                        &r,
                        if selected {
                            t.palette.raised
                        } else {
                            t.palette.panel
                        },
                    );
                    let range = s.settings.cue_ranges(cue,s.edit.source_duration_tick).first().map_or(String::new(), |(a, b)| {
                        format!("{} — {}", secs(*a), secs(*b))
                    });
                    desk::text_style(
                        item.hDC,
                        desk::rect(
                            r.left + d(10),
                            r.top + d(4),
                            r.right - r.left - d(20),
                            d(18),
                        ),
                        &range,
                        t.palette.text_muted,
                        t.dpi,
                        false,
                        TextStyle::Micro,
                    );
                    desk::text_style(
                        item.hDC,
                        desk::rect(
                            r.left + d(10),
                            r.top + d(23),
                            r.right - r.left - d(20),
                            d(22),
                        ),
                        &cue.text.replace('\n', " "),
                        t.palette.text,
                        t.dpi,
                        false,
                        TextStyle::Caption,
                    );
                }
                return LRESULT(1);
            }
        }
        WM_SIZE => layout(h, s),
        WM_TIMER => poll(h, s),
        WM_SHOWWINDOW => unsafe {
            SetTimer(
                Some(h),
                1,
                if w.0 != 0 || s.job.is_some() {
                    120
                } else {
                    750
                },
                None,
            );
        },
        WM_CLOSE => super::workspace::hide_form(h),
        desk::COLOR_PREVIEW | desk::COLOR_CLOSED => {
            let c = (w.0 as u32).to_le_bytes();
            s.loading = true;
            ui::text(
                s.fields[4].1,
                &format!("#{:02X}{:02X}{:02X}", c[2], c[1], c[0]),
            );
            s.loading = false;
            desk::choice(s.color, desk::ChoiceVisual::Swatch(c));
            if let Err(e) = live_subtitle_style(s) {
                info(s, &e);
            }
            if m == desk::COLOR_CLOSED {
                super::preview_window::workspace_media_color_group(s.video, false);
            }
        }
        WM_COMMAND => {
            let id = w.0 & 65535;
            let code = w.0 >> 16;
            if ((170..=174).contains(&id) || matches!(id, 180 | 181)) && !s.loading {
                if code == CBN_SELCHANGE as usize
                    || code == CBN_KILLFOCUS as usize
                    || (matches!(id, 171..=174 | 180 | 181) && code == 0)
                {
                    if let Err(e) = action(h, s, id) {
                        info(s, &e);
                    }
                }
            } else if id == 178 && code == EN_CHANGE as usize && !s.loading {
                let _ = live_subtitle_style(s);
            } else if id == 162 && code == LBN_SELCHANGE as usize {
                let index =
                    unsafe { SendMessageW(s.cue_list, LB_GETCURSEL, None, None) }.0 as usize;
                if s.draft {
                    match apply(s) {
                        Ok(()) => s.draft = false,
                        Err(e) => {
                            info(s, &e);
                            if let Some(old) = selected_index(s) {
                                unsafe {
                                    SendMessageW(s.cue_list, LB_SETCURSEL, Some(WPARAM(old)), None);
                                }
                            }
                        }
                    }
                }
                if !s.draft {
                    if let Some(c) = s.settings.subtitles.get(index) {
                        s.selected = Some(c.id.clone());
                        super::preview_window::select_workspace_media(
                            s.video,
                            true,
                            s.selected.as_deref(),
                        );
                        if let Some((a, _)) = s.settings.cue_ranges(c,s.edit.source_duration_tick).first() {
                            let _ = super::preview_window::media_seek(s.video, *a);
                        }
                        fields(s);
                        layout(h, s);
                    }
                }
            } else if code==EN_SETFOCUS as usize && (120..=140).contains(&id) && !s.loading {
                super::preview_window::workspace_media_color_group(s.video,true);
            } else if code==EN_KILLFOCUS as usize && (120..=140).contains(&id) && !s.loading {
                super::preview_window::workspace_media_color_group(s.video,false);
                if s.draft {
                    if let Err(e)=apply(s) {info(s,&e);}
                    s.draft=false;
                }
                if s.mode!=0 { fields(s); }
            } else if (code == EN_CHANGE as usize
                || code == CBN_EDITCHANGE as usize
                || code == CBN_SELCHANGE as usize)
                && ((120..=140).contains(&id) || id == 153)
                && !s.loading
            {
                if s.mode == 1 {
                    if matches!(id, 120 | 121 | 140) { s.draft=live_subtitle_content(s,id).is_err(); }
                    else { let _ = live_subtitle_style(s); }
                } else if s.mode==2 { s.draft=live_audio(s,id).is_err(); }
            } else if code == 0 {
                if let Err(e) = action(h, s, id) {
                    info(s, &e);
                }
            }
        }
        WM_NCDESTROY => {
            if s.recording_start.is_some() { super::preview_window::clear_recording_preview(s.video, &s.root); }
            PANELS.with(|v| v.borrow_mut().retain(|(_, p)| *p != h));
            unsafe {
                SetWindowLongPtrW(h, GWLP_USERDATA, 0);
                drop(Box::from_raw(std::ptr::from_mut(s)));
            }
        }
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}
unsafe extern "system" fn meter_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Some(s) = (unsafe {
        GetParent(h)
            .ok()
            .and_then(|p| GetParent(p).ok())
            .and_then(|p| state(p))
    }) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    if s.mode==2 {
        if m==WM_LBUTTONDOWN || m==WM_MOUSEMOVE && w.0&1!=0 || m==WM_LBUTTONUP {
            let x=(l.0 as i16 as i32).clamp(0,ui::rect(h).right.max(1));
            let value=(f64::from(x)/f64::from(ui::rect(h).right.max(1))*2000.).round()/10.;
            if m==WM_LBUTTONDOWN {super::preview_window::workspace_media_color_group(s.video,true);}
            ui::text(s.fields[3].1,&format!("{value:.1}"));
            if m==WM_LBUTTONDOWN {unsafe {windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(h);}}
            if m==WM_LBUTTONUP {unsafe {let _=windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();} super::preview_window::workspace_media_color_group(s.video,false);}
            desk::invalidate(h);return LRESULT(0);
        }
        if m==WM_PAINT {
            let mut ps=PAINTSTRUCT::default();let dc=unsafe {BeginPaint(h,&mut ps)};let r=ui::rect(h);let t=editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
            editor_ui::fill_rect(dc,&r,t.palette.panel);
            let y=r.bottom/2;let value=ui::value(s.fields[3].1).parse::<f64>().unwrap_or(100.).clamp(0.,200.);let x=(value/200.*f64::from(r.right-t.scale(10))) as i32+t.scale(5);
            editor_ui::rounded_surface(dc,desk::rect(t.scale(5),y-1,r.right-t.scale(10),2),t.palette.border,t.palette.border,1);
            editor_ui::rounded_surface(dc,desk::rect(x-t.scale(4),y-t.scale(4),t.scale(8),t.scale(8)),t.palette.accent,t.palette.accent,t.scale(4));
            unsafe{let _=EndPaint(h,&ps);}return LRESULT(0);
        }
    }
    if m == WM_PAINT {
        let mut ps = PAINTSTRUCT::default();
        let dc = unsafe { BeginPaint(h, &mut ps) };
        let r = ui::rect(h);
        let frame = desk::BufferedFrame::new(dc, r);
        let t = editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
        let _scale = editor_ui::DrawingScale::enter(t.dpi);
        let d = |n| t.scale(n);
        editor_ui::fill_rect(frame.dc, &r, t.palette.panel);
        let active = s.job.as_ref().is_some_and(|j| j.stop.is_some());
        let seconds = s.meter_frames as f64 / 48000.;
        let time = format!(
            "{:02}:{:06.3}",
            (seconds / 60.).floor() as u64,
            seconds % 60.
        );
        desk::text_style(
            frame.dc,
            desk::rect(0, d(4), r.right, d(42)),
            &time,
            t.palette.text,
            t.dpi,
            true,
            TextStyle::Title,
        );
        if !s.live_waveform.peaks.is_empty() {
            let peaks=&s.live_waveform.peaks;let columns=(r.right/d(3).max(1)).max(1)as usize;let middle=d(68);let half=d(20);
            for i in 0..columns {let a=i*peaks.len()/columns;let b=((i+1)*peaks.len()/columns).max(a+1).min(peaks.len());let peak=peaks[a.min(peaks.len()-1)..b].iter().copied().fold(0.0f32,f32::max);let height=(peak*half as f32).round()as i32;editor_ui::fill_rect(frame.dc,&desk::rect(i as i32*d(3),middle-height,d(2),height*2+1),t.palette.accent);}
        } else {
        let count = 20;
        let gap = d(3);
        let width = (r.right - gap * (count - 1)) / count;
        for i in 0..count {
            editor_ui::rounded_surface(
                frame.dc,
                desk::rect(i * (width + gap), d(54), width, d(20)),
                if active && f64::from(i) < s.meter_peak / 5. {
                    t.palette.accent
                } else {
                    t.palette.raised
                },
                t.palette.panel,
                d(1),
            );
        }
        }
        desk::text_style(
            frame.dc,
            desk::rect(0, d(94), r.right / 2, d(20)),
            if active {"录音波形"} else {"输入电平"},
            t.palette.text_muted,
            t.dpi,
            false,
            TextStyle::Caption,
        );
        let db = if s.meter_peak > 0. {
            format!("{:.0} dB", 20. * (s.meter_peak / 100.).log10())
        } else {
            "−∞ dB".into()
        };
        desk::text_style(
            frame.dc,
            desk::rect(r.right - d(70), d(94), d(70), d(20)),
            &db,
            t.palette.text_muted,
            t.dpi,
            true,
            TextStyle::Caption,
        );
        drop(frame);
        unsafe {
            let _ = EndPaint(h, &ps);
        }
        return LRESULT(0);
    }
    if m == WM_ERASEBKGND {
        return LRESULT(1);
    }
    unsafe { DefWindowProcW(h, m, w, l) }
}
fn file_dialog(owner: HWND, save: bool, srt: bool) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let filter = ui::wide(if srt {
        "SRT 字幕\0*.srt\0\0"
    } else {
        "音频文件\0*.wav;*.mp3;*.m4a;*.aac;*.wma;*.mp4\0所有文件\0*.*\0\0"
    });
    let mut file = vec![0u16; 32768];
    let title = ui::wide(if save {
        "导出字幕"
    } else if srt {
        "导入字幕"
    } else {
        "导入音频"
    });
    let extension = ui::wide("srt");
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        lpstrDefExt: if srt {
            PCWSTR(extension.as_ptr())
        } else {
            PCWSTR::null()
        },
        Flags: OFN_EXPLORER
            | OFN_PATHMUSTEXIST
            | OFN_NOCHANGEDIR
            | if save {
                OFN_OVERWRITEPROMPT
            } else {
                OFN_FILEMUSTEXIST
            },
        ..Default::default()
    };
    if !(unsafe {
        if save {
            GetSaveFileNameW(&raw mut dialog)
        } else {
            GetOpenFileNameW(&raw mut dialog)
        }
    })
    .as_bool()
    {
        return None;
    }
    let n = file.iter().position(|c| *c == 0)?;
    Some(std::ffi::OsString::from_wide(&file[..n]).into())
}
pub(crate) fn select_item(video: HWND, subtitle: bool, id: &str) {
    if open(video, if subtitle { 1 } else { 2 }).is_err() {
        return;
    }
    PANELS.with(|v| {
        for (_, h) in v.borrow().iter() {
            if let Some(s) = unsafe { state(*h) }
                && s.video == video
                && s.mode == if subtitle { 1 } else { 2 }
            {
                if s.draft && s.selected.as_deref() != Some(id) {
                    if let Err(e) = apply(s) {
                        info(s, &e);
                        super::preview_window::select_workspace_media(
                            video,
                            subtitle,
                            s.selected.as_deref(),
                        );
                        continue;
                    }
                }
                s.selected = Some(id.into());
                s.draft = false;
                super::preview_window::select_workspace_media(video,subtitle,Some(id));
                fields(s);
                layout(*h, s);
            }
        }
    });
}

fn enable(h: HWND, on: bool) {
    if unsafe { windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(h) }.as_bool() == on { return; }
    unsafe {
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(h, on);
    }
}

fn recording_device(s: &State) -> Result<Option<String>, String> {
    if s.devices.is_empty() {
        return Err("未检测到可用麦克风，请连接设备后刷新".into());
    }
    let index = ui::selection(s.source);
    if index == 0 {
        Ok(None)
    } else {
        s.devices
            .get(index - 1)
            .map(|d| Some(d.id.clone()))
            .ok_or("所选录音设备不可用，请刷新设备列表".into())
    }
}
#[cfg(test)]
pub(crate) fn review_device(h: HWND) -> Result<Option<String>, String> {
    recording_device(unsafe { state(h) }.ok_or("panel missing")?)
}

#[cfg(test)]
pub(crate) fn review_pending_job(
    h: HWND,
) -> (
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::sync::mpsc::Sender<Event>,
) {
    let s = unsafe { state(h) }.unwrap();
    assert!(s.job.is_none());
    let (tx, rx) = std::sync::mpsc::channel();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    s.job = Some(Job {
        events: rx,
        cancel: cancel.clone(),
        stop: None,
        meter: None,
    });
    s.expected = Some(s.settings.clone());
    layout(h, s);
    (cancel, tx)
}

// A clipped content viewport keeps scrolling fields away from persistent actions.
unsafe extern "system" fn body_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match m {
        WM_COMMAND | WM_DRAWITEM | WM_MOUSEWHEEL | WM_VSCROLL => {
            if let Ok(parent) = unsafe { GetParent(h) } {
                return unsafe { SendMessageW(parent, m, Some(w), Some(l)) };
            }
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &mut ps) };
            editor_ui::fill_rect(
                dc,
                &ui::rect(h),
                editor_ui::EditorTheme::current(editor_ui::window_dpi(h))
                    .palette
                    .panel,
            );
            if let Some(s) = unsafe { GetParent(h).ok().and_then(|p| state(p)) } {
                let t = editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
                let r = ui::rect(h);
                let ys: &[i32] = if s.mode == 0 {
                    &[]
                } else if s.mode == 1 {
                    if selected_index(s).is_some() { &[140,374] } else { &[140] }
                } else {
                    &[48,148]
                };
                for y in ys {
                    desk::line(
                        dc,
                        desk::rect(
                            t.scale(16),
                            t.scale(*y) - s.panel_scroll.get(),
                            r.right - t.scale(32),
                            1,
                        ),
                        t,
                    );
                }
            }
            unsafe {
                let _ = EndPaint(h, &ps);
            }
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(h, m, w, l) }
}

pub(crate) fn active_task() -> Option<(HWND, bool, String)> {
    let handles = PANELS.with(|v| v.borrow().iter().map(|(_, h)| *h).collect::<Vec<_>>());
    handles.into_iter().find_map(|h| {
        if !unsafe { IsWindow(Some(h)) }.as_bool() {
            return None;
        }
        let s = unsafe { state(h) }?;
        let job = s.job.as_ref()?;
        let recording = job.stop.is_some();
        Some((
            h,
            recording,
            if recording {
                "正在录制旁白"
            } else if s.mode == 1 {
                "正在生成字幕"
            } else {
                "正在导入音频"
            }
            .into(),
        ))
    })
}

#[cfg(test)]
pub(crate) fn review_body(h: HWND) -> HWND {
    unsafe { state(h) }.unwrap().body
}

#[cfg(test)]
pub(crate) fn review_recording(h: HWND, stop: PathBuf) -> std::sync::mpsc::Sender<Event> {
    let (_, tx) = review_pending_job(h);
    let s = unsafe { state(h) }.unwrap();
    s.job.as_mut().unwrap().stop = Some(stop);
    layout(h, s);
    tx
}

pub(crate) fn import_audio_path(video:HWND,path:PathBuf)->Result<(),String>{super::preview_window::import_project_asset_path(video,path)}

#[cfg(test)]
pub(crate) fn review_font_handles(h: HWND) -> Vec<HWND> {
    let s = unsafe { state(h) }.unwrap();
    let mut result = s.fonts.handles()[..7].to_vec();
    result.extend([s.opacity, s.fields[2].1, s.fields[4].1]);
    result
}

#[cfg(test)]
pub(crate) fn review_live_recording(h:HWND, folder:&std::path::Path, start:TimeTick)->std::sync::mpsc::Sender<Event> {
    let s=unsafe{state(h)}.unwrap();snapshot(s).unwrap();
    let tx=review_recording(h,folder.join("stop"));
    let s=unsafe{state(h)}.unwrap();s.job.as_mut().unwrap().meter=Some(folder.join("meter"));
    s.recording_start=Some(start);s.live_waveform=Default::default();
    super::preview_window::update_recording_preview(s.video,&s.root,start,0,&[]);
    tx
}

pub(crate) fn task_for(video: HWND) -> Option<bool> {
    let handles = PANELS.with(|v| v.borrow().iter().map(|(_, h)| *h).collect::<Vec<_>>());
    handles.into_iter().find_map(|h| unsafe { state(h) }.filter(|s| s.video == video).and_then(|s| s.job.as_ref().map(|j| j.stop.is_some())))
}

// These labels paint an opaque, vertically centred row. Native multiline STATIC
// controls could wrap long refreshed labels into adjacent input rows.
fn panel_label(parent: HWND, text: &str, id: usize) -> Result<HWND,String> {
    ui::register(w!("PanzoInspectorLabel"),Some(label_proc))?;
    ui::child(parent,w!("PanzoInspectorLabel"),text,id,WS_CLIPSIBLINGS)
}
unsafe extern "system" fn label_proc(h:HWND,m:u32,w:WPARAM,l:LPARAM)->LRESULT {
    if m==WM_ERASEBKGND {return LRESULT(1);}
    if m==WM_SETTEXT {
        let result=unsafe{DefWindowProcW(h,m,w,l)};desk::invalidate(h);return result;
    }
    if m==WM_PAINT {
        let mut ps=PAINTSTRUCT::default();let dc=unsafe{BeginPaint(h,&mut ps)};
        let r=ui::rect(h);let t=editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
        let frame=desk::BufferedFrame::new(dc,r);
        editor_ui::fill_rect(frame.dc,&r,t.palette.panel);
        let heading=(70..=73).contains(&unsafe{GetDlgCtrlID(h)});
        desk::text_style(frame.dc,r,&ui::value(h),if heading {t.palette.text}else{t.palette.text_secondary},t.dpi,false,if heading {TextStyle::Heading}else{TextStyle::Body});
        drop(frame);unsafe{let _=EndPaint(h,&ps);}return LRESULT(0);
    }
    unsafe{DefWindowProcW(h,m,w,l)}
}
fn live_audio(s:&mut State,id:usize)->Result<(),String> {
    if s.loading || s.mode!=2 || !matches!(id,123..=125) { return Ok(()); }
    let value=ui::value(s.fields[id-120].1);
    snapshot(s)?;
    let Some(i)=selected_index(s) else{return Ok(());};
    let mut next=s.settings.clone();let c=&mut next.audio_clips[i];
    match id {
        123=>c.gain=value.parse::<f64>().map_err(|_|"音量须为数字")?/100.0,
        124=>c.fade_in=media_edit::parse_time(&value)?,
        _=>c.fade_out=media_edit::parse_time(&value)?,
    }
    if !c.valid() || next==s.settings {return Ok(());}
    super::preview_window::apply_workspace_media(s.video,&s.root,&s.settings,next)?;
    snapshot(s)?;s.draft=false;desk::invalidate(s.meter);Ok(())
}
