# RE-MVP 可访问性与启动修复记录

日期：2026-09-05。状态：实现、局部实机验证与本轮统一自动回归完成；**不构成 UI 视觉、完整读屏或正式发布验收**。

## 本次实现

- 录制器与编辑器自绘控件接入 UI Automation：中文名称、帮助说明、启用状态、焦点、实际屏幕坐标和稳定身份。
- 普通命令提供 Invoke，镜头效果开关提供 Toggle，播放位置提供 RangeValue，视频 / 镜头片段提供 Selection / SelectionItem。
- 时间线片段滚出视野后仍可被发现、保持选择；选择片段会将相应时间滚入视野。键盘导航按实际布局排序，包含播放头和片段；焦点环与选中状态保持区分。
- UIA 查询只读取独立状态快照，不持有编辑模型或解码器引用。操作经最多 32 项的队列交回 UI 线程；连续定位只合并相邻同类请求，不跨越分割等离散命令。
- 模态窗口开启时，主窗口禁止操作；已经排队的旧请求不能经模态消息循环重入或关闭后意外执行。
- 支持焦点、结构、名称、启用状态、开关、播放位置、选择与 Invoke 通知。快照锁释放后才发送通知；窗口关闭后旧引用返回元素不可用。
- 修复暂停编辑器首次打开时只暴露窗口标题、需按键后才出现控件树的问题；初始树与项目标题在 Ready 通知前发布，Ready 在窗口显示步骤后发出。

实现位于 `crates/panzo-windows/src/platform/accessibility.rs`，由两个窗口 Host 接入；没有改写源视频、时间映射、编码器或保存格式。GUI 不引入新的运行时安装依赖。

接口契约依据 Microsoft 的[服务端 UIA 提供者说明](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-serversideprovider)和[WM_GETOBJECT 暴露方式](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-howto-expose-serverside-uiautomation-provider)。由于本项目的 windows-rs 0.62 生成接口将部分允许空返回的接口类型表达为非空包装，本实现使用相同 IID / ABI 的内部声明，正确返回 S_OK + null；不构造无效 Rust 引用。ABI、空返回及引用所有权均有测试。

## 构建与证据

- 当前构建：`target/re-mvp-accessibility/release/panzo.exe`，SHA256 `F30A256D7BDDCB9ECCFA3DC6F749BB265F8AD68FACC09A14D0F7F5F5B41EAB79`。
- 对应 CLI：`target/re-mvp-accessibility/release/panzo-cli.exe`，SHA256 `F7E96A805BB2746045D667177B3AFB92716992F9451D257FFD33117FD3885B4C`。
- 新增 9 项单元测试通过：ABI / 空返回、中文与模式、销毁失效、运行时身份、定位合并顺序、队列预算、选择接口所有权、属性通知、多线程只读查询。
- 实机发现旧录制器只有标题栏可访问。中间开发构建新增 6 个命令后，外部读树与连续 Tab 导航成功；Enter 打开原生工程选择器，Escape 取消成功。这不是完整文件打开流程通过。
- 最终修正版通过外部 `sky.get_window_state` 读取：在未发送任何按键前，已经能识别完整编辑器命令、开关、播放位置、1 个视频片段和 5 个镜头片段。[原始树](acceptance/RE-MVP/accessibility-20260905/startup-tree.json)记录对应构建 Hash。
- 完整自动回归结束后，用最终 GUI 再次执行初始读树 → Tab 到来源 → Tab 到录制 → Tab 到打开工程 → Enter → Escape：控件焦点正确，选择器出现时主窗口及命令禁用，取消后恢复启用，最后正常关闭。见[过滤无关 Shell 条目的实际记录](acceptance/RE-MVP/accessibility-20260905/recorder-keyboard.json)。未开始录制、未选择或保存工程；这仍不是完整文件打开 / 编辑任务通过。
- 工程副本：`.tmp/re-mvp-ui-review-20260905-193520.panzo`。源视频 Hash `8C8AA0CE985F2FB5BFCA136C079453F046E8D4FE1DAC94D0666F7BFA23284DFE`；不得将其与 F04 五分钟真实任务素材混淆。

