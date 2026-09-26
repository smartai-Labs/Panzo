# Panzo RE-MVP 统一验收报告

更新：2026-09-19。结论：**尚未整体通过，不是正式发布。**

## 2026-09-19 第十五轮：编辑体验优化

当前候选：`dist/Panzo-RE-MVP-workflow-20260919`。新增可视化裁剪、比例锁、画布预设与自定义尺寸、逐帧和片段边界定位、吸附提示、布局记忆及后台草稿恢复；修正局部可见属性控件命中，优化拖动解码复制。schema 升为 5，兼容读取 1–4。功能及性能实测见 [本轮记录](docs/Editor-Workflow-2026-09-19.md)。

201 项常规测试通过；15 个界面 / 媒体专项和独立设置持久化专项通过（首次综合运行 14 项通过，新增小画布有损对照阈值不适用，调整测试容差后该项单独复验通过，初始记录保留）。[画布导出对照](docs/acceptance/workflow-20260919/canvas-export.json)检查 1440×2560、1080×1920、640×640，全部 90 帧经 Windows 解码。新旧 release 在同一混合变速裁剪工程上对照，零媒体错误、无降清帧且释放精确；完整物理输入、冷缓存与跨 GPU 门槛继续保留。

## 2026-09-19 第十四轮：标题栏与画面裁剪

当前候选：`dist/Panzo-RE-MVP-crop-titlebar-20260919`。统一标题区及系统按钮底色，保留原生命中和窗口操作；“视频”页新增每段四边裁剪、比例预设与恢复，贯通预览、镜头 / 光标、撤销重做和保存导出。编辑格式升为 schema 4，旧版默认完整画面。193 项常规测试、13 项显式界面 / 媒体专项、严格 Clippy、格式检查及 release 构建通过，详见 [本轮说明](docs/Crop-Titlebar-2026-09-19.md)。

[裁剪导出](docs/acceptance/crop-titlebar-20260919/crop-export.json)覆盖完整、方形变速和非对称裁剪，150 帧 / 2.5 秒全部经 Windows 解码；边界采样平均 RGB 差异 0.28～0.50/255。原生标题栏检查普通 / 最大化 / 还原的底色与图标像素，属性栏检查深浅主题及三档 DPI。整体验收、全部物理输入和跨 GPU 缺口保留。

## 2026-09-19 第十三轮：色彩与变速稳定性

当前候选：`dist/Panzo-RE-MVP-color-stability-20260919`。修正 RGB / NV12 范围与 BT.709 标记，失效预览缓存按需重建；修复本轮镜头拖动复现的异常时间戳定位失败及撤销分割后速度控件失效。189 项常规测试、11 项显式界面 / 媒体专项、严格 Clippy、格式与 release 构建通过。详见 [实现与证据](docs/Color-Stability-2026-09-19.md)。

[同素材颜色对照](docs/acceptance/color-stability-20260919/mixed-speed-export.json)平均 RGB 误差由约 7.4/255 降至 0.24～0.29/255；[独立解码](docs/acceptance/color-stability-20260919/independent-color-check.json)色块最大通道误差 2/255，BT.709 / 视频范围标记正确。[30 分钟工程检查](docs/acceptance/color-stability-20260919/speed-stability.json)连续变速、随机精确定位、跨段、片尾及保存重开通过，源文件 Hash 不变。Release 拖动媒体错误 0 且松手精确定位，但仍有约 171 ms 最大间隔；本轮不是完整性能或跨 GPU 放行。

## 2026-09-19 第十二轮：视频属性与变速

当前候选：`dist/Panzo-RE-MVP-video-speed-20260919`。新增“画布 / 视频”双标签，选中片段支持 0.25×–4×、滑块、预设、精确输入与恢复原速。边缘拖动仍是裁剪，分割继承速度；镜头、光标、预览与导出共享映射，保存升级 schema 3。188 项工作区回归和 9 项显式界面 / 媒体专项通过，见 [本轮实现与证据](docs/Video-Speed-2026-09-19.md)。

[混合速度媒体检查](docs/acceptance/video-speed-20260919/mixed-speed-export.json)：2× / 0.5× / 1.25× 共 6.6 秒、396 帧，经 Windows 全部解码；7 个边界与中间采样点对照原速编码对应源画面，平均 RGB 差异 0–0.044/255。修复前文件缺少显式 tfdt，Windows 只读到 6 秒；导出结束补全片段时间并修正字节偏移，压缩载荷不重编码。未压缩预览与 MP4 的既有色彩转换差异仍有记录，不等于色彩一致性通过。完整物理输入、长期性能与跨 GPU 验收边界保留。

## 2026-09-19 第十一轮：时间线文字精简

用户澄清后已修正：恢复“时间”及两个轨道图标和原位置，仅删除“视频”“镜头”四个字，上方未选择提示继续删除。9 项布局检查及编辑器离屏绘制通过，见 [当前修正版](docs/Timeline-Labels-2026-09-19.md)。下方描述为已被纠正的前一候选记录。

删除时间线左侧标签列和“选择视频片段或镜头”常驻提示，轨道、标尺与滚动条向左扩展，收回 56 DIP 空间。见 [本轮记录](docs/Timeline-Clean-2026-09-19.md)。历史完整桌面和媒体性能验收边界保留。

## 2026-09-19 第十轮：标题栏整合

