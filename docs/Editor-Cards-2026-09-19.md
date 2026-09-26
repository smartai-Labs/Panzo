# 编辑器紧凑卡片布局 — 2026-09-19

候选：`dist/Panzo-RE-MVP-editor-cards-20260919/panzo.exe`。

## 布局与颜色

- 视频预览和播放控制组成一个卡片，属性区一个卡片，时间线一个卡片。圆角 6 DIP、卡片间距 6 DIP、窗口左右与底部留白 8 DIP，随系统 DPI 缩放。
- 深色模式采用中性灰：整体 `#1B1B1B`，三个卡片 `#272727`；标题栏沿用整体底色。浅色模式使用较深浅灰底与白色卡片，保留主题切换与原有设置。
- 时间线功能按钮、参数条、标尺、空轨道和留白使用相同卡片底色，片段仍保留视频 / 镜头的识别色。属性组改为同一卡片内的轻分隔线。
- 竖向窄缝调整属性卡片宽度；横向窄缝调整时间线高度，预览和属性卡片底边保持对齐。Esc 取消尺寸调整，收起属性区后预览卡片占满可用宽度。
- 时间线最高高度仍为 `max(窗口高度×22%, 244 DIP)`。工具栏由 52 降为 44 DIP、参数条由 44 降为 40 DIP、标尺与轨道间距收紧，下限由 244 降为 216 DIP，使普通窗口也能缩小时间线；扩高时两条轨道分配新增空间。
- 卡片底色在局部重绘前统一铺设，避免拖动时间线时局部擦成整体深色。视频的边距、圆角和导出样式不随 UI 卡片变化。

## 验证

- [工作区回归](acceptance/editor-cards-20260919/tests.txt)：180 项通过。现有布局矩阵补充卡片边缘对齐、窄缝命中、不同 DPI 和窗口尺寸的间距检查。
- [原生专项](acceptance/editor-cards-20260919/ui-review.txt)：6 项通过，覆盖宽高拖动、最小 / 最大限制、取消、收起展开、轨道空间分配、黄十字选点、保留预览帧、背景更换与低清缩略图。
- [严格 Clippy](acceptance/editor-cards-20260919/clippy.txt)、[格式](acceptance/editor-cards-20260919/fmt.txt)、[release 构建](acceptance/editor-cards-20260919/build.txt)通过。
- 已查看 [深色布局](acceptance/editor-cards-20260919/editor-Dark-96.png)、[浅色布局](acceptance/editor-cards-20260919/editor-Light-96.png)、[高 DPI 布局](acceptance/editor-cards-20260919/editor-Dark-192.png)及[背景 / 缩略图布局](acceptance/editor-cards-20260919/background-filmstrip-dark.png)，三个卡片边界、紧凑间距和统一时间线底色符合本轮要求。
- 最终同构建的 [原尺寸拖动检查](acceptance/editor-cards-20260919/final-scrub-warm.txt)通过：2560×1440 GPU 预览，247 次不同源帧更新，拖动响应 P95 105.131 ms；媒体错误、降分辨率帧、CPU 回读均为 0，松手后精确定位及后续播放完成。本次为缓存就绪检查，不替代历史冷态性能结论。[探针构建记录](acceptance/editor-cards-20260919/final-scrub-warm.json)。

最终 CLI SHA256：`F656C282FA4A169C9A281FE8AA730F6A2A35106ECA2E145C05840A8C37F742E5`。

绘制图使用测试素材，属于离屏检查；原生窗口专项由程序调用事件处理，不冒充完整桌面物理输入验收。历史媒体性能、跨 GPU 和完整 UI 验收边界保留。未改用户工程和已有视频。
