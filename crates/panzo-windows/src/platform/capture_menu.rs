//! Feature menu and capture actions hosted by the existing main window.
use super::{
    ButtonRole, ButtonVisualState, ControlLayout, DT_LEFT, DT_SINGLELINE, DT_VCENTER, HWND,
    NavigationIntent, OsStringExt, PCWSTR, POINT, Path, PathBuf, RECT, RecorderControlId,
    RecorderPhase, RecordingSource, SW_SHOW, SetForegroundWindow, ShowWindow, TextStyle,
    WindowState, button_colors, button_interaction, draw_icon, draw_text_styled, focus_ring,
    rounded_surface, w, wide,
};
use crate::platform::capture_tools::{self, CaptureIntent, CaptureOutcome, Raster};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HDC, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, SW_HIDE};

impl WindowState {
    fn capture_error(&mut self, hwnd: HWND, message: &str) {
        self.set_status(
            crate::platform::editor_ui::StatusSeverity::Error,
            message.into(),
        );
        if crate::platform::workspace::active() {
            crate::platform::workspace::notice_error(message);
            self.refresh(hwnd);
            return;
        }
        let value = wide(message);
        unsafe {
            crate::platform::app_dialog::message(
                Some(hwnd),
                PCWSTR(value.as_ptr()),
                w!("Panzo"),
                MB_OK | MB_ICONERROR,
            );
        }
        self.refresh(hwnd);
    }
    pub(super) fn capture_selection(&mut self, hwnd: HWND, intent: CaptureIntent) {
        if self.phase.is_active() || self.transition.is_some() {
            return;
        }
        if let Err(error) = capture_tools::begin(hwnd, intent) {
            self.capture_error(hwnd, &error);
        }
    }
    pub(super) fn capture_complete(&mut self, hwnd: HWND) {
        match capture_tools::take_outcome() {
            Some(CaptureOutcome::Ocr(image)) if crate::platform::workspace::active() => {
                if let Err(e) = capture_tools::show_image(image, &self.library_root, false) {
                    self.capture_error(hwnd, &e);
                }
            }
            Some(CaptureOutcome::Ocr(image)) => {
                if let Err(e) =
                    crate::platform::image_workflows::ocr(hwnd, image, &self.library_root)
                {
                    self.capture_error(hwnd, &e);
                }
            }
            Some(CaptureOutcome::Scroll(region)) => {
                if let Err(e) = capture_tools::start_scroll(hwnd, region, &self.library_root) {
                    self.capture_error(hwnd, &e);
                }
            }
            Some(CaptureOutcome::Image(image)) => {
                if let Err(error) = capture_tools::remember_capture(&image) {
                    self.capture_error(hwnd, &error);
                }
                if let Err(error) = capture_tools::show_image(image, &self.library_root, false) {
                    self.capture_error(hwnd, &error);
                }
            }
            Some(CaptureOutcome::Pin(image)) => {
                if let Err(error) = capture_tools::show_image(image, &self.library_root, true) {
                    self.capture_error(hwnd, &error);
                }
            }
            Some(CaptureOutcome::Project(path)) => {
                if let Some(editor) = &mut self.editor {
                    editor.request_navigation(hwnd, NavigationIntent::OpenProject(path));
                } else {
                    self.requested_project = Some(path);
                    self.post_navigation();
                }
            }
            Some(CaptureOutcome::Region(rect)) => {
                crate::platform::workspace::switch(1);
                let monitor = unsafe {
                    MonitorFromPoint(
                        POINT {
                            x: rect.x,
                            y: rect.y,
                        },
                        MONITOR_DEFAULTTONULL,
                    )
                };
                let mut info = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                let bounds = if unsafe { GetMonitorInfoW(monitor, &raw mut info) }.as_bool() {
                    panzo_core::PhysicalRect {
                        x: info.rcMonitor.left,
                        y: info.rcMonitor.top,
                        width: info.rcMonitor.right.abs_diff(info.rcMonitor.left),
                        height: info.rcMonitor.bottom.abs_diff(info.rcMonitor.top),
                    }
                } else {
                    self.capture_error(hwnd, "无法读取选区所在的显示器");
                    return;
                };
                let Some(rect) = panzo_core::geometry::recording_region(rect, bounds) else {
                    self.capture_error(hwnd, "录屏选区须位于同一个显示器内，且至少为 2 × 2 像素");
                    return;
                };
                self.capture_width = rect.width;
                self.capture_height = rect.height;
                self.capture_source = RecordingSource::Region(rect);
                if crate::platform::recording_setup::is_open() {
                    crate::platform::recording_setup::source(&format!("已选区域 · {} × {}",rect.width,rect.height));
                } else if let Some(editor) = &mut self.editor {
                    editor.request_navigation(hwnd, NavigationIntent::NewRecording);
                } else {
                    self.home = false;
                    self.phase = RecorderPhase::Ready;
                    self.status = "区域已选定，设置缩放后点击开始录制".into();
                    self.refresh(hwnd);
                }
            }
            _ => {if crate::platform::recording_setup::is_open(){crate::platform::recording_setup::source(&self.capture_source.label());}}
        }
    }
    #[allow(clippy::too_many_lines)] // Native command dispatch shares one state owner.
    pub(super) fn shell_command(&mut self, hwnd: HWND, command: u32) {
        if command == 12 && crate::platform::app_shell::close_confirmation_open() {
            return;
        }

        if self.import_job.is_some() && ![8, 9, 11, 12, 17, 2501, 2502].contains(&command) {
            return;
        }
        if crate::platform::workspace::active() {
            match command {
                10 => {
                    let root = crate::platform::workspace::window(hwnd);
                    unsafe {
                        let _ =
                            ShowWindow(root, windows::Win32::UI::WindowsAndMessaging::SW_RESTORE);
                        let _ = SetForegroundWindow(root);
                    }
                    return;
                }
                8 | 25 => {
                    crate::platform::workspace::switch(2);
                    return;
                }
                24 | 27 | 28 => {
                    crate::platform::workspace::switch(3);
                    return;
                }
                26 => {
                    crate::platform::workspace::switch(0);
                    return;
                }
                22 | 23 => {
                    if let Err(e) = crate::platform::workspace_image_tools::open(
                        command == 23,
                        &self.library_root,
                    ) {
                        self.capture_error(hwnd, &e);
                    }
                    return;
                }
                _ => {}
            }
        }
        match command {
            20 => self.capture_selection(hwnd, CaptureIntent::Ocr),
            21 => self.capture_selection(hwnd, CaptureIntent::Scroll),
            26 => self.show_image_tools(hwnd),
            27 => match crate::platform::vision_ui::configure(hwnd) {
                Ok(true) => {
                    self.status = "多模态识图配置已保存，重启后仍会保留".into();
                    self.refresh(hwnd);
                }
                Ok(false) => {}
                Err(e) => self.capture_error(hwnd, &e),
            },
            28 => {
                if let Err(e) =
                    crate::platform::vision_ui::test_connection(hwnd, &self.library_root)
                {
                    self.capture_error(hwnd, &e);
                }
            }
            25 => {
                if let Err(e) = capture_tools::manage_pin_groups(hwnd) {
                    self.capture_error(hwnd, &e);
                }
            }
            22..=24 if !self.phase.is_active() => {
                let result = match command {
                    22 => crate::platform::image_workflows::collage(hwnd, &self.library_root),
                    23 => crate::platform::image_workflows::batch(hwnd, &self.library_root),
                    _ => crate::platform::image_workflows::manage_ocr(hwnd),
                };
                if let Err(e) = result {
                    self.capture_error(hwnd, &e);
                }
            }
            18 | 19 if !self.phase.is_active() => {
                match crate::platform::ui_preferences::select_recording_preset(command == 19) {
                    Ok(options) => {
                        self.camera_enabled = options.camera.enabled;
                        self.recording_options = options;
                        self.status = if command == 19 {
                            "演示录屏预设已应用"
                        } else {
                            "普通录屏预设已应用"
                        }
                        .into();
                        self.refresh(hwnd);
                    }
                    Err(e) => self.capture_error(hwnd, &e),
                }
            }

            13 => {
                if let Some(control) = &self.control {
                    let _ = control.toggle_pause();
                }
            }
            14 => {
                if let Some(control) = &self.control {
                    let _ = control.set_camera_enabled(!control.camera_active());
                }
            }
            15 | 16 if !self.phase.is_active() => self.record_options(hwnd, command == 16),
            17 => {
                if let Some(job) = &self.import_job {
                    job.cancel.store(true, std::sync::atomic::Ordering::Release);
                }
            }
            1 => self.capture_selection(hwnd, CaptureIntent::Screenshot),
            2 if self.phase.is_active() => {
                self.request_stop();
                self.refresh(hwnd);
            }
            2 => self.capture_selection(hwnd, CaptureIntent::Region),
            3 => {
                if let Err(error) = capture_tools::pin_clipboard(hwnd, &self.library_root) {
                    self.capture_error(hwnd, &error);
                }
            }
            4 => {
                if let Err(error) = capture_tools::toggle_pins() {
                    self.capture_error(hwnd, &error);
                }
            }
            5 => {
                if let Err(error) = capture_tools::recover_pins() {
                    self.capture_error(hwnd, &error);
                }
            }
            6 => {
                if self.phase.is_active() || self.transition.is_some() {
                    return;
                }
                unsafe {
                    let _ = ShowWindow(crate::platform::workspace::window(hwnd), SW_HIDE);
                    windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(hwnd), 74, 3000, None);
                }
                self.status = "3 秒后截图".into();
            }
            7 => {
                if let Err(error) = capture_tools::restore_closed_pin() {
                    self.capture_error(hwnd, &error);
                }
            }
            8 => self.show_recent(hwnd),
            9 => {
                let enabled = !crate::platform::ui_preferences::close_to_tray();
                if let Err(error) = crate::platform::ui_preferences::set_close_to_tray(enabled) {
                    self.capture_error(hwnd, &error);
                } else {
                    self.status = if enabled {
                        "关闭主窗口后留在托盘；从托盘菜单退出应用".into()
                    } else {
                        "关闭主窗口将退出应用".into()
                    };
                    self.refresh(hwnd);
                }
            }
            10 => {
                unsafe {
                    let command =
                        if windows::Win32::UI::WindowsAndMessaging::IsIconic(hwnd).as_bool() {
                            windows::Win32::UI::WindowsAndMessaging::SW_RESTORE
                        } else {
                            SW_SHOW
                        };
                    let _ = ShowWindow(hwnd, command);
                    let _ = SetForegroundWindow(hwnd);
                }
                if self.phase.is_active() {
                    return;
                }
                if let Some(editor) = &mut self.editor {
                    editor.request_navigation(hwnd, NavigationIntent::Home);
                } else {
                    self.home = true;
                    self.refresh(hwnd);
                }
            }
            11 if self
                .shell
                .as_ref()
                .is_some_and(crate::platform::app_shell::ShellIntegration::available) =>
            unsafe {
                let _ = ShowWindow(crate::platform::workspace::window(hwnd), SW_HIDE);
            },
            12 => {
                if let Some(editor) = &mut self.editor {
                    editor.request_navigation(hwnd, NavigationIntent::Quit);
                } else {
                    self.close_when_finished = true;
                    self.on_close(hwnd);
                }
            }
            _ => {}
        }
    }
    pub(super) fn open_image(&mut self, hwnd: HWND) {
        let mut name = vec![0u16; 32768];
        let filter =
            wide("媒体（图片 / GIF / MP4 视频）\0*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.mp4\0\0");
        let mut dialog = OPENFILENAMEW {
            lStructSize: size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: hwnd,
            lpstrFilter: PCWSTR(filter.as_ptr()),
            lpstrFile: windows::core::PWSTR(name.as_mut_ptr()),
            nMaxFile: name.len() as u32,
            Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
            ..Default::default()
        };
        if unsafe { GetOpenFileNameW(&raw mut dialog) }.as_bool() {
            let end = name.iter().position(|c| *c == 0).unwrap_or(name.len());
            let path = PathBuf::from(std::ffi::OsString::from_wide(&name[..end]));
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("mp4"))
            {
                if self.import_job.is_none() && !self.phase.is_active() {
                    self.import_job = Some(crate::platform::media_import::ImportJob::start(
                        path,
                        self.library_root.clone(),
                    ));
                    self.status = "正在导入视频，可在设置中取消…".into();
                    self.refresh(hwnd);
                }
            } else {
                self.open_picture(hwnd, &path);
            }
        }
    }
    fn open_picture(&mut self, hwnd: HWND, path: &Path) {
        let _ = crate::platform::asset_index::register(&self.library_root, path);
        let result = if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("gif"))
        {
            capture_tools::show_gif(path, &self.library_root)
        } else {
            Raster::load(path)
                .and_then(|image| capture_tools::show_image(image, &self.library_root, false))
        };
        if let Err(error) = result {
            self.capture_error(hwnd, &error);
        }
    }
    pub(super) fn show_image_tools(&mut self, hwnd: HWND) {
        use crate::platform::{
            app_menu::{self, Row},
            editor_ui::IconKind,
        };
        let enabled = !self.phase.is_active() && self.import_job.is_none();
        let rows = vec![
            Row::heading("图片工具"),
            Row::command(20, "截图识别文字 / 多模态识图…", IconKind::Image, enabled),
            Row::command(21, "滚动长截图…", IconKind::Image, enabled),
            Row::command(22, "图片拼图…", IconKind::Image, enabled),
            Row::command(23, "批量压缩 / 格式转换…", IconKind::Image, enabled),
            Row::command(25, "贴图分组管理…", IconKind::Window, true),
            Row::command(24, "OCR 语言管理…", IconKind::Settings, enabled),
            Row::command(27, "多模态识图设置…", IconKind::Settings, enabled),
            Row::command(28, "测试多模态连接…", IconKind::Settings, enabled),
        ];
        let c = ControlLayout::for_state(hwnd, self);
        match app_menu::show_rows(
            hwnd,
            if self.home {
                c.features[4].0
            } else {
                c.diagnostics.0
            },
            self.theme,
            rows,
            false,
        ) {
            Ok(Some(action)) => self.shell_command(hwnd, action.cast_unsigned()),
            Ok(None) => {}
            Err(e) => self.capture_error(hwnd, &e.to_string()),
        }
    }
    pub(super) fn show_recent(&mut self, hwnd: HWND) {
        if let Err(error) = capture_tools::show_history(hwnd, &self.library_root) {
            self.capture_error(hwnd, &error);
        }
    }
}