移除应用内额外顶部栏，将设置、保存、导出放到系统最小化按钮左边；保留图标、悬浮提示、快捷键及操作入口。工程名改为单行，保存星号同步刷新。详见 [实现与验证](docs/Editor-Titlebar-2026-09-19.md)。180 项工作区回归、7 项 UI 专项、严格 Clippy 和格式检查通过；包含原生标题栏截图与普通 / 最大化 / 还原的命中检查，不替代跨显示器和全部物理输入验收。

## 2026-09-19 第九轮：紧凑卡片布局

当前候选：`dist/Panzo-RE-MVP-editor-cards-20260919`。预览 / 播放、属性和时间线为三个 6 DIP 圆角卡片，卡片间隙 6 DIP，外边距 8 DIP。深色底为 `#1B1B1B`，卡片为 `#272727`；时间线按钮和轨道区域同色。分隔窄缝支持宽高调整，最高时间线高度规则保留，下限降至 216 DIP 并压缩工具区，为普通窗口保留实际调节范围。

180 项回归和 6 项显式原生专项通过，已检查深浅主题离屏绘制图及不同 DPI 的卡片边界。专项验证两个分隔处的拖动、取消、收起展开、轨道扩高及黄十字全画面选点。详见 [本轮记录](docs/Editor-Cards-2026-09-19.md)。离屏和程序触发事件不代替完整物理输入验收。

## 2026-09-19 第八轮：镜头十字全画面选点

当前候选：`dist/Panzo-RE-MVP-camera-crosshair-20260919`。用户选定焦点与受边界限制的裁剪中心分开保存；黄色十字可到达视频四边四角，倍率变化 / 保存重开保留选点。GPU 十字在最终画面上层绘制，圆角与背景不再遮住编辑标记；导出不包含标记。

180 项回归、6 项原生专项通过。新增核心持久化 / 历史 / 兼容性检查、GPU 圆角边界像素检查，以及 96 / 144 / 192 DPI、0 / 9% / 25% 边距下的原生窗口拖动事件检查。具体证据与媒体复验见 [本轮记录](docs/Camera-Crosshair-Fix-2026-09-19.md)。不将程序触发事件替代完整桌面物理输入验收。

## 2026-09-19 第七轮：镜头聚焦修正

当前候选：`dist/Panzo-RE-MVP-camera-focus-fix-20260919`。共享镜头插值改为裁剪视口范围插值，消除缩放时的反向偏移；新旧自动镜头抑制视口 2.5% 内的细小平移；容忍边界浮点舍入，避免角落光标瞬间隐藏。手动 / 锁定镜头数据和原始自动初稿保留，旧工程清理仅在显式保存后写入。

176 项工作区回归、5 项原生专项、格式检查、严格 Clippy 和 release 构建通过。实际录屏副本约 10.277 秒处水平 / 垂直反向偏移由 43.07 / 22.45 px 降为 0；完整数值采样的最大残差约 1.31e-13 px。三个问题时间点预览 / 导出未压缩帧 Hash 一致，完整 25.606 秒 MP4 导出成功。见[本轮实现及验证](docs/Camera-Focus-Fix-2026-09-19.md)。本轮不声明完成外部物理输入、跨 GPU 或整体运镜自然度验收。

## 2026-09-19 第六轮：恢复低清时间线缩略图

当前候选：`dist/Panzo-RE-MVP-ui-thumbnails-low-20260919`。按用户要求恢复固定 160 像素缩略图与原代理读取方式，取消原视频高清缩略图与对应调度。背景图片预览、时间线高度上限和主预览 / 导出质量保留。见[本轮记录](docs/UI-Thumbnails-Low-2026-09-19.md)。以下第五轮原视频缩略图结果属于旧候选。

## 2026-09-19 第五轮：时间线高度、背景与原视频缩略图

当前候选：`dist/Panzo-RE-MVP-ui-background-20260919`。时间线最高约占窗口 22%，背景区绘制实际图片，时间线取消 160 像素和代理缩略图，改用原视频精确索引帧按显示尺寸生成。[本轮记录](docs/UI-Background-2026-09-19.md)包含 169 项回归、5 项原生专项与实际录屏副本复验。冷缓存检查发现的缩略图与代理生成资源争用已调整调度并复验，原始失败日志保留；不替代此前冷态拖动性能或完整桌面验收结论。

最终同构建的 [缓存就绪拖动](docs/acceptance/ui-background-20260919/final-scrub-warm.json)与 [预览 / 导出一致性](docs/acceptance/ui-background-20260919/final-render-hash.json)通过。拖动 P95 63.068 ms，媒体错误及降分辨率帧均为 0，松手定位和随后播放完成。

## 2026-09-19 第四轮：深色主题与属性布局

当前候选：`dist/Panzo-RE-MVP-ui-theme-20260919`。设置可切换并保存深浅主题，录制器 / 编辑器共享配色；右侧为固定外观分组，视频片段 / 镜头参数迁至时间线工具区下方，收起属性栏后仍可编辑。[本轮报告](docs/UI-Theme-2026-09-19.md)包含 168 项回归、4 项原生专项、独立的设置持久化检查与深浅绘制图。外部物理输入和完整桌面验收边界不变。

最终程序 [深色主题拖动](docs/acceptance/ui-theme-20260919/final-dark-scrub.json)、[片尾定位](docs/acceptance/ui-theme-20260919/final-tail-seek.json)、[预览 / 导出一致性](docs/acceptance/ui-theme-20260919/final-render-hash.json)、[短片导出](docs/acceptance/ui-theme-20260919/final-export.json)通过。拖动 P95 96.209 ms，媒体错误 0；不将该缓存就绪成绩替代之前失败的冷缓存性能记录。

