use crate::project::ProjectManifest;
use crate::time::TimeTick;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const PROJECT_MANIFEST_FILE: &str = "project.json";
pub const RECORDING_LOCK_FILE: &str = "recording.lock";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingLock {
    pub schema_version: u32,
    pub process_id: u32,
    pub session_id: String,
    pub last_checkpoint_tick: TimeTick,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLayout {
    root: PathBuf,
}

impl ProjectLayout {
    pub fn create_new(
        root: impl AsRef<Path>,
        manifest: &ProjectManifest,
        recording_lock: &RecordingLock,
    ) -> Result<Self, ProjectIoError> {
        manifest.validate()?;
        let root = root.as_ref();
        fs::create_dir(root).map_err(|source| ProjectIoError::CreateRoot {
            path: root.into(),
            source,
        })?;

        let layout = Self { root: root.into() };
        for path in [
            layout.media_dir(),
            layout.events_dir(),
            layout.tracks_dir(),
            layout.diagnostics_dir(),
        ] {
            fs::create_dir(&path)
                .map_err(|source| ProjectIoError::CreateDirectory { path, source })?;
        }

        write_new_json_atomically(&layout.manifest_path(), manifest)?;
        write_new_json_atomically(&layout.recording_lock_path(), recording_lock)?;
        Ok(layout)
    }

    pub fn open(root: impl AsRef<Path>) -> Result<Self, ProjectIoError> {
        let layout = Self {
            root: root.as_ref().into(),
        };
        let manifest = layout.load_manifest()?;
        manifest.validate()?;
        Ok(layout)
    }

    pub fn load_manifest(&self) -> Result<ProjectManifest, ProjectIoError> {
        let path = self.manifest_path();
        let bytes = fs::read(&path).map_err(|source| ProjectIoError::Read { path, source })?;
        serde_json::from_slice(&bytes).map_err(ProjectIoError::Deserialize)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root.join(PROJECT_MANIFEST_FILE)
    }

    pub fn recording_lock_path(&self) -> PathBuf {
        self.root.join(RECORDING_LOCK_FILE)
    }

    pub fn media_dir(&self) -> PathBuf {
        self.root.join("media")
    }

    pub fn events_dir(&self) -> PathBuf {
        self.root.join("events")
    }

    pub fn tracks_dir(&self) -> PathBuf {
        self.root.join("tracks")
    }

    pub fn diagnostics_dir(&self) -> PathBuf {
        self.root.join("diagnostics")
    }

    pub fn journal_path(&self) -> PathBuf {
        self.root.join("journal.jsonl")
    }

    pub fn cursor_events_path(&self) -> PathBuf {
        self.events_dir().join("cursor.jsonl")
    }

    pub fn click_events_path(&self) -> PathBuf {
        self.events_dir().join("clicks.jsonl")
    }

    pub fn capture_frames_path(&self) -> PathBuf {
        self.diagnostics_dir().join("capture_frames.jsonl")
    }

    pub fn metrics_path(&self) -> PathBuf {
        self.diagnostics_dir().join("metrics.json")
    }

    pub fn session_log_path(&self) -> PathBuf {
        self.diagnostics_dir().join("session.log")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonlRecoveryReport {
    pub original_bytes: u64,
    pub retained_bytes: u64,
    pub truncated_bytes: u64,
    pub retained_records: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonlTrimReport {
    pub retained_records: usize,
    pub removed_records: usize,
    pub retained_bytes: u64,
    pub removed_bytes: u64,
}

pub fn recover_jsonl_tail(path: impl AsRef<Path>) -> Result<JsonlRecoveryReport, ProjectIoError> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|source| ProjectIoError::Read {
        path: path.into(),
        source,
    })?;
    let retained_len = if bytes.last() == Some(&b'\n') {
        bytes.len()
    } else {
        bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1)
    };

    let complete = &bytes[..retained_len];
    let mut retained_records = 0;
    for (index, line) in complete.split(|byte| *byte == b'\n').enumerate() {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        serde_json::from_slice::<serde_json::Value>(line).map_err(|source| {
            ProjectIoError::InvalidJsonlRecord {
                line: index + 1,
                source,
            }
        })?;
        retained_records += 1;
    }

    if retained_len < bytes.len() {
        let file = OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|source| ProjectIoError::OpenForRecovery {
                path: path.into(),
                source,
            })?;
        file.set_len(retained_len as u64)
            .map_err(|source| ProjectIoError::Truncate {
                path: path.into(),
                source,
            })?;
        file.sync_data().map_err(|source| ProjectIoError::Sync {
            path: path.into(),
            source,
        })?;
    }

    Ok(JsonlRecoveryReport {
        original_bytes: bytes.len() as u64,
        retained_bytes: retained_len as u64,
        truncated_bytes: (bytes.len() - retained_len) as u64,
        retained_records,
    })
}

