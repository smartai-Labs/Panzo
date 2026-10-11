# 声音、旁白与 AI 字幕（桌面工作台 v11）

## 启动与安装

1. 运行 `target/desktop-v11/release/panzo.exe`。
2. 顶部「插件 → 安装本地插件」，选择同目录的 `panzo-media-tools.panzoplugin`，确认安装。该包为本次构建的独立媒体插件。
3. 打开视频工程，在「文件 → 声音与旁白 / 字幕与 AI 自动字幕」进入工作台侧栏；也可以从时间线效果菜单进入。两个入口可在「设置 → 快捷键」自定义，默认不占用新快捷键。
4. 在「设置 → AI 字幕」配置自己的服务，再在字幕侧栏选择音轨并点击三角形的 AI 生成按钮（悬停显示说明）。生成成功会替换字幕轨，支持撤销；失败或取消保留已有字幕。

## 本次能力

| 功能 | 操作与结果 |
| --- | --- |
| 音频导入 | 导入 WAV、MP3、M4A、AAC 等 Windows Media Foundation 能解码的文件；独立插件转成工程内的 PCM，保留原文件 |
| 旁白录音 | 从当前播放头位置开始，选择麦克风，三秒准备后录制；显示时长、电平，可停止或取消；本版录音时预览默认暂停 |
| 声音编辑 | 旁白、音乐两条显示轨；拖动片段移动，拖动两端裁切；支持入点、出点、位置、音量、淡入淡出、分割、删除；音量为 0 可静音 |
| 字幕 | 添加、编辑、分割、删除；导入 UTF-8 SRT，导出 SRT；字号、位置、颜色、背景和显示开关 |
| AI 自动字幕 | 选择视频原声 / 系统声音、录屏麦克风、后期旁白或当前混音；后台生成并显示进度，可取消，生成后直接在侧栏校对 |
| 时间线 | 波形和字幕片段与视频共用缩放、滚动和播放头；声音与字幕片段可移动、裁切，修改接入统一撤销和保存 |
| 预览与导出 | 新增音轨混音参与实时播放与 MP4 导出；字幕使用共用合成器，显示在画面中并烧录到导出视频 |
| 插件卸载后 | 已导入声音、字幕文字及编辑结果保存在工程中，仍可编辑、预览和导出；新导入、录音、AI 生成需要重新启用插件 |

后期声音和手工字幕按工程时间定位，调整视频顺序、速度后应检查位置。基于原声生成或导入的字幕绑定原始画面，随后裁剪和变速会重新映射。此版不包含自动压低音乐、逐字高亮、翻译配音、声音克隆或复杂多视频轨。

## 自定义 AI 服务

配置内容：API 地址、语音模型名称、API Key、返回格式、语言、术语提示和请求超时。

- 接口使用 OpenAI 兼容的 `POST /audio/transcriptions`，模型名称由用户填写。需要能接收音频的转写模型；仅支持文字或图片的模型无法替代。
- 支持 `verbose_json`、`srt`、`diarized_json` 返回时间戳。对于只返回文本的 `json`，将音频按五秒分段定位，并在结果中提示校对时间。
- 精确时间戳模式按最多六十秒分段处理；切点附近的词句需要校对。本版不提供词级强制对齐。
- 只在点击生成后发送所选音轨，不上传视频画面。远程地址使用 HTTPS；本机回环地址可使用 HTTP，方便接入本地兼容服务。
- `diarized_json` 请求自动携带 `chunking_strategy=auto`，不发送术语提示；模型和格式是否兼容由服务决定。格式应按服务说明选择。
- 配置保存在 `%LOCALAPPDATA%\Panzo\speech.json`。API Key 使用 Windows DPAPI 与当前账户绑定加密保存，不放进工程、插件包、命令行或日志；更换服务地址需要重新填写或删除旧密钥。设置中留空保留原密钥。

接口格式核对：[语音转文字指南](https://developers.openai.com/api/docs/guides/speech-to-text)、[Transcriptions API](https://developers.openai.com/api/reference/resources/audio/subresources/transcriptions/methods/create)。

## 插件实现与数据

`plugins/panzo-media-tools` 是独立二进制插件，清单协议为 2，声明 `audio.import`、`audio.record`、`speech.transcribe` 三种任务。宿主通过有界 JSON 管道调用经 SHA-256 校验的已安装程序，传递任务相关文件路径；取消和超时由 Windows Job Object 清理进程树。API Key 仅在用户启动转写时通过管道交给已信任的插件。

插件完成后，宿主校验返回内容并通过统一工程编辑事务提交。音频位于 `media/audio-clips/`，工作台声音、字幕和样式字段使用工程 schema 8。保存过的新工程应继续使用 v11 或更新版本编辑。媒体服务不直接改写工程编辑状态。

媒体插件可单独分发；宿主保留现有工程的基础编辑、混音与字幕合成。当前协议仅支持单个原生 EXE，仍需信任插件作者，尚未提供发布者签名或安全沙箱。协议 1 图片插件保持兼容，详见 [插件开发](Plugin-Development.md)。

## 验证与边界

验收使用隔离配置目录、合成视频和正弦音频，以及本机模拟语音服务，没有调用用户 API 或上传用户素材。

- `cargo test --workspace --offline --no-fail-fast` 串行运行：319 项通过；39 项需专项环境的测试默认跳过。并行运行曾遇到旧的硬件编码器占用断言，串行复测通过。
- 真实窗口检查插件安装、模型与加密密钥保存、音频导入、参数修改、拖动移动 / 裁切、撤销重做、录音倒计时取消、字幕生成和校对、异常时间戳、取消网络任务、SRT 导出、工程重开和插件停用。
- 导出六秒 MP4 后重新解码：180 帧（30 fps）；声音在片段范围外静音，范围内音量正确；字幕在有效时间出现，结束后消失。
- 麦克风实际采集、不同厂商线上模型的识别准确率和完整长视频性能仍需真实环境验证，本机模拟服务仅验证调用约定及工作流。

发布 EXE 和独立媒体插件的真实窗口复测为 12 项通过，重启前后两次退出码均为 0。最新导出解码证据在 `export-release/export-proof.json`。

**退出兼容性待跟踪**：发布版首轮测试在退出时发生一次 `0xc0000005`；Windows 应用事件指向 `C:\Program Files\DoubaoIME\versions\v0.9.0.0\tsf-oime-core.dll`。同一二进制随后完整复测和两次退出正常，尚未确认根因，也未宣称已修复。首轮失败记录保留为 `native/release-first-run-failure.json`，最终复测记录为 `native/media-smoke.json`。

证据位于 `docs/acceptance/media-v11-20261009/`。复测脚本：

```powershell
$env:RUST_TEST_THREADS = '1'
cargo test --workspace --offline --no-fail-fast
python scripts/test-media-release.py --binary target/desktop-v11/release/panzo.exe
$env:PANZO_MEDIA_PROJECT = (Get-Content docs/acceptance/media-v11-20261009/native/media-smoke.json -Raw | ConvertFrom-Json).project
$env:PANZO_MEDIA_OUTPUT = Join-Path $PWD ('docs/acceptance/media-v11-20261009/export-' + [guid]::NewGuid())
cargo test -p panzo-windows --offline media_audio_subtitle_export_roundtrip -- --ignored --nocapture
```

原生脚本依赖本地生成的合成 fixture 路径，换环境需先提供等价的六秒以上 Panzo 测试工程。脚本只关闭自身启动的隔离进程。