## 2026-09-19 第三轮：按钮、镜头选择与定位

当前候选：`dist/Panzo-RE-MVP-ui-actions-20260919`。编辑器操作按钮统一为图标，悬浮边框 / 提示、独立底色时间线工具区、短圆角镜头手柄已实现。复现并修复待显示帧与会话像素错配，以及连续随机定位后读取器提前 EOS。[本轮报告](docs/UI-Actions-2026-09-19.md)包含修复前失败证据、165 项回归、4 项显式专项检查和绘制图。

最终构建 [缓存就绪的原尺寸拖动](docs/acceptance/ui-actions-20260919/final-scrub-warm.json)、[片尾定位](docs/acceptance/ui-actions-20260919/final-tail-seek.json)、[预览 / 导出一致性](docs/acceptance/ui-actions-20260919/final-render-hash.json)、[导出](docs/acceptance/ui-actions-20260919/final-export.json)通过，媒体错误 0。实际录屏素材 [冷缓存拖动](docs/acceptance/ui-actions-20260919/final-scrub-recording.json)性能门槛未通过；[上一轮对照](docs/acceptance/ui-actions-20260919/baseline-scrub-recording.json)同样未达标，保留失败与差异，不能据此宣布性能整体恢复。以下第二轮及更早数据属于各自旧构建，不能替代本轮或整体放行。

## 2026-09-19 第二轮：六项 UI 细节修订

最新候选为 `dist/Panzo-RE-MVP-ui-detail-20260919`。撤销 / 重做位置、轨道高度分配、属性栏宽度、滑块与视频圆角抗锯齿及播放头 / 滑动条指针已修改。[本轮报告](docs/UI-Detail-2026-09-19.md)含 165 项回归、3 项显式 UI 检查及绘制图。原生事件处理器已验证面板调宽、恢复和轨道拉高，未产生工程编辑。

同构建 [拖动探针](docs/acceptance/ui-detail-20260919/final-scrub-warm.json)、[圆角画面预览 / 导出一致性](docs/acceptance/ui-detail-20260919/final-rounded-render-hash.json)、[短片导出](docs/acceptance/ui-detail-20260919/final-rounded-export.json)通过。此次原生事件检查与离屏绘制不替代外部物理输入、真实桌面视觉和长时间资源验收。以下旧数据按其构建保留。

## 2026-09-19：UI 细化与拖动清晰度修复

最新候选：`dist/Panzo-RE-MVP-ui-polish-20260919`。完整变更、原生离屏效果图、质量检查与性能边界见 [本轮记录](docs/UI-Polish-2026-09-19.md)。下方 9 月 6 日数据属于历史构建，不能当作新版成绩。

- 中文字体、按钮组合对齐、录制器布局和编辑器信息层级已调整；删除常驻“本地工程 / 非破坏性编辑”等说明，片段参数仅在选中后显示于可滚动侧栏。
- 光标使用带箭杆和抗锯齿描边的形状；窗口指针按文本输入、裁边、拖动和分隔条切换。
- 拖动预览保留源尺寸，旧低分辨率缓存隔离。最终 CLI Hash：`773850FC0D4649B2F11519ADE30700EBFB94943D21308C5DAF5CCEAA3AA64EEA`。
- 串行测试 161 Pass / 3 Ignored；另外显式运行 2 项原生离屏渲染检查。格式和严格 Clippy 通过。
- 同构建 [60 秒冷态](docs/acceptance/ui-polish-20260919/final-scrub-cold.json)、[热态第一轮](docs/acceptance/ui-polish-20260919/final-scrub-warm-1.json)、[热态第二轮](docs/acceptance/ui-polish-20260919/final-scrub-warm-2.json)、[30 分钟工程冷态](docs/acceptance/ui-polish-20260919/final-scrub-long.json)均通过既有探针门槛；缩小分辨率帧数和媒体错误均为 0。
- 初期构建冷态 P95 曾达到 177.781 ms，仍保留此失败。最终单轮通过不代表冷态性能已完全稳定；原尺寸缓存的准备耗时与体积也高于旧版。
- 当前桌面工具无法启动 app-server；本轮效果图来自同一套原生 GDI 绘制代码的离屏渲染，不能代替真实窗口、物理鼠标、IME 与跨显示器验证。既有偶发代理 PTS 错误不据此关闭。

用户要求剩余任务连续实施后统一验收。本轮已实现录制 + 基础编辑的主要闭环并启动统一回归，但不能写成“计划全部任务完成”：真实 UI / 物理输入 / 五分钟任务评审及生产 UI 选型对比仍未关闭。自动测试仅覆盖下述明确范围。

## 2026-09-06：按确认原型重构真实 UI

本轮以 `docs/prototypes/re-mvp/index.html` 及其 CSS/交互为基准，原型未修改。已接入真实录制器、编辑器三区布局、独立播放条、带轨道名称的时间线、内联时间/倍率输入、外观滑杆与色块、光标开关、独立导出面板；不再使用旧的堆叠文字按钮布局。实现与边界见 [原型落地说明](docs/RE-MVP-Prototype-UI-Implementation.md)。

