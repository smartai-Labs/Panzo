# Panzo V0.1 M3：Render Foundation 阶段报告

执行日期：2026-09-01（Asia/Shanghai）

本轮完成 M3 Playback、Render 与 Export 的底层 Foundation。共享帧求值、真实 H.264/fMP4 解码、D3D11 Camera/Cursor 合成、1080p60 H.264 导出及自动复验均已通过。M3 尚未整体关闭；技术规范 11.2 的 Preview Play / Pause 与窗口交互壳仍是下一增量。

## 实现摘要

| 能力 | 状态 | 说明 |
|---|---|---|
| VFR PTS 选帧 | 完成 | 选择不晚于 Source Time 的最近帧；首帧前保持首帧，尾帧后保持尾帧 |
| Cursor 求值 | 完成 | 位置线性插值，可见性不泄漏未来状态，经 Camera Transform 后固定为输出空间 32 px |
| Camera 几何 | 完成 | 正向/逆向 Transform 与 Source Rect 统一，四角 Clamp 不暴露源画面外区域 |
| H.264/fMP4 解码 | 完成 | Media Foundation Source Reader 输出 top-down packed BGRA8 与严格递增 PTS |
| 1080 可见尺寸 | 完成 | 使用 Advanced Video Processing 按原生可见尺寸协商，剔除 H.264 1920×1088 宏块填充行 |
| D3D11 Compositor | 完成 | Full-screen Shader 执行 Camera Crop / Scale，并绘制确定性 Panzo Cursor |
| Preview / Export 一致性 | 完成 | 两个消费者复用同一个 `FrameEvaluator` 和 `D3d11Compositor`，比较未压缩 BGRA SHA-256 |
| 1080p60 Export | 完成 | 60 CFR 整数 Tick、BGRA→NV12、Hardware H.264 High、无音频 fMP4 |
| 失败隔离 | 完成 | 同目录临时输出、成功后提交；禁止覆盖 Project Source Media |
| Preview 交互壳 | 待下一增量 | Play、Pause、时间显示、Camera 诊断与重新生成入口 |

## 验收结果

自动验收命令：

```powershell
.\scripts\verify-m3-foundation.ps1 `
  -ProjectRoot .tmp\m1-acceptance-49a75b0350cc4e79a2e31fbdfce3b618.panzo `
  -DurationSeconds 2 `
  -OutputPath .tmp\m3-foundation-2s.mp4
```

结果：

- `panzo-core`：41 个单元测试通过。
- Camera Golden：10 个 Fixture 全部匹配；更新测试保持 ignored。
- `panzo-windows`：16 个测试通过，包括四角 Camera GPU 合成无黑边。
- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `IT-RENDER-001`：Project Tick `19,990,000`，选择 Source PTS `19,883,944`；Preview / Export BGRA Hash 均为 `c9049b331fd00d7057651e52f3b76633a5643f07a2726c1d753cbdcda271ee1e`。
- `IT-EXPORT-001`：120 帧，Duration Tick `20,000,000`，1920×1080，7 个 fMP4 Fragment，Trailing Bytes=0。
- 导出回读：Media Foundation 完整解码 120 帧；首 PTS=0，末 PTS=`19,833,333`，严格递增。
- 编码器：`NVIDIA H.264 Encoder MFT`（hardware）。
- 2 秒短导出核心链耗时约 2.2 秒。
- `IT-EXPORT-002`：四个 Corner Camera State 的 GPU 输出逐像素保持源颜色，无黑边、无越界采样。

验收产物：`.tmp\m3-foundation-2s.mp4`。

## 当前结论

M3 Render / Export Foundation 可进入复验。完整 M3 的下一步是 Preview Controller 与窗口交互：Play、Pause、Seek、跳到开头、当前 Project Time、Camera State 诊断开关及 Camera Track 重新生成。
