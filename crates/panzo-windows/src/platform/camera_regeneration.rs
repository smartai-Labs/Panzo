use crate::platform::editor_transaction::{
    EditorTransactionError, recover_pending_editor_transaction,
};
use crate::platform::manifest_store::{ManifestStoreError, persist_json_atomically};
use panzo_core::planner::PlannerError;
use panzo_core::{
    CameraPlanner, CameraSegment, CameraTrack, ClickEvent, PlannerInput, PlannerParams,
    ProjectIoError, ProjectLayout, TimeTick,
};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraRegenerationReport {
    pub project_root: PathBuf,
    pub camera_path: PathBuf,
    pub click_records: usize,
    pub triggering_clicks: usize,
    pub generated_segments: usize,
    pub preserved_user_segments: usize,
    pub previous_file_hash: String,
    pub regenerated_file_hash: String,
    pub content_changed: bool,
}

impl fmt::Display for CameraRegenerationReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Panzo M3 camera regeneration")?;
        writeln!(formatter, "  Project: {}", self.project_root.display())?;
        writeln!(formatter, "  Camera track: {}", self.camera_path.display())?;
        writeln!(formatter, "  Click records: {}", self.click_records)?;
        writeln!(
            formatter,
            "  Camera-triggering clicks: {}",
            self.triggering_clicks
        )?;
        writeln!(
            formatter,
            "  Generated segments: {}",
            self.generated_segments
        )?;
        writeln!(
            formatter,
            "  Preserved user segments: {}",
            self.preserved_user_segments
        )?;
        writeln!(
            formatter,
            "  Previous file hash: {}",
            self.previous_file_hash
        )?;
        writeln!(
            formatter,
            "  Regenerated file hash: {}",
            self.regenerated_file_hash
        )?;
        write!(formatter, "  Content changed: {}", self.content_changed)
    }
}

pub struct CameraTrackRegenerator;

impl CameraTrackRegenerator {
    pub fn regenerate(
        project_root: impl AsRef<Path>,
    ) -> Result<CameraRegenerationReport, CameraRegenerationError> {
        let layout = ProjectLayout::open(project_root)?;
        recover_pending_editor_transaction(layout.root())?;
        let manifest = layout.load_manifest()?;
        let duration = TimeTick(manifest.media.duration_tick);
        if duration <= TimeTick::ZERO {
            return Err(CameraRegenerationError::InvalidDuration(duration));
        }

        let click_path = layout.root().join(&manifest.tracks.clicks);
        let clicks = read_click_events(&click_path)?;
        let click_json = serde_json::to_vec(&clicks)?;
        let geometry_json = serde_json::to_vec(&manifest.capture.desktop_rect_px)?;
        let click_hash = sha256(&click_json);
        let geometry_hash = sha256(&geometry_json);
        let planner = CameraPlanner::new(PlannerParams::default(), "planner-v1")?;
        let generated = planner.plan(PlannerInput {
            recording_duration: duration,
            clicks: &clicks,
            click_track_hash: &click_hash,
            capture_geometry_hash: &geometry_hash,
        })?;

        let camera_path = layout.root().join(&manifest.tracks.camera);
        let previous = fs::read(&camera_path).map_err(|source| CameraRegenerationError::Read {
            path: camera_path.clone(),
            source,
        })?;
        let previous_track: CameraTrack = serde_json::from_slice(&previous)?;
        let preserved_user_segments = previous_track
            .segments
            .iter()
            .filter(|segment| segment.locked || segment.user_modified)
            .count();
        let track = merge_user_segments(generated, &previous_track)?;
        let previous_file_hash = sha256(&previous);
        persist_json_atomically(&camera_path, &track)?;
        let regenerated =
            fs::read(&camera_path).map_err(|source| CameraRegenerationError::Read {
                path: camera_path.clone(),
                source,
            })?;
        let regenerated_file_hash = sha256(&regenerated);

        Ok(CameraRegenerationReport {
            project_root: layout.root().into(),
            camera_path,
            click_records: clicks.len(),
            triggering_clicks: clicks
                .iter()
                .filter(|click| click.triggers_camera())
                .count(),
            generated_segments: track.segments.len(),
            preserved_user_segments,
            content_changed: previous_file_hash != regenerated_file_hash,
            previous_file_hash,
            regenerated_file_hash,
        })
    }
}

