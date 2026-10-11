//! Independent media worker runtime with cancellation and process-tree cleanup.
use super::{plugin_runtime::WorkerJob, plugins};
use panzo_plugin_api::media::{self, Output, Request, Response, Task};
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
    task: Task,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<Output, String> {
    let (manifest, path) = lease.executable()?;
    if manifest.protocol != media::PROTOCOL
        || !manifest.commands.iter().any(|c| c.id == task.command())
    {
        return Err("此插件不支持所选媒体操作".into());
    }
    let request = Request {
        protocol: media::PROTOCOL,
        request_id: uuid::Uuid::new_v4().to_string(),
        task,
    };
    run(&path, request, cancel, timeout)
}
pub(super) fn run(
    path: &std::path::Path,
    request: Request,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<Output, String> {
    if cancel.load(Ordering::Acquire) {
        return Err("任务已取消".into());
    }
    let input = serde_json::to_vec(&request).map_err(|_| "插件请求无法编码")?;
    if input.len() > 64 * 1024 {
        return Err("媒体请求过大".into());
    }
    let mut child = Command::new(path)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("插件无法启动：{e}"))?;
    let _job = WorkerJob::attach(&mut child)?;
    let mut stdin = child.stdin.take().ok_or("无法打开插件管道")?;
    let stdout = child.stdout.take().ok_or("无法打开插件管道")?;
    let (tx, rx) = mpsc::channel();
    let writer = tx.clone();
    std::thread::spawn(move || {
        let r = stdin
            .write_all(&input)
            .map(|_| None)
            .map_err(|_| "插件输入中断".to_owned());
        drop(stdin);
        let _ = writer.send(r);
    });
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let r = stdout
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "插件输出中断".to_owned())
            .and_then(|_| {
                if bytes.len() > 4 * 1024 * 1024 {
                    Err("插件输出过大".into())
                } else {
                    Ok(Some(bytes))
                }
            });
        let _ = tx.send(r);
    });
    let result = (|| {
        let deadline = Instant::now() + timeout;
        let mut output = None;
        let mut written = false;
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err("任务已取消".into());
            }
            if Instant::now() >= deadline {
                return Err("媒体插件超时，任务已终止".into());
            }
            for r in rx.try_iter() {
                match r? {
                    Some(b) => output = Some(b),
                    None => written = true,
                }
            }
            if let Some(status) = child.try_wait().map_err(|_| "无法读取插件状态")? {
                if !status.success() {
                    return Err("媒体插件异常退出，工程未修改".into());
                }
                if written && let Some(bytes) = output.take() {
                    let r: Response =
                        serde_json::from_slice(&bytes).map_err(|_| "媒体插件响应格式无效")?;
                    if r.protocol != media::PROTOCOL || r.request_id != request.request_id {
                        return Err("媒体插件响应与任务不匹配".into());
                    }
                    if let Some(e) = r.error {
                        return Err(e.chars().take(1000).collect());
                    }
                    return r.output.ok_or_else(|| "媒体插件没有返回结果".into());
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}
