//! Bounded pin persistence. Exported screenshots never include this private edit state.
use super::{
    DestroyWindow, Document, GetWindowRect, HWND, ImageState, IsWindowVisible, PINS, Path, PathBuf,
    RECT, Raster, RefCell, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER,
    SetForegroundWindow, SetWindowPos, ShowWindow, picture_mut,
};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::rc::Rc;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
};
use windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST;
const MAX_PINS: usize = 24;
const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_CLOSED: usize = 12;
#[derive(Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // Serialized independent pin options.
pub(super) struct PinRecord {
    pub id: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub animated: bool,
    #[serde(default = "default_playing")]
    pub playing: bool,
    pub bounds: [i32; 4],
    pub opacity: u8,
    pub locked: bool,
    pub passthrough: bool,
    pub collapsed: bool,
    pub expanded_size: (i32, i32),
    pub visible: bool,
    pub edit: ImageState,
}
fn default_playing() -> bool {
    true
}
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Session {
    schema: u32,
    active: Vec<PinRecord>,
    closed: Vec<PinRecord>,
}
struct Context {
    library: PathBuf,
    session: Session,
    suspended: bool,
    shutting_down: bool,
    recovery_available: bool,
}
thread_local! {static CONTEXT:RefCell<Option<Context>>=const{RefCell::new(None)};}
pub(super) fn set_recovery_available(enabled: bool) {
    CONTEXT.with(|context| {
        if let Some(context) = context.borrow_mut().as_mut() {
            context.recovery_available = enabled;
        }
    });
}
pub(super) fn recovery_available() -> bool {
    CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .is_some_and(|context| context.recovery_available)
    })
}
pub(super) fn library() -> PathBuf {
    CONTEXT.with(|context| {
        context.borrow().as_ref().map_or_else(
            || std::env::temp_dir().join("Panzo"),
            |context| context.library.clone(),
        )
    })
}
fn directory() -> PathBuf {
    library().join(".panzo-pins")
}
fn persist(session: &Session) -> Result<(), String> {
    let directory = directory();
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    crate::platform::manifest_store::persist_json_atomically(
        directory.join("session.json"),
        session,
    )
    .map_err(|e| e.to_string())
}
fn enabled() -> bool {
    CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .is_some_and(|context| !context.suspended && !context.shutting_down)
    })
}
pub(super) fn configure(library: &Path, restore: bool) -> Vec<String> {
    CONTEXT.with(|context| {
        *context.borrow_mut() = Some(Context {
            library: library.into(),
            session: Session {
                schema: 1,
                ..Default::default()
            },
            suspended: true,
            shutting_down: false,
            recovery_available: false,
        });
    });
    let mut errors = Vec::new();
    if restore {
        let path = directory().join("session.json");
        if path.is_file() {
            match read_session(&path) {
                Ok(session) => {
                    CONTEXT
                        .with(|context| context.borrow_mut().as_mut().unwrap().session = session);
                }
                Err(error) => {
                    let backup = path.with_file_name(format!(
                        "session-unreadable-{}.json",
                        uuid::Uuid::new_v4()
                    ));
                    if std::fs::rename(&path, &backup).is_err() {
                        errors.push("贴图恢复文件无法读取且无法备份，本次不会覆盖它".into());
                        return errors;
                    }
                    errors.push(format!("贴图恢复文件已保留备份：{error}"));
                }
            }
        }
    }
    let records = CONTEXT.with(|context| context.borrow().as_ref().unwrap().session.active.clone());
    for record in records {
        match restore_record(&record) {
            Ok(_) => {}
            Err(error) => errors.push(format!("一张贴图暂未恢复：{error}")),
        }
    }
    // Preserve failed records for retry, never erase recovery data just because a monitor or file is unavailable.
    CONTEXT.with(|context| {
        let mut context = context.borrow_mut();
        let state = context.as_mut().unwrap();
        state.suspended = false;
    });
    errors
}
fn read_session(path: &Path) -> Result<Session, String> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.len() > 8 * 1024 * 1024 {
        return Err("贴图索引过大".into());
    }
    let session: Session = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if session.schema != 1 || session.active.len() > MAX_PINS || session.closed.len() > MAX_CLOSED {
        return Err("不支持的贴图索引版本或数量".into());
    }
    for record in session.active.iter().chain(&session.closed) {
        validate_record(record)?;
    }
    Ok(session)
}
fn validate_record(record: &PinRecord) -> Result<(), String> {
    if record.group.chars().count() > 64
        || uuid::Uuid::parse_str(&record.id).is_err()
        || record.id.len() != 36
        || !(40..=8192).contains(&record.bounds[2])
        || !(24..=8192).contains(&record.bounds[3])
        || record.bounds[0].abs_diff(0) > 100_000
        || record.bounds[1].abs_diff(0) > 100_000
    {
        return Err("贴图位置或资源标识无效".into());
    }
    Ok(())
}
pub(super) fn check_budget(bytes: usize) -> Result<(), String> {
    let windows = PINS.with(|pins| pins.borrow().clone());
    let reserved = CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .filter(|context| !context.suspended)
            .map_or(0, |context| context.session.active.len())
    });
    if windows.len().max(reserved) >= MAX_PINS {
        return Err("最多同时保留 24 张贴图，请先关闭不需要的贴图".into());
    }
    let used: usize = windows
        .iter()
        .filter_map(|hwnd| unsafe { picture_mut(*hwnd) })
        .map(|state| state.memory_bytes())
        .sum();
    if used.saturating_add(bytes) > MAX_BYTES {
        return Err("贴图原始图片合计超过 256 MB，请先关闭部分贴图".into());
    }
    Ok(())
}
pub(super) fn register(hwnd: HWND) -> Result<(), String> {
    let state = unsafe { picture_mut(hwnd) }.ok_or("贴图窗口不存在")?;
    if !enabled() {
        return Ok(());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let assets = directory().join("assets");
    std::fs::create_dir_all(&assets).map_err(|e| e.to_string())?;
    let used: u64 = std::fs::read_dir(&assets)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .map(|metadata| metadata.len())
        .sum();
    if used > 512 * 1024 * 1024 {
        return Err("贴图恢复资源超过 512 MB，请清理最近关闭的贴图后重试".into());
    }
    let asset = assets.join(format!("{id}.png"));
    state.document.borrow().original.save(&asset)?;
    if used.saturating_add(std::fs::metadata(&asset).map_err(|e| e.to_string())?.len())
        > 512 * 1024 * 1024
    {
        let _ = std::fs::remove_file(&asset);
        return Err("贴图恢复资源超过 512 MB，请从历史管理清理最近关闭贴图后重试".into());
    }
    if let Some(source) = &state.gif_path {
        let data = std::fs::read(source).map_err(|e| e.to_string())?;
        if used
            .saturating_add(data.len() as u64)
            .saturating_add(std::fs::metadata(&asset).map_err(|e| e.to_string())?.len())
            > 512 * 1024 * 1024
        {
            let _ = std::fs::remove_file(&asset);
            return Err("贴图恢复资源超过 512 MB".into());
        }
        let gif_asset = assets.join(format!("{id}.gif"));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&gif_asset)
            .map_err(|e| e.to_string())?;
        if let Err(e) = file.write_all(&data).and_then(|()| file.sync_all()) {
            drop(file);
            let _ = std::fs::remove_file(&gif_asset);
            let _ = std::fs::remove_file(&asset);
            return Err(e.to_string());
        }
        state.gif_path = Some(gif_asset);
    }
    state.pin_id = Some(id);
    save_session()
}
fn record(hwnd: HWND) -> Option<PinRecord> {
    let state = unsafe { picture_mut(hwnd) }?;
    let id = state.pin_id.clone()?;
    let mut rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &raw mut rect);
    }
    Some(PinRecord {
        id,
        group: state.group.clone(),
        animated: state.animation.is_some(),
        playing: state.animation.as_ref().is_none_or(|p| p.playing),
        bounds: [
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
        ],
        opacity: state.opacity,
        locked: state.locked,
        passthrough: state.passthrough,
        collapsed: state.collapsed,
        expanded_size: state.expanded_size,
        visible: unsafe { IsWindowVisible(hwnd) }.as_bool(),
        edit: state.document.borrow().state.clone(),
    })
}
pub(super) fn save_session() -> Result<(), String> {
    if !enabled() {
        return Ok(());
    }
    let records = PINS.with(|pins| {
        pins.borrow()
            .iter()
            .filter_map(|hwnd| record(*hwnd))
            .collect::<Vec<_>>()
    });
    let mut next = CONTEXT.with(|context| context.borrow().as_ref().unwrap().session.clone());
    for record in records {
        if let Some(old) = next.active.iter_mut().find(|old| old.id == record.id) {
            *old = record;
        } else {
            next.active.push(record);
        }
    }
    persist(&next)?;
    CONTEXT.with(|context| context.borrow_mut().as_mut().unwrap().session = next);
    Ok(())
}
pub(super) fn archive(hwnd: HWND) -> Result<(), String> {
    if !enabled() {
        return Ok(());
    }
    let Some(record) = record(hwnd) else {
        return Ok(());
    };
    let mut next = CONTEXT.with(|context| context.borrow().as_ref().unwrap().session.clone());
    next.active.retain(|entry| entry.id != record.id);
    next.closed.retain(|entry| entry.id != record.id);
    next.closed.insert(0, record);
    next.closed.truncate(MAX_CLOSED);
    persist(&next)?;
    CONTEXT.with(|context| context.borrow_mut().as_mut().unwrap().session = next);
    cleanup_assets();
    Ok(())
}
fn cleanup_assets() {
    let used = CONTEXT.with(|context| {
        let context = context.borrow();
        let session = &context.as_ref().unwrap().session;
        session
            .active
            .iter()
            .chain(&session.closed)
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>()
    });
    let assets = directory().join("assets");
    if let Ok(files) = std::fs::read_dir(&assets) {
        for entry in files.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "png" || extension == "gif")
                && let Some(name) = path.file_stem().and_then(|name| name.to_str())
                && uuid::Uuid::parse_str(name).is_ok()
                && !used.iter().any(|id| id == name)
            {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}
pub(super) fn shutdown() {
    if let Err(error) = save_session() {
        eprintln!("Panzo pin persistence: {error}");
    }
    CONTEXT.with(|context| {
        if let Some(context) = context.borrow_mut().as_mut() {
            context.shutting_down = true;
        }
    });
}
fn restore_record(record: &PinRecord) -> Result<HWND, String> {
    validate_record(record)?;
    let path = directory()
        .join("assets")
        .join(format!("{}.png", record.id));
    let document = Document::from_state(Raster::load(&path)?, record.edit.clone())?;
    let clip = if record.animated {
        let path = path.with_extension("gif");
        Some((crate::platform::gif_player::Clip::load(&path)?, path))
    } else {
        None
    };
    let hwnd = super::picture_window::create_picture_with_clip(
        Rc::new(RefCell::new(document)),
        &library(),
        true,
        clip,
    )?;
    let state = unsafe { picture_mut(hwnd) }.ok_or("无法创建贴图")?;
    state.pin_id = Some(record.id.clone());
    state.group.clone_from(&record.group);
    if let Some(player) = &mut state.animation
        && !record.playing
    {
        player.toggle();
    }
    state.animation_timer(hwnd);
    state.opacity = record.opacity.clamp(45, 255);
    state.locked = record.locked;
    // Always restore interactively after restart. Passthrough can be re-enabled after the recovery shortcut is available.
    state.passthrough = false;
    state.collapsed = record.collapsed;
    state.expanded_size = (
        record.expanded_size.0.clamp(40, 8192),
        record.expanded_size.1.clamp(24, 8192),
    );
    state.apply_pin_style(hwnd);
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            record.bounds[0],
            record.bounds[1],
            record.bounds[2],
            record.bounds[3],
            SWP_NOACTIVATE,
        );
        let _ = ShowWindow(
            hwnd,
            if record.visible {
                SW_SHOWNOACTIVATE
            } else {
                SW_HIDE
            },
        );
    }
    ensure_visible(hwnd);
    Ok(hwnd)
}
pub(super) fn clear_closed() -> Result<(), String> {
    if !enabled() {
        return Err("贴图恢复数据当前不可写入".into());
    }
    let mut next = CONTEXT.with(|context| context.borrow().as_ref().unwrap().session.clone());
    next.closed.clear();
    persist(&next)?;
    CONTEXT.with(|context| context.borrow_mut().as_mut().unwrap().session = next);
    cleanup_assets();
    Ok(())
}
pub(super) fn restore_closed(id: Option<&str>) -> Result<(), String> {
    let record = CONTEXT
        .with(|context| {
            context
                .borrow()
                .as_ref()
                .and_then(|context| {
                    context
                        .session
                        .closed
                        .iter()
                        .find(|entry| id.is_none_or(|id| entry.id == id))
                })
                .cloned()
        })
        .ok_or("暂无最近关闭的贴图")?;
    CONTEXT.with(|context| context.borrow_mut().as_mut().unwrap().suspended = true);
    let result = restore_record(&record);
    CONTEXT.with(|context| context.borrow_mut().as_mut().unwrap().suspended = false);
    let hwnd = result?;
    let mut next = CONTEXT.with(|context| context.borrow().as_ref().unwrap().session.clone());
    next.closed.retain(|entry| entry.id != record.id);
    next.active.push(record);
    if let Err(error) = persist(&next) {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return Err(error);
    }
    CONTEXT.with(|context| context.borrow_mut().as_mut().unwrap().session = next);
    Ok(())
}
pub(super) fn recover_interaction() -> Result<(), String> {
    for hwnd in PINS.with(|pins| pins.borrow().clone()) {
        if let Some(state) = unsafe { picture_mut(hwnd) } {
            state.passthrough = false;
            state.locked = false;
            state.opacity = 255;
            state.apply_pin_style(hwnd);
            ensure_visible(hwnd);
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
        }
    }
    save_session()
}
pub(super) fn ensure_visible(hwnd: HWND) {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &raw mut rect);
    }
    let monitor = unsafe { MonitorFromRect(&raw const rect, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &raw mut info) }.as_bool() {
        let bounds = clamp_bounds(
            [
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            ],
            [
                info.rcWork.left,
                info.rcWork.top,
                info.rcWork.right,
                info.rcWork.bottom,
            ],
        );
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                bounds[0],
                bounds[1],
                bounds[2],
                bounds[3],
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
    }
}
fn clamp_bounds(mut rect: [i32; 4], work: [i32; 4]) -> [i32; 4] {
    rect[2] = rect[2].clamp(40, (work[2] - work[0]).max(40));
    rect[3] = rect[3].clamp(24, (work[3] - work[1]).max(24));
    rect[0] = rect[0].clamp(work[0], (work[2] - rect[2]).max(work[0]));
    rect[1] = rect[1].clamp(work[1], (work[3] - rect[3]).max(work[1]));
    rect
}
pub(super) fn remember_image(image: &Raster) -> Result<PathBuf, String> {
    let folder = library().join("Screenshots");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let path = folder.join(format!("截图-{stamp}-{}.png", uuid::Uuid::new_v4()));
    image.save(&path)?;
    Ok(path)
}
pub(super) fn save_last_region(region: [i64; 4]) -> Result<(), String> {
    let folder = directory();
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    crate::platform::manifest_store::persist_json_atomically(
        folder.join("last-region.json"),
        &region,
    )
    .map_err(|e| e.to_string())
}
pub(super) fn load_last_region() -> Option<[i64; 4]> {
    let bytes = std::fs::read(directory().join("last-region.json")).ok()?;
    if bytes.len() > 1024 {
        return None;
    }
    let region: [i64; 4] = serde_json::from_slice(&bytes).ok()?;
    (region[0].abs_diff(0) < 100_000
        && region[1].abs_diff(0) < 100_000
        && (1..=32000).contains(&region[2])
        && (1..=32000).contains(&region[3]))
    .then_some(region)
}
pub(super) fn entries() -> Vec<(String, bool)> {
    CONTEXT.with(|context| {
        context.borrow().as_ref().map_or_else(Vec::new, |context| {
            context
                .session
                .active
                .iter()
                .map(|entry| (entry.id.clone(), true))
                .chain(
                    context
                        .session
                        .closed
                        .iter()
                        .map(|entry| (entry.id.clone(), false)),
                )
                .collect()
        })
    })
}
pub(super) fn show_pin(id: &str) -> Result<(), String> {
    for hwnd in PINS.with(|list| list.borrow().clone()) {
        if let Some(state) = unsafe { picture_mut(hwnd) }
            && state.pin_id.as_deref() == Some(id)
        {
            state.passthrough = false;
            state.apply_pin_style(hwnd);
            ensure_visible(hwnd);
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetForegroundWindow(hwnd);
            }
            return save_session();
        }
    }
    Err("该贴图未能恢复，可重新启动后重试".into())
}

