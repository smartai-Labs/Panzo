use super::{
    CameraTrack, CameraTrackEditor, Digest, EditorSnapshot, ProjectEditorSession, ProjectLayout,
    Sha256, TimeTick, WorkbenchSettings, fs, load_effective_background, lock_project_edit,
    read_json,
};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

const DRAFT_FILE: &str = "edit/recovery-draft.json";
const MAX_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryDraft {
    schema_version: u32,
    project_id: uuid::Uuid,
    duration: TimeTick,
    base_hash: String,
    content_hash: String,
    state: EditorSnapshot,
}

fn hash<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_vec(value)
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        .map_err(|e| e.to_string())
}

impl ProjectEditorSession {
    fn saved_fingerprint(&self) -> Result<String, String> {
        hash(&(&self.saved_camera, &self.saved_settings, self.duration))
    }

    pub fn draft_fingerprint(&self) -> Result<String, String> {
        hash(&(self.saved_fingerprint()?, self.snapshot()))
    }

    pub fn write_recovery_draft(&self) -> Result<(), String> {
        let _lock = lock_project_edit(&self.project_root).map_err(|e| e.to_string())?;
        let layout = ProjectLayout::open(&self.project_root).map_err(|e| e.to_string())?;
        let manifest = layout.load_manifest().map_err(|e| e.to_string())?;
        let camera: CameraTrack = read_json(&self.camera_path).map_err(|e| e.to_string())?;
        let settings: WorkbenchSettings = if self.workbench_path.is_file() {
            read_json(&self.workbench_path).map_err(|e| e.to_string())?
        } else {
            WorkbenchSettings::default()
        };
        if hash(&(&camera, &settings, self.duration))? != self.saved_fingerprint()? {
            return Err("工程已由其他保存操作更新，等待下一次草稿同步".into());
        }
        if !self.is_dirty() {
            return self.clear_recovery_draft();
        }
        let state = self.snapshot();
        let draft = RecoveryDraft {
            schema_version: 1,
            project_id: manifest.project_id,
            duration: self.duration,
            base_hash: self.saved_fingerprint()?,
            content_hash: hash(&state)?,
            state,
        };
        let bytes = serde_json::to_vec(&draft).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("编辑恢复副本超出大小限制".into());
        }
        fs::create_dir_all(self.project_root.join("edit")).map_err(|e| e.to_string())?;
        crate::platform::manifest_store::persist_bytes_atomically(
            self.project_root.join(DRAFT_FILE),
            &bytes,
        )
        .map_err(|e| e.to_string())
    }

    pub fn clear_recovery_draft(&self) -> Result<(), String> {
        match fs::remove_file(self.project_root.join(DRAFT_FILE)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Restores an unsaved revision into memory as one undoable action. Saved files stay intact.
    pub fn restore_recovery_draft(&mut self) -> Result<bool, String> {
        let path = self.project_root.join(DRAFT_FILE);
        let metadata = match fs::metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.to_string()),
        };
        let restore = (|| {
            if metadata.len() > MAX_BYTES {
                return Err("恢复副本过大".into());
            }
            let draft: RecoveryDraft =
                serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            let manifest = ProjectLayout::open(&self.project_root)
                .map_err(|e| e.to_string())?
                .load_manifest()
                .map_err(|e| e.to_string())?;
            if draft.schema_version != 1
                || draft.project_id != manifest.project_id
                || draft.duration != self.duration
                || draft.base_hash != self.saved_fingerprint()?
                || draft.content_hash != hash(&draft.state)?
            {
                return Err("恢复副本与当前已保存工程不匹配".into());
            }
            draft.state.settings.validate().map_err(|e| e.to_string())?;
            draft
                .state
                .settings
                .video_edit(self.duration)
                .map_err(|e| e.to_string())?;
            CameraTrackEditor::new(draft.state.camera.clone(), self.duration)
                .map_err(|e| e.to_string())?;
            load_effective_background(&self.project_root, &draft.state.settings)
                .map_err(|e| e.to_string())?;
            let before = self.snapshot();
            self.restore(draft.state);
            self.commit_undo(before);
            Ok(self.is_dirty())
        })();
        if restore.is_err() {
            // Preserve rejected data for inspection; it must neither replace saved edits nor
            // prevent subsequent valid drafts from being written.
            let retained = self.project_root.join(format!(
                "edit/recovery-retained-{}.json",
                uuid::Uuid::new_v4()
            ));
            fs::rename(&path, retained).map_err(|e| e.to_string())?;
        }
        restore
    }
}

