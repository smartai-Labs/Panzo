# 色彩一致性与变速稳定性 — 2026-09-19

候选：`dist/Panzo-RE-MVP-color-stability-20260919/panzo.exe`。

## 修正内容

1. 录制、导出及拖动预览缓存的 BGRA → NV12 转换显式指定完整范围 RGB 输入、BT.709 视频范围输出，并关闭驱动自动画面处理。编码器输入与输出均写入 BT.709 原色、传递函数、矩阵、视频范围及色度采样位置。
2. 拖动预览缓存更新为 `scrub-v3-native-bt709`，旧版有色差的缓存不会继续使用，按需重新生成。时间线仍用 160 像素低清缩略图，主预览和导出仍保留源尺寸。
3. 精确定位经过所有回退锚点仍失败时，在媒体工作线程上重建一次解码器并重试。提前 EOS 同样走该恢复路径；原有取消信号、5 秒请求期限与精确 PTS 检查保留，错误帧不会当作成功返回。
4. 撤销分割导致选中片段消失时，属性面板重新绑定播放头所在的有效片段；清除失效镜头选择，避免速度控件空白或按钮仍绑定旧 ID。

## 原因与前后对照

旧转换没有明确指定颜色矩阵与范围。[转换前检查](acceptance/color-stability-20260919/nv12-before.txt)实测黑色的 Y 为 0，而视频范围应为 16。未标记范围的成片经解码后，RGB 32 的深灰变为 19；修正后为 33。独立色块测试覆盖黑白、两档灰阶、六个饱和色及蓝橙色，既检查原始尺寸转换，也检查缩小输出。颜色空间定义参考 [Microsoft DXGI 文档](https://learn.microsoft.com/en-us/windows/win32/api/dxgicommon/ne-dxgicommon-dxgi_color_space_type)，媒体标记参考 [MF 扩展色彩信息](https://learn.microsoft.com/en-us/windows/win32/medfound/extended-color-information)；本机范围失配由实际像素测量确认。

[Windows 色块往返](acceptance/color-stability-20260919/color-roundtrip.json)最大通道误差为 1/255；[独立 FFmpeg 解码与 FFprobe 元数据检查](acceptance/color-stability-20260919/independent-color-check.json)最大通道误差从 14/255 降至 2/255，输出标记为 `tv / bt709 / bt709 / bt709`。已检查[解码色块图](acceptance/color-stability-20260919/color-ffmpeg-after.png)。FFmpeg 只用于开发验证，不是运行依赖。

[混合速度成片](acceptance/color-stability-20260919/mixed-speed-export.json)使用同一蓝色测试工程，7 个采样点预览至 MP4 平均 RGB 误差从上一轮约 7.4/255 降至 0.24～0.29/255；新增小于 1/255 的回归断言。2× / 0.5× / 1.25× 共 6.6 秒、396 帧，Windows 全部解码，时间戳递增且片尾完整。有损 H.264 与色度采样仍不承诺逐像素相同。旧文件中已经编码的颜色不会自动还原；重新导出的文件使用新转换。

镜头拖动回归两次复现了定位失败：要求的源 PTS 为 98352991，现有解码器返回 98552323，尝试回退到开头仍失败；新建读取器的连续解码和定位均包含正确的 98352991。[失败日志](acceptance/color-stability-20260919/seek-before.txt)、[独立时间戳核对](acceptance/color-stability-20260919/seek-diagnosis.txt)及[修复后复验](acceptance/color-stability-20260919/seek-after.txt)保留。该证据支持本机 Reader 状态异常的恢复修正，不泛化为所有历史定位问题都已关闭。

## 验证结果

- [工作区测试](acceptance/color-stability-20260919/tests.txt)：189 项通过，13 项需要显式环境的测试默认忽略；其中本轮显式运行的[界面和媒体专项](acceptance/color-stability-20260919/ui-review.txt)为 11/11 通过。
- [30 分钟工程压力检查](acceptance/color-stability-20260919/speed-stability.json)：108 次连续变速、192 个可被后续请求替代的定位请求、64 次完成的随机精确定位、跨速度边界及终点的 15 个播放帧；保存重开、终点重定位、保留帧内容一致，媒体错误 0。[源视频、事件和原始镜头 Hash](acceptance/color-stability-20260919/source-preserved.json)不变。变速后的工程约 51 分钟；这是定位与持久化检查，不是连续播放 51 分钟。
- 原生事件专项覆盖 0.25×、2×、4× 镜头拖动、源区间换算、撤销和重做；复现并修复撤销分割后速度控件失效。[修复前证据](acceptance/color-stability-20260919/selection-before.txt)。原有主题、卡片分隔、标题栏、背景图片、低清缩略图及十字选点专项也通过。
- [Release 混合速度拖动](acceptance/color-stability-20260919/final-mixed-speed-scrub.json)原始日志确认媒体错误 0、松手精确定位、2560×1440 输出、无降清帧。独立执行，报告记录一次外部输入干扰；P95 请求到呈现约 112 ms、最大呈现间隔约 171 ms，不能据此宣布完整性能门槛通过。
- [Release 剪辑与导出](acceptance/color-stability-20260919/release-edit-compat.json)：裁剪 / 分割 / 删除、保存重开、导出快照、同名保护、取消清理与源资产保护通过。2560×1440、1920×1080 均完整解码 168 帧 / 2.8 秒；采样误差约 0.47～0.89/255。
- [共享合成 Hash](acceptance/color-stability-20260919/final-mixed-speed-render.json)对应日志确认一致；[严格 Clippy](acceptance/color-stability-20260919/clippy.txt)、[格式检查](acceptance/color-stability-20260919/fmt.txt)、[release 构建](acceptance/color-stability-20260919/build.txt)通过。

当前证据来自本机 NVIDIA H.264 硬件编码器。完整物理输入、长期性能和跨 GPU 验收边界仍以统一验收报告为准；本轮没有增加裁切旋转、录音或自动保存等后续功能。
