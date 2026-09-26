# Panzo：Windows 智能运镜录屏软件总体方案

> 文档状态：历史产品愿景与长期技术参考（原基线 2026-08-31）
>
> 2026-09-05 起，当前目标收敛为“录制 + 基础编辑”，范围、优先级和交付条件以 [当前项目计划](./PROJECT-PLAN.md) 为准，UI/UX 重设计列为接下来首先处理的工作。本文中的自动化优先、音频、变速和长期路线不再自动构成当前版本承诺。
>
> 下文保留原始方案便于追溯；既有数据契约参考 [V0.1 技术方案](./V0.1-Technical-Spec.md) 和 [V0.2 工作台规范](./V0.2-Editor-Workbench-Spec.md)，新视频剪辑模型按当前计划版本化扩展。

## 1. 产品定位

开发一款面向 Windows 的智能录屏软件，核心参考 Screen Studio。

产品主要解决一个问题：

**用户正常录制电脑操作后，软件能够根据鼠标点击和操作行为，自动生成自然的 Zoom / Pan 运镜，并允许用户在录制结束后快速人工调整。**

软件不以取代 Premiere、DaVinci Resolve、剪映等专业视频编辑器为目标。

第一阶段定位：

> 智能录屏 + 自动运镜 + 轻量剪辑 + 高质量导出

主要使用场景：

- 软件教程
- 编程教程
- 产品演示
- AI 工具教程
- 网页操作演示
- 功能讲解
- 教学视频素材制作
- 社交媒体软件演示视频

第一阶段主用户不是所有视频创作者，而是：

> 在 Windows 上制作软件、编程、网页或 AI 工具教程，希望用最少后期时间得到专业运镜效果的个人创作者和小团队。

V0.1 的标准验证素材固定为浏览器、VS Code 和常见桌面软件中的 30 秒操作。先用统一场景验证核心体验，再扩大用户和内容类型。

核心目标：

> 用户录完屏以后，不需要手动给每一次操作打关键帧，就能够得到一段已经完成基本运镜和鼠标优化的高质量录屏素材。

---

# 2. 产品核心原则

整个产品围绕四个原则设计。

## 2.1 自动化优先

用户正常操作电脑即可。

软件自动完成：

- 鼠标轨迹记录
- 点击检测
- 自动 Zoom
- 自动 Pan
- 自动 Zoom Out
- 镜头平滑
- 连续操作合并

用户不需要录制过程中考虑运镜。

---

## 2.2 自动生成，但必须容易修改

自动算法不追求 100% 正确。

更重要的是：

> 自动结果错了以后，用户能不能在几秒内修改。

因此 Zoom、Pan 等都不是直接烧进视频，而是独立的 Camera Track 数据。

用户可以：

- 删除 Zoom
- 添加 Zoom
- 移动 Zoom
- 调整 Zoom 时间
- 调整缩放倍率
- 修改镜头中心
- 修改进入/退出速度
- 修改运动曲线

---

## 2.3 非破坏性编辑

原始录屏始终保留。

软件内部保存：

```text
原始屏幕视频
+
鼠标事件
+
点击事件
+
Camera Track
+
Audio Track
+
Speed Track
+
Visual Settings
```

最终视频是在导出阶段重新渲染生成。

这样用户后期修改 Zoom、鼠标大小、背景、速度，都不需要重新录制。

---

## 2.4 不做完整视频编辑器

V1 坚决不做：

- 多轨复杂剪辑
- 花字
- 大量转场
- 特效商城
- AI 数字人
- AI 配音
- 复杂字幕系统
- 绿幕
- 调色
- 素材管理
- 云端协作
- 完整音频工作站

这些以后有真实需求再增加。

---

## 2.5 时间、坐标和渲染必须统一

视频帧、鼠标、点击、窗口、音频和 Camera Track 必须建立在同一个单调时钟之上。

