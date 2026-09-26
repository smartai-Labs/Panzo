//! Pure gesture range math, shared by ghost drawing and commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeDrag {
    Move,
    TrimStart,
    TrimEnd,
}

pub fn adjusted_range(
    original: (i64, i64),
    delta: i64,
    bounds: (i64, i64),
    mode: RangeDrag,
    snaps: &[i64],
    tolerance: u64,
) -> (i64, i64) {
    let (start, end) = original;
    let (lower, upper) = bounds;
    let snap = |value: i64| {
        snaps
            .iter()
            .copied()
            .filter(|candidate| candidate.abs_diff(value) <= tolerance)
            .min_by_key(|candidate| candidate.abs_diff(value))
    };
    match mode {
        RangeDrag::Move => {
            let duration = end - start;
            let candidate = start.saturating_add(delta).clamp(lower, upper - duration);
            let near_start = snap(candidate);
            let near_end = snap(candidate + duration).map(|value| value - duration);
            let snapped = near_start
                .into_iter()
                .chain(near_end)
                .min_by_key(|value| value.abs_diff(candidate))
                .unwrap_or(candidate);
            let result = snapped.clamp(lower, upper - duration);
            (result, result + duration)
        }
        RangeDrag::TrimStart => {
            let candidate = start.saturating_add(delta);
            (
                snap(candidate).unwrap_or(candidate).clamp(lower, end - 1),
                end,
            )
        }
        RangeDrag::TrimEnd => (
            start,
            snap(end.saturating_add(delta))
                .unwrap_or(end.saturating_add(delta))
                .clamp(start + 1, upper),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn move_clamps_without_shortening_and_trim_never_collapses() {
        assert_eq!(
            adjusted_range((10, 20), 100, (0, 30), RangeDrag::Move, &[], 0),
            (20, 30)
        );
        assert_eq!(
            adjusted_range((10, 20), -100, (0, 30), RangeDrag::Move, &[], 0),
            (0, 10)
        );
        assert_eq!(
            adjusted_range((10, 20), 100, (0, 30), RangeDrag::TrimStart, &[], 0),
            (19, 20)
        );
        assert_eq!(
            adjusted_range((10, 20), -100, (0, 30), RangeDrag::TrimEnd, &[], 0),
            (10, 11)
        );
    }
    #[test]
    fn snapping_stays_inside_neighbor_bounds() {
        assert_eq!(
            adjusted_range((10, 20), 8, (5, 40), RangeDrag::Move, &[19, 50], 2),
            (19, 29)
        );
        assert_eq!(
            adjusted_range((10, 20), 22, (5, 40), RangeDrag::TrimEnd, &[41], 2),
            (10, 40)
        );
    }
}
