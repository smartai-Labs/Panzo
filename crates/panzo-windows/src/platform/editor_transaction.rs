use crate::platform::manifest_store::{
    ManifestStoreError, move_file_atomically, persist_bytes_atomically,
    persist_bytes_new_atomically, persist_json_atomically,
};
use panzo_core::{
    CameraEditRecord, CameraTrack, ProjectIoError, ProjectLayout, WorkbenchSettings,
    WorkbenchValidationError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;
use thiserror::Error;
use uuid::Uuid;

pub const EDIT_TRANSACTION_FILE: &str = "edit/save.transaction.json";
pub const EDIT_TRANSACTION_DIRECTORY: &str = "edit/.transactions";
const RETIRED_TRANSACTION_FILE: &str = "edit/save.transaction.committed";
const TRANSACTION_SCHEMA_VERSION: u32 = 1;
const STAGED_AUTO_CAMERA_FILE: &str = "camera.auto.json";
const STAGED_CAMERA_FILE: &str = "camera.json";
const STAGED_WORKBENCH_FILE: &str = "workbench.json";
const STAGED_HISTORY_FILE: &str = "history.jsonl";
const AUTO_CAMERA_DESTINATION: &str = "tracks/camera.auto.json";
const WORKBENCH_DESTINATION: &str = "edit/workbench.json";
const HISTORY_DESTINATION: &str = "edit/history.jsonl";

#[derive(Clone, Copy)]
pub struct EditorTransactionRequest<'a> {
    pub project_root: &'a Path,
    pub camera_path: &'a Path,
    pub camera: &'a CameraTrack,
    pub auto_draft: Option<&'a CameraTrack>,
    pub settings: &'a WorkbenchSettings,
    pub history: &'a [u8],
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorTransactionCommit {
    pub transaction_id: Uuid,
    pub auto_draft_created: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorTransactionRecovery {
    None,
    Recovered { revision: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorTransactionStage {
    AutoDraftStaged,
    CameraStaged,
    WorkbenchStaged,
    HistoryStaged,
    CommitMarkerPublished,
    AutoDraftMaterialized,
    CameraMaterialized,
    WorkbenchMaterialized,
    HistoryMaterialized,
    CommitMarkerRemoved,
}

impl EditorTransactionStage {
    pub const ALL: [Self; 10] = [
        Self::AutoDraftStaged,
        Self::CameraStaged,
        Self::WorkbenchStaged,
        Self::HistoryStaged,
        Self::CommitMarkerPublished,
        Self::AutoDraftMaterialized,
        Self::CameraMaterialized,
        Self::WorkbenchMaterialized,
        Self::HistoryMaterialized,
        Self::CommitMarkerRemoved,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::AutoDraftStaged => "auto-draft-staged",
            Self::CameraStaged => "camera-staged",
            Self::WorkbenchStaged => "workbench-staged",
            Self::HistoryStaged => "history-staged",
            Self::CommitMarkerPublished => "commit-marker-published",
            Self::AutoDraftMaterialized => "auto-draft-materialized",
            Self::CameraMaterialized => "camera-materialized",
            Self::WorkbenchMaterialized => "workbench-materialized",
            Self::HistoryMaterialized => "history-materialized",
            Self::CommitMarkerRemoved => "commit-marker-removed",
        }
    }

    pub const fn committed(self) -> bool {
        matches!(
            self,
            Self::CommitMarkerPublished
                | Self::AutoDraftMaterialized
                | Self::CameraMaterialized
                | Self::WorkbenchMaterialized
                | Self::HistoryMaterialized
                | Self::CommitMarkerRemoved
        )
    }
}

impl fmt::Display for EditorTransactionStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

impl FromStr for EditorTransactionStage {
    type Err = EditorTransactionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|stage| stage.label() == value)
            .ok_or_else(|| EditorTransactionError::InvalidFaultStage(value.into()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditorSaveTransaction {
    schema_version: u32,
    transaction_id: Uuid,
    revision: u64,
    camera_destination: String,
    hashes: EditorTransactionHashes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditorTransactionHashes {
    auto_camera: Option<String>,
    camera: String,
    workbench: String,
    history: String,
}

pub fn commit_editor_transaction(
    request: EditorTransactionRequest<'_>,
) -> Result<EditorTransactionCommit, EditorTransactionError> {
    commit_editor_transaction_with_hook(request, |_| Ok(()))
}

pub fn commit_editor_transaction_with_hook(
    request: EditorTransactionRequest<'_>,
    mut hook: impl FnMut(EditorTransactionStage) -> Result<(), EditorTransactionError>,
) -> Result<EditorTransactionCommit, EditorTransactionError> {
    let _lock = lock_project_edit(request.project_root)?;
    request.camera.validate()?;
    request.settings.validate()?;
    let camera_destination = project_relative_path(request.project_root, request.camera_path)?;
    validate_history(request.history, request.revision, request.camera)?;
    if let Some(auto_draft) = request.auto_draft {
        auto_draft.validate()?;
    }

    recover_pending_editor_transaction_locked(request.project_root)?;
    let current_path = request.project_root.join(WORKBENCH_DESTINATION);
    let current_revision = if current_path.exists() {
        let current: WorkbenchSettings = read_json(&current_path)?;
        current.validate()?;
        current.revision
    } else {
        0
    };
    if current_revision.checked_add(1) != Some(request.revision) {
        return Err(EditorTransactionError::RevisionConflict {
            expected: request.revision.saturating_sub(1),
            actual: current_revision,
        });
    }
    let transaction_id = Uuid::new_v4();
    let transaction_root = transaction_root(request.project_root, transaction_id);
    fs::create_dir_all(&transaction_root).map_err(|source| {
        EditorTransactionError::CreateDirectory {
            path: transaction_root.clone(),
            source,
        }
    })?;

    let result = prepare_and_commit(
        &request,
        camera_destination,
        transaction_id,
        &transaction_root,
        &mut hook,
    );
    if !request.project_root.join(EDIT_TRANSACTION_FILE).is_file() {
        let _ = fs::remove_dir_all(&transaction_root);
    }
    result
}

fn prepare_and_commit(
    request: &EditorTransactionRequest<'_>,
    camera_destination: String,
    transaction_id: Uuid,
    transaction_root: &Path,
    hook: &mut impl FnMut(EditorTransactionStage) -> Result<(), EditorTransactionError>,
) -> Result<EditorTransactionCommit, EditorTransactionError> {
    let auto_bytes = request.auto_draft.map(serialize_json).transpose()?;
    if let Some(bytes) = auto_bytes.as_deref() {
        write_durable(&transaction_root.join(STAGED_AUTO_CAMERA_FILE), bytes)?;
        hook(EditorTransactionStage::AutoDraftStaged)?;
    }
    let camera_bytes = serialize_json(request.camera)?;
    write_durable(&transaction_root.join(STAGED_CAMERA_FILE), &camera_bytes)?;
    hook(EditorTransactionStage::CameraStaged)?;
    let workbench_bytes = serialize_json(request.settings)?;
    write_durable(
        &transaction_root.join(STAGED_WORKBENCH_FILE),
        &workbench_bytes,
    )?;
    hook(EditorTransactionStage::WorkbenchStaged)?;
    write_durable(&transaction_root.join(STAGED_HISTORY_FILE), request.history)?;
    hook(EditorTransactionStage::HistoryStaged)?;

    let transaction = EditorSaveTransaction {
        schema_version: TRANSACTION_SCHEMA_VERSION,
        transaction_id,
        revision: request.revision,
        camera_destination,
        hashes: EditorTransactionHashes {
            auto_camera: auto_bytes.as_deref().map(content_hash),
            camera: content_hash(&camera_bytes),
            workbench: content_hash(&workbench_bytes),
            history: content_hash(request.history),
        },
    };
    persist_json_atomically(
        request.project_root.join(EDIT_TRANSACTION_FILE),
        &transaction,
    )?;
    hook(EditorTransactionStage::CommitMarkerPublished)?;
    materialize_transaction(request.project_root, &transaction, hook)?;
    Ok(EditorTransactionCommit {
        transaction_id,
        auto_draft_created: auto_bytes.is_some(),
    })
}

pub fn recover_pending_editor_transaction(
    project_root: impl AsRef<Path>,
) -> Result<EditorTransactionRecovery, EditorTransactionError> {
    let project_root = project_root.as_ref();
    let _lock = lock_project_edit(project_root)?;
    recover_pending_editor_transaction_locked(project_root)
}

/// Windows releases this exclusive handle after a crash; no stale-lock deletion is needed.
pub(crate) fn lock_project_edit(project_root: &Path) -> Result<fs::File, EditorTransactionError> {
    let path = project_root.join(".editor-write.lock");
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(&path)
        .map_err(|source| EditorTransactionError::ProjectBusy { path, source })
}

pub(crate) fn recover_pending_editor_transaction_locked(
    project_root: &Path,
) -> Result<EditorTransactionRecovery, EditorTransactionError> {
    let marker_path = project_root.join(EDIT_TRANSACTION_FILE);
    if !marker_path.is_file() {
        cleanup_orphan_transactions(project_root)?;
        let marker_temporary = marker_path.with_file_name("save.transaction.json.tmp");
        remove_file_if_present(&marker_temporary)?;
        remove_file_if_present(&project_root.join(RETIRED_TRANSACTION_FILE))?;
        return Ok(EditorTransactionRecovery::None);
    }
    let transaction: EditorSaveTransaction = read_json(&marker_path)?;
    materialize_transaction(project_root, &transaction, &mut |_| Ok(()))?;
    Ok(EditorTransactionRecovery::Recovered {
        revision: transaction.revision,
    })
}

fn materialize_transaction(
    project_root: &Path,
    transaction: &EditorSaveTransaction,
    hook: &mut impl FnMut(EditorTransactionStage) -> Result<(), EditorTransactionError>,
) -> Result<(), EditorTransactionError> {
    validate_transaction(project_root, transaction)?;
    let root = transaction_root(project_root, transaction.transaction_id);
    let auto = read_staged_optional(
        &root.join(STAGED_AUTO_CAMERA_FILE),
        transaction.hashes.auto_camera.as_deref(),
    )?;
    let camera = read_staged(&root.join(STAGED_CAMERA_FILE), &transaction.hashes.camera)?;
    let workbench = read_staged(
        &root.join(STAGED_WORKBENCH_FILE),
        &transaction.hashes.workbench,
    )?;
    let history = read_staged(&root.join(STAGED_HISTORY_FILE), &transaction.hashes.history)?;

    let camera_value: CameraTrack = serde_json::from_slice(&camera)?;
    camera_value.validate()?;
    let settings_value: WorkbenchSettings = serde_json::from_slice(&workbench)?;
    settings_value.validate()?;
    validate_history(&history, transaction.revision, &camera_value)?;
    if let Some(bytes) = auto.as_deref() {
        let auto_value: CameraTrack = serde_json::from_slice(bytes)?;
        auto_value.validate()?;
        materialize_immutable(project_root.join(AUTO_CAMERA_DESTINATION), bytes)?;
        hook(EditorTransactionStage::AutoDraftMaterialized)?;
    }

    persist_bytes_atomically(project_root.join(&transaction.camera_destination), &camera)?;
    hook(EditorTransactionStage::CameraMaterialized)?;
    persist_bytes_atomically(project_root.join(WORKBENCH_DESTINATION), &workbench)?;
    hook(EditorTransactionStage::WorkbenchMaterialized)?;
    persist_bytes_atomically(project_root.join(HISTORY_DESTINATION), &history)?;
    hook(EditorTransactionStage::HistoryMaterialized)?;

    let marker_path = project_root.join(EDIT_TRANSACTION_FILE);
    let retired_marker = project_root.join(RETIRED_TRANSACTION_FILE);
    move_file_atomically(&marker_path, &retired_marker)?;
    hook(EditorTransactionStage::CommitMarkerRemoved)?;
    let _ = fs::remove_dir_all(&root);
    remove_empty_transaction_directory(project_root);
    let _ = fs::remove_file(retired_marker);
    Ok(())
}

fn validate_transaction(
    project_root: &Path,
    transaction: &EditorSaveTransaction,
) -> Result<(), EditorTransactionError> {
    if transaction.schema_version != TRANSACTION_SCHEMA_VERSION {
        return Err(EditorTransactionError::UnsupportedSchemaVersion(
            transaction.schema_version,
        ));
    }
    let layout = ProjectLayout::open(project_root)?;
    let manifest = layout.load_manifest()?;
    if transaction.camera_destination != manifest.tracks.camera {
        return Err(EditorTransactionError::CameraDestinationMismatch {
            transaction: transaction.camera_destination.clone(),
            manifest: manifest.tracks.camera,
        });
    }
    validate_relative_path(&transaction.camera_destination)
}

fn materialize_immutable(destination: PathBuf, bytes: &[u8]) -> Result<(), EditorTransactionError> {
    if destination.is_file() {
        let existing = fs::read(&destination).map_err(|source| EditorTransactionError::Read {
            path: destination.clone(),
            source,
        })?;
        if existing != bytes {
            return Err(EditorTransactionError::AutoDraftChanged(destination));
        }
        return Ok(());
    }
    persist_bytes_new_atomically(destination, bytes)?;
    Ok(())
}

fn validate_history(
    bytes: &[u8],
    revision: u64,
    camera: &CameraTrack,
) -> Result<(), EditorTransactionError> {
    let mut last = None;
    for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let record: CameraEditRecord = serde_json::from_slice(line).map_err(|source| {
            EditorTransactionError::InvalidHistoryRecord {
                line: index + 1,
                source,
            }
        })?;
        last = Some(record);
    }
    let last = last.ok_or(EditorTransactionError::MissingHistoryRecord)?;
    let camera_hash = camera.canonical_hash()?;
    if last.revision != revision || last.after_hash != camera_hash {
        return Err(EditorTransactionError::HistoryRevisionMismatch {
            expected_revision: revision,
            actual_revision: last.revision,
            expected_camera_hash: camera_hash,
            actual_camera_hash: last.after_hash,
        });
    }
    Ok(())
}

fn serialize_json(value: &impl Serialize) -> Result<Vec<u8>, EditorTransactionError> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn write_durable(path: &Path, bytes: &[u8]) -> Result<(), EditorTransactionError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|source| EditorTransactionError::Write {
            path: path.into(),
            source,
        })?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|source| EditorTransactionError::Write {
            path: path.into(),
            source,
        })
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, EditorTransactionError> {
    let bytes = fs::read(path).map_err(|source| EditorTransactionError::Read {
        path: path.into(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(EditorTransactionError::Json)
}

fn read_staged(path: &Path, expected_hash: &str) -> Result<Vec<u8>, EditorTransactionError> {
    let bytes = fs::read(path).map_err(|source| EditorTransactionError::Read {
        path: path.into(),
        source,
    })?;
    verify_hash(path, &bytes, expected_hash)?;
    Ok(bytes)
}

fn read_staged_optional(
    path: &Path,
    expected_hash: Option<&str>,
) -> Result<Option<Vec<u8>>, EditorTransactionError> {
    expected_hash
        .map(|hash| read_staged(path, hash))
        .transpose()
}

fn verify_hash(
    path: &Path,
    bytes: &[u8],
    expected_hash: &str,
) -> Result<(), EditorTransactionError> {
    let actual = content_hash(bytes);
    if actual == expected_hash {
        Ok(())
    } else {
        Err(EditorTransactionError::HashMismatch {
            path: path.into(),
            expected: expected_hash.into(),
            actual,
        })
    }
}

fn content_hash(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn transaction_root(project_root: &Path, transaction_id: Uuid) -> PathBuf {
    project_root
        .join(EDIT_TRANSACTION_DIRECTORY)
        .join(transaction_id.to_string())
}

fn project_relative_path(
    project_root: &Path,
    path: &Path,
) -> Result<String, EditorTransactionError> {
    let relative = path
        .strip_prefix(project_root)
        .map_err(|_| EditorTransactionError::DestinationOutsideProject(path.into()))?;
    let value = relative
        .to_str()
        .ok_or_else(|| EditorTransactionError::NonUnicodePath(relative.into()))?
        .replace('\\', "/");
    validate_relative_path(&value)?;
    Ok(value)
}

fn validate_relative_path(value: &str) -> Result<(), EditorTransactionError> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(EditorTransactionError::InvalidRelativePath(value.into()));
    }
    Ok(())
}

fn cleanup_orphan_transactions(project_root: &Path) -> Result<(), EditorTransactionError> {
    let directory = project_root.join(EDIT_TRANSACTION_DIRECTORY);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(EditorTransactionError::ReadDirectory {
                path: directory,
                source,
            });
        }
    };
    for entry in entries {
        let entry = entry.map_err(|source| EditorTransactionError::ReadDirectory {
            path: directory.clone(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            fs::remove_dir_all(&path)
                .map_err(|source| EditorTransactionError::Remove { path, source })?;
        } else {
            fs::remove_file(&path)
                .map_err(|source| EditorTransactionError::Remove { path, source })?;
        }
    }
    remove_empty_transaction_directory(project_root);
    Ok(())
}

fn remove_empty_transaction_directory(project_root: &Path) {
    let _ = fs::remove_dir(project_root.join(EDIT_TRANSACTION_DIRECTORY));
}

fn remove_file_if_present(path: &Path) -> Result<(), EditorTransactionError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(EditorTransactionError::Remove {
            path: path.into(),
            source,
        }),
    }
}

#[derive(Debug, Error)]
pub enum EditorTransactionError {
    #[error("工程正被其他窗口保存或恢复，请稍后重试：{path}（{source}）")]
    ProjectBusy {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "工程已被其他窗口更新（本窗口版本 {expected}，磁盘版本 {actual}）。请重新打开工程；本窗口修改仍保留。"
    )]
    RevisionConflict { expected: u64, actual: u64 },
    #[error(transparent)]
    Project(#[from] ProjectIoError),
    #[error(transparent)]
    AtomicWrite(#[from] ManifestStoreError),
    #[error(transparent)]
    Camera(#[from] panzo_core::camera::CameraValidationError),
    #[error(transparent)]
    Settings(#[from] WorkbenchValidationError),
    #[error("editor transaction JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("failed to create editor transaction directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write editor transaction file {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read editor transaction file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to enumerate editor transaction directory {path}: {source}")]
    ReadDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to remove editor transaction artifact {path}: {source}")]
    Remove {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unsupported editor transaction schema version: {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("editor transaction destination is outside the Project: {0}")]
    DestinationOutsideProject(PathBuf),
    #[error("editor transaction path is not Unicode: {0}")]
    NonUnicodePath(PathBuf),
    #[error("invalid editor transaction relative path: {0}")]
    InvalidRelativePath(String),
    #[error(
        "editor transaction Camera destination {transaction} does not match manifest {manifest}"
    )]
    CameraDestinationMismatch {
        transaction: String,
        manifest: String,
    },
    #[error(
        "editor transaction staged hash mismatch for {path}: expected {expected}, got {actual}"
    )]
    HashMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("editor transaction would replace immutable Auto Draft: {0}")]
    AutoDraftChanged(PathBuf),
    #[error("editor transaction history is empty")]
    MissingHistoryRecord,
    #[error("invalid editor transaction history record on line {line}: {source}")]
    InvalidHistoryRecord {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "editor transaction history does not match revision {expected_revision}/{actual_revision} or Camera hash {expected_camera_hash}/{actual_camera_hash}"
    )]
    HistoryRevisionMismatch {
        expected_revision: u64,
        actual_revision: u64,
        expected_camera_hash: String,
        actual_camera_hash: String,
    },
    #[error("invalid editor transaction crash stage: {0}")]
    InvalidFaultStage(String),
    #[error("editor transaction fault injected at {0}")]
    FaultInjected(EditorTransactionStage),
}
