# 第四阶段：图片工具与视频标注

日期：2026-10-08。对应集成方案 D 阶段，继续以减少额外软件安装为目标。

候选程序：[panzo.exe](../target/integrated-stage-d/release/panzo.exe)。开发启动：`cargo run -p panzo-app --bin panzo`。

## 功能与入口

| 任务 | 入口与操作 |
|---|---|
| 截图识别文字 | 首页「识别文字」→ 框选 →「识别」/ Enter → 选择本机语言。结果窗口支持校对、编辑、复制。图片编辑器工具栏与贴图右键也有「识别文字」。 |
| 滚动长截图 | 首页「图片工具」→「滚动长截图」→ 框选滚动内容 →「长图」/ Enter。可手动滚动后采集下一屏，或开启自动下滚采集；支持暂停、撤回、预览、完成。 |
| 修正拼接 | 遇到无变化、重叠不明确或尺寸变化会停止自动采集并显示原因。失败帧保留在内存，填写与上一屏重叠的高度后点「手动拼接」；也可撤回重采。 |
| 图片拼图 |「图片工具」→「图片拼图」，多选 PNG/JPEG/BMP。支持纵向、横向、2/3/4 列，最长边、间距、背景、原顺序/反序。结果回到图片编辑器继续标注、裁剪、复制、保存和贴图。文件选择器的返回顺序作为排列顺序。 |
| 批量压缩/转换 |「图片工具」→「批量压缩 / 格式转换」。选择 JPEG、PNG 或 BMP，设置最长边、JPEG 质量及输出目录。后台显示进度，可取消；结束报告成功数量、体积和逐文件失败原因。 |
| GIF 桌面播放 | 首页「导入媒体」选择 GIF，直接创建动态贴图。右键或 Space 播放/暂停；「提取当前帧编辑」进入静态图片编辑器。复制/保存输出当前帧。 |
| 贴图分组 | 贴图右键「设置分组」；「图片工具」→「贴图分组管理」统一显示、隐藏、排列或恢复交互。组名、位置、播放状态与 GIF 资源一并保存。 |
| 视频标注 | 视频编辑器左上角菜单 →「标注、跟随与声音」→「视频标注」。新增、选择编辑或删除文字、箭头、矩形、椭圆、荧光和遮挡。设置素材起止时间、百分比坐标、颜色、线宽/字号及位置参照。 |

主界面调整为八项功能菜单，新入口包含「图片工具」「识别文字」。短窗口会压缩卡片高度，100%/125%/150%/200% 的卡片几何布局有自动检查。前三阶段的录制、声音、缩放开关、编辑和导出继续保留。

## 行为与数据

OCR 使用 Windows 本地识别能力，图片不会上传。截图已经做过的遮挡会进入识别输入。中文相邻字之间的 OCR 分词空格会合并，英文空格和换行保留。识别结果必须校对：本机样例中的「别」曾被识别为「另刂」，报告保留真实结果，不将任务执行成功记作文字准确率满分。超出系统单次识别尺寸限制时提示先裁剪。

长截图固定捕获所选物理矩形。控制条启用排除捕获；系统不能排除时不启动。自动模式会把鼠标移到选区中心并发送滚轮输入，中心被其他窗口或控制条挡住时暂停并提示。固定页眉、滚动条、视频和动画应尽量排除在选区外。当前支持向下的竖向滚动，未实现横向滚动、固定页眉自动剔除或任意网页的无重叠拼接。

批处理保留源文件。输出先写独立临时文件，再以不覆盖已有目标的方式发布；重名报告失败，已完成文件在取消后保留。PNG 保留透明度；JPEG 的透明区填白。PNG 无损重新编码不保证比原文件更小，BMP 通常用于格式兼容。

GIF 隐藏或暂停时停止播放计时和刷新，恢复显示后继续；按各帧延时循环，最短延时 20 毫秒。原 GIF 复制到工程库 `.panzo-pins/assets`，删除导入源文件后仍能恢复。GIF 单文件最多 64 MB、500 帧、96 MB 解码数据；贴图总数和解码资源继续受 24 张/256 MB 预算限制，持久化资产不超过 512 MB。拼图最多 64 张，批处理每次最多 500 张，长截图最多 64 屏；图片结果最大 6400 万像素，采集缓存最大 256 MB。

视频标注使用素材时间的半开区间 `[start,end)`；剪辑、变速复用已有时间映射。「固定画布」适合标题，「跟随素材」随镜头/裁剪变换，并限制在视频内容矩形区域。当前通过属性窗口编辑坐标，尚无视频画布上的鼠标拖拽标注。预览、当前帧截图、MP4 和 GIF 共用标注合成器，支持撤销/重做、保存重开。工作台 schema 升至 7，兼容读取 1–6；原视频不改写。

## OCR 安装能力与打包

「图片工具」→「OCR 语言管理」可检查本机语言，按需安装简体中文、繁体中文或英语的 Windows 离线识别能力。安装由用户主动选择后触发系统 UAC，首次通常需要联网；完成后重新打开管理窗口检查。程序本次验证没有改动机器的语言能力。

