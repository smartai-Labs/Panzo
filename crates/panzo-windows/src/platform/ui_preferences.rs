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

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    preview_quality: super::preview_quality::PreviewQuality,
    recent_colors: Vec<String>,
    theme: ThemeMode,
    animations: bool,
    layout: LayoutPreferences,
    camera_enabled: bool,
    close_to_tray: bool,
    recording: super::recorder::RecordingOptions,
    recording_presets: [Option<super::recorder::RecordingOptions>; 2],
    demo_preset: bool,
    library_directory: Option<PathBuf>,
    output_directory: Option<PathBuf>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            preview_quality: Default::default(),
            recent_colors: Vec::new(),
            theme: ThemeMode::default(),
            animations: true,
            layout: LayoutPreferences::default(),
            camera_enabled: false,
            close_to_tray: true,
            recording: super::recorder::RecordingOptions::default(),
            recording_presets: [None, None],
            demo_preset: false,
            library_directory: None,
            output_directory: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LayoutPreferences {
    pub project_bin_width: i32,
    pub inspector_width: i32,
    pub inspector_visible: bool,
    pub timeline_height: i32,
    pub video_collapsed: [bool; 2],
}
impl Default for LayoutPreferences {
    fn default() -> Self {
        Self {
            project_bin_width: 0,
            inspector_width: super::workbench_layout::INSPECTOR,
            inspector_visible: true,
            timeline_height: super::workbench_layout::TIMELINE,
            video_collapsed: [false; 2],
        }
    }
}