pub(super) fn manage_groups(owner: HWND) -> Result<(), String> {
    let windows = PINS.with(|p| p.borrow().clone());
    let mut groups = windows
        .iter()
        .filter_map(|h| unsafe { picture_mut(*h) })
        .map(|s| s.group.clone())
        .collect::<Vec<_>>();
    groups.sort();
    groups.dedup();
    if groups.is_empty() {
        return Err("请先创建贴图，可通过贴图右键菜单设置分组".into());
    }
    let names = groups
        .iter()
        .map(|s| {
            if s.is_empty() {
                "（未分组）"
            } else {
                s.as_str()
            }
        })
        .collect::<Vec<_>>();
    let Some(values) = crate::platform::editor_dialog::form_with_description(
        owner,
        "贴图分组管理",
        "选择一组统一操作。分组、播放状态与位置会随贴图恢复。",
        &[("分组", names[0].into()), ("操作", "显示".into())],
        &[(0, &names), (1, &["显示", "隐藏", "排列", "恢复交互"])],
        "应用",
    )
    .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    let group = &groups[names
        .iter()
        .position(|v| *v == values[0])
        .ok_or("请选择分组")?];
    apply_group(group, &values[1])
}
fn apply_group(group: &str, action: &str) -> Result<(), String> {
    let windows = PINS.with(|p| p.borrow().clone());
    let mut index = 0;
    for hwnd in windows {
        if let Some(state) = unsafe { picture_mut(hwnd) }
            && state.group == group
        {
            match action {
                "隐藏" => unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                },
                "排列" => {
                    unsafe {
                        let _ = SetWindowPos(
                            hwnd,
                            None,
                            40 + index * 36,
                            80 + index * 36,
                            0,
                            0,
                            SWP_NOACTIVATE
                                | SWP_NOZORDER
                                | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
                        );
                        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    }
                    ensure_visible(hwnd);
                    index += 1;
                }
                _ => {
                    if action == "恢复交互" {
                        state.passthrough = false;
                        state.locked = false;
                        state.opacity = 255;
                        state.apply_pin_style(hwnd);
                    }
                    ensure_visible(hwnd);
                    unsafe {
                        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    }
                }
            }
        }
    }
    save_session()
}