- 内部时间使用整数 Tick，禁止使用浮点秒作为持久化主时间。
- 从 V0.1 起定义 Project Time、Source Time 和统一 Time Mapping。
- 所有输入事件必须明确坐标空间，统一经过 Desktop、Capture Source、Content、Canvas、Output 的坐标转换。
- Preview 和 Export 必须调用同一套 Camera、Cursor 和合成计算。

变速功能可以晚于 V0.1 开放，但统一时间模型不能推迟到后期补做。

---

## 2.6 隐私与本地优先

默认所有录制、事件分析和渲染均在本机完成。

- V0.1 不记录键盘文本，也不采集音频。
- 后续键盘可视化只记录用户明确开启的快捷键语义，不保存普通文本输入。
- 项目日志不得记录密码、完整键盘事件或不必要的窗口正文。
- 如果未来增加云能力，必须独立授权，不能成为基础录制流程的前置条件。

---

# 3. 完整工作流程

整个用户流程保持极简。

```text
启动软件

    ↓

选择录制区域

    ↓

开始录制

    ↓

正常操作电脑

    ↓

软件同步记录：

屏幕
鼠标轨迹
鼠标点击
系统声音
麦克风
窗口信息

    ↓

停止录制

    ↓

自动生成 Camera Track

    ↓

进入编辑器

    ↓

检查 / 调整自动 Zoom

    ↓

简单剪辑 / 加速

    ↓

调整背景、鼠标、画布

    ↓

导出 MP4
```

---

# 4. 核心数据架构

这是整个项目最重要的底层设计之一。

录制一次以后，不生成单一 MP4 项目。

应该生成一个 Project。

Project 不是若干媒体文件的松散集合，而是一个可迁移、可恢复的数据包。必须包含：

- schemaVersion、projectId、应用版本和项目状态
- 统一 timebase 与录制会话起点
- 捕获源尺寸、DPI、旋转和坐标空间元数据
- 媒体文件、事件轨道和 Camera Track 的版本与校验信息
- 录制中的 append-only journal 和异常退出恢复标记
- 诊断统计，例如捕获帧、编码帧、丢帧和队列峰值

例如：

```text
project/
│
├── project.json
│
├── screen.mp4
├── microphone.wav
├── system_audio.wav
│
├── cursor.json
├── clicks.json
├── windows.json
│
├── camera.json
├── speed.json
│
└── thumbnail.jpg
```

---

# 5. Screen Track

保存原始屏幕录制。

建议录制：

- 原始屏幕分辨率
- 目标 FPS；每一帧仍保存真实 PTS，不能假定帧等间隔到达
- 尽量不包含后期鼠标效果
- 不包含 Zoom
- 不包含背景
- 不包含圆角和阴影

例如：

```text
screen.mp4
3840 × 2160
60 FPS
```

它是最原始的视频素材。

录制阶段必须采用可恢复的媒体写入策略，例如 fragmented MP4 加定期检查点。正常停止后才把临时媒体原子切换为正式 screen.mp4；异常退出时优先恢复已有素材，任何故障都不得自动删除 Project。

---

# 6. Cursor Track

鼠标必须作为独立数据记录。

例如：

```json
[
  {
    "timeTick": 124000000,
    "x": 1420,
    "y": 810,
    "coordinateSpace": "desktopPhysicalPx"
  },
  {
    "timeTick": 124160000,
    "x": 1428,
    "y": 813,
    "coordinateSpace": "desktopPhysicalPx"
  }
]
```

记录：

- 整数时间 Tick
- X / Y
- 坐标空间
- 捕获源几何版本
- 鼠标状态
- 是否可见

以后可以在后期重新绘制 Cursor。

这样就能支持：

- 鼠标大小
- Cursor Smoothing
- 鼠标样式
- 鼠标隐藏
- 点击动画
- 鼠标运动优化

---

# 7. Click Track

点击事件单独保存。

例如：

```json
{
  "timeTick": 124210000,
  "x": 1450,
  "y": 820,
  "coordinateSpace": "desktopPhysicalPx",
  "button": "left",
  "action": "down"
}
```

支持：

- Left Click
- Right Click
- Double Click

后续 Auto Zoom Engine 主要依赖这一轨道生成镜头。

