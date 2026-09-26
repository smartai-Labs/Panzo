use panzo_core::{TICKS_PER_SECOND, TimeTick};

const MIN_VISIBLE_TICKS: i64 = TICKS_PER_SECOND / 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineViewport {
    project_duration: TimeTick,
    visible_start: TimeTick,
    visible_duration: TimeTick,
}

impl TimelineViewport {
    /// Creates a viewport showing the complete project.
    ///
    /// # Panics
    ///
    /// Panics when `project_duration` is zero or negative.
    pub fn fit(project_duration: TimeTick) -> Self {
        assert!(project_duration > TimeTick::ZERO);
        Self {
            project_duration,
            visible_start: TimeTick::ZERO,
            visible_duration: project_duration,
        }
    }

    pub const fn visible_start(self) -> TimeTick {
        self.visible_start
    }

    pub fn visible_end(self) -> TimeTick {
        TimeTick(
            self.visible_start
                .as_i64()
                .saturating_add(self.visible_duration.as_i64())
                .min(self.project_duration.as_i64()),
        )
    }

    pub const fn visible_duration(self) -> TimeTick {
        self.visible_duration
    }

    pub fn is_fit(self) -> bool {
        self.visible_start == TimeTick::ZERO && self.visible_duration == self.project_duration
    }

    pub fn contains(self, tick: TimeTick) -> bool {
        self.visible_start <= tick && tick <= self.visible_end()
    }

    pub fn tick_to_offset(self, tick: TimeTick, pixel_width: i32) -> i32 {
        let width = i64::from(pixel_width.max(1));
        let relative = tick.as_i64().saturating_sub(self.visible_start.as_i64());
        i32::try_from(
            i128::from(width) * i128::from(relative)
                / i128::from(self.visible_duration.as_i64().max(1)),
        )
        .unwrap_or(if relative < 0 { i32::MIN } else { i32::MAX })
    }

    pub fn offset_to_tick(self, pixel_offset: i32, pixel_width: i32) -> TimeTick {
        let width = i64::from(pixel_width.max(1));
        let offset = i64::from(pixel_offset.clamp(0, pixel_width.max(1)));
        TimeTick(
            self.visible_start.as_i64()
                + i64::try_from(
                    i128::from(self.visible_duration.as_i64()) * i128::from(offset)
                        / i128::from(width),
                )
                .unwrap_or(self.visible_duration.as_i64()),
        )
    }

    pub fn pixel_delta_to_tick(self, pixels: i32, pixel_width: i32) -> TimeTick {
        let width = i64::from(pixel_width.max(1));
        TimeTick(
            i64::try_from(
                i128::from(self.visible_duration.as_i64()) * i128::from(pixels) / i128::from(width),
            )
            .unwrap_or_default(),
        )
    }

    pub fn zoom_at(&mut self, anchor: TimeTick, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let minimum = MIN_VISIBLE_TICKS.min(self.project_duration.as_i64());
        let old_duration = self.visible_duration.as_i64();
        let new_duration = ((old_duration as f64 / factor).round() as i64)
            .clamp(minimum, self.project_duration.as_i64());
        let anchor = anchor
            .as_i64()
            .clamp(self.visible_start.as_i64(), self.visible_end().as_i64());
        let relative = (anchor - self.visible_start.as_i64()) as f64 / old_duration as f64;
        let start = anchor - (relative * new_duration as f64).round() as i64;
        self.visible_duration = TimeTick(new_duration);
        self.visible_start = TimeTick(start);
        self.clamp_start();
    }

    pub fn pan_by(&mut self, delta: TimeTick) {
        self.visible_start = TimeTick(self.visible_start.as_i64().saturating_add(delta.as_i64()));
        self.clamp_start();
    }

    pub fn ensure_visible(&mut self, tick: TimeTick) {
        if self.contains(tick) {
            return;
        }
        let margin = self.visible_duration.as_i64() / 10;
        if tick < self.visible_start {
            self.visible_start = TimeTick(tick.as_i64().saturating_sub(margin));
        } else {
            self.visible_start = TimeTick(
                tick.as_i64()
                    .saturating_sub(self.visible_duration.as_i64() - margin),
            );
        }
        self.clamp_start();
    }

    pub fn reset(&mut self) {
        self.visible_start = TimeTick::ZERO;
        self.visible_duration = self.project_duration;
    }

    pub fn set_duration(&mut self, duration: TimeTick) {
        let was_fit = self.is_fit();
        self.project_duration = duration;
        if was_fit {
            self.reset();
        } else {
            self.visible_duration = self.visible_duration.min(duration);
            self.clamp_start();
        }
    }

    fn clamp_start(&mut self) {
        let maximum = self
            .project_duration
            .as_i64()
            .saturating_sub(self.visible_duration.as_i64())
            .max(0);
        self.visible_start = TimeTick(self.visible_start.as_i64().clamp(0, maximum));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_mapping_round_trips_pixels() {
        let viewport = TimelineViewport::fit(TimeTick(30 * TICKS_PER_SECOND));
        let tick = viewport.offset_to_tick(500, 1_000);
        assert_eq!(tick, TimeTick(15 * TICKS_PER_SECOND));
        assert_eq!(viewport.tick_to_offset(tick, 1_000), 500);
    }

    #[test]
    fn anchored_zoom_keeps_tick_under_pointer() {
        let mut viewport = TimelineViewport::fit(TimeTick(60 * TICKS_PER_SECOND));
        let anchor = TimeTick(45 * TICKS_PER_SECOND);
        viewport.zoom_at(anchor, 2.0);
        assert_eq!(viewport.visible_duration(), TimeTick(30 * TICKS_PER_SECOND));
        assert_eq!(viewport.tick_to_offset(anchor, 1_000), 750);
    }

    #[test]
    fn repeated_zoom_reaches_sub_millisecond_per_pixel() {
        let mut viewport = TimelineViewport::fit(TimeTick(30 * 60 * TICKS_PER_SECOND));
        let anchor = TimeTick(15 * 60 * TICKS_PER_SECOND);
        for _ in 0..20 {
            viewport.zoom_at(anchor, 2.0);
        }
        assert_eq!(viewport.visible_duration(), TimeTick(MIN_VISIBLE_TICKS));
        assert!(viewport.pixel_delta_to_tick(1, 1_000).as_i64() <= 5_000);
    }

    #[test]
    fn pan_and_follow_stay_inside_project() {
        let mut viewport = TimelineViewport::fit(TimeTick(10 * TICKS_PER_SECOND));
        viewport.zoom_at(TimeTick(5 * TICKS_PER_SECOND), 2.0);
        viewport.pan_by(TimeTick(100 * TICKS_PER_SECOND));
        assert_eq!(viewport.visible_end(), TimeTick(10 * TICKS_PER_SECOND));
        viewport.ensure_visible(TimeTick::ZERO);
        assert_eq!(viewport.visible_start(), TimeTick::ZERO);
    }
}
