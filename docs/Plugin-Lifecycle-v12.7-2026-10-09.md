# 插件可拔插逻辑审查与修复 · v12.7

日期：2026-10-09。审查范围：注册、安装、启用、停用、任务提交、进程隔离、更新、回退、卸载、失败清理，以及宿主与用户结果的存续。

**结论：独立插件使用按任务启动的工作进程。正常卸载某个插件不需要重启主程序，也不会重启或终止其他插件的任务。本轮补上了跨 Panzo 实例的任务保护，并通过实际发布版界面验证。**

## 当前哪些是真正的独立插件

| 功能 | 当前形态 | 卸载行为 |
|---|---|---|
| 声音、旁白与 AI 字幕的媒体处理/模型调用 | 独立 `.panzoplugin` 包，协议 2，独立 EXE | 可以安装、更新、回退、卸载；生成任务需要插件，已有音频/字幕的编辑与播放保留在宿主 |
| 外部图片处理，例如灰度/反色 | 独立 `.panzoplugin` 包，协议 1，独立 EXE | 可以安装、更新、回退、卸载；生成的图片独立保留 |
| 离线 OCR、多模态识图、拼图与批处理 | 随主程序编译的内置功能 | 当前只支持启用/停用，不能独立卸载或减少主程序体积 |
| 工作区、轨道、撤销、项目保存、既有素材播放/编辑 | 宿主基础能力 | 不随某个处理插件卸载而销毁 |

因此，“当前所有功能已经完成独立插件化”不成立。后续若提供真正按需下载的精简版，还要将内置功能继续拆包。目前媒体协议绑定 `panzo.media-tools` 的固定能力，不是任意第三方 UI 面板扩展接口。

## 生命周期规则

| 操作 | 对目标插件 | 对主程序与其他插件 |
|---|---|---|
| 安装 | 校验包、写入独立版本目录、更新登记，即时可用 | 保持运行 |
| 启用 | 允许接受新任务 | 保持运行 |
| 停用 | 立即阻止新任务；已经接受的任务使用原版本继续完成 | 保持运行 |
| 卸载空闲插件 | 移除当前/上一版本登记和对应程序目录 | 保持运行；保留用户素材、结果及工程 |
| 卸载正在工作的插件 | 拒绝卸载并解释原因；先完成或取消任务再卸载 | 不自动取消其他任务，不重启任何宿主 |
| 更新/回退 | 目标插件空闲时切换版本；运行时拒绝切换 | 其他插件的任务继续 |
| 插件取消/超时 | 只清理该请求的工作进程及其子进程 | 其他请求继续 |
| 插件异常退出/返回无效响应 | 本次任务报错，不提交无效结果 | 宿主与其他工作进程继续 |

插件工作进程在任务结束后退出，下一次任务启动新的工作进程。这是当前按任务执行的模型；没有要求所有插件常驻，更没有用重新启动全部插件来完成卸载。

## 发现并修复的缺口

原实现的 `running` 计数只在各自主程序进程内存中。一个 Panzo 实例正在处理插件任务，另一个实例重新读取插件登记后，其本地计数仍为零，可能移除登记并尝试删除在用版本。还有一个窗口期：任务先获得逻辑许可，实际执行时再从当前登记查找程序，版本选择没有固定在任务接受时。

修复措施：

1. **每个插件增加系统级共享任务锁。** 同一插件的多个任务可同时持有共享锁；更新、回退、卸载需要独占锁。所有新版本 Panzo 实例共享这一规则。
2. **提交任务前重新读取登记。** 检查当前是否已安装和启用，避免其他实例停用/卸载后，旧内存状态仍接受新任务。
3. **任务固定已接受的版本。** Lease 持有当时的安装元数据，图片与媒体执行器使用这份信息；停用和界面刷新不会改写已开始任务的执行对象。
4. **任务释放不依赖全局管理器锁。** 本地计数改为共享原子计数，任务结束自动释放系统文件锁；持锁进程异常结束时操作系统也会释放锁。
5. **完整性校验仍在工作线程进行。** UI 提交任务时只固定登记和任务锁，避免额外在界面线程读取并计算整个插件 EXE 的哈希。

注册表格式与图片/媒体协议保持兼容；已有独立插件包无需重装。任务锁文件的文件名由插件 ID 的哈希生成，并保留为稳定锁对象，避免删除后创建新的锁对象导致保护失效。

主要实现：[插件登记与任务 Lease](../crates/panzo-windows/src/platform/plugins.rs)、[图片执行器](../crates/panzo-windows/src/platform/plugin_runtime.rs)、[媒体执行器](../crates/panzo-windows/src/platform/media_plugin_runtime.rs)。

