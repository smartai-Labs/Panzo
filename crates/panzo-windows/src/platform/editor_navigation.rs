//! Content transitions are completed by the main window after the current callback returns.
use super::{
    Arc, CameraEditKind, DestroyWindow, EditorTheme, GetModuleHandleW, HINSTANCE, HWND, Instant,
    LPARAM, NavigationIntent, Ordering, PCWSTR, PostMessageW, PreviewSurface, PreviewWindowError,
    SmokeAction, WPARAM, WindowState, editor_titlebar, w, wide, win, window_dpi,
};
#[cfg(test)]
use super::{ControlId, PlaybackState};
use std::path::Path;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::UI::WindowsAndMessaging::{RemovePropW, SetPropW};

impl WindowState {
    pub(crate) fn accept_loaded(
        loaded: crate::platform::editor_load::PreparedEditorLoad,
    ) -> Result<Box<Self>, PreviewWindowError> {
        Self::from_loaded(loaded, None, None, SmokeAction::None, Arc::default()).map(Box::new)
    }

    pub(crate) fn loading(&mut self, hwnd: HWND) {
        self.navigation_authorized = true;
        self.set_status(
            crate::platform::editor_ui::StatusSeverity::Information,
            "正在打开工程… · Esc 取消".into(),
        );
        self.refresh(hwnd, true);
    }

    pub(crate) fn cancel_loading(&mut self, hwnd: HWND) {
        self.cancel_navigation();
        self.set_status(
            crate::platform::editor_ui::StatusSeverity::Information,
            "已取消打开，当前工程仍保持打开".into(),
        );
        self.refresh(hwnd, true);
    }

    pub(crate) fn cleanup_failed(&mut self, hwnd: HWND) {
        self.cancel_navigation();
        self.refresh(hwnd, true);
    }

