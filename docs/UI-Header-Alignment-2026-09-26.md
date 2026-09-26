# 标题栏整行对齐

根据第二次截图反馈，将应用操作和窗口控制统一到同一条水平中心线。上一版只增高应用操作区、仍以 32 DIP 绘制窗口控制，因此产生了 8 DIP 的错位；本次同步调整绘制和命中区域。

## 布局

```text
设置  新录制  打开工程   工程名                         保存  导出    最小化  最大化  关闭
────────────────────────────── 共同的垂直中心线 ──────────────────────────────
```

- 标题区统一高 48 DIP，全部内容中心位于该区域的 24 DIP 处。
- 应用按钮高 28 DIP，上下留白各 10 DIP；窗口控制使用完整标题区高度作为命中范围，图标居中。
- 左侧保留全局导航，中间工程名弹性占位，右侧保留工程操作和窗口控制；两种工作区采用同一布局。
- 继续使用无常驻描边的样式。窗口按钮返回对应的非客户区命中值，原有窗口命令、退出确认仍按原流程处理；修正扩高后下半部可能成为拖动区的问题。

## 验证与候选

- 已有布局、公共 UI、录制器测试 27 项通过；fmt 与 Clippy 通过。
- 真实窗口专项覆盖普通／最大化／还原、应用按钮命中、窗口控制中心和下沿命中、拖动区、最小化还原与导出面板；通过。离屏录制区绘制专项通过。
- 已查看本次生成的标题栏图片，确认所有图标与文本整行居中。
- 新候选路径：`target/release/panzo.exe`。当前运行的 `target/ui-spacing/release/panzo.exe` 是上一版，需要保存当前编辑后再切换。

[普通窗口标题栏](acceptance/header-alignment-20260926/native-titlebar-normal.png) · [最大化标题栏](acceptance/header-alignment-20260926/native-titlebar-maximized.png) · [检查记录](acceptance/header-alignment-20260926/native-titlebar-review.txt)

本次仅修复标题栏布局和相应命中，之前的录制捕获、拖动性能等未关闭项仍见[开发记录](Development-Progress-2026-09-26.md)。非客户区命中处理参考 [Microsoft 自定义窗口框架说明](https://learn.microsoft.com/en-us/windows/win32/dwm/customframe)。
