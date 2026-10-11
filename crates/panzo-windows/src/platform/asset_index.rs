//! Local asset discovery plus durable references to files opened or exported elsewhere.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
    time::UNIX_EPOCH,
};
static INDEX_LOCK: Mutex<()> = Mutex::new(());
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Kind {
    Image,
    Gif,
    Video,
    Audio,
    Project,
}
impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Image => "图片",
            Self::Gif => "GIF 动图",
            Self::Audio => "声音",
            Self::Video => "视频",
            Self::Project => "视频工程",
        }
    }
    pub fn is_image(self) -> bool {
        matches!(self, Self::Image | Self::Gif)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Asset {
    pub path: PathBuf,
    pub kind: Kind,
    pub modified: u128,
    pub bytes: u64,
    pub missing: bool,
}
#[derive(Default, Serialize, Deserialize)]
struct Index {
    #[serde(default)]
    paths: Vec<PathBuf>,
    #[serde(default)]
    excluded: Vec<PathBuf>,
}
pub(crate) fn kind(path: &Path) -> Option<Kind> {
    // These are in-progress encoder files, never completed assets.
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".panzo-part.mp4") || name.ends_with(".panzo-part.encoding.mp4") {
        return None;
    }
    if path.is_dir() && path.join("project.json").is_file() {
        return Some(Kind::Project);
    }
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "bmp" => Some(Kind::Image),
        "gif" => Some(Kind::Gif),
        "mp4" => Some(Kind::Video),
        "wav" | "mp3" | "m4a" | "aac" | "flac" => Some(Kind::Audio),
        "panzo" => Some(Kind::Project),
        _ => None,
    }
}
fn key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}
fn index_path(library: &Path) -> PathBuf {
    library.join(".panzo-assets.json")
}
fn read_index(library: &Path) -> Index {
    std::fs::read(index_path(library))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}
/// Registration stores a reference; source and exported files are never copied or moved.
pub(crate) fn register(library: &Path, path: &Path) -> Result<(), String> {
    let Some(kind) = kind(path) else {
        return Ok(());
    };
    let exists = if kind == Kind::Project {
        path.is_dir() && path.join("project.json").is_file()
    } else {
        path.is_file()
    };
    if !exists {
        return Err("素材文件尚未完成或已不存在".into());
    }
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    let _guard = INDEX_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut index = read_index(library);
    index.excluded.retain(|p| key(p) != key(&path));
    if index.paths.iter().any(|p| key(p) == key(&path)) {
        return Ok(());
    }
    index.paths.push(path);
    std::fs::create_dir_all(library).map_err(|e| e.to_string())?;
    super::manifest_store::persist_json_atomically(index_path(library), &index)
        .map_err(|e| e.to_string())
}
/// Relocate an indexed reference without modifying either source file or project media.
pub(crate) fn relocate(library: &Path, old: &Path, new: &Path) -> Result<(), String> {
    let expected = kind(old).ok_or("未知素材类型")?;
    if kind(new) != Some(expected) || !new.exists() {
        return Err("请选择同类型且存在的素材".into());
    }
    let _guard = INDEX_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut index = read_index(library);
    index
        .paths
        .retain(|p| key(p) != key(old) && key(p) != key(new));
    index.excluded.retain(|p| key(p) != key(new));
    index.paths.push(new.to_path_buf());
    std::fs::create_dir_all(library).map_err(|e| e.to_string())?;
    super::manifest_store::persist_json_atomically(index_path(library), &index)
        .map_err(|e| e.to_string())
}

