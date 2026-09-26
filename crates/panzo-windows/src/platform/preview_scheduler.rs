//! Presentation policy independent of Win32: continuous input must not starve completed
//! frames, while old gestures / reverse-direction work must never overwrite new intent.
use panzo_core::TimeTick;
use std::time::Duration;

const MAX_SCRUB_AGE: Duration = Duration::from_millis(250);

pub struct ScrubPresentationGate {
    first_generation: Option<u64>,
    latest_generation: u64,
    presented_generation: u64,
    target: Option<TimeTick>,
    direction: std::cmp::Ordering,
}

impl Default for ScrubPresentationGate {
    fn default() -> Self {
        Self {
            first_generation: None,
            latest_generation: 0,
            presented_generation: 0,
            target: None,
            direction: std::cmp::Ordering::Equal,
        }
    }
}

impl ScrubPresentationGate {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Returns true if in-flight work is incompatible with the new drag direction.
    pub fn move_to(&mut self, target: TimeTick) -> bool {
        let direction = self
            .target
            .map_or(std::cmp::Ordering::Equal, |previous| target.cmp(&previous));
        let reversed = direction != std::cmp::Ordering::Equal
            && self.direction != std::cmp::Ordering::Equal
            && direction != self.direction;
        if reversed {
            self.reset();
        }
        self.target = Some(target);
        if direction != std::cmp::Ordering::Equal {
            self.direction = direction;
        }
        reversed
    }

    pub fn submitted(&mut self, generation: u64) {
        self.first_generation.get_or_insert(generation);
        self.latest_generation = generation;
    }

    pub fn accept(&mut self, generation: u64, age: Duration) -> bool {
        if self.first_generation.is_none_or(|first| generation < first)
            || generation > self.latest_generation
            || generation <= self.presented_generation
            || age > MAX_SCRUB_AGE
        {
            return false;
        }
        self.presented_generation = generation;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moving_target_still_accepts_recent_completed_frame() {
        let mut gate = ScrubPresentationGate::default();
        gate.move_to(TimeTick(1));
        gate.submitted(1);
        gate.move_to(TimeTick(2));
        gate.submitted(2);
        assert!(gate.accept(1, Duration::from_millis(80)));
        assert!(!gate.accept(1, Duration::ZERO));
        assert!(gate.accept(2, Duration::from_millis(20)));
    }
    #[test]
    fn reversal_invalidates_previous_direction() {
        let mut gate = ScrubPresentationGate::default();
        gate.move_to(TimeTick(1));
        gate.submitted(10);
        gate.move_to(TimeTick(3));
        gate.submitted(11);
        assert!(gate.move_to(TimeTick(2)));
        assert!(!gate.accept(11, Duration::ZERO));
        gate.submitted(12);
        assert!(!gate.accept(10, Duration::ZERO));
        assert!(gate.accept(12, Duration::ZERO));
    }
    #[test]
    fn old_gesture_and_expired_result_are_rejected() {
        let mut gate = ScrubPresentationGate::default();
        gate.submitted(1);
        gate.reset();
        gate.submitted(2);
        assert!(!gate.accept(1, Duration::ZERO));
        assert!(!gate.accept(2, Duration::from_millis(251)));
        assert!(!gate.accept(3, Duration::ZERO));
    }
}
