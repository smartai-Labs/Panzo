use std::path::PathBuf;

pub fn default_project_library() -> PathBuf {
    if let Some(path) = panzo_windows::platform::ui_preferences::library_directory() {
        return path;
    }
    std::env::var_os("USERPROFILE").map_or_else(
        || PathBuf::from(".tmp/recordings"),
        |profile| PathBuf::from(profile).join("Videos").join("Panzo"),
    )
}
