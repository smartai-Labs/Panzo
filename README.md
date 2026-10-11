# Panzo

Panzo 是面向 Windows 的录制与基础编辑应用，当前沿用 Windows 11 技术基线。目标是让用户在一个应用内完成录制、流畅预览、基础剪辑、镜头与背景调整、保存和清晰导出；自动运镜是可选辅助。

RE-MVP 候选已接通录制、视频裁剪 / 分割 / 删除闭合、镜头与背景、统一撤销、原子保存和编辑器内原始尺寸 / 1080p60 导出。**候选实现不等于整体验收通过**；最新结论见 [统一验收报告](Acceptance-Report.md)。旧技术规范保留为历史基线。

## 当前候选与启动

2026-10-11 已清理历史构建，`target` 仅保留当前版本 `desktop-v14-library-popup-fix`、`debug` 和 `release`。历史验收文档中的旧构建路径仅作记录，程序统一通过下方入口启动。

最新 [桌面 UI 商业化就绪审查](docs/UI-Commercial-Readiness-Audit-2026-10-09.md) 已完成：基于 v12.6 的真实窗口、流程回归和响应测量，列明性能、组件、布局、动效与验收缺口，以及整改顺序。该 UI 审查的测量基线为 v12.6。

双击 [Start-Panzo.cmd](Start-Panzo.cmd) 启动当前版本，或直接打开 [桌面工作台 v14 · 素材库弹窗修正版](target/desktop-v14-library-popup-fix/release/panzo.exe)。素材先导入，通过中央标签切换预览，再拖入时间轴；录音结束只加入素材库。支持新增／删除轨道、片段自由移动、同类型跨轨拖动、边缘吸附（Alt 暂停）、Esc 取消和边缘自动滚动。播放及导出时长取时间轴片段的最晚结束位置，空画面区为黑屏，音频继续播放。素材被引用时禁止删除。右侧页签为画面、视频、声音、字幕。面板标题为与正文同字体的 14 px 普通字重，正文 13 px。视频页精简重复片段信息与入出点输入，增加片段缩放；同组按钮采用深色分段样式，背景类型回归背景分组并正确作用于预览及导出。像素为整数，百分比与速度为一位小数，详见 [设置面板与背景修复](docs/Inspector-Settings-2026-10-10.md)。按钮首次点击即可执行，移除按钮焦点下划线；视频“自由”直接进入预览裁剪，可拖动边角和取景范围。详见 [本轮修复与验证记录](docs/Interaction-Crop-Fixes-2026-10-10.md) 与 [此前录制视频修复](docs/Recording-Video-Fixes-2026-10-10.md)。保留此前的 [面板拖动绘制修复](docs/Panel-Drag-Rendering-2026-10-10.md) 和 [空白工作台调整](docs/Panels-And-Empty-Workspace-2026-10-10.md)。

播放按钮左侧新增“1/4、1/2、原画”预览清晰度，默认原画并记忆选择，视频和素材共用此设置；预览质量不改变导出质量。导出面板按名称、输出位置、分辨率、帧率、格式排列，高级设置在面板内展开。详见 [预览清晰度与导出面板验证](docs/Preview-Quality-And-Export-2026-10-10.md)。

此前合并完成时间轴选框、素材拖动卡片、旁白倒计时、声音／字幕面板对齐、即时修改、同轨重叠约束和波形显示。视频与素材预览现在共用一个播放器，切换按钮与播放控件同一行，使用统一深色分段容器；字幕样式只在选中字幕后直接显示，并逐条保存。详见 [修复与共享播放器验证](docs/Media-Polish-And-Shared-Preview-2026-10-10.md)。

此前的 [颜色面板修复](docs/Image-Color-Popup-Fix-2026-10-09.md) 与 [图片页原型对齐](docs/Image-Workspace-v14-2026-10-09.md) 保留：数值框悬浮或聚焦显示增减箭头，数字、箭头和单位垂直居中；颜色选择即时修改，点击面板外关闭后更新最近使用颜色；多显示器定位修复保留。

