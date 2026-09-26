//! Application appearance preferences, independent of project edit history.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    theme: ThemeMode,
    layout: LayoutPreferences,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LayoutPreferences {
    pub inspector_width: i32,
    pub inspector_visible: bool,
    pub timeline_height: i32,
    pub video_collapsed: [bool; 2],
}
impl Default for LayoutPreferences {
    fn default() -> Self {
        Self {
            inspector_width: 288,
            inspector_visible: true,
            timeline_height: 240,
            video_collapsed: [false; 2],
        }
    }
}

pub fn layout() -> LayoutPreferences {
    let mut value = preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .layout;
    value.inspector_width = value.inspector_width.clamp(264, 560);
    value.timeline_height = value.timeline_height.clamp(216, 1200);
    value
}

pub fn set_layout(layout: LayoutPreferences) -> Result<(), String> {
    let path = settings_path().ok_or("找不到应用设置目录")?;
    let mut preferences = preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let next = Preferences {
        theme: preferences.theme,
        layout,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    super::manifest_store::persist_json_atomically(&path, &next).map_err(|e| e.to_string())?;
    preferences.layout = layout;
    Ok(())
}

fn settings_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Panzo/settings.json"))
}

fn read(path: &Path) -> Preferences {
    std::fs::metadata(path)
        .ok()
        .filter(|m| m.len() <= 65_536)
        .and_then(|_| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn preferences() -> &'static Mutex<Preferences> {
    static VALUE: OnceLock<Mutex<Preferences>> = OnceLock::new();
    VALUE
        .get_or_init(|| Mutex::new(settings_path().map_or_else(Preferences::default, |p| read(&p))))
}

pub fn theme() -> ThemeMode {
    preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .theme
}

fn save(path: &Path, theme: ThemeMode) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let layout = read(path).layout;
    super::manifest_store::persist_json_atomically(path, &Preferences { theme, layout })
        .map_err(|e| e.to_string())
}

pub fn set_theme(theme: ThemeMode) -> Result<(), String> {
    let path = settings_path().ok_or("找不到应用设置目录")?;
    let mut preferences = preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    save(&path, theme)?;
    preferences.theme = theme;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires isolated LOCALAPPDATA matching PANZO_SETTINGS_REVIEW_DIR"]
    fn settings_menu_persists_and_updates_runtime_theme() {
        let isolated =
            std::env::var_os("PANZO_SETTINGS_REVIEW_DIR").expect("isolated settings directory");
        assert_eq!(Some(isolated.clone()), std::env::var_os("LOCALAPPDATA"));
        assert!(PathBuf::from(isolated).is_dir());
        let expected_layout = LayoutPreferences {
            inspector_width: 416,
            inspector_visible: false,
            timeline_height: 278,
            video_collapsed: [true, false],
        };
        set_layout(expected_layout).unwrap();
        for (command, expected) in [
            (2502, ThemeMode::Dark),
            (2501, ThemeMode::Light),
            (2502, ThemeMode::Dark),
        ] {
            super::super::editor_ui::theme_command(command)
                .unwrap()
                .unwrap();
            assert_eq!(theme(), expected);
            assert_eq!(read(&settings_path().unwrap()).theme, expected);
            assert_eq!(layout(), expected_layout);
            assert_eq!(read(&settings_path().unwrap()).layout, expected_layout);
            assert_eq!(
                super::super::editor_ui::EditorTheme::current(144).mode,
                expected
            );
        }
        assert!(super::super::editor_ui::theme_command(0).is_none());
        assert_eq!(theme(), ThemeMode::Dark);
    }

    #[test]
    fn theme_roundtrips_and_invalid_settings_fall_back_to_light() {
        let root = std::env::temp_dir().join(format!("panzo-appearance-{}", uuid::Uuid::new_v4()));
        let path = root.join("settings.json");
        assert_eq!(read(&path).theme, ThemeMode::Light);
        for theme in [ThemeMode::Dark, ThemeMode::Light] {
            save(&path, theme).unwrap();
            assert_eq!(read(&path).theme, theme);
        }
        std::fs::write(&path, b"{broken").unwrap();
        assert_eq!(read(&path).theme, ThemeMode::Light);
        std::fs::write(&path, br#"{"theme":"future-theme"}"#).unwrap();
        assert_eq!(read(&path).theme, ThemeMode::Light);
        std::fs::remove_dir_all(root).unwrap();
    }
}
