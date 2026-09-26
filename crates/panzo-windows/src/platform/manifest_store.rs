use panzo_core::project::ProjectManifest;
use panzo_core::project_io::{PROJECT_MANIFEST_FILE, RECORDING_LOCK_FILE, RecordingLock};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use thiserror::Error;
use windows::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};
use windows::core::PCWSTR;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestStore {
    project_root: PathBuf,
}

impl ManifestStore {
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            project_root: project_root.as_ref().into(),
        }
    }

    pub fn persist(&self, manifest: &ProjectManifest) -> Result<(), ManifestStoreError> {
        manifest.validate()?;
        self.persist_json(PROJECT_MANIFEST_FILE, "project.json.tmp", manifest)
    }

    pub fn persist_recording_lock(
        &self,
        recording_lock: &RecordingLock,
    ) -> Result<(), ManifestStoreError> {
        self.persist_json(RECORDING_LOCK_FILE, "recording.lock.tmp", recording_lock)
    }

    pub fn load(&self) -> Result<ProjectManifest, ManifestStoreError> {
        self.load_json(PROJECT_MANIFEST_FILE)
    }

    pub fn load_recording_lock(&self) -> Result<RecordingLock, ManifestStoreError> {
        self.load_json(RECORDING_LOCK_FILE)
    }

    pub fn remove_recording_lock(&self) -> Result<(), ManifestStoreError> {
        let path = self.project_root.join(RECORDING_LOCK_FILE);
        fs::remove_file(&path).map_err(|source| ManifestStoreError::Remove { path, source })
    }

    fn persist_json<T: Serialize>(
        &self,
        destination_name: &str,
        temporary_name: &str,
        value: &T,
    ) -> Result<(), ManifestStoreError> {
        let destination = self.project_root.join(destination_name);
        let temporary = self.project_root.join(temporary_name);
        persist_json_to_paths(&destination, &temporary, value)
    }

    fn load_json<T: DeserializeOwned>(&self, name: &str) -> Result<T, ManifestStoreError> {
        let path = self.project_root.join(name);
        let bytes = fs::read(&path).map_err(|source| ManifestStoreError::Read { path, source })?;
        serde_json::from_slice(&bytes).map_err(ManifestStoreError::Deserialize)
    }
}

/// Durably replaces an existing JSON file through a sibling temporary file and
/// `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`.
pub fn persist_json_atomically<T: Serialize>(
    destination: impl AsRef<Path>,
    value: &T,
) -> Result<(), ManifestStoreError> {
    let destination = destination.as_ref();
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ManifestStoreError::InvalidDestination(destination.into()))?;
    let temporary = destination.with_file_name(format!("{file_name}.tmp"));
    persist_json_to_paths(destination, &temporary, value)
}

/// Durably replaces a file with already-serialized bytes through a sibling temporary file.
pub fn persist_bytes_atomically(
    destination: impl AsRef<Path>,
    bytes: &[u8],
) -> Result<(), ManifestStoreError> {
    let destination = destination.as_ref();
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ManifestStoreError::InvalidDestination(destination.into()))?;
    let temporary = destination.with_file_name(format!("{file_name}.tmp"));
    persist_bytes_to_paths(destination, &temporary, bytes, true)
}

/// Durably creates a file without replacing an existing destination.
pub fn persist_bytes_new_atomically(
    destination: impl AsRef<Path>,
    bytes: &[u8],
) -> Result<(), ManifestStoreError> {
    let destination = destination.as_ref();
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ManifestStoreError::InvalidDestination(destination.into()))?;
    let temporary = destination.with_file_name(format!("{file_name}.tmp"));
    persist_bytes_to_paths(destination, &temporary, bytes, false)
}

/// Durably renames a file, replacing the destination when it already exists.
pub fn move_file_atomically(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> Result<(), ManifestStoreError> {
    move_file_with_policy(source.as_ref(), destination.as_ref(), true)
}

/// Publishes finalized media without replacing a file created by another operation.
pub fn move_file_new_atomically(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> Result<(), ManifestStoreError> {
    move_file_with_policy(source.as_ref(), destination.as_ref(), false)
}

fn move_file_with_policy(
    source: &Path,
    destination: &Path,
    replace_existing: bool,
) -> Result<(), ManifestStoreError> {
    let source_wide = wide_path(source);
    let destination_wide = wide_path(destination);
    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            if replace_existing {
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH
            } else {
                MOVEFILE_WRITE_THROUGH
            },
        )
    }
    .map_err(|source_error| ManifestStoreError::Commit {
        from: source.into(),
        to: destination.into(),
        source: source_error,
    })
}

fn persist_json_to_paths<T: Serialize>(
    destination: &Path,
    temporary: &Path,
    value: &T,
) -> Result<(), ManifestStoreError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(ManifestStoreError::Serialize)?;
    bytes.push(b'\n');

    persist_bytes_to_paths(destination, temporary, &bytes, true)
}