pub fn trim_jsonl_after_tick(
    path: impl AsRef<Path>,
    maximum_tick: TimeTick,
) -> Result<JsonlTrimReport, ProjectIoError> {
    let path = path.as_ref();
    recover_jsonl_tail(path)?;
    let bytes = fs::read(path).map_err(|source| ProjectIoError::Read {
        path: path.into(),
        source,
    })?;
    let mut retained_bytes = 0usize;
    let mut retained_records = 0usize;
    let mut removed_records = 0usize;
    let mut previous_tick = None;

    for (index, line) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
        let without_newline = line.strip_suffix(b"\n").unwrap_or(line);
        let json = without_newline
            .strip_suffix(b"\r")
            .unwrap_or(without_newline);
        if json.is_empty() {
            retained_bytes += line.len();
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_slice(json).map_err(|source| ProjectIoError::InvalidJsonlRecord {
                line: index + 1,
                source,
            })?;
        let tick = value
            .get("timeTick")
            .and_then(serde_json::Value::as_i64)
            .ok_or(ProjectIoError::MissingJsonlTimeTick { line: index + 1 })?;
        if let Some(previous) = previous_tick
            && tick < previous
        {
            return Err(ProjectIoError::NonMonotonicJsonlTimeTick {
                line: index + 1,
                previous,
                current: tick,
            });
        }
        previous_tick = Some(tick);
        if tick <= maximum_tick.0 {
            retained_bytes += line.len();
            retained_records += 1;
        } else {
            removed_records += 1;
        }
    }

    if retained_bytes < bytes.len() {
        let file = OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|source| ProjectIoError::OpenForRecovery {
                path: path.into(),
                source,
            })?;
        file.set_len(retained_bytes as u64)
            .map_err(|source| ProjectIoError::Truncate {
                path: path.into(),
                source,
            })?;
        file.sync_data().map_err(|source| ProjectIoError::Sync {
            path: path.into(),
            source,
        })?;
    }

    Ok(JsonlTrimReport {
        retained_records,
        removed_records,
        retained_bytes: retained_bytes as u64,
        removed_bytes: (bytes.len() - retained_bytes) as u64,
    })
}

fn write_new_json_atomically<T: Serialize>(
    destination: &Path,
    value: &T,
) -> Result<(), ProjectIoError> {
    let temp_path = destination.with_extension("tmp");
    let mut json = serde_json::to_vec_pretty(value).map_err(ProjectIoError::Serialize)?;
    json.push(b'\n');

    let mut temp = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|source| ProjectIoError::CreateTemp {
            path: temp_path.clone(),
            source,
        })?;
    temp.write_all(&json)
        .and_then(|()| temp.sync_all())
        .map_err(|source| ProjectIoError::WriteTemp {
            path: temp_path.clone(),
            source,
        })?;
    drop(temp);

    fs::rename(&temp_path, destination).map_err(|source| ProjectIoError::Commit {
        from: temp_path,
        to: destination.into(),
        source,
    })
}