/// Remove a library record without touching the source; exclusions survive rescans.
pub(crate) fn forget(library: &Path, path: &Path) -> Result<(), String> {
    let _guard = INDEX_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut index = read_index(library);
    index.paths.retain(|p| key(p) != key(path));
    if !index.excluded.iter().any(|p| key(p) == key(path)) {
        index.excluded.push(path.to_path_buf());
    }
    std::fs::create_dir_all(library).map_err(|e| e.to_string())?;
    super::manifest_store::persist_json_atomically(index_path(library), &index)
        .map_err(|e| e.to_string())
}
fn asset(path: PathBuf) -> Option<Asset> {
    let kind = kind(&path)?;
    let metadata = std::fs::metadata(if kind == Kind::Project {
        path.join("project.json")
    } else {
        path.clone()
    })
    .ok();
    Some(Asset {
        path,
        kind,
        modified: metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |t| t.as_millis()),
        bytes: metadata.as_ref().map_or(0, std::fs::Metadata::len),
        missing: metadata.is_none(),
    })
}
/// Recursion stops at project roots. Internal media, recovery data, and junctions are not assets.
pub(crate) fn scan(library: &Path) -> Vec<Asset> {
    use std::os::windows::fs::MetadataExt;
    let mut found = HashMap::new();
    let mut pending = vec![(library.to_path_buf(), 0)];
    while let Some((folder, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.file_attributes() & 0x400 != 0 {
                continue;
            }
            if path.is_dir() && !path.join("project.json").is_file() {
                if depth < 32 {
                    pending.push((path, depth + 1));
                }
            } else if let Some(item) = asset(path) {
                found.insert(key(&item.path), item);
            }
        }
    }
    let index = read_index(library);
    for path in index.paths {
        if let Some(item) = asset(path) {
            found.insert(key(&item.path), item);
        }
    }
    for path in index.excluded {
        found.remove(&key(&path));
    }
    let mut result = found.into_values().collect::<Vec<_>>();
    result.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| key(&a.path).cmp(&key(&b.path)))
    });
    result
}
pub(crate) fn video_source(asset: &Asset) -> Result<PathBuf, String> {
    if asset.kind != Kind::Project {
        return Ok(asset.path.clone());
    }
    let layout = panzo_core::ProjectLayout::open(&asset.path).map_err(|e| e.to_string())?;
    let manifest = layout.load_manifest().map_err(|e| e.to_string())?;
    let source = asset.path.join(manifest.media.screen);
    if !source.is_file() {
        return Err("工程的视频文件已移动或不存在".into());
    }
    Ok(source)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        let p = std::env::temp_dir().join(format!("panzo-asset-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn removing_record_preserves_file_and_survives_rescan_and_reimport() {
        let p = root();
        let file = p.join("kept.png");
        std::fs::write(&file, b"unchanged").unwrap();
        assert_eq!(scan(&p).len(), 1);
        forget(&p, &file).unwrap();
        assert!(scan(&p).is_empty());
        assert_eq!(std::fs::read(&file).unwrap(), b"unchanged");
        register(&p, &file).unwrap();
        assert_eq!(scan(&p).len(), 1);
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn scans_nested_outputs_and_skips_project_internals() {
        let p = root();
        let batch = p.join("批处理/输出");
        let project = p.join("录制.panzo");
        std::fs::create_dir_all(&batch).unwrap();
        std::fs::create_dir_all(project.join("media")).unwrap();
        std::fs::write(batch.join("照片.JPG"), []).unwrap();
        std::fs::write(batch.join("导出.gif"), []).unwrap();
        std::fs::write(project.join("project.json"), "{}").unwrap();
        std::fs::write(project.join("media/screen.mp4"), []).unwrap();
        let assets = scan(&p);
        assert_eq!(assets.len(), 3);
        assert!(assets.iter().any(|a| a.kind == Kind::Project));
        assert!(!assets.iter().any(|a| a.path.ends_with("screen.mp4")));
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn external_registration_is_deduplicated_and_missing_files_remain_visible() {
        let p = root();
        let outside = root();
        let file = outside.join("original.png");
        std::fs::write(&file, []).unwrap();
        register(&p, &file).unwrap();
        register(&p, &file).unwrap();
        assert_eq!(scan(&p).len(), 1);
        std::fs::remove_file(&file).unwrap();
        assert!(scan(&p)[0].missing);
        std::fs::remove_dir_all(p).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }
    #[test]
    fn registration_requires_completed_files_and_discovery_skips_encoder_temporaries() {
        let p = root();
        assert!(register(&p, &p.join("missing.mp4")).is_err());
        assert!(!index_path(&p).exists());
        for name in [
            "export.mp4.run.panzo-part.mp4",
            "export.mp4.run.panzo-part.encoding.mp4",
        ] {
            let temporary = p.join(name);
            std::fs::write(&temporary, []).unwrap();
            register(&p, &temporary).unwrap();
        }
        assert!(scan(&p).is_empty());
        assert!(!index_path(&p).exists());
        let complete = p.join("export.mp4");
        std::fs::write(&complete, []).unwrap();
        register(&p, &complete).unwrap();
        assert_eq!(scan(&p).len(), 1);
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn relocating_missing_reference_keeps_source_and_rejects_wrong_type() {
        let library = root();
        let outside = root();
        let old = outside.join("old.png");
        let new = outside.join("new.png");
        let wrong = outside.join("wrong.wav");
        std::fs::write(&old, b"source").unwrap();
        register(&library, &old).unwrap();
        std::fs::rename(&old, &new).unwrap();
        std::fs::write(&wrong, []).unwrap();
        assert!(relocate(&library, &old, &wrong).is_err());
        assert!(scan(&library)[0].missing);
        relocate(&library, &old, &new).unwrap();
        let found = scan(&library);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, new);
        assert!(!found[0].missing);
        assert_eq!(std::fs::read(&new).unwrap(), b"source");
        std::fs::remove_dir_all(library).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }
    #[test]
    fn discovery_has_no_legacy_500_item_limit() {
        let p = root();
        for i in 0..505 {
            std::fs::write(p.join(format!("{i}.png")), []).unwrap();
        }
        assert_eq!(scan(&p).len(), 505);
        std::fs::remove_dir_all(p).unwrap();
    }
}