同包脚本 `scripts/manage-ocr-capability.ps1` 默认只检查已安装语言。管理员可使用 `-Install -Language zh-CN` 安装；企业离线环境可指定 `-OfflineSource`，路径须提供与系统版本匹配的 Windows 语言能力包，调用 `-LimitAccess` 禁止访问在线源。

微软对 Windows.Media.Ocr 桌面应用的正式支持要求包身份。因此新增 `scripts/build-msix-candidate.ps1`，从源码身份匹配的候选构建 MSIX，包含应用、语言能力管理脚本和依赖许可证；可通过已有证书指纹签名。脚本不会生成信任证书或安装包。[OCR 官方说明](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr?view=winrt-26100)、[包身份说明](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/package-identity-overview)、[Windows 语言能力](https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/add-language-packs-to-windows?view=windows-11)。

```powershell
./scripts/build-candidate.ps1 -TargetDirectory target/integrated-stage-d
./scripts/build-msix-candidate.ps1 -BuildDirectory target/integrated-stage-d/release -OutputDirectory dist/stage-d
# 正式签名时指定证书对应的 Publisher 与 CertificateThumbprint。
```

本机便携候选已验证能够离线 OCR。正式安装分发仍需签名证书以及干净 Windows 环境安装/升级验收；未把便携版在本机运行成功当作所有系统的安装保证。

## 本机验证

- `scripts/verify.ps1`：256 项自动测试通过，28 项专用环境测试默认跳过（含 golden 测试 1 项）；格式检查、严格 Clippy、WGC/D3D11/H.264 与 BGRA→NV12 探针通过。日志：`.tmp/stage-d-verify1.log`。
- 后续原生验证辅助代码的最终严格 Clippy：`.tmp/stage-d-final-clippy.log`。
- OCR：三个独立工作线程重复识别生成的中英文图片，结果一致；真实结果窗口可编辑并可复制。记录：`.tmp/stage-d-native-04/ocr-proof.json`，截图 `ocr-result-window.png`。识别准确率限制见前文。
- 自动长截图：三屏合成 320×480，结果与生成的完整页面逐像素相等；无变化拒绝、手动重叠修正、撤回恢复通过。记录：`.tmp/stage-d-native-04/scroll-proof.json`。
- GIF 与分组：双帧 400ms 循环、暂停/隐藏计时、关闭恢复；删除原 GIF 后，在新进程从本地资产恢复组名与播放状态。记录：`.tmp/stage-d-native-04/image-tools-proof.json`；日志：`.tmp/stage-d-native4.log`、`.tmp/stage-d-fresh-restore.log`。
- 拼图/批处理：尺寸与颜色、透明处理、缩小不放大、原文件不变、同名不覆盖、取消及临时文件清理；原生任务生成拼图和两张 JPEG。记录：`.tmp/stage-d-native-04/collage.png`、`batch-report.txt`。
- 视频标注：生成素材裁去前 0.5 秒后设为 2 倍速，标注撤销/重做/保存重开；预览/导出哈希一致；30 FPS MP4 与 15 FPS GIF 再解码验证出现/结束时间和遮挡颜色。记录：`.tmp/stage-d-video-01/annotation-proof.json`。
- GPU 测试另覆盖标注缓存失效、结束时不残留、跟随素材的缩放与固定画布参照区别。
- A/B 原生回归：区域捕获、MP4/GIF 输出、截图标注与贴图恢复通过，日志 `.tmp/stage-d-regression-a.log`、`.tmp/stage-d-regression-b.log`。20 次真实编辑器往返通过：`.tmp/stage-d-navigation.log`。

构建身份与归档证据：[validation.json](acceptance/integrated-stage-d-20261008/validation.json)。Release 源码身份校验通过，带标注工程的 Release 预览/导出哈希一致；第三阶段空闲窗口已正常退出，第四阶段窗口已启动并响应，PID 49780。可查看 [首页](acceptance/integrated-stage-d-20261008/home.png)、[OCR 结果窗口](acceptance/integrated-stage-d-20261008/ocr-result-window.png)、[视频标注样例](acceptance/integrated-stage-d-20261008/annotated-preview.png)。

MSIX 已由 Windows SDK MakeAppx 校验并构建：`dist/stage-d/Panzo-0.4.0.0-x64.msix`，约 5.3 MB，当前未签名、未安装。打包报告：`dist/stage-d/Panzo-0.4.0.0-x64.msix.json`。此包是待签名构建产物；本机试用使用前述 EXE。

## 仍需验收

不同应用的滚动行为、动态内容/长页面、混合 DPI 实际跨屏操作、复杂 GIF 的长期资源占用、不同 Windows OCR 语言/版本准确率、MSIX 签名安装升级仍需专项验证。视频标注第一版使用属性窗口；交互式拖拽与独立标注时间轨可继续完善。前三阶段列出的多设备音画同步、跨 GPU、长时间录制及异常断电恢复边界继续有效。
