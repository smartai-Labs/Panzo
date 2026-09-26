# M4 稳定性验收基础设施报告

状态：基础设施、短时稳定性烟测和正式 5/5 强制终止恢复通过；M4 正式退出条件尚未完成。

> 2026-09-05 状态补充：以上是本报告编写时的历史结论。后续主机正式 30 分钟录制已通过，证据为 [stability-acceptance.json](../.tmp/m4-stability-dee876307aab4faea49b6fc0b2ee6173.panzo/diagnostics/stability-acceptance.json)，2560×1440，实际约 1800.023 秒，`passed=true`。不得再把“30 分钟尚未执行”作为当前状态。跨 GPU 仍延期，当前发布安排按[新项目计划](../PROJECT-PLAN.md)执行；这不表示原 M4 全部退出条件已验收。

## 本阶段交付

- Capture Metrics 新增原始捕获帧正间隔的 `frameGapP99Tick` 与 `frameGapSampleCount`，`maxFrameGapTick` 修正为同一数据源的最大间隔。
- `RecordingStabilityReport` 使用整数运算统一核验第 16.3 节门槛，避免长录制比率计算中的浮点漂移和溢出。
- `stability-probe <project.panzo> [required-seconds]` 同时检查 Project Ready、录制时长、Metrics、Capture/Input Queue、fMP4 片段、尾随字节和 Manifest/容器时长一致性。
- Media Foundation 随机 Seek 增加 120 帧索引预滚，兼容硬件解码器把非关键帧 seek 跳到目标之后的行为。
- `stability-stimulus` 覆盖 Primary Monitor，使用 10 ms 高频唤醒和单调时钟控制 60 Hz 节拍；棋盘只预渲染一次，每帧局部恢复并移动扫描带，同时报告 Wakeup/Timer/Paint、有效刷新率和正常关闭状态。
- `scripts/verify-m4-stability.ps1` 默认自动启动动画刺激源、录制 1800 秒，并把验收结果保存为项目内 `diagnostics/stability-acceptance.json`；`-NoStimulus` 可用于外部动画源。
- `scripts/verify-m4-recovery.ps1` 默认执行 5 次受控强制终止，逐次检查恢复时长、锁文件、JSONL、fMP4，以及导出失败时源媒体 Hash 和临时文件清理。

## 本机短时真实证据

### 稳定性烟测

项目：`.tmp/m4-stability-0e4f107337d44536b64b9c4a1d496eba.panzo`

| 指标 | 实测 | 短时门槛 | 结果 |
|---|---:|---:|---|
| 捕获分辨率 | 2560 × 1440 | 原生尺寸 | Pass |
| 动画刺激源 | 2560 × 1440；314 timer ticks；315 paint calls；正常关闭 | 持续绘制且正常关闭 | Pass |
| 实际时长 | 50,194,500 tick | ≥ 50,000,000 tick | Pass |
| 编码帧 | 301 | > 0 | Pass |
| 回压丢帧 | 0 ppm | ≤ 5,000 ppm | Pass |
| 平均编码帧率 | 59.966 FPS | ≤ 60.5 FPS | Pass |
| 捕获间隔 P99 | 100,878 tick（10.088 ms） | ≤ 333,400 tick | Pass |
| 最大捕获间隔 | 101,189 tick（10.119 ms） | ≤ 1,000,000 tick | Pass |
| Capture Queue Peak | 2 / 4 | ≤ 4 | Pass |
| Input Queue Overflow | 0 | 0 | Pass |
| fMP4 | 16 fragments，0 trailing bytes | 可解析且无尾随字节 | Pass |
| 解码 | 2560 × 1440，PTS 单调；索引 Seek 可回读 | 可回读 | Pass |

证据：`.tmp/m4-stability-0e4f107337d44536b64b9c4a1d496eba.panzo/diagnostics/stability-acceptance.json`，刺激源输出：`.tmp/m4-stimulus-0e4f107337d44536b64b9c4a1d496eba.stdout.log`。

### 30 分钟首次运行与刺激源优化

