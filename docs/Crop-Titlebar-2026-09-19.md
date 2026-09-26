# 标题栏修正与画面裁剪 — 2026-09-19

候选：`dist/Panzo-RE-MVP-crop-titlebar-20260919/panzo.exe`。

## 标题栏

截图中的黑块来自系统按钮区域的透明像素与自绘标题背景不一致。现在整个标题区使用同一主题底色，并用统一矢量图标绘制最小化、最大化 / 还原和关闭按钮，悬浮时显示反馈。设置、保存、导出仍在最小化左侧。

保留 DWM 非客户区命中及系统窗口操作，标题空白处仍可拖动、双击最大化；关闭仍经过现有未保存确认。原生处理依据 [Microsoft 自定义窗口框架文档](https://learn.microsoft.com/en-us/windows/win32/dwm/customframe)。本机原生窗口测试覆盖普通、最大化、还原，检查按钮区域底色与标题底色相同、三个图标存在，以及系统命中、最小化还原、应用按钮和导出入口。[截图](acceptance/crop-titlebar-20260919/native-titlebar-normal.png)、[原生窗口记录](acceptance/crop-titlebar-20260919/native-titlebar-review.txt)。

## 画面裁剪

选中视频片段，在右侧“视频”页的“画面裁剪”中操作：

- 左、上、右、下四边滑块，点击数值可精确输入百分比，精度 0.1%。数值表示从原视频该侧去掉的比例，宽高至少各保留 5%。
- 16:9、4:3、1:1、9:16 居中比例预设，以及恢复完整画面的图标按钮。
- 每段独立保存；分割继承裁剪，变速及裁头尾保留裁剪。裁剪不改变片段时长或速度，输出画布尺寸保持原设置，保留的画面等比居中显示，剩余区域显示工程背景。
- 拖动实时预览，一次手势对应一次撤销；支持重做、Esc 取消、保存和重开。修改当前播放位置之外的片段时，先定位到所选片段以显示结果。
- 镜头、光标及黄色焦点标记共用裁剪后的坐标映射；缩放限制在保留区域内，区域外光标隐藏。预览与导出共用合成，原始视频不改写。

新增控件沿用现有图标、悬浮提示、滑块与数值样式，属性页可滚动，标签固定。已检查深浅主题及 96 / 144 / 192 DPI 的 12 张界面图，例如[深色完整面板](acceptance/crop-titlebar-20260919/crop-Dark-96-0.png)、[高 DPI 浅色滚动面板](acceptance/crop-titlebar-20260919/crop-Light-192-10000.png)。

编辑文件保存为 schema 4，每段增加四边裁剪字段。schema 1 / 2 / 3 工程打开时默认完整画面；保存重开及源资产不变已有测试。旧程序不能回写新版工程。

## 验证

- [常规测试](acceptance/crop-titlebar-20260919/tests.txt)：193 项通过，包含裁剪有效性、旧数据兼容、分割继承、撤销重做、保存重开、浮点边界及 GPU 裁剪像素检查。
- [显式界面 / 媒体专项](acceptance/crop-titlebar-20260919/ui-review.txt)：13 项通过。新增四边输入、非法值保留、拖动取消、一次撤销、预设恢复、切换标签和原生标题区像素回归；此前变速、定位恢复、背景、低清缩略图及镜头十字回归保留。
- [裁剪 MP4 对照](acceptance/crop-titlebar-20260919/crop-export.json)：完整画面、方形裁剪 + 2×、非对称裁剪共 2.5 秒、150 帧，Windows 全部解码。6 个首尾与切换边界采样点，预览与有损 MP4 平均 RGB 差异为 0.28～0.50/255。
- [独立 FFprobe 检查](acceptance/crop-titlebar-20260919/crop-ffprobe.json)确认 1920×1080、150 帧、2.5 秒，视频范围及 BT.709 标记正确。FFmpeg 仅用于开发验证。
- [Release 旧工程剪辑 / 导出](acceptance/crop-titlebar-20260919/release-edit-compat.txt)覆盖裁头尾、分割删除、保存重开、快照、同名保护、取消清理及源资产保护；原始尺寸和 1080p 均完整解码 168 帧 / 2.8 秒。
- [Release 裁剪渲染](acceptance/crop-titlebar-20260919/release-crop-render.txt)预览 / 导出未压缩 Hash 相同；[混合裁剪变速拖动](acceptance/crop-titlebar-20260919/release-crop-scrub.txt)媒体错误 0、松手精确定位、无降清帧、后续播放完成。该次含一次外部输入干扰，最大呈现间隔约 178 ms，不记为完整性能验收通过。
- [严格 Clippy](acceptance/crop-titlebar-20260919/clippy.txt)、[格式检查](acceptance/crop-titlebar-20260919/fmt.txt)与 [release 构建](acceptance/crop-titlebar-20260919/build.txt)通过。

原生窗口截图和程序触发事件不代替完整物理输入、Windows 11 Snap 菜单、跨显示器及跨 GPU 验收。整体验收状态继续见根目录 `Acceptance-Report.md`。