fn merge_user_segments(
    mut generated: CameraTrack,
    previous: &CameraTrack,
) -> Result<CameraTrack, CameraRegenerationError> {
    let protected: Vec<CameraSegment> = previous
        .segments
        .iter()
        .filter(|segment| segment.locked || segment.user_modified)
        .cloned()
        .collect();
    if protected.is_empty() {
        return Ok(generated);
    }
    generated.segments.retain(|candidate| {
        !protected
            .iter()
            .any(|segment| segments_overlap(candidate, segment))
    });
    generated.segments.extend(protected);
    generated
        .segments
        .sort_by_key(|segment| (segment.start_tick, segment.end_tick));

    let mut expected = generated.base_state;
    for segment in &mut generated.segments {
        segment.from = expected;
        expected = segment.to;
    }
    generated
        .validate()
        .map_err(CameraRegenerationError::InvalidTrack)?;
    Ok(generated)
}

fn segments_overlap(left: &CameraSegment, right: &CameraSegment) -> bool {
    left.start_tick < right.end_tick && right.start_tick < left.end_tick
}

fn read_click_events(path: &Path) -> Result<Vec<ClickEvent>, CameraRegenerationError> {
    let file = fs::File::open(path).map_err(|source| CameraRegenerationError::Read {
        path: path.into(),
        source,
    })?;
    let mut events = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|source| CameraRegenerationError::Read {
            path: path.into(),
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }
        events.push(serde_json::from_str(&line).map_err(|source| {
            CameraRegenerationError::InvalidClickRecord {
                path: path.into(),
                line: index + 1,
                source,
            }
        })?);
    }
    Ok(events)
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Error)]
pub enum CameraRegenerationError {
    #[error(transparent)]
    Project(#[from] ProjectIoError),
    #[error(transparent)]
    EditorTransaction(#[from] EditorTransactionError),
    #[error("project recording duration must be positive: {0:?}")]
    InvalidDuration(TimeTick),
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid click JSONL record {path}:{line}: {source}")]
    InvalidClickRecord {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not serialize camera planner input: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Planner(#[from] PlannerError),
    #[error("merged regenerated Camera Track is invalid: {0}")]
    InvalidTrack(panzo_core::camera::CameraValidationError),
    #[error(transparent)]
    AtomicWrite(#[from] ManifestStoreError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use panzo_core::camera::{
        CAMERA_SCHEMA_VERSION, Easing, GeneratorMetadata, SegmentOrigin, SourceRevision,
    };
    use panzo_core::{CameraSegmentKind, CameraState};

    fn segment(
        id: &str,
        start: i64,
        end: i64,
        from: CameraState,
        to: CameraState,
        protected: bool,
    ) -> CameraSegment {
        CameraSegment {
            id: id.into(),
            kind: CameraSegmentKind::ZoomIn,
            start_tick: TimeTick(start),
            end_tick: TimeTick(end),
            from,
            to,
            easing: Easing::Smootherstep,
            origin: if protected {
                SegmentOrigin::Manual
            } else {
                SegmentOrigin::Auto
            },
            source_click_ids: Vec::new(),
            locked: protected,
            user_modified: protected,
            focus: None,
        }
    }

    fn track(segments: Vec<CameraSegment>) -> CameraTrack {
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
            segments,
        }
    }

    #[test]
    fn regeneration_keeps_protected_segment_and_repairs_following_auto_transition() {
        let auto_focus = CameraState {
            center_x: 0.5,
            center_y: 0.5,
            scale: 1.6,
        };
        let manual_focus = CameraState {
            center_x: 0.5,
            center_y: 0.5,
            scale: 2.0,
        };
        let generated = track(vec![
            segment("auto-overlap", 10, 20, CameraState::BASE, auto_focus, false),
            segment("auto-later", 30, 40, auto_focus, CameraState::BASE, false),
        ]);
        let previous = track(vec![segment(
            "manual",
            12,
            18,
            CameraState::BASE,
            manual_focus,
            true,
        )]);

        let merged = merge_user_segments(generated, &previous).unwrap();
        assert_eq!(merged.segments.len(), 2);
        assert_eq!(merged.segments[0].id, "manual");
        assert_eq!(merged.segments[0].to, manual_focus);
        assert_eq!(merged.segments[1].id, "auto-later");
        assert_eq!(merged.segments[1].from, manual_focus);
        merged.validate().unwrap();
    }
}
