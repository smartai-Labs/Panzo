# 预览清晰度与导出面板（2026-10-10）

当前程序：target/desktop-v14-preview-quality/release/panzo.exe。根目录 Start-Panzo.cmd 已指向新版；先保存并关闭旧窗口，再启动新版。已安装插件可以继续使用，发布目录也附带两个插件包。

## 调整内容

- 在播放控制左侧加入清晰度下拉框：**1/4、1/2、原画**，默认原画，保存为应用偏好。视频预览和素材预览共用选择，同一播放器与 GPU 窗口继续复用。
- 比例按源视频宽、高分别计算。例如 2560 × 1440 素材的三档分别为 640 × 360、1280 × 720、2560 × 1440。项目画布和导出分辨率不受预览选择影响。
- 两个预览切换按钮统一为深色容器、灰色选中块，与速度分段按钮一致。
- 导出字段依次为名称、输出位置、分辨率、帧率、格式；保留 MP4 / GIF。移除标题下面重复的工程名。输入框与下拉框同高、同列；路径右侧放文件夹按钮，长路径省略显示。
- “导出”标题使用与正文相同字体、普通字重，14 px；正文 13 px。高级设置在本面板展开，保留导出时间范围、声音与 MP4 质量。小窗口内可滚动设置区，底部进度和操作按钮固定。
- 导出面板空白区域使用箭头，输入区域保留文字光标，不再继承预览画布的十字光标。

## 原因与实现

时间轴素材预览原先固定以最长边 1280 解码，高分辨率画面再次放大后变模糊；导出使用原始尺寸，所以导出较清晰。现在取消这项隐式限制，按用户选择设置实际解码尺寸。

源视频和时间轴素材解码都在后台处理。切换清晰度取消过期请求，并将分辨率加入缓存匹配条件；同一播放位置从低清恢复原画也会重新请求正确尺寸。拖动代理仍保留原始尺寸元数据及源时间映射。切换中保留已有画面，播放意图在新帧就绪后恢复，无需重建播放器或 GPU 窗口。

导出设置仅改变表单组织，仍由原导出任务后台编码。高级范围在提交时校验，格式切换保留用户修改的目录和名称。

## 验证

证据目录：[preview-quality-export-20261010](acceptance/preview-quality-export-20261010)。

- [单元测试](acceptance/preview-quality-export-20261010/unit-tests.txt)：核心库 99 项、Windows 库 264 项通过；53 项专项测试默认跳过，以下相关原生检查另行执行。
- [清晰度原生检查](acceptance/preview-quality-export-20261010/native-final/quality-review.json)：源视频、时间轴放置素材、普通素材预览的三档解码尺寸均正确。实际下拉控件传递选择、同帧低清转原画、播放中切换和拖动时间轴通过；工程设置不变，同一个 GPU 窗口，GPU 回读增量为 0。
- [导出表单原生检查](acceptance/preview-quality-export-20261010/native-final/run.txt)：字段顺序、对齐、高度、展开与折叠、短窗口滚动后恢复、范围校验、声音与质量参数、GIF 切换及文件名目录草稿保留通过。面板箭头光标验证通过。
- [共享播放器回归](acceptance/preview-quality-export-20261010/shared-player/polish-review.json)：8 次来源切换复用同一 GPU 窗口，回读 0；即时声音修改、同轨重叠拒绝、逐条字幕样式保存通过。
- [点击与裁剪](acceptance/preview-quality-export-20261010/interaction-repeat/interaction-review.txt)、[原裁剪夹具](acceptance/preview-quality-export-20261010/interaction-verified/interaction-review.txt)：一次点击、失焦提交、裁剪拖动、比例、缩放后裁剪及导入通过。首轮另有一次鼠标裁剪断言失败，保留在 interaction/run.txt；未改源码，同一夹具复跑及原验收夹具均通过，未确认该偶发失败的原因。
- [布局回归](acceptance/preview-quality-export-20261010/video-layout/run.txt)：深浅主题、紧凑布局、录制与导出页面通过。
- [实际 MP4](acceptance/preview-quality-export-20261010/crop-export/crop-export.json)：1920 × 1080、60 fps、150 帧；裁剪、缩放、图片背景和边距经解码抽检，与预览对应。
- [发布版启动](acceptance/preview-quality-export-20261010/release-smoke/run.txt)：正常加载、播放与退出，媒体错误 0，GPU 直出，CPU 回读 0。
- [源码和二进制身份](acceptance/preview-quality-export-20261010/candidate-identity.json)核验匹配。

截图：[播放行](acceptance/preview-quality-export-20261010/native-final/quality-and-preview-tabs.png)、[导出收起](acceptance/preview-quality-export-20261010/native-final/export-collapsed.png)、[导出展开](acceptance/preview-quality-export-20261010/native-final/export-expanded.png)、[短窗口](acceptance/preview-quality-export-20261010/native-final/export-short-scrolled.png)。

本轮测试使用隔离工程与配置。性能记录来自本机夹具，不覆盖所有显卡、编码格式和超大工程；构建仍有历史未使用代码警告。