#[allow(clippy::too_many_lines)] // Feature cards share a single visual layout.
pub(super) fn draw_home(hdc: HDC, controls: &ControlLayout, state: &WindowState) {
    let theme = state.theme;
    let palette = theme.palette;
    let scale = |v| theme.scale(v);
    let area = controls.hero;
    draw_text_styled(
        hdc,
        RECT {
            bottom: area.top + scale(36),
            ..area
        },
        "Panzo",
        palette.text,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        TextStyle::Title,
    );
    draw_text_styled(
        hdc,
        RECT {
            top: area.top + scale(42),
            bottom: area.top + scale(68),
            ..area
        },
        "截图、贴图与录屏，在一个地方完成",
        palette.text_secondary,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        TextStyle::Body,
    );
    for (id, title, description) in [
        (
            RecorderControlId::Screenshot,
            "截图",
            "选择区域 · 标注 · 复制或保存",
        ),
        (
            RecorderControlId::NewRecording,
            "录屏",
            "屏幕 / 窗口 / 区域 · MP4 / GIF",
        ),
        (RecorderControlId::Pin, "贴图", "将剪贴板图片置顶到桌面"),
        (
            RecorderControlId::ImageTools,
            "图片工具",
            "长截图 · 拼图 · 批量压缩 · 分组",
        ),
        (
            RecorderControlId::Ocr,
            "文字识图",
            "离线 OCR / 多模态 · 提取文字与问答",
        ),
        (
            RecorderControlId::Recent,
            "最近内容",
            "继续处理截图和录屏工程",
        ),
        (
            RecorderControlId::OpenImage,
            "导入媒体",
            "图片 / GIF 贴图 · MP4 视频编辑",
        ),
        (
            RecorderControlId::OpenProject,
            "打开工程",
            "剪辑录屏 · 调整镜头 · 导出",
        ),
    ] {
        let rect = controls.control_rect(id).0;
        let colors = button_colors(
            theme,
            ButtonRole::Standard,
            ButtonVisualState {
                interaction: button_interaction(
                    state.control_enabled(id),
                    state.pointer.pressed == Some(id),
                    state.pointer.hot == Some(id),
                ),
                selected: false,
            },
        );
        rounded_surface(hdc, rect, colors.0, colors.1, scale(10));
        let icon_rect = RECT {
            left: rect.left + scale(20),
            top: rect.top + scale(24),
            right: rect.left + scale(46),
            bottom: rect.top + scale(50),
        };
        draw_icon(hdc, icon_rect, id.icon(state.phase), palette.accent);
        let label_rect = RECT {
            left: rect.left + scale(64),
            top: rect.top + scale(13),
            right: rect.right - scale(12),
            bottom: rect.top + scale(37),
        };
        draw_text_styled(
            hdc,
            label_rect,
            title,
            palette.text,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            TextStyle::Body,
        );
        draw_text_styled(
            hdc,
            RECT {
                top: rect.top + scale(40),
                bottom: rect.bottom - scale(10),
                ..label_rect
            },
            description,
            palette.text_secondary,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            TextStyle::Caption,
        );
        if state.pointer.keyboard_focus && state.pointer.focused == Some(id) {
            focus_ring(hdc, rect, palette.accent, scale(10));
        }
    }
    draw_text_styled(
        hdc,
        controls.status,
        &state.status,
        palette.text_secondary,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        TextStyle::Caption,
    );
}

