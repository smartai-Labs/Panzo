//! Persistent, exact-match action bindings and transactional Win32 hotkey registration.
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::BTreeMap, path::PathBuf};
use windows::Win32::{
    Foundation::HWND,
    UI::Input::KeyboardAndMouse::{
        GetKeyState, HOT_KEY_MODIFIERS, IsWindowEnabled, MOD_NOREPEAT, RegisterHotKey,
        UnregisterHotKey,
    },
};

pub(crate) const CTRL: u32 = 2;
pub(crate) const ALT: u32 = 1;
pub(crate) const SHIFT: u32 = 4;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct Chord {
    pub modifiers: u32,
    pub key: u32,
}
impl Chord {
    pub fn label(self) -> String {
        let mut s = String::new();
        for (bit, name) in [(CTRL, "Ctrl+"), (ALT, "Alt+"), (SHIFT, "Shift+")] {
            if self.modifiers & bit != 0 {
                s.push_str(name);
            }
        }
        s.push_str(&key_name(self.key));
        s
    }
}
pub(crate) fn key_name(key: u32) -> String {
    match key {
        0x30..=0x39 | 0x41..=0x5a => char::from_u32(key).unwrap().to_string(),
        0x70..=0x87 => format!("F{}", key - 0x6f),
        0x20 => "Space".into(),
        0x2e => "Delete".into(),
        0x24 => "Home".into(),
        0x23 => "End".into(),
        0x25 => "Left".into(),
        0x27 => "Right".into(),
        0x26 => "Up".into(),
        0x28 => "Down".into(),
        0xbc => ",".into(),
        0xbe => ".".into(),
        0xbb => "=".into(),
        0xbd => "-".into(),
        _ => String::new(),
    }
}
pub(crate) fn current_chord(key: u32) -> Chord {
    let mut modifiers = 0;
    for (vk, bit) in [(0x11, CTRL), (0x12, ALT), (0x10, SHIFT)] {
        if unsafe { GetKeyState(vk) } < 0 {
            modifiers |= bit;
        }
    }
    Chord { modifiers, key }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Scope {
    App,
    Global,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Binding {
    pub chord: Option<Chord>,
    pub scope: Scope,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Shell(u32),
    Workspace(usize),
    Image(usize),
    Picture(usize),
    Video(i32),
}
pub(crate) struct Action {
    pub id: &'static str,
    pub label: &'static str,
    pub category: usize,
    pub target: Target,
    pub global: bool,
    default: Option<Chord>,
    scope: Scope,
}
macro_rules! a {
    ($id:literal,$label:literal,$cat:expr,$target:expr,$global:expr,$mods:expr,$key:expr,$scope:ident) => {
        Action {
            id: $id,
            label: $label,
            category: $cat,
            target: $target,
            global: $global,
            default: if $key == 0 {
                None
            } else {
                Some(Chord {
                    modifiers: $mods,
                    key: $key,
                })
            },
            scope: Scope::$scope,
        }
    };
}
use Target::{Image as I, Picture as P, Shell as S, Video as V, Workspace as W};
pub(crate) static ACTIONS: &[Action] = &[
    a!(
        "capture",
        "区域截图",
        1,
        S(1),
        true,
        CTRL | ALT | SHIFT,
        0x78,
        Global
    ),
    a!(
        "record",
        "区域录屏 / 停止录制",
        1,
        S(2),
        true,
        CTRL | ALT | SHIFT,
        0x79,
        Global
    ),
    a!(
        "pause_record",
        "暂停 / 继续录制",
        1,
        S(13),
        true,
        CTRL | SHIFT,
        0x20,
        App
    ),
    a!(
        "follow_mouse",
        "切换鼠标跟随缩放",
        1,
        S(14),
        true,
        0,
        0,
        App
    ),
    a!(
        "pin_clipboard",
        "贴出剪贴板图片",
        1,
        S(3),
        true,
        CTRL | SHIFT,
        0x50,
        App
    ),
    a!(
        "toggle_pins",
        "隐藏 / 显示全部贴图",
        1,
        S(4),
        true,
        0,
        0,
        App
    ),
    a!("recover_pins", "恢复贴图交互", 1, S(5), true, 0, 0, App),
    a!(
        "restore_pin",
        "恢复最近关闭的贴图",
        1,
        S(7),
        true,
        0,
        0,
        App
    ),
    a!("show_app", "显示 Panzo", 1, S(10), true, 0, 0, App),
    a!("delayed_capture", "延时截图", 1, S(6), true, 0, 0, App),
    a!("rotate_image", "旋转图片", 3, P(40), false, 0, 0, App),
    a!(
        "flip_horizontal",
        "图片左右翻转",
        3,
        P(41),
        false,
        0,
        0,
        App
    ),
    a!("flip_vertical", "图片上下翻转", 3, P(42), false, 0, 0, App),
    a!("restore_crop", "恢复图片裁剪", 3, P(43), false, 0, 0, App),
    a!(
        "pin_image",
        "将当前图片贴到桌面",
        3,
        P(11),
        false,
        0,
        0,
        App
    ),
    a!(
        "play_gif",
        "播放 / 暂停 GIF",
        3,
        P(47),
        false,
        CTRL,
        0x20,
        App
    ),
    a!(
        "previous_frame",
        "视频上一帧",
        4,
        V(246),
        false,
        0,
        0x25,
        App
    ),
    a!("next_frame", "视频下一帧", 4, V(247), false, 0, 0x27, App),
    a!(
        "previous_clip",
        "上一个视频片段边界",
        4,
        V(248),
        false,
        CTRL,
        0x25,
        App
    ),
    a!(
        "next_clip",
        "下一个视频片段边界",
        4,
        V(249),
        false,
        CTRL,
        0x27,
        App
    ),
    a!(
        "seek_backward",
        "视频向后跳转",
        4,
        V(250),
        false,
        SHIFT,
        0x25,
        App
    ),
    a!(
        "seek_forward",
        "视频向前跳转",
        4,
        V(251),
        false,
        SHIFT,
        0x27,
        App
    ),
    a!(
        "images",
        "切换到图片工具",
        2,
        W(100),
        false,
        CTRL | ALT,
        0x31,
        App
    ),
    a!(
        "video",
        "切换到录制与视频",
        2,
        W(101),
        false,
        CTRL | ALT,
        0x32,
        App
    ),
    a!(
        "assets",
        "切换到素材",
        2,
        W(102),
        false,
        CTRL | ALT,
        0x33,
        App
    ),
    a!("settings", "打开设置", 2, W(116), false, CTRL, 0xbc, App),
    a!("plugins", "打开插件管理", 2, W(115), false, 0, 0, App),
    a!(
        "shortcuts",
        "打开快捷键设置",
        2,
        W(220),
        false,
        CTRL | ALT,
        0x4b,
        App
    ),
    a!("theme", "切换浅色 / 深色主题", 2, W(110), false, 0, 0, App),
    a!(
        "save",
        "保存当前图片 / 工程",
        2,
        W(202),
        false,
        CTRL,
        0x53,
        App
    ),
    a!("undo", "撤销", 2, W(207), false, CTRL, 0x5a, App),
    a!("redo", "重做", 2, W(208), false, CTRL, 0x59, App),
    a!(
        "copy_image",
        "复制当前图片",
        3,
        W(209),
        false,
        CTRL,
        0x43,
        App
    ),
    a!(
        "open_image",
        "打开图片 / GIF",
        3,
        I(2),
        false,
        CTRL,
        0x4f,
        App
    ),
    a!("audio_tools", "声音与旁白", 4, V(260), false, 0, 0, App),
    a!(
        "subtitle_tools",
        "字幕与 AI 自动字幕",
        4,
        V(261),
        false,
        0,
        0,
        App
    ),
    a!("scroll_capture", "滚动长截图", 3, I(3), false, 0, 0, App),
    a!(
        "collage",
        "打开拼图",
        3,
        I(4),
        false,
        CTRL | SHIFT,
        0x4a,
        App
    ),
    a!("batch", "打开批处理", 3, I(5), false, 0, 0, App),
    a!(
        "close_image",
        "关闭当前图片",
        3,
        W(203),
        false,
        CTRL,
        0x57,
        App
    ),
    a!(
        "recognize",
        "开始 OCR / 多模态识别",
        3,
        I(7),
        false,
        CTRL | SHIFT,
        0x4f,
        App
    ),
    a!(
        "recognition_panel",
        "显示 / 隐藏识别面板",
        3,
        W(212),
        false,
        0,
        0,
        App
    ),
    a!(
        "quick_save",
        "快速保存到截图历史",
        3,
        P(44),
        false,
        CTRL | SHIFT,
        0x53,
        App
    ),
    a!(
        "fit_image",
        "图片适应窗口",
        3,
        P(63),
        false,
        CTRL,
        0x30,
        App
    ),
    a!(
        "actual_image",
        "图片实际大小",
        3,
        P(64),
        false,
        CTRL,
        0x31,
        App
    ),
    a!("zoom_in", "图片放大", 3, P(66), false, CTRL, 0xbb, App),
    a!("zoom_out", "图片缩小", 3, P(65), false, CTRL, 0xbd, App),
    a!("select", "选择标注", 3, P(30), false, 0, 0x56, App),
    a!("crop", "裁剪图片", 3, P(36), false, 0, 0x43, App),
    a!("pen", "画笔", 3, P(3), false, 0, 0, App),
    a!("rectangle", "矩形标注", 3, P(1), false, 0, 0, App),
    a!("ellipse", "椭圆标注", 3, P(31), false, 0, 0, App),
    a!("arrow", "箭头标注", 3, P(2), false, 0, 0, App),
    a!("line", "直线标注", 3, P(32), false, 0, 0, App),
    a!("text", "文字标注", 3, P(4), false, 0, 0x54, App),
    a!("number", "编号标注", 3, P(34), false, 0, 0, App),
    a!("mosaic", "马赛克", 3, P(5), false, 0, 0x4d, App),
    a!("blur", "模糊", 3, P(35), false, 0, 0, App),
    a!("highlight", "高亮", 3, P(33), false, 0, 0, App),
    a!("cover", "遮盖", 3, P(6), false, 0, 0, App),
    a!(
        "delete",
        "删除选中的标注 / 视频片段",
        2,
        W(240),
        false,
        0,
        0x2e,
        App
    ),
    a!(
        "import_video",
        "导入视频 / 图片",
        4,
        W(230),
        false,
        CTRL | SHIFT,
        0x49,
        App
    ),
    a!(
        "open_project",
        "打开视频工程",
        4,
        W(231),
        false,
        CTRL | ALT,
        0x4f,
        App
    ),
    a!(
        "export_video",
        "导出视频 / GIF",
        4,
        W(211),
        false,
        CTRL,
        0x45,
        App
    ),
    a!("play", "播放 / 暂停视频", 4, V(241), false, 0, 0x20, App),
    a!("split", "分割所选片段", 4, V(242), false, 0, 0x42, App),
    a!("video_home", "回到视频起点", 4, V(243), false, 0, 0x24, App),
    a!(
        "reset_camera",
        "恢复自动镜头",
        4,
        V(244),
        false,
        0,
        0x52,
        App
    ),
];
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Config {
    pub version: u32,
    pub bindings: BTreeMap<String, Binding>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            bindings: ACTIONS
                .iter()
                .map(|a| {
                    (
                        a.id.into(),
                        Binding {
                            chord: a.default,
                            scope: a.scope,
                        },
                    )
                })
                .collect(),
        }
    }
}
impl Config {
    pub fn binding(&self, a: &Action) -> Binding {
        self.bindings.get(a.id).cloned().unwrap_or(Binding {
            chord: a.default,
            scope: a.scope,
        })
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("快捷键配置版本不受支持".into());
        }
        let mut used = BTreeMap::new();
        for a in ACTIONS {
            let b = self.binding(a);
            let Some(c) = b.chord else { continue };
            if key_name(c.key).is_empty() || c.modifiers & !7 != 0 {
                return Err(format!(
                    "{}：不支持此按键，Esc、Enter、Tab 和 Windows 组合键保留给系统及界面操作",
                    a.label
                ));
            }
            if b.scope == Scope::Global
                && (!a.global || c.modifiers & (CTRL | ALT) == 0 || c.key == 0x7b)
            {
                return Err(format!(
                    "{}：全局快捷键需要 Ctrl 或 Alt，且不能使用 F12",
                    a.label
                ));
            }
            if (c.modifiers == ALT && c.key == 0x73)
                || (c.modifiers == (CTRL | ALT) && c.key == 0x2e)
            {
                return Err(format!("{}：此组合保留给系统", a.label));
            }
            if let Some(label) = used.insert(c, a.label) {
                return Err(format!(
                    "{} 与 {} 使用了相同的 {}，请修改其中一项",
                    label,
                    a.label,
                    c.label()
                ));
            }
        }
        Ok(())
    }
}
fn path() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|p| PathBuf::from(p).join("Panzo/shortcuts.json"))
        .ok_or("找不到快捷键设置目录".into())
}
fn load() -> Result<Config, String> {
    let p = path()?;
    if !p.exists() {
        return Ok(Config::default());
    }
    if std::fs::metadata(&p).map_err(|e| e.to_string())?.len() > 131072 {
        return Err("快捷键配置文件过大".into());
    }
    let c: Config = serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?)
        .map_err(|e| format!("快捷键配置无法读取：{e}"))?;
    c.validate()?;
    Ok(c)
}
fn persist(c: &Config) -> Result<(), String> {
    let p = path()?;
    std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    super::manifest_store::persist_json_atomically(&p, c).map_err(|e| e.to_string())
}
struct Registered {
    id: i32,
    chord: Chord,
    action: usize,
}
struct Runtime {
    hwnd: HWND,
    config: Config,
    registered: Vec<Registered>,
    next_id: i32,
    issues: Vec<String>,
}
thread_local! {static RUNTIME:RefCell<Option<Runtime>>=const{RefCell::new(None)};}
fn register(h: HWND, id: i32, c: Chord) -> Result<(), String> {
    unsafe {
        RegisterHotKey(
            Some(h),
            id,
            HOT_KEY_MODIFIERS(c.modifiers) | MOD_NOREPEAT,
            c.key,
        )
    }
    .map_err(|_| format!("{} 无法注册，可能已被其他软件或系统占用", c.label()))
}
pub(crate) fn install(hwnd: HWND) -> Vec<String> {
    let mut issues = Vec::new();
    let config = load().unwrap_or_else(|e| {
        issues.push(format!("{e}；本次使用默认快捷键，原文件保留"));
        Config::default()
    });
    let mut r = Runtime {
        hwnd,
        config,
        registered: Vec::new(),
        next_id: 5000,
        issues: Vec::new(),
    };
    for (i, a) in ACTIONS.iter().enumerate() {
        let b = r.config.binding(a);
        if b.scope == Scope::Global {
            if let Some(c) = b.chord {
                let id = r.next_id;
                r.next_id += 1;
                match register(hwnd, id, c) {
                    Ok(()) => r.registered.push(Registered {
                        id,
                        chord: c,
                        action: i,
                    }),
                    Err(e) => issues.push(format!("{}：{e}", a.label)),
                }
            }
        }
    }
    r.issues = issues.clone();
    RUNTIME.with(|v| *v.borrow_mut() = Some(r));
    issues
}
pub(crate) fn uninstall() {
    RUNTIME.with(|v| {
        if let Some(r) = v.borrow_mut().take() {
            for b in r.registered {
                unsafe {
                    let _ = UnregisterHotKey(Some(r.hwnd), b.id);
                }
            }
        }
    });
}
pub(crate) fn config() -> Config {
    RUNTIME.with(|v| {
        v.borrow()
            .as_ref()
            .map(|r| r.config.clone())
            .unwrap_or_default()
    })
}
pub(crate) fn issues() -> String {
    RUNTIME.with(|v| {
        v.borrow()
            .as_ref()
            .map(|r| r.issues.join("\n"))
            .unwrap_or_default()
    })
}
pub(crate) fn apply(config: Config) -> Result<(), String> {
    config.validate()?;
    RUNTIME.with(|v| {
        let mut slot = v.borrow_mut();
        let r = slot.as_mut().ok_or("快捷键服务尚未启动")?;
        let mut next = Vec::new();
        let mut added = Vec::new();
        let result = (|| {
            for (i, a) in ACTIONS.iter().enumerate() {
                let b = config.binding(a);
                if b.scope != Scope::Global {
                    continue;
                }
                let Some(c) = b.chord else { continue };
                let id = if let Some(old) = r.registered.iter().find(|old| old.chord == c) {
                    old.id
                } else {
                    if r.next_id >= 0xbfff {
                        return Err("请重启后继续修改快捷键".into());
                    }
                    let id = r.next_id;
                    r.next_id += 1;
                    register(r.hwnd, id, c)
                        .map_err(|e| format!("{}：{e}。本次修改未保存，原快捷键仍有效", a.label))?;
                    added.push(id);
                    id
                };
                next.push(Registered {
                    id,
                    chord: c,
                    action: i,
                });
            }
            persist(&config)
        })();
        if let Err(e) = result {
            for id in added {
                unsafe {
                    let _ = UnregisterHotKey(Some(r.hwnd), id);
                }
            }
            return Err(e);
        }
        for old in &r.registered {
            if !next.iter().any(|n| n.id == old.id) {
                unsafe {
                    let _ = UnregisterHotKey(Some(r.hwnd), old.id);
                }
            }
        }
        r.registered = next;
        r.config = config;
        r.issues.clear();
        Ok(())
    })
}
pub(crate) fn global(id: i32) -> Option<(usize, Chord)> {
    RUNTIME.with(|v| {
        v.borrow().as_ref().and_then(|r| {
            r.registered
                .iter()
                .find(|b| b.id == id)
                .map(|b| (b.action, b.chord))
        })
    })
}
pub(crate) fn globally_active(action: &str) -> bool {
    RUNTIME.with(|v| {
        v.borrow()
            .as_ref()
            .is_some_and(|r| r.registered.iter().any(|b| ACTIONS[b.action].id == action))
    })
}
pub(crate) fn app_action(c: Chord) -> Option<usize> {
    let cfg = config();
    ACTIONS.iter().position(|a| {
        let b = cfg.binding(a);
        b.scope == Scope::App && b.chord == Some(c)
    })
}
pub(crate) fn label(id: &str) -> String {
    ACTIONS
        .iter()
        .find(|a| a.id == id)
        .and_then(|a| config().binding(a).chord)
        .map(Chord::label)
        .unwrap_or_default()
}
pub(crate) fn shell_shortcut(command: u32) -> String {
    ACTIONS
        .iter()
        .find(|a| a.target == S(command))
        .map(|a| self::label(a.id))
        .unwrap_or_default()
}

