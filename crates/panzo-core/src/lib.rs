//! Platform-independent domain model for Panzo.

pub mod camera;
pub mod canvas;
pub mod crop_gesture;
pub use canvas::CanvasSize;
pub mod diagnostics;
pub mod editor;
pub mod events;
pub mod geometry;
pub mod journal;
pub mod planner;
pub mod preview;
pub mod project;
pub mod project_io;
pub mod project_writer;
pub mod render;
pub mod time;
pub mod video_edit;
pub use video_edit::{VideoClip, VideoCrop, VideoEdit, VideoEditError};

pub use camera::{
    CameraSegment, CameraSegmentKind, CameraState, CameraTrack, Easing, GeneratorMetadata,
    SegmentOrigin, SourceRevision,
};
pub use editor::{
    BackgroundKind, BackgroundSettings, CameraEditError, CameraEditKind, CameraEditRecord,
    CameraTrackEditor, CanvasSettings, CursorStyle, WORKBENCH_SCHEMA_VERSION, WorkbenchSettings,
    WorkbenchValidationError,
};
pub use events::{
    ClickEvent, CursorEvent, CursorEventKind, InputEventNormalizer, InputNormalizationError,
    MouseAction, MouseButton, PersistedInputEvent, RawInputKind, StampedInputEvent,
};
pub use geometry::{CoordinateSpace, NormalizedPoint, PhysicalPoint, PhysicalRect, PhysicalSize};
pub use journal::{JOURNAL_SCHEMA_VERSION, JournalEntry, JournalOperation, JournalResult};
pub use planner::{CameraPlanner, PlannerInput, PlannerParams};
pub use preview::{PlaybackState, PreviewController, PreviewControllerError, PreviewSnapshot};
pub use project::{CaptureDescriptor, ProjectManifest, ProjectState};
pub use project_io::{
    JsonlRecoveryReport, JsonlTrimReport, ProjectIoError, ProjectLayout, RecordingLock,
    recover_jsonl_tail, trim_jsonl_after_tick,
};
pub use project_writer::{
    CHECKPOINT_INTERVAL_TICK, FLUSH_INTERVAL_TICK, ProjectWriteStats, ProjectWriter,
    ProjectWriterError, WriterServiceReport,
};
pub use render::{
    CURSOR_SIZE_PX, CameraTransform, CursorRenderState, CursorTimeline, FrameEvaluation,
    FrameEvaluator, OutputDescriptor, OutputPixelFormat, RenderEvaluationError, RenderPoint,
    SourceFrameSelection, SourceFrameTimeline, SourceRect,
};
pub use time::{
    IdentityTimeMapping, QpcClock, SessionClock, TICKS_PER_SECOND, TimeMapping, TimeTick,
    VideoFrameRateDecision, VideoFrameRateLimiter, VideoTimestampDecision,
    VideoTimestampNormalizer,
};
