use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
fn package(version: &str) -> Package {
    let mut bytes = vec![0u8; 256];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
    bytes[64..68].copy_from_slice(b"PE\0\0");
    bytes[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
    bytes[100] = version.bytes().last().unwrap();
    Package {
        format: 1,
        manifest: Manifest {
            id: "example.filter".into(),
            name: "测试滤镜".into(),
            version: version.into(),
            publisher: "测试".into(),
            description: "仅单元测试".into(),
            protocol: 1,
            target: panzo_plugin_api::target(),
            commands: vec![panzo_plugin_api::Command {
                id: "invert".into(),
                name: "反色".into(),
            }],
            executable_sha256: panzo_plugin_api::hash(&bytes),
        },
        executable_base64: STANDARD.encode(bytes),
    }
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("panzo-plugins-test-{}", uuid::Uuid::new_v4())))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn independent_package_install_disable_reload_update_rollback_uninstall() {
    let temp = Fixture::new();
    let mut s = Store::open(temp.0.clone()).unwrap();
    let first = package("0.1.0");
    s.install(&first).unwrap();
    let (_, path) = s.executable(&first.manifest.id).unwrap();
    assert!(path.exists());
    s.set_enabled(&first.manifest.id, false).unwrap();
    let mut s = Store::open(temp.0.clone()).unwrap();
    assert!(!s.enabled(&first.manifest.id));
    s.install(&package("0.2.0")).unwrap();
    assert!(!s.enabled(&first.manifest.id));
    s.rollback(&first.manifest.id).unwrap();
    assert_eq!(
        s.registry.installed[&first.manifest.id].manifest.version,
        "0.1.0"
    );
    s.set_enabled(&first.manifest.id, true).unwrap();
    fs::write(s.root.join("user-result.png"), b"keep").unwrap();
    s.uninstall(&first.manifest.id).unwrap();
    assert!(!s.enabled(&first.manifest.id));
    assert!(!path.exists());
    assert_eq!(fs::read(s.root.join("user-result.png")).unwrap(), b"keep");
}
#[test]
fn invalid_update_leaves_previous_plugin_intact() {
    let temp = Fixture::new();
    let mut s = Store::open(temp.0.clone()).unwrap();
    let good = package("0.1.0");
    s.install(&good).unwrap();
    let mut bad = package("0.2.0");
    bad.manifest.executable_sha256 = "0".repeat(64);
    assert!(s.install(&bad).is_err());
    assert_eq!(
        s.registry.installed[&good.manifest.id].manifest.version,
        "0.1.0"
    );
    let mut wrong = good.clone();
    wrong.manifest.protocol = 999;
    assert!(s.install(&wrong).is_err());
    let mut wrong = good.clone();
    wrong.manifest.id = "../escape".into();
    assert!(s.install(&wrong).is_err());
    let mut wrong = good.clone();
    wrong.manifest.target = "wrong-platform".into();
    assert!(s.install(&wrong).is_err());
}
#[test]
fn in_flight_jobs_block_removal_and_update_but_allow_disable() {
    let temp = Fixture::new();
    let mut s = Store::open(temp.0.clone()).unwrap();
    let p = package("0.1.0");
    s.install(&p).unwrap();
    let lease = s.acquire(&p.manifest.id).unwrap();
    s.set_enabled(&p.manifest.id, false).unwrap();
    assert!(s.uninstall(&p.manifest.id).is_err());
    assert!(s.install(&package("0.2.0")).is_err());
    assert!(s.executable(&p.manifest.id).is_ok());
    drop(lease);
    assert!(s.uninstall(&p.manifest.id).is_ok());
}
#[test]
fn detects_payload_tampering_and_keeps_builtin_preferences() {
    let temp = Fixture::new();
    let mut s = Store::open(temp.0.clone()).unwrap();
    s.set_enabled(OCR, false).unwrap();
    let p = package("0.1.0");
    s.install(&p).unwrap();
    let (_, path) = s.executable(&p.manifest.id).unwrap();
    fs::write(path, b"changed").unwrap();
    assert!(s.executable(&p.manifest.id).is_err());
    let s = Store::open(temp.0.clone()).unwrap();
    assert!(!s.enabled(OCR));
    assert!(s.enabled(VISION));
}
#[test]
fn corrupt_registry_is_reported_without_reset() {
    let temp = Fixture::new();
    fs::create_dir_all(&temp.0).unwrap();
    fs::write(temp.0.join("registry.json"), b"broken").unwrap();
    assert!(Store::open(temp.0.clone()).is_err());
    assert_eq!(fs::read(temp.0.join("registry.json")).unwrap(), b"broken");
}

#[test]
fn damaged_rollback_keeps_current_version_and_registry() {
    let temp = Fixture::new();
    let mut s = Store::open(temp.0.clone()).unwrap();
    let first = package("0.1.0");
    s.install(&first).unwrap();
    let (_, previous_path) = s.executable(&first.manifest.id).unwrap();
    s.install(&package("0.2.0")).unwrap();
    fs::write(previous_path, b"damaged").unwrap();
    assert!(s.rollback(&first.manifest.id).is_err());
    let s = Store::open(temp.0.clone()).unwrap();
    let (manifest, _) = s.executable(&first.manifest.id).unwrap();
    assert_eq!(manifest.version, "0.2.0");
    assert_eq!(
        s.registry.previous[&first.manifest.id].manifest.version,
        "0.1.0"
    );
}

