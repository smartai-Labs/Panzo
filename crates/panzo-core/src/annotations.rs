//! Non-destructive video marks use source time, so trims and speed changes retain alignment.
use crate::TimeTick;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnnotationKind {
    Text,
    Arrow,
    Rectangle,
    Ellipse,
    Highlight,
    Cover,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnnotationAnchor {
    Canvas,
    Source,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoAnnotation {
    pub id: String,
    pub start: TimeTick,
    pub end: TimeTick,
    pub kind: AnnotationKind,
    pub anchor: AnnotationAnchor,
    /// Normalized x1,y1,x2,y2. Arrows may point in either direction.
    pub bounds: [f64; 4],
    pub text: String,
    pub color: String,
    /// Relative to canvas height (or visible source height for source-attached marks).
    pub stroke: f64,
    pub font_size: f64,
}
impl VideoAnnotation {
    pub fn active(&self, source: TimeTick) -> bool {
        source >= self.start && source < self.end
    }
    pub fn valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 64
            && self.start >= TimeTick::ZERO
            && self.end > self.start
            && self.end.0 <= 24 * 60 * 60 * 10_000_000
            && self
                .bounds
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && (if self.kind == AnnotationKind::Arrow {
                (self.bounds[2] - self.bounds[0]).abs() > 0.0001
                    || (self.bounds[3] - self.bounds[1]).abs() > 0.0001
            } else {
                (self.bounds[2] - self.bounds[0]).abs() > 0.0001
                    && (self.bounds[3] - self.bounds[1]).abs() > 0.0001
            })
            && self.text.len() <= 16_384
            && !self.text.contains('\0')
            && crate::editor::parse_hex_color(&self.color).is_ok()
            && self.stroke.is_finite()
            && (0.0005..=0.03).contains(&self.stroke)
            && self.font_size.is_finite()
            && (0.005..=0.2).contains(&self.font_size)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn item() -> VideoAnnotation {
        VideoAnnotation {
            id: "a".into(),
            start: TimeTick::from_millis(1000),
            end: TimeTick::from_millis(2000),
            kind: AnnotationKind::Cover,
            anchor: AnnotationAnchor::Canvas,
            bounds: [0.1, 0.1, 0.4, 0.4],
            text: String::new(),
            color: "#ff0000".into(),
            stroke: 0.003,
            font_size: 0.03,
        }
    }
    #[test]
    fn source_time_and_half_open_ranges() {
        let a = item();
        assert!(a.valid());
        assert!(!a.active(TimeTick::from_millis(999)));
        assert!(a.active(TimeTick::from_millis(1000)));
        assert!(!a.active(TimeTick::from_millis(2000)));
        let mut video = crate::VideoEdit::full(TimeTick::from_millis(4000)).unwrap();
        let id = video.clips[0].id.clone();
        video.set_speed_percent(&id, 200).unwrap();
        assert!(
            a.active(crate::TimeMapping::source_time(&video, TimeTick::from_millis(750)).unwrap())
        );
    }
    #[test]
    fn invalid_data_and_legacy_settings() {
        let mut a = item();
        a.bounds[0] = f64::NAN;
        assert!(!a.valid());
        let mut settings = crate::WorkbenchSettings::default();
        settings.annotations.push(item());
        assert!(settings.validate().is_ok());
        settings.annotations.push(item());
        assert!(settings.validate().is_err());
        let mut value = serde_json::to_value(crate::WorkbenchSettings::default()).unwrap();
        value["schemaVersion"] = 6.into();
        value.as_object_mut().unwrap().remove("annotations");
        let old: crate::WorkbenchSettings = serde_json::from_value(value).unwrap();
        assert!(old.annotations.is_empty());
        assert!(old.validate().is_ok());
    }
}