fn persist_bytes_to_paths(
    destination: &Path,
    temporary: &Path,
    bytes: &[u8],
    replace_existing: bool,
) -> Result<(), ManifestStoreError> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(temporary)
        .map_err(|source| ManifestStoreError::OpenTemporary {
            path: temporary.into(),
            source,
        })?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|source| ManifestStoreError::WriteTemporary {
            path: temporary.into(),
            source,
        })?;
    drop(file);

    let temporary_wide = wide_path(temporary);
    let destination_wide = wide_path(destination);
    let flags = if replace_existing {
        MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH
    } else {
        MOVEFILE_WRITE_THROUGH
    };
    let commit = unsafe {
        MoveFileExW(
            PCWSTR(temporary_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            flags,
        )
    };
    if let Err(source) = commit {
        let _ = fs::remove_file(temporary);
        return Err(ManifestStoreError::Commit {
            from: temporary.into(),
            to: destination.into(),
            source,
        });
    }
    Ok(())
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[derive(Debug, Error)]
pub enum ManifestStoreError {
    #[error(transparent)]
    InvalidManifest(#[from] panzo_core::project::ProjectValidationError),
    #[error("could not serialize project metadata: {0}")]
    Serialize(serde_json::Error),
    #[error("invalid atomic JSON destination: {0}")]
    InvalidDestination(PathBuf),
    #[error("could not deserialize project metadata: {0}")]
    Deserialize(serde_json::Error),
    #[error("could not open temporary manifest {path}: {source}")]
    OpenTemporary {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not write temporary manifest {path}: {source}")]
    WriteTemporary {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not atomically replace {to} with {from}: {source}")]
    Commit {
        from: PathBuf,
        to: PathBuf,
        source: windows::core::Error,
    },
    #[error("could not read manifest {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not remove recording lock {path}: {source}")]
    Remove {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use panzo_core::geometry::{PhysicalRect, PhysicalSize};
    use panzo_core::project::{CaptureDescriptor, CaptureKind, ProjectState, TimebaseDescriptor};
    use uuid::Uuid;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("panzo-manifest-{}", Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn atomically_replaces_existing_manifest() {
        let directory = TestDirectory::new();
        let store = ManifestStore::new(&directory.0);
        let mut manifest = manifest();
        store.persist(&manifest).unwrap();
        manifest.transition_to(ProjectState::Finalizing).unwrap();
        store.persist(&manifest).unwrap();

        assert_eq!(store.load().unwrap(), manifest);
        assert!(!directory.0.join("project.json.tmp").exists());
    }

    #[test]
    fn atomically_updates_and_removes_recording_lock() {
        let directory = TestDirectory::new();
        let store = ManifestStore::new(&directory.0);
        let mut recording_lock = RecordingLock {
            schema_version: 1,
            process_id: 42,
            session_id: "session-1".into(),
            last_checkpoint_tick: panzo_core::TimeTick::ZERO,
        };
        store.persist_recording_lock(&recording_lock).unwrap();
        recording_lock.last_checkpoint_tick = panzo_core::TimeTick(20_000_000);
        store.persist_recording_lock(&recording_lock).unwrap();
        assert_eq!(store.load_recording_lock().unwrap(), recording_lock);
        store.remove_recording_lock().unwrap();
        assert!(!directory.0.join(RECORDING_LOCK_FILE).exists());
    }

    #[test]
    fn atomic_create_never_replaces_an_existing_file_or_leaves_a_temporary() {
        let directory = TestDirectory::new();
        let destination = directory.0.join("immutable.json");
        persist_bytes_new_atomically(&destination, b"old").unwrap();
        assert!(persist_bytes_new_atomically(&destination, b"new").is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"old");
        assert!(!directory.0.join("immutable.json.tmp").exists());
    }

    #[test]
    fn finalized_media_publication_never_replaces_an_existing_file() {
        let directory = TestDirectory::new();
        let source = directory.0.join("screen.part.mp4");
        let destination = directory.0.join("screen.mp4");
        fs::write(&source, b"recording").unwrap();
        fs::write(&destination, b"previous-video").unwrap();
        assert!(move_file_new_atomically(&source, &destination).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"recording");
        assert_eq!(fs::read(&destination).unwrap(), b"previous-video");
        fs::remove_file(&destination).unwrap();
        move_file_new_atomically(&source, &destination).unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&destination).unwrap(), b"recording");
    }

    fn manifest() -> ProjectManifest {
        ProjectManifest::new_v01(
            Uuid::nil(),
            "0.1.0",
            "2026-08-31T08:00:00Z",
            TimebaseDescriptor {
                ticks_per_second: panzo_core::TICKS_PER_SECOND,
                session_start_qpc: 10,
                qpc_frequency: 10_000_000,
            },
            CaptureDescriptor {
                kind: CaptureKind::Monitor,
                monitor_id: "primary".into(),
                window_id: None,
                window_title: None,
                desktop_rect_px: PhysicalRect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                content_size_px: PhysicalSize {
                    width: 1920,
                    height: 1080,
                },
                target_fps: 60,
                pixel_format: "BGRA8".into(),
                color_mode: "sdr-srgb".into(),
                cursor_captured_in_video: false,
            },
        )
    }
}
