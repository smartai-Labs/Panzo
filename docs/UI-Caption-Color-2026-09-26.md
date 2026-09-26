# 原生窗口按钮与标题栏底色统一

此方案已被[固定灰色标题栏](UI-Fixed-Caption-2026-09-26.md)替代：透明背景仍会随系统激活状态变黑，不满足用户最新要求。以下保留为历史记录。

用户反馈：最小化、最大化、关闭区域不应单独显示灰色背景，应与整条顶部背景一致。

## 修改

当前验证系统为 Windows 10 19045，不支持 `DWMWA_CAPTION_COLOR` 自定义颜色；该属性从 Windows 11 22000 起提供，见 [Microsoft 文档](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute)。此前标题栏左侧使用应用不透明底色，右侧显示原生底色，因此出现灰色矩形。

现在整条标题栏共用 DWM 背景，按钮默认背景与标题栏其余位置一致。Windows 11 继续使用已有的自定义标题栏颜色请求；不支持该请求的系统由 DWM 提供整条背景，不再混合两种底色。窗口失去焦点时，系统底色变化也作用于整条标题栏。

菜单、设置图标及其悬停、按下、键盘焦点图形采用预乘透明度绘制，保留原有大小和位置。三个系统按钮仍完全由 Windows 绘制和处理。录制页、编辑页共用此实现，菜单内容和功能没有改变。

涉及 `editor_ui.rs` 和 `preview_window.rs`；原有窗口检查与录制页截图检查同步覆盖真实标题栏。

## 验证

- 深色与浅色模式：编辑器普通、最大化、还原状态下，三个按钮背景采样与标题栏中部背景完全相同，三个图标可见，原生命中码正确，程序化最小化／恢复通过。
- 深色与浅色模式：录制页真实窗口的三个按钮背景与标题栏中部一致。浅色检查使用隔离的设置目录，未更改用户的主题配置。
- 常规工作区测试 223 通过、0 失败，19 项环境相关测试默认忽略；Clippy（`-D warnings`）和格式检查通过。日志：[workspace-tests.txt](acceptance/caption-color-20260926/workspace-tests.txt)。
- 本次验证没有覆盖实际鼠标点击的完整流程，不把像素和命中检查当作鼠标输入验收。

图像：[深色编辑器标题栏](acceptance/caption-color-20260926/native-titlebar-normal.png)、[浅色编辑器标题栏](acceptance/caption-color-20260926/light/native-titlebar-normal.png)、[深色录制页标题栏](acceptance/caption-color-20260926/native-recorder-titlebar.png)、[浅色录制页标题栏](acceptance/caption-color-20260926/light/native-recorder-titlebar.png)。

新版路径：`target/caption-color/release/panzo.exe`，构建身份保存在同目录 `candidate-build.json`。已运行的旧候选未关闭或覆盖。
