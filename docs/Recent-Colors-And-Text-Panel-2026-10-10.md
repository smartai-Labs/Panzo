# 最近颜色与文字面板调整 · 2026-10-10

当前版本：`target/desktop-v14-recent-colors/release/panzo.exe`。退出旧程序后，双击根目录 `Start-Panzo.cmd`。

SHA256：`275d54eb8ab3779d8b2af57f4fe9aec9ea7dea906b012f487ec21148ca095063`。

## 行为

- 字体名称独占一行。加粗／斜体／下划线和左／中／右对齐分成两组，六个按钮均使用主题 SVG，统一 28 DIP 高度及组内 8 DIP 间距，保留中文名称和提示。不透明度与文字内容采用统一的上下标签布局；保留顶部唯一的可输入字号框。
- 颜色浮层移除取消、应用按钮。色块、渐变拖动、有效 HEX 和透明度输入即时更新；点击外部关闭，Esc 或 Enter 同样关闭并保留最后有效颜色。未完成的 HEX 不覆盖已经生效的颜色。
- 浮层是非模态窗口，主窗口保持可用。外部激活时先完成颜色与历史提交，再延后销毁窗口，避免销毁过程吞掉同一次按钮点击。浮层限制在工作区／当前显示器可见范围内。
- 属性面板的圆点显示最近 4 个 RGB 颜色；选中态为白色圆环，圆点与当前颜色矩形垂直居中。没有使用记录时不伪造历史颜色。
- 浮层显示最多 16 个最近颜色、8 个固定标准色、现有 HSV 渐变、HEX 和不透明度。打开期间最近颜色顺序保持不动，关闭后仅将最终有效选择移到首位；按 RGB 去重，透明度不产生重复颜色。外侧快捷圆点点击后再更新顺序。
- 最近颜色写入本机 `Panzo/settings.json`，与工程编辑历史分离，重启保留。图片、字幕、拼图和通用表单共用浮层及颜色记录。
- 已选中图片对象的连续调色在关闭浮层时形成一次属性提交，可一次撤销恢复。画布中的文字草稿继续遵循原有文字编辑提交规则。

## 验证

- [实际 release 颜色验收](acceptance/recent-colors-20261010/release-final/report.json)：主、副显示器定位，窗口归属，主窗口保持启用，点击外部关闭并执行同一次工具按钮点击，实时更新、历史冻结／最终记录、无效 HEX、透明度去重、快捷圆点排序、标准色、重启恢复全部通过。
- 两块显示器调色期间背景采样分别 117／122 次，均为单一稳定背景色；等待超过 5 秒仍响应。采样不能排除采样间隔内的极短闪烁。
- [文字回归](acceptance/recent-colors-20261010/text-verified/report.json)：空文字光标、透明背景、21 个字号预设、自定义字号、画布／面板同步、移动缩放、样式切换无重影、取消恢复、保存通过。270 次控件重绘无白块，GDI 178 → 178。
- 原生 `native_live_color_history_alignment_and_single_undo`：对齐按钮更新、实时颜色预览、一次撤销／重做、16 色上限、RGB 去重及落盘通过。
- 原生 `native_text_region_style_resize_undo`、颜色 HSV 通道、正负显示器坐标与高 DPI 定位、字体测量测试通过。
- [共享字幕控件回归](acceptance/recent-colors-20261010/subtitle/font-video-report.json)：SVG 字体／对齐样式保存、重开、撤销、视频导出通过。测试使用复制并恢复普通字重的专用工程。

截图：[文字面板](acceptance/recent-colors-20261010/release-final/0-inspector.png)、[颜色浮层](acceptance/recent-colors-20261010/release-final/0-live-colour.png)、[文字编辑](acceptance/recent-colors-20261010/text-verified/01-inline-entry.png)。

测试均使用独立进程及临时用户目录，未关闭既有 Panzo 进程或修改用户工程。验证范围为上述定向场景，未执行全量仓库测试。
