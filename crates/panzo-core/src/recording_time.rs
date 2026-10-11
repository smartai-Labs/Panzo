//! Remove paused wall-time intervals identically from video, cursor, camera and audio timestamps.
use crate::TimeTick;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordingTimeline {
    pub pauses: Vec<(TimeTick, Option<TimeTick>)>,
}
impl RecordingTimeline {
    pub fn paused(&self) -> bool {
        self.pauses.last().is_some_and(|(_, end)| end.is_none())
    }
    pub fn set_paused(&mut self, wall: TimeTick, paused: bool) {
        if paused == self.paused() {
            return;
        }
        if paused {
            self.pauses.push((wall, None));
        } else if let Some((start, end)) = self.pauses.last_mut() {
            *end = Some(wall.max(*start));
        }
    }
    pub fn elapsed(&self, wall: TimeTick) -> TimeTick {
        let removed = self
            .pauses
            .iter()
            .take_while(|(start, _)| *start < wall)
            .fold(0i64, |sum, (start, end)| {
                sum.saturating_add(end.unwrap_or(wall).min(wall).0 - start.0)
            });
        TimeTick(wall.0.saturating_sub(removed).max(0))
    }
    pub fn map(&self, wall: TimeTick) -> Option<TimeTick> {
        if wall < TimeTick::ZERO
            || self
                .pauses
                .iter()
                .any(|(start, end)| wall >= *start && end.is_none_or(|end| wall < end))
        {
            None
        } else {
            Some(self.elapsed(wall))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pause_edges_and_late_packets_share_one_mapping() {
        let mut t = RecordingTimeline::default();
        t.set_paused(TimeTick(100), true);
        assert_eq!(t.elapsed(TimeTick(200)), TimeTick(100));
        assert_eq!(t.map(TimeTick(100)), None);
        t.set_paused(TimeTick(300), false);
        assert_eq!(t.map(TimeTick(299)), None);
        assert_eq!(t.map(TimeTick(300)), Some(TimeTick(100)));
        assert_eq!(t.map(TimeTick(90)), Some(TimeTick(90)));
        t.set_paused(TimeTick(400), true);
        t.set_paused(TimeTick(450), false);
        assert_eq!(t.map(TimeTick(500)), Some(TimeTick(250)));
    }
}
