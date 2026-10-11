# Panzo 桌面工作台 v3：原生实现

日期：2026-10-08。候选：[panzo.exe](../target/desktop-v3/release/panzo.exe)。本次把 v3 桌面布局接入实际 Rust / Win32 应用；保留原有 GPU 视频预览、录制、图像文档、任务与插件后端。

## 已接入

- 外壳：图片、录制与视频、素材、插件、设置五个持久工作区；文件 / 编辑 / 视图 / 帮助菜单、主题切换、底部任务状态。
- 图片：顶部文档标签、未保存标记、左侧标注工具及中文提示、中央画布、底部会话缩略图；OCR / 多模态识图面板可收起。每张图片保留文字结果、提问、语言和识别方式；异步任务继续绑定原文档。
- 录制：全屏 / 窗口 / 区域入口，中央来源区域、录制参数和鼠标跟随缩放开关；窄工作区时顶部命令换行。中央显示当前来源及尺寸，并非实时桌面预览。
- 视频：原有真实预览、时间线、画布 / 视频属性、保存及导出入口继续嵌入工作区，采用共享主题。
- 素材：分类、搜索、实际图像缩略图与工程列表；切回时刷新，缩略图后台生成并限制数量和队列长度。
- 插件：独立工作区列出名称、版本、来源和启用状态；安装、启停、回退、卸载调用既有插件管理后端。图片操作仍在图片工作区执行。
- 设置：外观、识图服务、OCR 语言分类。配置继续本机加密持久保存；长表单可滚动，反馈就地显示。

共享按钮与下拉选择器采用原生 HWND 的统一绘制，保留本机键盘 / 焦点行为。按钮通过缓冲绘制写入不透明 alpha，修复组合窗口中未选中标签文字偶发消失的问题。输入框、滚动条和系统菜单的部分细节仍由系统绘制。

## 标题栏

标题、最小化、最大化和关闭区域与外壳共用颜色：深色 #303235，浅色 #E7E9E5。

Windows 11 使用 DWM 颜色设置；当前实机为 Windows 10 build 19045，该接口返回不支持，因此采用既有原生框架的兼容绘制，窗口拖动、缩放和系统按钮仍走非客户区命中测试。最大化时保留工作区边界。没有引入 WebView 运行时。

微软文档将 DWMWA_CAPTION_COLOR 列为 DwmSetWindowAttribute 的参数，从 Windows 11 build 22000 起支持；不能用 DwmGetWindowAttribute 读取该颜色来证明视觉结果。[官方说明](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute)

## 验证

最终实机记录：[workspace-review.json](acceptance/desktop-v3-native-20261008/final/workspace-review.json)。

通过生产窗口与消息循环验证：

- 图片标注、撤销及编辑后的 OCR 结果，在重复切换工作区后保留。
- 后台 A 文档结果不会替换当前 B 文档或跳转选中项；视频工程的 dirty / generation / playhead 保留。
- 示例插件经安装确认界面安装，运行生成新图片；停用后拒绝新任务，卸载保留素材与已打开图片。
- 五个工作区深浅主题截图；标题栏激活 / 非激活颜色采样；最小化 / 最大化 / 关闭按钮命中值及拖动区检查；最大化与还原。
- 960×720 图片多模态面板、录制表单滚动与应用，以及窄录制工作区命令命中检查。
- 没有新增独立业务顶层窗口；系统下拉列表与工具提示仍为其正常的临时窗口。

发布 EXE 另经隔离配置启动、五个工作区切换、导航文字像素检查和正常退出验证，退出码 0：[结果](acceptance/desktop-v3-native-20261008/final/release-smoke.json) · [启动截图](acceptance/desktop-v3-native-20261008/final/release-startup.png)。可用 Python / Pillow 运行 scripts/test-desktop-v3-release.py 复查。

全工作区自动测试：275 通过、32 个需专门环境的测试按原标记忽略。另单独运行上述原生持久工作区测试并通过。cargo fmt 检查与 cargo clippy（全 workspace、all-targets、-D warnings）通过。

真实窗口截图：

| 视图 | 深色 | 浅色 |
| --- | --- | --- |
| 图片 | [查看](acceptance/desktop-v3-native-20261008/final/desktop-dark-image.png) | [查看](acceptance/desktop-v3-native-20261008/final/desktop-light-image.png) |
| 视频 | [查看](acceptance/desktop-v3-native-20261008/final/desktop-dark-video.png) | [查看](acceptance/desktop-v3-native-20261008/final/desktop-light-video.png) |
| 素材 | [查看](acceptance/desktop-v3-native-20261008/final/desktop-dark-library.png) | [查看](acceptance/desktop-v3-native-20261008/final/desktop-light-library.png) |
| 插件 | [查看](acceptance/desktop-v3-native-20261008/final/desktop-dark-plugins.png) | [查看](acceptance/desktop-v3-native-20261008/final/desktop-light-plugins.png) |
| 设置 | [查看](acceptance/desktop-v3-native-20261008/final/desktop-dark-settings.png) | [查看](acceptance/desktop-v3-native-20261008/final/desktop-light-settings.png) |

[录制准备](acceptance/desktop-v3-native-20261008/final/desktop-dark-recording.png) · [小窗口图片](acceptance/desktop-v3-native-20261008/final/desktop-compact-image.png) · [小窗口录制参数](acceptance/desktop-v3-native-20261008/final/desktop-compact-record-settings.png)

测试使用复制的工程、合成图片和隔离的 LOCALAPPDATA，未使用用户 API Key，未发送真实图片到在线服务。程序化消息回归不等同于完整真人操作验收。

## 当前边界

本次交付可运行的原生候选，设计稿中的增强项仍需后续实现：独立「图像信息」页、编辑后缩略图实时重渲染、素材多选 / 排序 / 大列表虚拟化、跨重启恢复整组打开文档。当前会话缩略图展示原始图像，尺寸与未保存状态随文档更新。

真实窗口已在本机 100% DPI 检查；125% / 150% / 200% 多显示器切换、系统文字缩放、完整读屏 / 高对比度、Windows 11 原生标题栏路径仍需专项实机验收。录制编码和在线模型调用没有因 UI 变更重新执行完整媒体验收矩阵，相关历史能力与约束继续有效。ADR-0002 的渲染技术对比仍保持 Proposed，不据此关闭。

旧候选 [plugin-workspace](../target/plugin-workspace/release/panzo.exe) 保留。启动新版后可在右上角切换主题；应用沿用已有用户设置。