pub struct DraftManager {
    active: Arc<AtomicBool>,
    io: Arc<Mutex<()>>,
    pending: Option<(String, mpsc::Receiver<Result<bool, String>>)>,
    written: Option<String>,
    last_attempt: Instant,
}

impl Default for DraftManager {
    fn default() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(true)),
            io: Arc::new(Mutex::new(())),
            pending: None,
            written: None,
            last_attempt: Instant::now(),
        }
    }
}

impl DraftManager {
    /// Keep in-flight results, but never reactivate an old queued snapshot after a discard.
    pub fn resume(&mut self) {
        if !self.active.load(Ordering::Acquire) {
            self.active = Arc::new(AtomicBool::new(true));
            self.written = None;
        }
    }
    /// True only when a draft write completed or the current fingerprint is already written.
    pub fn poll(&mut self, editor: &ProjectEditorSession, can_write: bool) -> Result<bool, String> {
        let mut completed = false;
        if let Some((fingerprint, receiver)) = &self.pending {
            match receiver.try_recv() {
                Ok(result) => {
                    let fingerprint = fingerprint.clone();
                    self.pending = None;
                    if result? {
                        self.written = Some(fingerprint);
                        completed = true;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return Ok(false),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    return Err("恢复副本写入线程意外退出".into());
                }
            }
        }
        if !can_write
            || !self.active.load(Ordering::Acquire)
            || self.last_attempt.elapsed() < Duration::from_secs(2)
        {
            return Ok(completed);
        }
        self.last_attempt = Instant::now();
        let fingerprint = editor.draft_fingerprint()?;
        if self.written.as_ref() == Some(&fingerprint) {
            return Ok(true);
        }
        let snapshot = editor.background_snapshot();
        let active = self.active.clone();
        let io = self.io.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("panzo-recovery-draft".into())
            .spawn(move || {
                let _guard = io.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let result = if active.load(Ordering::Acquire) {
                    snapshot.write_recovery_draft().map(|()| true)
                } else {
                    Ok(false)
                };
                let _ = sender.send(result);
            })
            .map_err(|e| e.to_string())?;
        self.pending = Some((fingerprint, receiver));
        Ok(completed)
    }

    /// Disable pending writers before clearing, so a delayed task cannot resurrect discarded edits.
    /// `None` means a writer is still finishing; the window can poll without waiting on disk.
    pub fn try_discard_and_stop(
        &mut self,
        editor: &ProjectEditorSession,
    ) -> Option<Result<(), String>> {
        self.active.store(false, Ordering::Release);
        let _guard = match self.io.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::WouldBlock) => return None,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        };
        let result = editor.clear_recovery_draft();
        if result.is_err() {
            self.active.store(true, Ordering::Release);
        } else {
            // The lock excludes a writer already touching the file. Queued writers
            // retain the disabled token, even if this manager is resumed later.
            self.pending = None;
            self.written = None;
        }
        Some(result)
    }

    /// Blocking counterpart for non-UI callers and deterministic storage tests.
    pub fn discard_and_stop(&mut self, editor: &ProjectEditorSession) -> Result<(), String> {
        self.active.store(false, Ordering::Release);
        let _guard = self
            .io
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = editor.clear_recovery_draft();
        if result.is_err() {
            self.active.store(true, Ordering::Release);
        } else {
            self.pending = None;
            self.written = None;
        }
        result
    }
}

impl Drop for DraftManager {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete_pending(manager: &mut DraftManager, editor: &ProjectEditorSession) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while manager.pending.is_some() {
            manager.poll(editor, false).unwrap();
            assert!(Instant::now() < deadline, "draft worker did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn schedule_draft(manager: &mut DraftManager, editor: &ProjectEditorSession) {
        manager.last_attempt = Instant::now().checked_sub(Duration::from_secs(3)).unwrap();
        manager.poll(editor, true).unwrap();
        assert!(manager.pending.is_some(), "draft should be queued");
    }

