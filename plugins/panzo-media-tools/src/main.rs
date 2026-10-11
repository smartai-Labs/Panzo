mod speech;
use panzo_plugin_api::media::{self, Request, Response, Task};
use std::io::{Read, Write};
fn main() {
    let mut input = Vec::new();
    if std::io::stdin()
        .take(65537)
        .read_to_end(&mut input)
        .is_err()
        || input.len() > 65536
    {
        std::process::exit(2);
    }
    let Ok(request) = serde_json::from_slice::<Request>(&input) else {
        std::process::exit(2)
    };
    let result = if request.protocol != media::PROTOCOL {
        Err("媒体协议不兼容".into())
    } else {
        match request.task {
            Task::DecodeAudio { source, output } => panzo_windows::platform::media_worker::decode(
                std::path::Path::new(&source),
                std::path::Path::new(&output),
            ),
            Task::RecordVoice {
                output,
                device,
                stop_file,
                meter_file,
                seconds,
            } => panzo_windows::platform::media_worker::record(
                std::path::Path::new(&output),
                device.as_deref(),
                std::path::Path::new(&stop_file),
                std::path::Path::new(&meter_file),
                seconds,
            ),
            Task::Transcribe {
                audio,
                duration_seconds,
                service,
            } => speech::transcribe(std::path::Path::new(&audio), duration_seconds, &service),
        }
    };
    let response = match result {
        Ok(output) => Response {
            protocol: media::PROTOCOL,
            request_id: request.request_id,
            output: Some(output),
            error: None,
        },
        Err(error) => Response {
            protocol: media::PROTOCOL,
            request_id: request.request_id,
            output: None,
            error: Some(error),
        },
    };
    if let Ok(bytes) = serde_json::to_vec(&response) {
        let _ = std::io::stdout().write_all(&bytes);
    }
}