- 本轮最终候选目录：`target/re-mvp-prototype-ui-candidate/release`。
- GUI SHA256：`5990700DCFF36F1018BE50BFF47898D9A5CC239C7963E370E914C541D5696751`。
- CLI SHA256：`F2EA205838CA088458B449C5C5237A8803F41E81971D74CEC8F508F814C686FF`。
- 独立包：`dist/Panzo-RE-MVP-prototype-ui-20260906`；旧包和用户源工程保留。仅用于候选复验，`releaseAccepted=false`。
- 数值输入框未修改就退出不再提交编辑，避免显示精度改变原始时间或发起多余预览定位。
- 同 Hash [基础回归](docs/acceptance/RE-MVP/unified-20260906-152648/automated-regression.json) **22/22 Pass**；[追加长工程回归](docs/acceptance/RE-MVP/final-ui-20260906-152648/final-regression.json) **12/12 Pass**，运行期间 CLI Hash 未变。
- 完整串行测试 **158 Pass / 1 Ignored**（59 核心、1 Golden、98 Windows）；[格式 / 严格 Clippy / 测试退出码](docs/acceptance/RE-MVP/final-ui-20260906-152648/quality-exit-codes.json)均为 0。
- [最新性能与素材 Hash 汇总](docs/acceptance/RE-MVP/final-ui-20260906-152648/metrics-summary.json)：五分钟往返 **17,607** 个不同源帧，全程最大更新间隔 **69.823 ms**，媒体错误 **0**，松手精确收敛；末尾最多 3,600 个请求样本 P50/P95/P99/max 为 **5.500/6.987/8.245/18.450 ms**。这些是软件请求与 Present 指标，不是物理输入到屏幕可见延迟。
- 本次通过不关闭下面保留的偶发映射错误和长帧记录；最终候选没有声称已修复其根因。

### 本轮保留的失败与诊断

1. 首轮 UI 构建 CLI `E2D9B2D5…` 的[基础回归](docs/acceptance/RE-MVP/unified-20260906-142317/automated-regression.json) 22/22 Pass；[追加回归](docs/acceptance/RE-MVP/final-ui-20260906-142316/final-regression.json) 11 Pass / 1 Fail。F05-scrub-3 有 66 个媒体错误，最大间隔 426.686 ms；该次未记录具体错误内容。五分钟持续往返独立通过，不能抵消该失败。[质量退出码](docs/acceptance/RE-MVP/final-ui-20260906-142316/quality-exit-codes.json) fmt=1、Clippy=101、tests=0；格式与新增测试断言警告已在后续源码修正，原报告不改写。
2. 诊断 CLI `407C8360…` 的[五轮复现](docs/acceptance/RE-MVP/ui-scrub-diagnostic-20260906-144203/scrub-diagnostic.json) 4 Pass / 1 Fail。失败轮媒体错误为 0，但单帧呈现约 305 ms、最大更新间隔 395.345 ms；不能把它归因于已观察到的映射错误。
3. 增加错误详情后的 CLI `15B1DEF2…` [十轮复现](docs/acceptance/RE-MVP/ui-scrub-diagnostic-20260906-150515/scrub-diagnostic.json) 8 Pass / 2 Fail。两轮都明确出现 `proxy timestamp is not in its source mapping`，定位到 `PreviewProxy::restore_source_time` 的严格 PTS 匹配失败。未获得差值之前，不把它武断归因于量化、驱动或 UI 布局，也不任意吸附到相邻帧。
4. 再增加实际 PTS/前后索引日志后，[五轮](docs/acceptance/RE-MVP/ui-scrub-diagnostic-20260906-151624/scrub-diagnostic.json)与[二十轮](docs/acceptance/RE-MVP/ui-scrub-diagnostic-20260906-151930/scrub-diagnostic.json)均通过；这只是未再次复现，**不是映射错误已修复的证据**。新增合成/Present 分段慢帧日志，不放宽原有 300 ms 上限、不吞掉媒体错误。

### 真实桌面验证边界

[外部工具实机记录](docs/acceptance/RE-MVP/prototype-ui-20260906/desktop-tool-check.json)使用诊断 GUI，验证启动、中文 UIA 控件、Tab、工程选择器打开/取消和窗口关闭。截图连续报 `SetIsBorderRequired / 0x80004002`，元素点击缺少几何，文件名 `SetValue` 又报 UIA CacheRequest `0x80070057`。没有绕过技能使用其他截图或直接 PowerShell UI Automation。

因此本轮**真实视觉还原、字体与遮挡、物理拖动、内联输入/中文 IME 与导出面板完整 GUI 任务尚未验收**。这不是“桌面未授权”，也不是“Panzo 无法启动”。几何/单元测试、内部原生处理器回归和外部读树不能替代这些项目。

## 2026-09-05 历史记录

以下各节中的“最新候选”指 9 月 5 日构建，只保留用于追溯；不得与上方 9 月 6 日新 UI 构建混用。

## 1. 候选与环境

