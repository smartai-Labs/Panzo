//! Opt-in independent-process checks for the production plugin registry/runtimes.
use super::*;
use crate::platform::{capture_tools::Raster, media_plugin_runtime, plugin_runtime};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    process::{Child, Command, Stdio},
    sync::atomic::AtomicBool,
    thread,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_TIMEOUT},
    System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
};

fn wait_json(path: &Path) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(bytes) = fs::read(path)
            && let Ok(value) = serde_json::from_slice(&bytes)
        {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "fixture marker timed out: {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(15));
    }
}
struct Process(HANDLE);
impl Process {
    fn open(pid: u32) -> Self {
        Self(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.unwrap())
    }
    fn alive(&self) -> bool {
        (unsafe { WaitForSingleObject(self.0, 0) }) == WAIT_TIMEOUT
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn package(bytes: &[u8], id: &str, version: &str, protocol: u32) -> Package {
    let commands = if protocol == 1 {
        vec!["hold", "echo", "fail", "invalid", "tree"]
    } else {
        vec!["audio.import"]
    };
    Package {
        format: 1,
        manifest: Manifest {
            id: id.into(),
            name: id.into(),
            version: version.into(),
            publisher: "Panzo lifecycle test".into(),
            description: "Isolated lifecycle fixture; never distributed".into(),
            protocol,
            target: panzo_plugin_api::target(),
            commands: commands
                .into_iter()
                .map(|id| panzo_plugin_api::Command {
                    id: id.into(),
                    name: id.into(),
                })
                .collect(),
            executable_sha256: panzo_plugin_api::hash(bytes),
        },
        executable_base64: STANDARD.encode(bytes),
    }
}
fn image() -> Raster {
    Raster {
        width: 1,
        height: 1,
        pixels: vec![12, 34, 56, 255],
    }
}
struct Worker {
    directory: PathBuf,
    id: u64,
    cancel: Arc<AtomicBool>,
    task: Option<thread::JoinHandle<Result<Raster, String>>>,
}
impl Worker {
    fn start(store: &mut Store, plugin: &str, command: &str, id: u64) -> Self {
        let lease = store.acquire(plugin).unwrap();
        let directory = lease
            .executable()
            .unwrap()
            .1
            .parent()
            .unwrap()
            .to_path_buf();
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        let command = command.to_owned();
        let task = thread::spawn(move || {
            plugin_runtime::execute(&lease, &command, id, 7, &image(), &token)
        });
        let worker = Self {
            directory,
            id,
            cancel,
            task: Some(task),
        };
        wait_json(&worker.directory.join(format!("started-{id}.json")));
        worker
    }
    fn pid(&self) -> u32 {
        wait_json(&self.directory.join(format!("started-{}.json", self.id)))["pid"]
            .as_u64()
            .unwrap() as u32
    }
    fn finish(mut self) -> Result<Raster, String> {
        fs::write(self.directory.join(format!("release-{}", self.id)), b"go").unwrap();
        self.task.take().unwrap().join().unwrap()
    }
    fn cancel(mut self) -> Result<Raster, String> {
        self.cancel.store(true, Ordering::Release);
        self.task.take().unwrap().join().unwrap()
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(task) = self.task.take() {
            let _ = task.join();
        }
    }
}

#[test]
#[ignore = "only spawned by independent_process_lifecycle_isolation with an isolated lease root"]
fn lease_holder_process() {
    let Some(root) = std::env::var_os("PANZO_LEASE_CHILD_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let mut store = Store::open(root.clone()).unwrap();
    let _lease = store.acquire("review.alpha").unwrap();
    fs::write(
        root.join("lease-child.json"),
        serde_json::to_vec(&json!({"pid": std::process::id()})).unwrap(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !root.join("lease-child-release").exists() {
        assert!(Instant::now() < deadline, "lease helper timeout");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "requires PANZO_LIFECYCLE_FIXTURE_EXE and PANZO_LIFECYCLE_REVIEW_DIR; isolated processes only"]
fn independent_process_lifecycle_isolation() {
    let fixture =
        PathBuf::from(std::env::var_os("PANZO_LIFECYCLE_FIXTURE_EXE").expect("fixture EXE"));
    let out = PathBuf::from(std::env::var_os("PANZO_LIFECYCLE_REVIEW_DIR").expect("review output"));
    fs::create_dir_all(&out).unwrap();
    let root = out.join(format!("isolated-{}", uuid::Uuid::new_v4()));
    let host_pid = std::process::id();
    let bytes = fs::read(fixture).unwrap();
    let alpha = package(&bytes, "review.alpha", "1.0.0", 1);
    let beta = package(&bytes, "review.beta", "1.0.0", 1);
    let media = package(&bytes, "review.media", "1.0.0", 2);
    let mut host = Store::open(root.clone()).unwrap();
    for package in [&alpha, &beta, &media] {
        host.install(package).unwrap();
    }
    let mut manager = Store::open(root.clone()).unwrap();
    let survivor = Worker::start(&mut host, &beta.manifest.id, "hold", 20);
    let survivor_pid = survivor.pid();
    let survivor_process = Process::open(survivor_pid);
    let retained = root.join("user-output.txt");
    fs::write(&retained, b"user result").unwrap();
    let assert_survivor = || {
        assert_eq!(std::process::id(), host_pid);
        assert!(survivor_process.alive(), "unrelated plugin was stopped");
        assert_eq!(fs::read(&retained).unwrap(), b"user result");
    };
    let normal = Worker::start(&mut host, &alpha.manifest.id, "echo", 1)
        .finish()
        .unwrap();
    assert_eq!(normal.pixels, image().pixels);
    manager.uninstall(&alpha.manifest.id).unwrap();
    assert!(host.acquire(&alpha.manifest.id).is_err());
    assert_survivor();
    manager.install(&alpha).unwrap();
    manager
        .install(&package(&bytes, &alpha.manifest.id, "2.0.0", 1))
        .unwrap();
    let accepted = Worker::start(&mut host, &alpha.manifest.id, "hold", 2);
    let accepted_process = Process::open(accepted.pid());
    assert!(manager.uninstall(&alpha.manifest.id).is_err());
    assert!(manager.install(&alpha).is_err());
    assert!(manager.rollback(&alpha.manifest.id).is_err());
    manager.set_enabled(&alpha.manifest.id, false).unwrap();
    assert!(host.acquire(&alpha.manifest.id).is_err());
    assert!(accepted_process.alive());
    assert_survivor();
    assert_eq!(accepted.finish().unwrap().pixels, image().pixels);
    manager.uninstall(&alpha.manifest.id).unwrap();
    assert_survivor();
    manager.install(&alpha).unwrap();
    manager.set_enabled(&alpha.manifest.id, true).unwrap();
    assert!(
        Worker::start(&mut host, &alpha.manifest.id, "fail", 3)
            .finish()
            .is_err()
    );
    assert_survivor();
    assert!(
        Worker::start(&mut host, &alpha.manifest.id, "invalid", 4)
            .finish()
            .is_err()
    );
    assert_survivor();
    let tree = Worker::start(&mut host, &alpha.manifest.id, "tree", 5);
    let parent_pid = tree.pid();
    let parent_process = Process::open(parent_pid);
    let descendant_pid = wait_json(&tree.directory.join("descendant-5.json"))["pid"]
        .as_u64()
        .unwrap() as u32;
    let descendant = Process::open(descendant_pid);
    assert!(tree.cancel().err().unwrap().contains("取消"));
    let deadline = Instant::now() + Duration::from_secs(3);
    while descendant.alive() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!parent_process.alive());
    assert!(!descendant.alive());
    assert_survivor();
    // The media runtime uses its own job object; a real timeout must not stop beta.
    let lease = host.acquire(&media.manifest.id).unwrap();
    let media_directory = lease
        .executable()
        .unwrap()
        .1
        .parent()
        .unwrap()
        .to_path_buf();
    let error = media_plugin_runtime::execute(
        &lease,
        panzo_plugin_api::media::Task::DecodeAudio {
            source: "90".into(),
            output: "unused-fixture-output".into(),
        },
        &AtomicBool::new(false),
        Duration::from_millis(500),
    )
    .err()
    .unwrap();
    assert!(error.contains("超时"));
    let media_pid = wait_json(&media_directory.join("started-90.json"))["pid"]
        .as_u64()
        .unwrap();
    drop(lease);
    manager.uninstall(&media.manifest.id).unwrap();
    assert_survivor();
    // Separate host process owns a task lease: no shared Rust state/counters.
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "platform::plugins::lifecycle_review::lease_holder_process",
            "--ignored",
            "--nocapture",
        ])
        .env("PANZO_LEASE_CHILD_ROOT", &root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = ChildGuard(child);
    let lease_pid = wait_json(&root.join("lease-child.json"))["pid"]
        .as_u64()
        .unwrap();
    assert_eq!(u64::from(child.0.id()), lease_pid);
    assert!(manager.uninstall(&alpha.manifest.id).is_err());
    assert!(manager.install(&alpha).is_err());
    assert_survivor();
    // Simulated host crash: OS releases its lease without restart or manual repair.
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    manager.uninstall(&alpha.manifest.id).unwrap();
    assert_survivor();
    assert_eq!(survivor.pid(), survivor_pid);
    assert_eq!(survivor.finish().unwrap().pixels, image().pixels);
    assert!(!survivor_process.alive());
    manager.uninstall(&beta.manifest.id).unwrap();
    assert_eq!(fs::read(&retained).unwrap(), b"user result");
    fs::write(out.join("lifecycle-review.json"), serde_json::to_vec_pretty(&json!({
        "passed": true, "hostPid": host_pid, "survivingPluginPid": survivor_pid,
        "canceledWorkerPid": parent_pid, "canceledDescendantPid": descendant_pid,
        "timedOutMediaWorkerPid": media_pid, "independentLeaseHostPid": lease_pid,
        "isolatedRegistry": root, "hostKind": "native test host running production registry and worker runtimes",
        "checks": ["uninstall alpha while beta runs; host and beta unchanged", "busy update/rollback/uninstall rejected", "disable blocks new work while accepted work completes", "hot reinstall without restarting host", "abnormal exit and malformed output isolated", "cancel kills only target process tree", "media timeout isolated", "independent host lease blocks uninstall/update", "host crash releases OS lease", "user output retained"]
    })).unwrap()).unwrap();
}