impl WindowState {
    pub(super) fn record_options(&mut self, hwnd: HWND, audio: bool) {
        if crate::platform::workspace::active() {
            if let Err(e) = crate::platform::media_settings::record_async(
                hwnd,
                self.recording_options.clone(),
                audio,
            ) {
                self.capture_error(hwnd, &e);
            }
            return;
        }
        let mut options = self.recording_options.clone();
        options.camera.enabled = self.camera_enabled;
        let result = if audio {
            crate::platform::media_settings::record_audio(hwnd, &mut options)
        } else {
            crate::platform::media_settings::record_options(hwnd, &mut options)
        };
        match result {
            Ok(true) => {
                match crate::platform::ui_preferences::set_recording_options(options.clone()) {
                    Ok(()) => {
                        self.camera_enabled = options.camera.enabled;
                        self.recording_options = options;
                        self.status = "录制设置已保存".into();
                    }
                    Err(e) => self.capture_error(hwnd, &e),
                }
            }
            Ok(false) => {}
            Err(e) => self.capture_error(hwnd, &e),
        }
        self.refresh(hwnd);
    }
    pub(super) fn poll_import(&mut self, hwnd: HWND) {
        let Some(job) = &mut self.import_job else {
            return;
        };
        if let Some(result) = job.take_result() {
            self.import_job = None;
            match result {
                Ok(path) => {
                    self.last_project = Some(path.clone());
                    self.requested_project = Some(path);
                    self.post_navigation();
                }
                Err(error) => {
                    self.status = error;
                    self.refresh(hwnd);
                }
            }
        } else {
            self.status = format!(
                "正在导入视频 {}% · 设置 → 取消视频导入",
                job.progress.load(std::sync::atomic::Ordering::Acquire)
            );
            self.refresh(hwnd);
        }
    }
}
