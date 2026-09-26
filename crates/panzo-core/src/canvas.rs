use serde::{Deserialize, Serialize};

/// Actual project canvas dimensions, independent of spatial cropping and source media.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasSize {
    pub width: u32,
    pub height: u32,
}

impl CanvasSize {
    pub fn is_valid(self) -> bool {
        (64..=8192).contains(&self.width)
            && (64..=8192).contains(&self.height)
            && self.width.is_multiple_of(2)
            && self.height.is_multiple_of(2)
            && u64::from(self.width) * u64::from(self.height) <= 33_554_432
    }

    pub fn from_ratio(source: (u32, u32), ratio: (u32, u32)) -> Self {
        let edge = source.0.max(source.1).clamp(64, 8192);
        let divisor = ratio.0.max(ratio.1).max(1);
        let even = |n: u64| u32::try_from(n / 2 * 2).unwrap_or(8192).clamp(64, 8192);
        let mut size = Self {
            width: even(u64::from(edge) * u64::from(ratio.0) / u64::from(divisor)),
            height: even(u64::from(edge) * u64::from(ratio.1) / u64::from(divisor)),
        };
        // Keep presets inside the same allocation limit as custom dimensions.
        while !size.is_valid() {
            let longest = size.width.max(size.height);
            size = Self {
                width: even(u64::from(size.width) * u64::from(longest - 2) / u64::from(longest)),
                height: even(u64::from(size.height) * u64::from(longest - 2) / u64::from(longest)),
            };
        }
        size
    }

    /// 1080-class output follows project orientation and never enlarges a small canvas.
    #[allow(clippy::cast_sign_loss)] // Dimensions and scale are nonnegative.
    pub fn hd(self) -> Self {
        let (max_width, max_height) = match self.width.cmp(&self.height) {
            std::cmp::Ordering::Greater => (1920, 1080),
            std::cmp::Ordering::Less => (1080, 1920),
            std::cmp::Ordering::Equal => (1080, 1080),
        };
        let scale = (f64::from(max_width) / f64::from(self.width))
            .min(f64::from(max_height) / f64::from(self.height))
            .min(1.0);
        Self {
            width: ((f64::from(self.width) * scale).round() as u32 / 2 * 2).max(2),
            height: ((f64::from(self.height) * scale).round() as u32 / 2 * 2).max(2),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canvas_dimensions_preserve_orientation_and_bound_allocations() {
        let portrait = CanvasSize::from_ratio((2560, 1440), (9, 16));
        assert_eq!(
            portrait,
            CanvasSize {
                width: 1440,
                height: 2560
            }
        );
        assert_eq!(
            portrait.hd(),
            CanvasSize {
                width: 1080,
                height: 1920
            }
        );
        assert_eq!(
            CanvasSize {
                width: 800,
                height: 600
            }
            .hd(),
            CanvasSize {
                width: 800,
                height: 600
            }
        );
        assert!(portrait.is_valid());
        assert!(CanvasSize::from_ratio((8192, 8192), (1, 1)).is_valid());
        assert!(CanvasSize::from_ratio((u32::MAX, 1), (u32::MAX, u32::MAX)).is_valid());
        for size in [
            CanvasSize {
                width: 0,
                height: 1080,
            },
            CanvasSize {
                width: 1919,
                height: 1080,
            },
            CanvasSize {
                width: 8192,
                height: 8192,
            },
        ] {
            assert!(!size.is_valid());
        }
    }
}