- 最新 GUI：`target/re-mvp-accessibility/release/panzo.exe`，Release，SHA256 `F30A256D7BDDCB9ECCFA3DC6F749BB265F8AD68FACC09A14D0F7F5F5B41EAB79`。
- 最新 CLI：`target/re-mvp-accessibility/release/panzo-cli.exe`，SHA256 `F7E96A805BB2746045D667177B3AFB92716992F9451D257FFD33117FD3885B4C`。
- 上一候选 `target/re-mvp-ready/release` 的 GUI Hash 为 `8DBF9A098908827ACAF80DA0AEBF5A7E84C93AE1B6704554F2C4468D1252CC55`，CLI 为 `EB716013F4A3D18C99040A5618F78129B8D60DA60808D50F3BAD18CED05E5B2F`；本轮在其基础上增加 UIA 与初始控件树修复。
- 冻结旧候选 `target/re-mvp-final/release/panzo-cli.exe` 的 SHA256 为 `6766E24D7C1B253E633D3AC619A56552708FB10AD9BF1BDB10652A6FF28F799F`。30 分钟录制与 5 次恢复用它执行；后续增加预览时钟、代理优先级、诊断映射、裁头尾媒体断言与 UIA，录制 / 恢复核心未改，但不混淆二进制身份。
- 测试环境：Windows 10 专业版 19045，i7-14700KF，RTX 4080 SUPER，驱动 32.0.16.1062，2560×1440 / 约 99 Hz，64 GiB 内存。
- 不宣称 Windows 11、其他 GPU、4K、音频或自动镜头自然度已经通过。
- 原始工程及旧构建未被替换；自动编辑 / 故障注入均在新建副本中执行。

实现见 [RE-MVP-Implementation](docs/RE-MVP-Implementation.md)，操作见 [使用说明](docs/RE-MVP-User-Guide.md)，未关闭项见 [Known-Issues](Known-Issues.md)。

## 2. 自动证据

### 最新续开发：可访问性与暂停启动

已实现两个窗口的 UI Automation 提供者及有界线程交接，新增 9 项测试通过。外部工具无需发送任何输入便能读取暂停编辑器的完整初始树，见[实际树与构建身份](docs/acceptance/RE-MVP/accessibility-20260905/startup-tree.json)及[实现 / 失败记录](docs/RE-MVP-Accessibility-Report.md)。同构建 60 秒暂停启动复验正常退出：1 个媒体渲染帧、17 次窗口绘制、2 次 GPU Present、0 次 CPU Readback、0 个媒体错误。

最终 GUI 的[局部实机键盘记录](docs/acceptance/RE-MVP/accessibility-20260905/recorder-keyboard.json)验证了三个按钮的 Tab 导航、Enter 打开工程选择器、模态期间主窗口禁用、Escape 取消恢复及关闭退出。记录仅保留相关控件，过滤无关 Shell 条目；未录制、未选择工程，不能代替完整 GUI 任务。当前独立候选包为 `dist/Panzo-RE-MVP-accessibility-20260905`，原包保留。

最新候选[基础回归](docs/acceptance/RE-MVP/unified-20260905-195510/automated-regression.json) **22/22 Pass**，[追加长工程回归](docs/acceptance/RE-MVP/final-ui-20260905-195509/final-regression.json) **12/12 Pass**，运行期间 CLI Hash 未变。源码完整串行测试 **154 Pass / 1 Ignored**，[格式 / 严格 Clippy / 单元测试退出码](docs/acceptance/RE-MVP/final-ui-20260905-195509/quality-exit-codes.json)均为 0。

本轮再次覆盖末尾定位、三轮往返与定位后播放、暂停、20 次编辑器生命周期、视频剪辑 / 两种尺寸导出 / 关键帧对照、窗口录制、录制器原生处理器，以及[十个事务故障点](.tmp/unified-20260905-195510/editor-atomic-5077cb249a8644e4a491d74c9c3625c8/editor-atomic-acceptance.json)。实际媒体输出断言见[显示器源](.tmp/unified-20260905-195510/re-mvp-edit-0f63cb49-713a-47d7-9965-5f6ed8884199.panzo/acceptance.json)和[窗口源](.tmp/unified-20260905-195510/re-mvp-edit-c6cd13b6-edcf-4d7a-b4c4-35b8628d16c1.panzo/acceptance.json)。

最新[性能与素材 Hash 汇总](docs/acceptance/RE-MVP/final-ui-20260905-195509/metrics-summary.json)：

| 场景 | 最新实测 | 软件判定 |
|---|---|---|
| F03 冷 / 热态播放各 60 秒 | Present P95 20.815 / 20.894 ms；最大 38.518 / 39.732 ms | Pass |
| F03 三轮十秒往返 | 请求→Present P95 6.894～7.083 ms；采样两秒窗口最少 116 个不同源帧 | Pass |
| F05 三轮十秒往返 | 请求→Present P95 7.025～7.695 ms；全组最大更新间隔 137.922 ms；松手精确收敛 | Pass |
| F05 五分钟持续往返 | 17576 个不同源帧；全程最大更新间隔 72.992 ms；松手精确收敛；媒体错误 0 | Pass |

五分钟请求分位数仅覆盖末尾最多 3600 个样本：P50 / P95 / P99 / 最大值为 5.770 / 7.931 / 9.997 / 19.891 ms；全程最大更新间隔由独立累计量记录。100 ms 采样的工作集 / 私有内存峰值为 701247488 / 807227392 字节。F03 首次代理准备 9.708 秒；F05 复用已有代理，加载 51 ms，不把它写成首次生成成本。

以上均为内部请求 / 软件 Present 指标，不证明肉眼无闪烁或物理鼠标流畅。下方保留明确标注的旧候选基线用于追踪，不冒充最新结果。

### 上一候选 EB716013… 的自动基线

