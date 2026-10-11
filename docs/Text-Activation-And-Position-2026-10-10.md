# 文字激活与位置修复 · 2026-10-10

当前版本：`target/desktop-v14-text-activation/release/panzo.exe`。退出旧版本后，双击根目录 `Start-Panzo.cmd`。

SHA256：`c3c0e05569a5c75b3fa42f9731f3a4c55a7ba273b91e22e741b27c8af2aa5428`。

## 修复

- 文字工具点击已有文字前，先提交右侧属性预览，再加载选中对象，避免字体和下划线等被旧数据覆盖。
- 已有文字激活时，点击画布空白提交当前修改并清空选择，不在同一次点击中创建新文字框。未激活文字时字体、样式、字号和颜色属性不可修改。点击右侧属性仍保留当前文字对象。
- 新增共享的原生文字格式和栅格化模块。编辑草稿、属性预览与最终图片均先在原图坐标下渲染，再经过相同的裁剪、变换和缩放流程；替换预览保持原有对象顺序。
- RichEdit 保留键盘、输入、选区和光标服务，编辑层只叠加边框、选择高亮及光标，不重复绘制另一套字形。固定滚动原点，避免多行输入的滚动偏移带到视觉位置中；字体缩放后再设置编辑框边距。
- 草稿内容加入绘制缓存条件，光标闪烁不重复生成文字源图。图片数据结构和已有工程格式保持兼容。

## 验证

- [最终程序原问题回归](acceptance/text-activation-20261010/state-delivery/report.json)：Arial 改成 Courier New 后重新编辑保留；点击空白关闭编辑且不创建新草稿；未选中时字体控件停用。编辑态与提交后的字形边界完全一致，偏差 `[0, 0, 0, 0]`。
- 原生 `native_text_activation_preserves_styles_and_identical_pixels`：0.55、0.815、1.0、1.25 四种缩放 × 左、中、右三种对齐，含中文、多行、加粗、斜体、下划线和半透明文字，共 12 组画布逐像素比较完全一致；重入样式、点击空白和格式边距断言通过。
- [最终程序文字交互回归](acceptance/text-activation-20261010/text-delivery/report.json)：空文本及清空后光标、透明背景、画布／右侧同步、21 个字号预设、自定义字号、区域移动缩放、反复切换样式无重影、取消恢复、保存通过；270 次控件重绘无白块，GDI 208 → 208，无未响应。
- `native_text_region_style_resize_undo`、`text_region_wraps_persists_and_undoes`、`image_font_style_persists_renders_and_undoes` 通过。
- 图片文档 9 项测试通过，覆盖对象修改、透明度、裁剪旋转翻转、撤销重做及各标注工具。
- `native_live_color_history_alignment_and_single_undo` 通过，实时颜色、历史记录、对齐按钮及单步撤销未回归。

测试使用独立进程及隔离用户目录，未关闭已有用户进程或修改用户工程。上述是定向验证；未执行全量仓库测试，也未覆盖所有第三方字体和实体输入法候选窗。

原生缩放 API 语义参考：[Microsoft EM_SETZOOM](https://learn.microsoft.com/en-us/windows/win32/controls/em-setzoom)。