---

# 8. Window Track

记录当前活动窗口。

例如：

```json
{
  "timeTick": 124000000,
  "process": "Code.exe",
  "title": "main.rs - Visual Studio Code",
  "rect": {
    "x": 120,
    "y": 80,
    "width": 1800,
    "height": 1000
  }
}
```

第一阶段主要用于：

- 避免 Zoom 超出窗口
- 判断当前操作区域
- 改进镜头构图

后续可以结合 Windows UI Automation 做 Semantic Zoom。

---

# 9. Camera Track

Camera Track 是整个软件最核心的数据。它必须是带版本的、确定性的时间函数，而不是一个孤立的 Zoom 列表。

例如：

```json
{
  "startTick": 120000000,
  "endTick": 154000000,

  "scale": 1.6,

  "target": {
    "centerX": 0.7552,
    "centerY": 0.7593
  },

  "enterDurationTick": 3500000,
  "exitDurationTick": 4500000,

  "easing": "smooth",
  "origin": "auto",
  "sourceClickIds": ["click-42"],
  "locked": false
}
```

它描述的不是：

> 视频放大多少。

而是：

> 虚拟摄影机在某一个时间观察屏幕的哪个位置。

统一使用 Virtual Camera 思维。

持久化模型还必须满足：

- Camera 中心使用 Capture Content 归一化坐标，而不是桌面绝对像素。
- Track 由按时间排序、互不重叠的 transition / hold segment 组成。
- 每个 segment 明确 from、to、easing、来源事件和生成器版本。
- 自动重新生成时不得覆盖 locked 或 userModified 的人工结果。
- 任意时间都能通过 RenderCamera(timeTick) 得到唯一 Camera State。

---

# 10. Auto Zoom Engine

这是整个软件最重要的核心算法。

第一阶段不需要 AI。

使用规则算法即可。

输入：

```text
鼠标位置
点击位置
点击间隔
鼠标速度
点击距离
当前 Camera 状态
窗口边界
```

输出：

```text
Camera Track
```

Auto Zoom Engine 是录制结束后的离线镜头规划器，可以观察完整事件序列。推荐流程：

```text
事件校验与坐标归一化

→

点击与操作聚类

→

候选镜头生成

→

连续操作合并

→

Pan / Reframe / Zoom Out 决策

→

生成 Camera Segment

→

边界 Clamp 与运动曲线平滑
```

同一组输入、参数版本和生成器版本必须得到完全相同的 Camera Track，方便测试、回归和重新生成。

---

# 11. 基础 Auto Zoom 规则

最简单规则：

```text
检测到 Click

↓

提前约 0.2～0.5 秒开始 Zoom

↓

以点击区域作为 Focus

↓

Zoom 到约 1.4～1.8x

↓

保持一段时间

↓

没有进一步操作

↓

Zoom Out
```

具体数值以后通过真实使用不断调整。

---

# 12. 连续点击处理

这是决定运镜是否高级的关键。

错误方式：

```text
点击 A

Zoom In
Zoom Out

点击 B

Zoom In
Zoom Out
```

这样会让画面不断呼吸，非常晕。

正确方式：

```text
点击 A

Zoom In

↓

检测到短时间内点击 B

↓

判断 A 与 B 距离

↓

距离较近：

保持 Zoom
+
Pan 到 B
```

最终：

```text
Zoom In
     ↓
     Pan
     ↓
     Pan
     ↓
Zoom Out
```

---

# 13. Camera 状态机

Camera 仍然需要明确状态，但状态机不是 Auto Zoom Engine 的全部。

离线规划器负责利用未来事件生成完整 Camera Track；播放和渲染阶段再根据时间进入相应状态。

例如：

```text
IDLE

↓

ZOOMING_IN

↓

FOCUSED

↓

PANNING

↓

ZOOMING_OUT

↓

IDLE
```

不能退化成简单的：

```text
Click → Zoom
```

这样既保留清晰的运行时状态，又不会让后处理算法失去向前观察和全局合并事件的能力。

---

