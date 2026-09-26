use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CoordinateSpace {
    DesktopPhysicalPx,
    CaptureContentPx,
    CaptureNormalized,
    OutputPx,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalSize {
    pub width: u32,
    pub height: u32,
}

impl PhysicalSize {
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn aspect_ratio(self) -> Result<f64, GeometryError> {
        if self.is_empty() {
            return Err(GeometryError::EmptySize);
        }
        Ok(f64::from(self.width) / f64::from(self.height))
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl PhysicalRect {
    pub const fn size(self) -> PhysicalSize {
        PhysicalSize {
            width: self.width,
            height: self.height,
        }
    }

    pub fn contains(self, point: PhysicalPoint) -> bool {
        let right = i64::from(self.x) + i64::from(self.width);
        let bottom = i64::from(self.y) + i64::from(self.height);

        i64::from(point.x) >= i64::from(self.x)
            && i64::from(point.x) < right
            && i64::from(point.y) >= i64::from(self.y)
            && i64::from(point.y) < bottom
    }

    pub fn desktop_to_content(self, point: PhysicalPoint) -> Option<PhysicalPoint> {
        self.contains(point).then(|| PhysicalPoint {
            x: point.x - self.x,
            y: point.y - self.y,
        })
    }

    pub fn desktop_to_normalized(
        self,
        point: PhysicalPoint,
    ) -> Result<NormalizedPoint, GeometryError> {
        let content = self
            .desktop_to_content(point)
            .ok_or(GeometryError::PointOutsideRect)?;
        NormalizedPoint::from_content(content, self.size())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedPoint {
    pub x: f64,
    pub y: f64,
}

impl NormalizedPoint {
    pub fn new(x: f64, y: f64) -> Result<Self, GeometryError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(GeometryError::NonFiniteCoordinate);
        }
        if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
            return Err(GeometryError::NormalizedCoordinateOutOfRange { x, y });
        }
        Ok(Self { x, y })
    }

    pub fn from_content(point: PhysicalPoint, size: PhysicalSize) -> Result<Self, GeometryError> {
        if size.is_empty() {
            return Err(GeometryError::EmptySize);
        }
        if point.x < 0
            || point.y < 0
            || i64::from(point.x) >= i64::from(size.width)
            || i64::from(point.y) >= i64::from(size.height)
        {
            return Err(GeometryError::PointOutsideRect);
        }

        Self::new(
            f64::from(point.x) / f64::from(size.width),
            f64::from(point.y) / f64::from(size.height),
        )
    }

    pub fn distance(self, other: Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        dx.hypot(dy)
    }

    pub fn clamped_for_scale(self, scale: f64) -> Result<Self, GeometryError> {
        if !scale.is_finite() || scale < 1.0 {
            return Err(GeometryError::InvalidScale(scale));
        }
        let half_extent = 0.5 / scale;
        Ok(Self {
            x: self.x.clamp(half_extent, 1.0 - half_extent),
            y: self.y.clamp(half_extent, 1.0 - half_extent),
        })
    }
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum GeometryError {
    #[error("physical size must not be empty")]
    EmptySize,
    #[error("point lies outside the rectangle")]
    PointOutsideRect,
    #[error("coordinate must be finite")]
    NonFiniteCoordinate,
    #[error("normalized coordinate ({x}, {y}) is outside 0..=1")]
    NormalizedCoordinateOutOfRange { x: f64, y: f64 },
    #[error("camera scale must be finite and at least 1.0, got {0}")]
    InvalidScale(f64),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_negative_desktop_origin() {
        let monitor = PhysicalRect {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let point = monitor
            .desktop_to_normalized(PhysicalPoint { x: -960, y: 540 })
            .unwrap();
        assert!((point.x - 0.5).abs() < f64::EPSILON);
        assert!((point.y - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn clamps_camera_at_bottom_right() {
        let point = NormalizedPoint::new(1.0, 1.0)
            .unwrap()
            .clamped_for_scale(2.0)
            .unwrap();
        assert_eq!(point, NormalizedPoint { x: 0.75, y: 0.75 });
    }

    #[test]
    fn rejects_empty_geometry() {
        assert!(matches!(
            PhysicalSize {
                width: 0,
                height: 1080
            }
            .aspect_ratio(),
            Err(GeometryError::EmptySize)
        ));
    }
}
