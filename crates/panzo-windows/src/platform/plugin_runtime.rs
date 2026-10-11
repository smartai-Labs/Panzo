//! Trusted native plugins run as short-lived workers with a bounded JSON protocol.
use super::{capture_tools::Raster, plugins};
use panzo_plugin_api::{Image, MAX_MESSAGE, PROTOCOL, Request, Response};
use std::{
    io::{Read, Write},
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
pub(crate) fn execute(
    lease: &plugins::Lease,
    command: &str,
    document_id: u64,
    revision: u64,
    image: &Raster,
    cancel: &AtomicBool,
) -> Result<Raster, String> {
    let (manifest, path) = lease.executable()?;
    run_worker(
        &manifest,
        &path,
        command,
        document_id,
        revision,
        image,
        cancel,
    )
}
fn run_worker(
    manifest: &panzo_plugin_api::Manifest,
    path: &std::path::Path,
    command: &str,
    document_id: u64,
    revision: u64,
    image: &Raster,
    cancel: &AtomicBool,
) -> Result<Raster, String> {
    if !manifest.commands.iter().any(|c| c.id == command) {
        return Err("插件未声明此操作".into());
    }
    let request_id = uuid::Uuid::new_v4().to_string();
    let request = Request {
        protocol: PROTOCOL,
        request_id: request_id.clone(),
        document_id,
        revision,
        command: command.into(),
        image: Image::new(image.width, image.height, &image.pixels)?,
    };
    let input = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
    if input.len() as u64 > MAX_MESSAGE {
        return Err("插件输入过大".into());
    }
    if cancel.load(Ordering::Acquire) {
        return Err("已取消插件任务".into());
    }
    let mut child = Command::new(path)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("插件无法启动：{e}"))?;
    let _job = WorkerJob::attach(&mut child)?;
    let stdin = child.stdin.take().ok_or("无法创建插件输入管道")?;
    let stdout = child.stdout.take().ok_or("无法创建插件输出管道")?;
    let (tx, rx) = mpsc::channel();
    let tx_input = tx.clone();
    std::thread::spawn(move || {
        let mut stdin = stdin;
        let result = stdin
            .write_all(&input)
            .map(|()| None)
            .map_err(|e| e.to_string());
        drop(stdin);
        let _ = tx_input.send(result);
    });
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(MAX_MESSAGE + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())
            .and_then(|_| {
                if bytes.len() as u64 > MAX_MESSAGE {
                    Err("插件输出超限".into())
                } else {
                    Ok(Some(bytes))
                }
            });
        let _ = tx.send(result);
    });
    let result = (|| {
        let deadline = Instant::now() + Duration::from_mins(1);
        let mut output = None;
        let mut writes = false;
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err("已取消插件任务".into());
            }
            if Instant::now() > deadline {
                return Err("插件运行超过 60 秒，任务已终止".into());
            }
            for message in rx.try_iter() {
                match message? {
                    Some(bytes) => output = Some(bytes),
                    None => writes = true,
                }
            }
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                if !status.success() {
                    return Err(format!("插件异常退出（{status}），原图已保留"));
                }
                if writes && let Some(bytes) = output.take() {
                    let response: Response =
                        serde_json::from_slice(&bytes).map_err(|e| format!("插件响应无效：{e}"))?;
                    if response.protocol != PROTOCOL || response.request_id != request_id {
                        return Err("插件响应与当前任务不匹配".into());
                    }
                    if let Some(e) = response.error {
                        return Err(e.chars().take(2048).collect());
                    }
                    let image = response.image.ok_or("插件没有返回图片")?;
                    let pixels = image.decode()?;
                    return Ok(Raster {
                        width: image.width,
                        height: image.height,
                        pixels,
                    });
                }
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    })();
    // Reap the worker on success, cancellation, invalid output and timeout alike.
    let _ = child.kill();
    let _ = child.wait();
    result
}

pub(super) struct WorkerJob(windows::Win32::Foundation::HANDLE);
impl WorkerJob {
    pub(super) fn attach(child: &mut std::process::Child) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        let result = (|| {
            let h = unsafe { CreateJobObjectW(None, None) }.map_err(|e| e.to_string())?;
            let job = Self(h);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            unsafe {
                SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    (&raw const info).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            }
            .map_err(|e| e.to_string())?;
            if let Err(e) = unsafe {
                AssignProcessToJobObject(
                    h,
                    windows::Win32::Foundation::HANDLE(child.as_raw_handle()),
                )
            } && child.try_wait().map_err(|e| e.to_string())?.is_none()
            {
                return Err(e.to_string());
            }
            Ok(job)
        })();
        if result.is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
        result
    }
}
impl Drop for WorkerJob {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires independently built PANZO_PLUGIN_REVIEW_PACKAGE"]
    fn independent_worker_round_trip() {
        let path = std::path::PathBuf::from(
            std::env::var_os("PANZO_PLUGIN_REVIEW_PACKAGE").expect("sample package"),
        );
        let package = plugins::read_package(&path).unwrap();
        let root =
            std::env::temp_dir().join(format!("panzo-plugin-worker-{}", uuid::Uuid::new_v4()));
        let mut store = plugins::Store::open(root.clone()).unwrap();
        store.install(&package).unwrap();
        let (manifest, path) = store.executable(&package.manifest.id).unwrap();
        let image = Raster {
            width: 2,
            height: 1,
            pixels: vec![0, 0, 255, 123, 255, 128, 0, 240],
        };
        let cancel = AtomicBool::new(false);
        let gray = run_worker(&manifest, &path, "grayscale", 41, 7, &image, &cancel).unwrap();
        assert_eq!(gray.pixels, vec![76, 76, 76, 123, 103, 103, 103, 240]);
        let inverse = run_worker(&manifest, &path, "invert", 41, 7, &image, &cancel).unwrap();
        assert_eq!(inverse.pixels, vec![255, 255, 0, 123, 0, 127, 255, 240]);
        assert!(run_worker(&manifest, &path, "not-declared", 41, 7, &image, &cancel).is_err());
        cancel.store(true, Ordering::Release);
        assert!(
            run_worker(&manifest, &path, "invert", 41, 7, &image, &cancel)
                .err()
                .unwrap()
                .contains("取消")
        );
        store.uninstall(&package.manifest.id).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
