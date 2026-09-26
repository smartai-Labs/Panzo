use super::{
    CameraSegment, ControlId, DT_LEFT, DT_SINGLELINE, DT_VCENTER, HWND, HitRect, Instant,
    NormalizedPoint, PCWSTR, RECT, TICKS_PER_SECOND, TextStyle, TimeTick, WindowState, border,
    draw_text_styled, fill, mem, rounded_surface, timeline_x, w, wide,
};
use crate::platform::{
    editor_dialog,
    editor_tasks::ExportTask,
    export::{ExportRequest, ExportSnapshot},
};
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONWARNING, MB_OK, MessageBoxW};

impl WindowState {
    pub(super) fn application_menu(&mut self, hwnd: HWND) {
        use crate::platform::app_menu::{self, Action, Availability};
        let recent = crate::platform::recorder_window::recent_project(hwnd)
            .filter(|path| path != self.editor.project_root());
        let available = Availability {
            new_recording: self.control_enabled(ControlId::NewRecording),
            open: self.control_enabled(ControlId::OpenProject),
            recent: recent.is_some() && self.control_enabled(ControlId::OpenProject),
            save: self.control_enabled(ControlId::Save),
            export: self.control_enabled(ControlId::Export),
        };
        let anchor = self
            .control_layout(super::client_rect(hwnd))
            .control_rect(ControlId::Menu)
            .0;
        match app_menu::show(hwnd, anchor, self.theme, available) {
            Ok(Some(Action::ContinueRecent)) => {
                if let Some(path) = recent {
                    self.request_navigation(hwnd, super::NavigationIntent::OpenProject(path));
                }
            }
            Ok(Some(action)) => {
                let control = match action {
                    Action::NewRecording => ControlId::NewRecording,
                    Action::Open => ControlId::OpenProject,
                    Action::Save => ControlId::Save,
                    Action::Export => ControlId::Export,
                    Action::OpenLocation => ControlId::Location,
                    Action::ContinueRecent => unreachable!(),
                };
                if self.control_enabled(control) {
                    self.activate_control(hwnd, control);
                }
            }
            Ok(None) => (),
            Err(error) => self.note_problem(
                "菜单未打开，请再次点击左上角菜单",
                format!("无法打开主菜单：{error}"),
            ),
        }
        self.hot_control = None;
        self.refresh(hwnd, true);
    }

