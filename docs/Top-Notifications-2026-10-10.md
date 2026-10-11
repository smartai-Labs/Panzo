# 顶部短时通知

版本：`target/desktop-v14-top-notifications/release/panzo.exe`

SHA256：`332146f28223e468f150bf536957be7fcd690057e81cb40e2945e78286d70c81`

## 调整

移除工作台底部常驻状态文本控件。普通操作消息、任务完成消息以及原有错误入口统一使用顶部通知；任务进度继续在任务入口和对应面板显示，结果及错误详情由原面板保留。

通知相对应用客户区水平居中，位于顶部导航下方 16 DIP。使用主题背景、圆角和 SVG 图标，按用户补充要求不绘制外边框。宽度随内容变化，并限制在窗口范围内；过长文本省略显示。

普通及成功提示存续 2.5 秒，错误提示存续 5 秒，带 160ms 淡入淡出，并尊重系统动画设置。同一时间只有一条提示；重复或新消息更新同一个窗口及计时，不堆叠。通知不激活窗口、支持鼠标点击穿透，不抢输入焦点；应用移动、缩放和 DPI 变化后重新定位，失焦或最小化时隐藏，到期清除内容并停止计时器。

## 实现与验证

`workspace_toast.rs` 提供通知生命周期及主题绘制，`workspace.rs` 负责消息和任务事件路由。由于当前程序没有分层子窗口所需的兼容性清单，最终使用工作台拥有的非激活工具浮层，并限制在工作台客户区内；未修改应用全局兼容性设置。Windows 分层窗口行为参考 [Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features)。

原生隔离测试通过：水平居中、尺寸变化、输入焦点保留、点击穿透、重复消息刷新、2.5/5 秒到期、任务进度与完成分流、深浅主题、切换到其他窗口后隐藏、最小化及所有者销毁清理。真实 2.5 秒计时通过消息循环验证；错误 5 秒边界通过调整测试时钟起点验证。

最终发布版实际执行离线 OCR 两次，识别到 `PANZO OCR TEST` 和 `Text stays available`。提示水平居中误差 0.0 像素，完成消息约 2.51 秒后清除；结果文本仍保留，窗口移动缩放后继续居中，最小化时隐藏，无未响应。验收脚本按消息到期清除判断生命周期，以区分桌面焦点变化引起的临时隐藏。

- [发布版报告](acceptance/top-notifications-20261010/release/report.json)
- [深色主题，无边框](acceptance/top-notifications-20261010/release/01-ocr-success-top.png)
- [自动消失后](acceptance/top-notifications-20261010/release/02-after-auto-dismiss.png)
- [浅色主题，无边框](acceptance/top-notifications-20261010/release/03-ocr-success-light.png)

启动入口已切换到本版本，旧版本文件及用户正在运行的进程保持原状。构建源码身份已与候选版清单核对一致。