pub fn layout() -> LayoutPreferences {
    let mut value = preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .layout;
    value.inspector_width = value.inspector_width.clamp(264, 420);
    value.timeline_height = value.timeline_height.clamp(150, 1200);
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
        camera_enabled: preferences.camera_enabled,
        close_to_tray: preferences.close_to_tray,
        recording: preferences.recording.clone(),
        ..preferences.clone()
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
    super::manifest_store::persist_json_atomically(
        path,
        &Preferences {
            theme,
            layout,
            camera_enabled: read(path).camera_enabled,
            close_to_tray: read(path).close_to_tray,
            recording: read(path).recording,
            ..read(path)
        },
    )
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

pub fn capture_camera_enabled() -> bool {
    preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .camera_enabled
}
pub fn set_capture_camera_enabled(enabled: bool) -> Result<(), String> {
    let path = settings_path().ok_or("找不到应用设置目录")?;
    let mut prefs = preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let next = Preferences {
        theme: prefs.theme,
        layout: prefs.layout,
        camera_enabled: enabled,
        close_to_tray: prefs.close_to_tray,
        recording: prefs.recording.clone(),
        ..prefs.clone()
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    super::manifest_store::persist_json_atomically(&path, &next).map_err(|e| e.to_string())?;
    prefs.camera_enabled = enabled;
    Ok(())
}

pub fn close_to_tray() -> bool {
    preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .close_to_tray
}
pub fn set_close_to_tray(enabled: bool) -> Result<(), String> {
    let path = settings_path().ok_or("找不到应用设置目录")?;
    let mut prefs = preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let next = Preferences {
        theme: prefs.theme,
        layout: prefs.layout,
        camera_enabled: prefs.camera_enabled,
        close_to_tray: enabled,
        recording: prefs.recording.clone(),
        ..prefs.clone()
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    super::manifest_store::persist_json_atomically(&path, &next).map_err(|e| e.to_string())?;
    prefs.close_to_tray = enabled;
    Ok(())
}

pub fn recording_options() -> super::recorder::RecordingOptions {
    preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .recording
        .clone()
}
pub fn set_recording_options(options: super::recorder::RecordingOptions) -> Result<(), String> {
    let path = settings_path().ok_or("找不到应用设置目录")?;
    let mut prefs = preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let next = Preferences {
        theme: prefs.theme,
        layout: prefs.layout,
        camera_enabled: options.camera.enabled,
        close_to_tray: prefs.close_to_tray,
        recording: options.clone(),
        ..prefs.clone()
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    super::manifest_store::persist_json_atomically(&path, &next).map_err(|e| e.to_string())?;
    prefs.recording = options;
    prefs.camera_enabled = next.camera_enabled;
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
            project_bin_width: 280,
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

fn switch_preset(prefs: &Preferences, demo: bool) -> Preferences {
    let mut next = prefs.clone();
    let mut current = prefs.recording.clone();
    current.camera.enabled = prefs.camera_enabled;
    next.recording_presets[usize::from(prefs.demo_preset)] = Some(current);
    let selected = next.recording_presets[usize::from(demo)]
        .clone()
        .unwrap_or_else(|| {
            let mut options = prefs.recording.clone();
            options.camera = panzo_core::camera_policy::AutoCameraConfig {
                enabled: demo,
                ..Default::default()
            };
            options
        });
    next.demo_preset = demo;
    next.camera_enabled = selected.camera.enabled;
    next.recording = selected;
    next
}
pub fn select_recording_preset(demo: bool) -> Result<super::recorder::RecordingOptions, String> {
    let path = settings_path().ok_or("找不到应用设置目录")?;
    let mut prefs = preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let next = switch_preset(&prefs, demo);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    super::manifest_store::persist_json_atomically(&path, &next).map_err(|e| e.to_string())?;
    *prefs = next;
    Ok(prefs.recording.clone())
}
#[cfg(test)]
mod preset_tests {
    use super::*;
    #[test]
    fn presets_remember_each_profile_and_main_camera_toggle() {
        let mut prefs = Preferences::default();
        prefs.recording.fps = 30;
        prefs = switch_preset(&prefs, true);
        assert!(prefs.camera_enabled);
        prefs.recording.camera.zoom = 2.0;
        prefs.camera_enabled = false;
        prefs = switch_preset(&prefs, false);
        assert_eq!(prefs.recording.fps, 30);
        assert_eq!(prefs.recording.camera.zoom, 1.5);
        prefs = switch_preset(&prefs, true);
        assert_eq!(prefs.recording.camera.zoom, 2.0);
        assert!(!prefs.camera_enabled);
    }
}

/// New library locations apply on the next launch; existing projects are never moved.
pub fn library_directory() -> Option<PathBuf> {
    preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .library_directory
        .clone()
}
pub fn set_library_directory(directory: PathBuf) -> Result<(), String> {
    super::path_field::validate(&directory.to_string_lossy())?;
    let path = settings_path().ok_or("找不到应用设置目录")?;
    let mut prefs = preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut next = prefs.clone();
    next.library_directory = Some(directory);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    super::manifest_store::persist_json_atomically(&path, &next).map_err(|e| e.to_string())?;
    *prefs = next;
    Ok(())
}

/// User export destination; capture/source storage remains the library.
pub(crate) fn output_directory()->Option<PathBuf>{
    preferences().lock().unwrap_or_else(std::sync::PoisonError::into_inner).output_directory.clone()
}
pub(crate) fn set_directories(library:PathBuf,output:PathBuf)->Result<(),String>{
    super::path_field::validate(&library.to_string_lossy())?;
    super::path_field::validate(&output.to_string_lossy())?;
    let path=settings_path().ok_or("找不到应用设置目录")?;
    let mut prefs=preferences().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut next=prefs.clone();next.library_directory=Some(library);next.output_directory=Some(output);
    if let Some(parent)=path.parent(){std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;}
    super::manifest_store::persist_json_atomically(&path,&next).map_err(|e|e.to_string())?;*prefs=next;Ok(())
}
pub(crate) fn animations() -> bool {
    preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .animations
}
pub(crate) fn set_animations(value: bool) -> Result<(), String> {
    let path = settings_path().ok_or("找不到应用设置目录")?;
    let mut prefs = preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut next = prefs.clone();
    next.animations = value;
    super::manifest_store::persist_json_atomically(path, &next).map_err(|e| e.to_string())?;
    *prefs = next;
    Ok(())
}

/// Recent RGB choices belong to the application, not an individual project.
pub(crate) fn recent_colors() -> Vec<[u8; 4]> {
    preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .recent_colors
        .iter()
        .filter_map(|s| {
            let s = s.trim_start_matches('#');
            if s.len() != 6 {
                return None;
            }
            u32::from_str_radix(s, 16)
                .ok()
                .map(|c| [c as u8, (c >> 8) as u8, (c >> 16) as u8, 255])
        })
        .take(16)
        .collect()
}
pub(crate) fn remember_color(color: [u8; 4]) -> Result<(), String> {
    let path = settings_path().ok_or("找不到应用设置目录")?;
    let mut prefs = preferences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut next = prefs.clone();
    let key = format!("#{:02X}{:02X}{:02X}", color[2], color[1], color[0]);
    next.recent_colors.retain(|c| !c.eq_ignore_ascii_case(&key));
    next.recent_colors.insert(0, key);
    next.recent_colors.truncate(16);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    super::manifest_store::persist_json_atomically(&path, &next).map_err(|e| e.to_string())?;
    *prefs = next;
    Ok(())
}

pub fn preview_quality()->super::preview_quality::PreviewQuality {preferences().lock().unwrap_or_else(std::sync::PoisonError::into_inner).preview_quality}
pub fn set_preview_quality(quality:super::preview_quality::PreviewQuality)->Result<(),String>{
 let path=settings_path().ok_or("找不到应用设置目录")?;
 let mut prefs=preferences().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
 let next=Preferences{preview_quality:quality,..prefs.clone()};
 if let Some(parent)=path.parent(){std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;}
 super::manifest_store::persist_json_atomically(&path,&next).map_err(|e|e.to_string())?;
 prefs.preview_quality=quality;Ok(())
}
