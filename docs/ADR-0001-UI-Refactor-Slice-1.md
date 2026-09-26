# ADR-0001：UI 重构第一阶段保留 Win32 Host 与 DXGI Preview

状态：Accepted for Slice 1；最终 UI 框架仍未锁定  
日期：2026-09-02（Asia/Shanghai）

> 2026-09-05 补充：本决策只适用于历史 Slice 1。新一轮按[当前项目计划](../PROJECT-PLAN.md) R0/R1 复核 UI 实现方式，并以[重设计规范](Recording-Editing-UI-UX-Spec.md)验收；保留 Win32 Host 不意味着当前视觉或交互已被接受，也不授权继续扩展固定坐标控件。

## 背景

旧 UI 虽然使用深色 Palette、圆角和共享控件状态，但真实窗口复验暴露了固定像素布局、中文文字溢出、时间尺信息重叠、手绘 GDI 图标粗糙和 HiDPI 缺失。它不能继续被认定为已通过的 macOS 风格界面。

Editor 的视频区域已经使用独立 DXGI Flip Swap Chain，并通过 `WS_CLIPCHILDREN` 与父窗口 Chrome 隔离。直接替换整个窗口框架会同时触碰已经通过的播放、Scrub、GPU Present 和窗口生命周期链路。

## 决策

第一阶段采用增量重构：

1. 保留 Win32 顶层窗口、消息循环和独立 DXGI Preview 子窗口，不改录制、解码、合成与导出核心。
2. UI 进程启用 Per-Monitor V2 DPI Awareness，Theme、窗口初始尺寸、最小尺寸、布局和字体均按当前 Monitor DPI 缩放，并处理 `WM_DPICHANGED`。
3. 图标统一使用 24×24 SVG 描述，通过 `resvg`/`tiny-skia` 栅格化并以 Alpha Blend 绘制；删除逐图标 GDI 线段拼装。
4. 字体使用支持版本均内置的 Segoe UI，并建立 Caption、Body、Button、Heading、Title 五个层级；不依赖缺席时可能回退成等宽系统字体的 Segoe UI Variable。
5. 单行文字默认使用实际字体测量、禁止快捷键前缀解析并在边界处显示省略号；Tooltip 宽度由实际文字宽度决定。
6. Timeline 的 Timecode、Ruler、Click Track、Camera Track、上下文工具和状态/统计信息使用不同布局行，任何摘要不得覆盖刻度尺。
7. 常用符号命令使用纯图标按钮和中文 Tooltip；删除、自动初稿、保存等需要明确语义的命令保留文字。
8. Inspector 改为全宽属性行，标题和值分列绘制；选中镜头卡逐行绘制，不允许依赖自动换行塞入固定高度。

## 未采用的方案

- 立即迁移 WinUI 3：能获得原生控件、DPI 与 UI Automation，但本阶段会扩大 Preview 嵌入和打包风险。
- 立即迁移 Slint/Tauri：更容易实现响应式视觉系统，但需要先验证原生 D3D 子窗口、输入延迟、时间线长列表和安装体积。
- 继续修补旧固定像素 GDI：改动最小，但无法消除文字测量、DPI 和图标资产层面的系统性问题。

## 后果与后续检查点

第一阶段能在不回退 GPU Preview 的情况下快速修正主要视觉缺陷，但 Win32/GDI Chrome 仍不是最终框架承诺。完成真实窗口视觉复验后，再以 D3D Preview 嵌入、1440p60 播放、Scrub 延迟、HiDPI、UI Automation、打包体积六项指标比较 Direct2D/DirectWrite、WinUI 3 与 Slint；若当前 Chrome 无法达到视觉或无障碍门槛，则进入完整框架迁移。
