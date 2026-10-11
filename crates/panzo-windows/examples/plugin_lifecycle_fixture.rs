//! Test-only worker for lifecycle isolation reviews. Never shipped in application packages.
use panzo_plugin_api::{MAX_MESSAGE, Request, Response, media};
use serde_json::json;
use std::{
    io::{Read, Write},
    path::Path,
    time::{Duration, Instant},
};

fn hold(directory: &Path, id: u64) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !directory.join(format!("release-{id}")).exists() {
        if Instant::now() > deadline {
            return Err("fixture gate timed out".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let executable = std::env::current_exe()?;
    let directory = executable.parent().ok_or("fixture directory unavailable")?;
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).is_some_and(|s| s == "--descendant") {
        let id = args.get(2).ok_or("fixture id missing")?.parse::<u64>()?;
        std::fs::write(
            directory.join(format!("descendant-{id}.json")),
            serde_json::to_vec(&json!({"pid": std::process::id()}))?,
        )?;
        return hold(directory, id);
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_MESSAGE + 1)
        .read_to_end(&mut bytes)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if value["protocol"] == media::PROTOCOL {
        let request: media::Request = serde_json::from_slice(&bytes)?;
        let media::Task::DecodeAudio { source, .. } = request.task else {
            return Err("unsupported fixture task".into());
        };
        let id = source.parse::<u64>()?;
        std::fs::write(
            directory.join(format!("started-{id}.json")),
            serde_json::to_vec(&json!({"pid": std::process::id(), "protocol": 2}))?,
        )?;
        hold(directory, id)?;
        let response = media::Response {
            protocol: media::PROTOCOL,
            request_id: request.request_id,
            output: Some(media::Output::Audio {
                frames: 1,
                waveform: vec![0.0],
            }),
            error: None,
        };
        std::io::stdout().write_all(&serde_json::to_vec(&response)?)?;
        return Ok(());
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    let id = request.document_id;
    std::fs::write(
        directory.join(format!("started-{id}.json")),
        serde_json::to_vec(&json!({"pid": std::process::id(), "request_id": request.request_id}))?,
    )?;
    match request.command.as_str() {
        "hold" => hold(directory, id)?,
        "tree" => {
            use std::os::windows::process::CommandExt;
            let mut child = std::process::Command::new(&executable)
                .args(["--descendant", &id.to_string()])
                .creation_flags(0x0800_0000)
                .spawn()?;
            hold(directory, id)?;
            child.wait()?;
        }
        "fail" => std::process::exit(31),
        "invalid" => {
            std::io::stdout().write_all(b"{invalid-json")?;
            return Ok(());
        }
        "echo" => {}
        _ => return Err("unknown fixture command".into()),
    }
    let response = Response {
        protocol: request.protocol,
        request_id: request.request_id,
        image: Some(request.image),
        error: None,
    };
    std::io::stdout().write_all(&serde_json::to_vec(&response)?)?;
    Ok(())
}
