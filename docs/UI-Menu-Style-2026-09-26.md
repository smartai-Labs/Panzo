# 菜单样式统一与编辑器窗口按钮修复

后续已统一三个原生按钮与标题栏的底色，消除右侧灰色块。当前候选改为 `target/caption-color/release/panzo.exe`，见[标题栏底色统一](UI-Caption-Color-2026-09-26.md)。本页保留前一轮记录。

本轮按用户反馈统一三横主菜单和设置菜单，以三横菜单样式为准，去除白色外框；同时修复编辑器中最小化、最大化、关闭图标不可见的问题。没有新增菜单功能或工程能力。

## 修改

- `app_menu.rs`：共用菜单项、分组标题、分隔线和弹出菜单绘制。录制页设置、编辑页设置和三横菜单使用相同的面板底色、34 DIP 操作行高、16 DIP 图标、文字缩进与悬停底色。长文案通过字体实测调整宽度。
- 设置菜单保留原有命令、分组、主题选择和诊断入口。原生命令返回值映射到原有处理；主题选中态同时保留视觉标记和原生 MF_CHECKED 可访问状态。
- `popup_frame.rs`：仅在当前 UI 线程、本次弹出菜单生命周期内接管系统菜单的非客户区绘制，用面板底色覆盖默认亮色边框。菜单关闭后撤销钩子和子类化，释放画刷；Windows 继续处理菜单输入、滚动和关闭。
- `workbench_actions.rs` / `recorder_window.rs`：设置菜单复用上述绘制，向左展开并对齐设置按钮右边。原有菜单打开期间的页面切换保护同样适用。
- `preview_window.rs`：编辑器此前给整条标题栏的缓冲区写入不透明 alpha，覆盖了 DWM 原生窗口按钮。现仅将实际原生按钮矩形清为透明，其余应用标题栏保持不透明；低资源绘制分支也保留原生区域。三个系统按钮仍由 Windows 绘制与处理，没有恢复自绘替代按钮。

原生窗口修复依据：[Microsoft DWM 自定义窗口边框的透明像素与命中处理说明](https://learn.microsoft.com/en-us/windows/win32/dwm/customframe)。

## 验证

- 全工作区 Clippy（`-D warnings`）、格式检查和常规测试通过；223 通过、0 失败，19 个环境相关测试默认忽略。日志：[workspace-tests.txt](acceptance/menu-style-20260926/workspace-tests.txt)。
- 显式原生窗口检查通过：普通、最大化、还原状态下，三个系统按钮均具备对应命中码，并且截图中的各图标区域存在可见字形。此次增加的是“可见性”检查，防止再出现只有命中区域、没有图标的情况。
- 三横主菜单与编辑器设置菜单在深色、浅色下均成功打开、绘制和关闭，图片已检查，无白色外框。菜单取消不触发导航，原导出命令仍可打开内嵌面板。
- 单窗口 20 次录制准备／编辑往返及原有未保存、保存失败、异步回调保护回归通过。日志：[navigation-review.json](acceptance/menu-style-20260926/unified-window-68d77ee7-ce33-4802-99f7-07bc229bda6f/navigation-review.json)。
- 程序化最小化／恢复通过。上述原生窗口测试未模拟实际鼠标点击，不据此声称鼠标输入全流程验收完成。

截图：[编辑器三个原生按钮](acceptance/menu-style-20260926/native-titlebar-normal.png)、[最大化标题栏](acceptance/menu-style-20260926/native-titlebar-maximized.png)、[深色主菜单](acceptance/menu-style-20260926/native-main-menu-Dark.png)、[深色设置](acceptance/menu-style-20260926/native-settings-menu-Dark.png)、[浅色设置](acceptance/menu-style-20260926/native-settings-menu-Light.png)。

## 使用本轮候选

`target/menu-style/release/panzo.exe`，构建身份保存在同目录 `candidate-build.json`。用户当前运行的 `target/main-menu` 旧候选未关闭或替换；该旧程序不会自动获得本轮修复。
