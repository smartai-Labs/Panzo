# 颜色面板、圆形色块与窗口响应修复

2026-10-09。针对点击自定义颜色后像是将点击传入后方交互原型、项目应用卡住的反馈。

## 结论与证据边界

当前进程检查确认正式应用与原型分别为 Panzo 原生工作台和 Chrome 原型窗口，属于不同进程。颜色按钮调用 `color_popup.rs` 的 `PanzoColorPopover`，没有打开原型 HTML 或主动发送鼠标消息给原型的代码。

已通过原生回归复现：传入图片编辑子窗口时，弹窗的实际 owner 是工作台，但代码仅禁用子窗口；`GetMessage` 又只取颜色弹窗的消息，导致工作台投递的消息无法被处理。修改前回归结果为 `(owner_root=true, root_disabled=false, host_pumped=false, cancel_woke=true)`，修改后四项均为 true。

此外，原代码先销毁活动弹窗、再启用调用窗口，存在焦点回落到其他窗口的风险；同步失焦消息只设置结束标志，缺少明确唤醒。这些窗口状态问题已一起修复。后续对真实 EXE 的颜色按钮执行回归，已复现“颜色面板跑到另一显示器、工作台被系统标记未响应”。实测颜色弹窗仍归属 Panzo；面板出现在原型附近不代表它属于原型或接收了跨进程转发的点击。

## 修改

- 使用 `GetAncestor(GA_ROOT)` 管理工作台级模态状态。
- 消息循环处理整个 UI 线程，保持主窗口绘制、计时器与任务消息运行；保留退出消息。
- 失焦和关闭时投递唤醒消息，保证循环能退出。
- 先恢复主窗口启用状态，再销毁弹窗；弹窗仍处于前台时恢复本应用焦点，用户主动切到其他应用时不抢回焦点。
- 预设色块使用 2 DIP 圆形选中边框，焦点边框保持圆形，移除方形底色。
- 自定义颜色按钮高度从 30 DIP 改为 26 DIP，与前方色块共享垂直中心。

## 验证资料

[回归与截图目录](acceptance/image-color-fix-20261009/) 包含修改前复现记录、嵌入式弹窗回归、深浅主题取消操作测试及图片工作台截图。

Windows 行为参考：[GetMessage](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getmessage)、[EnableWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enablewindow)。

## 启动

修复版：[panzo.exe](../target/desktop-v14-color-fix/release/panzo.exe)。此前已打开的程序不会载入本次修改；请保存工作后关闭旧版，再使用这个新路径启动。


## 后续定位：错屏弹窗与系统未响应

这次使用 `scripts/test-color-popup-release.py` 启动隔离配置的实际 EXE，导入测试图片，点击原生自定义颜色按钮（BUTTON / BN_CLICKED 路径），在两个实际 2560 × 1440 显示器上各保持面板打开 6 秒。没有同步轮询主窗口消息来掩盖旧版阻塞。

- **旧版 `desktop-v14-image`**：副屏主窗口范围 `(2572, 12)–(3672, 812)`，颜色按钮在 x=3524，弹窗却被钳制到主屏 `(2268, 299)–(2560, 709)`；`IsHungAppWindow=true`，主窗口绘制消息积压。主屏上弹窗也超出应用右侧。
- **第一轮 `desktop-v14-color-fix`**：同一按钮回归确认主窗口继续处理绘制，`IsHungAppWindow=false`；但错屏/超出应用的问题仍然存在。这是第一轮没有覆盖多显示器的遗漏。
- **定位根因**：`SystemParametersInfo(SPI_GETWORKAREA)` 获取主显示器工作区，代码把副屏按钮的屏幕坐标强行钳制到这个范围；同时只限制屏幕边界，没有限制应用边界。
- **本次修复**：使用按钮所在显示器的工作区，结合工作台客户区的屏幕坐标确定范围；弹层右侧对齐按钮，优先放在应用内部，空间不足时向上或向内调整。仅当整个窗口小于弹层时才退回当前显示器范围。
- 定位单元回归覆盖主屏、副屏、负屏幕坐标、96 / 144 / 192 DPI 及顶部/底部按钮。

前后对照数据及截图：[真实发布版回归资料](acceptance/image-color-placement-20261009/)。

统一启动入口：[Start-Panzo.cmd](../Start-Panzo.cmd)。该入口指向当前 `desktop-v14-color-fix`，避免继续从旧目录打开旧版。


定位修复阶段的 EXE（SHA-256 `bc4b4cbf6e3bcd6c58032f2021bca41fcf51342cf560658e3c7e34a24ba54106`，已由下方实时预览修复版替代）在两个真实显示器上均通过：弹窗处于应用内部、owner 正确、模态启用状态正确、保持打开 6 秒无未响应且无绘制消息积压；取消后恢复、应用 `#3278C4`、再次打开保留颜色及 Escape 关闭通过。副屏弹窗修正为 `(3266, 295)–(3558, 705)`，在副屏应用范围内。

[最终副屏截图](acceptance/image-color-placement-20261009/final-release/case-0-open.png) · [最终主屏截图](acceptance/image-color-placement-20261009/final-release/case-1-open.png) · [最终 EXE 回归数据](acceptance/image-color-placement-20261009/final-release/report.json)


## 取色闪动、实时预览和应用后色块不更新

源代码中确认了两个独立问题：

1. 原色盘在每次鼠标移动后直接清屏、逐块绘制约 2,860 个渐变色块，画面没有缓冲。改为缓存饱和度 / 明度渐变和色相条，通过整帧缓冲一次提交；增加鼠标捕获和范围限制。
2. 图片页的颜色按钮在“应用”后只赋值 `self.color`，没有刷新 `ChoiceVisual::Swatch` 缓存；弹窗也没有实时通知宿主。改为可取消的颜色预览消息，实时刷新色块、预设色选中态、不透明度与选中对象预览。“取消”恢复原色，“应用”保留新色。原有对象属性提交与撤销机制保留。

最终 EXE：`0450cf0b87816ebc5eeaaeb373b1d4cede9fb611e55b63dbcd70450032362f20`。实际发布版在主、副屏分别通过以下检查：

- 弹窗位于应用内部、持续 6 秒可见，命中检测确认为颜色弹窗，主窗口无未响应、无待处理绘制。
- 80 次拖动消息期间固定背景像素采样分别为 164 / 127 次，仅观察到同一背景色；这是有限抽样，不等同于完整帧率或所有设备上的无闪烁保证。
- 输入 `#1A73E8` 后，弹窗仍打开时属性栏色块像素立即匹配；取消恢复原色。
- 应用 `#3278C4` 后，属性栏色块像素匹配；再次打开仍为该值，Escape 正常退出。
- 在实际图片画布绘制笔迹，快速保存后检查新生成的 PNG：存在足够数量的 `(50,120,196)` 颜色像素，证明绘制和保存使用了新颜色。

修正了验收脚本强制工作台置顶的行为，最终截图及命中检查确认弹窗真实显示在工作台前方。早期定位阶段截图不能替代这项可见性验证。

[最新副屏截图](acceptance/image-color-placement-20261009/live-preview-release/case-0-open.png) · [最新主屏截图](acceptance/image-color-placement-20261009/live-preview-release/case-1-open.png) · [完整链路数据](acceptance/image-color-placement-20261009/live-preview-release/report.json)
