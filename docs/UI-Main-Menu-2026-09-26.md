# 主菜单、原生标题栏与暖灰配色

后续修正：本候选编辑器的标题栏不透明缓冲区覆盖了原生窗口图标，现已在[菜单样式与窗口按钮修复](UI-Menu-Style-2026-09-26.md)中处理。最新程序为 `target/menu-style/release/panzo.exe`，本页保留前一轮记录。

范围：重排既有操作、修复标题栏命中与对齐、统一现有浅色和深色主题；不新增录制、编辑或工程历史能力。

## 实现

- 左侧只保留三横主菜单；右侧设置位于原生最小化按钮左侧。顶部中间留空，不绘制工程名称或 Panzo 字样。
- 移除自绘最小化、最大化、关闭图标及其扩大的命中矩形，恢复 DwmDefWindowProc / DefWindowProc 的系统处理。菜单和设置根据 DWM 返回的完整原生按钮矩形对齐，不再强制窗口控制适配 48 DIP。
- 主菜单集中录制新视频、打开工程、继续最近工程、保存、导出。不可用命令置灰；继续最近工程沿用现有最后工程入口，不新增多条最近打开历史。
- 主菜单为原生弹出菜单，绘制采用共享主题。图标、文字和禁用状态共用现有命令状态；通过 MSAAMENUINFO 暴露中文名称。实现依据 [Microsoft 的自绘菜单可访问名称约定](https://learn.microsoft.com/en-us/windows/win32/api/oleacc/ns-oleacc-msaamenuinfo)。
- 菜单操作复用已有导航、保存、未保存确认和导出处理。菜单的嵌套消息循环运行期间暂缓页面切换，防止后台任务完成时释放仍在使用的编辑器状态。
- 浅色改为米白与暖灰，深色改为炭灰与米白；轨道使用低饱和灰绿与暖棕。来源卡片和普通工具按钮继续采用无常驻亮色描边样式。工程的画布颜色和录制内容不受应用主题变更影响。

主要文件：`editor_ui.rs`、`editor_titlebar.rs`、`app_menu.rs`、`editor_layout.rs`、`editor_chrome.rs`、`editor_navigation.rs`、`workbench_actions.rs`、`preview_window.rs`、`recorder_window.rs`。

## 验证结果与边界

- `cargo fmt --all --check`、全工作区 Clippy（`-D warnings`）通过。
- 全工作区常规测试：223 通过，0 失败，19 项需环境或显式参数的测试默认忽略。日志：[workspace-tests.txt](acceptance/main-menu-20260926/workspace-tests.txt)。
- 显式执行原生标题栏检查：普通、最大化、恢复三种窗口状态下，菜单和设置均返回 HTCLIENT，空白区支持拖动，原生最小化／最大化／关闭位置分别返回对应命中码；程序化最小化与恢复通过。
- 原生主菜单在浅色、深色下打开、绘制并取消通过；取消后不产生导航，现有导出命令仍打开嵌入式面板。记录：[native-titlebar-review.txt](acceptance/main-menu-20260926/native-titlebar-review.txt)。
- 单主窗口 20 次录制准备／编辑往返及原有导航保护回归通过。记录：[navigation-review.json](acceptance/main-menu-20260926/unified-window-0fe920cb-d975-49b2-8e83-7f894f3913f8/navigation-review.json)。
- 独立 Release 候选的暂停预览启动检查通过：窗口成功创建、GPU 呈现一帧、正常退出，媒体错误为 0。该检查不代表连续播放或录制性能验收。日志：[candidate-smoke.stdout.txt](acceptance/main-menu-20260926/candidate-smoke.stdout.txt)。
- 录制页和编辑页多 DPI 离屏绘制检查通过。首次编辑器截图测试使用短素材时，原有测试写死的 10–24 秒范围越界；改为使用当前片段范围的中间一半后，短素材检查通过，未修改业务剪辑行为。
- Computer Use 可以读取新主菜单和设置的中文可访问名称，并用 Tab 聚焦主菜单；截图失败为 `SetIsBorderRequired: 0x80004002`，鼠标点击接口报 `coordinate input geometry is unavailable`。因此本次没有完成原生按钮的实际鼠标点击、菜单项实际点击和完整键盘选择验收，不能以程序化测试代替这些结论。
- 更正：此前把标题栏 PrintWindow 图中缺少系统按钮归因于截图方式，判断不正确。后续确认是编辑器不透明缓冲区覆盖了系统按钮；修复后同一 PrintWindow 检查可以取得三个原生图标。旧命中测试只能证明按钮位置可命中，不能证明可见。

## 可检查的图像

- [深色录制准备](acceptance/main-menu-20260926/recorder-dark-ready.png)
- [深色编辑器](acceptance/main-menu-20260926/editor-Dark-96.png)
- [浅色编辑器](acceptance/main-menu-20260926/editor-Light-96.png)
- [深色原生菜单](acceptance/main-menu-20260926/native-main-menu-Dark.png)
- [浅色原生菜单](acceptance/main-menu-20260926/native-main-menu-Light.png)

## 候选构建

路径：`target/main-menu/release/panzo.exe`。使用 `scripts/build-candidate.ps1 -TargetDirectory target/main-menu` 独立构建，构建身份见该目录的 `candidate-build.json`。用户正在运行的旧版本未关闭、替换或重启。此前的 `target/release` 候选不包含本轮主菜单布局。

本记录替代先前 48 DIP 自绘标题栏的布局结论；既有长时间录制、性能与完整实机验收未因此关闭。