    pub(crate) fn attach(&mut self, hwnd: HWND) -> Result<(), PreviewWindowError> {
        self.theme = EditorTheme::current(window_dpi(hwnd));
        self.restore_layout();
        let instance = HINSTANCE(win("GetModuleHandleW", unsafe { GetModuleHandleW(None) })?.0);
        self.initialize_preview_surface(hwnd, instance)?;
        self.caption_bounds = editor_titlebar::caption_bounds(hwnd);
        self.telemetry.window_created.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub(crate) fn bind(&mut self, hwnd: HWND) -> windows::core::Result<()> {
        // Publish only a committed, owned session. Failed SetPropW preserves the
        // previous property; callers must keep that previous owner alive until success.
        unsafe {
            SetPropW(
                hwnd,
                w!("PanzoEditorState"),
                Some(HANDLE(std::ptr::from_mut(self).cast())),
            )
        }
    }

    pub(crate) fn publish(&mut self, hwnd: HWND) -> windows::core::Result<()> {
        self.bind(hwnd)?;
        self.show_content(hwnd);
        Ok(())
    }

    pub(crate) fn show_content(&mut self, hwnd: HWND) {
        if let Some(surface) = &self.preview_surface {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                    surface.hwnd,
                    windows::Win32::UI::WindowsAndMessaging::SW_SHOW,
                );
            }
        }
        self.last_clock = Instant::now();
        self.refresh(hwnd, true);
        if let Some(notice) = self.recovery_notice.take() {
            let message = wide(&notice);
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    Some(hwnd),
                    PCWSTR(message.as_ptr()),
                    w!("编辑恢复"),
                    windows::Win32::UI::WindowsAndMessaging::MB_OK,
                );
            }
        }
    }

    pub(crate) fn project_root(&self) -> &Path {
        self.editor.project_root()
    }

    pub(crate) fn panel_hwnd(&self) -> Option<HWND> {
        self.export_panel
            .as_ref()
            .filter(|panel| panel.is_visible())
            .map(crate::platform::export_window::ExportPanel::hwnd)
    }

    pub(crate) fn take_navigation(&mut self) -> Option<NavigationIntent> {
        if self.navigation_authorized
            && self.save_task.is_none()
            && self.export_task.is_none()
            && self.background_task.is_none()
        {
            self.navigation_authorized = false;
            self.pending_navigation.take()
        } else {
            None
        }
    }

    pub(crate) fn navigation_failed(&mut self, hwnd: HWND, error: &str) {
        self.cancel_navigation();
        self.note_problem(
            "未能打开目标工程，请重新选择有效工程；当前工程保持打开",
            format!("错误：{error} · 当前工程仍保持打开"),
        );
        self.refresh(hwnd, true);
    }

    pub(crate) fn confirm_leave(&mut self) -> Option<bool> {
        if let Some(drafts) = &mut self.draft_manager {
            match drafts.try_discard_and_stop(&self.editor)? {
                Ok(()) => (),
                Err(error) => {
                    self.report_problem(
                        crate::platform::editor_ui::StatusScope::RecoveryDraft,
                        "暂时无法清理恢复副本，请保存当前修改后再关闭",
                        format!("错误：恢复副本未能清理，工程仍保持打开 · {error}"),
                    );
                    return Some(false);
                }
            }
        }
        Some(true)
    }

    pub(crate) fn detach(&mut self, hwnd: HWND) {
        // Invalidate the published pointer before destroying child controls. Their focus
        // notifications must not reach this session again during teardown.
        unsafe {
            if windows::Win32::UI::WindowsAndMessaging::GetPropW(hwnd, w!("PanzoEditorState")).0
                == std::ptr::from_mut(self).cast()
            {
                let _ = RemovePropW(hwnd, w!("PanzoEditorState"));
            }
        }
        self.accessibility.detach();
        self.session.pause();
        self.deferred_playback.cancel();
        self.frame_clock = None;
        self.timer_resolution = None;
        self.inline_input = None;
        self.export_panel = None;
        self.preview_surface = None;
        self.host_hwnd = None;
        self.telemetry.graceful_close.store(true, Ordering::Relaxed);
    }

    pub(super) fn cancel_navigation(&mut self) {
        self.pending_navigation = None;
        self.navigation_authorized = false;
        if let Some(drafts) = &mut self.draft_manager {
            drafts.resume();
        }
    }

    pub(super) fn on_close(&mut self, hwnd: HWND) {
        self.request_navigation(hwnd, NavigationIntent::Quit);
    }

    pub(super) fn request_navigation(&mut self, hwnd: HWND, intent: NavigationIntent) {
        // One intent only: repeated clicks during a save/finalization never queue exits.
        if self.pending_navigation.is_some() || !self.finish_inline_input(hwnd, true) {
            return;
        }
        self.cancel_gesture(hwnd);
        self.cancel_pressed_control(hwnd, true);
        self.finish_wheel_edit();
        self.persist_layout();
        self.session.pause();
        self.deferred_playback.cancel();
        if let Some(task) = &self.export_task {
            use windows::Win32::UI::WindowsAndMessaging::{
                IDYES, MB_ICONQUESTION, MB_YESNO, MessageBoxW,
            };
            let answer = unsafe {
                MessageBoxW(
                    Some(hwnd),
                    w!("导出尚未完成。取消导出并离开当前工程？"),
                    w!("正在导出"),
                    MB_YESNO | MB_ICONQUESTION,
                )
            };
            if answer != IDYES {
                return;
            }
            task.cancel();
        }
        self.pending_navigation = Some(intent);
        self.advance_navigation(hwnd);
    }

    pub(super) fn advance_navigation(&mut self, hwnd: HWND) {
        if self.pending_navigation.is_none()
            || self.navigation_prompt_open
            || crate::platform::app_menu::is_open()
        {
            return;
        }
        if self.save_task.is_some() || self.export_task.is_some() || self.background_task.is_some()
        {
            self.set_status(
                crate::platform::editor_ui::StatusSeverity::Information,
                "正在完成当前任务，完成后继续…".into(),
            );
            return;
        }
        if !self.navigation_authorized {
            if self.editor.is_dirty() {
                use windows::Win32::UI::WindowsAndMessaging::{
                    IDNO, IDYES, MB_ICONQUESTION, MB_YESNOCANCEL, MessageBoxW,
                };
                self.navigation_prompt_open = true;
                #[cfg(test)]
                let review_answer = self.review_navigation_answer.take();
                #[cfg(not(test))]
                let review_answer = None;
                let answer = if let Some(answer) = review_answer {
                    answer
                } else if self.close_at.is_some() {
                    IDYES
                } else {
                    unsafe {
                        MessageBoxW(
                            Some(hwnd),
                            w!(
                                "是否保存当前修改？\n是：保存后继续\n否：不保存\n取消：留在当前工程"
                            ),
                            w!("未保存的修改"),
                            MB_YESNOCANCEL | MB_ICONQUESTION,
                        )
                    }
                };
                self.navigation_prompt_open = false;
                if answer == IDYES {
                    self.save_editor(CameraEditKind::Update);
                    if self.save_task.is_none() {
                        self.cancel_navigation();
                    }
                    return;
                }
                if answer != IDNO {
                    self.cancel_navigation();
                    return;
                }
            }
            self.navigation_authorized = true;
        }
        unsafe {
            let _ = PostMessageW(
                Some(hwnd),
                crate::platform::recorder_window::WM_NAVIGATE,
                WPARAM(self.generation),
                LPARAM(0),
            );
        }
    }
}

