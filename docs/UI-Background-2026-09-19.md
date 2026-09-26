# 时间线高度、背景图片和原视频缩略图 — 2026-09-19

候选：`dist/Panzo-RE-MVP-ui-background-20260919/panzo.exe`。上一轮程序保留。

## 本轮修改

- 时间线最高占客户区高度的 22%，与参考截图接近：2560 × 1417 时上限为 311 像素。较小窗口保留 244 DIP 的最小操作高度。拖动、窗口缩放和 DPI 变化共用限制；视频与镜头轨道共同分配高度，底部间距保持固定。
- 背景图片区域此前只画了图标和文件名。现在显示实际图片，整块预览可点击更换，悬浮时显示操作图标和边框。支持横图、竖图和透明 PNG；更换、撤销、重做后重新显示对应图片，保留上一画面直到新图片载入完成。
- 时间线缩略图直接读取原视频的精确索引帧，以原始分辨率解码，再按轨道实际像素尺寸进行 Lanczos3 缩放。取消固定 160 像素解码与预览代理依赖；更改轨道高度 / DPI 后重新生成，片尾不足一格时正确裁切。
- 缩略图按视频比例连续排列，小轨道不再因固定最小格宽留大块空隙。生成工作在后台进行，可见帧缓存受数量和 64 MiB 字节预算限制；播放、拖动与导出优先。
- 冷缓存录屏检查暴露了缩略图解码与预览缓存编码争用资源的问题。现在先加载可见缩略图，再准备流畅预览缓存；后台失败会延时重试，不在窗口线程等待。

## 检查

- 169 项工作区串行回归通过，格式检查和严格 Clippy 通过。
- 5 项显式原生专项通过，覆盖布局事件、连续定位保留帧、编辑器 / 录制器绘制、背景和原视频缩略图。
- 合成素材和实际录屏副本分别检查：所有可见缩略图的 PTS 对应原视频索引；300 / 600 像素缩略图与独立原分辨率解码后缩放的像素一致。
- 实际录屏未完成预览缓存时首次检查失败，记录保留在 `recording-filmstrip-initial.txt`；调整调度后，新的冷缓存副本专项在 3.59 秒内通过（整个专项耗时，非单次输入延迟）。
- 背景导入、更换、透明竖图、撤销 / 重做及深浅主题离屏绘制通过。测试只写入 `.tmp` 中的工程副本，不更改原工程或用户主题设置。

记录：[回归](acceptance/ui-background-20260919/tests-final.txt)、[原生专项](acceptance/ui-background-20260919/ui-review.txt)、[冷缓存录屏](acceptance/ui-background-20260919/recording-filmstrip-cold.txt)、[像素一致性](acceptance/ui-background-20260919/native-filmstrip-review.txt)。

绘制图：[深色](acceptance/ui-background-20260919/background-filmstrip-dark.png)、[浅色](acceptance/ui-background-20260919/background-filmstrip-light.png)、[透明竖图](acceptance/ui-background-20260919/background-replaced-portrait-alpha.png)。图片由应用绘制代码离屏生成，不是桌面截图；实际录屏图像仅留在本地临时目录，不随候选包分发。

最终 release 程序的 [12 秒缓存就绪拖动](acceptance/ui-background-20260919/final-scrub-warm.json)通过：P95 63.068 ms，最大画面间隔 79.372 ms，389 个不同源帧，媒体错误 0、降分辨率帧 0，松手精确定位和随后播放完成。[预览 / 导出哈希](acceptance/ui-background-20260919/final-render-hash.json)一致。

GUI SHA256：`2681FE664BDB0C16F1976D07E28B61939F36E92762131D5A97BC5712451C25AF`。

CLI SHA256：`55EFADFD968D51F4555670F6F52AA1EE8C69313D3480A691A34D7263B04A38B1`。

此前冷态拖动性能、偶发代理时间戳与跨设备验收边界继续保留。本轮缩略图检查不代表完整桌面物理输入或媒体性能验收通过。

修改前源码备份：`.tmp/ui-background-backup-20260919-044224/crates`。
