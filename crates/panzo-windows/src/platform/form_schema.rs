//! Stable form identities and explicit field contracts, independent of translated labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FormId {
    General,
    Vision,
    Speech,
    Ocr,
    Recording,
    Devices,
    AudioMix,
    Camera,
    CameraRange,
    AudioClip,
    AnnotationList,
    Annotation,
    Export,
    Batch,
    PinGroups,
    PluginInstall,
    ImagePlugin,
    #[cfg(test)]
    Review,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum FieldKind {
    Text,
    Secret,
    Directory,
    Color,
    Choice,
    Number {
        min: f64,
        max: f64,
        step: f64,
        unit: &'static str,
        presets: &'static [&'static str],
        optional: bool,
    },
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct FieldSpec {
    pub id: &'static str,
    pub kind: FieldKind,
}
const fn field(id: &'static str, kind: FieldKind) -> FieldSpec {
    FieldSpec { id, kind }
}
const fn number(
    id: &'static str,
    min: f64,
    max: f64,
    step: f64,
    unit: &'static str,
    presets: &'static [&'static str],
) -> FieldSpec {
    field(
        id,
        FieldKind::Number {
            min,
            max,
            step,
            unit,
            presets,
            optional: false,
        },
    )
}
const SECONDS: &[&str] = &["0", "0.5", "1", "2", "5", "10", "30", "60"];
const fn seconds(id: &'static str) -> FieldSpec {
    number(id, 0., 86400., 0.001, "s", SECONDS)
}
impl FormId {
    pub fn fields(self) -> Vec<FieldSpec> {
        use FieldKind::*;
        let camera = || {
            vec![
                field("enabled", Choice),
                field("follow-mode", Choice),
                number("zoom", 1., 3., 0.1, "×", &["1", "1.5", "2", "2.5", "3"]),
                number(
                    "response",
                    80.,
                    1000.,
                    1.,
                    "ms",
                    &["80", "120", "200", "400", "600", "1000"],
                ),
            ]
        };
        match self {
            Self::General => vec![
                field("close-action", Choice),
                field("theme", Choice),
                field("library", Directory),
                field("animations", Choice),
                field("output-directory", Directory),
            ],
            Self::Vision => vec![
                field("endpoint", Text),
                field("model", Text),
                field("api-key", Secret),
                field("key-action", Choice),
                field("recognizer", Choice),
                number("timeout", 10., 300., 1., "s", &["30", "60", "120", "300"]),
            ],
            Self::Speech => vec![
                field("endpoint", Text),
                field("model", Text),
                field("api-key", Secret),
                field("key-action", Choice),
                field("response-format", Choice),
                field("language", Text),
                field("prompt", Text),
                number("timeout", 10., 3600., 1., "s", &["60", "120", "300", "600"]),
            ],
            Self::Ocr => vec![field("language", Choice)],
            Self::Recording => {
                let mut f = camera();
                f.extend([
                    field("fps", Choice),
                    field("countdown", Choice),
                    field("cursor", Choice),
                ]);
                f
            }
            Self::Devices => vec![
                field("system-enabled", Choice),
                field("system-device", Choice),
                field("mic-enabled", Choice),
                field("mic-device", Choice),
            ],
            Self::AudioMix => vec![
                field("enabled", Choice),
                number(
                    "system-gain",
                    0.,
                    200.,
                    1.,
                    "%",
                    &["0", "50", "100", "150", "200"],
                ),
                number(
                    "mic-gain",
                    0.,
                    200.,
                    1.,
                    "%",
                    &["0", "50", "100", "150", "200"],
                ),
                number(
                    "import-gain",
                    0.,
                    200.,
                    1.,
                    "%",
                    &["0", "50", "100", "150", "200"],
                ),
            ],
            Self::Camera => {
                let mut f = camera();
                f.push(field("manual-camera", Choice));
                f
            }
            Self::CameraRange => {
                let mut f = camera();
                f.extend([seconds("start"), seconds("end")]);
                f
            }
            Self::AudioClip => vec![
                seconds("position"),
                seconds("source-in"),
                seconds("source-out"),
                number("gain", 0., 200., 1., "%", &["0", "50", "100", "150", "200"]),
                seconds("fade-in"),
                seconds("fade-out"),
                field("role", Choice),
                field("muted", Choice),
            ],
            Self::AnnotationList => vec![field("annotation", Choice)],
            Self::Annotation => vec![
                field("kind", Choice),
                field("text", Text),
                field("time-range", Text),
                field("bounds", Text),
                field("color", Color),
                field("stroke-font", Text),
                field("anchor", Choice),
            ],
            Self::Export => vec![
                seconds("start"),
                field(
                    "end",
                    Number {
                        min: 0.,
                        max: 86400.,
                        step: 0.001,
                        unit: "s",
                        presets: SECONDS,
                        optional: true,
                    },
                ),
                field("audio", Choice),
                field("quality", Choice),
            ],
            Self::Batch => vec![
                field("format", Choice),
                number(
                    "quality",
                    10.,
                    100.,
                    1.,
                    "%",
                    &["50", "70", "80", "85", "90", "95", "100"],
                ),
                number(
                    "long-edge",
                    0.,
                    16384.,
                    1.,
                    "px",
                    &["0", "720", "1080", "1920", "2560", "3840", "4096"],
                ),
                field("output", Directory),
            ],
            Self::PinGroups => vec![field("group", Choice), field("action", Choice)],
            Self::PluginInstall => vec![field("plugin-id", Text), field("sha256", Text)],
            Self::ImagePlugin => vec![field("command", Choice)],
            #[cfg(test)]
            Self::Review => vec![field("value", Text)],
        }
    }
    pub fn project_scoped(self) -> bool {
        matches!(
            self,
            Self::AudioMix
                | Self::Camera
                | Self::CameraRange
                | Self::AudioClip
                | Self::AnnotationList
                | Self::Annotation
                | Self::Export
        )
    }
    pub fn navigation(self) -> Option<usize> {
        match self {
            Self::General => Some(0),
            Self::Vision => Some(1),
            Self::Ocr => Some(2),
            Self::Speech => Some(4),
            _ => None,
        }
    }
}
impl FieldSpec {
    pub fn validate(self, value: &str) -> Result<(), String> {
        if matches!(self.kind, FieldKind::Color) {
            let hex = value.trim().strip_prefix('#').unwrap_or("");
            if hex.len() != 6 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err("请选择有效颜色".into());
            }
        }
        if let FieldKind::Number {
            min,
            max,
            step,
            unit,
            optional,
            ..
        } = self.kind
        {
            if optional && value.trim().is_empty() {
                return Ok(());
            }
            let valid = value
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && *n >= min && *n <= max);
            if valid.is_none() {
                return Err(format!("请输入 {min}–{max} {unit} 范围内的数值"));
            }
            // Fractional fields allow arbitrary precision. Whole-number contracts reject decimals.
            if step >= 1. && valid.unwrap().fract().abs() > 1e-9 {
                return Err("请输入整数".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_contracts_reject_non_finite_fractional_and_out_of_range_values() {
        let quality = FormId::Batch.fields()[1];
        for value in ["NaN", "inf", "9", "101", "80.5"] {
            assert!(quality.validate(value).is_err(), "{value}");
        }
        assert!(quality.validate("85").is_ok());
        let time = FormId::AudioClip.fields()[0];
        assert!(time.validate("0.125").is_ok());
        assert!(FormId::Export.fields()[1].validate("").is_ok());
        assert!(FormId::Export.fields()[0].validate("").is_err());
    }
    #[test]
    fn semantic_fields_and_scopes_are_independent_of_visible_titles() {
        assert!(matches!(
            FormId::General.fields()[2].kind,
            FieldKind::Directory
        ));
        assert!(matches!(FormId::Vision.fields()[2].kind, FieldKind::Secret));
        assert!(matches!(
            FormId::Annotation.fields()[4].kind,
            FieldKind::Color
        ));
        assert!(FormId::AudioClip.project_scoped());
        assert!(!FormId::Devices.project_scoped());
        assert_eq!(FormId::Vision.navigation(), Some(1));
    }
}