pub(crate) fn text_reserves(c: Chord) -> bool {
    c.modifiers == 0
        || (c.modifiers & CTRL != 0
            && c.modifiers & ALT == 0
            && matches!(
                c.key,
                0x41 | 0x43 | 0x56 | 0x58 | 0x5a | 0x59 | 0x25..=0x28 | 0x24 | 0x23 | 0x2e
            ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_are_unique_and_only_two_rare_global_combinations() {
        let c = Config::default();
        c.validate().unwrap();
        let globals: Vec<_> = ACTIONS
            .iter()
            .filter(|a| c.binding(a).scope == Scope::Global)
            .collect();
        assert_eq!(globals.len(), 2);
        for a in globals {
            assert_eq!(c.binding(a).chord.unwrap().modifiers, CTRL | ALT | SHIFT);
        }
    }
    #[test]
    fn duplicate_scopes_invalid_and_reserved_keys_are_rejected() {
        let mut c = Config::default();
        let first = c.binding(&ACTIONS[0]);
        c.bindings.insert(
            "undo".into(),
            Binding {
                scope: Scope::App,
                ..first
            },
        );
        assert!(c.validate().unwrap_err().contains("相同"));
        for (m, k) in [(ALT, 0x73), (CTRL | ALT, 0x2e), (8, 0x41), (0, 27)] {
            let mut c = Config::default();
            c.bindings.insert(
                "undo".into(),
                Binding {
                    chord: Some(Chord {
                        modifiers: m,
                        key: k,
                    }),
                    scope: Scope::App,
                },
            );
            assert!(c.validate().is_err());
        }
    }
    #[test]
    fn edits_disabled_bindings_and_scope_survive_serialization() {
        let mut c = Config::default();
        c.bindings.insert(
            "capture".into(),
            Binding {
                chord: None,
                scope: Scope::Global,
            },
        );
        c.bindings.insert(
            "collage".into(),
            Binding {
                chord: Some(Chord {
                    modifiers: CTRL | ALT,
                    key: 0x4a,
                }),
                scope: Scope::App,
            },
        );
        c.validate().unwrap();
        let restored: Config = serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
        assert_eq!(restored.bindings, c.bindings);
        assert_eq!(restored.binding(&ACTIONS[0]).chord, None);
    }
    #[test]
    fn exact_modifiers_and_native_text_editing_are_distinct() {
        assert!(text_reserves(Chord {
            modifiers: CTRL,
            key: 0x5a
        }));
        assert!(!text_reserves(Chord {
            modifiers: CTRL | ALT,
            key: 0x5a
        }));
        assert_ne!(
            Chord {
                modifiers: CTRL,
                key: 0x53
            },
            Chord {
                modifiers: CTRL | SHIFT,
                key: 0x53
            }
        );
    }
}
pub(crate) fn dialog_message(msg: &windows::Win32::UI::WindowsAndMessaging::MSG) -> bool {
    use windows::Win32::{
        Foundation::{LPARAM, WPARAM},
        UI::WindowsAndMessaging::*,
    };
    if super::shortcut_settings::dialog_message(msg) {
        return true;
    }
    if !matches!(msg.message, WM_KEYDOWN | WM_SYSKEYDOWN) {
        return false;
    }
    let Some(root) = super::workspace::root() else {
        return false;
    };
    if unsafe { GetAncestor(msg.hwnd, GA_ROOT) } != root
        || !unsafe { IsWindowEnabled(root) }.as_bool()
    {
        return false;
    }
    if unsafe { GetKeyState(0x5b) } < 0 || unsafe { GetKeyState(0x5c) } < 0 {
        return false;
    }
    let chord = current_chord(msg.wParam.0 as u32);
    let mut cls = [0u16; 80];
    let n = unsafe { GetClassNameW(msg.hwnd, &mut cls) };
    let class = String::from_utf16_lossy(&cls[..n.max(0) as usize]).to_lowercase();
    if class == "listbox" && chord.modifiers == 0 && chord.key == 0x2e {
        if let Ok(parent)=unsafe{GetParent(msg.hwnd)} {
            let mut name=[0u16;64];let n=unsafe{GetClassNameW(parent,&mut name)};
            if String::from_utf16_lossy(&name[..n.max(0) as usize])=="PanzoProjectBin" {return false;}
        }
    }
    let input = class.contains("edit") || class == "combobox";
    if input && (text_reserves(chord) || unsafe { GetKeyState(0xa5) } < 0) {
        return false;
    }
    if chord.modifiers == 0
        && ((chord.key == 0x20 && class == "button")
            || super::workspace::video_handle()
                .is_some_and(|h| super::preview_window::keyboard_key_is_control(h, chord.key)))
    {
        return false;
    }
    let Some(index) = app_action(chord) else {
        return false;
    };
    if !super::workspace::shortcut_context(index) {
        return false;
    }
    // Avoid key-repeat toggling recording, opening several dialogs, or deleting repeatedly.
    if msg.lParam.0 & (1 << 30) == 0 {
        unsafe {
            let _ = PostMessageW(Some(root), WM_COMMAND, WPARAM(6000 + index), LPARAM(0));
        }
    }
    true
}