本轮按 v14 原型完成原生工作台布局：统一三层顶部、项目素材栏、片段属性、声音与字幕、固定高度轨道、图片工具属性和三栏素材库。详见 [原生改造与验证记录](docs/Workbench-v14-Native-2026-10-09.md)。v13 的重绘、任务状态、插件隔离及辅助功能基础继续保留。

正式目标仍为 Windows 11；当前机器的 Windows 10 回归不能替代目标系统、混合 DPI、输入法和 Narrator 实机验收。商业发布门槛及复现入口见 [UI 发布验收矩阵](docs/UI-Release-Gates.md)。录音与 AI 字幕继续通过插件执行，四个属性标签共用一个面板，轨道始终保持全宽。预览支持 Ctrl＋滚轮缩放、拖动平移和居中适配。独立插件的任务隔离与兼容性见 [v12.7 插件生命周期修复](docs/Plugin-Lifecycle-v12.7-2026-10-09.md)。

已安装的插件无需重装。首次使用录音与 AI 字幕时，在顶部「插件」安装同目录的 [媒体插件包](target/desktop-v14-library-popup-fix/release/panzo-media-tools.panzoplugin)，然后在「设置 → AI 字幕」配置自己的 API 地址、语音模型和 API Key。字幕面板提供“添加字幕”和“AI 自动字幕”，普通字幕也可导入 / 导出 SRT。保留 v11.1 的 [插件列表刷新修复](docs/Plugin-List-Refresh-2026-10-09.md) 与 v11.2 的 [轨道高度限制](docs/Timeline-Track-Height-2026-10-09.md)；录音与模型通信仍由插件承担。

v10 快捷键设置功能已合入当前版本。快捷键仍在「设置 → 快捷键」长期保存；默认全局截图 Ctrl+Alt+Shift+F9，区域录屏 / 停止 Ctrl+Alt+Shift+F10。新声音、字幕入口默认不占用快捷键。详见 [快捷键说明](docs/Custom-Shortcuts-2026-10-09.md)。

顶部「插件」弹窗支持内置 OCR / 多模态 / 批处理的启停，以及本地独立插件安装、更新、回退和卸载。可以导入 [灰度与反色示例插件](dist/plugins/panzo-image-filters-0.1.0-windows-x64.panzoplugin)，然后在图片工具栏的插件入口运行。详见 [本阶段说明](docs/Plugin-Stage-A-2026-10-08.md) 和 [插件开发文档](docs/Plugin-Development.md)。OCR、录屏等尚随主程序打包，精简版 / 全功能组合包将在后续拆包后提供。

[多模态识图配置说明](docs/Multimodal-Vision-2026-10-08.md)、[区域选择闪屏修复](docs/Selection-Flicker-Fix-2026-10-08.md) 和 [第四阶段功能说明](docs/Integrated-Capture-Stage-D-2026-10-08.md) 中的底层能力均保留；历史说明里的独立窗口入口已逐步迁入当前工作区。API Key 继续由用户配置并在本机加密保存。

[第三阶段说明](docs/Integrated-Capture-Stage-C-2026-10-08.md)、[第二阶段说明](docs/Integrated-Capture-Stage-B-2026-10-08.md)、[第一阶段说明](docs/Integrated-Capture-Stage-A-2026-10-08.md) 保留作对照。

开发启动：`cargo run -p panzo-app --bin panzo`。

9 月 27 日候选的改动保留在 [历史说明](docs/UI-Editor-Feedback-2026-09-27.md)。

## 新工作区 UI 设计

**工作台布局 v14 设计基线**：[交互原型](docs/prototypes/workbench-v14/index.html) · [问题、布局规格与实施方案](docs/UI-Layout-Review-v14-2026-10-09.md) · [使用说明与截图](docs/prototypes/workbench-v14/README.md)。保留现有主题，重排任务层级、上下文工具、统一属性栏、轨道和素材接入；原型已用于当前 v14 原生开发，原型中的模拟流程与实际应用须分别验证；原生结果见上方改造记录。预览：`powershell -ExecutionPolicy Bypass -File scripts/open-workbench-v14-preview.ps1`。

