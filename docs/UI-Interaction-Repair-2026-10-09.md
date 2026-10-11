# Panzo 原生界面与交互修正

日期：2026-10-09。对应 [问题筛查](UI-Interaction-Audit-2026-10-09.md)。本次已实施全部 12 项调整，以及审查发现的命令路由、草稿、后台任务和编辑事务问题。

启动：`target/desktop-v4/release/panzo.exe`。旧版 v3 保留，不替换用户正在运行的进程。构建身份见该目录的 `candidate-build.json`。

## 逐项交付

| 要求 | 当前行为 |
| --- | --- |
| 1. 原生控件统一 | 录制浮条、展开的下拉列表、颜色浮层、文字和素材滚动条、上下文菜单使用共享主题。输入/输出框取消原生亮边、立体边和白色滚动条。保留 IME 与文字选择。 |
| 2. 绘制闪烁 | 静态内容缓存、交互预览与持久离屏缓冲分离；完整画面一次提交，画布禁止先擦背景。切换工作区后完整补画。 |
| 3. 目录 | 共用“文件夹图标 + 路径”；图标选择目录，点击路径临时编辑，Enter 提交、Escape 恢复。批处理、素材库和导出接入；导出文件名独立可编辑，切换格式保留名称。 |
| 4. 设置与插件 | 主窗口所属的唯一弹窗，重复打开激活现有窗口。关闭保留本次会话草稿；任务完成更新原面板，不自动弹窗。 |
| 5. 顶部命令 | 文件、编辑、视图、帮助、插件、设置按顺序排列，右侧太阳/月亮切换主题。标题栏与主体同色。 |
| 6. 工作区位置 | 菜单下固定图片工具、录制与视频、素材三个标签；移除内容区重复品牌。 |
| 7. 深浅主题 | 深色主体改为中性层次，按钮去绿色，主操作用柔和蓝色；浅色白色主体、浅灰辅助区。 |
| 8. 画布视图 | 每张图独立缩放和平移；滚轮围绕鼠标缩放、空格拖动、适应窗口、100%。视图不改变导出尺寸或撤销记录。 |
| 9. 单图标 | 截图、打开、长截图、拼图、批处理、保存和工具按钮采用单图标，并保留提示、名称与禁用状态。 |
| 10. 素材预览 | 单击预览，空格展开，双击编辑。图片缩放、GIF 播放/暂停、视频封面/播放/暂停/定位。递归发现和外部文件登记，按可见项加载缩略图，无旧 120/500 项限制。 |
| 11. 裁剪 | 初始整图边界、八个手柄、比例/尺寸、确认/取消；确认形成一次撤销，全图裁剪保留最后一行/列。 |
| 12. 工具属性 | 文件标签下的属性区随工具和所选对象更新；可重新编辑已有文字，样式预览与正式提交分离。 |

## 同步修正的交互缺陷

- 保存、撤销、复制按文本焦点和当前工作区分发；视频页不会操作后台图片。
- 选择对象不再清空重做；Escape、失去捕获和工具切换正确结束手势。
- 表单按工程和对象区分，保留草稿，避免多个视频标注共用参数；重入回调不会在执行中被销毁。
- OCR 结果按来源版本提示是否过期；失败时明确保留旧结果，不自动重复付费请求。
- 文档缩略图跟随编辑修订更新；素材列表保留筛选、选择和滚动，缺失/解码失败有明确状态。
- 打开视频工程选择工程目录，无需寻找内部 JSON；录制、导入、批处理和两条导出路径均登记实际完成的文件。
- 设置弹窗在 Windows 10 的标题栏回退路径补齐不透明绘制，修复系统按钮重影。

## 验证证据

