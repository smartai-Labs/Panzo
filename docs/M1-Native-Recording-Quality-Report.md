# Panzo V0.1 原生录制画质修订报告

执行日期：2026-09-01（Asia/Shanghai）

## 问题

旧链路把 2560 × 1440 WGC 画面固定缩放为 1920 × 1080 后编码，Preview 又只合成 960 × 540 并放大到窗口。细小文字因此经历两次降采样，视觉上明显发虚。

## 修订

- 录制 Video Processor 的输出尺寸改为 Capture Content Size。
- fMP4 Writer 支持可配置宽、高、FPS 和目标码率。
- 原生录制码率以 1080p 20 Mbps 为基准按像素数同比例增加，上限 80 Mbps。
- 2560 × 1440 的目标码率为 35,555,556 bps。
- Export 保持技术规范要求的 1920 × 1080、60 FPS。
- Windows Preview 不再固定为 1920 × 1080，改为读取源视频实际尺寸并使用最高原生分辨率合成；最终显示仅按窗口客户区缩放。

## 验收结果

真实主显示器录制 5 秒：

- Capture Content Size：2560 × 1440。
- H.264 Encoder：NVIDIA H.264 Encoder MFT。
- Writer 配置：2560 × 1440、35,555,556 bps。
- 解码尺寸：2560 × 1440。
- 解码 BGRA 首帧：14,745,600 字节。
- 解码 PTS：严格递增。
- fMP4：16 个 Fragment，Trailing Bytes 为 0。
- 1440p Preview 播放 Smoke：按 2560 × 1440 原生尺寸合成并正常关闭。
- Paused Preview：Timer 运行但只渲染、绘制首帧一次。

验收项目：`.tmp/native-quality-review.panzo`。
