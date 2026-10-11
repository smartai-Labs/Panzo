# 图片工具编辑页 v14 调整记录

日期：2026-10-09。设计依据为 `docs/prototypes/workbench-v14/index.html`、`app.js`、`styles.css`；用户截图用于核对视觉与控件状态。

## 启动

双击 [新版 Panzo](../target/desktop-v14-image/release/panzo.exe)。开发启动仍为 `cargo run -p panzo-app --bin panzo`。已有旧程序进程不会自动载入新版本，需使用这个新路径启动。

## 本轮调整

- 图片工具栏与文档栏使用原型的 SVG 路径，统一尺寸、线宽、选中颜色、分隔线与底部操作位置；接入顶部撤销。
- 画布适配采用原型的最大显示宽度与留白规则，保留图片实际比例；默认适配显示为 100%。
- 裁剪面板对齐标题、比例分段控件、宽高输入框、单位、调整操作和底部“取消 / 应用裁剪”布局。
- 裁剪框使用四角控制点、半透明遮罩与白色九宫格线，补充裁剪尺寸标牌。
- 画笔、矩形、文字等属性中的数值输入接入真正的增减操作；悬浮或聚焦显示紧凑实心上下箭头，支持单击、按住连续变化、键盘上下键与范围限制。
- 修正原生单行输入框和可编辑下拉框的文字区域高度；数字、上下箭头组与单位共用字段的垂直中心，切换悬浮状态不会改变数字位置。
- 调整选择工具、OCR / 识图属性区的标题、间距、结果区域和底部操作布局。

主要实现位于 `picture_window.rs`、`image_workspace.rs`、`image_interaction.rs`、`image_icons.rs`；通用输入控件修复位于 `field_controls.rs` 和 `workspace_ui.rs`，图标渲染位于 `desktop_ui.rs`、`editor_ui.rs`。

本轮聚焦图片编辑页。选择工具中的画布尺寸仍为信息展示，未新增修改画布尺寸功能；不将本轮结果视为其他工作区或整个项目已完全还原原型。

## 验证

- 发布版构建成功：`scripts/build-candidate.ps1 -TargetDirectory target/desktop-v14-image`。
- 原生数值控件测试通过：鼠标增减、键盘上下键、上下限与编辑；在 96 / 120 / 144 / 192 DPI 字体度量下检查单行编辑框及可编辑下拉框的内容垂直中心，误差不超过 1 像素。
- 原生图片操作测试通过：笔迹、文字编辑、裁剪控制点、应用、取消、撤销与缓冲绘制。
- 工作台原生验收通过：深浅主题、紧凑窗口、720 × 405 裁剪输入与应用、顶部撤销恢复；使用隔离生成的测试素材。
- 完整库测试：333 项通过，46 项忽略，1 项原有失败：`platform::preview_window::tests::macos_workbench_regions_do_not_overlap_at_default_size`，断言 `layout.background.left >= layout.panel.left`。恢复本轮修改前源码后单独运行，仍在同一位置失败，基线复现日志随验收资料保存。
- 发布版独立启动及绘制抽样通过：100 次绘制移动、208 次采样，未变化区域的像素变化次数为 0。一轮检测因桌面被其他窗口遮挡而未进入采样，独立重跑通过。抽样日志用于检查绘制期间未变化区域，不作为帧率或完整性能验收。

## 截图和日志

验收资料位于 [acceptance/image-workspace-v14-20261009](acceptance/image-workspace-v14-20261009/)。

- [图片默认页](acceptance/image-workspace-v14-20261009/10-image-dark.png)
- [矩形属性](acceptance/image-workspace-v14-20261009/10b-image-rectangle-dark.png)
- [裁剪与数字箭头](acceptance/image-workspace-v14-20261009/11e-number-focused-dark.png)
- [浅色紧凑裁剪页](acceptance/image-workspace-v14-20261009/11d-crop-compact-light.png)
- [原生控件测试](acceptance/image-workspace-v14-20261009/field-review.txt)
- [图片操作测试](acceptance/image-workspace-v14-20261009/gesture-review.txt)
- [工作台验收](acceptance/image-workspace-v14-20261009/workbench-review.txt)
- [完整库测试](acceptance/image-workspace-v14-20261009/library-tests.txt)
- [原有失败基线复现](acceptance/image-workspace-v14-20261009/baseline-layout-test.txt)

- [发布版启动及绘制抽样](acceptance/image-workspace-v14-20261009/drawing-audit.json)
