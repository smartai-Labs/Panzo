//! Durable user-configured speech service. Keys are bound to an endpoint and DPAPI encrypted.
use super::vision_settings::{KeyChange, crypt, validate_key};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub base_url: String,
    pub model: String,
    pub response_format: String,
    pub language: String,
    pub prompt: String,
    pub timeout_seconds: u64,
    protected_key: Vec<u8>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".into(),
            model: String::new(),
            response_format: "verbose_json".into(),
            language: String::new(),
            prompt: String::new(),
            timeout_seconds: 180,
            protected_key: Vec::new(),
        }
    }
}
pub(crate) fn endpoint(base: &str) -> Result<reqwest::Url, String> {
    let mut url = super::vision::endpoint(base)?;
    let p = url
        .path()
        .trim_end_matches("/chat/completions")
        .trim_end_matches('/');
    let p = if p.ends_with("/audio/transcriptions") {
        p.to_owned()
    } else {
        format!("{p}/audio/transcriptions")
    };
    url.set_path(&p);
    Ok(url)
}
impl Settings {
    pub fn has_key(&self) -> bool {
        !self.protected_key.is_empty()
    }
    pub fn validate(&self) -> Result<(), String> {
        endpoint(&self.base_url)?;
        if self.model.trim().is_empty()
            || self.model.len() > 256
            || self.model.chars().any(char::is_control)
        {
            return Err("请填写语音识别模型名称".into());
        }
        if !["verbose_json", "srt", "diarized_json", "json"]
            .contains(&self.response_format.as_str())
        {
            return Err("请选择支持的字幕返回格式".into());
        }
        if !(10..=600).contains(&self.timeout_seconds) {
            return Err("超时须为 10–600 秒".into());
        }
        if !self.language.is_empty()
            && (self.language.len() != 2 || !self.language.bytes().all(|c| c.is_ascii_lowercase()))
        {
            return Err("语言留空自动识别，或填写 zh、en 等两位代码".into());
        }
        if self.prompt.len() > 4000 || self.prompt.contains('\0') {
            return Err("术语提示过长或无效".into());
        }
        Ok(())
    }
    pub fn service(&self) -> Result<panzo_plugin_api::media::SpeechService, String> {
        self.validate()?;
        let key = if self.has_key() {
            String::from_utf8(crypt(&self.protected_key, false)?).map_err(|_| "无法读取密钥")?
        } else {
            String::new()
        };
        if !key.is_empty() {
            validate_key(&key)?;
        }
        Ok(panzo_plugin_api::media::SpeechService {
            base_url: endpoint(&self.base_url)?.to_string(),
            model: self.model.clone(),
            api_key: key,
            response_format: self.response_format.clone(),
            language: self.language.clone(),
            prompt: self.prompt.clone(),
            timeout_seconds: self.timeout_seconds,
        })
    }
}
fn path() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|s| PathBuf::from(s).join("Panzo/speech.json"))
        .ok_or_else(|| "找不到设置目录".into())
}
pub(crate) fn load() -> Result<Settings, String> {
    load_at(&path()?)
}
fn load_at(path: &Path) -> Result<Settings, String> {
    if !path.exists() {
        return Ok(Settings::default());
    }
    serde_json::from_slice(&super::plugins::read_bounded(path, 65536)?)
        .map_err(|_| "AI 字幕配置损坏，请检查 speech.json".into())
}
pub(crate) fn save(next: Settings, change: KeyChange) -> Result<(), String> {
    save_at(&path()?, next, change)
}
fn save_at(path: &Path, mut next: Settings, change: KeyChange) -> Result<(), String> {
    let old = load_at(path)?;
    next.base_url = next.base_url.trim().trim_end_matches('/').into();
    next.model = next.model.trim().into();
    next.language = next.language.trim().into();
    next.validate()?;
    next.protected_key = match change {
        KeyChange::Keep => {
            if old.has_key() && endpoint(&old.base_url)? != endpoint(&next.base_url)? {
                return Err("更换服务地址时请重新填写密钥，或选择删除已存密钥".into());
            }
            old.protected_key
        }
        KeyChange::Clear => Vec::new(),
        KeyChange::Replace(k) => {
            let k = k.trim();
            validate_key(k)?;
            crypt(k.as_bytes(), true)?
        }
    };
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|_| "无法创建配置目录")?;
    }
    super::manifest_store::persist_json_atomically(path, &next)
        .map_err(|_| "AI 字幕配置保存失败".into())
}
pub(crate) fn open() -> Result<(), String> {
    let old = load()?;
    let holder = std::rc::Rc::new(std::cell::Cell::new(
        windows::Win32::Foundation::HWND::default(),
    ));
    let target = holder.clone();
    let h = super::workspace_form::open(
        3,
        crate::platform::workspace_form::FormId::Speech,
        "设置 · AI 字幕",
        "语音转写兼容接口。点击生成时发送所选音频；API Key 使用当前 Windows 账户加密保存。json 模式按短音频段定位，需要校对时间。",
        &[
            ("API 地址", old.base_url),
            ("语音模型名称", old.model),
            ("API Key（留空保留）", String::new()),
            ("密钥操作", "保留或更新".into()),
            ("返回格式", old.response_format),
            ("语言（自动 / zh / en）", old.language),
            ("术语提示（可选）", old.prompt),
            ("请求超时 · 秒", old.timeout_seconds.to_string()),
        ],
        &[
            (3, &["保留或更新", "删除已存密钥"]),
            (4, &["verbose_json", "srt", "diarized_json", "json"]),
        ],
        &[2],
        &["保存配置"],
        Box::new(move |_, v| {
            let mut next = load()?;
            next.base_url = v[0].clone();
            next.model = v[1].clone();
            next.response_format = v[4].clone();
            next.language = if v[5].trim() == "自动" {
                String::new()
            } else {
                v[5].trim().into()
            };
            next.prompt = v[6].clone();
            next.timeout_seconds = v[7].parse().map_err(|_| "超时须为整数")?;
            let change = if v[3] == "删除已存密钥" {
                KeyChange::Clear
            } else if v[2].trim().is_empty() {
                KeyChange::Keep
            } else {
                KeyChange::Replace(v[2].clone())
            };
            save(next, change)?;
            if let Some(e) = super::workspace_form::field(target.get(), "api-key") {
                super::workspace_ui::text(e, "");
            }
            Ok("AI 字幕配置已保存，重启后继续使用。可返回视频工作台生成字幕。".into())
        }),
    )?;
    holder.set(h);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_speech_settings() {
        let root = std::env::temp_dir().join(format!("panzo-speech-{}", uuid::Uuid::new_v4()));
        let path = root.join("speech.json");
        let mut s = Settings {
            model: "custom-asr".into(),
            ..Default::default()
        };
        save_at(
            &path,
            s.clone(),
            KeyChange::Replace("synthetic-speech-test-key".into()),
        )
        .unwrap();
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("synthetic-speech-test-key")
        );
        assert_eq!(
            load_at(&path).unwrap().service().unwrap().api_key,
            "synthetic-speech-test-key"
        );
        s.base_url = "https://example.org/v1".into();
        assert!(save_at(&path, s.clone(), KeyChange::Keep).is_err());
        save_at(&path, s, KeyChange::Clear).unwrap();
        assert!(!load_at(&path).unwrap().has_key());
        std::fs::remove_dir_all(root).unwrap();
    }
}
