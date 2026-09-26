# 单主窗口与审查修复：开发记录

日期：2026-09-26。对应[开发方案](Next-Development-Plan-2026-09-26.md)。本轮只重组、修复和优化现有功能。

## 已实施的行为

- 录制准备和编辑器复用一个业务主窗口、同一 UI 线程和消息循环。无参数启动与指定工程启动均走该宿主；录制完成后在原窗口进入编辑。
- 标题栏共用设置、新录制、打开工程、保存、导出和系统窗口按钮。新录制返回准备区，关闭才退出；导出设置和进度改为主窗口内的子面板。
- 打开工程的文件读取、时间索引、背景和首帧准备移到后台。准备成功后替换当前工程；失败或按 Esc 取消保留当前工程。已授权离开后拦截编辑输入和过期 UIA 操作。
- 打开工程、新录制、退出复用同一未保存处理。保存期间可继续编辑；保存失败或保存期间产生新修改时取消原跳转。Esc 取消等待中的跳转，不中断正在进行的安全保存。
- 销毁旧会话时清理预览子窗口、定时器和无障碍 provider；异步渲染通知按会话代际过滤。草稿写入不阻塞 UI，取消离开后恢复写入，旧排队快照不能复活。
- 录制正常收尾和中断恢复共用必要文件补齐规则。处理媒体已改名、Processing 缺锁、旧 Recovered 缺文件等状态；保留有效人工编辑和已完成媒体。
- MP4 检查按 box 头和有界元数据读取，不读取完整 mdat；代理时间映射共享像素缓冲，避免整帧深拷贝。
- 导出面板从启动时的同一编辑快照冻结规格；修复裁剪捕获丢失与比例锁 UIA 状态；统一按钮、tooltip、状态严重程度与时间码测量。
- 验收脚本显式处理子进程退出码、超时、缺报告和缺必测项。构建记录源码与两个 EXE 的身份；打包必须显式指定匹配的候选和验收报告。

## 问题项与证据边界

“已实施”表示代码已经落地，不代表原方案列出的全部实机关闭条件均已完成。

| 问题项 | 已实施范围 | 尚需保留的验收边界 |
|---|---|---|
| R01、R02 | 恢复补齐、收尾顺序、状态和锁处理；异常状态单元测试；最终 Release 恢复后编辑／保存／导出闭环 | 新候选的长时间真实录制及多阶段强杀矩阵 |
| P01、P02 | 流式容器检查、共享像素；异常 box 与共享引用回归 | 大媒体峰值内存与冷缓存长尾性能对照 |
| S01、S02、S03 | 单主窗口、返回与退出分离、可取消导航和保存状态协调 | 完整用户操作、跨显示器与外部物理输入 |
| V01、V02、V03、V04、L01 | 公共标题栏、固定导航、公共按钮与提示、显式反馈语义 | 深浅主题全部状态与 DPI 的完整视觉矩阵 |
| U01、U02、U03、U04 | 裁剪取消、导出快照、Toggle 状态、字宽布局 | 外部 UIA 客户端、捕获转移及导出全过程的人工复验 |
| E01、E02、E03、L02 | 验收失败传播、构建身份、打包约束、调用方路径解析 | 只有同候选完整验收通过后才可产生正式包 |

## 验证记录

当前候选为 `target/release/panzo.exe`（同目录含 `panzo-cli.exe`）。使用 `scripts/build-candidate.ps1 -TargetDirectory target` 重新构建两个 Release 程序并冻结源码／二进制身份，未使用历史候选默认目录。身份副本见[构建记录](acceptance/unified-20260926/candidate-build.json)。目前是开发候选，未通过全部发布门槛，未生成正式发布包。

- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked --offline -- -D warnings` 通过；常规工作区测试 **223 passed，19 ignored by default**。显式忽略项不计入 223。日志见[检查结果](acceptance/unified-20260926/quality-results.json)及同目录 `fmt.log`、`clippy.log`、`tests.log`。
- 真实主窗口专项通过：**20 次**打开／返回均保持 HWND、宿主对象和 UI 线程；无效工程保留旧会话，旧 provider／渲染通知失效。另覆盖加载期间滚轮与排队 UIA 门禁、保存中 Esc、保存失败、保存后还有新修改，以及正常保存允许离开的正控。[完整记录](acceptance/unified-20260926/navigation-review.json)。
- 验收脚本最小回归 **17/17**，包含非零退出、超时进程树、缺报告／必测项、嵌套失败、身份不符、中文及空格路径。[回归记录](acceptance/unified-20260926/validation-regression.json)。
- 真实标题栏专项通过：普通／最大化／还原、按钮命中、拖动区、悬浮提示、最小化还原、取消点击与导出面板打开。[检查记录](acceptance/unified-20260926/native-titlebar-review.txt)及同目录三张 `native-titlebar-*.png`。
- 录制区离屏绘制通过，深浅主题及 96/144 DPI 图片保存在同一证据目录；其性质是绘制检查，不替代真实窗口视觉验收。原始记录：`.tmp/unified-ui-render/check-results.md`。
- 最终 Release 的短程统一回归 **13 Pass / 9 Fail**。通过项包括首尾定位、排队播放、暂停重绘、20 次生命周期、剪辑／导出画面对照及 10 个保存故障注入。6 个拖动用例未通过，另有 3 个录制／依赖录制的用例因捕获预检失败未完成。[完整报告](acceptance/unified-20260926/short-regression/automated-regression.json)。保留失败，未降低门槛或将警告过滤后记为通过。
- 单独使用最终 Release 对缺少 `recording.lock` 和 `tracks/camera.json` 的 Processing 工程副本执行恢复。结果 Recovered，随后剪辑、源文件保护、保存重开、快照冻结、取消导出清理和两种规格完整解码均通过。输入是已有真实录制素材的故障副本，不是本轮新录制。[恢复结果](acceptance/unified-20260926/recovered-scan.json)、[编辑与导出结果](acceptance/unified-20260926/recovered-edit-export.json)。
- `verify-recorder-ui.ps1` 改用公共子进程执行器，避免 Windows PowerShell `Start-Process` 的空退出码；独立 0／17 退出夹具通过。[夹具结果](acceptance/unified-20260926/recorder-exit-fixture.json)。重试录制验收正确报告预检失败，原始诊断保留在 `recorder-recheck.stdout.txt`。

## 未关闭事项

1. Computer Use 抓取真实窗口两次失败，返回 `SetIsBorderRequired / 0x80004002`；同次操作附近应用日志记录 `TextShaping.dll / 0xc000041d`。之后隔离的原生窗口与离屏检查未复现，原因尚未确定。不能据此宣称外部桌面自动化和完整人工验收通过。
2. 连续反向拖动仍有长尾：F01 三轮最大间隔约 204–209 ms，慢日志导致脚本 Fail；F02 一轮达到 343.901 ms，超过 300 ms 门槛。日志显示慢点不是首帧，实际合成／呈现约 2 ms，后续应定向测量解码、请求等待与消息调度。历史已有类似问题，但缺少同画质、同缓存的改前／改后对照，不能断言本轮没有性能回归。[详细日志](acceptance/unified-20260926/short-regression/)。
3. 当前环境 `GraphicsCaptureSession::IsSupported` 返回 `0x8007000E`，本轮候选和历史 `target/re-mvp-ready/release/panzo-cli.exe` 均复现；D3D11 和 H.264 编码预检通过。这证明捕获失败并非仅本轮候选出现，尚未确定系统根因。[当前预检](acceptance/unified-20260926/preflight-current.txt)、[历史候选同环境预检](acceptance/unified-20260926/preflight-historical.txt)。未调整系统设置或终止其他应用以规避错误。
4. 因上述捕获失败，未继续执行本轮新录制的 5 次强杀恢复和 30 分钟录制；全部跨屏／DPI／物理输入、长工程冷缓存及性能长尾矩阵也未重新完成。历史性能问题沿用 [Known-Issues](../Known-Issues.md)，不会因共享像素或单窗口重构自动关闭。
5. 不新增音频、多轨、工程库、多文档或跨进程单实例。项目数据格式兼容与已有编辑能力继续保留。

完整候选构建、验收和打包操作见 [scripts/VALIDATION.md](../scripts/VALIDATION.md)。历史版本包和历史验收记录不作为本轮候选通过证据。
