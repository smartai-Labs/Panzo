# 悬浮提示、面板拖拽与空白工作台修复

## 原因与修改

- 提示闪退：原实现每 50 ms 要求提示所属窗口必须是前台窗口。原生验证记录显示，提示显示成功后，第一次定时器因前台窗口不同将其关闭。现在允许对非前台窗口悬停，仅在显示后发生前台窗口切换时关闭；鼠标离开、点击、所属窗口隐藏/移动和超时仍会关闭提示。鼠标状态由控件消息负责，避免另一套全局坐标轮询覆盖悬停状态。
- 素材栏：增加右侧 6 DIP 分隔条，可拖动调整为 160–420 DIP；根据预览区可用空间进一步限宽。松手保存宽度，取消或失去捕获恢复原宽度；紧凑窗口保留原有收起和覆盖显示行为。
- 视频布局：素材栏、属性栏和时间轴只记录最新拖拽位置，在约 60 Hz 的帧时钟上统一提交。松手提交最后位置。布局调整不调用整页 refresh，不重建标题与无障碍树，不修改工程内容，不触发解码或重新生成素材缩略图。预览尺寸确实变化时仍需要重新呈现已有帧。
- 图片属性栏：用 16 ms 定时器合并拖拽消息；未变化的宽度直接跳过布局，取消操作恢复原值。
- 公共布局：批量窗口定位只失效控件原位置和新位置，移除整个父窗口失效。视频时间轴和属性绘制增加可见区域过滤。
- 空白录制与视频工作台：中央显示“继续编辑最近工程”和“新录制”，没有最近工程时禁用继续编辑。不再显示主显示器大卡片、底部开始录制按钮、底部继续编辑和功能提示文字。
- 点击“新录制”打开来源与参数面板，可选屏幕、窗口或区域，再点击“开始录制”；取消仍回到空白工作台。

## 验证

- 14 项编辑器布局单元测试通过。
- 提示跨 DPI 的上方/侧方定位测试通过。
- 原生提示测试：复现原定时器误关闭；修复后非前台悬停超过 1 秒，再分发真实提示定时器仍保持可见；离开关闭、长文本换行、浅深主题和销毁释放通过。证据：`docs/acceptance/panels-empty-20261010/hover-fixed`。
- 视频原生拖拽：80 次移动合并为一次尺寸提交，松手提交和取消恢复通过；无关面板几何、标题刷新时刻和工程版本不变。证据：`docs/acceptance/panels-empty-20261010/panels-release`。
- 空白页原生验证：居中位置、无主屏幕卡片和底部操作、最近工程启用条件、打开/取消录制面板、选择屏幕通过。
- 图片原生拖拽：40 次移动在定时器提交前保持原尺寸，提交后改变宽度，取消恢复；图片内容版本不变。证据：`docs/acceptance/panels-empty-20261010/empty`。
- 原生测试使用隔离用户目录和生成的图片/工程副本，没有启动真实屏幕录制或调用外部模型。

## 复现

```powershell
python scripts/test-workbench-v14-native.py --project docs/acceptance/workbench-v14-native/final/fixture.panzo --package target/desktop-v14-hover-fix/release/panzo-media-tools.panzoplugin --output <新输出目录> --panel-only
python scripts/test-workbench-v14-native.py --project docs/acceptance/workbench-v14-native/final/fixture.panzo --package target/desktop-v14-hover-fix/release/panzo-media-tools.panzoplugin --output <另一个新输出目录> --hover-only
python scripts/test-panels-empty-native.py --output <另一个新输出目录>
```

## 发行版

构建目录：`target/desktop-v14-panels-empty/release`。构建身份和二进制哈希记录在同目录的 `candidate-build.json`。

最终源码身份核对通过。发行版 `preview-smoke`：Window created / Graceful close 均为 true，Media errors 为 0；证据：`docs/acceptance/panels-empty-20261010/release-final/preview-smoke.txt`。

`panzo.exe` SHA256：`98ABC6354DE8D1DD7CB3491EEF20C7ABC34EB3241CF6570A47C82B7B8BC5466F`。
