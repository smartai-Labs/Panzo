# 单击、焦点标记与视频裁剪修复（2026-10-10）

## 原因与修复

- 第一次点击只获焦：预览窗口和录制窗口的失焦处理无条件释放线程的鼠标捕获，打断子按钮原生按下/释放链。现在先检查 `GetCapture() == hwnd`，仅释放自己的捕获；取消编辑手势也遵守该规则。没有改成双击或人为补发点击。
- 所有共享原生按钮移除焦点下划线；自绘控件共用的焦点标记改为键盘操作时的内侧细轮廓。分段按钮悬浮只使用背景反馈。保留 Tab、Space 等键盘操作以及工作区选中标签指示。
- 属性面板的“启用”行原先将 Cursor 图标画在文字起点，现在去掉这处错误图标；真正的“显示光标”行给图标和文字分别留位。
- “自由”单击直接进入视频预览裁剪，退出素材预览并暂停播放。选择固定比例也直接进入裁剪；可拖四边、四角与框内取景区域，重新进入保留现有范围，重置恢复完整画面。原有“在预览中裁剪 / 结束画面裁剪”入口保留。
- 固定比例选中状态按实际裁剪比例判断，移动、缩放框后不丢失。导入片段使用自身尺寸判断比例，避免误用工程源视频尺寸。
- 拖动复用已解码画面，只刷新边框、相关数值和必要的预览区域，不再逐次调用整个工作台刷新。一次拖动仍作为一组撤销事务。

## 验证

- [修复前原生失败记录](acceptance/interaction-crop-20261010/before-native/run.txt)：从主窗口转移焦点后第一次按下按钮，`GetCapture()` 变为 0，复现了用户描述的第一击只获焦现象。此前只从搜索框转移焦点的测试遗漏了此路径。
- [修复后原生记录](acceptance/interaction-crop-20261010/verified/run.txt) / [断言摘要](acceptance/interaction-crop-20261010/verified/interaction-review.txt)：导入、录制、素材库及更多按钮分别从主窗口、搜索框、按钮自身转移焦点，一次原生按下/释放只发出一次命令。计数测试拦截命令避免打开录制/文件对话框；另有实际单击导入到项目、内联输入提交后单击切换页签的验证。
- 裁剪验证覆盖：自由入口切回视频预览、边角命中与鼠标拖动、框内平移、撤销、退出重入、重置、锁定比例、预览放大、项目尺寸改变、导入片段自身尺寸，以及拖动期间素材面板版本不刷新。
- [单元测试](acceptance/interaction-crop-20261010/unit-tests.txt)：核心库 93 项、Windows 库 259 项通过，53 项专用环境测试在该命令中忽略。原生验收和下述导出测试另外显式执行。
- [实际裁剪导出](acceptance/interaction-crop-20261010/crop-export/run.txt)：输出并解码 150 帧，包含不同裁剪比例、偏移和速度；[抽样像素对比](acceptance/interaction-crop-20261010/crop-export/crop-export.json)通过，预览与导出抽样 RGB 平均误差小于 1。
- [发行程序冒烟](acceptance/interaction-crop-20261010/release-smoke/run.txt)：窗口创建、GPU 直接预览、播放及正常退出通过；媒体错误 0，CPU 回读 0。
- 验证使用隔离工程、配置和合成素材；未关闭正在运行的旧程序、未操作用户正在编辑的工程、未录制麦克风。

## 启动

保存并退出旧版后，双击根目录 `Start-Panzo.cmd`。入口已指向 `target/desktop-v14-interaction/release/panzo.exe`，同目录包含两个插件包和构建指纹。旧版本目录保留。

新 EXE SHA-256：`5792EB6BC0845AC1A42911C5487980E91E9C9B0DD3535F957F02BE33BC68A260`。

## 截图

- [属性标签与“启用”行](acceptance/interaction-crop-20261010/verified/follow-no-overlap.png)
- [自由裁剪框](acceptance/interaction-crop-20261010/verified/free-crop-overlay.png)
- [放大预览中的裁剪](acceptance/interaction-crop-20261010/verified/zoomed-free-crop.png)
- [导入片段固定比例裁剪](acceptance/interaction-crop-20261010/verified/imported-locked-crop.png)
