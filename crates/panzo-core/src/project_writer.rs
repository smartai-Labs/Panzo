use crate::events::{ClickEvent, CursorEvent, PersistedInputEvent};
use crate::journal::{JournalEntry, JournalOperation, JournalResult};
use crate::project_io::ProjectLayout;
use crate::time::{TICKS_PER_SECOND, TimeTick};
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use thiserror::Error;

pub const FLUSH_INTERVAL_TICK: i64 = TICKS_PER_SECOND;
pub const CHECKPOINT_INTERVAL_TICK: i64 = TICKS_PER_SECOND * 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectWriteStats {
    pub cursor_records: u64,
    pub click_records: u64,
    pub journal_records: u64,
    pub flushes: u64,
    pub durable_checkpoints: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriterServiceReport {
    pub flushed: bool,
    pub checkpointed: bool,
}

pub struct ProjectWriter {
    cursor: JsonlWriter,
    clicks: JsonlWriter,
    journal: JsonlWriter,
    next_journal_seq: u64,
    last_cursor_tick: Option<TimeTick>,
    last_click_tick: Option<TimeTick>,
    last_journal_tick: Option<TimeTick>,
    last_flush_tick: TimeTick,
    last_checkpoint_tick: TimeTick,
    stats: ProjectWriteStats,
}

impl ProjectWriter {
    pub fn create(layout: &ProjectLayout) -> Result<Self, ProjectWriterError> {
        Ok(Self {
            cursor: JsonlWriter::create_new(layout.cursor_events_path())?,
            clicks: JsonlWriter::create_new(layout.click_events_path())?,
            journal: JsonlWriter::create_new(layout.journal_path())?,
            next_journal_seq: 1,
            last_cursor_tick: None,
            last_click_tick: None,
            last_journal_tick: None,
            last_flush_tick: TimeTick::ZERO,
            last_checkpoint_tick: TimeTick::ZERO,
            stats: ProjectWriteStats::default(),
        })
    }

    pub fn append_input(&mut self, event: &PersistedInputEvent) -> Result<(), ProjectWriterError> {
        match event {
            PersistedInputEvent::Cursor(event) => self.append_cursor(event),
            PersistedInputEvent::Click(event) => self.append_click(event),
        }
    }

    pub fn append_journal(
        &mut self,
        time_tick: TimeTick,
        operation: JournalOperation,
        result: JournalResult,
        detail: Option<String>,
    ) -> Result<u64, ProjectWriterError> {
        ensure_monotonic("journal", self.last_journal_tick, time_tick)?;
        let seq = self.next_journal_seq;
        let entry = JournalEntry::new(seq, time_tick, operation, result, detail);
        self.journal.append(&entry)?;
        self.next_journal_seq += 1;
        self.last_journal_tick = Some(time_tick);
        self.stats.journal_records += 1;
        Ok(seq)
    }

    pub fn service_due(
        &mut self,
        now: TimeTick,
    ) -> Result<WriterServiceReport, ProjectWriterError> {
        if now < self.last_flush_tick || now < self.last_checkpoint_tick {
            return Err(ProjectWriterError::ClockRegression {
                current: now,
                last_flush: self.last_flush_tick,
                last_checkpoint: self.last_checkpoint_tick,
            });
        }

        let mut report = WriterServiceReport::default();
        if now.0 - self.last_checkpoint_tick.0 >= CHECKPOINT_INTERVAL_TICK {
            self.checkpoint()?;
            self.last_checkpoint_tick = now;
            self.last_flush_tick = now;
            report.flushed = true;
            report.checkpointed = true;
        } else if now.0 - self.last_flush_tick.0 >= FLUSH_INTERVAL_TICK {
            self.flush()?;
            self.last_flush_tick = now;
            report.flushed = true;
        }
        Ok(report)
    }

    pub fn flush(&mut self) -> Result<(), ProjectWriterError> {
        self.cursor.flush()?;
        self.clicks.flush()?;
        self.journal.flush()?;
        self.stats.flushes += 1;
        Ok(())
    }

    pub fn checkpoint(&mut self) -> Result<(), ProjectWriterError> {
        self.cursor.checkpoint()?;
        self.clicks.checkpoint()?;
        self.journal.checkpoint()?;
        self.stats.flushes += 1;
        self.stats.durable_checkpoints += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Result<ProjectWriteStats, ProjectWriterError> {
        self.checkpoint()?;
        Ok(self.stats)
    }

    pub const fn stats(&self) -> ProjectWriteStats {
        self.stats
    }

    fn append_cursor(&mut self, event: &CursorEvent) -> Result<(), ProjectWriterError> {
        ensure_monotonic("cursor", self.last_cursor_tick, event.time_tick)?;
        self.cursor.append(event)?;
        self.last_cursor_tick = Some(event.time_tick);
        self.stats.cursor_records += 1;
        Ok(())
    }

    fn append_click(&mut self, event: &ClickEvent) -> Result<(), ProjectWriterError> {
        ensure_monotonic("click", self.last_click_tick, event.time_tick)?;
        self.clicks.append(event)?;
        self.last_click_tick = Some(event.time_tick);
        self.stats.click_records += 1;
        Ok(())
    }
}

fn ensure_monotonic(
    track: &'static str,
    previous: Option<TimeTick>,
    current: TimeTick,
) -> Result<(), ProjectWriterError> {
    if previous.is_some_and(|previous| current < previous) {
        return Err(ProjectWriterError::NonMonotonicTrack {
            track,
            previous: previous.expect("checked as present"),
            current,
        });
    }
    Ok(())
}

struct JsonlWriter {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl JsonlWriter {
    fn create_new(path: PathBuf) -> Result<Self, ProjectWriterError> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| ProjectWriterError::Open {
                path: path.clone(),
                source,
            })?;
        Ok(Self {
            path,
            writer: BufWriter::new(file),
        })
    }

    fn append<T: Serialize>(&mut self, record: &T) -> Result<(), ProjectWriterError> {
        serde_json::to_writer(&mut self.writer, record).map_err(ProjectWriterError::Serialize)?;
        self.writer
            .write_all(b"\n")
            .map_err(|source| ProjectWriterError::Write {
                path: self.path.clone(),
                source,
            })
    }

    fn flush(&mut self) -> Result<(), ProjectWriterError> {
        self.writer
            .flush()
            .map_err(|source| ProjectWriterError::Write {
                path: self.path.clone(),
                source,
            })
    }

    fn checkpoint(&mut self) -> Result<(), ProjectWriterError> {
        self.flush()?;
        self.writer
            .get_ref()
            .sync_data()
            .map_err(|source| ProjectWriterError::Sync {
                path: self.path.clone(),
                source,
            })
    }
}

#[derive(Debug, Error)]
pub enum ProjectWriterError {
    #[error("could not create JSONL track {path}: {source}")]
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not serialize JSONL record: {0}")]
    Serialize(serde_json::Error),
    #[error("could not write JSONL track {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not durably sync JSONL track {path}: {source}")]
    Sync {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{track} track moved backwards from {previous:?} to {current:?}")]
    NonMonotonicTrack {
        track: &'static str,
        previous: TimeTick,
        current: TimeTick,
    },
    #[error(
        "writer clock moved backwards to {current:?}; last flush={last_flush:?}, last checkpoint={last_checkpoint:?}"
    )]
    ClockRegression {
        current: TimeTick,
        last_flush: TimeTick,
        last_checkpoint: TimeTick,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{CursorEventKind, EVENT_SCHEMA_VERSION};
    use crate::geometry::{PhysicalRect, PhysicalSize};
    use crate::project::{
        CaptureDescriptor, CaptureKind, PROJECT_SCHEMA_VERSION, ProjectManifest, ProjectState,
        ScreenMediaDescriptor, TimebaseDescriptor, TrackPaths,
    };
    use crate::project_io::RecordingLock;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn writes_tracks_and_services_flush_and_checkpoint_deadlines() {
        let root = std::env::temp_dir().join(format!("panzo-writer-{}", Uuid::new_v4()));
        let layout = ProjectLayout::create_new(&root, &manifest(), &recording_lock()).unwrap();
        let mut writer = ProjectWriter::create(&layout).unwrap();

        writer
            .append_input(&PersistedInputEvent::Cursor(cursor(TimeTick::ZERO)))
            .unwrap();
        writer
            .append_input(&PersistedInputEvent::Cursor(cursor(TimeTick::ZERO)))
            .unwrap();
        writer
            .append_journal(
                TimeTick::ZERO,
                JournalOperation::ProjectCreated,
                JournalResult::Success,
                None,
            )
            .unwrap();

        let flush = writer.service_due(TimeTick(FLUSH_INTERVAL_TICK)).unwrap();
        assert_eq!(
            flush,
            WriterServiceReport {
                flushed: true,
                checkpointed: false
            }
        );
        let checkpoint = writer
            .service_due(TimeTick(CHECKPOINT_INTERVAL_TICK))
            .unwrap();
        assert_eq!(
            checkpoint,
            WriterServiceReport {
                flushed: true,
                checkpointed: true
            }
        );
        let stats = writer.finish().unwrap();
        assert_eq!(stats.cursor_records, 2);
        assert_eq!(stats.flushes, 3);
        assert_eq!(stats.durable_checkpoints, 2);
        assert_eq!(
            fs::read_to_string(layout.cursor_events_path())
                .unwrap()
                .lines()
                .count(),
            2
        );
        assert_eq!(
            fs::read_to_string(layout.journal_path())
                .unwrap()
                .lines()
                .count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_time_regression_per_track() {
        let root = std::env::temp_dir().join(format!("panzo-writer-{}", Uuid::new_v4()));
        let layout = ProjectLayout::create_new(&root, &manifest(), &recording_lock()).unwrap();
        let mut writer = ProjectWriter::create(&layout).unwrap();
        writer
            .append_input(&PersistedInputEvent::Cursor(cursor(TimeTick(2))))
            .unwrap();
        assert!(matches!(
            writer.append_input(&PersistedInputEvent::Cursor(cursor(TimeTick(1)))),
            Err(ProjectWriterError::NonMonotonicTrack {
                track: "cursor",
                ..
            })
        ));
        drop(writer);
        fs::remove_dir_all(root).unwrap();
    }

    fn cursor(time_tick: TimeTick) -> CursorEvent {
        CursorEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            id: 1,
            time_tick,
            kind: CursorEventKind::Move,
            desktop_x: 10,
            desktop_y: 10,
            content_x: 10,
            content_y: 10,
            normalized_x: 0.01,
            normalized_y: 0.01,
            visible: true,
            geometry_revision: 1,
        }
    }

    fn recording_lock() -> RecordingLock {
        RecordingLock {
            schema_version: 1,
            process_id: 42,
            session_id: "session-1".into(),
            last_checkpoint_tick: TimeTick::ZERO,
        }
    }

    fn manifest() -> ProjectManifest {
        ProjectManifest {
            schema_version: PROJECT_SCHEMA_VERSION,
            project_id: Uuid::nil(),
            app_version: "0.1.0".into(),
            state: ProjectState::Recording,
            created_at_utc: "2026-08-31T08:00:00Z".into(),
            timebase: TimebaseDescriptor {
                ticks_per_second: TICKS_PER_SECOND,
                session_start_qpc: 10,
                qpc_frequency: TICKS_PER_SECOND,
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
}
