# Panzo 桌面工作台 v3 · 交互设计稿

依据用户“太像网页应用”的反馈，重做为紧凑的桌面编辑器布局：顶部工作区、左侧窄工具栏、文档标签、暗色画布、底部缩略图和可收起的识别面板。默认深色，右上角可切换浅色。

打开方式：

- 双击 `index.html`。不需要联网或安装依赖。
- 独立预览窗口：在项目根目录运行 `powershell -ExecutionPolicy Bypass -File scripts/open-desktop-v3-preview.ps1`。
- URL 附加 `?empty=1` 查看空工作区；默认加载示例文档。

原型支持导入 / 拖放 / 粘贴本地图片、矩形和画笔标注、撤销、文档切换、面板收起、OCR 模拟任务与结果校对、录制和导出流程、素材筛选、插件启停及主题切换。`Ctrl K` 搜索命令，`Ctrl O` 打开图片，`Ctrl V` 粘贴，`Ctrl Z` 撤销。

这是设计预览，尚未替换原生应用。截图、识别、录制、编码、插件安装均为演示，不执行真实系统操作。原型只保存浏览器主题偏好；其他输入仅驻留本次会话，API Key 不保存、不发送，请勿输入真实密钥。标尺和颜色样块是视觉示意。

- [设计规格与原生改造顺序](../../UI-Workspace-Redesign-2026-10-08.md)
- [深色预览](previews/desktop-overview-dark.png) · [浅色预览](previews/desktop-overview-light.png)
- [交互验证](previews/prototype-tests.json)

验证：`node scripts/test-desktop-v3-prototype.cjs`。专用无头 Chromium，测试结果写入 `docs/acceptance/desktop-v3-design`，通过后同步到 `previews`。包含 1440×900、1170×820、960×720 视口；不代表原生多 DPI 和真实媒体功能已通过。

文件说明：`engine.js` 复用 v2 的原型状态与本地导入；`desktop.js` 重建桌面结构、示例文档和面板；`base.css` 是本目录的基础控件样式快照，`desktop.css` 定义 v3 视觉。无需依赖 v2 目录。

标题栏同色：原型提供与主体外壳同步的主题颜色提示；浏览器可能保留自己的标题栏颜色。原生版本将通过现有 Windows 标题栏着色接口统一，当前截图与浏览器检查不包含这项原生验收。
