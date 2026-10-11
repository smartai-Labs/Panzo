//! Versioned, process-independent image plugin protocol. No host pointers cross this boundary.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
pub const PROTOCOL: u32 = 1;
pub const MAX_PIXELS: u64 = 4_194_304;
pub const MAX_MESSAGE: u64 = 24 * 1024 * 1024;
pub const MAX_PACKAGE: u64 = 48 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub description: String,
    pub protocol: u32,
    pub target: String,
    pub commands: Vec<Command>,
    pub executable_sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub format: u32,
    pub manifest: Manifest,
    pub executable_base64: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub bgra_base64: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: u32,
    pub request_id: String,
    pub document_id: u64,
    pub revision: u64,
    pub command: String,
    pub image: Image,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub protocol: u32,
    pub request_id: String,
    pub image: Option<Image>,
    pub error: Option<String>,
}
pub fn target() -> String {
    format!("{}-windows", std::env::consts::ARCH)
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn valid_id(s: &str) -> bool {
    (3..=80).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
        && !s.starts_with(['.', '-'])
        && !s.ends_with(['.', '-'])
        && !s.contains("..")
}
fn text(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}
impl Manifest {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_id(&self.id) || self.id.starts_with("panzo.builtin.") {
            return Err("插件 ID 无效或使用了保留名称".into());
        }
        let parts = self.version.split('.').collect::<Vec<_>>();
        if parts.len() != 3
            || parts
                .iter()
                .any(|p| p.is_empty() || p.len() > 5 || p.parse::<u16>().is_err())
        {
            return Err("插件版本须为三个数字，例如 1.0.0".into());
        }
        if !matches!(self.protocol, PROTOCOL | media::PROTOCOL) || self.target != target() {
            return Err("插件协议版本或系统架构与当前应用不兼容".into());
        }
        if !text(&self.name, 120) || !text(&self.publisher, 120) || !text(&self.description, 1000) {
            return Err("插件名称、作者或说明无效".into());
        }
        if self.commands.is_empty() || self.commands.len() > 16 {
            return Err("插件须提供 1–16 个操作".into());
        }
        let mut ids = std::collections::HashSet::new();
        for c in &self.commands {
            if !valid_id(&c.id) || !text(&c.name, 120) || !ids.insert(&c.id) {
                return Err("插件操作标识重复或无效".into());
            }
        }
        if self.executable_sha256.len() != 64
            || !self
                .executable_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err("插件缺少有效的 SHA-256 校验值".into());
        }
        Ok(())
    }
}
impl Package {
    pub fn validate(&self) -> Result<Vec<u8>, String> {
        self.manifest.validate()?;
        if self.format != 1 || self.executable_base64.len() as u64 > MAX_PACKAGE {
            return Err("插件包格式不支持或体积超限".into());
        }
        let bytes = STANDARD
            .decode(&self.executable_base64)
            .map_err(|_| "插件文件编码无效")?;
        if bytes.len() < 64 || bytes.len() > 32 * 1024 * 1024 || !bytes.starts_with(b"MZ") {
            return Err("插件可执行文件无效或超过 32 MiB".into());
        }
        let offset =
            u32::from_le_bytes(bytes[60..64].try_into().map_err(|_| "PE 头无效")?) as usize;
        let header = bytes
            .get(offset..offset.saturating_add(6))
            .ok_or("PE 头无效")?;
        let machine = u16::from_le_bytes([header[4], header[5]]);
        let expected = match std::env::consts::ARCH {
            "x86_64" => 0x8664,
            "aarch64" => 0xaa64,
            _ => return Err("暂不支持此架构".into()),
        };
        if &header[..4] != b"PE\0\0" || machine != expected {
            return Err("插件可执行文件架构不兼容".into());
        }
        if hash(&bytes) != self.manifest.executable_sha256.to_ascii_lowercase() {
            return Err("插件完整性校验失败".into());
        }
        Ok(bytes)
    }
}
impl Image {
    pub fn new(width: u32, height: u32, bgra: &[u8]) -> Result<Self, String> {
        let length = checked_length(width, height)?;
        if bgra.len() != length {
            return Err("图片数据长度不匹配".into());
        }
        Ok(Self {
            width,
            height,
            bgra_base64: STANDARD.encode(bgra),
        })
    }
    pub fn decode(&self) -> Result<Vec<u8>, String> {
        let length = checked_length(self.width, self.height)?;
        if self.bgra_base64.len() > length.div_ceil(3) * 4 {
            return Err("图片数据超限".into());
        }
        let bytes = STANDARD
            .decode(&self.bgra_base64)
            .map_err(|_| "图片编码无效")?;
        if bytes.len() != length {
            return Err("图片数据长度不匹配".into());
        }
        Ok(bytes)
    }
}
fn checked_length(w: u32, h: u32) -> Result<usize, String> {
    let pixels = u64::from(w) * u64::from(h);
    if w == 0 || h == 0 || pixels > MAX_PIXELS {
        return Err("插件图片最大 419 万像素，请先裁剪或缩小图片".into());
    }
    usize::try_from(pixels * 4).map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_paths_and_reserved_ids() {
        for id in ["../evil", "a/b", "a\\b", "C:evil", ".hidden", "foo..bar"] {
            assert!(!valid_id(id));
        }
        assert!(valid_id("org.example.filter"));
    }
    #[test]
    fn validates_image_boundaries() {
        assert!(Image::new(u32::MAX, u32::MAX, &[]).is_err());
        assert!(Image::new(1, 1, &[1, 2, 3]).is_err());
        let image = Image::new(1, 1, &[1, 2, 3, 255]).unwrap();
        assert_eq!(image.decode().unwrap(), [1, 2, 3, 255]);
        let mut image = image;
        image.width = 2;
        assert!(image.decode().is_err());
    }
}

pub mod media;
