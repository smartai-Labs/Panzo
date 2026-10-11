//! User-owned multimodal service settings. Secrets are encrypted for the current Windows user.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    },
    core::w,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub base_url: String,
    pub model: String,
    pub timeout_seconds: u64,
    pub default_online: bool,
    protected_key: Vec<u8>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".into(),
            model: String::new(),
            timeout_seconds: 120,
            default_online: false,
            protected_key: Vec::new(),
        }
    }
}
pub(crate) enum KeyChange {
    Keep,
    Replace(String),
    Clear,
}
impl Settings {
    #[cfg(test)]
    pub(super) fn for_test(base_url: String, model: &str, timeout_seconds: u64) -> Self {
        Self {
            base_url,
            model: model.into(),
            timeout_seconds,
            ..Self::default()
        }
    }

    pub fn has_key(&self) -> bool {
        !self.protected_key.is_empty()
    }
    pub fn validate(&self) -> Result<(), String> {
        super::vision::endpoint(&self.base_url)?;
        if self.model.trim().is_empty()
            || self.model.len() > 256
            || self.model.chars().any(char::is_control)
        {
            return Err("请填写服务商提供的视觉模型名称（最多 256 字节）".into());
        }
        if !(10..=300).contains(&self.timeout_seconds) {
            return Err("请求超时须为 10–300 秒".into());
        }
        Ok(())
    }
    pub fn api_key(&self) -> Result<String, String> {
        if !self.has_key() {
            return Err("尚未保存 API Key，请先打开多模态识图设置".into());
        }
        let bytes = crypt(&self.protected_key, false)?;
        let key = String::from_utf8(bytes).map_err(|_| "无法读取 API Key，请重新填写并保存")?;
        validate_key(&key)?;
        Ok(key)
    }
}
pub(super) fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > 4096 || !key.bytes().all(|b| (33..=126).contains(&b)) {
        return Err("API Key 须为不含空白的 ASCII 字符，长度不超过 4096 字节".into());
    }
    Ok(())
}
pub(super) fn crypt(bytes: &[u8], protect: bool) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(|_| "密钥数据过大")?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let result = unsafe {
        if protect {
            CryptProtectData(
                &raw const input,
                w!("Panzo multimodal API key"),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &raw mut output,
            )
        } else {
            CryptUnprotectData(
                &raw const input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &raw mut output,
            )
        }
    };
    result.map_err(|_| {
        if protect {
            "Windows 无法加密 API Key，配置未保存"
        } else {
            "无法解密已存 API Key，请在当前 Windows 账户下重新填写并保存"
        }
    })?;
    let data =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
    }
    Ok(data)
}
fn path() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|p| PathBuf::from(p).join("Panzo/vision.json"))
        .ok_or_else(|| "找不到应用设置目录".into())
}
pub(crate) fn load() -> Result<Settings, String> {
    load_at(&path()?)
}
pub(super) fn load_at(path: &Path) -> Result<Settings, String> {
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(_) => return Err("无法读取多模态识图配置文件".into()),
        Ok(m) if m.len() > 65_536 => return Err("多模态识图配置文件过大".into()),
        Ok(_) => {}
    }
    let bytes = std::fs::read(path).map_err(|_| "无法读取多模态识图配置文件")?;
    serde_json::from_slice(&bytes)
        .map_err(|_| "多模态识图配置文件损坏，请检查本地 vision.json".into())
}
pub(crate) fn save(next: Settings, change: KeyChange) -> Result<(), String> {
    save_at(&path()?, next, change)
}
pub(super) fn save_at(path: &Path, mut next: Settings, change: KeyChange) -> Result<(), String> {
    let old = load_at(path)?;
    next.base_url = next.base_url.trim().trim_end_matches('/').into();
    next.model = next.model.trim().into();
    next.validate()?;
    next.protected_key = match change {
        KeyChange::Keep => {
            if old.has_key()
                && super::vision::endpoint(&old.base_url)?
                    != super::vision::endpoint(&next.base_url)?
            {
                return Err("更换服务地址时请重新填写 API Key，或选择删除已存密钥".into());
            }
            old.protected_key
        }
        KeyChange::Replace(key) => {
            let key = key.trim();
            validate_key(key)?;
            crypt(key.as_bytes(), true)?
        }
        KeyChange::Clear => Vec::new(),
    };
    if !next.has_key() {
        next.default_online = false;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "无法创建多模态识图设置目录")?;
    }
    super::manifest_store::persist_json_atomically(path, &next)
        .map_err(|_| "无法保存多模态识图配置，请检查设置目录权限".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_settings_reload_replace_clear_and_endpoint_binding() {
        let root = std::env::temp_dir().join(format!("panzo-vision-{}", uuid::Uuid::new_v4()));
        let path = root.join("vision.json");
        let settings = Settings {
            model: "test-vision".into(),
            default_online: true,
            ..Settings::default()
        };
        save_at(
            &path,
            settings,
            KeyChange::Replace("panzo-synthetic-test-key".into()),
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("panzo-synthetic-test-key"));
        let mut restored = load_at(&path).unwrap();
        assert_eq!(restored.api_key().unwrap(), "panzo-synthetic-test-key");
        assert!(restored.default_online);
        restored.model = "another-vision-model".into();
        save_at(&path, restored.clone(), KeyChange::Keep).unwrap();
        assert_eq!(
            load_at(&path).unwrap().api_key().unwrap(),
            "panzo-synthetic-test-key"
        );
        restored.base_url = "https://example.org/v1".into();
        assert!(save_at(&path, restored.clone(), KeyChange::Keep).is_err());
        assert_eq!(
            load_at(&path).unwrap().base_url,
            Settings::default().base_url
        );
        save_at(
            &path,
            restored.clone(),
            KeyChange::Replace("replacement-test-key".into()),
        )
        .unwrap();
        assert_eq!(
            load_at(&path).unwrap().api_key().unwrap(),
            "replacement-test-key"
        );
        save_at(&path, restored, KeyChange::Clear).unwrap();
        let cleared = load_at(&path).unwrap();
        assert!(!cleared.has_key());
        assert!(!cleared.default_online);
        assert!(cleared.api_key().is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn invalid_key_and_corrupt_protected_data_are_rejected_without_echoing_them() {
        assert!(validate_key("key\r\nInjected: secret").is_err());
        assert!(validate_key("").is_err());
        let settings = Settings {
            protected_key: b"corrupted-secret".to_vec(),
            ..Settings::default()
        };
        assert!(!settings.api_key().unwrap_err().contains("corrupted-secret"));
    }
}