#[test]
fn manager_refresh_observes_media_install_from_another_host() {
    let temp = Fixture::new();
    let mut stale = Store::open(temp.0.clone()).unwrap();
    let mut installer = Store::open(temp.0.clone()).unwrap();
    let mut media = package("1.0.0");
    media.manifest.id = panzo_plugin_api::media::PLUGIN_ID.into();
    media.manifest.name = "声音、旁白与 AI 字幕".into();
    media.manifest.protocol = panzo_plugin_api::media::PROTOCOL;
    media.manifest.commands[0].id = "speech.transcribe".into();
    installer.install(&media).unwrap();
    assert!(!stale.entries().iter().any(|e| e.id == media.manifest.id));
    let entries = stale.refreshed_entries().unwrap();
    let entry = entries.iter().find(|e| e.id == media.manifest.id).unwrap();
    assert_eq!(entry.name, "声音、旁白与 AI 字幕");
    assert!(entry.enabled && entry.external);
    installer.set_enabled(&media.manifest.id, false).unwrap();
    let entries = stale.refreshed_entries().unwrap();
    assert!(
        !entries
            .iter()
            .find(|e| e.id == media.manifest.id)
            .unwrap()
            .enabled
    );
}

#[test]
fn invalid_refresh_preserves_last_valid_registry_and_job_leases() {
    let temp = Fixture::new();
    let mut store = Store::open(temp.0.clone()).unwrap();
    let media = package("1.0.0");
    store.install(&media).unwrap();
    let _lease = store.acquire(&media.manifest.id).unwrap();
    let mut bad = store.registry.clone();
    bad.installed
        .get_mut(&media.manifest.id)
        .unwrap()
        .manifest
        .protocol = 999;
    fs::write(
        temp.0.join("registry.json"),
        serde_json::to_vec(&bad).unwrap(),
    )
    .unwrap();
    assert!(store.refreshed_entries().is_err());
    let entry = store
        .entries()
        .into_iter()
        .find(|e| e.id == media.manifest.id)
        .unwrap();
    assert_eq!(
        store.registry.installed[&media.manifest.id]
            .manifest
            .protocol,
        1
    );
    assert!(entry.enabled);
    assert_eq!(entry.running, 1);
}

#[test]
fn independent_stores_share_job_leases_and_pin_accepted_version() {
    let temp = Fixture::new();
    let mut host = Store::open(temp.0.clone()).unwrap();
    let mut manager = Store::open(temp.0.clone()).unwrap();
    let first = package("0.1.0");
    host.install(&first).unwrap();
    host.install(&package("0.2.0")).unwrap();
    let mut other = package("1.0.0");
    other.manifest.id = "example.other".into();
    host.install(&other).unwrap();
    let accepted = host.acquire(&first.manifest.id).unwrap();
    let other_job = host.acquire(&other.manifest.id).unwrap();
    // An independently loaded registry has no in-memory task counter for host.
    assert_eq!(manager.running_count(&first.manifest.id), 0);
    assert!(manager.uninstall(&first.manifest.id).is_err());
    assert!(manager.install(&package("0.3.0")).is_err());
    assert!(manager.rollback(&first.manifest.id).is_err());
    manager.set_enabled(&first.manifest.id, false).unwrap();
    assert!(host.acquire(&first.manifest.id).is_err());
    assert_eq!(accepted.executable().unwrap().0.version, "0.2.0");
    assert!(other_job.executable().is_ok());
    drop(accepted);
    manager.uninstall(&first.manifest.id).unwrap();
    assert!(host.acquire(&first.manifest.id).is_err());
    assert!(other_job.executable().is_ok());
    assert_eq!(host.running_count(&other.manifest.id), 1);
    drop(other_job);
    manager.uninstall(&other.manifest.id).unwrap();
}

#[test]
fn multiple_jobs_hold_shared_leases_until_the_last_job_finishes() {
    let temp = Fixture::new();
    let mut host = Store::open(temp.0.clone()).unwrap();
    let p = package("0.1.0");
    host.install(&p).unwrap();
    let a = host.acquire(&p.manifest.id).unwrap();
    let b = host.acquire(&p.manifest.id).unwrap();
    assert_eq!(host.running_count(&p.manifest.id), 2);
    assert!(host.uninstall(&p.manifest.id).is_err());
    drop(a);
    assert_eq!(host.running_count(&p.manifest.id), 1);
    assert!(host.uninstall(&p.manifest.id).is_err());
    drop(b);
    assert_eq!(host.running_count(&p.manifest.id), 0);
    host.uninstall(&p.manifest.id).unwrap();
}

#[test]
fn stale_host_observes_reinstallation_without_restart() {
    let temp = Fixture::new();
    let mut host = Store::open(temp.0.clone()).unwrap();
    let mut manager = Store::open(temp.0.clone()).unwrap();
    let p = package("0.1.0");
    manager.install(&p).unwrap();
    let first = host.acquire(&p.manifest.id).unwrap();
    assert_eq!(first.executable().unwrap().0.version, "0.1.0");
    drop(first);
    manager.uninstall(&p.manifest.id).unwrap();
    assert!(host.acquire(&p.manifest.id).is_err());
    manager.install(&package("0.2.0")).unwrap();
    let updated = host.acquire(&p.manifest.id).unwrap();
    assert_eq!(updated.executable().unwrap().0.version, "0.2.0");
}
