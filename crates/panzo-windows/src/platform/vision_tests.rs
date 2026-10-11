use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, mpsc},
    thread,
    time::Instant,
};

fn sample_image() -> Raster {
    Raster {
        width: 2,
        height: 1,
        pixels: vec![0, 0, 255, 255, 255, 0, 0, 0],
    }
}
fn mock_server(
    status: &str,
    headers: &str,
    body: Vec<u8>,
    delay: Duration,
) -> (String, mpsc::Receiver<Vec<u8>>, thread::JoinHandle<()>) {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", server.local_addr().unwrap());
    let status = status.to_owned();
    let headers = headers.to_owned();
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        server.set_nonblocking(true).unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match server.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < until => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("mock server accept: {e}"),
            }
        };
        // Accepted sockets inherit nonblocking mode on Windows.
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let n = stream.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..n]);
            if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&request[..end]);
                let length = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let _ = tx.send(request);
        thread::sleep(delay);
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
            body.len()
        );
        let _ = stream.write_all(reply.as_bytes());
        let _ = stream.write_all(&body);
    });
    (url, rx, worker)
}
#[test]
fn multimodal_request_sends_exact_image_prompt_and_auth_to_configured_endpoint() {
    let body = serde_json::to_vec(&serde_json::json!({"choices":[{"message":{"content":"中文 OCR\nPanzo"},"finish_reason":"stop"}]})).unwrap();
    let (base_url, received, server) = mock_server("200 OK", "", body, Duration::ZERO);
    let settings = Settings::for_test(base_url, "user-selected-vision", 120);
    let root = std::env::temp_dir().join(format!("panzo-vision-request-{}", uuid::Uuid::new_v4()));
    let path = root.join("vision.json");
    super::super::vision_settings::save_at(
        &path,
        settings,
        super::super::vision_settings::KeyChange::Replace("synthetic-api-key".into()),
    )
    .unwrap();
    let loaded = super::super::vision_settings::load_at(&path).unwrap();
    let result = recognize(
        &loaded,
        &sample_image(),
        OCR_PROMPT,
        &AtomicBool::new(false),
    )
    .unwrap();
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(root).unwrap();
    assert_eq!(result, "中文 OCR\r\nPanzo");
    let bytes = received.recv_timeout(Duration::from_secs(5)).unwrap();
    server.join().unwrap();
    let end = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap();
    let headers = String::from_utf8_lossy(&bytes[..end]);
    assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("authorization: bearer synthetic-api-key")
    );
    let body: serde_json::Value = serde_json::from_slice(&bytes[end + 4..]).unwrap();
    assert_eq!(body["model"], "user-selected-vision");
    assert_eq!(body["stream"], false);
    assert_eq!(body["messages"][0]["content"][0]["text"], OCR_PROMPT);
    let data = body["messages"][0]["content"][1]["image_url"]["url"]
        .as_str()
        .unwrap();
    let png = STANDARD
        .decode(data.strip_prefix("data:image/png;base64,").unwrap())
        .unwrap();
    let image = image::load_from_memory(&png).unwrap().to_rgb8();
    assert_eq!(image.dimensions(), (2, 1));
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0]);
    assert_eq!(image.get_pixel(1, 0).0, [255, 255, 255]);
}
#[test]
fn endpoints_accept_base_or_full_url_and_reject_credentials_or_remote_cleartext() {
    for base in [
        "https://example.com/v1",
        "https://example.com/v1/",
        "https://example.com/v1/chat/completions",
        "https://example.com/v1/chat/completions/",
    ] {
        assert_eq!(
            endpoint(base).unwrap().as_str(),
            "https://example.com/v1/chat/completions"
        );
    }
    assert_eq!(
        endpoint("http://localhost:11434/v1").unwrap().as_str(),
        "http://localhost:11434/v1/chat/completions"
    );
    assert!(endpoint("http://[::1]:8000/v1").is_ok());
    for base in [
        "http://example.com/v1",
        "file:///tmp/image.png",
        "https://key@example.com/v1",
        "https://example.com/v1?api_key=secret",
        "https://example.com/v1#fragment",
        "https://example.com/\r\n",
    ] {
        assert!(endpoint(base).is_err(), "{base}");
    }
}
#[test]
fn errors_do_not_echo_provider_secrets_and_redirects_are_not_followed() {
    for (status, headers, code) in [
        ("401 Unauthorized", "", "401"),
        ("429 Too Many Requests", "", "429"),
        (
            "307 Temporary Redirect",
            "Location: http://127.0.0.1:1/secret\r\n",
            "307",
        ),
    ] {
        let (base_url, _, server) = mock_server(
            status,
            headers,
            b"provider echoed synthetic-api-key".to_vec(),
            Duration::ZERO,
        );
        let settings = Settings::for_test(base_url, "vision", 120);
        let error = request(
            &settings,
            "synthetic-api-key",
            &sample_image(),
            OCR_PROMPT,
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(error.contains(code));
        assert!(!error.contains("synthetic-api-key"));
        server.join().unwrap();
    }
    assert!(
        !response_text(br#"{"error":{"message":"secret-key"}}"#)
            .unwrap_err()
            .contains("secret-key")
    );
}
#[test]
fn cancellation_interrupts_response_wait_and_pre_cancel_sends_nothing() {
    let (base_url, received, server) =
        mock_server("200 OK", "", b"{}".to_vec(), Duration::from_secs(2));
    let settings = Settings::for_test(base_url, "vision", 120);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let worker = thread::spawn(move || {
        request(
            &settings,
            "synthetic-key",
            &sample_image(),
            OCR_PROMPT,
            &worker_cancel,
        )
    });
    received.recv_timeout(Duration::from_secs(5)).unwrap();
    let start = Instant::now();
    cancel.store(true, Ordering::Release);
    assert!(worker.join().unwrap().unwrap_err().contains("取消"));
    assert!(start.elapsed() < Duration::from_secs(1));
    server.join().unwrap();
    assert!(
        request(
            &Settings::default(),
            "unused",
            &sample_image(),
            OCR_PROMPT,
            &AtomicBool::new(true)
        )
        .unwrap_err()
        .contains("取消")
    );
}
#[test]
fn response_limits_timeout_empty_and_truncation_are_reported() {
    let (base_url, _, server) =
        mock_server("200 OK", "", vec![b'x'; MAX_RESPONSE + 1], Duration::ZERO);
    let settings = Settings::for_test(base_url, "vision", 120);
    assert!(
        request(
            &settings,
            "test",
            &sample_image(),
            OCR_PROMPT,
            &AtomicBool::new(false)
        )
        .unwrap_err()
        .contains("过大")
    );
    server.join().unwrap();
    let (base_url, _, server) = mock_server("200 OK", "", b"{}".to_vec(), Duration::from_secs(2));
    let settings = Settings::for_test(base_url, "vision", 1);
    assert!(
        request(
            &settings,
            "test",
            &sample_image(),
            OCR_PROMPT,
            &AtomicBool::new(false)
        )
        .unwrap_err()
        .contains("超时")
    );
    server.join().unwrap();
    assert!(response_text(b"not json").is_err());
    assert!(response_text(br#"{"choices":[{"message":{"content":""}}]}"#).is_err());
    let output = response_text(br#"{"choices":[{"message":{"content":[{"type":"text","text":"part one"},{"type":"text","text":"part two"}]},"finish_reason":"length"}]}"#).unwrap();
    assert!(output.contains("可能不完整"));
    assert!(output.ends_with("part one\r\npart two"));
}
