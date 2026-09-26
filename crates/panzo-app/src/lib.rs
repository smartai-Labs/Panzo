use std::path::PathBuf;

pub fn default_project_library() -> PathBuf {
    std::env::var_os("USERPROFILE").map_or_else(
        || PathBuf::from(".tmp/recordings"),
        |profile| PathBuf::from(profile).join("Videos").join("Panzo"),
    )
}