# 14. Camera 自动判断参数

第一阶段重点研究几个变量：

### 点击间隔

例如：

```text
< 1.5 秒
```

认为属于连续操作。

---

### 点击距离

例如：

```text
Distance(A,B)
```

较近：

```text
Pan
```

很远：

```text
Zoom Out → Reframe → Zoom In
```

---

### 鼠标停留

如果鼠标长时间停留：

```text
延长 Focus
```

---

### 屏幕边缘

点击靠近屏幕右下角时，不能简单把点击点放到屏幕中心。

需要保证画面不会出现黑边。

因此 Camera Position 必须做 Clamp。

---

# 15. Camera Motion

镜头运动不能使用 Linear。

否则会非常像 PPT。

需要统一 Motion Engine。

第一阶段可以提供：

```text
Smooth
Fast
Soft
```

内部对应：

```text
EaseInOut
EaseOut
Spring-like curve
```

用户不需要看到复杂动画参数。

高级设置以后再开放。

---

# 16. 手动 Zoom

用户可以主动创建 Zoom。

操作方式：

```text
时间线上选择区域

↓

Add Zoom
```

然后 Preview 出现一个焦点框。

用户直接拖动：

```text
┌──────────────────────────┐
│                          │
│       ┌──────────┐       │
│       │     ×    │       │
│       └──────────┘       │
│                          │
└──────────────────────────┘
```

调整 Focus。

右侧属性：

```text
Zoom

Scale           1.6x

Duration        2.4s

Enter           0.35s

Exit            0.45s

Motion          Smooth
```

---

# 17. Timeline

V1 时间线不要做复杂。

保持五个核心 Track。

```text
Camera
──────[ Zoom ]────────[ Zoom ]────────

Cursor
──────────────────────────────────────

Video
██████████████████████████████████████

Audio
▂▅▇▃▆▂▅▇▃▂▆▇▅▂▃▅▆▂▇▅

Speed
────1x──────[4x]────────1x────────────
```

---

# 18. 基础剪辑

V1 只支持：

### Trim

修改视频开始和结束。

### Split

切开视频。

### Delete

删除某一段。

### Speed

修改片段速度。

不加入传统 NLE 的复杂工具。

---

# 19. Speed Track

这是教程录制非常重要的能力。

例如：

```text
正常讲解
1x

安装过程
4x

等待加载
8x

继续讲解
1x
```

提供：

```text
0.5x
1x
1.5x
2x
4x
8x
```

以后再考虑：

```text
Speed Ramp
```

---

# 20. Time Mapping

Time Mapping 是从 V0.1 开始存在的底层能力，而不是加入 Speed Track 时才建立的补丁。

统一时间基准：

```text
1 second = 10,000,000 ticks

Session QPC

→

Source Time

→

Project Time
```

所有持久化时间使用有符号 64 位整数 Tick。变速不能只改变视频。

它必须影响：

```text
Screen
Cursor
Click
Camera
Microphone
System Audio
```

所以项目内部必须有统一时间映射：

```text
Project Time

↓

Time Mapping

↓

Source Time
```

这样 Speed Track 才不会导致鼠标和 Zoom 错位。

V0.1 中 Project Time 与 Source Time 是恒等映射；V0.4 只是在既有模型上增加分段映射和 Speed Track。

---

# 21. Cursor System

Cursor 是第二个非常重要的视觉能力。

V1 支持：

### Cursor Smoothing

将原始轨迹：

```text
·  ·   · ·    ·
```

重新计算成：

```text
──────────────→
```

但不能改变实际点击位置。

---

### Cursor Size

例如：

```text
100%
125%
150%
175%
200%
```

---

### Hide Idle Cursor

例如：

```text
鼠标静止超过 2 秒

↓

Fade Out
```

鼠标重新移动：

```text
Fade In
```

---

### Click Animation

点击时：

```text
○
◎
●
```

轻微提示即可。

不能太花哨。

---

# 22. Visual Styling

录屏画面可以放入 Canvas。

例如：

