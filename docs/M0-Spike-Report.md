# Panzo V0.1 M0 技术 Spike 报告

执行日期：2026-08-31（Asia/Shanghai）

本报告记录当前开发机的真实 API 验证结果，不代表 V0.1 产品验收完成。测试入口和实现均已提交到仓库，可通过根目录 `README.md` 重放。

## 结果摘要

| M0 决定项 | 状态 | 证据 |
|---|---|---|
| QPC 100 ns Session Tick | Pass | QPC frequency = 10,000,000；转换和边界单测通过 |
| WGC 无 Cursor 捕获 | Blocked | `IsCursorCaptureEnabled(false)` 与帧回调已实现；当前自动化交互会话在 `CreateForMonitor` 间歇返回 `0x8007000E` |
| WGC QPC 时间读取 | Code complete / runtime blocked | 帧回调读取 `SystemRelativeTime.Duration`；受同一 Capture Item 阻塞，尚无本机帧样本 |
| D3D11 BGRA→NV12 | Pass | 1920×1080 Video Processor 枚举、格式检查、输入/输出 View 和 `VideoProcessorBlt` 均成功 |
| Media Foundation Hardware H.264 | Pass | 枚举并实际选中 `NVIDIA H.264 Encoder MFT` |
| 60 秒 fMP4 | Pass | 3,600 帧，600,000,000 tick，H.264 High，1920×1080，60 fps，200 个 fragment |
| 强制终止恢复 | Pass（Spike） | 仅终止探针子进程且未调用 `Finalize`；遗留 fMP4 可读取，恢复时长 8.7 秒 |

## 60 秒 fMP4 证据

命令：

~~~powershell
cargo run -p panzo-app --bin panzo-cli -- fmp4-probe .tmp\m0-fmp4-60s.part.mp4 60
~~~

探针输出：

~~~text
Frames: 3600
Duration tick: 600000000
Output bytes: 365494
fMP4 fragments: 200
Encoder: NVIDIA H.264 Encoder MFT (hardware)
Finalized: true
Elapsed: 7725 ms
~~~

`ffprobe` 额外验证：H.264 High、1920×1080、`yuv420p`、60/1 fps、时长 60.000000 秒。黑帧内容使文件远小于真实桌面录制，不可用该文件大小推算实际码率或磁盘需求。

## 强制终止证据

测试以明确的子进程 PID 启动 600 秒媒体时间探针，1.5 秒后仅终止该子进程。生成的 `m0-fmp4-crash.part.mp4` 大小为 53,722 字节；未执行 Sink Writer `Finalize`，仍能被读取为 H.264 High、1920×1080、60 fps，恢复时长 8.7 秒。

这证明 fragmented MP4 方向满足异常退出后保留既有 fragment 的基本前提。完整 `IT-RECOVERY-001` 仍需加入 Project Lock、最后视频 PTS、事件裁剪与五次循环自动化后验收。

## WGC 阻塞说明

当前会话已经验证：

- `GetDesktopWindow` 与 `MonitorFromWindow(..., MONITOR_DEFAULTTOPRIMARY)` 返回有效 HMONITOR。
- `GetMonitorInfoW` 返回有效矩形 `(0, 0)-(2560, 1440)`。
- D3D11 硬件设备和 WinRT `IDirect3DDevice` 创建成功。
- `GraphicsCaptureSession::IsSupported` 有时返回 `true`，有时同样返回 `0x8007000E`。
- `IGraphicsCaptureItemInterop::CreateForMonitor` 稳定失败为 `0x8007000E`。

因此当前证据更符合自动化桌面会话或 Windows Capture Broker 状态限制，而非无效显示器句柄或 D3D11 不可用。下一步是在独立本地交互登录会话和第二台兼容机运行同一 `capture-probe`；在获得真实 WGC 帧、单调时间戳以及 Cursor Capture 关闭证据前，M0 不退出。

## 自动化状态

- `panzo-core`：25 个单元测试通过。
- Camera Golden：10 个 Fixture 由 1 个清单锁定测试覆盖并通过；更新测试默认 ignored。
- `panzo-windows`：5 个 QPC、Windows 原子 Manifest、fMP4 时间与容器辅助测试通过。
- 全工作区 `cargo fmt --check`、`cargo test --workspace --all-targets` 和 `cargo clippy -- -D warnings` 最终复验通过。
- `scripts/verify.ps1 -IncludeMediaProbe` 最终复验通过；2 秒媒体探针再次确认 NVIDIA Hardware MFT 与 7 个 fMP4 fragment。
