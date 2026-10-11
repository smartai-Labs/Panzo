# 时间轴空间与 SVG 图标 · v12.3

启动 [Panzo v12.3](../target/desktop-v12.3/release/panzo.exe)。保存当前工作、从托盘退出旧版本，再启动新版；已安装的媒体插件可继续使用。

## 本次调整

- 时间轴不再给底部状态文字预留整行，未选择素材时也不预留空白参数栏。轨道按实际可用空间分配高度。
- 含声音 / 字幕时，为五条轨道保留至少 24 DIP 的高度，缩小时限制整个轨道区的最小高度，避免挤成细线。普通轨道随空间增加到 40 DIP；视频轨道最高 72 DIP，低于 44 DIP 时隐藏缩略图，仅显示素材条。间距、标签和点击区域共用同一套 DPI 布局。
- 工程名和保存状态移至视频预览左上角。修改后显示“● 未保存”，保存后显示“已保存”；保存失败、导出等反馈在同一区域显示，保留诊断与无障碍信息。
- 新增 / 删除媒体轨道导致布局改变时，同步 GPU 预览的位置和裁剪范围；位置没有变化时不重复移动预览窗口。
- 新图标用折叠取景框和播放符号组合，采用深蓝底、冰蓝到靛紫的标记。源文件是可编辑的 [panzo.svg](../crates/panzo-windows/assets/panzo.svg)。窗口左上角的单色标记也已更新。Windows 程序资源、任务栏、Alt＋Tab、托盘及 MSIX 所需尺寸均由 SVG 生成。

## 验证

- Windows 库回归：229 项通过，41 项按条件忽略。
- 布局覆盖 96 / 120 / 144 / 192 DPI，以及紧凑、普通、展开和有 / 无选中素材状态；保留视频轨道高度上限。
- 隔离工程原生回归通过：GPU 预览随媒体轨道增减正确调整，Ctrl＋滚轮缩放、拖动、居中、轨道分隔线拖动、声音 / 字幕入口及字幕编辑正常。
- 原生回归首次运行完成交互断言后，在测试线程退出时遇到本机第三方输入法 `tsf-oime-core.dll` 崩溃。仅在测试线程按 [Microsoft 的 IME API](https://learn.microsoft.com/en-us/windows/desktop/api/Imm/nf-imm-immdisableime) 隔离输入法后重跑通过；应用正式版本的输入法行为未改动。
- 使用原生绘制代码输出深浅主题检查图，并核对紧凑状态和未保存标记。验证没有开启麦克风录制，也没有调用 AI 服务。
- 图标检查通过：10 档 ICO / 运行时 PNG、GUI / CLI 的图标资源、Windows Shell 的 16 / 32 像素图标提取，以及 44 / 50 / 150 像素 MSIX 资源。

检查图：[普通轨道](acceptance/timeline-v12.3-20261009/timeline-media-true-304-selected-false-dark.png) · [紧凑轨道](acceptance/timeline-v12.3-20261009/timeline-media-true-216-selected-true-dark.png) · [浅色展开](acceptance/timeline-v12.3-20261009/timeline-media-true-440-selected-false-light.png) · [未保存状态](acceptance/timeline-v12.3-20261009/project-unsaved-dark.png)。

构建与源码身份已核对：`4726971CB8E6AB4BAB6C91670BED0AFBDADB28C6EAAF4A0A18606EC0AE1B3863`。首次使用媒体插件可安装同目录的 [插件包](../target/desktop-v12.3/release/panzo-media-tools.panzoplugin)。
