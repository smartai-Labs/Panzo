# 时间线文字修正 — 2026-09-19

候选：`dist/Panzo-RE-MVP-timeline-labels-20260919/panzo.exe`。

恢复上版误删的“时间”、视频图标和镜头图标及其原有布局位置。只删除两个轨道图标旁的“视频”“镜头”四个字。“选择视频片段或镜头”常驻提示继续保持删除。

9 项[布局回归](acceptance/timeline-labels-20260919/tests.txt)与[编辑器离屏绘制](acceptance/timeline-labels-20260919/ui-review.txt)通过。已查看[未选择状态](acceptance/timeline-labels-20260919/editor-96-1440.png)，确认“时间”和两个图标均显示，四个文字及常驻提示已删除。[Clippy](acceptance/timeline-labels-20260919/clippy.txt)、[格式检查](acceptance/timeline-labels-20260919/fmt.txt)、[release 构建](acceptance/timeline-labels-20260919/build.txt)记录随包附带。本次为绘制修正，使用测试素材检查，不代表完整物理输入验收。
