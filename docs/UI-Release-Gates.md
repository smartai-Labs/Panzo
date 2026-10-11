# 桌面 UI 发布验收门槛

正式目标系统仍为 Windows 11。Windows 10 是当前开发机上的兼容性回归环境，不能替代 Windows 11 验收。候选构建完成与商业发布就绪是两个状态；下列实机项目未通过前，不标记商业发布就绪。

| 项目 | 环境与动作 | 通过标准 |
|---|---|---|
| 目标系统 | 真实 Windows 11；安装、启动、退出、托盘恢复 | 标题栏、系统按钮和主题一致，无确认窗重入或退出崩溃 |
| DPI | 100%、125%、150%、200%；混合 DPI 跨屏；负坐标副屏 | 图标和文字清晰；弹窗在工作区内；主要操作可见；命中区域正确 |
| 系统文字 | Windows 辅助功能文字 100%、150%、200%；运行中修改 | 字体和控件一起重新布局；滚动可到达内容；无按钮遮挡 |
| 高对比度 | Windows 实际高对比度明/暗方案 | 控件、选中、禁用、错误、焦点可区分；普通文字达到 4.5:1 |
| 减少动画 | Windows 关闭动画、Panzo 设置关闭动画、重新开启 | 关闭后立即切换；闲置没有动画定时器；编辑手势不加动画延迟 |
| 输入法/键盘 | 微软拼音及代表性第三方输入法；Tab/Shift+Tab/Enter/Escape；文字输入 | 候选窗位置正确；输入快捷键不触发工作区命令；纯键盘可完成主要流程 |
| 读屏 | Narrator 实机完成菜单、截图、选区、四个属性标签、轨道、插件管理 | 名称、角色、当前值、忙碌/禁用状态和焦点顺序可理解 |
| 设备与错误 | 麦克风拔出/重连、默认设备更换；模拟断网/429/取消；真实服务兼容性另测 | 工作不丢失，录音可结束，错误可恢复，插件任务不影响其他插件 |
| 资源与持续运行 | 100 次设置/插件切换；30 秒空闲；30 分钟编辑；代表性普通显卡设备 | GDI/USER 对象稳定；无持续内存增长；空闲无持续重绘；拖拽无闪白 |
| 性能 | 同一 4000×2500 图片、相同候选与设备；窗口尺寸消息及待绘制处理分开记录 | 保留原始样本；消息往返不冒充 FPS；16.7 ms 是优化目标，实际呈现另用跟踪验证 |
| 构建一致性 | candidate-build.json 与当前源码/二进制校验 | 截图、测试结果注明各自二进制指纹；只给最终候选标记最终验收 |

## 自动化入口

所有脚本创建自己的配置和测试进程，不连接用户正在编辑的实例。使用 `--output` 指向新目录，保留失败记录。

```powershell
./scripts/build-candidate.ps1 -TargetDirectory target/desktop-v13
python scripts/test-ui-repair-release.py --binary target/desktop-v13/release/panzo.exe --output docs/acceptance/my-review/workflows
python scripts/audit-ui-responsiveness.py --binary target/desktop-v13/release/panzo.exe --phase image --flush-paint --output docs/acceptance/my-review/response
python scripts/test-drawing-stability.py --binary target/desktop-v13/release/panzo.exe --output docs/acceptance/my-review/drawing
python scripts/test-exit-release.py --binary target/desktop-v13/release/panzo.exe --output docs/acceptance/my-review/exit
python scripts/test-ui-resource-lifetime.py --binary target/desktop-v13/release/panzo.exe --output docs/acceptance/my-review/resources
```

字幕、录音与 Inspector 的原生测试需要独立配置、合成工程和媒体插件包，见 `inspector_review.rs` 的环境变量约束。插件热插拔测试使用专用测试插件，不打包进应用。真实录音/收费 AI 服务未由这些合成测试覆盖。
