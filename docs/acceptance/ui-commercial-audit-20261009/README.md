# UI 审查证据索引

完整结论见 [商业化就绪审查](../../UI-Commercial-Readiness-Audit-2026-10-09.md)。候选为 desktop-v12.6。

- `drawing/`：本轮绘图稳定性与采样。
- `workflows/ui-smoke.json`：本轮流程检查；图片、素材、设置截图可用于视觉审查。
- `responsiveness/responsiveness.json`：第一次响应测量，视频没有携带代理缓存。无操作 CPU 样本不属于稳定空闲状态。
- `responsiveness-confirm/responsiveness.json`：复测；视频携带测试工程已有缓存。中途改变窗口尺寸仍可能触发缩略图生成。
- `management/plugins-dark.png`：本轮重新获取的有效插件窗口截图；`capture.json` 记录目标窗口命中。
- `token-audit.json`：主题颜色的 sRGB 对比度计算与文件行数（包含测试，不能据此直接判断架构好坏）。
- `environment.json`：本轮机器环境。

**作废的视觉证据：** `workflows/plugins-dark.png` 被设置窗口遮挡；`responsiveness/plugins-unoccluded.png` 实际被同进程主窗口遮挡。保留原始文件用于审计，不据此评判插件 UI。复测脚本加入根窗口命中校验后明确拒绝同类截图。

本目录未包含真实 AI 请求或麦克风录制。脚本结束只清理各自创建的测试进程。