上一候选[基础回归](docs/acceptance/RE-MVP/unified-20260905-180455/automated-regression.json) 22/22 Pass，[追加性能 / 资源回归](docs/acceptance/RE-MVP/final-ui-20260905-180455/final-regression.json) 12/12 Pass，两次运行期间 EXE Hash 均未变。[指标与素材 Hash 汇总](docs/acceptance/RE-MVP/final-ui-20260905-180455/metrics-summary.json)明确软件测量边界；没有采到的进程内存为 null，不填 0。

冻结旧候选[扩展回归](docs/acceptance/RE-MVP/unified-20260905-170100/automated-regression.json)为 29 Pass / 1 Fail；唯一失败为 F03 60 秒播放 P95 34.206 ms。该次 30 分钟录制、完整索引 / 随机定位和五轮恢复均通过；它不是当前二进制的整体验收。

- `cargo fmt --all -- --check`、严格 Clippy 已通过。
- 上一候选完整串行单元测试：**145 Pass，1 个 Golden 更新用例按设计忽略**。包含源 PTS 时钟、代理暂停、诊断映射、中文错误摘要、持续往返刺激、几何 / 非 16:9 像素、混合 20 步撤销、版本兼容、后台保存冲突与事务等。[格式 / Clippy / 测试退出码](docs/acceptance/RE-MVP/final-ui-20260905-180455/quality-exit-codes.json)均为 0；完整日志在同目录。
- F01 / F02 末尾定位、各三轮 10 秒往返拖动、定位后播放与暂停探针通过。
- 20 次同进程编辑器开关通过；这不代替关闭确认 / 手工反复操作。
- 新增剪辑 / 旁路 / 光标状态接入 10 个事务故障点后通过。[事务证据](.tmp/unified-20260905-180455/editor-atomic-b6f3e86483bf433392b7c15a5d76673a/editor-atomic-acceptance.json)
- F01 分割 / 删除 / 裁头尾 / 保存重开 / 不可变导出快照 / 同名保护 / 取消清理通过；原始尺寸 2560×1440 与 1920×1080 各 170 帧，时长 2.8325112 秒，与保留范围一致，完整解码、PTS 递增。[媒体证据](.tmp/unified-20260905-180455/re-mvp-edit-e9bc7b41-4e29-465b-baf8-cbbdbc76f245.panzo/acceptance.json)
- F06 实际捕获 1201×901 测试窗口，有效媒体 1200×900；原始尺寸 / 1080p 各 141 帧、2.3424567 秒，剪辑与关键 Tick 比对通过。[窗口导出证据](.tmp/unified-20260905-180455/re-mvp-edit-9743de9d-888b-4b67-a3b8-a9a9ee240a6d.panzo/acceptance.json)
- 两份工程的首帧、切口前后、末帧，在两种导出尺寸下，共 16 个未压缩预览 / 实际导出合成 Hash 一致；H.264 再解码另按有损误差检查，不混用 Hash 判定。
- 冻结候选独立 30 分钟复验：108002 帧 / 1800.0356666 秒，59.999 fps，背压丢帧 0；采集间隔 P99 10.1027 ms、最大 20.0191 ms，队列峰值 3/4，输入溢出 0。[稳定性证据](.tmp/unified-20260905-170100/F05/m4-stability-78cec7c6ffa94ab1a1806d003cc5072f.panzo/diagnostics/stability-acceptance.json)
- 冻结候选五次异常终止恢复均通过，源 Hash 保留，恢复约 19.95 秒有效内容，损失满足 2 秒上限。[恢复证据](.tmp/unified-20260905-170100/m4-recovery-2595a31357b64095af36e18784589ae1/recovery-acceptance.json)

### 拖动探针读数

下表来自上一轮 `unified-20260905-174019`（CLI Hash `D964B59A…`）；当前候选同组重跑仍通过，新的逐项日志见上方基础回归。每次新进程，代理已存在。不是物理指针到显示器可见画面的测量；`request-to-Present` 从后台请求提交到软件呈现返回，不能省略这个边界。

| 素材 / 轮次 | 请求→Present P95 | 最大画面更新间隔 | 松手→内部播放完成 | 精确收敛 |
|---|---:|---:|---:|---|
| F01 / 1 | 1.347 ms | 40.249 ms | 86.628 ms | Pass |
| F01 / 2 | 1.308 ms | 40.580 ms | 89.517 ms | Pass |
| F01 / 3 | 1.540 ms | 40.326 ms | 89.544 ms | Pass |
| F02 / 1 | 7.294 ms | 38.817 ms | 106.722 ms | Pass |
| F02 / 2 | 7.376 ms | 40.291 ms | 103.605 ms | Pass |
| F02 / 3 | 7.443 ms | 41.817 ms | 107.816 ms | Pass |

F02 每个被采样的完整 2 秒窗口最少 116 次不同源帧更新。F01 因短素材以较慢源时间速度遍历，计数较低；24 次/秒退出要求按计划使用 F03，而不是拿 F01 的重复呈现凑数。原始日志包含 P50/P95/P99/最大值、时间误差与系统输入中断计数。脚本手势与真实输入须分别验收。

### 上一候选 EB716013… 的播放与长工程结果