历史设计基线为 **桌面工作台 v3**：[可点击设计稿](docs/prototypes/desktop-v3/index.html) · [设计与原生改造规格](docs/UI-Workspace-Redesign-2026-10-08.md)。采用顶部工作区、窄工具栏、文档标签、画布和可收起属性面板，覆盖图片、录制、素材、插件和设置。v2 因过于接近网页应用已弃用。当前原生界面已按 2026-10-09 的问题筛查继续修正，以本页顶部列出的当前原生候选为准；此 HTML 入口仅保留作设计对照。独立窗口预览：`powershell -ExecutionPolicy Bypass -File scripts/open-desktop-v3-preview.ps1`。

## 当前开发安排

按用户要求连续实施，不再逐阶段暂停；以下任务已进入候选统一回归，详见 [实现说明](docs/RE-MVP-Implementation.md)。

1. 桌面工作台 v3 已接入原生候选，已检查真实窗口的深浅主题、960×720 布局和持久工作流；多显示器 DPI 与完整读屏矩阵仍待专项验收。
2. 已接入后台解码、短 GOP 代理、播放预读和有界调度；完整物理输入矩阵待验收。
3. 已按设计重排工作区、图片工具、录制准备、素材列表、插件和偏好设置；视频画布、时间线及属性区沿用实际 GPU 预览与编辑后端。当前已取得真实窗口截图，完整物理输入矩阵仍待专项验收。
4. 已补齐视频裁头尾、分割、删除闭合、效果映射及统一撤销。
5. 已接通编辑器原始尺寸 / 1080p 导出与录后衔接；完整 GUI 任务评审待完成。

规范入口：[UI/UX 重设计](docs/Recording-Editing-UI-UX-Spec.md)、[验收计划](docs/Recording-Editing-Acceptance-Plan.md)、[已知问题](Known-Issues.md)。历史 RE-MVP 的无音频范围已由集成 C 阶段扩展；自动镜头真人舒适度及跨 GPU 专项仍待验收，不记通过。以下功能/探针说明保留现有实现信息，新范围与开发顺序以上述计划为准。

R0 首轮已交付：[界面交互原型](docs/prototypes/re-mvp/index.html)（本地双击打开，不是新版EXE）、[评审说明](docs/prototypes/re-mvp/README.md)、[性能基线与交付报告](docs/RE-MVP-R0-Report.md)。原型不录屏、不写工程、不导出文件；原型视觉 / 布局已确认，不能替代真实窗口与性能验收。

## 开发环境

- Windows 11 22H2 或更高版本，x64。
- Visual Studio Build Tools 2022，安装 MSVC 与 Windows SDK。
- Rust 工具链由 `rust-toolchain.toml` 固定。
- 可选：FFmpeg `ffprobe`，仅用于额外检查 MP4，不参与构建。

## 构建与自动验证

```powershell
cargo build --workspace
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

统一验证脚本：

```powershell
.\scripts\verify.ps1
.\scripts\verify.ps1 -IncludeMediaProbe -IncludeInputProbe
```

需要交互式 Windows 桌面时，运行完整 M1 录制验收：

```powershell
.\scripts\verify-m1.ps1 -DurationSeconds 60
```

使用已通过 M1 的 Project 运行 M3 Render Foundation 一键验收：

```powershell
.\scripts\verify-m3-foundation.ps1 `
  -ProjectRoot .tmp\your-recording.panzo `
  -DurationSeconds 2
```

启动可交互 Recorder Window，或运行自动化 UI 录制验收：

```powershell
cargo run -p panzo-app --bin panzo
.\scripts\verify-recorder-ui.ps1 -RecordingMilliseconds 2000
```

## 技术与验收探针

```powershell
# 汇总 QPC、WGC、D3D11、BGRA→NV12 和硬件 H.264 能力
cargo run -p panzo-app --bin panzo-cli

# 读取 WGC 帧；源视频光标必须关闭
cargo run -p panzo-app --bin panzo-cli -- capture-probe 10

# 枚举硬件 H.264 MFT
cargo run -p panzo-app --bin panzo-cli -- encoder-probe

# 执行 1920×1080 D3D11 VideoProcessorBlt
cargo run -p panzo-app --bin panzo-cli -- video-probe

# 写入 1080p60 H.264 fragmented MP4
cargo run -p panzo-app --bin panzo-cli -- fmp4-probe .tmp\probe.part.mp4 2