#[derive(Debug, Error)]
pub enum ProjectIoError {
    #[error(transparent)]
    InvalidManifest(#[from] crate::project::ProjectValidationError),
    #[error("could not create project root {path}: {source}")]
    CreateRoot {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not create project directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not open {path} for recovery: {source}")]
    OpenForRecovery {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not truncate {path}: {source}")]
    Truncate {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not sync {path}: {source}")]
    Sync {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("JSONL record at line {line} is invalid: {source}")]
    InvalidJsonlRecord {
        line: usize,
        source: serde_json::Error,
    },
    #[error("JSONL record at line {line} has no integer timeTick")]
    MissingJsonlTimeTick { line: usize },
    #[error("JSONL timeTick moved backwards at line {line} from {previous} to {current}")]
    NonMonotonicJsonlTimeTick {
        line: usize,
        previous: i64,
        current: i64,
    },
    #[error("could not serialize project metadata: {0}")]
    Serialize(serde_json::Error),
    #[error("could not deserialize project manifest: {0}")]
    Deserialize(serde_json::Error),
    #[error("could not create temporary file {path}: {source}")]
    CreateTemp {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not write temporary file {path}: {source}")]
    WriteTemp {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not atomically commit {from} to {to}: {source}")]
    Commit {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{PhysicalRect, PhysicalSize};
    use crate::project::{
        CaptureDescriptor, CaptureKind, PROJECT_SCHEMA_VERSION, ProjectState,
        ScreenMediaDescriptor, TimebaseDescriptor, TrackPaths,
    };
    use uuid::Uuid;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            Self(std::env::temp_dir().join(format!("panzo-{name}-{}", Uuid::new_v4())))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn creates_and_opens_project_layout() {
        let directory = TestDirectory::new("project-layout");
        let manifest = manifest();
        let lock = RecordingLock {
            schema_version: 1,
            process_id: 42,
            session_id: "session-1".into(),
            last_checkpoint_tick: TimeTick::ZERO,
        };

        let layout = ProjectLayout::create_new(&directory.0, &manifest, &lock).unwrap();
        assert!(layout.media_dir().is_dir());
        assert!(layout.events_dir().is_dir());
        assert!(layout.tracks_dir().is_dir());
        assert!(layout.diagnostics_dir().is_dir());
        assert_eq!(ProjectLayout::open(&directory.0).unwrap(), layout);
    }

    #[test]
    fn truncates_only_incomplete_jsonl_tail() {
        let directory = TestDirectory::new("jsonl-recovery");
        fs::create_dir(&directory.0).unwrap();
        let path = directory.0.join("clicks.jsonl");
        fs::write(&path, b"{\"seq\":1}\n{\"seq\":2}\n{\"seq\":").unwrap();

        let report = recover_jsonl_tail(&path).unwrap();
        assert_eq!(report.retained_records, 2);
        assert_eq!(report.truncated_bytes, 7);
        assert_eq!(fs::read(path).unwrap(), b"{\"seq\":1}\n{\"seq\":2}\n");
    }

    #[test]
    fn rejects_invalid_complete_jsonl_record_without_modifying_file() {
        let directory = TestDirectory::new("jsonl-invalid");
        fs::create_dir(&directory.0).unwrap();
        let path = directory.0.join("clicks.jsonl");
        let original = b"{\"seq\":1}\nnot-json\npartial";
        fs::write(&path, original).unwrap();

        assert!(matches!(
            recover_jsonl_tail(&path),
            Err(ProjectIoError::InvalidJsonlRecord { line: 2, .. })
        ));
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn trims_only_records_after_maximum_tick() {
        let directory = TestDirectory::new("jsonl-trim");
        fs::create_dir(&directory.0).unwrap();
        let path = directory.0.join("cursor.jsonl");
        fs::write(
            &path,
            b"{\"timeTick\":1}\n{\"timeTick\":2}\n{\"timeTick\":3}\n",
        )
        .unwrap();

        let report = trim_jsonl_after_tick(&path, TimeTick(2)).unwrap();
        assert_eq!(report.retained_records, 2);
        assert_eq!(report.removed_records, 1);
        assert_eq!(
            fs::read(path).unwrap(),
            b"{\"timeTick\":1}\n{\"timeTick\":2}\n"
        );
    }

    fn manifest() -> ProjectManifest {
        ProjectManifest {
            schema_version: PROJECT_SCHEMA_VERSION,
            project_id: Uuid::nil(),
            app_version: "0.1.0".into(),
            state: ProjectState::Recording,
            created_at_utc: "2026-08-31T08:00:00Z".into(),
            timebase: TimebaseDescriptor {
                ticks_per_second: crate::time::TICKS_PER_SECOND,
                session_start_qpc: 10,
                qpc_frequency: 10_000_000,
            },
            capture: CaptureDescriptor {
                kind: CaptureKind::Monitor,
                monitor_id: "test-monitor".into(),
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
            media: ScreenMediaDescriptor {
                screen: "media/screen.part.mp4".into(),
                codec: "h264".into(),
                duration_tick: 0,
            },
            tracks: TrackPaths {
                cursor: "events/cursor.jsonl".into(),
                clicks: "events/clicks.jsonl".into(),
                camera: "tracks/camera.json".into(),
            },
        }
    }
}
