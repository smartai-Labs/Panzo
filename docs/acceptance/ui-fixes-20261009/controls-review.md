# 原生控件修正验收

日期：2026-10-09。以下均为隔离 LOCALAPPDATA 的测试窗口；没有读取用户图片、调用 OCR 服务或启动实际录制。

## 已完成

- 深色主体保留现有明度，绿色按钮/选中边改为中性灰及蓝色强调。浅色主背景为白色。
- 原生 EDIT 去除 WS_BORDER、WS_EX_CLIENTEDGE 和原生滚动条；保留 IME、文字选择、键盘、撤销及原生辅助技术。多行区域使用窄主题滚动条。文字右键菜单也使用统一菜单。
- COMBOBOX 闭合面与实际展开列表统一主题；保留原生键盘导航、选择和滚动。
- LISTBOX 在追加内容后也会抑制重新出现的原生滚动条。实现中已修复控件计数消息递归，以及隐藏窗口直接绘制的问题。
- 一般表单提供 scroll_panel / panel_scroll_info，避免通过 SetScrollInfo 创建原生滚动条。
- 颜色面板为锚定浮层，含 HSV、快捷色、会话最近色、HEX、不透明度。取消不改变传入 BGRA；未修改透明度时保留原精确 alpha。
- 录制控制改为可拖动的 450×68 DIP 浮条：时间、音量、暂停/继续、跟随缩放、停止。保持捕获排除；运行中主题改变会刷新全部控件。
- 录制源、托盘、导出结果、文本编辑上下文菜单改为共享菜单。
- 内联 EDIT、旧属性表单和长截图控制条接入共享控件；目录字段接入目录组件。
- 新增准确动作图标、太阳/月亮、路径中间省略及全路径提示。按钮提示复用原窗口，文字变化会更新提示。

## 验证结果

1. `hue_roundtrip_preserves_bgra_channels_and_alpha` 通过，检查色相边界、BGRA 顺序和 alpha。
2. `native_fields_and_expanded_lists_keep_theme_and_scroll` 通过。实际创建深浅主题 HWND、打开 ComboLBox、滚动文字和列表，检查无原生边框/滚动条。
3. `color_popover_native_theme_and_cancel_preserves_input` 通过。实际打开深浅颜色浮层，生成截图并取消，返回 None。
4. `recording_strip_native_theme_and_capture_exclusion` 通过。检查窗口排除属性及拖动命中；使用合成时间/音量文字。仅截图夹具验证排除属性后临时设置 WDA_NONE，生产代码仍始终排除录制。
5. 素材集成测试另验证追加 145 行之后 WS_VSCROLL 仍为 0，并确认只显示细主题滚动条。

最新控件图：

- [深色输入输出](controls-final/native-fields-dark.png)
- [浅色输入输出](controls-final/native-fields-light.png)
- [深色展开下拉](controls-final/native-dropdown-dark.png)
- [浅色展开下拉](controls-final/native-dropdown-light.png)
- [深色颜色浮层](controls/color-popover-dark.png)
- [浅色颜色浮层](controls/color-popover-light.png)
- [深色录制浮条](controls-hud/recording-strip-dark.png)
- [浅色录制浮条](controls-hud/recording-strip-light.png)

controls 目录最早的两张 recording-strip 图是捕获排除生效时的黑色输出；视觉验收应使用 controls-hud 目录。

以上截图在当前桌面 DPI 下检查。布局使用 DIP，并处理录制浮条 WM_DPICHANGED；本组检查不等同于多显示器 125%、150%、200% 的实际切换，也不代表录制音频硬件重新验收。
