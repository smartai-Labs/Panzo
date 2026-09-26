# ADR-0002：R0 UI 实现路径与验证闸门

日期：2026-09-05  
状态：Proposed；保留两个候选，尚未完成原生嵌入对比，不锁定迁移

## 连续开发期间的实施记录

本轮候选沿用 Win32 / GDI 控件绘制与独立 DXGI 预览，先落地共享 DPI 布局、系统字体、中文 SVG 控件、属性输入、可调时间线、后台解码 / 保存 / 导出。实现边界见 [RE-MVP 实现说明](RE-MVP-Implementation.md)。

这是对现有 Host 的收敛实现，不是候选 A 的 DirectWrite / Direct2D 全量迁移，也不是候选 B 的 WebView2 实现。选择继续复用原生预览的理由是：现有 GPU 直出与录制链已经可复用，新剪辑 / 保存 / 导出可以在不增加运行时分发依赖的情况下接通。该工程判断不构成两个原型路径的实测优劣结论。

后续已补充自绘控件的 UI Automation 提供者：录制器命令、编辑器命令 / 播放位置 / 片段选择、状态通知与关闭失效，见[实现及实机记录](RE-MVP-Accessibility-Report.md)。外部读树已验证，但截图与部分操作受 computer-use 工具错误阻断，不能写成完整读屏验收。

**未关闭项：**同条件双候选垂直切片比较、完整辅助技术操作验证、真实 DPI 截图和用户视觉复验尚未完成。不能据此将本 ADR 改为“选型验证已通过”，也不能把 HTML 原型通过当成真实 UI 通过。后续是否迁移按这些结果决定，不以框架名称代替体验门槛。

## 已作出的决定

1. R0 使用[本地 HTML/CSS/JS 原型](prototypes/re-mvp/index.html)评审信息架构和交互，原型不替换正式 EXE。
2. 保留 Rust 工程、原始媒体/事件、原子事务与共享合成链；不得为了界面迁移退回逐帧 BGRA 跨 UI 进程传输。
3. 不延续单一大窗口类管理输入、精确定位、后台任务和全部手绘控件；调度与界面组件分离适用于下面两条路径。

## 候选比较

| 维度 | A：Win32 Host + Direct2D/DirectWrite UI | B：Win32 Host + WebView2 UI + 原生预览 |
|---|---|---|
| 排版与设计落地 | 重写文本布局和控件渲染；能逐步替换现有 GDI；不直接复用 HTML | 可复用原型的布局/样式思路；仍须生产组件化，原型 JS 不直接作为编辑内核 |
| 布局、属性输入 | 需要组织布局系统和控件行为，不能继续固定坐标堆按钮 | 使用 DOM/CSS 和输入控件；原生焦点、快捷键与网页焦点需明确路由 |
| 预览 | 沿用现有独立 HWND/DXGI 表面 | 需验证原生预览与 WebView 的区域分隔、覆盖关系、焦点和 DPI；不能因 HTML 能播放视频就称已集成 |
| 可访问性 | 自绘控件需完善 UI Automation 提供者 | 网页语义控件有利于结构表达，但跨原生预览边界仍需实测 |
| 性能风险 | 现有增量解码仍可能阻塞窗口线程；换绘图 API 不解决此问题 | WebView 的 STA/消息循环同样不能阻塞；跨边界消息必须合并、有界 |
| 构建/分发 | 延续 Windows/Rust依赖，新增绘图接口与控件代码 | 需 WebView2 Runtime/绑定与离线分发策略；本机已发现 Runtime 151.0.4129.101，不代表目标机都有 |
| 当前证据 | 旧 Host 可运行，旧 UI 视觉未通过；新 D2D 控件切片尚未实现 | 独立 Chromium 原型测试通过；WebView2 原生桥接尚未实现 |

技术依据：DirectWrite 文本布局可与 Direct2D/GDI 分阶段互操作，见 [Microsoft 文本渲染说明](https://learn.microsoft.com/en-us/windows/win32/direct2d/direct2d-and-directwrite)。WebView2 提供窗口尺寸/父窗口/焦点管理，但遵循 STA 消息循环限制，见 [Controller](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller)、[线程模型](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/threading-model)。上表的工程成本与风险是结合本项目代码作出的判断，不是官方性能保证。

## 进入正式 UI 重写前的闸门

两个候选仅做同样的一条垂直切片：顶部命令、可输入属性、可拖动时间线、真实 GPU Preview。记录：

- 实际窗口的排版/中文/DPI截图及用户视觉意见。
- 短/长工程同条件输入到播放头、输入到预览画面的延迟；不使用原型动画耗时替代。
- 预览重绘隔离、焦点框、导出面板的覆盖关系；无黑帧与错误命中。
- Tab/中文输入/空格、数值输入与全局快捷键不冲突。
- 关闭重开20次，资源释放、取消和崩溃路径不回退；包体和启动成本有实测。

如果选 B：只加载受控本地资源；禁止任意网页获得宿主权限；消息校验类型、工程ID和修订；文件路径经原生文件选择流程提供。不得开放通用任意文件/执行命令接口。安全边界参考 [WebView2 安全指南](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security)。

在上述切片测量前，只能说“原型实现方式已确定、生产实现方式待验证”。本次不把浏览器原型通过改写成“最终 UI 框架选型完成”。