同一 CLI Hash 的[基础回归](acceptance/RE-MVP/unified-20260905-195510/automated-regression.json) 22/22 Pass，[追加长工程回归](acceptance/RE-MVP/final-ui-20260905-195509/final-regression.json) 12/12 Pass；运行期间二进制未变化。完整串行单元测试 **154 Pass / 1 Ignored**，包括新增 9 项 UIA 测试；[格式、严格 Clippy 和测试退出码](acceptance/RE-MVP/final-ui-20260905-195509/quality-exit-codes.json)均为 0。

- 最新 60 秒暂停启动复验：正常退出，媒体渲染 1 帧、窗口绘制 17 次、GPU Present 2 次、CPU Readback 0、媒体错误 0。原始日志 `accessibility-20260905/startup-retest.*.txt`。
- F03 冷 / 热态 60 秒播放 Present P95 为 20.815 / 20.894 ms。
- F05 五分钟往返：17576 个不同源帧，全程最大画面更新间隔 72.992 ms，松手精确收敛，媒体错误 0。末尾最多 3600 个请求样本的 P50 / P95 / P99 / 最大值为 5.770 / 7.931 / 9.997 / 19.891 ms。
- 五分钟进程峰值工作集 701247488 字节、私有内存 807227392 字节，100 ms 采样；不是无限时长保证。F05 为已有代理缓存复用，51 ms 加载，不是首次代理准备。

[指标及素材 Hash](acceptance/RE-MVP/final-ui-20260905-195509/metrics-summary.json)保留测量边界：内部请求到软件 Present，不是物理输入到屏幕发光；自动探针不替代鼠标拖动或无闪烁视觉验收。

## 本次失败与工具边界

1. 桌面授权已成功，不再沿用“授权超时”作为当前阻塞原因。
2. computer-use 请求截图两次均失败：`SetIsBorderRequired failed: 不支持此接口 (0x80004002)`；没有得到任何本轮真实 UI 截图。坐标点击也返回 `coordinate input geometry is unavailable`，未执行鼠标验收。
3. 文件选择器字段设置返回 `Cannot set a value for an element that is not settable`；播放位置设置返回 `read UIA range value read-only state: 所需属性不在 CacheRequest 中 (0x80070057)`。保留为客户端工具操作失败，不将接口单测或读树成功写成这些操作已实测通过。
4. 一次尝试先创建隐藏 Host 再显示，与本轮测试启动器的 Hidden 方式组合后窗口不可见：原始探针正常运行 60 秒并退出，`Graceful close=true`、`Paint calls=0`，不是已证实的死锁。启动脚本还将未取得的 ExitCode 判成失败。已撤回 Host 显示方式改动；仍保留首次 UIA 树发布修复。日志保留在 `accessibility-20260905/startup-fixed.*.txt`；后续使用有期限等待、保留进程句柄、正向完成与实际绘制断言复验。

第 4 项与 Windows 首次 ShowWindow 可受启动器 STARTUPINFO 覆盖的[官方行为](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow)相符；未据此声称掌握了内部每一步的调用栈。试图清理该测试进程时，它已经退出，身份检查拒绝停止；没有结束其他应用。

## 仍需关闭

- UI 路径的原生垂直切片比较与用户真实视觉确认（ADR-0002 保持 Proposed）。
- 实际鼠标拖动 / 黑帧 / 闪烁、DPI 截图、输入法与全套读屏使用流程。
- 原计划的完整物理输入采样、三轮五分钟真实任务以及最终同构建长时放行矩阵。

这些必测项不是 Deferred，也不被本次 UIA 实现代替；跨 GPU、音频等已有延期范围维持不变。
