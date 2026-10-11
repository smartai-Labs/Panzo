# Panzo 工作台布局原型 v14

这是保留当前主题的布局评审稿，也是 v14 原生开发的设计基线。本目录只包含原型；实际应用与验证见 [v14 原生改造记录](../../Workbench-v14-Native-2026-10-09.md)。

[打开交互原型](index.html) · [问题与改造方案](../../UI-Layout-Review-v14-2026-10-09.md)

## 打开

双击本目录的 `index.html`，用 Chrome 或 Edge 打开即可。没有外部字体、CDN、网络请求或安装依赖。

也可在仓库根目录运行：

```powershell
powershell -ExecutionPolicy Bypass -File scripts/open-workbench-v14-preview.ps1
```

建议先在 1440 × 900 左右查看，再缩小到 960 × 720。原型里的最小化、最大化、关闭图标用于排版示意；预览窗口使用真实浏览器窗口控制关闭。

## 建议体验顺序

1. **视频工作台**：点击不同的视频、旁白、音乐或字幕素材条，检查右侧属性如何跟随选择。试用分割、删除、撤销；拖动时间线上方分隔线，观察轨道高度和紧凑模式。
2. **预览区域**：Ctrl＋滚轮缩放，拖动画面平移，点击上下文栏右侧的适应窗口图标恢复居中。拖动右侧分隔线调整属性栏宽度。
3. **声音和字幕**：右侧“声音”试用模拟录制；切换工作区后从顶部任务入口返回并结束。“字幕”可编辑当前文本或模拟 AI 生成，全部字幕和样式在折叠项中。
4. **图片工具**：切到图片，点击左侧标注、裁剪、OCR、识图工具，检查参数是否只显示当前任务内容。识别结果留在右侧。
5. **拼图**：左侧宫格图标进入；选择布局和单格比例，在预览照片上滚轮缩放、拖动位置，拖动下方缩略图改变顺序，末尾加号与缩略图同尺寸。
6. **素材**：分类、搜索、切换网格和列表；选中后看预览，点击右下角主按钮或双击接入对应工作区。
7. **设置与插件**：从顶部打开；试用主题切换、设置分类与草稿、插件启停。模拟任务运行期间禁止停用正在使用的插件。
8. **紧凑窗口**：小于 1000 宽时项目素材自动收起，可从上下文栏最左侧按钮临时展开；属性与固定任务操作仍可使用。

## 截图

| 场景 | 预览 |
|---|---|
| 视频 / 深色 | [01-video-dark.png](previews/01-video-dark.png) |
| 声音 | [02-audio-dark.png](previews/02-audio-dark.png) |
| 字幕 | [03-subtitle-dark.png](previews/03-subtitle-dark.png) |
| 图片标注 | [04-image-dark.png](previews/04-image-dark.png) |
| 裁剪 | [05-image-crop-dark.png](previews/05-image-crop-dark.png) |
| OCR | [06-image-ocr-dark.png](previews/06-image-ocr-dark.png) |
| 拼图 | [07-collage-dark.png](previews/07-collage-dark.png) |
| 素材 | [08-assets-dark.png](previews/08-assets-dark.png) |
| 插件 | [09-plugins-dark.png](previews/09-plugins-dark.png) |
| 紧凑轨道 | [10-timeline-compact.png](previews/10-timeline-compact.png) |
| 设置 | [11-settings-dark.png](previews/11-settings-dark.png) |
| 视频 / 浅色 | [12-video-light.png](previews/12-video-light.png) |
| 录制准备 | [13-record-light.png](previews/13-record-light.png) |
| 导出 | [14-export-light.png](previews/14-export-light.png) |
| 960 × 720 声音面板 | [15-compact-audio-light.png](previews/15-compact-audio-light.png) |

## 演示范围

- 素材为内嵌 SVG 与演示数据。
- 录制、OCR、AI 字幕、导入、保存和导出是流程模拟，不访问麦克风、不处理实际文件、不连接 AI 服务。
- 设置中的服务地址、模型、API Key 仅展示本页草稿；不会发送或长期保存。只有主题写入当前浏览器本地存储。
- 片段操作演示选中、分割、删除与撤销；不是完整可用的非线性编辑器。部分数字字段、裁剪像素结果、全局快捷键与窗口控制尚未接入真实行为。
- 插件开关仅模拟界面状态，不替代原生插件的进程隔离测试。
- 原生阶段继续复用现有主题、任务、插件和工程模型，不以 HTML 页面替换应用技术架构。

## 验证

```powershell
node scripts/test-workbench-v14-prototype.cjs
```

脚本启动独立无头浏览器及临时配置，不操控用户已打开的浏览器。12 组交互与几何检查通过，15 张截图，运行时 JavaScript 错误为 0。

[测试结果](previews/prototype-tests.json) · [原生源码与 EXE 未变化核对](previews/scope-check.json)
