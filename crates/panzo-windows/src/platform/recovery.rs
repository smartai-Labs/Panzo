use crate::platform::editor_transaction::{
    lock_project_edit, recover_pending_editor_transaction_locked,
};
use crate::platform::fmp4_writer::{Fmp4ProbeError, inspect_fmp4};
use crate::platform::manifest_store::{ManifestStore, ManifestStoreError};
use crate::platform::recorder::{RecorderError, ensure_recording_edit_files};
use panzo_core::{
    JournalEntry, JournalOperation, JournalResult, ProjectIoError, ProjectLayout, ProjectState,
    TimeTick, recover_jsonl_tail, trim_jsonl_after_tick,
};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use thiserror::Error;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_PARAMETER, GetLastError, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};

pub const RECOVERY_EVENT_TOLERANCE_TICK: i64 = 166_667;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryDisposition {
    Active,
    Recovered,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryProjectReport {
    pub project_root: PathBuf,
    pub process_id: u32,
    pub disposition: RecoveryDisposition,
    pub fragment_count: usize,
    pub last_video_end_tick: i64,
    pub cursor_records_removed: usize,
    pub click_records_removed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryScanReport {
    pub library_root: PathBuf,
    pub projects: Vec<RecoveryProjectReport>,
}

impl RecoveryScanReport {
    pub fn recovered_count(&self) -> usize {
        self.projects
            .iter()
            .filter(|report| report.disposition == RecoveryDisposition::Recovered)
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.projects
            .iter()
            .filter(|report| report.disposition == RecoveryDisposition::Failed)
            .count()
    }
}

pub struct RecoveryScanner {
    library_root: PathBuf,
}

impl RecoveryScanner {
    pub fn new(library_root: impl AsRef<Path>) -> Self {
        Self {
            library_root: library_root.as_ref().into(),
        }
    }

    pub fn scan_and_recover(&self) -> Result<RecoveryScanReport, RecoveryError> {
        let mut roots = Vec::new();
        if needs_recovery(&self.library_root) {
            roots.push(self.library_root.clone());
        }
        for entry in fs::read_dir(&self.library_root).map_err(|source| RecoveryError::Scan {
            path: self.library_root.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| RecoveryError::Scan {
                path: self.library_root.clone(),
                source,
            })?;
            let path = entry.path();
            if path.is_dir() && needs_recovery(&path) {
                roots.push(path);
            }
        }
        roots.sort();
        roots.dedup();

        let mut projects = Vec::with_capacity(roots.len());
        for root in roots {
            projects.push(Self::recover_or_report(&root));
        }
        Ok(RecoveryScanReport {
            library_root: self.library_root.clone(),
            projects,
        })
    }

    fn recover_or_report(project_root: &Path) -> RecoveryProjectReport {
        let store = ManifestStore::new(project_root);
        let process_id = if project_root.join("recording.lock").exists() {
            match store.load_recording_lock() {
                Ok(recording_lock) => recording_lock.process_id,
                Err(error) => return failed_report(project_root, 0, error.to_string()),
            }
        } else {
            0 // Older versions removed the marker before completing Processing.
        };
        if process_id != 0 {
            match process_is_alive(process_id) {
                Ok(true) => return active_report(project_root, process_id),
                Ok(false) => {}
                Err(error) => return failed_report(project_root, process_id, error.to_string()),
            }
        }
        // Serializes recovery with recording finalization, editor saves and other scans.
        let _lock = match lock_project_edit(project_root) {
            Ok(lock) => lock,
            Err(crate::platform::editor_transaction::EditorTransactionError::ProjectBusy {
                source,
                ..
            }) if matches!(source.raw_os_error(), Some(32 | 33)) => {
                return active_report(project_root, process_id);
            }
            Err(error) => return failed_report(project_root, process_id, error.to_string()),
        };
        if !project_root.join("recording.lock").exists() {
            // Legacy recorders removed their marker during Processing but kept
            // ProjectWriter's journal handle until finishing. An exclusive open
            // proves that such a writer is no longer alive before repairing it.
            match OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(project_root.join("journal.jsonl"))
            {
                Ok(file) => drop(file),
                Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => {
                    return active_report(project_root, process_id);
                }
                Err(error) => return failed_report(project_root, process_id, error.to_string()),
            }
        }
        match recover_project(project_root, &store) {
            Ok(mut report) => {
                report.process_id = process_id;
                report
            }
            Err(error) => {
                // A sharing violation, disk-full error or interrupted publication must
                // remain retryable. Never discard its marker or saved data on failure.
                failed_report(project_root, process_id, error.to_string())
            }
        }
    }
}

fn needs_recovery(root: &Path) -> bool {
    if root.join("recording.lock").is_file() {
        return true;
    }
    let Ok(layout) = ProjectLayout::open(root) else {
        return false;
    };
    let Ok(manifest) = layout.load_manifest() else {
        return false;
    };
    matches!(
        manifest.state,
        ProjectState::Processing | ProjectState::Recovering
    ) || (manifest.state == ProjectState::Recovered
        && (!root.join(&manifest.tracks.camera).is_file()
            || !root.join("edit/workbench.json").is_file()))
}

fn active_report(project_root: &Path, process_id: u32) -> RecoveryProjectReport {
    RecoveryProjectReport {
        project_root: project_root.into(),
        process_id,
        disposition: RecoveryDisposition::Active,
        fragment_count: 0,
        last_video_end_tick: 0,
        cursor_records_removed: 0,
        click_records_removed: 0,
        error: None,
    }
}

fn recover_project(
    project_root: &Path,
    store: &ManifestStore,
) -> Result<RecoveryProjectReport, RecoveryError> {
    recover_project_with_hook(project_root, store, |_| Ok(()))
}

fn recover_project_with_hook(
    project_root: &Path,
    store: &ManifestStore,
    mut hook: impl FnMut(&str) -> Result<(), RecoveryError>,
) -> Result<RecoveryProjectReport, RecoveryError> {
    let layout = ProjectLayout::open(project_root)?;
    recover_pending_editor_transaction_locked(project_root)?;
    let mut manifest = store.load()?;
    let completed = matches!(
        manifest.state,
        ProjectState::Ready | ProjectState::Recovered
    );
    let media_path = resolve_recording_media(&layout, &mut manifest)?;
    if !completed && manifest.state != ProjectState::Recovering {
        manifest.transition_to(ProjectState::Recovering)?;
    }
    store.persist(&manifest)?;
    hook("manifest-resolved")?;

    recover_jsonl_tail(layout.cursor_events_path())?;
    recover_jsonl_tail(layout.click_events_path())?;
    recover_jsonl_tail(layout.journal_path())?;
    append_recovery_journal(
        &layout,
        TimeTick::ZERO,
        JournalOperation::RecoveryStarted,
        JournalResult::Success,
        None,
    )?;

    let inspection = inspect_fmp4(&media_path)?;
    if inspection.trailing_bytes > 0 {
        if completed || manifest.media.screen != "media/screen.part.mp4" {
            return Err(RecoveryError::IncompleteFinalMedia(media_path));
        }
        let file = OpenOptions::new()
            .write(true)
            .open(&media_path)
            .map_err(|source| RecoveryError::MediaTruncate {
                path: media_path.clone(),
                source,
            })?;
        file.set_len(inspection.retained_bytes)
            .and_then(|()| file.sync_data())
            .map_err(|source| RecoveryError::MediaTruncate {
                path: media_path.clone(),
                source,
            })?;
    }
    hook("media-checked")?;

    let maximum_event_tick = TimeTick(
        inspection
            .last_video_end_tick
            .saturating_add(RECOVERY_EVENT_TOLERANCE_TICK),
    );
    let cursor_trim = trim_jsonl_after_tick(layout.cursor_events_path(), maximum_event_tick)?;
    let click_trim = trim_jsonl_after_tick(layout.click_events_path(), maximum_event_tick)?;
    hook("events-trimmed")?;
    manifest.media.duration_tick = inspection.last_video_end_tick;
    let clicks = read_recovery_clicks(&layout.click_events_path())?;
    ensure_recording_edit_files(
        &layout,
        &clicks,
        TimeTick(inspection.last_video_end_tick),
        manifest.capture.desktop_rect_px,
    )?;
    hook("edit-files-ready")?;
    append_recovery_journal(
        &layout,
        TimeTick(inspection.last_video_end_tick),
        JournalOperation::RecoveryCompleted,
        JournalResult::Success,
        Some(format!(
            "fragments={}, cursorRemoved={}, clickRemoved={}",
            inspection.fragment_count, cursor_trim.removed_records, click_trim.removed_records
        )),
    )?;
    if !completed {
        manifest.transition_to(ProjectState::Recovered)?;
    }
    store.persist(&manifest)?;
    hook("completion-committed")?;
    if layout.recording_lock_path().exists() {
        store.remove_recording_lock()?;
    }
    hook("marker-removed")?;

    Ok(RecoveryProjectReport {
        project_root: project_root.into(),
        process_id: 0,
        disposition: RecoveryDisposition::Recovered,
        fragment_count: inspection.fragment_count,
        last_video_end_tick: inspection.last_video_end_tick,
        cursor_records_removed: cursor_trim.removed_records,
        click_records_removed: click_trim.removed_records,
        error: None,
    })
}

fn resolve_recording_media(
    layout: &ProjectLayout,
    manifest: &mut panzo_core::ProjectManifest,
) -> Result<PathBuf, RecoveryError> {
    let part = layout.root().join("media/screen.part.mp4");
    let final_path = layout.root().join("media/screen.mp4");
    if matches!(
        manifest.media.screen.as_str(),
        "media/screen.part.mp4" | "media/screen.mp4"
    ) {
        if part.exists() && final_path.exists() {
            return Err(RecoveryError::ConflictingMedia);
        }
        if manifest.media.screen == "media/screen.part.mp4"
            && !part.exists()
            && final_path.is_file()
        {
            manifest.media.screen = "media/screen.mp4".into();
        }
    }
    let path = layout.root().join(&manifest.media.screen);
    let root = layout
        .root()
        .canonicalize()
        .map_err(|source| RecoveryError::MediaTruncate {
            path: layout.root().into(),
            source,
        })?;
    let resolved = path
        .canonicalize()
        .map_err(|source| RecoveryError::MediaTruncate {
            path: path.clone(),
            source,
        })?;
    if !resolved.starts_with(root) {
        return Err(RecoveryError::MediaOutsideProject(path));
    }
    Ok(path)
}

fn read_recovery_clicks(path: &Path) -> Result<Vec<panzo_core::ClickEvent>, RecoveryError> {
    use std::io::BufRead;
    let file = fs::File::open(path).map_err(|source| RecoveryError::Journal {
        path: path.into(),
        source,
    })?;
    let mut clicks = Vec::new();
    for (index, line) in std::io::BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|source| RecoveryError::Journal {
            path: path.into(),
            source,
        })?;
        if !line.trim().is_empty() {
            clicks.push(serde_json::from_str(&line).map_err(|source| {
                RecoveryError::InvalidClickRecord {
                    path: path.into(),
                    line: index + 1,
                    source,
                }
            })?);
        }
    }
    Ok(clicks)
}

