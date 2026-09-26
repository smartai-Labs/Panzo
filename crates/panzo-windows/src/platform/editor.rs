use crate::platform::background_image::{
    BackgroundImageError, DecodedBackgroundImage, decode_background_bytes, decode_background_file,
    normalized_extension,
};
use crate::platform::editor_transaction::{
    EditorTransactionError, EditorTransactionRequest, EditorTransactionStage,
    commit_editor_transaction_with_hook, lock_project_edit,
    recover_pending_editor_transaction_locked,
};
#[cfg(test)]
use crate::platform::manifest_store::persist_json_atomically;
use panzo_core::{
    BackgroundKind, CameraEditError, CameraEditKind, CameraEditRecord, CameraSegmentKind,
    CameraState, CameraTrack, CameraTrackEditor, ClickEvent, ProjectIoError, ProjectLayout,
    ProjectState, TimeTick, WorkbenchSettings, WorkbenchValidationError,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const AUTO_CAMERA_FILE: &str = "tracks/camera.auto.json";
pub const WORKBENCH_FILE: &str = "edit/workbench.json";
pub const EDIT_HISTORY_FILE: &str = "edit/history.jsonl";
#[path = "editor_draft.rs"]
mod draft;
pub use draft::DraftManager;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorSaveReport {
    pub project_root: PathBuf,
    pub revision: u64,
    pub transaction_id: uuid::Uuid,
    pub camera_path: PathBuf,
    pub auto_camera_path: PathBuf,
    pub workbench_path: PathBuf,
    pub auto_draft_created: bool,
    pub camera_hash: String,
    pub segment_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorAcceptanceReport {
    pub project_root: PathBuf,
    pub edited_segment_id: String,
    pub revision: u64,
    pub auto_draft_created: bool,
    pub checks: EditorAcceptanceChecks,
    pub before_camera_hash: String,
    pub edited_camera_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorAcceptanceChecks {
    pub auto_draft_preserved: bool,
    pub edit_persisted: bool,
    pub style_persisted: bool,
    pub assets: EditorAssetAcceptanceChecks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorAssetAcceptanceChecks {
    pub background_asset_persisted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedBackground {
    pub project_path: String,
    pub absolute_path: PathBuf,
    pub width: u32,
    pub height: u32,
}

pub struct EditorAcceptanceProbe;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorAtomicStateReport {
    pub project_root: PathBuf,
    pub revision: u64,
    pub camera_hash: String,
    pub workbench_hash: String,
    pub history_hash: String,
    pub auto_draft_hash: String,
    pub source_media_hash: String,
    pub source_cursor_hash: String,
    pub source_clicks_hash: String,
    pub state_fingerprint: String,
    pub pending_transaction: bool,
    pub transaction_artifacts: usize,
}

pub struct EditorAtomicProbe;

impl EditorAtomicProbe {
    pub fn inspect(
        project_root: impl AsRef<Path>,
    ) -> Result<EditorAtomicStateReport, EditorSessionError> {
        let project_root = project_root.as_ref();
        let session = ProjectEditorSession::open(project_root)?;
        let layout = ProjectLayout::open(project_root)?;
        let manifest = layout.load_manifest()?;
        let camera_hash = session.camera().canonical_hash()?;
        let workbench_hash = sha256(&serde_json::to_vec(session.settings())?);
        let history_hash = hash_file_or_missing(&layout.root().join(EDIT_HISTORY_FILE))?;
        let auto_draft_hash = hash_file_or_missing(&layout.root().join(AUTO_CAMERA_FILE))?;
        let source_media_hash = hash_required_file(&layout.root().join(&manifest.media.screen))?;
        let source_cursor_hash = hash_required_file(&layout.root().join(&manifest.tracks.cursor))?;
        let source_clicks_hash = hash_required_file(&layout.root().join(&manifest.tracks.clicks))?;
        let fingerprint_bytes = [
            session.settings().revision.to_string(),
            camera_hash.clone(),
            workbench_hash.clone(),
            history_hash.clone(),
            auto_draft_hash.clone(),
            source_media_hash.clone(),
            source_cursor_hash.clone(),
            source_clicks_hash.clone(),
        ]
        .join("\n");
        let transaction_directory = layout
            .root()
            .join(crate::platform::editor_transaction::EDIT_TRANSACTION_DIRECTORY);
        let transaction_artifacts = match fs::read_dir(&transaction_directory) {
            Ok(entries) => entries.count(),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => 0,
            Err(source) => {
                return Err(EditorSessionError::Read {
                    path: transaction_directory,
                    source,
                });
            }
        };
        Ok(EditorAtomicStateReport {
            project_root: layout.root().into(),
            revision: session.settings().revision,
            camera_hash,
            workbench_hash,
            history_hash,
            auto_draft_hash,
            source_media_hash,
            source_cursor_hash,
            source_clicks_hash,
            state_fingerprint: sha256(fingerprint_bytes.as_bytes()),
            pending_transaction: layout
                .root()
                .join(crate::platform::editor_transaction::EDIT_TRANSACTION_FILE)
                .is_file(),
            transaction_artifacts,
        })
    }

    pub fn save(
        project_root: impl AsRef<Path>,
    ) -> Result<EditorAtomicStateReport, EditorSessionError> {
        let project_root = project_root.as_ref();
        let mut session = ProjectEditorSession::open(project_root)?;
        let segment_id = apply_atomic_probe_edit(&mut session)?;
        session.save(CameraEditKind::Update, Some(segment_id))?;
        Self::inspect(project_root)
    }

    pub fn crash(
        project_root: impl AsRef<Path>,
        crash_stage: EditorTransactionStage,
    ) -> Result<(), EditorSessionError> {
        let mut session = ProjectEditorSession::open(project_root)?;
        let segment_id = apply_atomic_probe_edit(&mut session)?;
        session.save_with_crash_injection(CameraEditKind::Update, Some(segment_id), crash_stage)?;
        Ok(())
    }
}

fn apply_atomic_probe_edit(
    session: &mut ProjectEditorSession,
) -> Result<String, EditorSessionError> {
    let segment = session
        .camera()
        .segments
        .first()
        .cloned()
        .ok_or(EditorSessionError::AtomicProbeMissingSegment)?;
    let scale = if segment.to.scale <= 7.75 {
        segment.to.scale + 0.125
    } else {
        segment.to.scale - 0.125
    };
    session.set_segment_target(
        &segment.id,
        CameraState {
            scale,
            ..segment.to
        },
    )?;
    session.set_solid_background("#223344")?;
    session.set_canvas_inset(0.0625)?;
    let duration = session.project_duration().as_i64();
    if duration >= 3 {
        // Fault runs are independent processes, so their probe edits must have
        // deterministic IDs for byte-for-byte old/new revision comparisons.
        // Interactive edits continue using UUIDs in split_video().
        let mut video = session.video_edit();
        video.split(TimeTick(duration / 3), "atomic-probe-middle".into())?;
        video.split(TimeTick(duration * 2 / 3), "atomic-probe-tail".into())?;
        video.delete("atomic-probe-middle")?;
        session.replace_video(video);
    }
    session.set_camera_enabled(false);
    session.set_cursor_style(false, 1.5)?;
    Ok(segment.id)
}

fn hash_required_file(path: &Path) -> Result<String, EditorSessionError> {
    fs::read(path)
        .map(|bytes| sha256(&bytes))
        .map_err(|source| EditorSessionError::Read {
            path: path.into(),
            source,
        })
}

fn hash_file_or_missing(path: &Path) -> Result<String, EditorSessionError> {
    match fs::read(path) {
        Ok(bytes) => Ok(sha256(&bytes)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok("missing".into()),
        Err(source) => Err(EditorSessionError::Read {
            path: path.into(),
            source,
        }),
    }
}

impl EditorAcceptanceProbe {
    /// Mutates the supplied acceptance-copy Project. Callers must not pass the user's only copy.
    pub fn exercise(
        project_root: impl AsRef<Path>,
    ) -> Result<EditorAcceptanceReport, EditorAcceptanceError> {
        let mut session = ProjectEditorSession::open(project_root)?;
        let segment = session
            .camera()
            .segments
            .first()
            .cloned()
            .ok_or(EditorAcceptanceError::NoCameraSegment)?;
        let before_camera_hash = session.camera().canonical_hash()?;
        let target = CameraState {
            scale: segment.to.scale + 0.1,
            ..segment.to
        };
        session.set_segment_target(&segment.id, target)?;
        session.set_solid_background("#223344")?;
        session.set_canvas_inset(0.06)?;
        session.set_corner_radius(0.02)?;
        session.set_shadow(0.5)?;
        let probe_background = session
            .project_root()
            .join("diagnostics/editor-probe-background.bmp");
        write_acceptance_bmp(&probe_background)?;
        let imported = session.import_background_image(&probe_background)?;
        let save = session.save(CameraEditKind::Update, Some(segment.id.clone()))?;

        let reopened = ProjectEditorSession::open(&save.project_root)?;
        let edited = reopened
            .camera()
            .segments
            .iter()
            .find(|candidate| candidate.id == segment.id)
            .ok_or(EditorAcceptanceError::EditedSegmentMissing)?;
        let auto: CameraTrack = read_json(&save.auto_camera_path)?;
        let auto_draft_preserved = auto.canonical_hash()? == before_camera_hash;
        let edit_persisted = edited.user_modified
            && edited.locked
            && (edited.to.scale - target.scale).abs() < 1.0e-9;
        let style_persisted = reopened.settings().background.color == "#223344"
            && (reopened.settings().canvas.inset - 0.06).abs() < 1.0e-9
            && (reopened.settings().canvas.corner_radius - 0.02).abs() < 1.0e-9
            && (reopened.settings().canvas.shadow - 0.5).abs() < 1.0e-9;
        let background_asset_persisted = reopened.settings().background.kind
            == BackgroundKind::Image
            && reopened.settings().background.image.as_deref() == Some(&imported.project_path)
            && imported.absolute_path.is_file()
            && load_effective_background(&save.project_root, reopened.settings())?.is_some();
        if !auto_draft_preserved
            || !edit_persisted
            || !style_persisted
            || !background_asset_persisted
        {
            return Err(EditorAcceptanceError::PersistenceMismatch);
        }
        Ok(EditorAcceptanceReport {
            project_root: save.project_root,
            edited_segment_id: segment.id,
            revision: save.revision,
            auto_draft_created: save.auto_draft_created,
            checks: EditorAcceptanceChecks {
                auto_draft_preserved,
                edit_persisted,
                style_persisted,
                assets: EditorAssetAcceptanceChecks {
                    background_asset_persisted,
                },
            },
            before_camera_hash,
            edited_camera_hash: save.camera_hash,
        })
    }
}

#[derive(Clone)]
pub struct ProjectEditorSession {
    project_root: PathBuf,
    camera_path: PathBuf,
    auto_camera_path: PathBuf,
    workbench_path: PathBuf,
    history_path: PathBuf,
    duration: TimeTick,
    click_ticks: Vec<TimeTick>,
    auto_draft: CameraTrack,
    saved_camera: CameraTrack,
    editor: CameraTrackEditor,
    saved_settings: WorkbenchSettings,
    settings: WorkbenchSettings,
    undo_stack: Vec<EditorSnapshot>,
    redo_stack: Vec<EditorSnapshot>,
    edit_group_before: Option<EditorSnapshot>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct EditorSnapshot {
    camera: CameraTrack,
    settings: WorkbenchSettings,
}

impl ProjectEditorSession {
    pub fn open(project_root: impl AsRef<Path>) -> Result<Self, EditorSessionError> {
        let layout = ProjectLayout::open(project_root)?;
        let _lock = lock_project_edit(layout.root())?;
        recover_pending_editor_transaction_locked(layout.root())?;
        let manifest = layout.load_manifest()?;
        if !matches!(
            manifest.state,
            ProjectState::Ready | ProjectState::Recovered
        ) {
            return Err(EditorSessionError::ProjectNotEditable(manifest.state));
        }
        let duration = TimeTick(manifest.media.duration_tick);
        let camera_path = layout.root().join(&manifest.tracks.camera);
        let saved_camera: CameraTrack = read_json(&camera_path)?;
        let auto_camera_path = layout.root().join(AUTO_CAMERA_FILE);
        let auto_draft = if auto_camera_path.is_file() {
            read_json(&auto_camera_path)?
        } else {
            saved_camera.clone()
        };
        auto_draft.validate()?;

        let workbench_path = layout.root().join(WORKBENCH_FILE);
        let settings = if workbench_path.is_file() {
            read_json(&workbench_path)?
        } else {
            WorkbenchSettings::default()
        };
        settings.validate()?;
        saved_camera.validate()?;
        let mut stabilized = saved_camera.clone();
        stabilized.stabilize_auto_pans();
        let editor = CameraTrackEditor::new(stabilized, duration)?.without_history();
        settings.video_edit(duration)?;
        let click_path = layout.root().join(&manifest.tracks.clicks);
        let click_ticks = if click_path.is_file() {
            read_click_ticks(&click_path)?
        } else {
            Vec::new()
        };

        Ok(Self {
            project_root: layout.root().into(),
            camera_path,
            auto_camera_path,
            workbench_path,
            history_path: layout.root().join(EDIT_HISTORY_FILE),
            duration,
            click_ticks,
            auto_draft,
            saved_camera,
            editor,
            saved_settings: settings.clone(),
            settings,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            edit_group_before: None,
        })
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Task payload excludes the undo/redo graph and click UI index.
    /// # Panics
    /// Panics only if the session's previously validated camera invariant is violated.
    pub fn background_snapshot(&self) -> Self {
        Self {
            project_root: self.project_root.clone(),
            camera_path: self.camera_path.clone(),
            auto_camera_path: self.auto_camera_path.clone(),
            workbench_path: self.workbench_path.clone(),
            history_path: self.history_path.clone(),
            duration: self.duration,
            click_ticks: vec![],
            auto_draft: self.auto_draft.clone(),
            saved_camera: self.saved_camera.clone(),
            editor: CameraTrackEditor::new(self.camera().clone(), self.duration)
                .expect("validated track")
                .without_history(),
            saved_settings: self.saved_settings.clone(),
            settings: self.settings.clone(),
            undo_stack: vec![],
            redo_stack: vec![],
            edit_group_before: None,
        }
    }

    pub const fn camera(&self) -> &CameraTrack {
        self.editor.track()
    }

    pub const fn settings(&self) -> &WorkbenchSettings {
        &self.settings
    }

    pub const fn duration(&self) -> TimeTick {
        self.duration
    }

    pub fn click_ticks(&self) -> &[TimeTick] {
        &self.click_ticks
    }

    /// # Panics
    /// Only if an internal invariant is broken; all loaded and edited states are validated.
    pub fn video_edit(&self) -> panzo_core::VideoEdit {
        self.settings
            .video_edit(self.duration)
            .expect("validated edit state")
    }

    pub fn project_duration(&self) -> TimeTick {
        self.video_edit().duration()
    }

    pub fn split_video(&mut self, at: TimeTick) -> Result<String, EditorSessionError> {
        let id = uuid::Uuid::new_v4().to_string();
        let mut video = self.video_edit();
        video.split(at, id.clone())?;
        self.replace_video(video);
        Ok(id)
    }

    pub fn delete_video(&mut self, id: &str) -> Result<(), EditorSessionError> {
        let mut video = self.video_edit();
        video.delete(id)?;
        self.replace_video(video);
        Ok(())
    }

    pub fn trim_video(
        &mut self,
        id: &str,
        start: TimeTick,
        end: TimeTick,
    ) -> Result<(), EditorSessionError> {
        let mut video = self.video_edit();
        video.trim(id, start, end)?;
        self.replace_video(video);
        Ok(())
    }

    pub fn set_video_crop(
        &mut self,
        id: &str,
        crop: panzo_core::VideoCrop,
    ) -> Result<(), EditorSessionError> {
        let mut video = self.video_edit();
        video.set_crop(id, crop)?;
        self.replace_video(video);
        Ok(())
    }

    pub fn set_video_speed(
        &mut self,
        id: &str,
        speed_percent: u16,
    ) -> Result<(), EditorSessionError> {
        let mut video = self.video_edit();
        video.set_speed_percent(id, speed_percent)?;
        self.replace_video(video);
        Ok(())
    }

    fn replace_video(&mut self, video: panzo_core::VideoEdit) {
        let before = self.snapshot();
        self.settings.schema_version = panzo_core::WORKBENCH_SCHEMA_VERSION;
        self.settings.video = Some(video);
        self.commit_undo(before);
    }

    pub fn set_camera_enabled(&mut self, enabled: bool) {
        let before = self.snapshot();
        self.settings.schema_version = panzo_core::WORKBENCH_SCHEMA_VERSION;
        self.settings.camera_enabled = enabled;
        self.commit_undo(before);
    }

    pub fn set_cursor_style(
        &mut self,
        visible: bool,
        scale: f64,
    ) -> Result<(), EditorSessionError> {
        let before = self.snapshot();
        let mut settings = self.settings.clone();
        settings.cursor = panzo_core::CursorStyle { visible, scale };
        settings.validate()?;
        self.settings = settings;
        self.commit_undo(before);
        Ok(())
    }

    pub fn cancel_edit_group(&mut self) -> bool {
        let Some(before) = self.edit_group_before.take() else {
            return false;
        };
        self.restore(before);
        true
    }

    pub fn is_dirty(&self) -> bool {
        self.editor.track() != &self.saved_camera
            || !self.settings_content_equal(&self.settings, &self.saved_settings)
    }

    fn settings_content_equal(&self, left: &WorkbenchSettings, right: &WorkbenchSettings) -> bool {
        let normalize = |settings: &WorkbenchSettings| {
            let mut normalized = settings.clone();
            normalized.revision = 0;
            normalized.schema_version = panzo_core::WORKBENCH_SCHEMA_VERSION;
            normalized.video = Some(
                settings
                    .video_edit(self.duration)
                    .expect("validated settings"),
            );
            normalized
        };
        normalize(left) == normalize(right)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn begin_edit_group(&mut self) {
        if self.edit_group_before.is_none() {
            self.edit_group_before = Some(self.snapshot());
        }
    }

    pub fn has_edit_group(&self) -> bool {
        self.edit_group_before.is_some()
    }

    pub fn finish_edit_group(&mut self) -> bool {
        let Some(before) = self.edit_group_before.take() else {
            return false;
        };
        if before.camera == *self.editor.track() && before.settings == self.settings {
            return false;
        }
        self.undo_stack.push(before);
        if self.undo_stack.len() > 100 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        true
    }

    pub fn update_segment(
        &mut self,
        id: &str,
        start_tick: TimeTick,
        end_tick: TimeTick,
        target: CameraState,
    ) -> Result<(), EditorSessionError> {
        let before = self.snapshot();
        self.editor
            .update_segment(id, start_tick, end_tick, target)?;
        self.commit_undo(before);
        Ok(())
    }

    pub fn move_segment(&mut self, id: &str, delta: TimeTick) -> Result<(), EditorSessionError> {
        let before = self.snapshot();
        self.editor.move_segment(id, delta)?;
        self.commit_undo(before);
        Ok(())
    }

    pub fn update_segment_focus(
        &mut self,
        id: &str,
        start_tick: TimeTick,
        end_tick: TimeTick,
        focus: panzo_core::NormalizedPoint,
        scale: f64,
    ) -> Result<(), EditorSessionError> {
        let before = self.snapshot();
        self.editor
            .update_segment_focus(id, start_tick, end_tick, focus, scale)?;
        self.commit_undo(before);
        Ok(())
    }

    pub fn set_segment_focus(
        &mut self,
        id: &str,
        focus: panzo_core::NormalizedPoint,
        scale: f64,
    ) -> Result<(), EditorSessionError> {
        let segment = self
            .camera()
            .segments
            .iter()
            .find(|segment| segment.id == id)
            .ok_or_else(|| CameraEditError::UnknownSegment(id.into()))?;
        self.update_segment_focus(id, segment.start_tick, segment.end_tick, focus, scale)
    }

    pub fn set_segment_target(
        &mut self,
        id: &str,
        target: CameraState,
    ) -> Result<(), EditorSessionError> {
        let before = self.snapshot();
        self.editor.set_segment_target(id, target)?;
        self.commit_undo(before);
        Ok(())
    }

    pub fn add_segment(
        &mut self,
        id: impl Into<String>,
        kind: CameraSegmentKind,
        start_tick: TimeTick,
        end_tick: TimeTick,
        target: CameraState,
    ) -> Result<String, EditorSessionError> {
        let before = self.snapshot();
        let mut track = self.camera().clone();
        if !self.settings.camera_enabled {
            track.segments.clear();
            track.base_state = CameraState::BASE;
        }
        let mut candidate = CameraTrackEditor::new(track, self.duration)?.without_history();
        let id = candidate.add_segment(id, kind, start_tick, end_tick, target)?;
        self.editor = candidate;
        self.settings.camera_enabled = true;
        self.settings.schema_version = panzo_core::WORKBENCH_SCHEMA_VERSION;
        self.commit_undo(before);
        Ok(id)
    }

    pub fn delete_segment(&mut self, id: &str) -> Result<(), EditorSessionError> {
        let before = self.snapshot();
        self.editor.delete_segment(id)?;
        self.commit_undo(before);
        Ok(())
    }

    pub fn undo(&mut self) -> bool {
        self.finish_edit_group();
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };
        let current = self.snapshot();
        self.restore(previous);
        self.redo_stack.push(current);
        true
    }

    pub fn redo(&mut self) -> bool {
        self.finish_edit_group();
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        let current = self.snapshot();
        self.restore(next);
        self.undo_stack.push(current);
        true
    }

    pub fn set_solid_background(
        &mut self,
        color: impl Into<String>,
    ) -> Result<(), EditorSessionError> {
        let before = self.snapshot();
        let mut settings = self.settings.clone();
        settings.background.kind = BackgroundKind::Solid;
        settings.background.color = color.into();
        settings.background.image = None;
        settings.validate()?;
        self.settings = settings;
        self.commit_undo(before);
        Ok(())
    }

    pub fn set_background_style(
        &mut self,
        background: panzo_core::BackgroundSettings,
    ) -> Result<(), EditorSessionError> {
        let before = self.snapshot();
        let mut settings = self.settings.clone();
        settings.background = background;
        settings.validate()?;
        self.settings = settings;
        self.commit_undo(before);
        Ok(())
    }

    pub fn import_background_image(
        &mut self,
        source_path: impl AsRef<Path>,
    ) -> Result<ImportedBackground, EditorSessionError> {
        let source_path = source_path.as_ref();
        let extension = normalized_extension(source_path)?;
        let bytes = crate::platform::background_image::read_background_bytes(source_path)?;
        let decoded = decode_background_bytes(&bytes, source_path)?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let relative = format!("assets/background-{}.{}", &hash[..16], extension);
        let destination = self.project_root.join(Path::new(&relative));
        if !destination.is_file() {
            let assets = self.project_root.join("assets");
            fs::create_dir_all(&assets).map_err(|source| EditorSessionError::CreateDirectory {
                path: assets,
                source,
            })?;
            persist_asset_atomically(&destination, &bytes)?;
        }

        let before = self.snapshot();
        let mut settings = self.settings.clone();
        settings.background.kind = BackgroundKind::Image;
        settings.background.image = Some(relative.clone());
        settings.validate()?;
        self.settings = settings;
        self.commit_undo(before);
        Ok(ImportedBackground {
            project_path: relative,
            absolute_path: destination,
            width: decoded.width,
            height: decoded.height,
        })
    }

    pub fn set_canvas_inset(&mut self, inset: f64) -> Result<(), EditorSessionError> {
        let before = self.snapshot();
        let mut settings = self.settings.clone();
        settings.canvas.inset = inset;
        settings.validate()?;
        self.settings = settings;
        self.commit_undo(before);
        Ok(())
    }

    pub fn set_canvas_size(
        &mut self,
        size: Option<panzo_core::CanvasSize>,
    ) -> Result<(), EditorSessionError> {
        let before = self.snapshot();
        let mut settings = self.settings.clone();
        settings.schema_version = panzo_core::WORKBENCH_SCHEMA_VERSION;
        settings.canvas.size = size;
        settings.validate()?;
        self.settings = settings;
        self.commit_undo(before);
        Ok(())
    }

    pub fn set_corner_radius(&mut self, radius: f64) -> Result<(), EditorSessionError> {
        let before = self.snapshot();
        let mut settings = self.settings.clone();
        settings.canvas.corner_radius = radius;
        settings.validate()?;
        self.settings = settings;
        self.commit_undo(before);
        Ok(())
    }

    pub fn set_shadow(&mut self, shadow: f64) -> Result<(), EditorSessionError> {
        let before = self.snapshot();
        let mut settings = self.settings.clone();
        settings.canvas.shadow = shadow;
        settings.validate()?;
        self.settings = settings;
        self.commit_undo(before);
        Ok(())
    }

    pub fn reset_to_auto_draft(&mut self) -> Result<(), EditorSessionError> {
        let before = self.snapshot();
        let mut stabilized = self.auto_draft.clone();
        stabilized.stabilize_auto_pans();
        self.editor = CameraTrackEditor::new(stabilized, self.duration)?.without_history();
        self.settings.camera_enabled = true;
        self.settings.schema_version = panzo_core::WORKBENCH_SCHEMA_VERSION;
        self.commit_undo(before);
        Ok(())
    }

    pub fn save(
        &mut self,
        kind: CameraEditKind,
        segment_id: Option<String>,
    ) -> Result<EditorSaveReport, EditorSessionError> {
        self.save_with_transaction_hook(kind, segment_id, |_| Ok(()))
    }

    pub fn acknowledge_background_save(&mut self, saved: &Self) {
        // Keep edits made while the immutable save snapshot was being written.
        let matches_saved = self.editor.track() == saved.editor.track()
            && self.settings_content_equal(&self.settings, &saved.settings);
        self.saved_camera = saved.saved_camera.clone();
        self.saved_settings = saved.saved_settings.clone();
        self.settings.revision = saved.saved_settings.revision;
        if matches_saved {
            self.settings = saved.settings.clone();
        }
    }

    /// Acceptance-only crash injection. The process exits without unwinding immediately after
    /// the requested durable transaction stage.
    pub fn save_with_crash_injection(
        &mut self,
        kind: CameraEditKind,
        segment_id: Option<String>,
        crash_stage: EditorTransactionStage,
    ) -> Result<EditorSaveReport, EditorSessionError> {
        self.save_with_transaction_hook(kind, segment_id, |stage| {
            if stage == crash_stage {
                std::process::exit(86);
            }
            Ok(())
        })
    }

    fn save_with_transaction_hook(
        &mut self,
        kind: CameraEditKind,
        segment_id: Option<String>,
        hook: impl FnMut(EditorTransactionStage) -> Result<(), EditorTransactionError>,
    ) -> Result<EditorSaveReport, EditorSessionError> {
        self.finish_edit_group();
        self.editor.track().validate()?;
        self.settings.validate()?;
        let edit_dir = self.project_root.join("edit");
        fs::create_dir_all(&edit_dir).map_err(|source| EditorSessionError::CreateDirectory {
            path: edit_dir,
            source,
        })?;

        let before_hash = self.saved_camera.canonical_hash()?;
        let camera = self.editor.track().clone();
        let after_hash = camera.canonical_hash()?;
        let mut settings = self.settings.clone();
        settings.revision = self.saved_settings.revision.saturating_add(1);
        settings.schema_version = panzo_core::WORKBENCH_SCHEMA_VERSION;
        settings.video = Some(self.video_edit());
        settings.validate()?;

        let record = CameraEditRecord {
            revision: settings.revision,
            kind,
            segment_id,
            before_hash,
            after_hash: after_hash.clone(),
        };
        let history = history_with_record(&self.history_path, &record)?;
        let transaction = commit_editor_transaction_with_hook(
            EditorTransactionRequest {
                project_root: &self.project_root,
                camera_path: &self.camera_path,
                camera: &camera,
                auto_draft: (!self.auto_camera_path.is_file()).then_some(&self.auto_draft),
                settings: &settings,
                history: &history,
                revision: settings.revision,
            },
            hook,
        )?;

        self.saved_camera = camera;
        self.saved_settings = settings.clone();
        self.settings = settings;
        Ok(EditorSaveReport {
            project_root: self.project_root.clone(),
            revision: self.settings.revision,
            transaction_id: transaction.transaction_id,
            camera_path: self.camera_path.clone(),
            auto_camera_path: self.auto_camera_path.clone(),
            workbench_path: self.workbench_path.clone(),
            auto_draft_created: transaction.auto_draft_created,
            camera_hash: after_hash,
            segment_count: self.editor.track().segments.len(),
        })
    }

    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            camera: self.editor.track().clone(),
            settings: self.settings.clone(),
        }
    }

    fn restore(&mut self, snapshot: EditorSnapshot) {
        self.editor = CameraTrackEditor::new(snapshot.camera, self.duration)
            .expect("editor history only stores validated Camera Tracks")
            .without_history();
        self.settings = snapshot.settings;
        self.settings.revision = self.saved_settings.revision;
    }

    fn commit_undo(&mut self, before: EditorSnapshot) {
        if before.camera == *self.editor.track() && before.settings == self.settings {
            return;
        }
        if self.edit_group_before.is_some() {
            // A draft is not a committed command. Escape must preserve the redo branch.
            return;
        }
        self.undo_stack.push(before);
        if self.undo_stack.len() > 100 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }
}

pub fn load_effective_edit_state(
    project_root: impl AsRef<Path>,
) -> Result<(CameraTrack, WorkbenchSettings), EditorSessionError> {
    let layout = ProjectLayout::open(project_root)?;
    let _lock = lock_project_edit(layout.root())?;
    recover_pending_editor_transaction_locked(layout.root())?;
    let manifest = layout.load_manifest()?;
    let mut camera: CameraTrack = read_json(&layout.root().join(manifest.tracks.camera))?;
    camera.validate()?;
    camera.stabilize_auto_pans();
    let workbench_path = layout.root().join(WORKBENCH_FILE);
    let settings = if workbench_path.is_file() {
        read_json(&workbench_path)?
    } else {
        WorkbenchSettings::default()
    };
    settings.validate()?;
    settings.video_edit(TimeTick(manifest.media.duration_tick))?;
    Ok((camera, settings))
}

pub fn load_effective_background(
    project_root: impl AsRef<Path>,
    settings: &WorkbenchSettings,
) -> Result<Option<DecodedBackgroundImage>, EditorSessionError> {
    settings.validate()?;
    if settings.background.kind == BackgroundKind::Solid {
        return Ok(None);
    }
    let relative = settings
        .background
        .image
        .as_deref()
        .ok_or(WorkbenchValidationError::MissingBackgroundImage)?;
    let root = project_root.as_ref().canonicalize().map_err(|source| {
        EditorSessionError::ResolveBackground {
            path: project_root.as_ref().into(),
            source,
        }
    })?;
    let path = root.join(relative);
    let resolved = path
        .canonicalize()
        .map_err(|source| EditorSessionError::ResolveBackground {
            path: path.clone(),
            source,
        })?;
    if !resolved.starts_with(&root) {
        return Err(EditorSessionError::BackgroundEscapesProject(resolved));
    }
    Ok(Some(decode_background_file(resolved)?))
}

fn persist_asset_atomically(path: &Path, bytes: &[u8]) -> Result<(), EditorSessionError> {
    let temporary = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("asset"),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let mut file =
            fs::File::create(&temporary).map_err(|source| EditorSessionError::WriteAsset {
                path: temporary.clone(),
                source,
            })?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|source| EditorSessionError::WriteAsset {
                path: temporary.clone(),
                source,
            })?;
        fs::rename(&temporary, path).map_err(|source| EditorSessionError::WriteAsset {
            path: path.into(),
            source,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn write_acceptance_bmp(path: &Path) -> Result<(), EditorSessionError> {
    let parent = path.parent().expect("acceptance background has a parent");
    fs::create_dir_all(parent).map_err(|source| EditorSessionError::CreateDirectory {
        path: parent.into(),
        source,
    })?;
    let mut bytes = vec![0_u8; 54 + 4 * 2 * 2];
    let file_length = bytes.len() as u32;
    bytes[0..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&file_length.to_le_bytes());
    bytes[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&2_i32.to_le_bytes());
    bytes[22..26].copy_from_slice(&2_i32.to_le_bytes());
    bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&32_u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&16_u32.to_le_bytes());
    for (index, pixel) in bytes[54..].chunks_exact_mut(4).enumerate() {
        pixel.copy_from_slice(if index.is_multiple_of(2) {
            &[0xD8, 0x78, 0x34, 0xFF]
        } else {
            &[0x34, 0x78, 0xD8, 0xFF]
        });
    }
    fs::write(path, bytes).map_err(|source| EditorSessionError::WriteAsset {
        path: path.into(),
        source,
    })
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, EditorSessionError> {
    let bytes = fs::read(path).map_err(|source| EditorSessionError::Read {
        path: path.into(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(EditorSessionError::Deserialize)
}

fn history_with_record(
    path: &Path,
    record: &CameraEditRecord,
) -> Result<Vec<u8>, EditorSessionError> {
    let mut history = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(source) => {
            return Err(EditorSessionError::Read {
                path: path.into(),
                source,
            });
        }
    };
    if !history.is_empty() && !history.ends_with(b"\n") {
        history.push(b'\n');
    }
    history.extend(serde_json::to_vec(record)?);
    history.push(b'\n');
    Ok(history)
}

fn read_click_ticks(path: &Path) -> Result<Vec<TimeTick>, EditorSessionError> {
    let file = fs::File::open(path).map_err(|source| EditorSessionError::Read {
        path: path.into(),
        source,
    })?;
    let mut ticks = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|source| EditorSessionError::Read {
            path: path.into(),
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let click: ClickEvent = serde_json::from_str(&line).map_err(|source| {
            EditorSessionError::InvalidClickRecord {
                path: path.into(),
                line: index + 1,
                source,
            }
        })?;
        ticks.push(click.time_tick);
    }
    Ok(ticks)
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Error)]
pub enum EditorSessionError {
    #[error(transparent)]
    Video(#[from] panzo_core::VideoEditError),
    #[error(transparent)]
    Project(#[from] ProjectIoError),
    #[error("project state is not editable: {0:?}")]
    ProjectNotEditable(ProjectState),
    #[error(transparent)]
    Camera(#[from] panzo_core::camera::CameraValidationError),
    #[error(transparent)]
    Edit(#[from] CameraEditError),
    #[error(transparent)]
    Settings(#[from] WorkbenchValidationError),
    #[error(transparent)]
    BackgroundImage(#[from] BackgroundImageError),
    #[error(transparent)]
    Transaction(#[from] EditorTransactionError),
    #[error("failed to create editor directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read editor file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid editor JSON: {0}")]
    Deserialize(serde_json::Error),
    #[error("invalid click JSONL record {path}:{line}: {source}")]
    InvalidClickRecord {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to serialize editor JSON: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("failed to open edit history {path}: {source}")]
    OpenHistory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write edit history {path}: {source}")]
    WriteHistory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write Project background asset {path}: {source}")]
    WriteAsset {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to resolve Project background asset {path}: {source}")]
    ResolveBackground {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("background asset resolves outside the Project: {0}")]
    BackgroundEscapesProject(PathBuf),
    #[error("editor atomic probe requires at least one Camera Segment")]
    AtomicProbeMissingSegment,
}

/// UI descriptions of existing edit validation; internal diagnostics retain the original error.
pub(crate) fn user_edit_error(error: &EditorSessionError) -> String {
    use panzo_core::{CameraEditError, VideoEditError, WorkbenchValidationError};
    match error {
        EditorSessionError::Video(error) => match error {
            VideoEditError::Empty => "需要保留至少一个视频片段，无法删除最后一段",
            VideoEditError::InvalidRange => "时间范围无效，请确保入点早于出点且位于原视频范围内",
            VideoEditError::MissingClip => "所选视频片段已变化，请重新选择片段",
            VideoEditError::InvalidSpeed => "视频速度应为 0.25–4 倍，请修改后再应用",
            VideoEditError::InvalidCrop => "裁剪后宽高至少保留原画面的 5%，请减小裁剪量",
            _ => "视频调整未应用，请缩小调整范围后重试",
        }
        .into(),
        EditorSessionError::Edit(error) => match error {
            CameraEditError::OverlappingPrevious | CameraEditError::OverlappingNext => {
                "镜头不能与相邻镜头重叠，请缩短范围或选择空白位置"
            }
            CameraEditError::UnknownSegment(_) => "所选镜头已变化，请重新选择镜头",
            CameraEditError::InvalidSegmentRange { .. }
            | CameraEditError::SegmentAfterProjectEnd { .. } => {
                "镜头开始时间应早于结束时间，且不能超出视频时长"
            }
            _ => "镜头调整未应用，请检查时间范围、倍率和焦点位置",
        }
        .into(),
        EditorSessionError::Settings(error) => match error {
            WorkbenchValidationError::InvalidColor(_) => {
                "请输入有效的十六进制颜色，例如 #AABBCC".into()
            }
            WorkbenchValidationError::InvalidCanvasSize => {
                "宽高须为 64–8192 的偶数，总像素不能超过 33554432".into()
            }
            WorkbenchValidationError::InvalidRange {
                name,
                minimum,
                maximum,
                ..
            } => match *name {
                "canvas.inset" => "画布边距应为 0–25%，请修改后再应用".into(),
                "canvas.cornerRadius" => "圆角不能超过画布短边的 10%，请减小像素值".into(),
                "canvas.shadow" => "阴影应为 0–100%，请修改后再应用".into(),
                "cursor.scale" => "光标大小应为 0.25–4 倍，请修改后再应用".into(),
                _ => format!("数值应为 {minimum}–{maximum}，请修改后再应用"),
            },
            _ => "参数未应用，请检查输入或重新选择背景图片".into(),
        },
        EditorSessionError::BackgroundImage(_) => "无法读取这张图片，请选择可正常打开的图片".into(),
        _ => "本次修改未完成，请保留当前工程并重试；仍失败时可查看设置中的诊断详情".into(),
    }
}

#[derive(Debug, Error)]
pub enum EditorAcceptanceError {
    #[error(transparent)]
    Session(#[from] EditorSessionError),
    #[error("acceptance Project has no Camera Segment to edit")]
    NoCameraSegment,
    #[error("edited Camera Segment disappeared after reopening")]
    EditedSegmentMissing,
    #[error("saved editor state did not round-trip exactly")]
    PersistenceMismatch,
    #[error("failed to hash Camera Track: {0}")]
    Hash(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::editor_transaction::{EDIT_TRANSACTION_DIRECTORY, EDIT_TRANSACTION_FILE};
    use panzo_core::camera::{
        CAMERA_SCHEMA_VERSION, Easing, GeneratorMetadata, SegmentOrigin, SourceRevision,
    };
    use panzo_core::project::{CaptureKind, TimebaseDescriptor};
    use panzo_core::{
        CameraSegment, CaptureDescriptor, PhysicalRect, PhysicalSize, ProjectManifest,
        TICKS_PER_SECOND,
    };
    use uuid::Uuid;

    pub(super) struct TestProject(pub(super) PathBuf);

    impl TestProject {
        pub(super) fn new() -> Self {
            let root = std::env::temp_dir().join(format!("panzo-editor-{}.panzo", Uuid::new_v4()));
            fs::create_dir_all(root.join("tracks")).unwrap();
            fs::create_dir_all(root.join("events")).unwrap();
            fs::create_dir_all(root.join("media")).unwrap();
            let manifest = manifest();
            persist_json_atomically(root.join("project.json"), &manifest).unwrap();
            persist_json_atomically(root.join("tracks/camera.json"), &camera()).unwrap();
            fs::write(root.join("media/screen.mp4"), b"immutable-media").unwrap();
            fs::write(root.join("events/cursor.jsonl"), b"").unwrap();
            fs::write(root.join("events/clicks.jsonl"), b"").unwrap();
            Self(root)
        }
    }

    impl Drop for TestProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn manifest() -> ProjectManifest {
        let mut manifest = ProjectManifest::new_v01(
            Uuid::new_v4(),
            "test",
            "2026-09-01T00:00:00Z",
            TimebaseDescriptor {
                ticks_per_second: TICKS_PER_SECOND,
                session_start_qpc: 1,
                qpc_frequency: TICKS_PER_SECOND,
            },
            CaptureDescriptor {
                kind: CaptureKind::Monitor,
                monitor_id: "test".into(),
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
        manifest.state = ProjectState::Ready;
        manifest.media.screen = "media/screen.mp4".into();
        manifest.media.duration_tick = 5 * TICKS_PER_SECOND;
        manifest
    }

    fn camera() -> CameraTrack {
        CameraTrack {
            schema_version: CAMERA_SCHEMA_VERSION,
            generator: GeneratorMetadata {
                name: "test".into(),
                version: "1".into(),
                parameter_set: "default".into(),
            },
            source_revision: SourceRevision {
                click_track_hash: "clicks".into(),
                capture_geometry_hash: "geometry".into(),
            },
            base_state: CameraState::BASE,
            segments: vec![CameraSegment {
                id: "zoom".into(),
                kind: CameraSegmentKind::ZoomIn,
                start_tick: TimeTick::from_millis(1_000),
                end_tick: TimeTick::from_millis(1_500),
                from: CameraState::BASE,
                to: CameraState {
                    center_x: 0.5,
                    center_y: 0.5,
                    scale: 1.6,
                },
                easing: Easing::Smootherstep,
                origin: SegmentOrigin::Auto,
                source_click_ids: vec!["click".into()],
                locked: false,
                user_modified: false,
                focus: None,
            }],
        }
    }

    #[test]
    fn clip_speed_round_trips_save_undo_and_legacy_schema_two() {
        let project = TestProject::new();
        let source_hashes = immutable_source_hashes(&project.0);
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        let right = session.split_video(TimeTick::from_millis(2000)).unwrap();
        session.save(CameraEditKind::Update, None).unwrap();
        // A real schema-two payload has no speed field.
        let path = project.0.join(WORKBENCH_FILE);
        let mut legacy = serde_json::to_value(session.settings()).unwrap();
        legacy["schemaVersion"] = 2.into();
        for clip in legacy["video"]["clips"].as_array_mut().unwrap() {
            clip.as_object_mut().unwrap().remove("speedPercent");
        }
        persist_json_atomically(&path, &legacy).unwrap();
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        assert_eq!(session.settings().schema_version, 2);
        assert_eq!(session.video_edit().clips[1].speed_percent, 100);
        session.begin_edit_group();
        for speed in [50, 75, 200] {
            session.set_video_speed(&right, speed).unwrap();
        }
        session.finish_edit_group();
        assert_eq!(session.project_duration(), TimeTick::from_millis(3500));
        assert!(session.undo());
        assert_eq!(session.project_duration(), TimeTick::from_millis(5000));
        assert!(!session.is_dirty());
        assert!(session.redo());
        session.save(CameraEditKind::Update, None).unwrap();
        let reopened = ProjectEditorSession::open(&project.0).unwrap();
        assert_eq!(reopened.video_edit().clips[1].speed_percent, 200);
        assert_eq!(reopened.project_duration(), TimeTick::from_millis(3500));
        assert_eq!(
            reopened.settings().schema_version,
            panzo_core::WORKBENCH_SCHEMA_VERSION
        );
        assert_eq!(immutable_source_hashes(&project.0), source_hashes);
    }

    #[test]
    fn stale_editor_cannot_overwrite_another_saved_revision() {
        let project = TestProject::new();
        let mut first = ProjectEditorSession::open(&project.0).unwrap();
        let mut stale = ProjectEditorSession::open(&project.0).unwrap();
        first.set_canvas_inset(0.04).unwrap();
        first.save(CameraEditKind::Style, None).unwrap();
        stale.set_shadow(0.5).unwrap();
        assert!(matches!(
            stale.save(CameraEditKind::Style, None),
            Err(EditorSessionError::Transaction(
                EditorTransactionError::RevisionConflict { .. }
            ))
        ));
        assert!(stale.is_dirty());
        let reopened = ProjectEditorSession::open(&project.0).unwrap();
        assert_eq!(reopened.settings().canvas.inset, 0.04);
        assert_eq!(reopened.settings().canvas.shadow, 0.0);
    }

    #[test]
    fn clip_crop_migrates_schema_three_and_round_trips_save_and_undo() {
        let project = TestProject::new();
        let hashes = immutable_source_hashes(&project.0);
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        session.split_video(TimeTick::from_millis(2000)).unwrap();
        session.save(CameraEditKind::Update, None).unwrap();
        let mut legacy = serde_json::to_value(session.settings()).unwrap();
        legacy["schemaVersion"] = 3.into();
        for clip in legacy["video"]["clips"].as_array_mut().unwrap() {
            clip.as_object_mut().unwrap().remove("crop");
        }
        persist_json_atomically(project.0.join(WORKBENCH_FILE), &legacy).unwrap();
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        let original = session.video_edit();
        let id = original.clips[1].id.clone();
        let crop = panzo_core::VideoCrop::centered_aspect(1920, 1080, 1.0);
        session.set_video_crop(&id, crop).unwrap();
        let mut invalid = session.settings().clone();
        invalid.schema_version = 3;
        assert!(invalid.validate().is_err());
        assert!(session.undo());
        assert_eq!(session.video_edit(), original);
        assert!(session.redo());
        session.save(CameraEditKind::Update, None).unwrap();
        let reopened = ProjectEditorSession::open(&project.0).unwrap();
        assert_eq!(reopened.video_edit().clips[1].crop, crop);
        assert_eq!(
            reopened.settings().schema_version,
            panzo_core::WORKBENCH_SCHEMA_VERSION
        );
        assert_eq!(immutable_source_hashes(&project.0), hashes);
    }

    #[test]
    fn recovery_draft_restores_canvas_crop_speed_as_one_undo_without_saving() {
        let project = TestProject::new();
        let hashes = immutable_source_hashes(&project.0);
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        let saved = session.snapshot();
        session
            .set_canvas_size(Some(panzo_core::CanvasSize {
                width: 1080,
                height: 1920,
            }))
            .unwrap();
        session
            .set_video_crop(
                "source-0",
                panzo_core::VideoCrop::centered_aspect(1920, 1080, 1.0),
            )
            .unwrap();
        session.set_video_speed("source-0", 150).unwrap();
        session.write_recovery_draft().unwrap();
        assert!(!project.0.join(WORKBENCH_FILE).exists());
        let mut reopened = ProjectEditorSession::open(&project.0).unwrap();
        assert!(reopened.restore_recovery_draft().unwrap());
        assert_eq!(reopened.settings(), session.settings());
        assert!(reopened.undo());
        assert_eq!(reopened.settings(), &saved.settings);
        assert_eq!(reopened.camera(), &saved.camera);
        assert!(reopened.redo());
        reopened.save(CameraEditKind::Update, None).unwrap();
        reopened.clear_recovery_draft().unwrap();
        let after = ProjectEditorSession::open(&project.0).unwrap();
        assert_eq!(
            after.settings().canvas.size,
            Some(panzo_core::CanvasSize {
                width: 1080,
                height: 1920
            })
        );
        assert_eq!(immutable_source_hashes(&project.0), hashes);
    }

    #[test]
    fn stale_or_corrupt_draft_never_replaces_newer_saved_edits() {
        let project = TestProject::new();
        let mut editor = ProjectEditorSession::open(&project.0).unwrap();
        editor
            .set_canvas_size(Some(panzo_core::CanvasSize {
                width: 1080,
                height: 1080,
            }))
            .unwrap();
        editor.write_recovery_draft().unwrap();
        let stale = editor.background_snapshot();
        editor
            .set_canvas_size(Some(panzo_core::CanvasSize {
                width: 1080,
                height: 1920,
            }))
            .unwrap();
        editor.save(CameraEditKind::Update, None).unwrap();
        assert!(stale.write_recovery_draft().is_err());
        let mut reopened = ProjectEditorSession::open(&project.0).unwrap();
        assert!(reopened.restore_recovery_draft().is_err());
        assert_eq!(reopened.settings(), editor.settings());
        assert!(!reopened.is_dirty());
        assert!(fs::read_dir(project.0.join("edit")).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("recovery-retained-")
        }));
        fs::write(project.0.join("edit/recovery-draft.json"), b"{corrupt").unwrap();
        assert!(reopened.restore_recovery_draft().is_err());
        assert!(!reopened.is_dirty());
    }

    #[test]
    fn invalid_canvas_is_atomic_and_legacy_canvas_remains_original() {
        let project = TestProject::new();
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        let original = session.settings().clone();
        assert!(
            session
                .set_canvas_size(Some(panzo_core::CanvasSize {
                    width: 1919,
                    height: 1080
                }))
                .is_err()
        );
        assert_eq!(session.settings(), &original);
        let mut legacy = original;
        legacy.schema_version = 4;
        assert!(legacy.validate().is_ok());
        legacy.canvas.size = Some(panzo_core::CanvasSize {
            width: 1920,
            height: 1080,
        });
        assert!(legacy.validate().is_err());
    }

    #[test]
    fn selected_edge_focus_survives_save_reopen_undo_and_equal_crop_moves() {
        let project = TestProject::new();
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        let corner = panzo_core::NormalizedPoint::new(0.0, 1.0).unwrap();
        session.set_segment_focus("zoom", corner, 1.6).unwrap();
        session
            .save(CameraEditKind::Update, Some("zoom".into()))
            .unwrap();
        let mut reopened = ProjectEditorSession::open(&project.0).unwrap();
        assert!(!reopened.is_dirty());
        assert_eq!(reopened.camera().segments[0].focus_point(), corner);
        assert_eq!(
            load_effective_edit_state(&project.0).unwrap().0,
            *reopened.camera()
        );
        let crop = reopened.camera().segments[0].to;
        let nearby = panzo_core::NormalizedPoint::new(0.05, 0.95).unwrap();
        reopened.set_segment_focus("zoom", nearby, 1.6).unwrap();
        assert_eq!(reopened.camera().segments[0].to, crop);
        assert!(reopened.is_dirty());
        assert!(reopened.undo());
        assert_eq!(reopened.camera().segments[0].focus_point(), corner);
        assert!(reopened.redo());
        assert_eq!(reopened.camera().segments[0].focus_point(), nearby);
    }

    #[test]
    fn acknowledging_save_does_not_discard_edits_made_during_save() {
        let project = TestProject::new();
        let mut editor = ProjectEditorSession::open(&project.0).unwrap();
        editor.set_canvas_inset(0.04).unwrap();
        let mut snapshot = editor.clone();
        editor.split_video(TimeTick::from_millis(500)).unwrap();
        snapshot.save(CameraEditKind::Style, None).unwrap();
        editor.acknowledge_background_save(&snapshot);
        assert!(editor.is_dirty());
        assert_eq!(editor.video_edit().clips.len(), 2);
        assert!(editor.undo());
        assert!(!editor.is_dirty());
        assert_eq!(editor.settings().revision, 1);
    }

    #[test]
    fn cancelled_draft_preserves_redo_and_saved_content_identity() {
        let project = TestProject::new();
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        session.set_canvas_inset(0.05).unwrap();
        session.save(CameraEditKind::Style, None).unwrap();
        assert!(session.undo());
        session.begin_edit_group();
        session.set_canvas_inset(0.10).unwrap();
        assert!(session.cancel_edit_group());
        assert!(session.can_redo());
        assert!(session.redo());
        assert!(!session.is_dirty());
        assert!(session.undo());
        assert_eq!(
            session.save(CameraEditKind::Style, None).unwrap().revision,
            2
        );
        assert!(!session.is_dirty());
        assert_eq!(
            ProjectEditorSession::open(&project.0)
                .unwrap()
                .settings()
                .revision,
            2
        );
    }

    #[test]
    fn twenty_mixed_commands_round_trip_and_gesture_is_one_command() {
        let project = TestProject::new();
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        let original = session.snapshot();
        for n in 1..=4 {
            session.split_video(TimeTick::from_millis(n * 500)).unwrap();
            session.set_canvas_inset(n as f64 / 100.0).unwrap();
            session
                .set_cursor_style(n % 2 == 0, 1.0 + n as f64 / 10.0)
                .unwrap();
            session.set_camera_enabled(n % 2 == 0);
            session.set_corner_radius(n as f64 / 100.0).unwrap();
        }
        let edited = session.snapshot();
        for _ in 0..20 {
            assert!(session.undo());
        }
        assert!(!session.can_undo());
        assert_eq!(session.camera(), &original.camera);
        assert!(session.settings_content_equal(session.settings(), &original.settings));
        for _ in 0..20 {
            assert!(session.redo());
        }
        assert!(!session.can_redo());
        assert_eq!(session.camera(), &edited.camera);
        assert!(session.settings_content_equal(session.settings(), &edited.settings));
        session.begin_edit_group();
        for n in 1..=30 {
            session.set_canvas_inset(f64::from(n) / 200.0).unwrap();
        }
        assert!(session.finish_edit_group());
        assert!(session.undo());
        assert!(session.settings_content_equal(session.settings(), &edited.settings));
    }

    #[test]
    fn legacy_schema_upgrades_only_on_save_and_unknown_schema_is_rejected() {
        let project = TestProject::new();
        let legacy = WorkbenchSettings {
            schema_version: 1,
            ..WorkbenchSettings::default()
        };
        fs::create_dir_all(project.0.join("edit")).unwrap();
        let path = project.0.join(WORKBENCH_FILE);
        persist_json_atomically(&path, &legacy).unwrap();
        let bytes = fs::read(&path).unwrap();
        let sources = immutable_source_hashes(&project.0);
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(!session.is_dirty());
        assert_eq!(session.video_edit().clips.len(), 1);
        session.save(CameraEditKind::Style, None).unwrap();
        let reopened = ProjectEditorSession::open(&project.0).unwrap();
        assert_eq!(
            reopened.settings().schema_version,
            panzo_core::WORKBENCH_SCHEMA_VERSION
        );
        assert!(reopened.settings().video.is_some());
        assert_eq!(immutable_source_hashes(&project.0), sources);
        let mut unknown = reopened.settings().clone();
        unknown.schema_version = 999;
        persist_json_atomically(&path, &unknown).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(ProjectEditorSession::open(&project.0).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn first_save_preserves_auto_draft_and_reopens_edits() {
        let project = TestProject::new();
        let original_hash = camera().canonical_hash().unwrap();
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        session
            .set_segment_target(
                "zoom",
                CameraState {
                    center_x: 0.5,
                    center_y: 0.5,
                    scale: 2.0,
                },
            )
            .unwrap();
        session.set_solid_background("#223344").unwrap();
        session.set_canvas_inset(0.08).unwrap();
        let report = session
            .save(CameraEditKind::Update, Some("zoom".into()))
            .unwrap();
        assert!(report.auto_draft_created);
        assert_ne!(report.camera_hash, original_hash);

        let reopened = ProjectEditorSession::open(&project.0).unwrap();
        assert_eq!(reopened.camera().segments[0].to.scale, 2.0);
        assert_eq!(reopened.settings().canvas.inset, 0.08);
        assert_eq!(reopened.auto_draft.canonical_hash().unwrap(), original_hash);
        assert!(!reopened.is_dirty());
    }

    #[test]
    fn legacy_micro_pan_is_shared_by_preview_and_editor_and_persists_only_on_save() {
        let project = TestProject::new();
        let mut original = camera();
        let template = original.segments[0].clone();
        let focus = template.to;
        let moved = CameraState {
            center_x: 0.51,
            ..focus
        };
        original.segments.push(CameraSegment {
            id: "tiny-pan".into(),
            kind: CameraSegmentKind::Pan,
            start_tick: TimeTick::from_millis(2000),
            end_tick: TimeTick::from_millis(2350),
            from: focus,
            to: moved,
            ..template.clone()
        });
        original.segments.push(CameraSegment {
            id: "zoom-out".into(),
            kind: CameraSegmentKind::ZoomOut,
            start_tick: TimeTick::from_millis(3500),
            end_tick: TimeTick::from_millis(3950),
            from: moved,
            to: CameraState::BASE,
            ..template
        });
        let path = project.0.join("tracks/camera.json");
        persist_json_atomically(&path, &original).unwrap();
        let before = fs::read(&path).unwrap();
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        assert!(session.is_dirty());
        assert_eq!(session.camera().segments.len(), 2);
        assert_eq!(
            session.camera(),
            &load_effective_edit_state(&project.0).unwrap().0
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        let stabilized = session.camera().clone();
        session.set_solid_background("#112233").unwrap();
        assert!(session.undo());
        assert_eq!(session.camera(), &stabilized);
        assert!(session.redo());
        session.save(CameraEditKind::Update, None).unwrap();
        let auto: CameraTrack = read_json(&project.0.join(AUTO_CAMERA_FILE)).unwrap();
        assert_eq!(auto, original);
        let mut reopened = ProjectEditorSession::open(&project.0).unwrap();
        assert!(!reopened.is_dirty());
        assert_eq!(reopened.camera(), &stabilized);
        reopened.reset_to_auto_draft().unwrap();
        assert_eq!(reopened.camera(), &stabilized);
    }

    #[test]
    fn later_saves_never_replace_auto_draft() {
        let project = TestProject::new();
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        session.set_canvas_inset(0.05).unwrap();
        session.save(CameraEditKind::Style, None).unwrap();
        let auto = fs::read(project.0.join(AUTO_CAMERA_FILE)).unwrap();
        let auto_hash = sha256(&auto);
        session.set_canvas_inset(0.10).unwrap();
        let report = session.save(CameraEditKind::Style, None).unwrap();
        assert!(!report.auto_draft_created);
        assert_eq!(
            sha256(&fs::read(project.0.join(AUTO_CAMERA_FILE)).unwrap()),
            auto_hash
        );
    }

    #[test]
    fn undo_and_redo_cover_camera_and_style_edits_in_order() {
        let project = TestProject::new();
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        session.set_canvas_inset(0.05).unwrap();
        session
            .set_segment_target(
                "zoom",
                CameraState {
                    center_x: 0.5,
                    center_y: 0.5,
                    scale: 2.0,
                },
            )
            .unwrap();
        assert_eq!(session.camera().segments[0].to.scale, 2.0);
        assert!(session.undo());
        assert_eq!(session.camera().segments[0].to.scale, 1.6);
        assert_eq!(session.settings().canvas.inset, 0.05);
        assert!(session.undo());
        assert_eq!(session.settings().canvas.inset, 0.0);
        assert!(session.redo());
        assert_eq!(session.settings().canvas.inset, 0.05);
        assert!(session.redo());
        assert_eq!(session.camera().segments[0].to.scale, 2.0);
    }

    #[test]
    fn background_import_is_project_owned_and_round_trips_through_history() {
        let project = TestProject::new();
        let source = project.0.join("input.bmp");
        write_acceptance_bmp(&source).unwrap();
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        let imported = session.import_background_image(&source).unwrap();
        assert!(imported.absolute_path.starts_with(project.0.join("assets")));
        assert_eq!((imported.width, imported.height), (2, 2));
        assert_eq!(session.settings().background.kind, BackgroundKind::Image);
        assert!(session.undo());
        assert_eq!(session.settings().background.kind, BackgroundKind::Solid);
        assert!(session.redo());
        session.save(CameraEditKind::Style, None).unwrap();

        let reopened = ProjectEditorSession::open(&project.0).unwrap();
        let decoded = load_effective_background(&project.0, reopened.settings())
            .unwrap()
            .unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 2));
    }

    #[test]
    fn edit_group_coalesces_many_updates_into_one_undo_state() {
        let project = TestProject::new();
        let mut session = ProjectEditorSession::open(&project.0).unwrap();
        session.begin_edit_group();
        for scale in [1.7, 1.8, 1.9, 2.0] {
            session
                .set_segment_target(
                    "zoom",
                    CameraState {
                        center_x: 0.5,
                        center_y: 0.5,
                        scale,
                    },
                )
                .unwrap();
        }
        assert!(session.finish_edit_group());
        assert_eq!(session.camera().segments[0].to.scale, 2.0);
        assert!(session.undo());
        assert_eq!(session.camera().segments[0].to.scale, 1.6);
        assert!(!session.undo());
        assert!(session.redo());
        assert_eq!(session.camera().segments[0].to.scale, 2.0);
    }

    #[test]
    fn every_transaction_crash_stage_recovers_one_complete_revision() {
        for crash_stage in EditorTransactionStage::ALL {
            let project = TestProject::new();
            let source_hashes = immutable_source_hashes(&project.0);
            let mut session = ProjectEditorSession::open(&project.0).unwrap();
            session
                .set_segment_target(
                    "zoom",
                    CameraState {
                        center_x: 0.5,
                        center_y: 0.5,
                        scale: 2.0,
                    },
                )
                .unwrap();
            session.set_solid_background("#223344").unwrap();
            let middle = session.split_video(TimeTick::from_millis(1_000)).unwrap();
            session.split_video(TimeTick::from_millis(2_000)).unwrap();
            session.delete_video(&middle).unwrap();
            session.set_camera_enabled(false);
            session.set_cursor_style(false, 1.5).unwrap();
            let result = session.save_with_transaction_hook(
                CameraEditKind::Update,
                Some("zoom".into()),
                |stage| {
                    if stage == crash_stage {
                        Err(EditorTransactionError::FaultInjected(stage))
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(
                result.is_err(),
                "fault stage {crash_stage} did not stop Save"
            );

            let recovered = ProjectEditorSession::open(&project.0).unwrap();
            if crash_stage.committed() {
                assert_eq!(recovered.camera().segments[0].to.scale, 2.0);
                assert_eq!(recovered.settings().background.color, "#223344");
                assert_eq!(recovered.settings().revision, 1);
                assert_eq!(recovered.project_duration(), TimeTick::from_millis(4_000));
                assert_eq!(recovered.video_edit().clips.len(), 2);
                assert!(!recovered.settings().camera_enabled);
                assert!(!recovered.settings().cursor.visible);
                assert_eq!(recovered.settings().cursor.scale, 1.5);
                let auto: CameraTrack = read_json(&project.0.join(AUTO_CAMERA_FILE)).unwrap();
                assert_eq!(auto, camera());
                let history = fs::read_to_string(project.0.join(EDIT_HISTORY_FILE)).unwrap();
                let records: Vec<CameraEditRecord> = history
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
                assert_eq!(records.len(), 1);
                assert_eq!(records[0].revision, 1);
            } else {
                assert_eq!(recovered.camera(), &camera());
                assert_eq!(recovered.settings(), &WorkbenchSettings::default());
                assert!(!project.0.join(AUTO_CAMERA_FILE).exists());
                assert!(!project.0.join(EDIT_HISTORY_FILE).exists());
            }
            assert_eq!(immutable_source_hashes(&project.0), source_hashes);
            assert!(!project.0.join(EDIT_TRANSACTION_FILE).exists());
            assert!(!project.0.join(EDIT_TRANSACTION_DIRECTORY).exists());
        }
    }

    fn immutable_source_hashes(project_root: &Path) -> [String; 3] {
        [
            sha256(&fs::read(project_root.join("media/screen.mp4")).unwrap()),
            sha256(&fs::read(project_root.join("events/cursor.jsonl")).unwrap()),
            sha256(&fs::read(project_root.join("events/clicks.jsonl")).unwrap()),
        ]
    }
}
