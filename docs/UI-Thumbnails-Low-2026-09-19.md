# 恢复低清时间线缩略图 — 2026-09-19

候选：`dist/Panzo-RE-MVP-ui-thumbnails-low-20260919/panzo.exe`。

按用户要求，时间线恢复原来的代理读取方式和固定 160 像素低清缩略图，解码器缓存恢复为 2 MiB。取消本轮新增的原视频缩略图解码、Lanczos3 重采样、按显示尺寸重新生成及缩略图优先于代理准备的调度。

时间线最高高度限制、连续排列和片尾裁切、背景实际图片预览及前几轮主题 / 属性栏修订保留。缩略图可见缓存仍有数量上限，以容纳紧凑轨道。此调整只影响时间线缩略图，不降低主视频预览与导出分辨率。

验证记录：[工作区回归](acceptance/ui-thumbnails-low-20260919/tests.txt)、[原生专项](acceptance/ui-thumbnails-low-20260919/ui-review.txt)、[低清缩略图比对](acceptance/ui-thumbnails-low-20260919/low-resolution-filmstrip-review.txt)、[Clippy](acceptance/ui-thumbnails-low-20260919/clippy.txt)、[release 构建](acceptance/ui-thumbnails-low-20260919/build.txt)。专项检查将缩略图与独立 160 像素代理解码结果比对，并检查不同 DPI 下仍使用低清缓存，以及背景更换、撤销 / 重做。

169 项回归和 5 项显式专项检查通过，格式、严格 Clippy 及 release 构建通过。素材检查实际返回 160 × 90 缩略图，与独立低清解码的像素一致；96 / 144 / 192 DPI 下仍使用 160 像素缓存。

离屏界面检查不代替完整桌面物理输入验收；此前媒体性能和设备兼容性边界保留。旧版本和原始工程未覆盖。