# 运行 WH_MOUSE_LL 与 120 Hz Cursor Sampler
cargo run -p panzo-app --bin panzo-cli -- input-probe 2

# 端到端录制为可恢复的 Panzo Project
cargo run -p panzo-app --bin panzo-cli -- record-probe .tmp\recording.panzo 5

# 枚举可捕获应用窗口，并按标题录制指定窗口
cargo run -p panzo-app --bin panzo-cli -- window-list
cargo run -p panzo-app --bin panzo-cli -- window-record-probe .tmp\window.panzo "窗口标题" 5

# 启动真实动画窗口并执行 Window Capture 一键验收
.\scripts\verify-window-capture.ps1 -DurationSeconds 3

# 对已完成 Project 执行 M4 稳定性阈值判定；默认要求 30 分钟
cargo run -p panzo-app --bin panzo-cli -- stability-probe .tmp\recording.panzo 1800

# 单独打开 M4 全屏 60 Hz 动画刺激源
cargo run -p panzo-app --bin panzo-cli -- stability-stimulus 30

# 打开支持 Record / Stop 的 V0.1 Recorder Window
cargo run -p panzo-app --bin panzo-cli -- recorder .tmp\recordings

# 无参数直接打开 Recorder；默认工程库为 %USERPROFILE%\Videos\Panzo
cargo run -p panzo-app --bin panzo

# 打开录后 Editor Workbench
cargo run -p panzo-app --bin panzo-cli -- editor .tmp\recording.panzo

# 在 Project 副本上执行 Editor 持久化、共享渲染、导出和 UI 一键验收
.\scripts\verify-editor-mvp.ps1 -SourceProject .tmp\recording.panzo

# 启动时扫描残留 recording.lock 并恢复 Project
cargo run -p panzo-app --bin panzo-cli -- recovery-scan .tmp

# 解码真实 H.264/fMP4 为可见尺寸的 BGRA8，并检查 PTS
cargo run -p panzo-app --bin panzo-cli -- decode-probe .tmp\recording.panzo\media\screen.mp4 120

# 同一 Project Tick 复验 Preview / Export 未压缩 BGRA Hash
cargo run -p panzo-app --bin panzo-cli -- render-hash-probe .tmp\recording.panzo 10000

