# Panzo 插件开发（图片协议 1 / 媒体协议 2）

这是第一阶段试用接口。宿主与插件通过标准输入/输出传递 UTF-8 JSON；一次进程处理一次请求。协议不传递 Rust 类型、内存地址或窗口句柄。插件可以使用其他语言实现同样协议，但当前包只部署单个 Windows 原生 EXE。

## 从示例构建

```powershell
cargo build --locked --offline -p panzo-image-filters --release
.\scripts\package-plugin.ps1 `
  -Executable target/release/panzo-image-filters.exe `
  -Manifest plugins/panzo-image-filters/plugin.json `
  -OutputPath dist/plugins/panzo-image-filters-0.1.0-windows-x64.panzoplugin
```

首次开发环境需先下载 Cargo 依赖；之后可离线构建。`panzo-image-filters` 是独立二进制 crate，宿主只依赖 `panzo-plugin-api` 的数据协议与校验规则。打包脚本同时生成包的 `.sha256.json` 文件。正式发布候选使用 `scripts/build-candidate.ps1`，输出目录由 `-TargetDirectory` 指定。

## 清单与容器

实际模板见 `plugins/panzo-image-filters/plugin.json`，字段为：

| 字段 | 约束 |
| --- | --- |
| id | 稳定插件 ID，3–80 位小写 ASCII 字母、数字、点、短横线；`panzo.builtin.*` 保留 |
| name / publisher / description | 功能名、作者声明和说明，限长且不可含控制字符 |
| version | 三段数字，如 `0.1.0` |
| protocol | 图片为 `1`，媒体为 `2`，必须与调用类型一致 |
| target | 当前 x64 包为 `x86_64-windows`，宿主拒绝架构不匹配的包 |
| commands | 1–16 个 `{id, name}`，同一插件的操作 ID 不重复 |
| executable_sha256 | 打包时自动计算，运行前复查 |

`.panzoplugin` 是无 BOM 的 JSON：`{"format":1,"manifest":{...},"executable_base64":"..."}`，不是 ZIP。未知字段会拒绝，以避免误认为支持未实现的协议能力。包内不支持任意文件路径、安装脚本、动态依赖或额外资源。

## 工作进程协议

从 stdin 一直读取至 EOF，解析一个请求；stdout 只写一个响应 JSON，日志不要写到 stdout。处理完成后正常退出；宿主先关闭 stdin，读取有界响应并确认退出状态。

请求示意（图片数据省略）：

```json
{
  "protocol": 1,
  "request_id": "由宿主生成的唯一 ID",
  "document_id": 42,
  "revision": 7,
  "command": "grayscale",
  "image": {"width": 1920, "height": 1080, "bgra_base64": "..."}
}
```

成功响应：

```json
{
  "protocol": 1,
  "request_id": "原样回传请求 ID",
  "image": {"width": 1920, "height": 1080, "bgra_base64": "..."},
  "error": null
}
```

失败响应的 `image` 为 `null`，`error` 为可读说明。宿主验证协议、请求 ID、像素尺寸及实际数据长度；无效或异常退出的结果不修改原图。像素为逐行紧密排列的 BGRA，四字节一像素，无额外行距；示例保留 alpha。数据通过标准 Base64 编码。

总像素不得超过 4,194,304；单个请求/响应不得超过 24 MiB；EXE 最多 32 MiB，包文件最多 48 MiB。超限直接拒绝，不自动缩小图片。宿主提供 60 秒时限及取消处理，并通过 Windows Job Object 清理后台进程；这不是安全沙箱。

结果作为新的工作区图片交给宿主，并保存至素材库。插件应保持纯处理行为，不直接编辑用户工程、宿主设置或素材文件。图片协议 1 不传递 API Key；在线识图仍使用宿主内置的加密设置。媒体协议 2 的转写请求会通过管道传递用户配置的语音服务密钥。

## 本地验收

```powershell
$env:RUST_TEST_THREADS = '1'
cargo test --locked --offline --workspace
$env:PANZO_PLUGIN_REVIEW_PACKAGE = (Resolve-Path dist/plugins/panzo-image-filters-0.1.0-windows-x64.panzoplugin).Path
cargo test --locked --offline -p panzo-windows independent_worker_round_trip -- --ignored --nocapture
```

真实桌面回归还需一个隔离测试工程路径 `PANZO_UI_REVIEW_PROJECT`、输出目录 `PANZO_UI_REVIEW_DIR`、隔离插件目录 `PANZO_PLUGIN_REVIEW_DIR`，再运行 `persistent_workspaces_keep_documents_and_job_ownership -- --ignored --nocapture`。这些隔离配置仅在测试编译中生效。

插件 ID 相同的本地包视为更新；保留上一版本供回退。停用阻止新任务，已在执行的任务完成后才能更新、回退或卸载。v12.7 起，任务接受时固定插件版本，并持有跨 Panzo 实例的共享系统锁；更新、回退和卸载必须获得目标插件的独占锁。操作只作用于目标插件，不重启宿主或其他插件；任务结束和持锁进程退出会自动释放锁。详见 [生命周期验证](Plugin-Lifecycle-v12.7-2026-10-09.md)。设置和包长期保存在用户本地应用数据中；卸载包不删除用户素材。作者声明和 SHA-256 尚未构成发布者签名，协议 1 仅适用于用户明确信任的原生代码。


## 媒体协议 2

参考 `plugins/panzo-media-tools/plugin.json`、`crates/panzo-plugin-api/src/media.rs` 和独立程序 `plugins/panzo-media-tools`。

请求外层为 `{ "protocol": 2, "request_id": "…", "task": { "kind": "…" } }`。任务有三种：

| kind | 清单 command | 输入 / 输出 |
| --- | --- | --- |
| `decode-audio` | `audio.import` | `source`、`output` 绝对路径；新建指定 PCM，返回 `frames` 和最多 2048 个 `waveform` 峰值 |
| `record-voice` | `audio.record` | `output`、可选 `device`、`stop_file`、`meter_file`、`seconds`；读取停止信号，返回 PCM 和波形 |
| `transcribe` | `speech.transcribe` | `audio` WAV、`duration_seconds`、`service`；返回 `segments`（`start`、`end` 秒及 `text`）和是否粗略定位的 `approximate` |

响应外层为 `{ "protocol": 2, "request_id": "…", "output": { "kind": "audio" 或 "subtitles", … }, "error": null }`。字段及命名以共享 Rust 协议的 serde 定义为准。失败返回 `output: null` 和错误说明。宿主校验请求 ID、时间区间、波形范围、PCM 实际字节数与工程参数后才提交。

音频资产统一为 48 kHz、双声道、16 位小端 PCM，无文件头。转写输入由宿主从选定音轨混音生成 16 kHz 单声道 WAV；返回字幕时间相对于这一段输入。请求最多 64 KiB，响应最多 4 MiB。解码超时 600 秒，录音最多一小时；转写按用户配置的请求时限执行并可取消。

`service` 包含 `base_url`、`model`、`api_key`、`response_format`、`language`、`prompt` 和 `timeout_seconds`。不得记录密钥。宿主提供的媒体插件只在显式转写操作中访问配置的服务，禁止重定向以避免凭据转送。

这期固定绑定 `panzo.media-tools` 插件的三种命令，未开放任意第三方编辑器面板扩展。协议 2 的独立程序复用 Windows 媒体基础库；宿主保留结果的编辑和播放能力，插件停用后工程仍可读取。

```powershell
cargo build --locked --offline -p panzo-media-tools --release
.\scripts\package-plugin.ps1 `
  -Executable target/release/panzo-media-tools.exe `
  -Manifest plugins/panzo-media-tools/plugin.json `
  -OutputPath dist/plugins/panzo-media-tools-1.0.0-windows-x64.panzoplugin
```
