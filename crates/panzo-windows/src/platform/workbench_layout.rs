//! Approved v14 workbench dimensions in device-independent pixels.
pub(crate) const WORKSPACES: i32 = 44;
pub(crate) const CONTEXT: i32 = 40;
pub(crate) const PANEL_HEADER: i32 = 44;
/// Both native child panes and parent-painted panes share this title row.
pub(crate) fn panel_title(
    panel: windows::Win32::Foundation::RECT,
    theme: super::editor_ui::EditorTheme,
) -> windows::Win32::Foundation::RECT {
    windows::Win32::Foundation::RECT {
        left: panel.left + theme.scale(12),
        top: panel.top + theme.scale(13),
        right: panel.right - theme.scale(54),
        bottom: panel.top + theme.scale(41),
    }
}
pub(crate) const INSPECTOR: i32 = 304;
pub(crate) const INSPECTOR_COMPACT: i32 = 284;
pub(crate) const INSPECTOR_MIN: i32 = 264;
pub(crate) const INSPECTOR_MAX: i32 = 420;
pub(crate) const TIMELINE: i32 = 282;
pub(crate) const TIMELINE_COMPACT: i32 = 230;
pub(crate) fn inspector(width_dip: i32) -> i32 {
    if width_dip <= 1150 {
        INSPECTOR_COMPACT
    } else {
        INSPECTOR
    }
}
pub(crate) fn project_bin(width_dip: i32) -> i32 {
    if width_dip <= 1000 {
        0
    } else if width_dip <= 1150 {
        180
    } else {
        212
    }
}
