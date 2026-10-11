# 插件生命周期 v12.7 验证

- `windows-tests.txt`：236 项通过，45 项专项测试默认跳过。
- `process-tests.txt`、`lifecycle-review.json`：独立原生测试进程中的生产注册表/执行器验证；包含两个插件并发、取消进程树、媒体超时、跨实例锁与宿主异常退出后释放锁。
- `image-worker-tests.txt`：实际灰度/反色插件进程往返。
- `release/release-hotplug.json`：实际 v12.7 EXE 通过插件管理界面卸载 A 时，宿主与 B 工作进程均保持原 PID，B 成功产出图片后可卸载，用户结果保留。
- `isolated-*`：独立进程测试保留的测试注册表、锁文件和结果，均为测试生成。

真实界面测试使用 `.tmp/plugin-hotplug-*` 隔离配置。未修改用户现有插件安装或工程。测试专用 fixture EXE 没有包含在应用发布包内。
