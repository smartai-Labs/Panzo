# 面板标题样式调整（2026-10-10）

分组标题与设置项继续共用 Microsoft YaHei UI 字体。正文为 13 px、400 字重；分组标题从 13 px、600 字重调整为 14 px、700 字重（逻辑像素，跟随 DPI 缩放）。保留既有分组行高与间隔，标题字号在现有行高内完整显示。

“项目画面”“背景”“圆角与阴影”“时间范围”等自绘标题使用统一 Heading 样式。声音、字幕面板的原生分组标签也接入该样式；主题和 DPI 刷新不会再把这些标题重置成正文字体。用户内容、字幕实际字体和图片文字字号不受此 UI 字体调整影响。

验证：复用[原生工作台验收](acceptance/panel-headings-20261010/native-14px/run.txt)，通过深色、浅色、紧凑布局，以及录制/导出/字幕面板的原生检查。检查截图中的标题与相邻控件没有裁切或重叠。[发行程序冒烟](acceptance/panel-headings-20261010/release-smoke-14px/run.txt)通过，媒体错误 0，正常退出。

截图：[画面面板](acceptance/panel-headings-20261010/native-14px/02-canvas-dark.png)、[声音面板](acceptance/panel-headings-20261010/native-14px/03-audio-dark.png)、[浅色字幕面板](acceptance/panel-headings-20261010/native-14px/09-subtitles-light.png)。

启动入口 `Start-Panzo.cmd` 已更新到 `target/desktop-v14-panel-headings-compact/release/panzo.exe`；旧版本保留，正在运行的程序未关闭。

EXE SHA-256：`8449EEB3291CE7C2B6CEC36131EDB71F12187087851B17BFA44AB8DE8ED4F522`。

