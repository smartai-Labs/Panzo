# 视频轨道高度与紧凑显示（2026-10-09）

启动：`target/desktop-v11.2/release/panzo.exe`。先保存当前工程并退出旧版（包括托盘后台），再打开新版。已安装媒体插件无需重装；本版包含 v11.1 的插件列表刷新修复。

## 行为

- 视频轨道最高为 72 DIP，随 Windows 显示缩放换算像素；继续拉大时间线区域时，下方保留空余空间。
- 视频轨道分配高度不足 44 DIP 时隐藏缩略图，切换为 20–28 DIP 的紧凑素材条，保留选中反馈、片段时长与两端裁切手柄。紧凑状态不再提交缩略图解码请求，已有缓存仍可在展开后复用。
- 普通视频工程与有旁白、音乐、字幕的工程使用相同高度上限；辅助轨道最高 40 DIP。上方视频预览仍根据其可用空间正常显示。
- 工程数据与导出尺寸不受这项显示调整影响。

## 验证

- `cargo test -p panzo-windows --offline --lib -- --test-threads=1`：227 通过，39 个需专用环境的测试按默认规则忽略。
- 几何回归覆盖 96 / 120 / 144 / 192 DPI、6 档轨道区高度、有无声音 / 字幕，检查高度上限、排列边界、命中区域与缩略图切换。
- 显式执行 `capped_and_compact_timeline_ui_review` 通过：使用合成视频工程副本，在 440 → 216 → 264 → 500 DIP 间切换，并检查缩略图恢复与两端裁切命中。实际原生绘制函数离屏输出深、浅色共 16 张效果图；这不是用户桌面的交互录制。
- 修改文件通过 rustfmt 检查。候选构建记录位于 release 目录的 `candidate-build.json`。

[高度与缩略图记录](acceptance/timeline-height-20261009/timeline-review.txt) · [拉大后的轨道](acceptance/timeline-height-20261009/timeline-media-false-500-dark.png) · [紧凑素材条](acceptance/timeline-height-20261009/timeline-media-false-216-light.png) · [声音与字幕轨道](acceptance/timeline-height-20261009/timeline-media-true-440-dark.png)
