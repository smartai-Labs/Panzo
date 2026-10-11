use panzo_plugin_api::media::{Output, Segment, SpeechService};
use reqwest::{
    Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use std::{io::Read, path::Path, time::Duration};
fn endpoint(base: &str) -> Result<Url, String> {
    let mut url = Url::parse(base).map_err(|_| "语音服务地址无效")?;
    let local = url.host_str().is_some_and(|h| {
        h == "localhost"
            || h.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|i| i.is_loopback())
    });
    if (url.scheme() != "https" && !(url.scheme() == "http" && local))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("远程接口须使用 HTTPS，地址不能包含密钥".into());
    }
    let p = url.path().trim_end_matches('/');
    if !p.ends_with("/audio/transcriptions") {
        let p = format!("{p}/audio/transcriptions");
        url.set_path(&p);
    }
    Ok(url)
}
fn parse(bytes: &[u8], duration: f64, format: &str) -> Result<Output, String> {
    if format == "srt" {
        let text = std::str::from_utf8(bytes).map_err(|_| "服务未返回 UTF-8 字幕")?;
        let cues = panzo_core::media_edit::parse_srt(text)?;
        let segments = cues
            .into_iter()
            .map(|c| Segment {
                start: c.start.0 as f64 / 1e7,
                end: c.end.0 as f64 / 1e7,
                text: c.text,
            })
            .collect();
        return validate(segments, duration, false);
    }
    let v: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| "服务没有返回 JSON，请检查 API 地址及返回格式")?;
    if v.get("error").is_some() {
        return Err("服务返回转写错误，请检查模型权限和配置".into());
    }
    if let Some(items) = v["segments"].as_array() {
        let mut segments = Vec::new();
        for s in items {
            segments.push(Segment {
                start: s["start"].as_f64().ok_or("字幕缺少开始时间")?,
                end: s["end"].as_f64().ok_or("字幕缺少结束时间")?,
                text: s["text"].as_str().ok_or("字幕缺少文字")?.trim().into(),
            });
        }
        return validate(segments, duration, false);
    }
    if format == "json" {
        let text = v["text"].as_str().ok_or("模型未返回文字")?.trim();
        return validate(
            if text.is_empty() {
                Vec::new()
            } else {
                vec![Segment {
                    start: 0.0,
                    end: duration,
                    text: text.into(),
                }]
            },
            duration,
            true,
        );
    }
    Err("模型没有返回字幕时间戳；选择支持时间戳的模型，或将返回格式设为 json 按短音频段定位".into())
}
fn validate(
    mut segments: Vec<Segment>,
    duration: f64,
    approximate: bool,
) -> Result<Output, String> {
    if segments.len() > 1000 {
        return Err("单段返回字幕过多".into());
    }
    for s in &mut segments {
        if !s.start.is_finite()
            || !s.end.is_finite()
            || s.start < 0.0
            || s.end <= s.start
            || s.start >= duration
            || s.end > duration + 0.25
            || s.text.is_empty()
            || s.text.len() > 4096
            || s.text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
        {
            return Err("模型返回了无效字幕时间或内容，未应用结果".into());
        }
        s.end = s.end.min(duration);
    }
    segments.sort_by(|a, b| a.start.total_cmp(&b.start));
    Ok(Output::Subtitles {
        segments,
        approximate,
    })
}
pub fn transcribe(audio: &Path, duration: f64, service: &SpeechService) -> Result<Output, String> {
    if !duration.is_finite() || !(0.0..=120.0).contains(&duration) || duration == 0.0 {
        return Err("音频段时长无效".into());
    }
    let url = endpoint(&service.base_url)?;
    if service.model.trim().is_empty()
        || service.model.len() > 256
        || service.model.chars().any(char::is_control)
        || service.prompt.len() > 4000
        || service.language.len() > 10
        || !(10..=600).contains(&service.timeout_seconds)
        || !["verbose_json", "srt", "json", "diarized_json"]
            .contains(&service.response_format.as_str())
    {
        return Err("语音模型配置无效".into());
    }
    let mut wave = Vec::new();
    std::fs::File::open(audio)
        .map_err(|_| "无法读取待转写音频")?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut wave)
        .map_err(|_| "读取音频失败")?;
    if wave.len() > 8 * 1024 * 1024 || !wave.starts_with(b"RIFF") {
        return Err("待转写音频必须为不超过 8 MB 的 WAV".into());
    }
    let boundary = format!("panzo{}", panzo_plugin_api::hash(&wave)[..24].to_owned());
    let mut body = Vec::new();
    for (name, value) in [
        ("model", service.model.as_str()),
        ("response_format", service.response_format.as_str()),
        ("language", service.language.as_str()),
        (
            "prompt",
            if service.response_format == "diarized_json" {
                ""
            } else {
                service.prompt.as_str()
            },
        ),
        (
            "chunking_strategy",
            if service.response_format == "diarized_json" {
                "auto"
            } else {
                ""
            },
        ),
    ] {
        if value.is_empty() {
            continue;
        }
        if value.contains(&boundary) {
            return Err("请求参数无效".into());
        }
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n").as_bytes());
    body.extend_from_slice(&wave);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "无法创建转写任务")?;
    runtime.block_on(async {
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(service.timeout_seconds));
        if url.scheme() == "http" {
            builder = builder.no_proxy();
        }
        let client = builder.build().map_err(|_| "无法初始化语音连接")?;
        let mut request = client
            .post(url)
            .header(
                CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body);
        if !service.api_key.is_empty() {
            let mut key = HeaderValue::from_str(&format!("Bearer {}", service.api_key))
                .map_err(|_| "API Key 格式无效")?;
            key.set_sensitive(true);
            request = request.header(AUTHORIZATION, key);
        }
        let mut response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                "语音服务超时"
            } else {
                "无法连接语音服务，请检查网络和地址"
            }
        })?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            return Err(format!(
                "{}（HTTP {status}）",
                match status {
                    401 | 403 => "鉴权失败，请检查 API Key 和模型权限",
                    404 => "语音接口或模型不存在",
                    429 => "额度不足或请求过于频繁",
                    300..=399 => "接口重定向已阻止，请填写最终地址",
                    400 | 422 => "服务拒绝请求，请检查模型及返回格式是否匹配",
                    _ => "语音转写请求失败",
                }
            ));
        }
        let mut bytes = Vec::new();
        while let Some(c) = response.chunk().await.map_err(|_| "语音响应读取失败")? {
            if bytes.len() + c.len() > 4 * 1024 * 1024 {
                return Err("语音响应过大".into());
            }
            bytes.extend_from_slice(&c);
        }
        parse(&bytes, duration, &service.response_format)
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_provider_timestamps() {
        assert!(matches!(
            parse(
                br#"{"segments":[{"start":0.2,"end":1.2,"text":"hello"}]}"#,
                2.0,
                "verbose_json"
            )
            .unwrap(),
            Output::Subtitles {
                approximate: false,
                ..
            }
        ));
        assert!(parse(br#"{"text":"hello"}"#, 2.0, "verbose_json").is_err());
        assert!(
            parse(
                br#"{"segments":[{"start":2,"end":4,"text":"x"}]}"#,
                2.0,
                "verbose_json"
            )
            .is_err()
        );
        assert!(matches!(
            parse(br#"{"text":"hello"}"#, 2.0, "json").unwrap(),
            Output::Subtitles {
                approximate: true,
                ..
            }
        ));
    }
    #[test]
    fn endpoint_rejects_credentials_and_insecure_remote() {
        assert!(endpoint("http://example.com/v1").is_err());
        assert!(endpoint("https://user:secret@example.com/v1").is_err());
        assert!(
            endpoint("http://127.0.0.1:1234/v1")
                .unwrap()
                .path()
                .ends_with("/audio/transcriptions")
        );
    }
}