    #[test]
    fn cleared_draft_is_rewritten_after_resume_without_further_edits() {
        let project = super::super::tests::TestProject::new();
        let mut editor = ProjectEditorSession::open(&project.0).unwrap();
        editor.set_video_speed("source-0", 150).unwrap();
        let mut manager = DraftManager::default();
        schedule_draft(&mut manager, &editor);
        complete_pending(&mut manager, &editor);
        assert!(manager.written.is_some());
        assert!(project.0.join(DRAFT_FILE).is_file());

        manager.try_discard_and_stop(&editor).unwrap().unwrap();
        assert!(manager.pending.is_none());
        assert!(!project.0.join(DRAFT_FILE).exists());
        manager.resume();
        schedule_draft(&mut manager, &editor);
        complete_pending(&mut manager, &editor);
        let mut reopened = ProjectEditorSession::open(&project.0).unwrap();
        assert!(reopened.restore_recovery_draft().unwrap());
        assert_eq!(reopened.settings(), editor.settings());
    }

    #[test]
    fn skipped_pending_writer_does_not_claim_a_draft_exists_after_resume() {
        let project = super::super::tests::TestProject::new();
        let mut editor = ProjectEditorSession::open(&project.0).unwrap();
        editor.set_video_speed("source-0", 150).unwrap();
        let mut manager = DraftManager::default();
        let io = Arc::clone(&manager.io);
        let guard = io.lock().unwrap();
        schedule_draft(&mut manager, &editor);
        assert!(manager.try_discard_and_stop(&editor).is_none());
        manager.resume();
        drop(guard);
        complete_pending(&mut manager, &editor);
        assert!(!project.0.join(DRAFT_FILE).exists());
        assert!(manager.written.is_none());

        schedule_draft(&mut manager, &editor);
        complete_pending(&mut manager, &editor);
        let mut reopened = ProjectEditorSession::open(&project.0).unwrap();
        assert!(reopened.restore_recovery_draft().unwrap());
        assert_eq!(reopened.settings(), editor.settings());
    }

    #[test]
    fn cancelling_leave_while_writer_is_busy_resumes_future_drafts() {
        let project = super::super::tests::TestProject::new();
        let mut editor = ProjectEditorSession::open(&project.0).unwrap();
        let mut manager = DraftManager::default();
        let io = Arc::clone(&manager.io);
        let guard = io.lock().unwrap();
        assert!(manager.try_discard_and_stop(&editor).is_none());
        manager.resume();
        drop(guard);
        editor.set_video_speed("source-0", 200).unwrap();
        manager.last_attempt = Instant::now().checked_sub(Duration::from_secs(3)).unwrap();
        manager.poll(&editor, true).unwrap();
        manager
            .pending
            .take()
            .unwrap()
            .1
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let mut reopened = ProjectEditorSession::open(&project.0).unwrap();
        assert!(reopened.restore_recovery_draft().unwrap());
        assert_eq!(reopened.settings(), editor.settings());
    }

    #[test]
    fn background_draft_recovers_and_discard_cannot_be_undone_by_pending_writer() {
        let project = super::super::tests::TestProject::new();
        let mut editor = ProjectEditorSession::open(&project.0).unwrap();
        editor.set_video_speed("source-0", 150).unwrap();
        let mut manager = DraftManager::default();
        manager.last_attempt = Instant::now().checked_sub(Duration::from_secs(3)).unwrap();
        manager.poll(&editor, true).unwrap();
        manager
            .pending
            .take()
            .unwrap()
            .1
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let mut recovered = ProjectEditorSession::open(&project.0).unwrap();
        assert!(recovered.restore_recovery_draft().unwrap());
        assert_eq!(recovered.settings(), editor.settings());

        editor.set_video_speed("source-0", 200).unwrap();
        manager.last_attempt = Instant::now().checked_sub(Duration::from_secs(3)).unwrap();
        manager.poll(&editor, true).unwrap();
        let pending = manager.pending.take().unwrap().1;
        manager.discard_and_stop(&editor).unwrap();
        pending
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert!(!project.0.join(DRAFT_FILE).exists());
        let mut reopened = ProjectEditorSession::open(&project.0).unwrap();
        assert!(!reopened.restore_recovery_draft().unwrap());
        assert!(!reopened.is_dirty());
    }
}
