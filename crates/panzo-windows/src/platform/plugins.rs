//! Plugin registry, durable installation and task leases. Native extensions are trusted code.
use panzo_plugin_api::{MAX_PACKAGE, Manifest, Package};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};
pub(crate) const OCR: &str = "panzo.builtin.ocr";
pub(crate) const VISION: &str = "panzo.builtin.vision";
pub(crate) const IMAGES: &str = "panzo.builtin.image-tools";
pub(crate) const BUILTINS: [(&str, &str); 3] = [
    (OCR, "离线 OCR"),
    (VISION, "多模态识图"),
    (IMAGES, "拼图与批处理"),
];
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Installed {
    pub manifest: Manifest,
    pub directory: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    disabled: BTreeSet<String>,
    installed: BTreeMap<String, Installed>,
    #[serde(default)]
    previous: BTreeMap<String, Installed>,
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub running: usize,
    pub external: bool,
    pub rollback: bool,
    pub busy: bool,
}
pub(crate) struct Store {
    root: PathBuf,
    registry: Registry,
    running: BTreeMap<String, Arc<AtomicUsize>>,
}
impl Store {
    pub(crate) fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let mut s = Self {
            root,
            registry: Registry::default(),
            running: BTreeMap::new(),
        };
        s.reload()?;
        Ok(s)
    }
    fn reload(&mut self) -> Result<(), String> {
        let path = self.root.join("registry.json");
        let registry: Registry = if path.exists() {
            let bytes = read_bounded(&path, 2 * 1024 * 1024)?;
            serde_json::from_slice(&bytes).map_err(|e| format!("插件登记文件无法读取：{e}"))?
        } else {
            Registry::default()
        };
        for (id, item) in registry.installed.iter().chain(&registry.previous) {
            item.manifest.validate()?;
            if id != &item.manifest.id || !safe_directory(&item.directory) {
                return Err("插件登记路径无效".into());
            }
        }
        self.registry = registry;
        Ok(())
    }
    /// Re-read durable installation state when presenting the manager. A different
    /// running host may have installed a package since this store was first opened.
    pub(crate) fn refreshed_entries(&mut self) -> Result<Vec<Entry>, String> {
        self.reload()?;
        Ok(self.entries())
    }
    fn lock(&mut self) -> Result<File, String> {
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join("registry.lock"))
            .map_err(|e| e.to_string())?;
        f.try_lock()
            .map_err(|_| "另一实例正在更新插件，请稍后重试")?;
        self.reload()?;
        Ok(f)
    }
    fn persist(&mut self) -> Result<(), String> {
        let result = super::manifest_store::persist_json_atomically(
            self.root.join("registry.json"),
            &self.registry,
        )
        .map_err(|e| e.to_string());
        if result.is_err() {
            self.reload()?;
        }
        result
    }
    fn enabled(&self, id: &str) -> bool {
        (BUILTINS.iter().any(|(v, _)| *v == id) || self.registry.installed.contains_key(id))
            && !self.registry.disabled.contains(id)
    }
    pub(crate) fn entries(&self) -> Vec<Entry> {
        BUILTINS
            .iter()
            .map(|(id, name)| Entry {
                id: (*id).into(),
                name: (*name).into(),
                version: "随主程序".into(),
                enabled: self.enabled(id),
                running: self.running_count(id),
                external: false,
                rollback: false,
                busy: self.running_count(id) > 0,
            })
            .chain(self.registry.installed.values().map(|p| Entry {
                id: p.manifest.id.clone(),
                name: p.manifest.name.clone(),
                version: p.manifest.version.clone(),
                enabled: self.enabled(&p.manifest.id),
                running: self.running_count(&p.manifest.id),
                external: true,
                rollback: self.registry.previous.contains_key(&p.manifest.id),
                busy: self.exclusive_activity(&p.manifest.id, "更新").is_err(),
            }))
            .collect()
    }
    pub(crate) fn set_enabled(&mut self, id: &str, on: bool) -> Result<(), String> {
        let _lock = self.lock()?;
        if !self.entries().iter().any(|e| e.id == id) {
            return Err("插件未安装".into());
        }
        if on {
            self.registry.disabled.remove(id);
        } else {
            self.registry.disabled.insert(id.into());
        }
        self.persist()
    }
    pub(crate) fn install(&mut self, package: &Package) -> Result<(), String> {
        let bytes = package.validate()?;
        let _lock = self.lock()?;
        let id = &package.manifest.id;
        let _activity = self.exclusive_activity(id, "更新")?;
        let directory = uuid::Uuid::new_v4().to_string();
        let folder = self.root.join(&directory);
        fs::create_dir(&folder).map_err(|e| e.to_string())?;
        let mut obsolete = None;
        let result = (|| {
            fs::write(folder.join("plugin.exe"), bytes).map_err(|e| e.to_string())?;
            let installed = Installed {
                manifest: package.manifest.clone(),
                directory,
            };
            if let Some(old) = self.registry.installed.insert(id.clone(), installed) {
                obsolete = self.registry.previous.insert(id.clone(), old);
            }
            self.persist()
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&folder);
            self.reload()?;
        }
        if result.is_ok()
            && let Some(old) = obsolete
        {
            self.remove_payload(&old.directory)?;
        }
        result
    }
    pub(crate) fn uninstall(&mut self, id: &str) -> Result<(), String> {
        let _lock = self.lock()?;
        if BUILTINS.iter().any(|(v, _)| *v == id) {
            return Err("此功能随主程序提供，可以停用".into());
        }
        if !self.registry.installed.contains_key(id) {
            return Err("插件未安装".into());
        }
        let _activity = self.exclusive_activity(id, "卸载")?;
        let installed = self.registry.installed.remove(id).ok_or("插件未安装")?;
        let previous = self.registry.previous.remove(id);
        self.persist()?;
        self.remove_payload(&installed.directory)?;
        if let Some(p) = previous {
            self.remove_payload(&p.directory)?;
        }
        Ok(())
    }
    fn remove_payload(&self, directory: &str) -> Result<(), String> {
        if !safe_directory(directory) {
            return Err("插件目录无效".into());
        }
        let dir = self.root.join(directory);
        if !dir.exists() {
            return Ok(());
        }
        let resolved = dir.canonicalize().map_err(|e| e.to_string())?;
        let root = self.root.canonicalize().map_err(|e| e.to_string())?;
        if !resolved.starts_with(&root) || resolved == root {
            return Err("插件目录超出允许范围".into());
        }
        fs::remove_dir_all(&dir).map_err(|e| format!("功能已卸载；剩余文件待释放后清理：{e}"))
    }
    pub(crate) fn rollback(&mut self, id: &str) -> Result<(), String> {
        let _lock = self.lock()?;
        if !self.registry.installed.contains_key(id) {
            return Err("插件未安装".into());
        }
        let _activity = self.exclusive_activity(id, "回退版本")?;
        let previous = self.registry.previous.get(id).ok_or("没有可回退的版本")?;
        self.checked_payload(previous)?;
        let previous = previous.clone();
        let current = self
            .registry
            .installed
            .insert(id.into(), previous)
            .ok_or("插件未安装")?;
        self.registry.previous.insert(id.into(), current);
        self.persist()
    }
    fn running_count(&self, id: &str) -> usize {
        self.running
            .get(id)
            .map_or(0, |count| count.load(Ordering::Acquire))
    }
    fn activity_file(&self, id: &str) -> Result<File, String> {
        // IDs have been validated before reaching this path. Never remove this file:
        // deleting/recreating a lock file could split tasks onto different locks.
        if !panzo_plugin_api::valid_id(id) {
            return Err("插件 ID 无效".into());
        }
        let directory = self.root.join("leases");
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join(format!("{}.lock", panzo_plugin_api::hash(id.as_bytes()))))
            .map_err(|e| e.to_string())
    }
    fn exclusive_activity(&self, id: &str, action: &str) -> Result<File, String> {
        let file = self.activity_file(id)?;
        file.try_lock().map_err(|_| format!(
            "插件正在处理任务（可能位于其他 Panzo 窗口），完成后再{action}；可先停用以阻止新任务"
        ))?;
        Ok(file)
    }
    pub(super) fn acquire(&mut self, id: &str) -> Result<Lease, String> {
        // Serialize fresh enabled/version selection with install/uninstall. The
        // shared OS lease outlives this short registry lock and spans the whole job.
        let _registry = self.lock()?;
        if !self.enabled(id) {
            return Err("此功能未安装或已停用，请到顶部「插件」检查".into());
        }
        let activity = self.activity_file(id)?;
        activity
            .try_lock_shared()
            .map_err(|_| "插件正在更新，请稍后重试")?;
        // Integrity verification remains in the background worker runtime; acquiring
        // a lease on the UI thread only pins metadata and the OS activity lock.
        let installed = self.registry.installed.get(id).cloned();
        let running = self.running.entry(id.into()).or_default().clone();
        running.fetch_add(1, Ordering::AcqRel);
        Ok(Lease {
            root: self.root.clone(),
            installed,
            running,
            _activity: activity,
        })
    }
    #[cfg(test)]
    pub(super) fn executable(&self, id: &str) -> Result<(Manifest, PathBuf), String> {
        let item = self.registry.installed.get(id).ok_or("插件未安装")?;
        Ok((item.manifest.clone(), self.checked_payload(item)?))
    }
    fn checked_payload(&self, item: &Installed) -> Result<PathBuf, String> {
        checked_payload(&self.root, item)
    }
}
fn checked_payload(root: &Path, item: &Installed) -> Result<PathBuf, String> {
    let path = root.join(&item.directory).join("plugin.exe");
    let resolved = path.canonicalize().map_err(|e| e.to_string())?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    if !resolved.starts_with(&root) {
        return Err("插件路径超出安装目录".into());
    }
    let bytes = read_bounded(&resolved, 32 * 1024 * 1024)?;
    if panzo_plugin_api::hash(&bytes) != item.manifest.executable_sha256.to_ascii_lowercase() {
        return Err("插件文件已改变，请重新安装".into());
    }
    Ok(resolved)
}
fn safe_directory(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok() && s.len() == 36
}
pub(crate) fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("文件超过允许大小".into());
    }
    Ok(bytes)
}
pub(crate) fn read_package(path: &Path) -> Result<Package, String> {
    let bytes = read_bounded(path, MAX_PACKAGE)?;
    let package: Package =
        serde_json::from_slice(&bytes).map_err(|e| format!("插件包无法读取：{e}"))?;
    package.validate()?;
    Ok(package)
}
fn store() -> Result<&'static Mutex<Store>, String> {
    static STORE: OnceLock<Result<Mutex<Store>, String>> = OnceLock::new();
    STORE
        .get_or_init(|| {
            let root = std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .ok_or("无法获取本机配置目录")?
                .join("Panzo")
                .join("Plugins");
            #[cfg(test)]
            let root = std::env::var_os("PANZO_PLUGIN_REVIEW_DIR").map_or(root, PathBuf::from);
            Store::open(root).map(Mutex::new)
        })
        .as_ref()
        .map_err(Clone::clone)
}
pub(crate) fn with_store<T>(
    operation: impl FnOnce(&mut Store) -> Result<T, String>,
) -> Result<T, String> {
    let mut s = store()?.lock().map_err(|_| "插件管理器不可用")?;
    operation(&mut s)
}
pub(crate) fn enabled(id: &str) -> bool {
    with_store(|s| Ok(s.enabled(id))).unwrap_or(false)
}
pub(crate) fn require(id: &str) -> Result<(), String> {
    if enabled(id) {
        Ok(())
    } else {
        Err("此功能已停用，请到顶部「插件」启用".into())
    }
}
pub(crate) fn external_commands() -> Vec<(String, String, String)> {
    with_store(|s| {
        s.reload()?;
        Ok(s.registry
            .installed
            .values()
            .filter(|p| {
                p.manifest.protocol == panzo_plugin_api::PROTOCOL && s.enabled(&p.manifest.id)
            })
            .flat_map(|p| {
                p.manifest.commands.iter().map(|c| {
                    (
                        p.manifest.id.clone(),
                        c.id.clone(),
                        format!(
                            "{} · {} [{} / {}]",
                            p.manifest.name, c.name, p.manifest.id, c.id
                        ),
                    )
                })
            })
            .collect())
    })
    .unwrap_or_default()
}
pub(crate) struct Lease {
    root: PathBuf,
    installed: Option<Installed>,
    running: Arc<AtomicUsize>,
    _activity: File,
}
impl Lease {
    pub(super) fn executable(&self) -> Result<(Manifest, PathBuf), String> {
        // Resolve the version selected when this job was accepted, even if the
        // registry is refreshed or this plugin is disabled while the job runs.
        let item = self.installed.as_ref().ok_or("内置功能没有独立插件程序")?;
        Ok((item.manifest.clone(), checked_payload(&self.root, item)?))
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.running.fetch_sub(1, Ordering::AcqRel);
        // File drops after this method, releasing the cross-process shared lease.
        // No global registry mutex or process control is needed during teardown.
    }
}
pub(crate) fn acquire(id: &str) -> Result<Lease, String> {
    with_store(|s| s.acquire(id))
}
#[cfg(test)]
#[path = "plugin_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "plugin_lifecycle_review.rs"]
mod lifecycle_review;