fn append_recovery_journal(
    layout: &ProjectLayout,
    time_tick: TimeTick,
    operation: JournalOperation,
    result: JournalResult,
    detail: Option<String>,
) -> Result<(), RecoveryError> {
    let path = layout.journal_path();
    let bytes = fs::read(&path).map_err(|source| RecoveryError::Journal {
        path: path.clone(),
        source,
    })?;
    let next_sequence = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_slice::<JournalEntry>(line).ok())
        .map(|entry| entry.seq)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let entry = JournalEntry::new(next_sequence, time_tick, operation, result, detail);
    let mut json = serde_json::to_vec(&entry).map_err(RecoveryError::Serialize)?;
    json.push(b'\n');
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .map_err(|source| RecoveryError::Journal {
            path: path.clone(),
            source,
        })?;
    file.write_all(&json)
        .and_then(|()| file.sync_data())
        .map_err(|source| RecoveryError::Journal { path, source })
}

fn process_is_alive(process_id: u32) -> Result<bool, RecoveryError> {
    let handle = match unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, process_id) } {
        Ok(handle) => handle,
        Err(source) => {
            let last_error = unsafe { GetLastError() };
            if last_error == ERROR_INVALID_PARAMETER {
                return Ok(false);
            }
            return Err(RecoveryError::ProcessQuery { process_id, source });
        }
    };
    let wait = unsafe { WaitForSingleObject(handle, 0) };
    unsafe { CloseHandle(handle) }
        .map_err(|source| RecoveryError::ProcessQuery { process_id, source })?;
    if wait == WAIT_TIMEOUT {
        Ok(true)
    } else if wait == WAIT_OBJECT_0 {
        Ok(false)
    } else {
        Err(RecoveryError::ProcessWait { process_id })
    }
}

