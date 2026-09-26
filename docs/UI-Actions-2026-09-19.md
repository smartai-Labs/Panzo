# 编辑器按钮、镜头选择与定位修复 — 2026-09-19

候选：`dist/Panzo-RE-MVP-ui-actions-20260919/panzo.exe`。旧包保留。

## 界面

- 编辑器顶部、播放条、时间线及属性栏的操作按钮统一为图标，工具按钮采用 32 DIP 尺寸、统一间距。默认无边框，悬浮显示边框，按下有反馈；选中功能使用淡底色和蓝色图标。键盘焦点仍保留可见轮廓。
- 移除按钮内的“保存”“导出视频”“分割”“添加镜头”“居中焦点”等常驻文字。功能说明在悬浮后显示，离开即消失；中文辅助技术名称、快捷键保留。属性名称、数值和录制器的来源说明保留。
- 时间线功能区使用整行浅灰蓝背景和细分隔线；撤销 / 重做、剪辑操作、镜头效果分组，缩放操作在右端。
- 选中镜头使用淡紫填充和细轮廓。原来随轨道高度拉长的两条深色线改为靠边的短圆角标记；窄片段只显示轮廓，避免两根标记挤在中间；被视口裁掉的边缘不画假手柄。

## 定位错误

新增检查复现了两条会被界面显示为“视频定位未完成”的错误：

1. 已准备帧原先仅保存时间与求值结果，重绘时读取会被下一次定位替换的会话像素。复现得到 `SelectionMismatch { evaluator_tick: 0, decoded_tick: 438032959 }`。现在每个待显示帧持有对应像素的 `Arc`，重新定位不改变旧帧的内容，也不复制整帧像素。
2. 连续定位至 73%、12%、99% 时，Media Foundation 读取器在到达片尾目标前返回 EOS；在同一读取器上继续回退定位仍然失败。目标 `594013276`，最后读到 `178895147`，见诊断记录。现在遇到提前结束时在媒体线程重建读取器一次，从前置锚点重新解码；沿用原有请求取消、5 秒期限、分辨率和硬件解码策略。仍要求目标 PTS 精确一致，真正的素材错误不会被隐藏。

证据：[帧引用错误修复前](acceptance/ui-actions-20260919/seek-before.txt)、[提前 EOS 诊断](acceptance/ui-actions-20260919/seek-diagnosis.txt)、[修复后](acceptance/ui-actions-20260919/seek-recovery.txt)。单独 Flush 未解决复现问题，未将该尝试保留在最终实现中。

## 验证

- 165 项串行回归通过；4 项依赖本地工程的专项检查显式运行通过，覆盖多 DPI 布局、属性栏调整、时间线高度、悬浮离开状态、连续同步 / 异步定位、片尾、回到起点及拖动帧切换。
- 实际录屏工程副本和 30 分钟工程也通过同一连续定位检查。
- 格式检查、严格 Clippy、release 构建通过。
- 最终发布程序的 1440p 缓存就绪拖动检查通过：P95 92.490 ms，最大更新间隔 102.789 ms，媒体错误 0，降分辨率帧 0，松手精确定位、随后播放通过。
- 发布程序片尾定位 / 随后播放通过；带边距、圆角、阴影的预览与导出哈希一致；2 秒 1080p60 硬件导出通过，120 帧。
- 实际录屏素材的冷缓存拖动性能检查未通过：P95 199.375 ms，最大更新间隔 649.980 ms；媒体错误 0、降分辨率帧 0、松手精确定位和随后播放均正常。同一素材、无已完成缓存条件下，上一轮程序同样未达到门槛（P95 213.221 ms、最大间隔 373.547 ms）。保留两次失败，不用一次对比推断全部性能差异或宣布该问题已解决。

检查记录：[回归](acceptance/ui-actions-20260919/tests-final.txt)、[专项](acceptance/ui-actions-20260919/ui-review.txt)、[实际录屏定位](acceptance/ui-actions-20260919/seek-recording.txt)、[发布程序拖动](acceptance/ui-actions-20260919/final-scrub-warm.json)。

补充：[30 分钟工程定位](acceptance/ui-actions-20260919/seek-long.txt)、[发布程序片尾定位](acceptance/ui-actions-20260919/final-tail-seek.json)、[预览 / 导出一致性](acceptance/ui-actions-20260919/final-render-hash.json)、[导出](acceptance/ui-actions-20260919/final-export.json)、[新构建冷缓存拖动](acceptance/ui-actions-20260919/final-scrub-recording.json)、[旧构建对照](acceptance/ui-actions-20260919/baseline-scrub-recording.json)。

GUI SHA256：`098693B9F2891026E6855B9CA8D9D91A13E2B97BDF2E2BC5A02292B01FE3D9E3`。

CLI SHA256：`E4510D3E5EA266C36AB006A985F0061DED1CD32FD50752B4838E5E5C087E7919`。

## 绘制检查与边界

[正常状态](acceptance/ui-actions-20260919/editor-expanded-inspector-360.png)、[按钮悬浮及选中镜头](acceptance/ui-actions-20260919/editor-toolbar-hover.png)、[200% DPI](acceptance/ui-actions-20260919/editor-192-1280.png)均由应用真实绘制代码离屏生成，不是桌面截图。原生事件测试调用应用自己的事件处理器，不等于外部物理鼠标验收。

本轮修复已复现的两条定位错误，不据此关闭此前独立记录的代理 PTS 映射错误、冷态性能波动或全部设备兼容性问题。用户具体工程的诊断尚未提供，不能确认其错误只来自上述路径。完整录制流程、长时间资源与外部输入矩阵沿用之前的待验收状态。

修改前源码备份：`.tmp/ui-actions-backup-20260919-032152/crates`。
