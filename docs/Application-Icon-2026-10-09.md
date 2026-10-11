# Panzo 应用图标 · 2026-10-09

启动 [v12.1](../target/desktop-v12.1/release/panzo.exe)。先保存当前工作，从系统托盘退出旧版，再启动新版；插件和用户设置可继续使用。

![Panzo 应用图标](../crates/panzo-windows/assets/panzo-128.png)

沿用 P 标识，采用蓝灰色圆角底与白色字形。SVG 为源文件，包含透明圆角，输出 16、20、24、32、40、48、64、96、128、256 像素的 PNG / ICO，以及 MSIX 使用的 44、50、150 像素 PNG。

- GUI 与 CLI 的 EXE 内嵌 Windows 图标资源（101），供资源管理器、快捷方式和固定到任务栏时使用。
- 顶层窗口设置大、小图标，任务栏和 Alt＋Tab 不再采用系统默认标识；窗口跨屏时按 DPI 更新。
- 托盘采用同一图标。窗口共享有界缓存，主题切换不反复创建图标句柄。
- MSIX 打包复用相同素材。

重新生成图标：

```powershell
cargo run -p panzo-windows --example generate_app_icon --offline
```

验证结果：

- `cargo check -p panzo-app --offline`、发布构建和新增 Rust 文件格式检查通过。
- Windows 原生 API 加载全部 10 个尺寸成功；GUI / CLI 的图标资源与 Shell 图标提取通过。
- 使用发布库创建独立隐藏测试窗口，验证窗口大、小图标均为自定义图标，重复应用主题复用相同句柄。未操作已有用户窗口。
- ICO 尺寸、PNG 透明度与 MSIX 素材尺寸检查通过；未执行 MSIX 安装。

若以前固定的是旧版 EXE，请将新版重新固定到任务栏。