| 场景 | 实测 | 软件判定 |
|---|---|---|
| F03 冷态连续播放 60 秒 | Present P95 20.885 ms，P99 21.706 ms，最大 44.477 ms | Pass |
| F03 热态连续播放 60 秒 | Present P95 20.788 ms，P99 21.615 ms，最大 37.188 ms | Pass |
| F03 三轮十秒往返 | 请求→Present P95 6.250～7.734 ms；采样的两秒窗口最少 116 个不同源帧 | Pass |
| F05 三轮十秒往返 | 请求→Present P95 7.592～8.237 ms；最大更新间隔 153.203 ms（第一轮首次请求）；松手→内部播放 67.545～82.314 ms | Pass |
| F05 五分钟持续往返 | 每十秒一次往返，17585 个不同源帧；全程最大更新间隔 67.567 ms；松手精确收敛；媒体错误 0 | Pass |

五分钟测量的请求延迟分位数使用末尾最多 3600 个样本：P50 5.585 / P95 7.751 / P99 10.183 / 最大 27.389 ms；全程更新间隔最大值另用独立累计量，不把末尾分位数写成全程完整样本。采样的两秒窗口最少 115 次不同帧更新。峰值工作集 714678272 字节（约 682 MiB），峰值私有内存 819109888 字节（约 781 MiB），100 ms 采样。缓存 / 任务预算还有独立单元测试；这不证明无限时长资源行为。

F03 首次代理准备 9.707 秒；F05 当前为缓存复用，加载约 52 ms。F05 首次准备的独立日志来自上一构建 `D964B59A…`：283.665 秒，54001 个代理帧；该构建的汇总器有下文所述误判，原始日志与报告保留，不能把缓存加载 52 ms 当成首次成本。原媒体精确定位与导出不使用代理。

所有上述指标都是内部请求 / 软件 Present 与进程采样，不包含物理输入到显示器发光，也不证明无肉眼闪烁；RE-PERF / UI 的完整必测结论仍按下一节执行。

## 3. 必测 ID 的当前结论

`Pass` 仅表示对应功能 / 数据断言确有证据；包含未执行的人工或性能条件时标 `NotRun`，不把局部通过提升为整项通过。

| ID | 状态 | 证据与剩余边界 |
|---|---|---|
| RE-REC-01 | NotRun | 显示器 / 窗口 / 原生控制器探针通过；完整手工 GUI、有效规格和无控制台视觉待复验 |
| RE-REC-02 | NotRun | Ready 后衔接与 Planner 失败旁路已接通；真实录后流程、窗口尺寸变化提示待测 |
| RE-LIFE-01 | NotRun | 20 次生命周期自动回归 Pass；未保存关闭选择与录制器重开需真实操作 |
| RE-SEEK-01 | Pass | F01/F02 末尾定位与往返后精确收敛；依据真实索引，不硬编码历史 359 |
| RE-SEEK-02 | NotRun | 损坏尾部核心处理已有测试；中文错误与可恢复 GUI 流程未完成 |
| RE-CUT-01 | NotRun | 首尾区间与真实成片时长 / 首末帧专项通过；真实边缘手势待验收 |
| RE-CUT-02 | Pass | 分割恒等性、稳定 ID、非法边界与媒体对照 |
| RE-CUT-03 | Pass | 删除闭合、禁止删空、保存重开、切口导出 / 再解码 |
| RE-MAP-01 | Pass | 删除区间反向映射为空；效果 / 光标源锚定核心测试与共享渲染 |
| RE-UNDO-01 | Pass | 混合 20 步往返、拖动草稿一次提交、取消保留 Redo |
| RE-CAMERA-01 | NotRun | 范围 / 目标 / 自动初稿保护已有核心回归；真实拖动与数值输入待验收 |
| RE-STYLE-01 | NotRun | 图片工程化、重开、属性范围、滚轮预览已实现并有核心测试；实际连续操作待测 |
| RE-SAVE-01 | Pass | 10/10 故障点，新字段旧 / 新完整修订、源 Hash 与重复恢复 |
| RE-COMPAT-01 | Pass | v1 无落盘迁移、首次保存 v2、未知版本拒绝、冲突不覆盖 |
| RE-EXPORT-01 | Pass | 1440p / 非 16:9 / 1080p、时长 / PTS / 帧数 / 完整解码；GUI 另列 |
| RE-EXPORT-02 | Pass | 不可变未保存快照、同名保护、取消清理；实际对话框另列 |
| RE-RENDER-01 | Pass | 16 个关键 Tick Hash 对照及有损成片再解码 |
| RE-STABLE-01 | NotRun | 冻结候选独立 30 分钟及 5/5 恢复 Pass；当前预览修正版尚缺同一二进制的完整最终放行复验，首轮 Fail 保留 |
| RE-FLOW-01 | NotRun | F04 五分钟真实操作素材、三轮不靠开发者的任务评审尚缺 |
| RE-PERF-01 | NotRun | 有界调度已实现，未完成 ≥100 样本事件处理分位数矩阵 |
| RE-PERF-02 | NotRun | 未完成物理输入到可见播放头测量 |
| RE-PERF-03 | NotRun | F01/F02/F03/F05 三轮软件探针通过；F04、完整冷热物理输入矩阵与可见画面采样尚缺 |
| RE-PERF-04 | NotRun | 松手精确探针已有通过值；每素材每类 ≥100 样本尚未达到 |
| RE-PERF-05 | NotRun | 定位后播放探针通过；输入意图 / 可见首帧 / 再次取消的完整采样尚缺 |
| RE-PERF-06 | NotRun | 范围 / Undo / 取消已有测试；实际边缘草稿 / 提交性能未齐 |
| RE-PERF-07 | NotRun | 旧候选 Fail 保留；最新冷 / 热态完整 60 秒软件指标 Pass（P95 20.815 / 20.894 ms）；实际无闪烁复验待完成 |
| RE-PERF-08 | NotRun | 暂停 / 有界缓存单测、F05 五分钟连续往返与峰值采样通过；F04 与完整实机交互矩阵尚未完成 |
| RE-UI-01 | NotRun | 真实窗口视觉确认缺失；设计原型确认不能替代 |
| RE-UI-02 | NotRun | 纯几何矩阵通过；截图 / 字体 / 溢出裁切待实测 |
| RE-UI-03 | NotRun | 按下 / 焦点 / 开关状态核心测试通过；真实键鼠逐项待测 |
| RE-UI-04 | NotRun | 时间线附近镜头工具、画布焦点、数值输入已实现；待真实操作 |
| RE-UI-05 | NotRun | 中文 SVG / Tooltip / Tab / UIA 已实现，外部初始读树成功；完整辅助技术操作、输入法仍未齐 |
| RE-UI-06 | NotRun | DXGI 预览隔离 / 零输出回读已有证据；动态闪动需实际交互记录 |
| RE-UI-07 | NotRun | 保存 / 背景 / 导出后台状态已实现；所有错误 / 加载 / 关闭交互待测 |

