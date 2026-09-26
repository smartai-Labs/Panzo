use crate::platform::export::{
    ExportPreset, ExportReport, ExportRequest, ExportSnapshot, ProjectExporter,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};

pub(crate) struct LoadTask {
    result: Receiver<Result<crate::platform::editor_load::PreparedEditorLoad, String>>,
    cancel: Arc<AtomicBool>,
}

impl LoadTask {
    pub(crate) fn start(project: std::path::PathBuf, restore_draft: bool) -> std::io::Result<Self> {
        let (sender, result) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        std::thread::Builder::new()
            .name("panzo-open-project".into())
            .spawn(move || {
                if worker_cancel.load(Ordering::Acquire) {
                    return;
                }
                let loaded = crate::platform::editor_load::PreparedEditorLoad::prepare_cancellable(
                    &project,
                    restore_draft,
                    &worker_cancel,
                )
                .map_err(|error| error.to_string());
                if !worker_cancel.load(Ordering::Acquire) {
                    let _ = sender.send(loaded);
                }
            })?;
        Ok(Self { result, cancel })
    }

    pub(crate) fn poll(
        &self,
    ) -> Option<Result<crate::platform::editor_load::PreparedEditorLoad, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("工程载入线程已退出".into())),
        }
    }
}

impl Drop for LoadTask {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

pub struct ExportTask {
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
    result: Receiver<Result<ExportReport, ExportTaskError>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ExportTaskError {
    #[error("导出已取消")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
}

impl From<crate::platform::export::ExportError> for ExportTaskError {
    fn from(error: crate::platform::export::ExportError) -> Self {
        match error {
            crate::platform::export::ExportError::Cancelled => Self::Cancelled,
            error => Self::Failed(error.to_string()),
        }
    }
}

impl ExportTask {
    pub fn start(
        request: ExportRequest,
        snapshot: ExportSnapshot,
        preset: ExportPreset,
    ) -> std::io::Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0));
        let (sender, result) = mpsc::sync_channel(1);
        let worker_cancel = cancel.clone();
        let worker_progress = progress.clone();
        std::thread::Builder::new()
            .name("panzo-export".into())
            .spawn(move || {
                let result = ProjectExporter::export_snapshot(
                    &request,
                    &snapshot,
                    preset,
                    &worker_cancel,
                    |done, total| {
                        worker_progress.store(
                            (u64::from(done) * 100 / u64::from(total.max(1))) as u32,
                            Ordering::Release,
                        );
                    },
                )
                .map_err(ExportTaskError::from);
                let _ = sender.send(result);
            })?;
        Ok(Self {
            cancel,
            progress,
            result,
        })
    }

    pub fn progress(&self) -> u32 {
        self.progress.load(Ordering::Acquire)
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub fn poll(&self) -> Option<Result<ExportReport, ExportTaskError>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(ExportTaskError::Failed("导出工作线程已退出".into())))
            }
        }
    }
}

impl Drop for ExportTask {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub struct SaveTask {
    result: Receiver<
        Result<
            (
                crate::platform::editor::ProjectEditorSession,
                crate::platform::editor::EditorSaveReport,
            ),
            String,
        >,
    >,
}

#[cfg(test)]
pub(crate) type ReviewSaveResult = Result<
    (
        crate::platform::editor::ProjectEditorSession,
        crate::platform::editor::EditorSaveReport,
    ),
    String,
>;

pub struct BackgroundTask {
    result: Receiver<
        Result<
            (
                panzo_core::BackgroundSettings,
                Option<crate::platform::background_image::DecodedBackgroundImage>,
            ),
            String,
        >,
    >,
    pub importing: bool,
    pub previous_style: panzo_core::BackgroundSettings,
}
impl BackgroundTask {
    pub fn start(
        mut snapshot: crate::platform::editor::ProjectEditorSession,
        import: Option<std::path::PathBuf>,
    ) -> std::io::Result<Self> {
        let importing = import.is_some();
        let previous_style = snapshot.settings().background.clone();
        let (sender, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("panzo-background-image".into())
            .spawn(move || {
                let result = (|| {
                    if let Some(path) = import {
                        snapshot
                            .import_background_image(path)
                            .map_err(|e| e.to_string())?;
                    }
                    let image = crate::platform::editor::load_effective_background(
                        snapshot.project_root(),
                        snapshot.settings(),
                    )
                    .map_err(|e| e.to_string())?;
                    Ok((snapshot.settings().background.clone(), image))
                })();
                let _ = sender.send(result);
            })?;
        Ok(Self {
            result,
            importing,
            previous_style,
        })
    }
    pub fn poll(
        &self,
    ) -> Option<
        Result<
            (
                panzo_core::BackgroundSettings,
                Option<crate::platform::background_image::DecodedBackgroundImage>,
            ),
            String,
        >,
    > {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("背景载入线程已退出".into())),
        }
    }
}
impl SaveTask {
    /// Let an integration test release a real save result at an exact UI boundary.
    #[cfg(test)]
    pub(crate) fn review_pending() -> (Self, mpsc::SyncSender<ReviewSaveResult>) {
        let (sender, result) = mpsc::sync_channel(1);
        (Self { result }, sender)
    }

    pub fn start(
        mut snapshot: crate::platform::editor::ProjectEditorSession,
        kind: panzo_core::CameraEditKind,
        segment: Option<String>,
    ) -> std::io::Result<Self> {
        let (sender, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("panzo-save".into())
            .spawn(move || {
                let result = snapshot
                    .save(kind, segment)
                    .map(|report| (snapshot, report))
                    .map_err(|e| e.to_string());
                let _ = sender.send(result);
            })?;
        Ok(Self { result })
    }
    pub fn poll(
        &self,
    ) -> Option<
        Result<
            (
                crate::platform::editor::ProjectEditorSession,
                crate::platform::editor::EditorSaveReport,
            ),
            String,
        >,
    > {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err("保存线程已退出；重新打开时会恢复完整修订".into()))
            }
        }
    }
}

#[cfg(test)]
mod feedback_tests {
    use super::*;
    #[test]
    fn user_cancel_keeps_its_type_across_the_worker_boundary() {
        assert!(matches!(
            ExportTaskError::from(crate::platform::export::ExportError::Cancelled),
            ExportTaskError::Cancelled
        ));
        assert!(!matches!(
            ExportTaskError::Failed("cancelled in an unrelated error".into()),
            ExportTaskError::Cancelled
        ));
    }
}