首次正式运行生成的 Project `.tmp/m4-stability-093c2af58ccc4ba6848da42c278c393f.panzo` 已完整 Ready，单独执行 `stability-probe 1800` 通过全部录制门槛：

- 实际时长 18,000,366,500 tick，108,001 编码帧，平均 59.999 FPS。
- 回压丢帧 0；捕获间隔 P99 100,981 tick，最大 299,983 tick。
- Capture Queue Peak 3/4，Input Queue Overflow 0。
- 5,685 个 fMP4 fragments，0 trailing bytes。

该轮整体未通过的唯一原因是旧刺激源仅完成 72,177 次绘制，即约 39.88 Hz，低于脚本的 50 Hz 下限。根因经分步实验定位为 `WM_TIMER(16 ms)` 在当前会话被合并到约 25 ms，而不是录制或编码性能。

优化后的 60 秒并发回归项目为 `.tmp/m4-stability-d5c02c42cde94bea8b1ca67a923e3490.panzo`：

| 指标 | 实测 | 门槛 | 结果 |
|---|---:|---:|---|
| 动画有效刷新率 | 59.748 Hz | ≥ 50 Hz | Pass |
| 动画 Timer / Paint | 3,754 / 3,755 | ≥ 3,000 / ≥ 3,000 | Pass |
| 录制时长 | 600,178,333 tick | ≥ 600,000,000 tick | Pass |
| 平均编码帧率 | 59.998 FPS | ≤ 60.5 FPS | Pass |
| 回压丢帧 | 0 ppm | ≤ 5,000 ppm | Pass |
| 捕获间隔 P99 / 最大 | 100,972 / 103,881 tick | ≤ 333,400 / ≤ 1,000,000 tick | Pass |
| Capture Queue / Input Overflow | 2 / 4；0 | ≤ 4；0 | Pass |
| fMP4 | 190 fragments，0 trailing bytes | 完整 | Pass |

证据：`.tmp/m4-stability-d5c02c42cde94bea8b1ca67a923e3490.panzo/diagnostics/stability-acceptance.json`。

由于首次 30 分钟运行使用的是旧刺激源，正式 IT-STABILITY-001 仍需使用优化后构建重跑一次 30 分钟；无需重复已通过的 5/5 Recovery Matrix。

### 正式强制终止恢复矩阵

运行目录：`.tmp/m4-recovery-4553b09f8ac343b8832d8017cd18add8`

- 受控终止对象仅为脚本创建的录制子进程。
- 恢复结果：5/5 `recovered`。
- 五次恢复媒体时长：199,504,666、199,506,166、199,508,166、199,503,333、199,503,500 tick；均满足 20 秒注入窗口下最多 2 秒损失的门槛。
- 每个恢复媒体均包含 63 个完整 fMP4 fragments。
- 五次“导出覆盖源媒体”故障均被明确拒绝；每次源媒体 SHA-256 保持不变；均无 `*.panzo-part.mp4` 残留。

证据：`.tmp/m4-recovery-4553b09f8ac343b8832d8017cd18add8/recovery-acceptance.json`。

## 自动化验证

- `cargo test --workspace`：panzo-core 46/46、Golden 1/1、panzo-windows 22/22，通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- 两个 PowerShell 验收脚本通过 AST 语法检查。

## 正式 M4 验收命令

~~~powershell
.\scripts\verify-m4-stability.ps1 -DurationSeconds 1800
.\scripts\verify-m4-recovery.ps1 -Iterations 5 -CaptureSeconds 20
~~~

脚本默认自动显示内置全屏 60 Hz 持续动画。正式测试还必须记录 Windows Build、CPU、GPU/驱动、显示器、内存和构建 ID。

## 尚未满足的 M4 退出条件

- T0 正式 30 分钟持续动画录制。
- T1 不同 GPU 的第二台物理机兼容矩阵。
- 10 个 30 秒真实操作脚本、至少 3 名评价者和自然度汇总。

因此本报告不能作为 V0.1/M4 最终通过声明；它证明正式验收入口已经可重复执行并能生成机器可读证据。
