# 临时文件清理（2026-10-11）

- 已移除 1291 个清理目标，共 71613 个文件；文件逻辑大小约 37.05 GiB，执行前后磁盘可用空间增加约 34.42 GiB（硬链接和系统其他写入可能造成差异）。
- 清理内容：没有项目引用的一次性排查脚本、测试日志、重复测试素材和临时配置；嵌套测试临时目录、Python 缓存；46 个历史候选构建的编译中间目录。
- 保留内容：全部源码和正式回归测试；文档引用的验收资料；全部候选程序、插件、构建说明；当前版本及运行中版本的完整构建目录；常用 debug/release 缓存；用途未确认的 .tmp/naturalness 录制项目。
- 已校验：263 个源码和测试脚本的 SHA-256 未变；209 个候选版本 EXE 的 SHA-256 未变；Git 已跟踪文件没有删除；Start-Panzo.cmd 的目标程序存在。
- Git 忽略规则：.tmp/ 现在覆盖嵌套目录，新增 __pycache__/、*.pyc、*.pyo，防止临时产物再次混入待提交文件。
- 清理清单：.tmp/workspace-cleanup-20261011/removed.csv；保留清单：同目录 preserved.csv。

此次未修改应用功能，不需要重新构建。
## 后续：历史构建目录清理

用户确认仅保留最新版本、debug、release 后，已确认旧版本进程退出，并删除 target 下其余 57 个目录、13,393 个文件，逻辑大小约 11.32 GiB，磁盘可用空间增加约 10.11 GiB。

现在 target 仅有三个目录：

- desktop-v14-library-popup-fix（最新启动版本）
- debug
- release

Cargo 根目录的 .rustc_info.json 和 CACHEDIR.TAG 元数据保留。当前 panzo.exe 与 Start-Panzo.cmd 的校验值未变。README 已更新当前程序和媒体插件包入口，移除旧候选启动链接；历史验收文档中的版本路径保留为历史记录。媒体插件包已使用最新版本目录中的现有可执行文件重新打包，无需重新编译。

详细清单：.tmp/workspace-cleanup-20261011/historical-builds-plan.csv；校验结果：同目录 historical-builds-result.json。