pub(super) fn manage_groups_async() -> Result<(), String> {
    let mut groups = PINS
        .with(|p| p.borrow().clone())
        .iter()
        .filter_map(|h| unsafe { picture_mut(*h) })
        .map(|s| s.group.clone())
        .collect::<Vec<_>>();
    groups.sort();
    groups.dedup();
    if groups.is_empty() {
        return Err("请先创建桌面贴图".into());
    }
    let names = groups
        .iter()
        .map(|s| {
            if s.is_empty() {
                "（未分组）"
            } else {
                s.as_str()
            }
        })
        .collect::<Vec<_>>();
    let lookup = names.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    crate::platform::workspace_form::open(
        2,
        crate::platform::workspace_form::FormId::PinGroups,
        "贴图分组管理",
        "选择分组，统一显示、隐藏、排列或恢复交互。",
        &[("分组", names[0].into()), ("操作", "显示".into())],
        &[(0, &names), (1, &["显示", "隐藏", "排列", "恢复交互"])],
        &[],
        &["应用"],
        Box::new(move |_, v| {
            let index = lookup.iter().position(|n| n == &v[0]).ok_or("请选择分组")?;
            let group = if lookup[index] == "（未分组）" {
                ""
            } else {
                &lookup[index]
            };
            apply_group(group, &v[1])?;
            Ok("分组操作已完成".into())
        }),
    )?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unplugged_monitor_moves_pin_inside_negative_origin_work_area() {
        assert_eq!(
            clamp_bounds([5000, 2000, 800, 500], [-1920, 0, 0, 1080]),
            [-800, 580, 800, 500]
        );
    }
    #[test]
    fn external_resource_paths_are_rejected() {
        let record = PinRecord {
            id: "../../private".into(),
            group: String::new(),
            animated: false,
            playing: true,
            bounds: [0, 0, 100, 100],
            opacity: 255,
            locked: false,
            passthrough: false,
            collapsed: false,
            expanded_size: (100, 100),
            visible: true,
            edit: ImageState::default(),
        };
        assert!(validate_record(&record).is_err());
    }
}

/// Preview a pin without restoring it or creating a document window.
pub(super) fn preview_image(id: &str) -> Result<Raster, String> {
    for hwnd in PINS.with(|pins| pins.borrow().clone()) {
        if let Some(state) = unsafe { picture_mut(hwnd) }
            && state.pin_id.as_deref() == Some(id)
        {
            return state.output();
        }
    }
    let record = CONTEXT
        .with(|context| {
            context.borrow().as_ref().and_then(|context| {
                context
                    .session
                    .active
                    .iter()
                    .chain(&context.session.closed)
                    .find(|r| r.id == id)
                    .cloned()
            })
        })
        .ok_or("贴图恢复记录不存在")?;
    validate_record(&record)?;
    let path = directory().join("assets").join(format!("{id}.png"));
    Document::from_state(Raster::load(&path)?, record.edit)?.render(None)
}
