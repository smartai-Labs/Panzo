# 编译、打包与运行核验（2026-10-11）

- Release 编译：build-candidate.ps1 成功；主程序、CLI、两个独立插件均生成。
- 工作区测试：cargo test --workspace --offline --locked -- --test-threads=1；374 通过、0 失败、55 跳过（需要额外桌面或环境配置的测试）。
- 打包：MakeAppx 验证并生成 Panzo-0.1.0.0-x64.msix；媒体和图片插件均成功打包。MSIX 未签名、未安装，安装流程不计入已验证范围。
- 运行：启动 MSIX 暂存目录中的实际 panzo.exe，使用隔离用户配置；图片、录制视频、素材库切换以及设置、插件窗口打开和关闭均通过，程序正常退出（退出码 0）。未启动录屏，也未结束用户已有的程序。
- 密钥检查：对 Git 候选文本扫描常见服务密钥、私钥、带凭据 URL、JWT、Bearer 和密钥赋值；匹配项经检查为本地回环模拟服务器测试值、example.com 拒绝凭据测试、键盘快捷键文案。未发现实际 API Key 或私钥。
- 提交排除：测试 profile/user/AppData、下载的 Rust 工具链、临时目录、编译产物及本地凭据文件通过 .gitignore 排除；不提交安装包。
- 本机结果：.tmp/git-release-validation/tests.log、startup.json、package.log；安装包及插件位于 dist/git-release-validation/。

这次核验覆盖当前环境的构建与启动，不代替签名安装、其他 Windows 设备或完整业务验收。