#[cfg(test)]
impl WindowState {
    pub(crate) fn review_snapshot(
        &self,
    ) -> crate::platform::recorder_window::unified_window_review::ReviewEditorSnapshot {
        crate::platform::recorder_window::unified_window_review::ReviewEditorSnapshot {
            generation: self.generation,
            project_tick: self.session.snapshot().project_tick.0,
            dirty: self.editor.is_dirty(),
            navigation_pending: self.pending_navigation.is_some() || self.navigation_authorized,
            saving: self.save_task.is_some(),
            playing: self.session.snapshot().playback == PlaybackState::Playing,
            rendered_frames: u64::from(self.telemetry.rendered_frames.load(Ordering::Relaxed)),
        }
    }

    pub(crate) fn review_request_navigation(&mut self, hwnd: HWND, intent: NavigationIntent) {
        self.request_navigation(hwnd, intent);
    }

    pub(crate) fn review_invoke_provider(
        &self,
    ) -> windows::core::Result<windows::Win32::UI::Accessibility::IInvokeProvider> {
        self.accessibility
            .review_invoke_provider(ControlId::Play as u32 + 1)
    }

    pub(crate) fn review_dirty_navigation(
        &mut self,
        hwnd: HWND,
        intent: NavigationIntent,
        answer: crate::platform::recorder_window::unified_window_review::ReviewLeaveAnswer,
    ) -> Result<(), String> {
        use crate::platform::recorder_window::unified_window_review::ReviewLeaveAnswer;
        use windows::Win32::UI::WindowsAndMessaging::{IDCANCEL, IDNO};
        let inset = if self.editor.settings().canvas.inset < 0.1 {
            0.15
        } else {
            0.05
        };
        self.editor
            .set_canvas_inset(inset)
            .map_err(|error| error.to_string())?;
        self.review_navigation_answer = Some(match answer {
            ReviewLeaveAnswer::Discard => IDNO,
            ReviewLeaveAnswer::Cancel => IDCANCEL,
        });
        self.request_navigation(hwnd, intent);
        Ok(())
    }

    pub(crate) fn review_begin_save_navigation(
        &mut self,
        hwnd: HWND,
    ) -> Result<crate::platform::recorder_window::unified_window_review::ReviewPendingSave, String>
    {
        self.review_edit_during_save()?;
        if !self.editor.is_dirty() {
            self.editor
                .set_canvas_inset(0.15)
                .map_err(|error| error.to_string())?;
        }
        if !self.editor.is_dirty() || self.save_task.is_some() || self.pending_navigation.is_some()
        {
            return Err("save-navigation test requires one dirty idle editor".into());
        }
        self.editor.finish_edit_group();
        let snapshot = self.editor.background_snapshot();
        let (task, complete) = crate::platform::editor_tasks::SaveTask::review_pending();
        self.save_task = Some(task);
        self.request_navigation(hwnd, NavigationIntent::NewRecording);
        Ok(
            crate::platform::recorder_window::unified_window_review::ReviewPendingSave {
                complete,
                snapshot,
            },
        )
    }

