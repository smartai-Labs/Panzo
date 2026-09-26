use crate::geometry::{NormalizedPoint, PhysicalPoint, PhysicalRect};
use crate::time::{SessionClock, TimeError, TimeTick};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const EVENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CursorEventKind {
    Move,
    Visibility,
    ButtonAnchor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorEvent {
    pub schema_version: u32,
    pub id: u64,
    pub time_tick: TimeTick,
    pub kind: CursorEventKind,
    pub desktop_x: i32,
    pub desktop_y: i32,
    pub content_x: i32,
    pub content_y: i32,
    pub normalized_x: f64,
    pub normalized_y: f64,
    pub visible: bool,
    pub geometry_revision: u32,
}

impl CursorEvent {
    pub fn normalized_point(&self) -> Option<NormalizedPoint> {
        NormalizedPoint::new(self.normalized_x, self.normalized_y).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseAction {
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawInputKind {
    Cursor {
        visible: bool,
    },
    Button {
        button: MouseButton,
        action: MouseAction,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StampedInputEvent {
    pub sequence: u64,
    pub qpc: i64,
    pub desktop_point: PhysicalPoint,
    pub kind: RawInputKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PersistedInputEvent {
    Cursor(CursorEvent),
    Click(ClickEvent),
}

#[derive(Debug, Clone)]
pub struct InputEventNormalizer {
    clock: SessionClock,
    capture_rect: PhysicalRect,
    geometry_revision: u32,
    next_cursor_id: u64,
    next_click_id: u64,
    last_qpc: Option<i64>,
    last_cursor_state: Option<(PhysicalPoint, bool)>,
    dropped_before_epoch: u64,
    omitted_duplicate_cursor: u64,
}

impl InputEventNormalizer {
    pub fn new(
        clock: SessionClock,
        capture_rect: PhysicalRect,
        geometry_revision: u32,
    ) -> Result<Self, InputNormalizationError> {
        if capture_rect.size().is_empty() {
            return Err(InputNormalizationError::EmptyCaptureGeometry);
        }
        Ok(Self {
            clock,
            capture_rect,
            geometry_revision,
            next_cursor_id: 1,
            next_click_id: 1,
            last_qpc: None,
            last_cursor_state: None,
            dropped_before_epoch: 0,
            omitted_duplicate_cursor: 0,
        })
    }

    pub const fn dropped_before_epoch(&self) -> u64 {
        self.dropped_before_epoch
    }

    pub const fn omitted_duplicate_cursor(&self) -> u64 {
        self.omitted_duplicate_cursor
    }

    pub fn update_capture_rect(
        &mut self,
        capture_rect: PhysicalRect,
    ) -> Result<bool, InputNormalizationError> {
        if capture_rect.size().is_empty() {
            return Err(InputNormalizationError::EmptyCaptureGeometry);
        }
        if self.capture_rect == capture_rect {
            return Ok(false);
        }
        self.capture_rect = capture_rect;
        self.geometry_revision = self.geometry_revision.saturating_add(1);
        self.last_cursor_state = None;
        Ok(true)
    }

    pub fn normalize_batch(
        &mut self,
        mut raw_events: Vec<StampedInputEvent>,
    ) -> Result<Vec<PersistedInputEvent>, InputNormalizationError> {
        raw_events.sort_by_key(|event| (event.qpc, event.sequence));
        let mut output = Vec::with_capacity(raw_events.len());
        for raw in raw_events {
            if raw.qpc < self.clock.session_start_qpc {
                self.dropped_before_epoch += 1;
                continue;
            }
            if let Some(previous) = self.last_qpc
                && raw.qpc < previous
            {
                return Err(InputNormalizationError::NonMonotonicQpc {
                    previous,
                    current: raw.qpc,
                });
            }
            self.last_qpc = Some(raw.qpc);
            let time_tick = self.clock.event_tick(raw.qpc)?;
            match raw.kind {
                RawInputKind::Cursor { visible } => {
                    let current_state = (raw.desktop_point, visible);
                    if self.last_cursor_state == Some(current_state) {
                        self.omitted_duplicate_cursor += 1;
                        continue;
                    }
                    let kind = if self
                        .last_cursor_state
                        .is_some_and(|(_, previous_visible)| previous_visible != visible)
                    {
                        CursorEventKind::Visibility
                    } else {
                        CursorEventKind::Move
                    };
                    self.last_cursor_state = Some(current_state);
                    output.push(PersistedInputEvent::Cursor(self.cursor_event(
                        time_tick,
                        raw.desktop_point,
                        kind,
                        visible,
                    )));
                }
                RawInputKind::Button { button, action } => {
                    output.push(PersistedInputEvent::Cursor(self.cursor_event(
                        time_tick,
                        raw.desktop_point,
                        CursorEventKind::ButtonAnchor,
                        true,
                    )));
                    output.push(PersistedInputEvent::Click(self.click_event(
                        time_tick,
                        raw.desktop_point,
                        button,
                        action,
                    )));
                }
            }
        }
        Ok(output)
    }

    fn cursor_event(
        &mut self,
        time_tick: TimeTick,
        desktop_point: PhysicalPoint,
        kind: CursorEventKind,
        system_visible: bool,
    ) -> CursorEvent {
        let coordinates = self.coordinates(desktop_point);
        let event = CursorEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            id: self.next_cursor_id,
            time_tick,
            kind,
            desktop_x: desktop_point.x,
            desktop_y: desktop_point.y,
            content_x: coordinates.content_x,
            content_y: coordinates.content_y,
            normalized_x: coordinates.normalized_x,
            normalized_y: coordinates.normalized_y,
            visible: system_visible && coordinates.inside_capture,
            geometry_revision: self.geometry_revision,
        };
        self.next_cursor_id += 1;
        event
    }

    fn click_event(
        &mut self,
        time_tick: TimeTick,
        desktop_point: PhysicalPoint,
        button: MouseButton,
        action: MouseAction,
    ) -> ClickEvent {
        let coordinates = self.coordinates(desktop_point);
        let event = ClickEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            id: format!("click-{}", self.next_click_id),
            time_tick,
            button,
            action,
            desktop_x: desktop_point.x,
            desktop_y: desktop_point.y,
            normalized_x: coordinates.normalized_x,
            normalized_y: coordinates.normalized_y,
            inside_capture: coordinates.inside_capture,
            geometry_revision: self.geometry_revision,
        };
        self.next_click_id += 1;
        event
    }

    fn coordinates(&self, desktop_point: PhysicalPoint) -> EventCoordinates {
        let content_x = i64::from(desktop_point.x) - i64::from(self.capture_rect.x);
        let content_y = i64::from(desktop_point.y) - i64::from(self.capture_rect.y);
        EventCoordinates {
            content_x: saturating_i32(content_x),
            content_y: saturating_i32(content_y),
            normalized_x: content_x as f64 / f64::from(self.capture_rect.width),
            normalized_y: content_y as f64 / f64::from(self.capture_rect.height),
            inside_capture: self.capture_rect.contains(desktop_point),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct EventCoordinates {
    content_x: i32,
    content_y: i32,
    normalized_x: f64,
    normalized_y: f64,
    inside_capture: bool,
}

fn saturating_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum InputNormalizationError {
    #[error("capture geometry must not be empty")]
    EmptyCaptureGeometry,
    #[error("raw input QPC moved backwards from {previous} to {current}")]
    NonMonotonicQpc { previous: i64, current: i64 },
    #[error(transparent)]
    Time(#[from] TimeError),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickEvent {
    pub schema_version: u32,
    pub id: String,
    pub time_tick: TimeTick,
    pub button: MouseButton,
    pub action: MouseAction,
    pub desktop_x: i32,
    pub desktop_y: i32,
    pub normalized_x: f64,
    pub normalized_y: f64,
    pub inside_capture: bool,
    pub geometry_revision: u32,
}

impl ClickEvent {
    pub fn normalized_point(&self) -> Option<NormalizedPoint> {
        NormalizedPoint::new(self.normalized_x, self.normalized_y).ok()
    }

    pub const fn triggers_camera(&self) -> bool {
        self.inside_capture
            && matches!(self.action, MouseAction::Down)
            && matches!(self.button, MouseButton::Left | MouseButton::Right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_inside_left_or_right_down_triggers_camera() {
        let mut event = ClickEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            id: "click-1".into(),
            time_tick: TimeTick(1),
            button: MouseButton::Left,
            action: MouseAction::Down,
            desktop_x: 10,
            desktop_y: 20,
            normalized_x: 0.5,
            normalized_y: 0.5,
            inside_capture: true,
            geometry_revision: 0,
        };
        assert!(event.triggers_camera());

        event.button = MouseButton::Middle;
        assert!(!event.triggers_camera());

        event.button = MouseButton::Right;
        event.action = MouseAction::Up;
        assert!(!event.triggers_camera());
    }

    #[test]
    fn normalizer_discards_pre_epoch_and_anchors_clicks() {
        let session_clock = SessionClock::establish(10_000_000, 100).unwrap();
        let mut normalizer = InputEventNormalizer::new(
            session_clock,
            PhysicalRect {
                x: -100,
                y: 50,
                width: 200,
                height: 100,
            },
            7,
        )
        .unwrap();
        let events = normalizer
            .normalize_batch(vec![
                StampedInputEvent {
                    sequence: 1,
                    qpc: 99,
                    desktop_point: PhysicalPoint { x: 0, y: 100 },
                    kind: RawInputKind::Cursor { visible: true },
                },
                StampedInputEvent {
                    sequence: 3,
                    qpc: 120,
                    desktop_point: PhysicalPoint { x: 0, y: 100 },
                    kind: RawInputKind::Button {
                        button: MouseButton::Left,
                        action: MouseAction::Down,
                    },
                },
                StampedInputEvent {
                    sequence: 2,
                    qpc: 110,
                    desktop_point: PhysicalPoint { x: -50, y: 75 },
                    kind: RawInputKind::Cursor { visible: true },
                },
            ])
            .unwrap();

        assert_eq!(normalizer.dropped_before_epoch(), 1);
        assert_eq!(events.len(), 3);
        let PersistedInputEvent::Cursor(move_event) = &events[0] else {
            panic!("expected cursor event");
        };
        assert_eq!(move_event.time_tick, TimeTick(10));
        assert_eq!(move_event.geometry_revision, 7);
        let PersistedInputEvent::Cursor(anchor) = &events[1] else {
            panic!("expected cursor anchor");
        };
        let PersistedInputEvent::Click(click_event) = &events[2] else {
            panic!("expected click");
        };
        assert_eq!(anchor.kind, CursorEventKind::ButtonAnchor);
        assert_eq!(anchor.time_tick, click_event.time_tick);
        assert_eq!(click_event.normalized_x, 0.5);
        assert_eq!(click_event.normalized_y, 0.5);
        assert!(click_event.inside_capture);
    }

    #[test]
    fn normalizer_keeps_outside_click_but_hides_cursor() {
        let session_clock = SessionClock::establish(10_000_000, 0).unwrap();
        let mut normalizer = InputEventNormalizer::new(
            session_clock,
            PhysicalRect {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
            0,
        )
        .unwrap();
        let events = normalizer
            .normalize_batch(vec![StampedInputEvent {
                sequence: 1,
                qpc: 10,
                desktop_point: PhysicalPoint { x: 150, y: 50 },
                kind: RawInputKind::Button {
                    button: MouseButton::Right,
                    action: MouseAction::Down,
                },
            }])
            .unwrap();
        let PersistedInputEvent::Cursor(cursor) = &events[0] else {
            panic!("expected cursor");
        };
        let PersistedInputEvent::Click(click_event) = &events[1] else {
            panic!("expected click");
        };
        assert!(!cursor.visible);
        assert!(!click_event.inside_capture);
        assert_eq!(click_event.normalized_x, 1.5);
    }

    #[test]
    fn moving_capture_window_updates_coordinates_and_geometry_revision() {
        let session_clock = SessionClock::establish(10_000_000, 0).unwrap();
        let mut normalizer = InputEventNormalizer::new(
            session_clock,
            PhysicalRect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
            0,
        )
        .unwrap();
        assert!(
            normalizer
                .update_capture_rect(PhysicalRect {
                    x: 100,
                    y: 50,
                    width: 800,
                    height: 600,
                })
                .unwrap()
        );
        let events = normalizer
            .normalize_batch(vec![StampedInputEvent {
                sequence: 1,
                qpc: 10,
                desktop_point: PhysicalPoint { x: 500, y: 350 },
                kind: RawInputKind::Cursor { visible: true },
            }])
            .unwrap();
        let PersistedInputEvent::Cursor(cursor) = &events[0] else {
            panic!("expected cursor");
        };
        assert_eq!(cursor.geometry_revision, 1);
        assert_eq!(cursor.content_x, 400);
        assert_eq!(cursor.content_y, 300);
        assert_eq!(cursor.normalized_x, 0.5);
        assert_eq!(cursor.normalized_y, 0.5);
    }
}
