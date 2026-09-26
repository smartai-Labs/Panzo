# 顶部留白与无描边控件调整

根据当前截图反馈调整现有样式，无功能变化。

后续更新：本记录中的窗口控制位置已由[整行对齐修正](UI-Header-Alignment-2026-09-26.md)替代，最新候选为 `target/release/panzo.exe`。

- 公共顶部区域从 32 DIP 增至 48 DIP，28 DIP 高的应用操作按钮垂直居中，上下各留 10 DIP；编辑器内容区同步下移。系统窗口按钮保留原生位置和命中规则。
- “新录制”“整个屏幕”等工具按钮取消常驻／悬停描边，使用底色和文字颜色区分选中、悬停、按下状态。
- “主显示器／应用窗口”来源卡片取消亮色边框。键盘焦点仍使用独立焦点提示。
- 录制器与编辑器沿用同一套顶部布局及按钮绘制，深浅主题同步。

开发候选：`target/ui-spacing/release/panzo.exe`。原 `target/release/panzo.exe` 是此前版本；当前已打开的旧窗口需要关闭后再从新路径启动。

验证：格式和 Clippy 通过；已有布局／公共 UI／录制器测试 27 项通过；离屏绘制与真实标题栏专项各 1 项通过；当前 Release 的录制准备打开、编辑器预览两个 smoke 均正常关闭，媒体错误为 0。未新增样式断言测试。

[深色录制区预览](acceptance/ui-spacing-20260926/recorder-dark-ready.png)、[浅色预览](acceptance/ui-spacing-20260926/recorder-96.png)、[真实编辑器标题栏](acceptance/ui-spacing-20260926/native-titlebar-normal.png)。构建身份和日志位于同一证据目录。

这些检查仅覆盖本次样式及相关命中布局。此前的捕获预检、连续拖动长尾和完整实机验收问题继续保留，见[上一轮记录](Development-Progress-2026-09-26.md)。
