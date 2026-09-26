use crate::geometry::{PhysicalRect, PhysicalSize};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};
use thiserror::Error;
use uuid::Uuid;

pub const PROJECT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectState {
    Recording,
    Finalizing,
    Processing,
    Ready,
    Recovering,
    Recovered,
    Failed,
}

impl ProjectState {
    pub const fn can_transition_to(self, target: Self) -> bool {
        matches!(
            (self, target),
            (
                Self::Recording,
                Self::Finalizing | Self::Recovering | Self::Failed
            ) | (
                Self::Finalizing,
                Self::Processing | Self::Recovering | Self::Failed
            ) | (
                Self::Processing,
                Self::Ready | Self::Recovering | Self::Failed
            ) | (Self::Recovering, Self::Recovered | Self::Failed)
                | (
                    Self::Recovered,
                    Self::Processing | Self::Ready | Self::Failed
                )
                | (Self::Ready, Self::Processing | Self::Failed)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimebaseDescriptor {
    pub ticks_per_second: i64,
    pub session_start_qpc: i64,
    pub qpc_frequency: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDescriptor {
    pub kind: CaptureKind,
    pub monitor_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_title: Option<String>,
    pub desktop_rect_px: PhysicalRect,
    pub content_size_px: PhysicalSize,
    pub target_fps: u32,
    pub pixel_format: String,
    pub color_mode: String,
    pub cursor_captured_in_video: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureKind {
    Monitor,
    Window,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenMediaDescriptor {
    pub screen: String,
    pub codec: String,
    pub duration_tick: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackPaths {
    pub cursor: String,
    pub clicks: String,
    pub camera: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManifest {
    pub schema_version: u32,
    pub project_id: Uuid,
    pub app_version: String,
    pub state: ProjectState,
    pub created_at_utc: String,
    pub timebase: TimebaseDescriptor,
    pub capture: CaptureDescriptor,
    pub media: ScreenMediaDescriptor,
    pub tracks: TrackPaths,
}

impl ProjectManifest {
    pub fn new_v01(
        project_id: Uuid,
        app_version: impl Into<String>,
        created_at_utc: impl Into<String>,
        timebase: TimebaseDescriptor,
        capture: CaptureDescriptor,
    ) -> Self {
        Self {
            schema_version: PROJECT_SCHEMA_VERSION,
            project_id,
            app_version: app_version.into(),
            state: ProjectState::Recording,
            created_at_utc: created_at_utc.into(),
            timebase,
            capture,
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

    pub fn validate(&self) -> Result<(), ProjectValidationError> {
        if self.schema_version != PROJECT_SCHEMA_VERSION {
            return Err(ProjectValidationError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        if self.timebase.ticks_per_second != crate::time::TICKS_PER_SECOND {
            return Err(ProjectValidationError::UnsupportedTimebase(
                self.timebase.ticks_per_second,
            ));
        }
        if self.timebase.qpc_frequency <= 0 {
            return Err(ProjectValidationError::InvalidQpcFrequency);
        }
        if self.capture.content_size_px.is_empty() {
            return Err(ProjectValidationError::EmptyCaptureSize);
        }
        match self.capture.kind {
            CaptureKind::Monitor if self.capture.monitor_id.trim().is_empty() => {
                return Err(ProjectValidationError::MissingMonitorId);
            }
            CaptureKind::Window if self.capture.window_id.as_deref().is_none_or(str::is_empty) => {
                return Err(ProjectValidationError::MissingWindowId);
            }
            CaptureKind::Monitor | CaptureKind::Window => {}
        }
        if self.capture.target_fps != 60 {
            return Err(ProjectValidationError::UnsupportedTargetFps(
                self.capture.target_fps,
            ));
        }
        if self.capture.cursor_captured_in_video {
            return Err(ProjectValidationError::CursorPresentInSourceVideo);
        }

        for path in [
            &self.media.screen,
            &self.tracks.cursor,
            &self.tracks.clicks,
            &self.tracks.camera,
        ] {
            validate_relative_project_path(path)?;
        }

        Ok(())
    }

    pub fn transition_to(&mut self, target: ProjectState) -> Result<(), ProjectValidationError> {
        if !self.state.can_transition_to(target) {
            return Err(ProjectValidationError::InvalidStateTransition {
                from: self.state,
                to: target,
            });
        }
        self.state = target;
        Ok(())
    }
}

fn validate_relative_project_path(value: &str) -> Result<(), ProjectValidationError> {
    let path = Path::new(value);
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(ProjectValidationError::InvalidProjectPath(value.into()));
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(ProjectValidationError::InvalidProjectPath(value.into()));
    }
    Ok(())
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum ProjectValidationError {
    #[error("unsupported project schema version {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("unsupported timebase {0}")]
    UnsupportedTimebase(i64),
    #[error("QPC frequency must be positive")]
    InvalidQpcFrequency,
    #[error("capture size must not be empty")]
    EmptyCaptureSize,
    #[error("monitor capture requires a monitor identifier")]
    MissingMonitorId,
    #[error("window capture requires a window identifier")]
    MissingWindowId,
    #[error("V0.1 only supports target 60 FPS, got {0}")]
    UnsupportedTargetFps(u32),
    #[error("source video must not contain the system cursor")]
    CursorPresentInSourceVideo,
    #[error("project path must be a safe relative path: {0}")]
    InvalidProjectPath(String),
    #[error("invalid project state transition from {from:?} to {to:?}")]
    InvalidStateTransition {
        from: ProjectState,
        to: ProjectState,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn validates_v01_manifest() {
        manifest().validate().unwrap();
    }

    #[test]
    fn rejects_parent_traversal() {
        let mut manifest = manifest();
        manifest.tracks.camera = "../camera.json".into();
        assert!(matches!(
            manifest.validate(),
            Err(ProjectValidationError::InvalidProjectPath(_))
        ));
    }

    #[test]
    fn enforces_state_machine() {
        let mut manifest = manifest();
        assert!(manifest.transition_to(ProjectState::Finalizing).is_ok());
        assert!(manifest.transition_to(ProjectState::Recovered).is_err());
        assert!(manifest.transition_to(ProjectState::Processing).is_ok());
        assert!(manifest.transition_to(ProjectState::Recovering).is_ok());
        assert!(manifest.transition_to(ProjectState::Recovered).is_ok());
        assert!(manifest.transition_to(ProjectState::Recovering).is_err());
    }

    #[test]
    fn round_trip_preserves_manifest() {
        let manifest = manifest();
        let json = serde_json::to_vec(&manifest).unwrap();
        let decoded: ProjectManifest = serde_json::from_slice(&json).unwrap();
        assert_eq!(manifest, decoded);
    }

    #[test]
    fn window_capture_accepts_non_widescreen_content() {
        let mut manifest = manifest();
        manifest.capture.kind = CaptureKind::Window;
        manifest.capture.monitor_id.clear();
        manifest.capture.window_id = Some("window-42".into());
        manifest.capture.window_title = Some("Document".into());
        manifest.capture.desktop_rect_px.width = 1_200;
        manifest.capture.desktop_rect_px.height = 900;
        manifest.capture.content_size_px.width = 1_200;
        manifest.capture.content_size_px.height = 900;
        manifest.validate().unwrap();
    }
}
