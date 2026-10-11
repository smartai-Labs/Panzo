//! Protocol 2: bounded media jobs. Paths identify explicitly supplied temporary/source files.
use serde::{Deserialize, Serialize};
pub const PROTOCOL: u32 = 2;
pub const PLUGIN_ID: &str = "panzo.media-tools";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechService {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub response_format: String,
    pub language: String,
    pub prompt: String,
    pub timeout_seconds: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Task {
    DecodeAudio {
        source: String,
        output: String,
    },
    RecordVoice {
        output: String,
        device: Option<String>,
        stop_file: String,
        meter_file: String,
        seconds: u64,
    },
    Transcribe {
        audio: String,
        duration_seconds: f64,
        service: SpeechService,
    },
}
impl Task {
    pub fn command(&self) -> &'static str {
        match self {
            Self::DecodeAudio { .. } => "audio.import",
            Self::RecordVoice { .. } => "audio.record",
            Self::Transcribe { .. } => "speech.transcribe",
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: u32,
    pub request_id: String,
    pub task: Task,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Output {
    Audio {
        frames: u64,
        waveform: Vec<f32>,
    },
    Subtitles {
        segments: Vec<Segment>,
        approximate: bool,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub protocol: u32,
    pub request_id: String,
    pub output: Option<Output>,
    pub error: Option<String>,
}