## 4. 中间失败与未关闭事项

- 受限运行环境中的 WGC `0x8007000E`：同一程序在已授权正常桌面环境下 WGC / D3D11 / 编码通过，不能直接诊断为真实机器内存不足。保留受限运行日志。
- 首轮事务 Hash 误判：故障探针与对照工程各生成随机片段 ID，无法逐文件比对。测试改用固定 ID；正式编辑仍使用 UUID。后续 10/10 通过。
- 首轮 F02 长拖动中测试手势被取消：脚本输入现与外来 OS 捕获 / 焦点消息隔离并报告中断计数；真实用户捕获丢失仍取消手势。该改动不算真实鼠标验收。
- 首轮稳定性失败：录制正常 Ready，107964 帧 / 1800.0362 秒，但最大采集间隔 170.0955 ms，发生于源时间 1105.7522767 秒。运行期间存在并行 GPU 回归，不能据此确定单一根因，更不能把 Fail 改成 Pass。[原始汇总](docs/acceptance/RE-MVP/unified-20260905-161047/automated-regression.json)
- 完整播放失败的进一步排查：后台代理生成与原片播放争用资源；固定 60 Hz 采样 10/20 ms VFR 时间戳还会产生 33 ms 重复帧停留。新增代理暂停 / 取消和源 PTS 驱动时钟后，当前完整冷 / 热态软件复验通过，实际无闪烁仍须桌面复验。
- 该次 5/5 恢复实际成功，汇总器误用了预期同名导出失败遗留的退出码。已修正；[五轮实际证据](.tmp/unified-20260905-161047/m4-recovery-ec26550fcf734fda88721c14cb48d888/recovery-acceptance.json)保留，不覆盖原始汇总。
- GPU 压力并行测试中出现过 VFR 写入器末时长 5.01 s / 预期 5.00 s；单项及串行复验通过。没有修改断言来掩盖它；高负载编码时长仍需关注。
- 追加汇总器把空日志读取为集合，`-notmatch` 没有产生预期布尔值，导致已有完整成功输出的用例被误判 Fail；已规范字符串并要求正向完成标记。五分钟脚本原本只缓慢往返一次，整像素坐标约 220 ms 才变化，出现 437 ms 间隔；改为每十秒重复完整往返再重跑，不把旧场景当成有效的持续拖动性能证据。[原始汇总](docs/acceptance/RE-MVP/final-ui-20260905-174018/final-regression.json)保留不覆盖。
- 上轮 computer-use 两次授权超时。本轮授权已成功，新增了外部读树和部分键盘操作证据；当前截图接口错误为 `SetIsBorderRequired failed: 不支持此接口 (0x80004002)`，坐标输入缺少几何信息。没有获得截图或实际鼠标拖动证据，不再把授权超时写成当前原因，详见可访问性记录。
- [ADR-0002](docs/ADR-0002-RE-MVP-UI-Candidates.md) 的双候选垂直切片比较及完整可访问性未完成。本候选仍为 Win32/GDI + DXGI，不声称 D2D / WebView2 迁移已完成。

## 5. 放行条件与复现

继续完成未通过项，解决桌面截图工具错误后执行 DPI / 键鼠 / 输入法 / 三轮五分钟任务及用户视觉复验，步骤见[桌面复验清单](docs/RE-MVP-Desktop-Review.md)。双候选原生切片 / 完整辅助技术验证以及最终构建的完整放行复验也仍未关闭；不能把这些必测项转为 Deferred 来宣布全部完成。

```powershell
cargo fmt --all -- --check
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo test --locked --offline --workspace -- --test-threads=1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-recording-editing.ps1 `
  -ShortProject <短工程副本> -LongProject <历史60秒工程副本> `
  -Executable target/re-mvp-accessibility/release/panzo-cli.exe -Extended
```

正式录制 / GPU 性能测试应顺序执行，不与其他 GPU 回归并发。T1 跨 GPU、音频、复杂编辑与自动镜头自然度保持 **Deferred**；其余上述 NotRun / Fail 仍是未完成任务。
