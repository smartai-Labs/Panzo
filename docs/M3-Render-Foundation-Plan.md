# Panzo V0.1 M3：Render Foundation 开发与验收计划

## 阶段目标

实现技术方案第 11 章的共享 `RenderFrame(projectTimeTick, outputDescriptor)` 链路，使 Preview 和 Export 对同一 Project Time 使用完全相同的 Source PTS 选帧、Camera、Cursor、几何变换、缩放和图层顺序。

## Foundation 实现边界

1. `panzo-core`：Source PTS 索引、Cursor 插值、Camera 评估、输出描述符和共享 Frame Evaluation。
2. `panzo-windows`：H.264/fMP4 解码、D3D11 BGRA Shader 合成、固定 32 px Panzo Cursor、无压缩帧 Hash。
3. Export：以 60 FPS CFR 遍历 Project Time，复用同一个 Frame Evaluator 和 Compositor，输出 1080p H.264 MP4。
4. Preview Foundation 提供可 Seek 的同帧验证入口；Play / Pause 和窗口交互壳在后续 M3 增量实现。

## 明确约束

- Source Video 是 VFR；选择 PTS 不晚于 Source Time 的最近帧，没有更晚帧时保持最后一帧。
- Camera 与 Cursor 均在共享评估层计算，Preview / Export 不得复制或简化逻辑。
- Cursor 经过 Camera Transform，但图形大小固定为输出空间 32 px。
- 输出 BGRA8 SDR；Export 唯一预设为 1920×1080、60 FPS、H.264、无音频。
- 不覆盖原始录制媒体；先写同目录临时文件，失败时只清理未完成导出，成功后再提交目标文件。

## Foundation 退出条件

- `UT-RENDER-001`：VFR PTS 的首帧、中间帧、尾帧选择正确。
- `UT-RENDER-002`：Cursor 位置按时间线性插值，可见性采用不泄漏未来状态的规则。
- `UT-RENDER-003`：Camera Transform 与逆变换、四角 Clamp 正确。
- `IT-RENDER-001`：Preview 与 Export 路径在相同 Tick、相同 Output Descriptor 下产生相同 BGRA Hash。
- `IT-EXPORT-001`：短样例导出可完整回读，60 FPS CFR，时长误差不超过一帧。
- `IT-EXPORT-002`：四角 Camera 集成测试无黑边、无越界。

Foundation 通过不等于 M3 整体结束；M3 仍需补齐技术规范 11.2 的 Preview Play、Pause、Seek、时间显示和诊断交互。