```text
┌────────────────────────────┐
│                            │
│    ┌──────────────────┐    │
│    │                  │    │
│    │   Screen Video   │    │
│    │                  │    │
│    └──────────────────┘    │
│                            │
└────────────────────────────┘
```

支持：

### Background

- Solid Color
- Gradient
- 简单 Wallpaper

### Padding

例如：

```text
0
24
48
72
96
```

### Corner Radius

### Shadow

这些功能成本不高，但对最终视频质感提升非常明显。

---

# 23. Canvas

支持：

```text
16:9
4:3
1:1
9:16
```

例如：

```text
1920 × 1080

1080 × 1920

1080 × 1080
```

Camera Engine 基于最终 Canvas 重新计算画面位置。

这样同一个项目可以导出：

```text
YouTube 横屏

+

短视频竖屏
```

后续再重点优化智能 Reframe。

---

# 24. Recorder 页面

Recorder 第一版保持极简。

```text
┌───────────────────────────┐
│                           │
│      Recording Area       │
│                           │
└───────────────────────────┘

Screen
Display 1

Microphone
Rode NT USB

System Audio
ON

Cursor
ON

        ● Record
```

支持：

```text
Full Screen
Window
Region
```

---

# 25. Editor 页面

Editor 是整个产品核心。

建议布局：

```text
┌─────────────┬───────────────────────────────┬──────────────┐
│             │                               │              │
│   Tools     │           Preview             │  Properties  │
│             │                               │              │
│             │                               │              │
├─────────────┴───────────────────────────────┴──────────────┤
│                                                            │
│ Camera   ───────[Zoom]──────────[Zoom]────────────────      │
│ Cursor   ─────────────────────────────────────────────      │
│ Video    ████████████████████████████████████████████      │
│ Audio    ▃▅▇▃▂▅▆▃▇▅▃▂▅▇▃▅▆▂▅▇▃                        │
│ Speed    ───────────[4×]──────────────────────────────      │
│                                                            │
└────────────────────────────────────────────────────────────┘
```

---

# 26. Properties Panel

根据选中对象变化。

选中 Camera：

```text
Camera

Zoom        1.6x

Focus       Cursor

Motion      Smooth

Enter       0.35s

Exit        0.45s
```

选中 Canvas：

```text
Canvas

Ratio       16:9

Background

Padding

Radius

Shadow
```

选中 Cursor：

```text
Cursor

Size

Smoothness

Click Effect

Hide Idle
```

---

# 27. Export

V1 支持：

```text
MP4

H.264

1080p
1440p
4K

30 FPS
60 FPS
```

质量：

```text
Medium
High
Very High
```

第一阶段不需要给用户暴露大量编码器参数。

---

# 28. Render Pipeline

Preview 和 Export 必须共用一套渲染逻辑。

核心接口思想：

```text
RenderFrame(projectTimeTick)
```

输入：

```text
Project
Time
```

输出：

```text
Frame
```

Preview：

```text
RenderFrame(t)

↓

GPU

↓

Preview Window
```

Export：

```text
RenderFrame(t)

↓

Encoder

↓

MP4
```

这样可以保证：

> 编辑器里看到什么，导出就是什么。

这是非常重要的架构原则。

Preview 和 Export 共用的不只是 Camera 数学，还包括：

- Source Time 取样规则
- Camera 插值和 Clamp
- Cursor 坐标变换与绘制尺寸
- 色彩空间、缩放滤镜和图层顺序

V0.1 明确只支持 8-bit SDR，避免在核心验证阶段同时引入 HDR 捕获、色调映射和多色彩空间差异。

---

# 29. Windows 技术架构

核心使用：

```text
Rust
```

Rust 负责：

```text
Recording Core

Capture

Audio

Input Events

Project

Timeline

Camera Engine

Renderer

Encoder
```

V0.1 固定技术链路：

```text
Rust stable-msvc

Windows Graphics Capture

Direct3D 11

Media Foundation

windows-rs
```

先完成 Windows 原生低拷贝链路，再根据真实需求评估跨平台渲染抽象。

---

