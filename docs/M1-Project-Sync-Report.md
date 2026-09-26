# Panzo V0.1 M1：Project 与同步阶段报告

执行日期：2026-08-31～2026-09-01（Asia/Shanghai）

本阶段已完成 M1 代码实现、自动复验和交互式 Windows 11 桌面验收。`IT-REC-001`、`IT-REC-002`、`IT-RECOVERY-001` 均已通过。

## 实现摘要

| 能力 | 状态 | 证据 |
|---|---|---|
| 统一 Session Clock | 完成 | 第一个有效 WGC `SystemRelativeTime` 建立 Epoch；QPC/视频统一换算为 100 ns Tick |
| 视频时间单调性 | 完成 | 重复时间戳替换待编码帧；倒退时间戳立即失败；fMP4 parser 计算最后媒体时间 |
| 输入采集 | 完成 | 120 Hz 高精度 Timer、`WH_MOUSE_LL`、65,536 有界无锁队列、按钮精确 Cursor Anchor |
| Project Writer | 完成 | Cursor/Click/Journal JSONL；1 秒 flush；2 秒 durable checkpoint；逐轨单调性检查 |
| `recording.lock` | 完成 | PID、Session ID、最后 checkpoint；Windows write-through 原子替换；正常终态删除 |
| WGC→编码 | 完成 | Capture / Processing D3D11 Device 隔离；NT Shared Texture + IDXGIKeyedMutex；60 FPS VFR 限帧；Hardware H.264 fMP4 |
| 正常 Finalize | 完成 | 状态机、排空队列、Finalize、媒体重命名、Camera/metrics/diagnostics、`ready` |
| 启动恢复 | 完成 | 活进程保护、JSONL 半行修复、完整 fragment 配对、媒体截止时间、事件修剪、终态迁移 |

## 自动复验结果

- `panzo-core`：32 个单元测试通过。
- Camera Golden：10 个 Fixture 由 1 个清单测试覆盖并通过；更新测试保持 ignored。
- `panzo-windows`：11 个测试通过，包括可复用 NV12 fMP4 Writer、GPU readback、活进程锁保护，以及 dead recording 的完整恢复与事件修剪。
- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- Hardware H.264：`NVIDIA H.264 Encoder MFT`。
- D3D11：真实 `VideoProcessorBlt` 与 NV12 staging readback 通过。
- 恢复夹具：1 秒 fMP4，残留锁 PID=0，Cursor JSONL 含半行和晚于视频的事件；扫描后状态为 `recovered`、半行截断、晚事件删除、锁移除。

## 最终交互式验收

- 60 秒主录制：`ready`，60.0185 秒，3601 编码帧，平均 59.9981 FPS，背压丢帧 0，Capture Queue 峰值 3/4。
- 输入：2487 条 Cursor、28 条 Click；按钮 Cursor Anchor 验证通过。
- 媒体：26,421,332 bytes，190 个完整 Fragment，无尾部损坏，PTS 截止 600,185,333 Tick。
- 强制恢复：录制 20 秒后终止，恢复为 `recovered`；保留 19.9504 秒、63 个完整 Fragment，事件 JSONL 合法，`recording.lock` 已删除。
- 恢复媒体保留 `screen.part.mp4`，Manifest 正确引用实际可播放文件，符合 7.1 节约束。

## 自动化会话环境说明

当前自动化会话没有交互式输入桌面：

- 1 秒 Input Probe 完成 118 次采样尝试，Timer 与线程退出正常，但 `GetCursorPos/GetCursorInfo` 118 次均被桌面隔离拒绝。
- WGC Preflight 在 `GraphicsCaptureSession::IsSupported` 返回 `0x8007000E`；录制探针按规范在创建 Project 前失败。
- D3D11、NV12、Hardware H.264、fMP4、Project Writer 和 Recovery 不受该限制，均已实际运行。

自动化开发会话仍不能直接完成 WGC 交互录制；最终结果已由独立 Windows 11 交互式登录会话执行以下命令取得：

```powershell
.\scripts\verify-m1.ps1 -DurationSeconds 60
```

录制期间应连续移动鼠标，并分别点击 Left、Right、Middle。脚本会创建唯一 Project 路径，不覆盖现有数据。

该脚本默认继续运行 `verify-m1-recovery.ps1`：启动明确的 `panzo-cli.exe` 子进程，Epoch 建立后录制 20 秒，仅按该 PID 强制终止，再执行启动恢复并验证 `recovered`、锁清理、合法 JSONL 以及最多 2 秒数据损失。只复验正常录制时可传入 `-SkipRecovery`。

## 验收映射

| 用例 | 当前状态 | 关闭条件 |
|---|---|---|
| IT-REC-001 | 通过 | 60 秒 Project=`ready`，媒体可读、PTS 单调、平均 59.9981 FPS |
| IT-REC-002 | 通过 | Cursor/Click 完整，按钮处存在精确 Cursor Anchor |
| IT-RECOVERY-001 | 通过 | 强制终止后恢复 19.9504 秒，超过 18 秒最低门槛 |
