//! The editor owns one inspector boundary for built-in and plugin content alike.
use super::{ControlId, HWND, WindowState, client_rect, state_mut};
use crate::platform::{media_panel, workspace_ui as ui};
use windows::Win32::UI::WindowsAndMessaging::{GetParent, SetParent};

impl WindowState {
    pub(super) fn clear_inspector_panel(&mut self) {
        if let Some(panel) = self.inspector_panel.take() {
            ui::show(panel, false);
        }
    }

    pub(super) fn inspector_tab_selected(&self, id: ControlId) -> bool {
        if let Some(panel) = self.inspector_panel {
            if let Some(audio)=crate::platform::visual_panel::page(panel){return id==if audio{ControlId::AudioTab}else{ControlId::VideoTab};}
            return match media_panel::page(panel) {
                Some(0) => id == ControlId::AudioTab,
                Some(1) => id == ControlId::SubtitleTab,
                Some(2) => id == ControlId::AudioTab,
                _ => false,
            };
        }
        if self.inspector_video_tab {
            id == ControlId::VideoTab
        } else {
            id == ControlId::CanvasTab
        }
    }

    pub(super) fn layout_inspector_panel(&self, hwnd: HWND) {
        if let Some(panel) = self.inspector_panel {
            let rect = self.control_layout(client_rect(hwnd)).inspector_content();
            let visible = rect.right > rect.left && rect.bottom > rect.top;
            if visible {
                ui::place(
                    panel,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                );
            }
            ui::show(panel, visible);
        }
    }
}

pub(crate) fn attach_inspector_panel(hwnd: HWND, panel: HWND) -> Result<(), String> {
    let s = unsafe { state_mut(hwnd) }.ok_or("请先打开视频工程")?;
    if unsafe { GetParent(panel) }.ok() != Some(hwnd) {
        unsafe { SetParent(panel, Some(hwnd)) }.map_err(|e| format!("无法打开属性面板：{e}"))?;
        ui::forget(panel);
    }
    if s.inspector_panel != Some(panel) {
        s.clear_inspector_panel();
    }
    s.finish_inline_input(hwnd, false);
    s.inspector_panel = Some(panel);
    if s.theme.metrics.inspector_width == 0 {
        s.theme.metrics.inspector_width = s.theme.scale(s.inspector_width_dip);
    }
    s.inspector_scroll = 0;
    s.refresh(hwnd, true);
    Ok(())
}

pub(crate) fn hide_inspector_panel(hwnd: HWND, panel: HWND) -> bool {
    let Some(s) = (unsafe { state_mut(hwnd) }) else {
        return false;
    };
    if s.inspector_panel != Some(panel) {
        return false;
    }
    s.clear_inspector_panel();
    s.refresh(hwnd, true);
    true
}

#[cfg(test)]
pub(crate) fn inspector_panel(hwnd: HWND) -> Option<HWND> {
    unsafe { state_mut(hwnd) }.and_then(|s| s.inspector_panel)
}
