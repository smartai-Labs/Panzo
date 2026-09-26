# 时间线文字精简 — 2026-09-19

此候选误删了“时间”和轨道图标，已由[文字修正版](Timeline-Labels-2026-09-19.md)恢复；以下保留为历史记录。

候选：`dist/Panzo-RE-MVP-timeline-clean-20260919/panzo.exe`。

- 删除时间线左侧“时间 / 视频 / 镜头”标签列及图标，标尺、视频轨道、镜头轨道和滚动条统一向左扩展 56 DIP，左右内边距均为 16 DIP。
- 删除未选择片段时的“选择视频片段或镜头”提示。选中后的时间、倍率等编辑控件保留，选择片段时轨道不会上下跳动。
- 编辑功能、辅助技术名称、低清缩略图、标题栏按钮和卡片拖拽沿用现有实现。

验证：180 项[工作区回归](acceptance/timeline-clean-20260919/tests.txt)、7 项[UI 专项](acceptance/timeline-clean-20260919/ui-review.txt)通过；[严格 Clippy](acceptance/timeline-clean-20260919/clippy.txt)、[格式检查](acceptance/timeline-clean-20260919/fmt.txt)及 [release 构建](acceptance/timeline-clean-20260919/build.txt)通过。已查看[未选择片段](acceptance/timeline-clean-20260919/editor-96-1440.png)、[深色](acceptance/timeline-clean-20260919/editor-Dark-96.png)与[浅色](acceptance/timeline-clean-20260919/editor-Light-96.png)绘制结果，标签列、提示已移除，选中参数和轨道保持对齐。

本轮使用可丢弃素材进行离屏绘制和程序驱动的原生事件检查，不替代全部桌面物理输入验收。
