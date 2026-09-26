use crate::platform::fmp4_writer::{Fmp4ProbeError, inspect_fmp4};
use panzo_core::diagnostics::{ProjectMetrics, RecordingStabilityReport};
use panzo_core::{ProjectIoError, ProjectLayout, ProjectState};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const CAPTURE_QUEUE_CAPACITY: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStabilityReport {
    pub project_root: PathBuf,
    pub project_state: ProjectState,
    pub source_width: u32,
    pub source_height: u32,
    pub target_fps: u32,
    pub fragment_count: usize,
    pub trailing_bytes: u64,
    #[serde(flatten)]
    pub acceptance: RecordingStabilityReport,
}

pub struct StabilityProbe;

impl StabilityProbe {
    pub fn inspect(
        project_root: impl AsRef<Path>,
        required_duration_tick: i64,
    ) -> Result<ProjectStabilityReport, StabilityProbeError> {
        if required_duration_tick <= 0 {
            return Err(StabilityProbeError::InvalidRequiredDuration(
                required_duration_tick,
            ));
        }
        let project_root = project_root.as_ref();
        let layout = ProjectLayout::open(project_root)?;
        let manifest = layout.load_manifest()?;
        let metrics_path = layout.metrics_path();
        let bytes = fs::read(&metrics_path).map_err(|source| StabilityProbeError::ReadMetrics {
            path: metrics_path.clone(),
            source,
        })?;
        let metrics: ProjectMetrics =
            serde_json::from_slice(&bytes).map_err(StabilityProbeError::DeserializeMetrics)?;
        let media_path = project_root.join(&manifest.media.screen);
        let inspection = inspect_fmp4(media_path)?;
        let mut acceptance = RecordingStabilityReport::evaluate(
            &metrics,
            manifest.media.duration_tick,
            required_duration_tick,
            CAPTURE_QUEUE_CAPACITY,
        );
        if manifest.state != ProjectState::Ready {
            acceptance.failures.push(format!(
                "project state is {:?}, expected Ready",
                manifest.state
            ));
        }
        if inspection.trailing_bytes != 0 {
            acceptance.failures.push(format!(
                "fragmented MP4 has {} trailing bytes",
                inspection.trailing_bytes
            ));
        }
        if inspection.last_video_end_tick != manifest.media.duration_tick {
            acceptance.failures.push(format!(
                "container duration {} differs from manifest duration {}",
                inspection.last_video_end_tick, manifest.media.duration_tick
            ));
        }
        acceptance.passed = acceptance.failures.is_empty();

        Ok(ProjectStabilityReport {
            project_root: project_root.into(),
            project_state: manifest.state,
            source_width: manifest.capture.content_size_px.width,
            source_height: manifest.capture.content_size_px.height,
            target_fps: manifest.capture.target_fps,
            fragment_count: inspection.fragment_count,
            trailing_bytes: inspection.trailing_bytes,
            acceptance,
        })
    }
}

#[derive(Debug, Error)]
pub enum StabilityProbeError {
    #[error("required stability duration must be positive, got {0} ticks")]
    InvalidRequiredDuration(i64),
    #[error(transparent)]
    Project(#[from] ProjectIoError),
    #[error("could not read metrics {path}: {source}")]
    ReadMetrics {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not deserialize project metrics: {0}")]
    DeserializeMetrics(serde_json::Error),
    #[error(transparent)]
    Media(#[from] Fmp4ProbeError),
}