    pub(super) fn camera_snap_guide(&self, track: HitRect) -> Option<TimeTick> {
        use panzo_core::TimeMapping;
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_MENU};
        if unsafe { GetKeyState(i32::from(VK_MENU.0)) } < 0 {
            return None;
        }
        let drag = self.timeline_drag.as_ref()?;
        let (start, end) = self.dragged_camera_range(drag, track);
        let video = self.editor.video_edit();
        let playhead = video.source_time(self.session.snapshot().project_tick).ok();
        [start, end].into_iter().find_map(|edge| {
            let snapped = edge == TimeTick::ZERO
                || edge == self.editor.duration()
                || Some(edge) == playhead
                || self.editor.camera().segments.iter().any(|s| {
                    s.id != drag.original.id && (edge == s.start_tick || edge == s.end_tick)
                });
            snapped
                .then(|| {
                    video.project_time(edge).ok().or_else(|| {
                        (video.clips.last()?.source_out_tick == edge).then_some(video.duration())
                    })
                })
                .flatten()
        })
    }

    pub(super) fn dragged_camera_range(
        &self,
        drag: &super::TimelineDrag,
        track: HitRect,
    ) -> (TimeTick, TimeTick) {
        use panzo_core::TimeMapping;
        let video = self.editor.video_edit();
        let at = super::timeline_tick(track, drag.current_x, self.timeline);
        let delta = video.source_time(at).unwrap_or(drag.grab_source).0 - drag.grab_source.0;
        let segments = &self.editor.camera().segments;
        let index = segments
            .iter()
            .position(|s| s.id == drag.original.id)
            .unwrap_or(0);
        let lower = if index == 0 {
            0
        } else {
            segments[index - 1].end_tick.0
        };
        let upper = segments
            .get(index + 1)
            .map_or(self.editor.duration().0, |s| s.start_tick.0);
        let mut snaps = vec![lower, upper];
        if let Ok(tick) = video.source_time(self.session.snapshot().project_tick) {
            snaps.push(tick.0);
        }
        let project_tolerance = self
            .timeline
            .pixel_delta_to_tick(self.theme.scale(6), track.right - track.left);
        let tolerance = video
            .clip_at(at.min(TimeTick(video.duration().0 - 1)))
            .map_or(project_tolerance, |span| {
                span.clip.source_delta(project_tolerance)
            })
            .0
            .unsigned_abs();
        if unsafe {
            windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(i32::from(
                windows::Win32::UI::Input::KeyboardAndMouse::VK_MENU.0,
            ))
        } < 0
        {
            snaps.clear();
        }
        let (start, end) = crate::platform::edit_gestures::adjusted_range(
            (drag.original.start_tick.0, drag.original.end_tick.0),
            delta,
            (lower, upper),
            drag.mode,
            &snaps,
            tolerance,
        );
        (TimeTick(start), TimeTick(end))
    }

    pub(super) fn draw_camera_drag(&self, hdc: windows::Win32::Graphics::Gdi::HDC, track: HitRect) {
        if let Some(drag) = &self.timeline_drag {
            let (start, end) = self.dragged_camera_range(drag, track);
            for (_, a, b) in self.editor.video_edit().project_ranges(start, end) {
                let mut display = drag.original.clone();
                display.start_tick = a;
                display.end_tick = b;
                let mut rect = super::segment_rect(track, &display, self.timeline);
                rect.left = rect.left.max(track.left);
                rect.right = rect.right.min(track.right);
                if rect.right > rect.left {
                    border(hdc, rect, self.theme.palette.accent, 2);
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)] // Menu command IDs and their dispatch stay together.
    pub(super) fn appearance_menu(&mut self, hwnd: HWND) {
        use crate::platform::app_menu::{self, Row};
        use crate::platform::editor_ui::IconKind;
        let mut rows = app_menu::theme_rows(self.theme);
        rows.push(Row::heading("诊断"));
        rows.push(Row::command(
            1002,
            "切换镜头诊断（D）",
            IconKind::Diagnostics,
            true,
        ));
        rows.push(Row::command(
            1003,
            "诊断详情（Ctrl+C 复制）",
            IconKind::Log,
            true,
        ));
        let controls = self.control_layout(super::client_rect(hwnd));
        let choice = match app_menu::show_rows(hwnd, controls.diagnostics.0, self.theme, rows, true)
        {
            Ok(Some(choice)) => choice,
            Ok(None) => return,
            Err(error) => {
                self.note_problem(
                    "设置操作未完成，请再次打开设置重试",
                    format!("无法打开设置：{error}"),
                );
                self.refresh(hwnd, true);
                return;
            }
        };
        if let Some(result) = crate::platform::editor_ui::theme_command(choice) {
            if let Err(error) = result {
                self.note_problem(
                    "设置操作未完成，请再次打开设置重试",
                    format!("错误：设置未保存：{error}"),
                );
            }
            crate::platform::editor_ui::sync_window_theme(&mut self.theme, hwnd);
            self.refresh(hwnd, true);
            return;
        }
        match choice {
            1002 => {
                self.session.toggle_camera_diagnostics();
            }
            1003 => {
                let text = wide(&format!(
                    "{}\n{}\n\n工程：{}\n当前时间：{}\n编辑时长：{}\n保存修订：{}\n视频尺寸：{}×{}\n\n可按 Ctrl+C 复制此对话框。",
                    format_args!(
                        "{}\n\n最近诊断：\n{}",
                        self.status_message().0,
                        self.feedback.details()
                    ),
                    self.scrub_preview.proxy_status().unwrap_or_default(),
                    self.editor.project_root().display(),
                    super::format_tick(self.session.snapshot().project_tick),
                    super::format_tick(self.editor.project_duration()),
                    self.editor.settings().revision,
                    self.frame.evaluation.output.width,
                    self.frame.evaluation.output.height
                ));
                unsafe {
                    MessageBoxW(
                        Some(hwnd),
                        PCWSTR(text.as_ptr()),
                        w!("Panzo 诊断详情"),
                        MB_OK,
                    );
                }
            }
            _ => {}
        }
        self.refresh(hwnd, true);
    }

    pub(super) fn split_video(&mut self) {
        if !self.control_enabled(ControlId::Split) {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "请将播放头移到片段内部再分割".into(),
            );
            return;
        }
        if self.crop_editing {
            self.toggle_crop_editing();
        }
        let result = self
            .editor
            .split_video(self.session.snapshot().project_tick);
        match result {
            Ok(id) => {
                self.selected_video = Some(id);
                self.selected_segment = None;
                self.inspector_video_tab = true;
                self.inspector_scroll = 0;
                let result = self.apply_editor_state();
                self.record_editor_result(result, "视频已分割 · Ctrl+Z 撤销");
            }
            Err(error) => {
                let message = self.edit_error(&error);
                self.set_status(crate::platform::editor_ui::StatusSeverity::Warning, message);
            }
        }
    }

    #[allow(clippy::too_many_lines)] // Existing property kinds share validation and retry semantics.
    pub(super) fn edit_properties(&mut self, hwnd: HWND, control: ControlId) {
        self.session.pause();
        self.deferred_playback.cancel();
        let settings = self.editor.settings();
        let mut fields = match control {
            ControlId::BackgroundColor => {
                vec![("背景颜色（#RRGGBB）", settings.background.color.clone())]
            }
            ControlId::CursorProperties => {
                vec![("光标倍率（0.25–4）", settings.cursor.scale.to_string())]
            }
            ControlId::CameraProperties => {
                let Some(segment) = self.selected_segment() else {
                    return;
                };
                vec![
                    ("源开始（秒）", seconds(segment.start_tick)),
                    ("源结束（秒）", seconds(segment.end_tick)),
                    ("镜头倍率（1–8）", segment.to.scale.to_string()),
                    ("焦点 X（0–1）", segment.focus_point().x.to_string()),
                    ("焦点 Y（0–1）", segment.focus_point().y.to_string()),
                ]
            }
            ControlId::VideoTrim => {
                let video = self.editor.video_edit();
                let Some(clip) = video
                    .clips
                    .iter()
                    .find(|clip| Some(&clip.id) == self.selected_video.as_ref())
                else {
                    return;
                };
                vec![
                    ("源入点（秒）", seconds(clip.source_in_tick)),
                    ("源出点（秒）", seconds(clip.source_out_tick)),
                ]
            }
            _ => return,
        };
        loop {
            let result = editor_dialog::form_with_options(
                hwnd,
                control.accessibility_name(),
                &fields,
                &[],
                "应用",
            );
            match result {
                Ok(Some(values)) => {
                    match self.apply_property_values(control, &values) {
                        Ok(()) => {
                            let result = self.apply_editor_state();
                            self.record_editor_result(result, "属性已更新 · Ctrl+Z 撤销");
                            break;
                        }
                        Err(error) => {
                            self.set_status(
                                crate::platform::editor_ui::StatusSeverity::Warning,
                                error.clone(),
                            );
                            warning(hwnd, &error);
                            // Keep the user's input so correcting one field does not reset the rest.
                            for (field, value) in fields.iter_mut().zip(values) {
                                field.1 = value;
                            }
                        }
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    self.note_problem("参数窗口未打开，请重新点击参数重试", error);
                    break;
                }
            }
        }
        self.last_clock = Instant::now();
    }

    pub(super) fn apply_property_values(
        &mut self,
        control: ControlId,
        values: &[String],
    ) -> Result<(), String> {
        let number = |i: usize| -> Result<f64, String> {
            values[i]
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| "请输入有效数字；本次修改未应用".into())
        };
        let tick = |i| {
            number(i).and_then(|v| {
                if (0.0..=(i64::MAX / TICKS_PER_SECOND) as f64).contains(&v) {
                    Ok(TimeTick((v * TICKS_PER_SECOND as f64).round() as i64))
                } else {
                    Err("时间必须为有效的非负秒数".into())
                }
            })
        };
        let result = match control {
            ControlId::BackgroundColor => self.editor.set_solid_background(values[0].trim()),
            ControlId::CursorProperties => self
                .editor
                .set_cursor_style(self.editor.settings().cursor.visible, number(0)?),
            ControlId::VideoTrim => self.editor.trim_video(
                self.selected_video.as_deref().ok_or("请先选择视频片段")?,
                tick(0)?,
                tick(1)?,
            ),
            ControlId::CameraProperties => {
                let scale = number(2)?;
                if !(1.0..=8.0).contains(&scale) {
                    return Err("镜头倍率范围为 1–8".into());
                }
                let focus = NormalizedPoint::new(number(3)?, number(4)?)
                    .map_err(|_| "焦点 X 和 Y 应为 0–1，请修改后再应用".to_owned())?;
                self.editor.update_segment_focus(
                    self.selected_segment.as_deref().ok_or("请先选择镜头")?,
                    tick(0)?,
                    tick(1)?,
                    focus,
                    scale,
                )
            }
            _ => return Ok(()),
        };
        result.map_err(|error| self.edit_error(&error))
    }

    pub(super) fn begin_export(&mut self, hwnd: HWND) {
        if let Some(panel) = &mut self.export_panel
            && !panel.is_closed()
        {
            panel.show();
            return;
        }
        self.session.pause();
        self.deferred_playback.cancel();
        let size = self.canvas_size();
        match crate::platform::export_window::ExportPanel::open(
            hwnd,
            &self.project_name,
            size.width,
            size.height,
            &super::format_tick(self.editor.project_duration()),
            self.editor.settings(),
        ) {
            Ok(panel) => self.export_panel = Some(panel),
            Err(error) => self.note_problem(
                "导出面板未打开，请从主菜单重新打开",
                format!("无法打开导出面板：{error}"),
            ),
        }
    }

    fn start_export_snapshot(
        &mut self,
        output: std::path::PathBuf,
        preset: crate::platform::export::ExportPreset,
    ) {
        if let Some(hwnd) = self.host_hwnd
            && !self.finish_inline_input(hwnd, true)
        {
            if let Some(panel) = &mut self.export_panel {
                panel.finish(false, "请先修正当前输入，再开始导出");
            }
            return;
        }
        self.finish_wheel_edit();
        let size = self.canvas_size();
        if let Some(panel) = &mut self.export_panel {
            panel.refresh_snapshot(
                size.width,
                size.height,
                &super::format_tick(self.editor.project_duration()),
                self.editor.settings(),
            );
        }
        let snapshot = ExportSnapshot {
            camera: self.editor.camera().clone(),
            settings: self.editor.settings().clone(),
        };
        let request = ExportRequest {
            project_root: self.editor.project_root().into(),
            output_path: output,
            maximum_duration: None,
        };
        match ExportTask::start(request, snapshot, preset) {
            Ok(task) => {
                if let Some(panel) = &mut self.export_panel {
                    panel.mark_started();
                }
                self.export_task = Some(task);
                self.set_status(
                    crate::platform::editor_ui::StatusSeverity::Information,
                    "正在导出当前编辑快照".into(),
                );
            }
            Err(error) => {
                self.report_problem(
                    crate::platform::editor_ui::StatusScope::Export,
                    "导出未能开始，请检查输出位置后重新导出",
                    format!("无法启动导出：{error}"),
                );
                if let Some(panel) = &mut self.export_panel {
                    panel.finish(false, &self.status);
                }
            }
        }
    }

    pub(super) fn poll_export(&mut self) {
        use crate::platform::export_window::ExportAction;
        if self
            .export_panel
            .as_ref()
            .is_some_and(crate::platform::export_window::ExportPanel::is_closed)
        {
            self.export_panel = None;
        }
        let size = self.canvas_size();
        if let Some(panel) = &mut self.export_panel {
            panel.refresh_snapshot(
                size.width,
                size.height,
                &super::format_tick(self.editor.project_duration()),
                self.editor.settings(),
            );
        }
        let action = self
            .export_panel
            .as_mut()
            .and_then(crate::platform::export_window::ExportPanel::action);
        match action {
            Some(ExportAction::Cancel) => {
                if let Some(task) = &self.export_task {
                    task.cancel();
                    self.set_status(
                        crate::platform::editor_ui::StatusSeverity::Information,
                        "正在取消导出…".into(),
                    );
                }
            }
            Some(ExportAction::Start { output, preset }) if self.export_task.is_none() => {
                self.start_export_snapshot(output, preset);
            }
            _ => {}
        }
        let Some(task) = &self.export_task else {
            return;
        };
        if let Some(result) = task.poll() {
            self.export_task = None;
            let success = result.is_ok();
            match result {
                Ok(report) => {
                    self.resolve_problem(crate::platform::editor_ui::StatusScope::Export);
                    self.set_status(
                        crate::platform::editor_ui::StatusSeverity::Success,
                        "导出完成 · 左侧菜单可打开文件位置".into(),
                    );
                    self.export_output = Some(report.output_path);
                }
                Err(crate::platform::editor_tasks::ExportTaskError::Cancelled) => {
                    self.resolve_problem(crate::platform::editor_ui::StatusScope::Export);
                    self.set_status(
                        crate::platform::editor_ui::StatusSeverity::Information,
                        "已取消导出；需要时可重新导出".into(),
                    );
                }
                Err(error) => self.report_problem(
                    crate::platform::editor_ui::StatusScope::Export,
                    "视频未导出完成，请检查输出位置后重新导出",
                    format!("导出未完成：{error}"),
                ),
            }
            if let Some(panel) = &mut self.export_panel {
                panel.finish(success, &self.status);
            }
            if let Some(hwnd) = self.host_hwnd {
                self.refresh(hwnd, true);
            }
        } else {
            let progress = task.progress();
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                format!("正在导出 {progress}% · 可在导出面板取消"),
            );
            if let Some(panel) = &mut self.export_panel {
                panel.progress(progress);
            }
        }
    }

    pub(super) fn show_export_location(&mut self) {
        use std::os::windows::process::CommandExt;
        use windows::Win32::Graphics::Gdi::ClientToScreen;
        use windows::Win32::UI::WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, MF_STRING, TPM_RETURNCMD, TrackPopupMenu,
        };
        let mut open_video = false;
        let mut project_folder = false;
        if self.export_output.is_some()
            && let Some(hwnd) = self.host_hwnd
        {
            let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
                return;
            };
            let layout = self.control_layout(super::client_rect(hwnd));
            let rect = layout.control_rect(ControlId::Location);
            let mut point = windows::Win32::Foundation::POINT {
                x: rect.left,
                y: rect.bottom,
            };
            unsafe {
                let _ = AppendMenuW(menu, MF_STRING, 1, w!("打开导出视频"));
                let _ = AppendMenuW(menu, MF_STRING, 2, w!("打开导出文件位置"));
                let _ = AppendMenuW(menu, MF_STRING, 3, w!("打开工程文件夹"));
                let _ = ClientToScreen(hwnd, &raw mut point);
            }
            let choice =
                unsafe { TrackPopupMenu(menu, TPM_RETURNCMD, point.x, point.y, None, hwnd, None) }
                    .0;
            unsafe {
                let _ = DestroyMenu(menu);
            }
            if choice == 0 {
                return;
            }
            open_video = choice == 1;
            project_folder = choice == 3;
        }
        let path = self
            .export_output
            .as_deref()
            .filter(|_| !project_folder)
            .unwrap_or(self.editor.project_root());
        let mut command = std::process::Command::new("explorer.exe");
        if !open_video {
            command.arg(if path.is_file() { "/select," } else { "/e," });
        }
        let result = command.arg(path).creation_flags(0x0800_0000).spawn();
        if let Err(error) = result {
            self.note_problem(
                "未能打开文件位置，请检查文件或文件夹是否仍存在",
                format!("无法打开位置：{error}"),
            );
        }
    }

    pub(super) fn display_segments(&self) -> Vec<CameraSegment> {
        let video = self.editor.video_edit();
        self.editor
            .camera()
            .segments
            .iter()
            .flat_map(|segment| {
                video
                    .project_ranges(segment.start_tick, segment.end_tick)
                    .into_iter()
                    .map(|(_, start, end)| {
                        let mut display = segment.clone();
                        display.start_tick = start;
                        display.end_tick = end;
                        display
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub(super) fn draw_video_track(&self, hdc: windows::Win32::Graphics::Gdi::HDC, track: HitRect) {
        let palette = self.theme.palette;
        rounded_surface(
            hdc,
            track.0,
            palette.track,
            palette.divider,
            self.theme.scale(6),
        );
        for span in self.editor.video_edit().spans() {
            let left = timeline_x(track, span.project_in, self.timeline).max(track.left);
            let right = timeline_x(track, span.project_out, self.timeline).min(track.right);
            if right <= left {
                continue;
            }
            let rect = RECT {
                left,
                right,
                top: track.top + 3,
                bottom: track.bottom - 3,
            };
            let selected = self.selected_video.as_deref() == Some(span.clip.id.as_str());
            rounded_surface(
                hdc,
                rect,
                palette.video_clip,
                if selected {
                    palette.accent
                } else {
                    palette.video_border
                },
                self.theme.scale(4),
            );
            for (tile, tick) in self
                .visible_thumbnails(track)
                .into_iter()
                .filter(|(tile, _)| tile.left >= left && tile.left < right)
            {
                if let Some(frame) = self.thumbnails.get(tick) {
                    use windows::Win32::Graphics::Gdi::{IntersectClipRect, RestoreDC, SaveDC};
                    let saved = unsafe { SaveDC(hdc) };
                    unsafe {
                        IntersectClipRect(
                            hdc,
                            left + 3,
                            track.top + 4,
                            right - 3,
                            track.bottom - 4,
                        );
                    }
                    draw_thumbnail(hdc, tile, frame);
                    unsafe {
                        let _ = RestoreDC(hdc, saved);
                    }
                }
            }
            if selected {
                border(hdc, rect, palette.accent, 2);
            }
            if right - left >= self.theme.scale(96) {
                let label = RECT {
                    left: left + self.theme.scale(7),
                    right: (left + self.theme.scale(125)).min(right - self.theme.scale(7)),
                    top: rect.top + self.theme.scale(3),
                    bottom: (rect.top + self.theme.scale(25))
                        .min(rect.bottom - self.theme.scale(3)),
                };
                rounded_surface(
                    hdc,
                    label,
                    palette.raised,
                    palette.raised,
                    self.theme.scale(4),
                );
                draw_text_styled(
                    hdc,
                    RECT {
                        left: label.left + self.theme.scale(5),
                        right: label.right - self.theme.scale(5),
                        ..label
                    },
                    &format!("视频 · {}", seconds(span.clip.duration())),
                    palette.text_secondary,
                    DT_LEFT | DT_SINGLELINE | DT_VCENTER,
                    TextStyle::Caption,
                );
            }
            for x in [left + 4, right - 6] {
                fill(
                    hdc,
                    &RECT {
                        left: x,
                        right: x + 2,
                        top: rect.top + 10,
                        bottom: rect.bottom - 10,
                    },
                    palette.accent,
                );
            }
        }
    }

    pub(super) fn scrollbar_thumb(&self, track: HitRect) -> HitRect {
        let width = track.right - track.left;
        let total = self.editor.project_duration().0.max(1);
        let thumb_width = ((i128::from(width) * i128::from(self.timeline.visible_duration().0)
            / i128::from(total)) as i32)
            .clamp(self.theme.scale(32).min(width), width);
        let available = (total - self.timeline.visible_duration().0).max(1);
        let offset = (i128::from(width - thumb_width) * i128::from(self.timeline.visible_start().0)
            / i128::from(available)) as i32;
        HitRect(RECT {
            left: track.left + offset,
            right: track.left + offset + thumb_width,
            ..track.0
        })
    }

    pub(super) fn draw_scrollbar(&self, hdc: windows::Win32::Graphics::Gdi::HDC, track: HitRect) {
        if self.timeline.is_fit() {
            return;
        }
        rounded_surface(
            hdc,
            track.0,
            self.theme.palette.track,
            self.theme.palette.track,
            self.theme.scale(3),
        );
        rounded_surface(
            hdc,
            RECT {
                top: track.top + self.theme.scale(3),
                bottom: track.bottom - self.theme.scale(3),
                ..self.scrollbar_thumb(track).0
            },
            self.theme.palette.border,
            self.theme.palette.border,
            self.theme.scale(3),
        );
    }

    pub(super) fn visible_thumbnails(&self, track: HitRect) -> Vec<(RECT, TimeTick)> {
        use panzo_core::TimeMapping;
        let video = self.editor.video_edit();
        let mut tiles = Vec::new();
        let tile_width = i32::try_from(self.thumbnail_size(track).0).unwrap_or(1) + 2;
        for span in video.spans() {
            let left = timeline_x(track, span.project_in, self.timeline).max(track.left);
            let right = timeline_x(track, span.project_out, self.timeline).min(track.right);
            let mut x = left + 3;
            while x < right - 3
                && tiles.len() < crate::platform::timeline_thumbnails::MAX_VISIBLE_THUMBNAILS
            {
                let project = super::timeline_tick(track, x, self.timeline);
                if let Ok(source) = video.source_time(project) {
                    tiles.push((
                        RECT {
                            left: x,
                            right: x + tile_width - 2,
                            top: track.top + 4,
                            bottom: track.bottom - 4,
                        },
                        source,
                    ));
                }
                x += tile_width;
            }
        }
        tiles
    }

    pub(super) fn thumbnail_size(&self, track: HitRect) -> (u32, u32) {
        let height = (track.bottom - track.top - 8).max(1);
        let source = self.session.output();
        let width = i64::from(height) * i64::from(source.width) / i64::from(source.height);
        (
            u32::try_from(width).unwrap_or(1).max(1),
            height.cast_unsigned(),
        )
    }
}

fn draw_thumbnail(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    frame: &crate::platform::media_decoder::DecodedBgraFrame,
) {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HALFTONE, RestoreDC, SRCCOPY, SaveDC,
        SetBrushOrgEx, SetStretchBltMode, StretchDIBits,
    };
    let bitmap = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: i32::try_from(frame.width).unwrap_or(160),
            biHeight: -i32::try_from(frame.height).unwrap_or(90),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let fit = super::video_rect(rect, frame.width, frame.height);
    unsafe {
        let saved = SaveDC(hdc);
        SetStretchBltMode(hdc, HALFTONE);
        let _ = SetBrushOrgEx(hdc, 0, 0, None);
        StretchDIBits(
            hdc,
            fit.left,
            fit.top,
            fit.right - fit.left,
            fit.bottom - fit.top,
            0,
            0,
            bitmap.bmiHeader.biWidth,
            -bitmap.bmiHeader.biHeight,
            Some(frame.pixels.as_ptr().cast()),
            &raw const bitmap,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        let _ = RestoreDC(hdc, saved);
    }
}

fn seconds(tick: TimeTick) -> String {
    format!("{:.3}", tick.0 as f64 / TICKS_PER_SECOND as f64)
}
fn warning(hwnd: HWND, text: &str) {
    let text = wide(text);
    unsafe {
        MessageBoxW(
            Some(hwnd),
            PCWSTR(text.as_ptr()),
            w!("请检查输入"),
            MB_OK | MB_ICONWARNING,
        );
    }
}
