# Panzo

Panzo 是 Windows 原生截图、图片处理、录制与视频编辑应用，使用 Rust 和 Windows 原生 API 开发。仓库保留源码、插件、编译资源和测试所需文件；设计原型、验收产物、用户配置及编译结果不纳入版本控制。

## 编译环境

- Windows x64。目标系统为 Windows 11，当前开发环境也在 Windows 10 上进行验证。
- Rust，通过 rustup 安装；仓库的 `rust-toolchain.toml` 固定所用工具链及 MSVC 目标。
- Visual Studio 或 Build Tools 的“使用 C++ 的桌面开发”组件，以及 Windows SDK。编译应用图标需要 SDK 的 `rc.exe`，MSIX 打包需要 `makeappx.exe`。
- 首次构建需要网络下载 Rust 工具链与 Cargo 依赖；无需配置任何 API Key。

## 获取、编译与运行

在 PowerShell 中执行：

```powershell
git clone https://github.com/smartai-Labs/Panzo.git
cd Panzo
cargo build --workspace --release --locked
.\target\release\panzo.exe
```

构建完成后也可以双击 `Start-Panzo.cmd`。开发模式启动：

```powershell
cargo run --locked -p panzo-app --bin panzo
```

主程序为 `target/release/panzo.exe`，命令行诊断工具为 `panzo-cli.exe`；两个插件也会随工作区构建生成。

## 插件

- `plugins/panzo-image-filters`：灰度、反色等图片处理示例。
- `plugins/panzo-media-tools`：音频导入、旁白录制、AI 字幕。
- `crates/panzo-plugin-api`：主程序与插件共享的通信协议。

构建后生成插件安装包：

```powershell
.\scripts\package-plugin.ps1 -Executable target/release/panzo-image-filters.exe -Manifest plugins/panzo-image-filters/plugin.json -OutputPath dist/panzo-image-filters.panzoplugin
.\scripts\package-plugin.ps1 -Executable target/release/panzo-media-tools.exe -Manifest plugins/panzo-media-tools/plugin.json -OutputPath dist/panzo-media-tools.panzoplugin
```

在应用顶部“插件”入口安装 `.panzoplugin` 文件。需要 AI 字幕或多模态识图时，由用户在设置中填写自己的服务地址、模型和 API Key；密钥在本机加密保存，不随源码提供。

## 测试

```powershell
cargo test --workspace --locked -- --test-threads=1
```

`fixtures/camera` 是自动运镜测试依赖的固定 JSON 样本。标记为 ignored 的桌面、录制等测试需按各测试说明准备环境，不属于上述默认测试的覆盖范围。部分原生测试需要交互式 Windows 桌面。

## 打包

生成带构建身份记录的 Release 候选：

```powershell
.\scripts\build-candidate.ps1 -TargetDirectory target
```

脚本允许下载缺失依赖；依赖已缓存时可添加 `-Offline`。然后生成 MSIX：

```powershell
.\scripts\build-msix-candidate.ps1 -BuildDirectory target/release -OutputDirectory dist -Version 0.1.0.0
```

默认 MSIX 未签名，不代表已完成安装验证。签名时按脚本参数提供自己的 Publisher 和 CertificateThumbprint；不要将签名证书或私钥提交到仓库。MSIX 包含主程序及 CLI，独立插件通过上述插件安装包安装。更严格的候选验证与归档流程见 [scripts/VALIDATION.md](scripts/VALIDATION.md)。

## 目录

- `crates/`：应用、核心逻辑、Windows 平台实现与插件协议，含所需图标资源和测试代码。
- `plugins/`：插件源码与清单。
- `fixtures/`：自动化测试所需的固定样本。
- `scripts/`：构建、打包和验证脚本。
- `target/`、`dist/`、`.tmp/`：本机生成内容，Git 忽略。

设计与历史验收资料仅保留在开发者本地，不是编译、打包或运行的前置条件。许可证见 [LICENSE](LICENSE)。
