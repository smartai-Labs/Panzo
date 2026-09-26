# Panzo V0.1 M3 Preview 阶段报告

执行日期：2026-09-01（Asia/Shanghai）

本增量完成技术规范 11.2 的 Preview 最小能力：Play、Pause、Seek、跳到开头、当前 Project Time、Camera State 诊断开关，以及 Camera Track 重新生成。预览与导出继续共用 `FrameEvaluator` 和 `D3d11Compositor`，没有引入第二套渲染语义。

## 实现范围

| 能力 | 实现 |
|---|---|
| 播放状态 | `PreviewController` 管理 Playing / Paused、Project Tick、结束自动暂停和末尾重播 |
| 随机 Seek | Media Foundation Source Reader 建立轻量 PTS 索引；Seek 到关键帧后向前解码并精确命中索引帧 |
| 项目预览会话 | `ProjectPreviewSession` 装载 Project、Camera、Cursor、源帧索引，并复用共享求值与 D3D11 合成 |
| Camera 诊断 | 显示当前 Camera center、scale、活动 Segment 和来源 Click ID |
| Camera 重生成 | 从 Click Track 与 Capture Geometry 重跑 `planner-v1`，通过 `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)` 原子替换 `camera.json` |
| Windows 外壳 | 独立的 Win32/GDI 验证窗口；不锁定 V1 的 UI 框架 |
| 操作入口 | 画面、Play/Pause、Home、可点击 Seek 条、Regenerate Camera、Diagnostics；同时支持 Space、Home、左右键、D、R、Esc |
| 防闪烁 | 暂停时 Timer 不触发无效重绘；播放画面通过 GDI 离屏双缓冲一次性提交；标题刷新限制为 10 Hz |

## 自动验收

命令：

```powershell
.\scripts\verify-m3-preview.ps1 `
  -ProjectRoot .tmp\m1-acceptance-49a75b0350cc4e79a2e31fbdfce3b618.panzo
```

验收覆盖：

1. 全工作区格式、单测、Golden、Clippy 与硬件 Foundation 探针。
2. 真实 H.264/fMP4 的完整 PTS 索引与 1 秒随机 Seek。
3. 0 秒 → 1 秒 Seek → Play/Advance → 回到 0 秒的确定性画面 Hash。
4. Camera 诊断开关。
5. Camera Track 连续两次重生成 Hash 一致，且没有遗留临时文件。
6. Windows 窗口真实创建、定时消息、自动播放、连续合成和正常关闭。
7. Paused 回归：Timer 保持运行，但 1.5 秒内只渲染、绘制首帧一次。

本机已验证的 60 秒项目结果：

- Source Frame Index：3562 帧。
- 1 秒请求选择 Source PTS：`9,941,972`。
- MF 从关键帧开始向前解码 60 帧后精确命中目标。
- Preview 首帧往返 Hash：`180dfadfac26811494827a13450549e514e56954c859272b4e5ddc226fd6f7ec`，一致。
- Camera 输入：28 条 Click Record，其中 13 条触发 Camera，生成 22 个 Segment。
- GUI 1.5 秒 Smoke：窗口创建成功、61 次 Timer Tick、62 次合成、Project Tick 推进到约 `15,200,000`、正常关闭。
- Camera 重生成最终 SHA-256：`24F25AFFA1F5AE1E4402EBEB98D40834E2E7CCC8657D9D5067DAC17679B4590B`。
- `panzo-core`：43 个单测通过；Camera Golden 全部匹配。
- `panzo-windows`：16 个测试通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。

## 手工复验

```powershell
cargo run -p panzo-app --bin panzo-cli -- preview `<project.panzo>`
```

验收者应确认：画面出现；Play/Pause 可切换；点击时间轴或使用左右键可以 Seek；Home 回到开头；D 显示 Camera 状态；R 重生成后画面仍可继续播放；Esc 正常关闭。

## 阶段结论

M3 Preview 增量已具备可复验实现。V0.1 的 Preview P0 主路径已经闭合；后续可进入最小应用入口整合（Open Last Project、Export MP4、Open Project Folder、Open Diagnostics）或按技术方案推进下一里程碑。
