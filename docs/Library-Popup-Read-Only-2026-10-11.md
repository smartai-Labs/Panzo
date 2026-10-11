# 素材库弹窗边框与文件信息只读修正（2026-10-11）

- 范围：仅素材库的弹窗预览和文件信息窗口。
- 原因：窗口使用 `WS_THICKFRAME`，但只自绘客户区标题，没有接管非客户区尺寸计算，系统边框与自绘标题叠加，顶部留下多余黑条。
- 修正：两处共用 `library_popup_frame.rs`，在窗口创建阶段就处理 `WM_NCCALCSIZE`，全窗口作为客户区；通过 `WM_NCHITTEST` 保留标题拖动、四边与四角缩放，并排除关闭按钮区域。
- 文件信息：此前文件名和路径使用已设只读的 EDIT 控件，仍呈现输入框和光标。现改为不接受焦点和编辑的 STATIC 文本，支持换行、保留文件名中的 & 字符；完整路径仍可通过原有按钮复制。
- 验证：`library_visual_review` 通过；原生窗口检查客户区与窗口边界一致，缩放前后无非客户区条带，标题/关闭按钮/四边/四角命中正确；输入、退格、清除、粘贴消息均不能修改信息。已检查深色和浅色截图。
- 截图：`.tmp/library-popup-fix/verified/13-popup-image.png`、`17-file-information-dark.png`、`18-file-information-light.png`。
- 构建：`target/desktop-v14-library-popup-fix/release/panzo.exe`；启动入口 `Start-Panzo.cmd`。
