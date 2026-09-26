# 项目整理记录（2026-09-27）

本次只整理开发产物、修正入口与脚本参数，没有修改应用源码或增加功能。

## 已清理

- 根目录的 11 个 `target-*` 历史构建目录。
- `target/` 下的旧候选与旧 `debug`、`release` 构建缓存；仅保留 `editor-feedback/` 当前候选及其缓存。
- `dist/` 中 19 组旧候选目录及对应压缩包。包内工程路径下的文件已与工作区原件逐一比对，93 个文件均有相同原件。
- `.tmp/` 中已完成任务的一次性脚本、临时日志、截图、修改前源码快照和旧构建缓存。

清理清单共 245 个条目、78,595 个文件，文件长度合计约 **36.27 GiB**。这是文件大小统计，不是文件系统实际回收空间的测量值。逐项路径、原因、大小及删除状态见 [JSON 明细](Project-Cleanup-2026-09-27.json)。本次执行用的临时清理脚本也在完成后移除，不作为长期维护工具保留。

## 保留内容

| 目录或文件 | 用途与保留原因 |
| --- | --- |
| `crates/`、Cargo 配置与锁文件 | 应用源码、依赖与构建基线 |
| `scripts/`、`fixtures/` | 可复用的验证脚本与自动测试样本 |
| `target/editor-feedback/` | 当前运行版本、CLI、调试符号、构建身份与最新构建缓存 |
| `docs/`、根目录规范与验收文档 | 使用说明、设计依据、验收证据和已知问题 |
| `.stitch/DESIGN.md` | 打包脚本仍引用的历史设计资料 |
| `outputs/panzo-ui-direction.html` | 界面设计参考，不是构建缓存 |
| `.tmp/` 中的工程、视频及包含这些文件的目录 | 曾存放真实录制，也包含回归样本；部分验证脚本仍引用它们，不能仅因目录名为临时目录而删除 |

未访问或清理用户视频库中的录制工程。清理过程没有关闭应用进程。

## 入口和维护规则

- README 的启动入口已改为 [当前候选](../target/editor-feedback/release/panzo.exe)，不再指向 9 月 19 日的旧包。
- `.gitignore` 补充 `/target-*/` 和 `/dist/`，防止生成文件进入以后的版本管理。
- `verify-prototype-ui-scrub.ps1` 的 `-Executable` 改为必填，避免默认运行已删除的历史 EXE。示例：`./scripts/verify-prototype-ui-scrub.ps1 -Executable ./target/editor-feedback/release/panzo-cli.exe -ProjectRoot <测试工程目录>`。
- 历史文档中的构建、压缩包和临时日志路径保留为当时记录，不表示对应文件仍存在；当前启动入口以 README 为准。
- 今后构建统一放在 `target/` 下，打包产物放在 `dist/`；过期产物可以清理，但应保留正在使用的程序及待复现问题对应的版本。
- `.tmp/` 不宜一键整目录清空。录制工程、视频、未保存编辑恢复数据需先确认用途，测试用日志和构建缓存可以按任务清理。

如需重新生成当前候选，在项目根目录执行：

```powershell
./scripts/build-candidate.ps1 -TargetDirectory target/editor-feedback
```

## 验证结果

- 清理前后 `Get-PanzoCandidate` 验证均通过：当前源码与构建记录匹配，`panzo.exe`、`panzo-cli.exe` 的 SHA-256 未改变。
- `cargo metadata --offline --locked --no-deps` 成功解析全部 3 个工作区成员。
- 修改后的 PowerShell 验证脚本语法检查通过。
- 应用源码未改动，本次未重新编译或重跑媒体/UI 测试；此前测试结论见 [编辑器操作反馈修复记录](UI-Editor-Feedback-2026-09-27.md)。