# 30. 屏幕捕获

首选：

```text
Windows Graphics Capture
```

用于：

- Monitor Capture
- Window Capture

V0.1 只实现单显示器 Monitor Capture，并关闭捕获画面中的系统鼠标。Window Capture 在 V0.2 加入，Region Capture 在稳定的显示器捕获基础上通过 GPU Crop 实现。

在 WGC 兼容性数据证明有必要后再考虑：

```text
Desktop Duplication API
```

作为兼容方案。V0.1 不同时维护两套捕获后端。

---

# 31. 音频

Windows 使用：

```text
WASAPI
```

系统声音：

```text
WASAPI Loopback
```

麦克风：

```text
WASAPI Capture
```

系统音频和麦克风建议独立保存。

音频不进入 V0.1。加入音频时必须与视频共享统一时钟，并实现长时间录制漂移测试。

---

# 32. 鼠标和输入

使用 Windows Input Hooks 记录：

```text
Mouse Move

Mouse Down

Mouse Up

```

输入回调只做时间戳和事件入队，不执行文件写入、图像处理或其他阻塞操作。

V0.1 不记录 Keyboard。后续快捷键可视化必须由用户显式开启，只保存组合键语义，不保存普通文本输入。

以后可以做：

```text
Ctrl + C

Ctrl + Shift + P
```

快捷键提示。

---

# 33. 视频编码

V0.1 使用：

```text
Media Foundation
```

后续在多格式导出、兼容性或编码质量确有需要时再评估：

```text
FFmpeg
```

第一阶段重点是：

- H.264
- GPU Hardware Encoder
- 稳定
- 低资源占用

不需要一开始支持大量格式。

---

# 34. GPU Renderer

Renderer 独立设计，但 V0.1 后端固定为 Direct3D 11。

V0.1 之后可以评估：

```text
wgpu
```

V0.1 直接使用：

```text
Direct3D 11
```

渲染内容：

```text
Screen

↓

Camera Transform

↓

Cursor

↓

Click Effect

↓

Background

↓

Shadow

↓

Canvas
```

---

# 35. UI 技术

UI 层和核心层必须分离。

结构：

```text
Windows UI

↓

Application Layer

↓

Recording / Project / Rendering Core
```

UI 可以根据开发体验选择：

```text
Tauri

Slint

WinUI 3

Flutter
```

但不要让 UI 框架决定底层项目架构。

核心 Engine 应该尽量独立。

V0.1 只做最小验证界面，不在尚未验证 4K60 预览嵌入前锁定最终 UI 框架。V0.2 开始前用技术 Spike 比较候选框架的原生 D3D Preview、时间线交互、HiDPI、可访问性和打包能力，并用 ADR 记录决定。

---

# 36. 推荐模块结构

例如：

```text
src/

capture/
    screen
    window
    audio

input/
    mouse
    keyboard

project/
    project
    serialization

timeline/
    timeline
    time_mapping
    speed

camera/
    auto_zoom
    camera_track
    motion

cursor/
    cursor_track
    smoothing

render/
    renderer
    effects
    canvas

encoder/
    h264
    audio

app/
    commands
    state
```

---

# 37. V0.1

第一阶段不要直接完成整个产品。

先验证最核心技术。

只实现：

```text
单显示器录屏

+

记录 Cursor

+

记录 Click

+

播放录屏

+

根据 Click 离线生成 Zoom / Pan / Zoom Out

+

使用统一渲染器预览和导出验证视频
```

甚至可以暂时没有完整 UI。

核心验证：

> 自动 Camera Track 这个思路是否成立。

V0.1 不包含音频、Window / Region Capture、完整编辑器、手动 Camera 编辑、Speed、样式和 HDR。具体数据协议、异常恢复、测试矩阵和量化退出条件见 [V0.1 技术方案与验收规范](./V0.1-Technical-Spec.md)。

---

# 38. V0.2

加入：

```text
Window Capture

Camera Timeline

手动修改 Zoom

手动新增 Zoom

修改倍率

修改焦点

删除 Zoom
```

