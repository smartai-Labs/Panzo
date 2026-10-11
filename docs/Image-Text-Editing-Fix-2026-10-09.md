# 图片文字、画笔示意与控件重绘修复

2026-10-09。当前启动入口：[Start-Panzo.cmd](../Start-Panzo.cmd)，实际 EXE：[desktop-v14-text-fix](../target/desktop-v14-text-fix/release/panzo.exe)。已打开的旧程序不会自动载入修改，保存工作后使用新入口重新启动。

## 本次行为

- 画笔粗细左侧圆点按数值线性变化，在固定属性行内限定为 2–28 DIP，以免大笔宽挤占控件。数值仍表示实际图片笔宽；原先固定大小的图标已移除。
- 选择文字工具后点击图片，直接出现原生多行输入框；新草稿为空，不再将右侧预填文字直接盖到图片上。
- 画布输入与右侧文字内容双向同步。拖动输入框边缘移动草稿；提交后，选择工具或文字工具均可拖动文字对象，双击重新编辑。
- Enter 换行，Ctrl+Enter 提交，Escape 取消当前修改；点击其他画布位置、切换工具或保存也会提交。选择文字后仍可在右侧修改内容、字号和颜色。
- 一次草稿编辑只形成一次文档修改，支持撤销；空白新草稿取消后不产生占位对象。编辑已有文字并清空后提交会删除该对象。
- 未提交但已修改的文字也计入未保存状态，关闭图片或应用时进入原有保存确认流程。

## 白块与背景残留定位

实际 EXE 的最小化 / 恢复循环复现了子控件残留后方窗口内容：父窗口已画好，但部分工具按钮和文件标签仍显示后方终端内容。这个结果说明不能只统计纯白像素来判断修复通过。

工作台 `layout` 完成后只使用 `RDW_INVALIDATE | RDW_NOERASE`，没有明确请求 `RDW_ALLCHILDREN`。布局缓存又会跳过位置未变化的控件，窗口恢复后这些子控件可能没有重新绘制。现已在布局完成后显式重绘子控件树；按钮自身使用完整初始化的不透明缓冲帧。

此外，原生数值 EDIT 更新时会同步触发父窗口 `EN_CHANGE`，回调中的布局与重绘可能导致数值暂时空白。回归中确认主动发送一次 WM_PAINT 后数字恢复；现已在 WM_SETTEXT / WM_CHAR / EM_REPLACESEL 完成后排队重绘输入框。最终验收直接检查数值更新后的文字像素，不再靠额外测试重绘消息使其恢复。

[修复前恢复后的残留截图](acceptance/image-text-fix-20261009/release/04-brush-24.png) · [修复后截图](acceptance/image-text-fix-20261009/final-release/04-brush-24.png)

原用户截图的全白状态没有逐像素复现；本次实际复现并修复的是同类子控件漏绘 / 背景残留路径，不据此宣称覆盖所有显卡与系统组合。

## 验证

最终 EXE SHA-256：`cfcccdceb928ada2237613a9938c920e737412ae56c8751f1875775c6eebe5c5`。

- 原生消息回归：创建草稿、中文多行文本、双向同步、提交 / 取消、再次编辑、对象移动、撤销、草稿未保存判断和 400 次按钮绘制通过；粗细 4 / 64 的圆点像素面积确实变化。
- 实际发布版：键盘字符输入、双向修改、拖动输入框边缘、提交、Escape 取消、重新编辑、选中后右侧修改以及保存 PNG 通过。保存图中检出 1087 个文字像素。
- 实际发布版：30 轮切换工具，含 6 次最小化 / 恢复，共检查 270 个按钮画面；无白块、无错误背景，GDI 资源计数前后均为 146，无未响应。
- 数值 4 / 24 更新后无需额外测试重绘，分别检出 21 / 40 个可见数字文字像素；原生数值 / 下拉控件主题与滚动回归通过。
- 工作台 v14 原生综合回归通过（33.49 秒），共享控件与布局调整后的其他工作区检查见 `workbench-final/run.txt`；修改文件 rustfmt 检查通过。
- 最终 EXE 主、副显示器颜色面板回归通过：保持打开 6 秒，弹窗位于应用内、工作台继续响应；实时预览、取消恢复、应用并重新打开、实际画笔颜色写入 PNG 均通过。

[画布文字输入截图](acceptance/image-text-fix-20261009/final-release/01-inline-entry.png) · [文字 / 重绘数据](acceptance/image-text-fix-20261009/final-release/report.json) · [颜色回归数据](acceptance/image-text-fix-20261009/final-color-release/report.json)

早期 `release/report.json` 只检查纯白像素，漏掉背景残留，不能作为最终重绘通过的依据；以 `final-release` 为准。颜色测试首次运行时弹窗提前消失，未通过可见性条件，记录保留于 `color-release`；后续最终 EXE 在两个显示器上的完整回归通过。该测试的异常退出采样线程清理也已补齐。

本轮验证针对 Windows 原生控件和当前桌面实际 EXE，不等同于完整 Windows 11、混合 DPI、输入法候选窗及所有 GPU 的验收。没有重新宣称整个仓库测试全绿。

复测命令：

```powershell
python scripts/test-canvas-text-release.py --binary target/desktop-v14-text-fix/release/panzo.exe --output .tmp/text-release-review
python scripts/test-color-popup-release.py --binary target/desktop-v14-text-fix/release/panzo.exe --output .tmp/text-color-review
```

脚本仅启动和关闭自己的隔离进程，使用独立配置及合成图片。
