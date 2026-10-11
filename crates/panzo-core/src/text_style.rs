//! Font identity and appearance shared by image labels and video subtitles.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextAlignment {
    #[default]
    Left,
    Center,
    Right,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FontStyle {
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    pub underline: bool,
    pub alignment: TextAlignment,
}
impl Default for FontStyle {
    fn default() -> Self {
        Self {
            family: "Microsoft YaHei UI".into(),
            weight: 500,
            italic: false,
            underline: false,
            alignment: TextAlignment::Left,
        }
    }
}
impl FontStyle {
    pub fn valid(&self) -> bool {
        !self.family.trim().is_empty()
            && self.family.chars().count() <= 128
            && !self.family.chars().any(char::is_control)
            && (100..=900).contains(&self.weight)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_defaults_and_font_roundtrip() {
        assert_eq!(
            serde_json::from_str::<FontStyle>("{}").unwrap(),
            FontStyle::default()
        );
        let font = FontStyle {
            family: "Arial".into(),
            weight: 700,
            italic: true,
            underline: true,
            alignment: TextAlignment::Right,
        };
        assert!(font.valid());
        assert_eq!(
            font,
            serde_json::from_str(&serde_json::to_string(&font).unwrap()).unwrap()
        );
    }
}
