# Panzo 工作台 v14 原生改造

日期：2026-10-09。按 [v14 原型与布局规格](UI-Layout-Review-v14-2026-10-09.md) 实施，继续使用 Rust / Win32 / GPU 预览及现有工程、撤销、插件模型。

[启动 v14](../target/desktop-v14/release/panzo.exe) · [媒体插件](../target/desktop-v14/release/panzo-media-tools.panzoplugin) · [图片滤镜插件](../target/desktop-v14/release/panzo-image-filters.panzoplugin)

旧版仍在运行时，先正常保存并退出旧版，再运行上面的 v14。已安装的插件和本机加密保存的模型配置继续沿用。本次测试使用独立配置与合成项目，没有操作用户工程或调用用户 AI 服务。

## 原型落地

| 范围 | 原生实现 |
|---|---|
| L01、L09：外壳与状态 | 标题菜单 32 DIP、居中工作区导航 44 DIP、上下文栏 40 DIP；下划线标识工作区；保存状态紧随左侧折叠按钮；缩放与居中在同一行，导出位于右上方 |
| L03、L08：视频结构 | 增加可搜索、可收起的当前项目素材栏；正常宽度 212，较窄 180；小于等于 1000 时自动收起，再展开覆盖预览。GPU 可见区域同步裁剪，素材栏不会被视频盖住 |
| L04、L10：统一属性宿主 | 画面、片段、声音、字幕使用同一宿主与分隔线，底边停在全宽轨道上方；默认 304 DIP，紧凑 284，可拖至 264—420；原布局参数行不再占用轨道 |
| L03：片段编辑 | 入出点进入片段属性，视频速度与裁剪归所选片段；画面页前置尺寸、比例、背景和边距，高级圆角、阴影、光标折叠；保留原有事务及撤销 |
| L05：声音 | 声音页只做导入、用途、设备、电平与录制，主操作固定底部；选择音轨后在“片段”编辑时间、音量、淡入淡出；分割、删除仍在轨道 |
| L06：字幕 | AI、导入、新增前置；当前字幕文字和时间优先；全部字幕及样式折叠；应用与导出操作固定底部，切换字幕先校验并保存有效草稿 |
| L08：轨道 | 视频行 62 DIP，紧凑窗口 48，轨道区域低于 200 时素材条 32 并隐藏缩略图；其他轨道 38/32。容器不足时垂直滚动，滚轮与拖动滚动条可用，选择媒体时将所在行带入可见区 |
| L01、L02：图片 | 普通图片使用顶部文档标签和左侧 44 DIP 工具栏；右栏按绘制、裁剪、OCR 或识图切换；删除重复底部文档缩略图；标签支持溢出选择、关闭，同一源文件重新打开激活已有文档 |
| 图片裁剪、拼图 | 裁剪比例直接选择、在原图拖手柄，底部固定确认/取消；拼图使用专属缩略条与末尾加号，四宫格/九宫格保持严格对齐，预览内选图、缩放、拖动定位，参数区可滚动，生成固定底部 |
| L07：素材 | 分类 / 网格或列表 / 有限宽预览三栏；目录为图标与临时路径编辑；保存筛选、选择和滚动位置；双击进入对应工作区；新增声音分类、真实波形和试听、按播放头添加音频；缺失文件可重新定位 |
| 全局能力保留 | 设置、插件仍是独立窗口。旧折叠菜单的编辑能力迁入“编辑 → 标注、跟随与混音…”，工程打开、位置、视图折叠与适应窗口保留在顶部菜单 |

宿主负责唯一布局和选中状态，媒体处理与 AI 字幕继续通过现有插件执行。声音、字幕、音频片段页面可缓存以保留任务，但只有一个属性宿主显示它们；没有建立第二套并行工作台或把 HTML 原型嵌入应用。

## 实机截图

截图来自原生程序与合成素材，原型中的山景演示素材不属于应用功能。