- 真实工作区流程：[报告](acceptance/ui-repair-20261009/workspace/workspace-review.json)，检查图片/视频/识别状态保留、独立管理窗口、托盘恢复及小窗口布局。
- 设置草稿与隐藏任务：[表单回归](acceptance/ui-repair-20261009/workspace-form-regression.json)。任务关闭后完成不会自动打开窗口。
- 原生控件：[验收及深浅主题截图](acceptance/ui-fixes-20261009/controls-review.md)，包括实际展开列表、颜色面板、录制浮条。
- 素材：[验证记录](acceptance/ui-repair-20261009/asset-review.json)，覆盖 145 项实际列表、超过 500 项索引、GIF 和合成 MP4 解码/定位。
- 录制链路：[暂停与运镜证据](acceptance/ui-repair-20261009/record-export-final/pause-proof.json)，仅录制应用生成的 320×180 合成图，验证暂停移除、时间戳单调及运镜开关。未录制音频。
- 图片编辑原生回归覆盖手势取消、撤销/重做、既有文字编辑、裁剪、缩放，以及 16 种旋转/翻转的文字预览与最终像素一致性。

- 全工作区 `cargo test --locked --offline --workspace --all-targets`：**296 通过，0 失败，38 项需显式条件的测试默认跳过**；其中与此次改造有关的原生窗口、录制、插件及图片手势测试另行运行。见 [完整日志](acceptance/ui-repair-20261009/unit-tests-final.txt)。
- [插件完整工作流](acceptance/ui-repair-20261009/workspace-plugins/workspace-review.json)：安装、处理图片、停用、卸载和数据保留通过，`plugin_ui_checked=true`。
- [发布版本窗口检查](acceptance/ui-repair-20261009/release/ui-smoke.json)：直接启动交付 EXE，验证目录临时编辑、设置草稿、唯一弹窗、菜单路由、裁剪和两种主题，进程正常退出。
- [发布版本连续绘制](acceptance/ui-repair-20261009/drawing-release/drawing-audit.json)：4000×2500 合成图片，750 次移动，连续绘制约 12.65 秒；约 13.99 秒内 1400 次像素采样，异常采样 **0**。基线为 280 次采样中 217 次出现画布背景色。测试结束后只清理自身隔离进程；绘制测量与正常退出验收分开记录。
- 最终 EXE SHA-256：`8547FE4D2A43B8EAEBE6C95A1DCFD52A63BBA3F0A01D6E8D491DBF4AFEEC7EBA`。构建前后及验收结束后核对 158 个源文件/配置与二进制，均匹配构建清单。

真实截图：[深色与裁剪](acceptance/ui-repair-20261009/release/image-crop-dark.png) · [浅色](acceptance/ui-repair-20261009/release/image-light.png) · [设置弹窗](acceptance/ui-repair-20261009/release/settings-directory-dark.png) · [素材预览](acceptance/ui-repair-20261009/release/assets-dark.png) · [导出目录与中文文件名](acceptance/ui-repair-20261009/record-export-final/export-gif-panel.png)。

复验交付 EXE：

```powershell
python -X utf8 scripts/test-ui-repair-release.py --binary target/desktop-v4/release/panzo.exe --output .tmp/ui-v4-review
python -X utf8 scripts/test-drawing-stability.py --binary target/desktop-v4/release/panzo.exe --output .tmp/drawing-v4-review
```

两个脚本均使用隔离配置和合成图片，不调用外部识图服务；运行时保持测试窗口可见。

## 明确的行为边界

- 素材视频是已标注的静音快速预览；双击进入编辑器播放音频。工程条目预览明确标注为原始录制，不宣称已渲染所有编辑效果。
- 更改素材库目录下次启动生效；不会隐式搬动现有文件。未保存的设置草稿保留到本次应用退出，点击保存后才长期生效。API Key 继续由当前 Windows 账户加密保存。
- 绘制采样不是帧率或帧数统计，异常采样为零不能排除采样间隔内极短闪帧。实际多显示器混合 DPI、完整读屏与不同 GPU/音频硬件不作为本次已通过的范围。