此阶段基本验证核心产品体验。

---

# 39. V0.3

加入：

```text
Cursor Smoothing

Cursor Size

Click Animation

Hide Idle Cursor

Background

Padding

Radius

Shadow
```

此时视觉效果开始接近 Screen Studio。

---

# 40. V0.4

加入基础剪辑：

```text
Trim

Split

Delete

Speed
```

并在 V0.1 已有的统一 Time Mapping 上增加分段速度映射。

---

# 41. V0.5 / Release Candidate

加入发布前完整链路：

```text
Region Capture

System Audio

Microphone

音视频漂移校正

导出预设

项目迁移

安装、升级与崩溃恢复
```

此阶段以稳定性、兼容性和真实教程生产为目标，不再扩展与核心体验无关的功能。

---

# 42. V1.0

完整 V1：

### Recording

- Screen
- Window
- Region
- System Audio
- Microphone
- Cursor
- Click

### Smart Camera

- Auto Zoom
- Auto Pan
- Auto Zoom Out
- Continuous Click Merge
- Edge Handling
- Camera Smoothing

### Manual Camera

- Add Zoom
- Delete Zoom
- Move Zoom
- Resize Duration
- Scale
- Focus
- Motion

### Cursor

- Smoothing
- Size
- Click Animation
- Hide Idle

### Edit

- Trim
- Split
- Delete
- Speed

### Style

- Canvas
- Background
- Padding
- Corner Radius
- Shadow

### Export

- MP4
- H.264
- 1080p
- 1440p
- 4K
- 30 / 60 FPS

---

# 43. V2 可以考虑的方向

V1 真正投入使用以后，再根据实际需求增加。

可能包括：

### Semantic Zoom

通过 Windows UI Automation 理解用户点击的实际 UI 元素。

例如：

```text
用户点击 VS Code Terminal

↓

识别 Terminal Panel

↓

不是围绕鼠标放大

↓

而是自动框住整个 Terminal
```

这是未来非常值得发展的能力。

---

### Keyboard Visualization

显示：

```text
Ctrl + C

Ctrl + V

Ctrl + Shift + P
```

---

### Annotation

简单支持：

```text
Arrow

Rectangle

Spotlight

Blur
```

---

### Smart Speed

自动检测：

```text
等待加载

连续输入

长时间无操作
```

并建议：

```text
2x

4x

8x
```

---

### Auto Cut

以后可以增加：

```text
长时间静止检测

错误操作删除

停顿剪辑
```

但不是第一阶段重点。

---

# 44. 产品明确不应该变成什么

不要变成：

```text
OBS
```

它不是直播软件。

不要变成：

```text
Premiere
```

它不是专业 NLE。

不要变成：

```text
剪映
```

它不是完整内容创作平台。

不要一开始变成：

```text
FocuSee 功能全集
```

第一阶段功能越多，越容易失去核心体验。

---

# 45. 真正的产品竞争力

这个项目真正值得长期打磨的东西只有几个。

第一：

**Auto Zoom 是否自然。**

第二：

**Cursor 是否漂亮。**

第三：

**Camera Motion 是否舒服。**

第四：

**自动结果是否容易修改。**

第五：

**录制是否稳定。**

只要这五点做好，即使功能数量非常少，产品仍然具有明显价值。

---

# 46. 产品的一句话定义

最终可以把这个软件定义为：

> 一款面向 Windows 的智能教程录屏工具，通过记录鼠标与操作行为自动生成平滑的 Zoom 和 Pan 运镜，并允许用户在时间线上快速调整镜头、鼠标和视频节奏，让普通屏幕录制直接变成高质量教程素材。

---

# 47. 第一阶段真正应该追求的效果

不是：

> 功能和 Screen Studio 一样多。

而是：

> 同样录制一段 30 秒的软件操作，自己的软件生成的自动运镜已经足够自然，让自己愿意真的拿它制作教程。

只要做到这一点，第一阶段就已经成功。

之后所有功能，都应该来自真实使用过程中遇到的问题，而不是来自竞品功能列表。