- [视频工作台](acceptance/workbench-v14-native/final/01-video-dark.png)
- [画面](acceptance/workbench-v14-native/final/02-canvas-dark.png)、[声音](acceptance/workbench-v14-native/final/03-audio-dark.png)、[字幕](acceptance/workbench-v14-native/final/04-subtitles-dark.png)、[音频片段](acceptance/workbench-v14-native/final/05-audio-clip-dark.png)
- [紧凑轨道](acceptance/workbench-v14-native/final/06-compact-tracks-dark.png)、[紧凑声音面板](acceptance/workbench-v14-native/final/07-compact-audio-dark.png)、[覆盖式项目素材](acceptance/workbench-v14-native/final/08-compact-bin-dark.png)
- [浅色字幕](acceptance/workbench-v14-native/final/09-subtitles-light.png)
- [图片](acceptance/workbench-v14-native/final/10-image-dark.png)、[裁剪](acceptance/workbench-v14-native/final/11-crop-dark.png)、[OCR](acceptance/workbench-v14-native/final/12-ocr-dark.png)、[拼图](acceptance/workbench-v14-native/final/13-collage-dark.png)、[素材](acceptance/workbench-v14-native/final/14-assets-dark.png)

## 验证

- 工作区库测试 **333 通过、0 失败**：panzo-core 88、plugin-api 2、panzo-windows 243；另有 46 个专用环境测试默认忽略。本次增加/调整了高 DPI 几何、紧凑轨道、媒体格式、PCM 波形、缺失素材定位、图片文档与新页面测试。[日志](acceptance/workbench-v14-native/workspace-tests.txt)
- v14 原生专项单独运行：14 个场景，验证单一属性宿主、字幕与音频修改及撤销、固定轨道高度、滚动、GPU 覆盖裁剪、同图重复打开、真实拼图生成和素材加载。[日志](acceptance/workbench-v14-native/final/run.txt)
- 发布 EXE 插件隔离：卸载 Alpha 后宿主 PID **57996** 和 Beta 任务 PID **78864** 保持不变；Beta 完成后可卸载，源图及生成图片保留。[记录](acceptance/workbench-v14-native/release-hotplug-verified/release-hotplug.json)
- 25 个变更 Rust 文件格式检查通过。[记录](acceptance/workbench-v14-native/format-check.txt)
- 发布版资源检查通过：预热后 100 次设置/插件窗口开关，GDI 54 → 54、USER 166 → 165；30 秒空闲采样为 0.0% 单核，正常退出且无新崩溃转储。[记录](acceptance/workbench-v14-native/release-resources/resources.json)。

构建源码 SHA-256：`049705EEF4D23D0DE92248A2257913BD8A480325E5D310C9FCBE66BA6A7ADD07`。

EXE SHA-256：`843a230ca421e827f6fcedbd110f82c66a5a1b6f0fc10993140ea632664afae8`。[完整构建身份](acceptance/workbench-v14-native/candidate-identity.json)

早期截图检查发现并修复了 GPU 预览覆盖窄屏素材栏、拼图容器漏绘、图片面板折叠及异步文档绑定问题；早期记录保留在同级 `review-*` 目录，最终证据以 `final` 为准。插件探针首次遇到 Windows 原子替换注册表文件时的短暂读取冲突，探针已增加有限重试，不修改应用存储；最终隔离证据以 `release-hotplug-verified` 为准。

## 复现与边界

```powershell
cargo test --locked --offline --workspace --lib -- --test-threads=1
powershell -ExecutionPolicy Bypass -File scripts/build-candidate.ps1 -TargetDirectory target/desktop-v14
python scripts/test-workbench-v14-native.py --project docs/acceptance/ui-v13-20261009/verified-inspector-142836/fixture.panzo --package target/desktop-v14/release/panzo-media-tools.panzoplugin --output .tmp/v14-native-new-run
python scripts/test-plugin-hotplug-release.py --binary target/desktop-v14/release/panzo.exe --fixture target/debug/examples/plugin_lifecycle_fixture.exe --output .tmp/v14-hotplug-new-run
python scripts/test-ui-resource-lifetime.py --binary target/desktop-v14/release/panzo.exe --output .tmp/v14-resources-new-run
```

原生专项会复制工程、创建独立配置并自动关闭自己的窗口；输出目录需不存在。插件隔离脚本的 fixture 是仓库内专用测试程序。媒体插件包构建方式见 `scripts/package-plugin.ps1`。

当前机器为 Windows 10；Windows 11、混合 DPI、多显示器拖动、系统文字放大和 Narrator 仍需按 [发布验收矩阵](UI-Release-Gates.md) 补齐。此次没有调用真实 AI 服务或进行真实麦克风录制，不以布局回归代替识别质量、长时间录音或商业发布认证。
