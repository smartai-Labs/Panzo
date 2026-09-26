//! Worker-side editor loading. Render resources and window controls remain on the UI thread.
use crate::platform::editor::{EditorSessionError, ProjectEditorSession};
use crate::platform::preview::{
    PreparedPreviewSession, PreviewSessionError, ProjectPreviewSession,
};
use crate::platform::scrub_preview::{ScrubPreviewWorker, ScrubPreviewWorkerError};
use panzo_core::OutputDescriptor;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use thiserror::Error;

pub(crate) struct PreparedEditorLoad {
    pub(crate) editor: ProjectEditorSession,
    pub(crate) preview: PreparedPreviewSession,
    pub(crate) scrub_preview: ScrubPreviewWorker,
    pub(crate) project_name: String,
    pub(crate) recovery_notice: Option<String>,
    pub(crate) camera_planning_warning: bool,
}

impl PreparedEditorLoad {
    /// This blocking work belongs to a load worker, never an interactive window handler.
    /// `restore_draft` is false when reopening the same project after discarding edits.
    pub(crate) fn prepare(project: &Path, restore_draft: bool) -> Result<Self, EditorLoadError> {
        Self::prepare_cancellable(project, restore_draft, &AtomicBool::new(false))
    }

    pub(crate) fn prepare_cancellable(
        project: &Path,
        restore_draft: bool,
        cancelled: &AtomicBool,
    ) -> Result<Self, EditorLoadError> {
        check_cancelled(cancelled)?;
        let mut editor = ProjectEditorSession::open(project)?;
        check_cancelled(cancelled)?;
        let recovery_notice = if restore_draft {
            match editor.restore_recovery_draft() {
                Ok(true) => {
                    Some("已恢复上次未保存的修改。\nCtrl+S 保存，Ctrl+Z 撤销这次恢复。".into())
                }
                Ok(false) => None,
                Err(_) => Some("恢复副本未能载入，已保留副本并打开上次保存的工程。".into()),
            }
        } else {
            None
        };
        check_cancelled(cancelled)?;
        let native_output = ProjectPreviewSession::native_output_descriptor(project)?;
        check_cancelled(cancelled)?;
        let output = editor.settings().canvas.size.map_or(native_output, |size| {
            OutputDescriptor::bgra8(size.width, size.height).expect("validated canvas")
        });
        let preview = PreparedPreviewSession::prepare_editor_cancellable(
            project,
            output,
            editor.camera().clone(),
            editor.settings().clone(),
            cancelled,
        )?;
        check_cancelled(cancelled)?;
        let scrub_preview = ScrubPreviewWorker::start(preview.source_media_path())?;
        check_cancelled(cancelled)?;
        Ok(Self {
            editor,
            preview,
            scrub_preview,
            project_name: project
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("未命名项目")
                .to_owned(),
            recovery_notice,
            camera_planning_warning: project
                .join("diagnostics/camera-planning-warning.json")
                .is_file(),
        })
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), EditorLoadError> {
    if cancelled.load(Ordering::Acquire) {
        Err(EditorLoadError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Debug, Error)]
pub(crate) enum EditorLoadError {
    #[error("editor load cancelled")]
    Cancelled,
    #[error(transparent)]
    Editor(#[from] EditorSessionError),
    #[error(transparent)]
    Preview(#[from] PreviewSessionError),
    #[error(transparent)]
    Scrub(#[from] ScrubPreviewWorkerError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_load_result_can_cross_threads() {
        fn assert_send<T: Send>() {}
        assert_send::<Result<PreparedEditorLoad, EditorLoadError>>();
    }

    #[test]
    fn cancelled_load_does_not_open_the_project() {
        let result = PreparedEditorLoad::prepare_cancellable(
            Path::new("missing-project.panzo"),
            false,
            &AtomicBool::new(true),
        );
        assert!(matches!(result, Err(EditorLoadError::Cancelled)));
    }
}
