# 视频属性与片段变速 — 2026-09-19

候选：`dist/Panzo-RE-MVP-video-speed-20260919/panzo.exe`。

后续更新：本文记录的预览 / 导出色差已在[色彩与稳定性修正版](Color-Stability-2026-09-19.md)中修复，原始测试结果保留。

## 操作与范围

- 属性面板新增固定的“画布 / 视频”标签，内容区域可滚动，隐藏页没有可误点的控件。卡片布局、宽高拖动、深浅主题和时间线低清缩略图保留。
- 画布页保留背景、边距、圆角、阴影和光标设置，作用于整个工程。视频页作用于选中的视频片段，提供 0.25×–4× 对数滑块、0.01× 数值精度、常用倍率、恢复 1× 图标，以及调整前后的时长。
- 明确选中视频片段时切换到视频页，移动播放头不切页。速度调整尽量保留播放头原来对应的源画面；分割继承速度，边缘拖动保持裁剪语义。
- 单次滑块拖动只产生一条撤销，Esc 取消；支持重做、保存与重开。时间线、镜头、光标和视频画面经同一时间映射处理，预览和导出共用。

## 数据与导出

`VideoClip.speedPercent` 使用 25–400 整数，100 为原速。旧工程缺省值为 100，保存升级 `edit/workbench.json` 到 schema 3；更旧构建拒绝回写未知版本。时间换算使用整数宽中间值，时长按 tick 向上取整，限制无效倍率和溢出，源区间仍为左闭右开。极端非整 tick 分割最多产生一个 tick 的时长取整差。

本轮实测发现 MF 生成的隐式时间 fMP4，6.6 秒 / 396 帧文件在 FFmpeg 中完整，但 Windows Reader 只读到 6 秒 / 360 帧。临时文件逐片段加入 `tfdt` 后恢复全部 396 帧。正式导出在提交前补齐该元数据、更新绝对数据偏移，并移除含旧偏移的可选 `mfra` 索引；视频编码载荷原样流式复制，最多使用 128 KiB 复制缓冲和有界的片段元数据缓冲。支持取消，失败不会发布输出。此步骤只处理导出暂存文件，录制与恢复链路不变。

Microsoft 文档说明 [MF 分片 MP4 sink](https://learn.microsoft.com/en-us/windows/win32/api/mfidl/nf-mfidl-mfcreatefmpeg4mediasink) 生成分片容器，[MPEG-4 支持说明](https://learn.microsoft.com/en-us/windows/win32/medfound/mpeg-4-file-sink)列出 Windows source 对片段的支持边界；本次 tfdt 修正依据本机前后对照验证，不将该现象推广到所有 Windows 版本。

## 验证

- [工作区回归](acceptance/video-speed-20260919/tests.txt)：188 项通过，包含速度取整、溢出、旧格式、保存 / 撤销、裁剪 / 分割、相应源帧与镜头 / 光标、播放期限，以及导出片段时间与数据偏移。
- [界面与媒体专项](acceptance/video-speed-20260919/ui-review.txt)：9 项通过，包括原生数值输入、滑块 / 撤销 / 取消、标签行为、视频边缘裁剪，及原有标题栏、卡片分隔、背景缩略图和镜头十字回归。
- [混合速度导出](acceptance/video-speed-20260919/mixed-speed-export.json)：6.6 秒、396 帧、1920×1080，Windows 完整解码且 PTS 递增。7 个采样点与原速编码结果对应源帧的平均 RGB 差异为 0–0.044/255。
- [深色面板](acceptance/video-speed-20260919/video-speed-Dark-96.png)、[高 DPI 浅色面板](acceptance/video-speed-20260919/video-speed-Light-192.png)已检查，另有 144 DPI 与其他主题组合的离屏绘制图。图中为合成测试素材。
- [Clippy](acceptance/video-speed-20260919/clippy.txt)、[格式](acceptance/video-speed-20260919/fmt.txt)、[release 构建](acceptance/video-speed-20260919/build.txt)记录随包。
- Release [混合速度拖动](acceptance/video-speed-20260919/final-mixed-speed-scrub.json)：媒体错误 0、松手精确定位、原尺寸输出、无降清帧；此轮与媒体导出同时运行，且短素材慢速段会重复源帧，不用于宣布完整性能矩阵通过。[共享合成 Hash](acceptance/video-speed-20260919/final-mixed-speed-render.json)对应原始日志确认相同。
- Release [剪辑与导出兼容检查](acceptance/video-speed-20260919/release-edit-compat.json)：源资产保留、分割 / 删除 / 裁剪、保存重开、不可变导出快照、同名保护、取消清理通过；原始尺寸 2560×1440 和 1080p 均完整解码 168 帧 / 2.8 秒。
- [独立 FFprobe 计数](acceptance/video-speed-20260919/ffprobe-mixed-speed.json)确认 396 帧 / 6.6 秒；FFmpeg 仅用于开发验证，不是应用运行依赖。

颜色对照中，未压缩 BGRA 预览到 NV12/H.264 成片仍有约 7.4/255 的平均 RGB 差异；原速对照也存在，已列入已知问题。本轮证明时间映射与效果同步，不宣称有损编码后逐像素相同。原生事件探针和离屏图不替代完整物理输入、长时间性能或跨 GPU 验收。