    pub(crate) fn review_edit_during_save(&mut self) -> Result<(), String> {
        let inset = if self.editor.settings().canvas.inset < 0.125 {
            0.2
        } else {
            0.1
        };
        self.editor
            .set_canvas_inset(inset)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn review_poll_save(&mut self, hwnd: HWND) {
        self.poll_save(hwnd);
    }

    /// Exercise both live input paths, then repeat them with authorization held.
    /// Restore the cloned model so this probe never persists its positive controls.
    pub(crate) fn review_blocked_navigation_inputs(&mut self, hwnd: HWND) -> Result<(), String> {
        let original = self.editor.clone();
        let selected = self.selected_segment.clone();
        let video_selected = self.selected_video.clone();
        let video_tab = self.inspector_video_tab;
        let result = self.review_navigation_input_cases(hwnd);
        self.cancel_navigation();
        self.wheel_edit_deadline = None;
        self.wheel_edit_pending = false;
        self.editor = original;
        self.selected_segment = selected;
        self.selected_video = video_selected;
        self.inspector_video_tab = video_tab;
        let restored = self.apply_editor_state().map_err(|error| error.to_string());
        self.refresh(hwnd, true);
        result.and(restored)
    }

    fn review_navigation_input_cases(&mut self, hwnd: HWND) -> Result<(), String> {
        use windows::Win32::UI::Accessibility::IRangeValueProvider;
        use windows::core::Interface;
        self.inspector_video_tab = false;
        if self.editor.camera().segments.is_empty() {
            self.add_manual_segment();
        }
        self.selected_segment = self
            .editor
            .camera()
            .segments
            .first()
            .map(|segment| segment.id.clone());
        if self.selected_segment.is_none() {
            return Err("wheel positive control needs a camera segment".into());
        }
        let video = super::preview_video_rect(super::client_rect(hwnd), &self.frame, self.theme);
        let (x, y) = (
            i32::midpoint(video.left, video.right),
            i32::midpoint(video.top, video.bottom),
        );
        let camera = self.editor.camera().clone();
        self.on_mouse_wheel(hwnd, 120, x, y);
        self.finish_wheel_edit();
        if self.editor.camera() == &camera {
            return Err("unlocked wheel positive control failed to change the camera".into());
        }
        self.publish_accessibility(hwnd);
        let range: IRangeValueProvider = self
            .accessibility
            .review_invoke_provider(ControlId::Inset as u32 + 1)
            .and_then(|provider| provider.cast())
            .map_err(|error| error.to_string())?;
        let inset = self.editor.settings().canvas.inset;
        let requested = if inset < 0.125 { 20.0 } else { 10.0 };
        unsafe { range.SetValue(requested) }.map_err(|error| error.to_string())?;
        self.accessibility_actions(hwnd);
        if self.editor.settings().canvas.inset == inset {
            return Err("unlocked UIA SetRange positive control failed to edit inset".into());
        }
        // Queue while enabled. The navigation gate must also reject actions which
        // were valid when queued, before loading() disables the published controls.
        unsafe { range.SetValue(if requested == 20.0 { 10.0 } else { 20.0 }) }
            .map_err(|error| error.to_string())?;
        let before = (self.editor.camera().clone(), self.editor.settings().clone());
        self.loading(hwnd);
        self.on_mouse_wheel(hwnd, 120, x, y);
        self.accessibility_actions(hwnd);
        if self.editor.camera() != &before.0 || self.editor.settings() != &before.1 {
            return Err("authorized navigation accepted wheel or queued UIA model edits".into());
        }
        self.cancel_loading(hwnd);
        self.accessibility_actions(hwnd);
        if self.editor.camera() != &before.0 || self.editor.settings() != &before.1 {
            return Err("blocked UIA edit replayed after cancelling navigation".into());
        }
        Ok(())
    }
}

impl Drop for PreviewSurface {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