## 验证结果

### 真实发布版界面

使用 v12.7 EXE、全新隔离配置、两个测试插件和合成图片：

- 主程序 PID：**17916 → 17916**。
- 插件 B 的工作进程 PID：**74292 → 74292**；通过同一个进程句柄确认卸载期间一直存活。
- B 执行任务期间，在顶部插件管理窗口选择 A，执行并确认卸载。
- A 的登记与安装目录被移除；B 的登记不变，主窗口 HWND 仍能响应。
- B 继续完成，成功添加新图片。
- B 完成后再卸载 B，主程序继续运行，原图与新图片都保留。
- 测试结束正常退出，退出码 0。

[发布版原始证据](acceptance/plugin-lifecycle-v12.7-20261009/release/release-hotplug.json) · [可复现脚本](../scripts/test-plugin-hotplug-release.py)

两个测试插件预先装入隔离登记，测试通过真实界面执行卸载；此次 GUI 测试不冒充安装向导验收，也没有操作用户真实插件目录。

### 跨进程、异常与任务生命周期

生产登记/执行器代码的独立进程测试通过，覆盖：

- A 卸载时 B 保持同一个进程并完成。
- 同插件多任务共享 Lease，最后一个任务结束后才能卸载。
- 运行中拒绝更新、回退和卸载。
- 停用阻止新任务，已接受任务正常结束。
- 卸载后重新安装，原宿主继续提交新任务，无需重启。
- A 非正常退出、输出无效 JSON，不影响 B。
- 取消 A 时清理 A 及其真实子进程，不清理 B。
- 媒体协议 2 的工作进程超时，不影响 B。
- 另一个真实宿主测试进程持有任务锁时，本进程不能卸载或更新它。
- 该持锁测试进程被终止后，系统自动释放锁，可以正常卸载。
- 用户结果文件保持原样。

[进程测试证据](acceptance/plugin-lifecycle-v12.7-20261009/lifecycle-review.json) · [测试代码](../crates/panzo-windows/src/platform/plugin_lifecycle_review.rs)

原生单元测试 **236 通过、45 个需专项环境的测试跳过**；本轮单独执行的进程生命周期测试通过；独立构建的真实灰度/反色插件往返检查通过。[完整日志](acceptance/plugin-lifecycle-v12.7-20261009/windows-tests.txt)、[进程测试日志](acceptance/plugin-lifecycle-v12.7-20261009/process-tests.txt)、[图片插件日志](acceptance/plugin-lifecycle-v12.7-20261009/image-worker-tests.txt)。

## 交付与边界

- [主程序 v12.7](../target/desktop-v12.7/release/panzo.exe)
- [媒体插件包](../target/desktop-v12.7/release/panzo-media-tools.panzoplugin)
- [图片处理示例插件包](../target/desktop-v12.7/release/panzo-image-filters.panzoplugin)
- [构建身份](../target/desktop-v12.7/release/candidate-build.json)

修复在新主程序中生效；测试和构建没有重启用户正在运行的旧实例。升级时保存工作并正常退出旧版，再打开 v12.7。之后插件安装、启停、更新与卸载均按上述规则处理。跨实例锁需要参与实例都使用包含此修复的版本，旧版不能自动获得新的任务锁协议。

本轮没有迁移内置 OCR/识图/拼图为独立包，也没有改变它们的 UI 布局。没有访问真实 AI 服务、录制麦克风，或删除用户实际安装的插件。已有用户素材的保留经过图片流程实测；既有音频/字幕编辑导出不依赖插件的结论来自代码职责核对，本轮没有重跑完整音视频导出矩阵。

```powershell
cargo build --locked --offline -p panzo-windows --example plugin_lifecycle_fixture
$env:PANZO_LIFECYCLE_FIXTURE_EXE = (Resolve-Path target/debug/examples/plugin_lifecycle_fixture.exe).Path
$env:PANZO_LIFECYCLE_REVIEW_DIR = Join-Path (Get-Location) 'docs/acceptance/plugin-lifecycle-local'
cargo test --locked --offline -p panzo-windows independent_process_lifecycle_isolation --lib -- --ignored --test-threads=1
python scripts/test-plugin-hotplug-release.py --binary target/desktop-v12.7/release/panzo.exe --fixture target/debug/examples/plugin_lifecycle_fixture.exe --output docs/acceptance/plugin-lifecycle-local/release
```

`plugin_lifecycle_fixture` 仅用于隔离测试，不进入用户插件包或发布目录。
