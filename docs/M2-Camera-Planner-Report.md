# Panzo V0.1 M2：Camera Planner 验收报告

执行日期：2026-09-01（Asia/Shanghai）

M2 已满足技术规范第 10 章与第 17 节的退出条件。该能力在录制链开发前已先行实现，本报告补齐正式阶段编号和可追溯证据。

## 已实现能力

- Capture 坐标到归一化坐标的确定性转换，支持负桌面原点与边缘 Clamp。
- Click 聚类、近距离 Pan、远距离 Reframe、Zoom In / Hold / Zoom Out。
- 全部 Segment 使用整数 Tick，端点连续且不重叠。
- Camera State 使用 `smootherstep`，边界速度为零。
- Planner 输出包含生成器版本、参数集和 Source Revision Hash。
- 相同输入重复运行 100 次，规范化 Camera Track Hash 完全一致。

## 验收证据

- Camera、Geometry、Planner 相关单元测试全部通过。
- `fixtures/camera` 中 10 个 Golden Fixtures 全部匹配。
- Golden 更新测试保持 `ignored`，普通验证不能覆盖人工审阅后的期望值。
- 四角 Focus 的 Camera State 均通过 Clamp 校验，不暴露源画面外区域。

## 复验命令

```powershell
cargo test -p panzo-core
cargo test -p panzo-core --test golden all_v01_camera_golden_fixtures_match -- --exact
```

结论：M2 Camera Planner 通过，下一阶段按技术规范编号进入 M3 Playback、Render 与 Export。
