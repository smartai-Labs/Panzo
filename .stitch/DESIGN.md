---
name: Panzo approved RE-MVP prototype
colors:
  background: '#f5f5f7'
  surface: '#ffffff'
  toolbar: '#fcfcfd'
  inspector: '#fafafb'
  preview: '#1c1d21'
  on-surface: '#242529'
  on-surface-variant: '#72757e'
  outline-variant: '#e3e4e8'
  primary: '#087bfa'
  on-primary: '#ffffff'
  secondary: '#8261d0'
  video-track: '#dce7ef'
  camera-track: '#e8e0f7'
---

# Design System: Panzo

唯一来源：用户确认的 `docs/prototypes/re-mvp/index.html` 与 `style.css`（包括 CSS 末尾字号覆盖）。原型不修改。本文件是提取记录，不是新设计；不向 Stitch 上传。

## 1. Visual Theme & Atmosphere

浅灰工具区、轻薄白色控件、深色预览舞台。通过留白和分隔线区分区域，不把每个功能堆成带边框卡片。蓝色用于主要动作与选择，浅紫用于镜头片段。

## 2. Color Palette & Roles

基础雾灰 #f5f5f7、纸白 #ffffff、工具栏近白 #fcfcfd、面板 #fafafb；预览炭黑 #1c1d21。交互蓝 #087bfa，悬停 #006eeb；文本石墨 #242529、说明灰 #72757e、发丝线 #e3e4e8。次要图标悬停底 #e9ebf0。六背景预设：雾蓝 #c5d5e7、暖杏 #f0d7c3、淡紫 #d8d1ef、石墨 #383a46、纯白 #f8f8f8、鼠尾草 #ccdace。功能状态仍须中文说明，不只靠颜色。

## 3. Typography Rules

Segoe UI / Microsoft YaHei UI / Microsoft YaHei 系统无衬线；不安装 Apple 字体。项目名 14px 半粗；常规按钮、属性、时间码 12px；标尺、状态与预览说明 11px；录制器标题 36px/1.4，短窗口 30px；导出标题 21px。数字对齐，长项目名省略，不缩小全部字号来填空间。

## 4. Component Stylings

普通按钮高 32、半径 7、横向内边距 13；白底细线与轻阴影。幽灵图标按钮 32×32，无常驻边框，悬停才显底色；时间线 28×28。主按钮蓝底白字。SVG viewBox 24，显示 18px，线宽 1.65，圆端点；时间线 16px。播放按钮浅灰，不保持蓝色按下状态。按钮按下、选择、键盘焦点分离。

属性面板使用背景分段选项、六色块、数值加滑杆、28×17 开关，不用全宽命令按钮替代。镜头上下文行：名称、开始/结束秒数、倍率、居中焦点；数值框白底、细边、半径 5、高 27。

录制器内容宽 440，居中；来源分段切换、白色来源卡片，46px 高主录制按钮。导出面板宽 460、半径 16、内边距 28，摘要、分辨率、帧率、位置、进度与底部动作。

## 5. Layout Principles

编辑器：62px 顶栏 / 自适应工作区 / 245px 时间线；高窗口 66/265，低窗口 54/235。工作区右侧栏 264，≤1120 宽为230，≤900折叠。预览下方独立52px播放栏，低窗口42。预览内边距40，禁止把媒体内容铺到整个工作区；不把外观边距当 UI 留白。

顶栏左侧返回、两行项目名、撤销/重做，右侧保存状态、保存、导出视频。轨道左侧72px固定名称栏；分割/删除/添加镜头/效果开关在时间线工具栏，缩放滑杆及适应在右侧。上下文49px、状态26px。只将浏览器评审栏、示例素材和模拟文案从产品排除。原生标题栏单独计算，不仿造交通灯。

## 6. Design System Notes for Stitch Generation

本轮不生成设计。若后续需要设计工具：使用“轻薄灰白录屏编辑器、深色预览、独立播放栏、浅紫镜头轨道”的描述；所有颜色尺寸以上述原型为准。分别对齐区域、控件、状态，不把视觉差异解释为平台必然限制。录制、预览、保存和导出内核不得因 UI 改写而降级。