fn failed_report(project_root: &Path, process_id: u32, error: String) -> RecoveryProjectReport {
    RecoveryProjectReport {
        project_root: project_root.into(),
        process_id,
        disposition: RecoveryDisposition::Failed,
        fragment_count: 0,
        last_video_end_tick: 0,
        cursor_records_removed: 0,
        click_records_removed: 0,
        error: Some(error),
    }
}

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("invalid click record {path}:{line}: {source}")]
    InvalidClickRecord {
        path: PathBuf,
        line: usize,
        source: serde_json::Error,
    },
    #[error("both partial and finalized recording media exist; neither file was changed")]
    ConflictingMedia,
    #[error("finalized media has an incomplete tail; it was not truncated: {0}")]
    IncompleteFinalMedia(PathBuf),
    #[error("recording media resolves outside the project: {0}")]
    MediaOutsideProject(PathBuf),
    #[error(transparent)]
    ProjectEdit(#[from] crate::platform::editor_transaction::EditorTransactionError),
    #[error(transparent)]
    RecordingInitialization(#[from] RecorderError),
    #[error("could not scan project library {path}: {source}")]
    Scan {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not query recording process {process_id}: {source}")]
    ProcessQuery {
        process_id: u32,
        source: windows::core::Error,
    },
    #[error("could not determine state of recording process {process_id}")]
    ProcessWait { process_id: u32 },
    #[error("could not truncate recovered media {path}: {source}")]
    MediaTruncate {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not update recovery journal {path}: {source}")]
    Journal {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not serialize recovery journal entry: {0}")]
    Serialize(serde_json::Error),
    #[error(transparent)]
    ProjectIo(#[from] ProjectIoError),
    #[error(transparent)]
    Manifest(#[from] ManifestStoreError),
    #[error(transparent)]
    ManifestValidation(#[from] panzo_core::project::ProjectValidationError),
    #[error(transparent)]
    Media(#[from] Fmp4ProbeError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::fmp4_writer::Fmp4WriterProbe;
    use panzo_core::{
        CaptureDescriptor, CursorEvent, CursorEventKind, JournalOperation, JournalResult,
        PersistedInputEvent, ProjectManifest, ProjectWriter, RecordingLock, SessionClock,
        project::CaptureKind,
    };
    use panzo_core::{PhysicalRect, PhysicalSize};
    use uuid::Uuid;

    struct InterruptedProject {
        library: PathBuf,
        layout: ProjectLayout,
    }

    impl InterruptedProject {
        fn new(state: ProjectState, marker: bool, renamed: bool) -> Self {
            static MEDIA: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
            let media = MEDIA.get_or_init(|| {
                let path = std::env::temp_dir()
                    .join(format!("panzo-recovery-media-{}.mp4", Uuid::new_v4()));
                Fmp4WriterProbe::write_black_video(&path, 1).unwrap();
                let bytes = fs::read(&path).unwrap();
                fs::remove_file(path).unwrap();
                bytes
            });
            let library =
                std::env::temp_dir().join(format!("panzo-recovery-stages-{}", Uuid::new_v4()));
            fs::create_dir(&library).unwrap();
            let mut manifest = manifest();
            manifest.state = state;
            let layout = ProjectLayout::create_new(
                library.join("test.panzo"),
                &manifest,
                &RecordingLock {
                    schema_version: 1,
                    process_id: 0,
                    session_id: "interrupted".into(),
                    last_checkpoint_tick: TimeTick::ZERO,
                },
            )
            .unwrap();
            let mut writer = ProjectWriter::create(&layout).unwrap();
            writer
                .append_journal(
                    TimeTick::ZERO,
                    JournalOperation::ProjectCreated,
                    JournalResult::Success,
                    None,
                )
                .unwrap();
            writer.finish().unwrap();
            fs::write(
                layout.root().join(if renamed {
                    "media/screen.mp4"
                } else {
                    "media/screen.part.mp4"
                }),
                media,
            )
            .unwrap();
            if !marker {
                fs::remove_file(layout.recording_lock_path()).unwrap();
            }
            Self { library, layout }
        }

        fn scan(&self) -> RecoveryScanReport {
            RecoveryScanner::new(&self.library)
                .scan_and_recover()
                .unwrap()
        }

        fn assert_editable(&self) {
            let editor =
                crate::platform::editor::ProjectEditorSession::open(self.layout.root()).unwrap();
            assert_eq!(editor.project_duration(), TimeTick(10_000_000));
            assert!(!editor.settings().camera_enabled);
            assert!(!self.layout.recording_lock_path().exists());
        }
    }

    impl Drop for InterruptedProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.library);
        }
    }

    #[test]
    fn recovery_completes_recording_rename_and_markerless_processing() {
        for (state, marker, renamed) in [
            (ProjectState::Recording, true, false),
            (ProjectState::Finalizing, true, true),
            (ProjectState::Processing, false, true),
            (ProjectState::Recovered, false, false),
            (ProjectState::Ready, true, true),
        ] {
            let project = InterruptedProject::new(state, marker, renamed);
            let media = project.layout.root().join(if renamed {
                "media/screen.mp4"
            } else {
                "media/screen.part.mp4"
            });
            let before = fs::read(&media).unwrap();
            assert_eq!(project.scan().recovered_count(), 1, "{state:?}");
            project.assert_editable();
            assert_eq!(fs::read(&media).unwrap(), before);
            assert!(project.scan().projects.is_empty());
            if state == ProjectState::Ready {
                assert_eq!(project.layout.load_manifest().unwrap().state, state);
            }
        }
    }

    #[test]
    fn every_recovery_boundary_is_retryable_and_preserves_completed_files() {
        for stage in [
            "manifest-resolved",
            "media-checked",
            "events-trimmed",
            "edit-files-ready",
            "completion-committed",
            "marker-removed",
        ] {
            let project = InterruptedProject::new(ProjectState::Finalizing, true, true);
            let store = ManifestStore::new(project.layout.root());
            let lock = lock_project_edit(project.layout.root()).unwrap();
            let result = recover_project_with_hook(project.layout.root(), &store, |current| {
                if current == stage {
                    Err(RecoveryError::Media(Fmp4ProbeError::InvalidMediaTiming))
                } else {
                    Ok(())
                }
            });
            assert!(result.is_err(), "{stage}");
            drop(lock);
            let camera = fs::read(project.layout.root().join("tracks/camera.json")).ok();
            let settings = fs::read(project.layout.root().join("edit/workbench.json")).ok();
            assert_eq!(project.scan().failed_count(), 0, "{stage}");
            project.assert_editable();
            if let Some(camera) = camera {
                assert_eq!(
                    fs::read(project.layout.root().join("tracks/camera.json")).unwrap(),
                    camera
                );
            }
            if let Some(settings) = settings {
                assert_eq!(
                    fs::read(project.layout.root().join("edit/workbench.json")).unwrap(),
                    settings
                );
            }
        }
    }

    #[test]
    fn recovery_preserves_conflicts_and_corrupt_edit_data_for_retry() {
        let project = InterruptedProject::new(ProjectState::Processing, true, true);
        let part = project.layout.root().join("media/screen.part.mp4");
        fs::write(&part, b"unrelated partial video").unwrap();
        assert_eq!(project.scan().failed_count(), 1);
        assert_eq!(fs::read(&part).unwrap(), b"unrelated partial video");
        assert!(project.layout.recording_lock_path().exists());
        fs::remove_file(part).unwrap();
        let camera = project.layout.root().join("tracks/camera.json");
        fs::write(&camera, b"{unfinished-user-data").unwrap();
        assert_eq!(project.scan().failed_count(), 1);
        assert_eq!(fs::read(&camera).unwrap(), b"{unfinished-user-data");
        assert!(project.layout.recording_lock_path().exists());
        assert_eq!(
            project.layout.load_manifest().unwrap().state,
            ProjectState::Recovering
        );
        fs::remove_file(camera).unwrap();
        assert_eq!(project.scan().recovered_count(), 1);
        project.assert_editable();
    }

    #[test]
    fn recovery_waits_for_project_owner_and_preserves_existing_edits() {
        let project = InterruptedProject::new(ProjectState::Processing, false, true);
        let legacy_writer = OpenOptions::new()
            .append(true)
            .open(project.layout.journal_path())
            .unwrap();
        assert_eq!(
            project.scan().projects[0].disposition,
            RecoveryDisposition::Active
        );
        assert_eq!(
            project.layout.load_manifest().unwrap().state,
            ProjectState::Processing
        );
        drop(legacy_writer);
        let lock = lock_project_edit(project.layout.root()).unwrap();
        assert_eq!(
            project.scan().projects[0].disposition,
            RecoveryDisposition::Active
        );
        assert_eq!(
            project.layout.load_manifest().unwrap().state,
            ProjectState::Processing
        );
        drop(lock);
        assert_eq!(project.scan().recovered_count(), 1);
        let camera = fs::read(project.layout.root().join("tracks/camera.json")).unwrap();
        let workbench = project.layout.root().join("edit/workbench.json");
        // Simulates an interruption after camera publication but before workbench publication.
        fs::remove_file(&workbench).unwrap();
        assert_eq!(project.scan().recovered_count(), 1);
        assert_eq!(
            fs::read(project.layout.root().join("tracks/camera.json")).unwrap(),
            camera
        );
        project.assert_editable();
        let mut editor =
            crate::platform::editor::ProjectEditorSession::open(project.layout.root()).unwrap();
        editor.set_solid_background("#234567").unwrap();
        editor
            .save(panzo_core::CameraEditKind::Style, None)
            .unwrap();
        drop(editor);
        let camera = fs::read(project.layout.root().join("tracks/camera.json")).unwrap();
        let settings = fs::read(&workbench).unwrap();
        let mut manifest = project.layout.load_manifest().unwrap();
        manifest.state = ProjectState::Processing;
        ManifestStore::new(project.layout.root())
            .persist(&manifest)
            .unwrap();
        assert_eq!(project.scan().recovered_count(), 1);
        assert_eq!(
            fs::read(project.layout.root().join("tracks/camera.json")).unwrap(),
            camera
        );
        assert_eq!(fs::read(&workbench).unwrap(), settings);
    }

    #[test]
    fn recovery_only_truncates_incomplete_partial_media() {
        for renamed in [false, true] {
            let project = InterruptedProject::new(ProjectState::Finalizing, true, renamed);
            let media = project.layout.root().join(if renamed {
                "media/screen.mp4"
            } else {
                "media/screen.part.mp4"
            });
            let complete = fs::read(&media).unwrap();
            let mut with_tail = complete.clone();
            with_tail.extend(b"bad");
            fs::write(&media, &with_tail).unwrap();
            let report = project.scan();
            if renamed {
                assert_eq!(report.failed_count(), 1);
                assert_eq!(fs::read(&media).unwrap(), with_tail);
                assert!(project.layout.recording_lock_path().exists());
            } else {
                assert_eq!(report.recovered_count(), 1);
                assert_eq!(fs::read(&media).unwrap(), complete);
                project.assert_editable();
            }
        }
    }

    #[test]
    fn leaves_project_owned_by_live_process_untouched() {
        let library = std::env::temp_dir().join(format!("panzo-recovery-{}", Uuid::new_v4()));
        let project = library.join("active.panzo");
        fs::create_dir(&library).unwrap();
        let clock = SessionClock::establish(10_000_000, 10).unwrap();
        let manifest = ProjectManifest::new_v01(
            Uuid::new_v4(),
            "0.1.0",
            "2026-08-31T08:00:00Z",
            panzo_core::project::TimebaseDescriptor {
                ticks_per_second: clock.ticks_per_second,
                session_start_qpc: clock.session_start_qpc,
                qpc_frequency: clock.qpc_frequency,
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
        );
        ProjectLayout::create_new(
            &project,
            &manifest,
            &RecordingLock {
                schema_version: 1,
                process_id: std::process::id(),
                session_id: "active-session".into(),
                last_checkpoint_tick: TimeTick::ZERO,
            },
        )
        .unwrap();

        let report = RecoveryScanner::new(&library).scan_and_recover().unwrap();
        assert_eq!(report.projects.len(), 1);
        assert_eq!(report.projects[0].disposition, RecoveryDisposition::Active);
        assert_eq!(
            ProjectLayout::open(project)
                .unwrap()
                .load_manifest()
                .unwrap()
                .state,
            ProjectState::Recording
        );
        fs::remove_dir_all(library).unwrap();
    }

    #[test]
    fn recovers_dead_recording_and_trims_events_after_video() {
        let library = std::env::temp_dir().join(format!("panzo-recovery-{}", Uuid::new_v4()));
        let project = library.join("dead.panzo");
        fs::create_dir(&library).unwrap();
        let layout = ProjectLayout::create_new(
            &project,
            &manifest(),
            &RecordingLock {
                schema_version: 1,
                process_id: 0,
                session_id: "dead-session".into(),
                last_checkpoint_tick: TimeTick::ZERO,
            },
        )
        .unwrap();
        let mut writer = ProjectWriter::create(&layout).unwrap();
        writer
            .append_input(&PersistedInputEvent::Cursor(cursor_event(1, 5_000_000)))
            .unwrap();
        writer
            .append_input(&PersistedInputEvent::Cursor(cursor_event(2, 20_000_000)))
            .unwrap();
        writer
            .append_journal(
                TimeTick::ZERO,
                JournalOperation::ProjectCreated,
                JournalResult::Success,
                None,
            )
            .unwrap();
        writer.finish().unwrap();
        let mut cursor_file = OpenOptions::new()
            .append(true)
            .open(layout.cursor_events_path())
            .unwrap();
        cursor_file.write_all(b"{\"timeTick\":").unwrap();
        drop(cursor_file);
        Fmp4WriterProbe::write_black_video(project.join("media/screen.part.mp4"), 1).unwrap();

        let report = RecoveryScanner::new(&library).scan_and_recover().unwrap();
        assert_eq!(report.recovered_count(), 1);
        assert_eq!(report.projects[0].cursor_records_removed, 1);
        assert_eq!(report.projects[0].last_video_end_tick, 10_000_000);
        assert!(!layout.recording_lock_path().exists());
        assert_eq!(
            layout.load_manifest().unwrap().state,
            ProjectState::Recovered
        );
        assert_eq!(
            fs::read_to_string(layout.cursor_events_path())
                .unwrap()
                .lines()
                .count(),
            1
        );
        fs::remove_dir_all(library).unwrap();
    }

    fn manifest() -> ProjectManifest {
        let clock = SessionClock::establish(10_000_000, 10).unwrap();
        ProjectManifest::new_v01(
            Uuid::new_v4(),
            "0.1.0",
            "2026-08-31T08:00:00Z",
            panzo_core::project::TimebaseDescriptor {
                ticks_per_second: clock.ticks_per_second,
                session_start_qpc: clock.session_start_qpc,
                qpc_frequency: clock.qpc_frequency,
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

    fn cursor_event(id: u64, tick: i64) -> CursorEvent {
        CursorEvent {
            schema_version: panzo_core::events::EVENT_SCHEMA_VERSION,
            id,
            time_tick: TimeTick(tick),
            kind: CursorEventKind::Move,
            desktop_x: 100,
            desktop_y: 100,
            content_x: 100,
            content_y: 100,
            normalized_x: 0.05,
            normalized_y: 0.09,
            visible: true,
            geometry_revision: 0,
        }
    }
}