# 使用共享 Frame Evaluator / D3D11 Compositor 导出 2 秒 1080p60 H.264
cargo run -p panzo-app --bin panzo-cli -- export-probe .tmp\recording.panzo .tmp\export.mp4 2
```

`record-probe` 会执行 Preflight。任何必需能力失败时，它会在创建 Project 前明确退出，不生成伪成功项目。

录制过程会把阶段与每秒心跳同时输出到控制台和项目内的
`diagnostics/session.log`。`verify-m1.ps1` 使用“请求录制时长 + 30 秒”的独立进程看门狗；
若底层 Windows 驱动调用失去响应，脚本会终止该次探针、保留可诊断 Project，并打印最后
20 条录制阶段，而不会无限停留在 `Running`。

## M1 已实现的链路

- 第一个有效 WGC `SystemRelativeTime` 建立统一 Session Epoch；视频、光标和点击均使用 100 ns 整数 Tick。
- WGC 回调复制到预分配 D3D11 纹理；Capture Queue 固定为 4，无法取得空闲纹理时丢弃当前帧并计数。
- WGC Capture Device 与 Video Processing Device 完全隔离，预分配 BGRA 纹理通过 NT Shared Handle 和 `IDXGIKeyedMutex` 在两台 D3D11 Device 间零拷贝交接；因此 WGC Copy 与 `VideoProcessorBlt` 不再共享 Immediate Context。Processing Device 使用 2 秒有界 `Map(DO_NOT_WAIT)` 轮询回读，不调用在 NVIDIA + WGC 共享 Context 下可能永久阻塞的 Fence Signal。每轮最多消费进入该轮时已有的 Capture Queue 帧，避免驱动等待或持续生产饿死停止期限。
- D3D11 Video Processor 完成 BGRA→NV12 并保持捕获源原生尺寸；Media Foundation Hardware MFT 写入 H.264 High fragmented MP4。
- `WH_MOUSE_LL` 记录 Left/Right/Middle Down/Up，120 Hz Sampler 记录位置和可见性；共享队列容量为 65,536。
- Cursor/Click/Journal 使用单调 JSONL；最多 1 秒 flush、2 秒 durable checkpoint，并原子更新 `recording.lock`。
- 恢复扫描校验进程、截断 JSONL 半行、解析完整 fMP4 fragment 和媒体时间、修剪晚于视频的事件，并迁移到 `recovered` 或 `failed`。
- 正常结束生成 `screen.mp4`、`camera.json`、`metrics.json` 和 capture frame diagnostics，最终进入 `ready`。
- Recorder 捕获来源可在主显示器和可见外部顶层窗口之间切换。Window Capture 使用 WGC `CreateForWindow`，在 Manifest 保存 `kind=window`、Window ID/Title，支持任意宽高比与窗口移动后的鼠标坐标更新；录制中改变窗口尺寸会明确终止并保留可恢复 Project。

## M3 Render Foundation 已实现的链路

- Media Foundation Source Reader 解码项目 H.264/fMP4，按容器可见尺寸协商 RGB32，避免把 1080p H.264 的 1088 宏块填充行带入合成。
- `FrameEvaluator` 统一处理 Project→Source Time、VFR PTS 选帧、Camera State、Cursor 插值和 Camera Transform。
- D3D11 Shader 负责 Camera Crop / Scale 与固定 32 px Panzo Cursor；Preview 和 Export Hash 探针调用同一个 Compositor。
- Export 以整数 Tick 生成 60 CFR，合成 BGRA→D3D11 NV12→Hardware H.264，输出 1920×1080、无音频 fMP4。
- 导出先写同目录 `*.panzo-part.mp4`，失败只清理临时输出，成功后再提交目标文件；原始 Project 媒体禁止作为输出目标。
- Preview 已实现 Play / Pause / Seek / Home、Camera Diagnostics 与 Camera Track 重生成，并默认按源视频最高原生分辨率合成。
- Recorder Window 已实现显示器/应用窗口捕获来源菜单、Record、Stop、录制时长、原生分辨率、Finalize 状态、Open Last Project、后台 Export MP4、Preview、Project Folder 与 Diagnostics 入口；关闭录制窗口会先安全停止并完成 Finalize，导出期间关闭则等待导出提交完成。

## M4 稳定性验收基础设施

- `metrics.json` 直接记录进入 Capture Queue 的 WGC 捕获帧间隔样本数、P99 与最大值，不再用限流后的编码帧间隔代替。
- `stability-probe` 按技术方案统一判定录制时长、回压比例、平均编码帧率、捕获间隔、Capture Queue、Input Overflow、Project 状态和 fMP4 完整性。
- `stability-stimulus` 使用 10 ms 高频唤醒和单调时钟进行 60 Hz 节拍控制；棋盘预渲染后只局部重绘移动扫描带，避免 1440p 全屏 GDI 搬运把动画限制在约 40 Hz。
- `verify-m4-stability.ps1` 默认自动打开全屏 60 Hz 动画刺激源、执行 30 分钟真实录制、保存判定 JSON，并复验开始帧解码与索引 Seek；可用 `-NoStimulus` 接入外部动画源。
- `verify-m4-recovery.ps1` 默认执行 5 次受控强制终止，只终止脚本自己启动的子进程；逐次验证最多 2 秒损失，并注入“导出覆盖源媒体”失败以复验源 Hash 和临时文件清理。

## Editor Workbench MVP 第二个增量

- 首次人工保存会创建只读 `tracks/camera.auto.json`；当前编辑结果继续保存在 `tracks/camera.json`，画布样式保存在 `edit/workbench.json`，编辑提交追加到 `edit/history.jsonl`。
- Camera Segment 支持时间线选择、主体移动、两端修剪、焦点/scale/起止时间调整、新增、删除以及跨 Camera 与 Style 的 Undo/Redo。
- 选中片段后的缩放、焦点与起止时间操作集中在 Camera Track 下方的上下文工具条；Inspector 只保留片段摘要和背景、画布外观等属性。
- 人工 Segment 和被修改 Segment 会设置 `locked/userModified`；Camera 重新生成会保留这些范围，不再无条件覆盖人工结果。
- Solid/Image Background、Canvas Inset、Corner Radius、Shadow 和 Cursor Style 已进入共享 D3D11 Compositor；Preview、Render Hash Probe 与 Export 读取同一编辑状态。
- PNG/JPEG/BMP 背景导入后按内容 Hash 复制到 Project 的 `assets/`，设置只保存安全的 Project 相对路径；背景图片按 Cover 方式由 GPU 采样。
- 选中 Camera Segment 后可在 Preview Canvas 直接拖动焦点十字，并用鼠标滚轮修改 scale；这些操作和样式修改统一进入 Undo/Redo。
- Recorder Window 的 `Preview` 入口已升级为 `Editor`。关闭 Dirty Editor 会先保存，保存失败则阻止关闭。

## Editor UX Foundation 性能升级

- 时间线已从“整段工程固定映射”升级为独立 Viewport，支持鼠标锚点缩放、`Shift + Wheel` 水平平移、Fit 和播放头自动跟随；30 分钟工程可缩放到不高于 `0.5 ms/px`。
- Timeline 与 Canvas 拖动改为轻量草稿和局部重绘，鼠标释放时才提交高成本合成；连续 Preview 滚轮缩放在 160 ms 静默窗口内合并为一个 Undo 状态和一次最终合成。
- Paused Preview 不再随 16 ms Timer 空转重绘；5 秒实测只渲染/绘制首帧 1 次。
- D3D11 Compositor 会话内复用背景、输出、Render Target 与 Staging 资源；背景设置不变时复用已解码图片。5 秒 2560×1440 播放实测中，背景解码、背景上传和输出纹理分配均为 1 次。
- Preview Smoke 新增平均/最大渲染耗时及缓存计数；`verify-editor-mvp.ps1` 已将暂停空转和资源重复创建列为回归失败条件，并支持 `-TargetDir` 隔离构建。
- 随机定位保留快速直跳；若 Media Foundation 硬件解码器跳过索引指定 PTS，则从目标前 30 帧预滚并逐级扩大到流起点，必须解码到精确索引帧，不能把正常 seek 偏差暴露成 Editor 致命错误。时间线交互定位改由自调度窗口消息增量执行，每批最多读取 4 帧，批次间立即让出消息循环且不再等待 16 ms Timer；非目标帧不做 BGRA 拷贝，新点击可中断旧定位。
- Editor 窗口类注册允许同一 Recorder 进程重复使用；关闭 Editor 后可再次从 Recorder 打开。`preview-reopen-smoke` 会在同一进程连续创建和关闭两次 Editor，作为该生命周期问题的回归探针。
- Camera Segment 移动与两端修剪会先暂停播放；鼠标移动重绘按约 60 Hz 合并，长工程只遍历当前可见的点击与片段。松手后的镜头更新复用增量精确定位，不再同步调用 `render_current()`；`preview-trim-smoke` 覆盖该非阻塞回归路径。
- Timeline Ruler、Click Track、空白 Camera Track 以及蓝色播放头已有拖动 Scrub；当前按约 33 ms 合并后台拖动解码请求，松手后执行精确定位。后台 Scrub 不等于所有精确定位/播放工作均已离开 UI 线程，也不能证明连续拖动的端到端画面反馈通过。播放意图可立即切换，但历史长工程仍有首帧延迟待复验。新录制 H.264 使用 1 秒 GOP；本轮仍须覆盖旧 GOP 工程，详见新验收计划。
- Editor 播放已改为独立子窗口上的 DXGI Flip Swap Chain：D3D11 合成结果直接写入 Back Buffer 并交给 DWM，常态播放、定位和修剪预览均不再执行 GPU Readback、CPU BGRA Copy 或 GDI `StretchDIBits`。原 CPU 合成接口仅保留给 Render Hash Probe、Export 一致性验证和 GPU 直出失败时的兼容回退。
- Preview Surface 与父窗口使用 `WS_CLIPCHILDREN` 隔离；Command Bar、Inspector 和 Timeline 的 GDI 脏区刷新不会再覆盖视频帧。镜头焦点十字也进入同一 GPU Shader，避免覆盖层与视频分两次上屏。
- Editor Save 使用写前多文件事务：Camera、Workbench、History 与首次 Auto Draft 全部完成暂存和 Flush 后，才原子发布单一提交标记；标记前崩溃保留完整旧版本，标记后崩溃在打开 Project 时幂等前滚为完整新版本。Preview、Export 和 Camera Regenerate 都会先恢复未完成事务。

## macOS 风格 UI 重构（第一阶段）

- 旧版 UI 的视觉完成结论已经撤回。当前第一阶段保留 Win32 Host 与独立 DXGI Preview，重构 UI Chrome，不触碰已经通过的录制、合成和导出核心；决策见 [ADR-0001](docs/ADR-0001-UI-Refactor-Slice-1.md)。
- Recorder 与 Editor 启用 Per-Monitor V2 DPI Awareness，Theme、窗口/最小尺寸、布局和字体随 Monitor DPI 缩放；字体统一为各支持版本均内置的 Segoe UI，避免缺少 Variable 字体时回退到等宽字体。
- 符号命令使用共享 24×24 SVG 图标，经 `resvg`/`tiny-skia` 栅格化并缓存，不再使用逐图标 GDI 线段拼装。缩放、焦点和起止时间均为纯图标按钮，并保留中文 Tooltip 与可读名称。
- Timeline 的 Timecode、Ruler、Click Track、Camera Track、镜头上下文工具和状态/统计信息使用独立行；刻度标签按实际字体宽度规避碰撞。Inspector 改为全宽属性行，镜头摘要逐行绘制。
- Recorder 与 Editor 的可见标题、状态、按钮、检查器、时间线和文件选择器已中文化；技术探针的 CLI 输出仍保留英文，便于现有验收脚本稳定解析。
- 按钮支持 Idle、Hovered、Pressed、Selected、Disabled，禁用控件不再执行命令；交互控件和可编辑 Preview/Timeline 使用 Hand Cursor。
- 悬停 550 ms 显示中文 Tooltip。Tab/Shift+Tab 在可用控件间循环，Enter/Space 激活焦点控件，焦点环与原生窗口标题同步显示控件可读名称。
- Windows 11 原生标题栏进入深色模式并启用圆角，系统关闭/最小化/最大化行为保持不变。
- 播放态每帧只重绘 Preview，Timeline 最多每 100 ms 更新；Command Bar 与 Inspector 不再逐帧重画。局部刷新使用双缓冲脏区提交，不直接在窗口 DC 上分步绘制。
- 正式桌面入口为 Windows GUI 子系统的 `panzo.exe`，无参数启动 Recorder，默认工程库为 `%USERPROFILE%\Videos\Panzo`，不会创建后台控制台窗口；启动失败通过原生错误对话框报告。
- 技术探针与自动验收统一使用控制台子系统的 `panzo-cli.exe`，保留可重定向的标准输出、错误输出和退出码。
- 第一阶段的代码、SVG、DPI 和布局矩阵自动验收已通过；真实窗口视觉观感仍须本地复验，当前不宣告 macOS 风格最终通过。阶段记录见 [UI 重构第一阶段报告](docs/V0.2-UI-Refactor-Slice-1-Report.md)。

## 仓库结构

- `crates/panzo-core`：时间、坐标、事件、Project Schema、JSONL Writer、Camera Track 和离线 Planner；不依赖 Win32/GPU。
- `crates/panzo-windows`：QPC、WGC、输入采集、D3D11、Media Foundation、fMP4、录制、恢复、解码、合成与导出。
- `crates/panzo-app`：`panzo.exe` 正式 GUI 入口与 `panzo-cli.exe` Preflight/验收探针入口。
- `fixtures/camera`：10 个 Camera Golden Fixtures。
- `scripts/verify.ps1`：格式、测试、静态检查与可选媒体/输入探针。
- `scripts/verify-m1.ps1`：交互式 M1 录制验收入口。
- `scripts/verify-m1-recovery.ps1`：仅终止明确子进程并复验最多 2 秒数据损失的恢复入口。
- `scripts/verify-m3-foundation.ps1`：共享帧 Hash、1080p60 短导出、容器解析与完整解码回读。
- `scripts/verify-m3-preview.ps1`：Preview 播放、暂停不重复重绘、原生分辨率与确定性 Seek 验收。
- `scripts/verify-recorder-ui.ps1`：真实 Recorder Window 的 Record、Stop、Finalize、Project Ready、后台 1080p60 Export、应用重启与 Open Last Project 验收。
- `scripts/verify-m4-stability.ps1`：30 分钟录制、稳定性阈值、fMP4 与解码回读验收。
- `scripts/verify-m4-recovery.ps1`：5/5 强制终止恢复与导出失败原子性验收。
- `scripts/verify-editor-mvp.ps1`：在 Project 副本上验收 Auto Draft、图片资产与编辑持久化、Preview/Export Hash、1080p60 导出回读、Editor UI、暂停态空转、渲染资源复用，以及定位未完成时播放意图不丢失且在 750 ms 内开始播放。
- `scripts/verify-editor-atomic.ps1`：在十个持久化阶段分别启动独立子进程并注入退出，验证提交点前为完整旧 Revision、提交点后为完整新 Revision，源视频/鼠标事件不变且恢复后无事务残留；默认也由 Editor MVP 一键验收调用。
- `scripts/verify-window-capture.ps1`：启动真实 60 Hz 可见窗口，复验窗口枚举、窗口级 WGC、项目来源元数据、硬件 H.264、系统光标关闭和媒体解码回读。
- Editor UI Smoke 还会强制检查 `GPU direct preview: true`、Present 次数大于零、CPU Readback 为零以及常态输出纹理分配为零；避免后续改动静默退回 GDI 视频路径。

Golden 期望值不会被普通测试更新。只有人工审阅算法变化后，才运行：

```powershell
cargo test -p panzo-core --test golden update_golden_fixtures_after_review -- --ignored --exact
```

## 当前环境状态

本轮实际环境为 Windows 10 19045 / RTX 4080 SUPER，不是计划中的 Windows 11 技术基线。已有真实 Monitor/Window WGC、输入采集、2560×1440 Hardware H.264、Recorder、Preview、Export 证据；30 分钟持续动画与 5/5 恢复通过的具体冻结构建见[统一验收报告](Acceptance-Report.md)，不得外推为最新构建的全部验收。T1 第二台不同 GPU 兼容测试继续延期；10 段自然度专项移到“录制 + 基础编辑”整体体验验收后，不作为本轮发布前置条件，也不记自然度已通过。非交互式会话仍可能被 Windows Capture Broker 拒绝，此时应在本地交互登录会话运行对应脚本。

阶段证据见 [M0-Spike-Report.md](docs/M0-Spike-Report.md)、[M1-Project-Sync-Report.md](docs/M1-Project-Sync-Report.md)、[M2-Camera-Planner-Report.md](docs/M2-Camera-Planner-Report.md)、[M3-Render-Foundation-Report.md](docs/M3-Render-Foundation-Report.md)、[M3-Preview-Report.md](docs/M3-Preview-Report.md)、[V0.1-Recorder-Window-Report.md](docs/V0.1-Recorder-Window-Report.md)、[M4-Stability-Infrastructure-Report.md](docs/M4-Stability-Infrastructure-Report.md)、[V0.2-Editor-Workbench-Foundation-Report.md](docs/V0.2-Editor-Workbench-Foundation-Report.md)、[V0.2-Editor-UX-Foundation-Report.md](docs/V0.2-Editor-UX-Foundation-Report.md)、[V0.2-macOS-UI-Foundation-Report.md](docs/V0.2-macOS-UI-Foundation-Report.md)、[V0.2-GPU-Preview-Report.md](docs/V0.2-GPU-Preview-Report.md)、[V0.2-Playback-Responsiveness-Report.md](docs/V0.2-Playback-Responsiveness-Report.md)、[V0.2-Editor-Atomic-Transaction-Report.md](docs/V0.2-Editor-Atomic-Transaction-Report.md) 与 [V0.2-Window-Capture-Report.md](docs/V0.2-Window-Capture-Report.md)。
