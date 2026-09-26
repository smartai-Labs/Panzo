# 编辑器标题栏整合 — 2026-09-19

候选：`dist/Panzo-RE-MVP-titlebar-actions-20260919/panzo.exe`。

## 调整

- 移除额外的 58 DIP 应用内顶部栏及双行工程 / 保存提示，将设置、保存、导出放进 32 DIP 窗口标题区，在系统最小化按钮左侧排列。按钮仍为纯图标，沿用悬浮提示、键盘操作和原有功能。
- 单行标题显示工程名；未保存修改用星号标识，编辑和后台保存完成时同步刷新。原返回录制器操作移入设置菜单，原有关闭时保存询问保留。
- 保留 Windows 原生最小化、最大化和关闭按钮；空白标题区用于拖动与双击，窗口边缘用于缩放。按 DWM 实际按钮区域放置功能键，避免覆盖系统按钮。
- 标题区使用带不透明 Alpha 的绘制缓冲，系统按钮区域保持透明，避免玻璃合成把自绘底色叠成灰条；普通卡片和时间线局部重绘继续使用原有路径。
- 三个卡片、卡片间拖拽、时间线高度限制及 160 像素低清缩略图保留。

原生框架依据 [Microsoft DWM 自定义窗口框架说明](https://learn.microsoft.com/en-us/windows/win32/dwm/customframe)；系统按钮区域通过 DWM 查询。

## 验证

- [工作区回归](acceptance/titlebar-actions-20260919/tests.txt)：180 项通过。
- [UI 专项](acceptance/titlebar-actions-20260919/ui-review.txt)：7 项通过，包括原生标题栏命中 / 悬浮提示、最小化和还原、取消点击、点击导出打开原面板，以及卡片拖拽、焦点十字、背景和低清缩略图。
- [标题栏检查记录](acceptance/titlebar-actions-20260919/native-titlebar-review.txt)、[普通窗口截图](acceptance/titlebar-actions-20260919/native-titlebar-normal.png)、[最大化截图](acceptance/titlebar-actions-20260919/native-titlebar-maximized.png)、[还原截图](acceptance/titlebar-actions-20260919/native-titlebar-restored.png)。截图为应用自身 PrintWindow 输出，已检查图标位置和系统按钮绘制。
- 已查看离屏 [深色](acceptance/titlebar-actions-20260919/editor-Dark-96.png)、[浅色](acceptance/titlebar-actions-20260919/editor-Light-96.png)及 [200% DPI](acceptance/titlebar-actions-20260919/editor-Dark-192.png)布局。离屏图不包含 DWM 绘制的系统按钮。
- [严格 Clippy](acceptance/titlebar-actions-20260919/clippy.txt)、[格式检查](acceptance/titlebar-actions-20260919/fmt.txt)、[release 构建](acceptance/titlebar-actions-20260919/build.txt)通过。
- 最终同构建的 [12 秒拖动检查](acceptance/titlebar-actions-20260919/final-scrub.txt)通过：266 个不同源帧、最大画面更新时间间隔 93.735 ms，媒体错误 / 降分辨率帧 / CPU 回读均为 0，松手精确定位和后续播放完成。[构建记录](acceptance/titlebar-actions-20260919/final-scrub.json)。这是已就绪缓存的本地回归，不替代历史完整性能结论。

最终 GUI SHA256：`813CA93462248B69FA80FD888D808A0361ED3F539B567C5A20571B5BCBC4901C`。CLI SHA256：`EB2E54ADF84008C4AFCDCE275D3748C2B345BAFEC60EC6CC576FB3D448683C87`。

本轮原生测试由程序事件和窗口消息驱动，不冒充完整桌面物理输入、跨显示器 DPI 或 Windows 11 / 跨 GPU 验收。测试使用可丢弃素材，未修改用户录制工程。
