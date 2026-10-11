//! Opt-in desktop regression through the production host and its message loop.
use super::*;
use crate::platform::{
    capture_tools, image_tasks::Output, image_workspace, workspace, workspace_ui as ui,
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::Win32::UI::WindowsAndMessaging::{
    CB_SETCURSEL, CBN_SELCHANGE, EnumWindows, GA_ROOT, GetAncestor, GetClassNameW, GetDlgItem,
    GetWindowThreadProcessId, WM_APP, WM_COMMAND,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, IsZoomed, SW_MAXIMIZE, SW_RESTORE, ShowWindow,
};
use windows::Win32::UI::WindowsAndMessaging::{
    HTCAPTION, HTCLOSE, HTMAXBUTTON, HTMINBUTTON, SendMessageW, WM_NCHITTEST,
};
const MESSAGE: u32 = WM_APP + 181;
type Step = Box<dyn FnOnce(HWND) -> Result<Value, String> + Send>;
struct Envelope {
    step: Step,
    reply: Sender<Result<Value, String>>,
}
thread_local! {static MAILBOX:RefCell<Option<Receiver<Envelope>>>=const{RefCell::new(None)};}
pub(super) fn handle(hwnd: HWND, msg: u32) -> Option<LRESULT> {
    if msg != MESSAGE {
        return None;
    }
    let envelope = MAILBOX.with(|m| m.borrow().as_ref().and_then(|m| m.try_recv().ok()));
    if let Some(e) = envelope {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (e.step)(hwnd)))
            .unwrap_or_else(|_| Err("workspace UI assertion failed".into()));
        let _ = e.reply.send(result);
    }
    Some(LRESULT(0))
}
struct Driver {
    hwnd: usize,
    tx: Sender<Envelope>,
}
impl Driver {
    fn step(
        &self,
        step: impl FnOnce(HWND) -> Result<Value, String> + Send + 'static,
    ) -> Result<Value, String> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(Envelope {
                step: Box::new(step),
                reply: tx,
            })
            .map_err(|e| e.to_string())?;
        unsafe {
            PostMessageW(
                Some(HWND(self.hwnd as *mut _)),
                MESSAGE,
                WPARAM(0),
                LPARAM(0),
            )
            .map_err(|e| e.to_string())?;
        }
        rx.recv_timeout(Duration::from_secs(20))
            .map_err(|e| e.to_string())?
    }
    #[allow(clippy::too_many_lines)] // One desktop flow follows install, processing, and data retention.
    fn plugin_flow(&self, package: PathBuf, out: &Path) -> Result<(), String> {
        self.step(move |_| {
            let package = crate::platform::plugins::read_package(&package)?;
            crate::platform::plugin_ui::preview_install(package)?;
            Ok(Value::Null)
        })?;
        self.step(|_| {
            let form = workspace::review_form(4);
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    form,
                    WM_COMMAND,
                    Some(WPARAM(1)),
                    None,
                );
            }
            Ok(Value::Null)
        })?;
        let shot = out.join("plugin-manager.png");
        self.step(move |_| {
            capture_tools::review_window_png(workspace::root().unwrap(), &shot);
            if !crate::platform::plugins::enabled("org.panzo.image-filters") {
                return Err("plugin was not installed through UI".into());
            }
            crate::platform::plugin_ui::image_tools()?;
            Ok(Value::Null)
        })?;
        self.step(|_| {
            let form = workspace::review_form(0);
            let panel = workspace::image_parent().unwrap();
            let chooser = unsafe { GetDlgItem(Some(panel), 20) }.map_err(|e| e.to_string())?;
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    chooser,
                    CB_SETCURSEL,
                    Some(WPARAM(1)),
                    None,
                );
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    panel,
                    WM_COMMAND,
                    Some(WPARAM(0x0014 | ((CBN_SELCHANGE as usize) << 16))),
                    None,
                );
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    form,
                    WM_COMMAND,
                    Some(WPARAM(1)),
                    None,
                );
            }
            workspace::switch(1);
            Ok(Value::Null)
        })?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let ready = self.step(|_| {
                Ok(json!(
                    image_workspace::review()["documents"]
                        .as_array()
                        .unwrap()
                        .len()
                        == 3
                ))
            })?;
            if ready == json!(true) {
                break;
            }
            if Instant::now() > deadline {
                return Err("plugin output was not added to workspace".into());
            }
            thread::sleep(Duration::from_millis(80));
        }
        self.step(|h| {
            let image = image_workspace::review();
            if image["selected"] != json!(1)
                || !image["documents"][1]["status"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("图片 2")
                || image["documents"][0]["text"] != json!("人工校对结果")
            {
                return Err("plugin result changed current image or edited OCR text".into());
            }
            crate::platform::plugins::with_store(|s| {
                s.set_enabled("org.panzo.image-filters", false)
            })?;
            if crate::platform::plugins::acquire("org.panzo.image-filters").is_ok() {
                return Err("disabled plugin accepted new task".into());
            }
            let saved = fs::read_dir(
                unsafe { state_mut(h) }
                    .unwrap()
                    .library_root
                    .join("Screenshots"),
            )
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect::<Vec<_>>();
            if saved.is_empty() {
                return Err("plugin output was not saved to the asset library".into());
            }
            crate::platform::plugins::with_store(|s| s.uninstall("org.panzo.image-filters"))?;
            if saved.iter().any(|p| !p.exists()) {
                return Err("uninstall removed saved output".into());
            }
            if image_workspace::review()["documents"]
                .as_array()
                .unwrap()
                .len()
                != 3
            {
                return Err("uninstall deleted image data".into());
            }
            workspace::switch(0);
            Ok(Value::Null)
        })?;
        let shot = out.join("plugin-result-retained.png");
        self.step(move |_| {
            capture_tools::review_window_png(workspace::root().unwrap(), &shot);
            Ok(Value::Null)
        })?;
        Ok(())
    }
    #[allow(clippy::too_many_lines)] // End-to-end sequence intentionally verifies persistent state across steps.
    fn run(&self, out: &Path) -> Result<Value, String> {
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            if self.step(|h| {
                Ok(json!(
                    unsafe { state_mut(h) }.is_some_and(|s| s.editor.is_some())
                ))
            })? == json!(true)
            {
                break;
            }
            if Instant::now() > deadline {
                return Err("project load timed out".into());
            }
            thread::sleep(Duration::from_millis(100));
        }
        let initial = self.step(|h| {
            let state = unsafe { state_mut(h) }.ok_or("host missing")?;
            let e = state.editor.as_mut().ok_or("editor missing")?;
            e.review_edit_during_save()?;
            let snap = e.review_snapshot();
            Ok(json!({"generation":snap.generation,"tick":snap.project_tick,"dirty":snap.dirty}))
        })?;
        let a = self
            .step(|h| {
                let library = unsafe { state_mut(h) }.unwrap().library_root.clone();
                let pic =
                    capture_tools::show_image(capture_tools::review_ocr_image(), &library, false)?;
                if unsafe { GetAncestor(pic, GA_ROOT) } != workspace::root().unwrap() {
                    return Err("image was not embedded".into());
                }
                unsafe {
                    windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                        pic,
                        WM_COMMAND,
                        Some(WPARAM(40)),
                        None,
                    );
                }
                Ok(json!(pic.0 as usize))
            })?
            .as_u64()
            .unwrap() as usize;
        let (release, gate) = mpsc::channel();
        self.step(move |_| {
            let image = image_workspace::review();
            let id = image["documents"][0]["id"].as_u64().unwrap();
            let (_, revision) = capture_tools::workspace_snapshot(HWND(a as *mut _))?;
            workspace::start_job(
                workspace::Target::Image { id, revision },
                "测试识别 A",
                move |_, _| {
                    gate.recv_timeout(Duration::from_secs(30))
                        .map_err(|e| e.to_string())?;
                    Ok(Output::Text("A 的专属识别结果".into()))
                },
            );
            Ok(json!(revision))
        })?;
        self.step(|h| {
            let library = unsafe { state_mut(h) }.unwrap().library_root.clone();
            capture_tools::show_image(
                capture_tools::Raster {
                    width: 400,
                    height: 260,
                    pixels: [180, 225, 245, 255].repeat(400 * 260),
                },
                &library,
                false,
            )?;
            Ok(Value::Null)
        })?;
        for tab in [1, 2, 3, 0, 2, 1, 0] {
            self.step(move |_| {
                workspace::switch(tab);
                Ok(Value::Null)
            })?;
            thread::sleep(Duration::from_millis(80));
        }
        release.send(()).map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let value = self.step(|_| Ok(image_workspace::review()))?;
            if value["documents"][0]["text"] == json!("A 的专属识别结果") {
                if value["selected"] != json!(1) || value["documents"][1]["text"] != json!("") {
                    return Err("result changed another image or selection".into());
                }
                break;
            }
            if Instant::now() > deadline {
                return Err("background recognition did not complete".into());
            }
            thread::sleep(Duration::from_millis(100));
        }
        let after = self.step(|h| {
            let state = unsafe { state_mut(h) }.unwrap();
            let e = state.editor.as_ref().ok_or("tab switch destroyed editor")?;
            let snap = e.review_snapshot();
            if snap.navigation_pending {
                return Err("tab switch triggered navigation".into());
            }
            Ok(json!({"generation":snap.generation,"tick":snap.project_tick,"dirty":snap.dirty}))
        })?;
        if initial != after {
            return Err(format!("editor changed across tabs: {initial} -> {after}"));
        }
        self.step(move |_| {
            let p = workspace::image_parent().unwrap();
            let combo = unsafe { GetDlgItem(Some(p), 20) }.map_err(|e| e.to_string())?;
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    combo,
                    CB_SETCURSEL,
                    Some(WPARAM(0)),
                    None,
                );
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    p,
                    WM_COMMAND,
                    Some(WPARAM(0x0014 | ((CBN_SELCHANGE as usize) << 16))),
                    None,
                );
            }
            let result = unsafe { GetDlgItem(Some(p), 25) }.map_err(|e| e.to_string())?;
            if ui::value(result) != "A 的专属识别结果" {
                return Err("selected result missing".into());
            }
            let before = capture_tools::workspace_snapshot(HWND(a as *mut _))?.0;
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    HWND(a as *mut _),
                    WM_COMMAND,
                    Some(WPARAM(7)),
                    None,
                );
            }
            let after = capture_tools::workspace_snapshot(HWND(a as *mut _))?.0;
            if before.width == after.width {
                return Err("image undo state lost".into());
            }
            ui::text(result, "人工校对结果");
            Ok(Value::Null)
        })?;
        self.step(|h| {
            unsafe { state_mut(h) }.unwrap().shell_command(h, 10);
            if unsafe { state_mut(h) }
                .unwrap()
                .editor
                .as_ref()
                .unwrap()
                .review_snapshot()
                .navigation_pending
            {
                return Err("tray restore left document".into());
            }
            Ok(Value::Null)
        })?;
        for _ in 0..12 {
            for tab in [1, 2, 3, 0] {
                self.step(move |_| {
                    workspace::switch(tab);
                    Ok(Value::Null)
                })?;
            }
        }
        let shot = out.join("image-workspace.png");
        self.step(move |_| {
            capture_tools::review_window_png(workspace::root().unwrap(), &shot);
            let p = workspace::image_parent().unwrap();
            let edit = unsafe { GetDlgItem(Some(p), 25) }.map_err(|e| e.to_string())?;
            if ui::value(edit) != "人工校对结果" {
                return Err("editable result lost".into());
            }
            Ok(Value::Null)
        })?;
        let plugin_checked = if let Some(package) = std::env::var_os("PANZO_PLUGIN_REVIEW_PACKAGE")
        {
            self.plugin_flow(PathBuf::from(package), out)?;
            true
        } else {
            false
        };
        // Three persistent workspaces and two intentional, owned management windows.
        self.step(|_| {
            let p = workspace::image_parent().unwrap();
            if std::env::var_os("PANZO_PLUGIN_REVIEW_PACKAGE").is_some()
                && let Ok(close) = unsafe { GetDlgItem(Some(workspace::review_form(0)), 50) }
            {
                unsafe {
                    windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                        workspace::review_form(0),
                        WM_COMMAND,
                        Some(WPARAM(50)),
                        Some(LPARAM(close.0 as isize)),
                    );
                }
            }
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    p,
                    WM_COMMAND,
                    Some(WPARAM(15)),
                    None,
                );
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    p,
                    WM_COMMAND,
                    Some(WPARAM(12)),
                    None,
                );
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    p,
                    WM_COMMAND,
                    Some(WPARAM(12)),
                    None,
                );
            }
            Ok(Value::Null)
        })?;
        for (name, mode) in [
            ("dark", crate::platform::ui_preferences::ThemeMode::Dark),
            ("light", crate::platform::ui_preferences::ThemeMode::Light),
        ] {
            self.step(move |_| {
                crate::platform::ui_preferences::set_theme(mode)?;
                Ok(Value::Null)
            })?;
            thread::sleep(Duration::from_millis(180));
            for tab in [0, 1, 2, 3, 4] {
                self.step(move |_| {
                    workspace::switch(tab);
                    Ok(Value::Null)
                })?;
                thread::sleep(Duration::from_millis(80));
                let shot = out.join(format!(
                    "desktop-{name}-{}.png",
                    ["image", "video", "library", "settings", "plugins"][tab]
                ));
                self.step(move |_| {
                    let window = if tab >= 3 {
                        crate::platform::management_window::get(tab)
                            .ok_or("management window missing")?
                    } else {
                        workspace::root().unwrap()
                    };
                    capture_tools::review_chrome_png(window, &shot);
                    if tab >= 3 {
                        ui::show(window, false);
                    }
                    Ok(Value::Null)
                })?;
            }
        }
        self.step(|_| {
            let root = workspace::root().unwrap();
            unsafe {
                let _ = ShowWindow(root, SW_MAXIMIZE);
            }
            if !unsafe { IsZoomed(root) }.as_bool() {
                return Err("maximize failed".into());
            }
            let mut rect = RECT::default();
            unsafe {
                GetClientRect(root, &raw mut rect).map_err(|e| e.to_string())?;
            }
            if rect.right < 960 || rect.bottom < 700 {
                return Err("maximized client dimensions invalid".into());
            }
            unsafe {
                let _ = ShowWindow(root, SW_RESTORE);
            }
            Ok(Value::Null)
        })?;
        let captions = out.to_path_buf();
        self.step(move |_| {
            let root = workspace::root().unwrap();
            if workspace::custom_caption(root) {
                crate::platform::preview_window::editor_titlebar::review_activation_colors(
                    root, &captions, "caption",
                );
                let bounds = crate::platform::preview_window::editor_titlebar::caption_bounds(root)
                    .ok_or("caption buttons missing")?;
                for (index, expected) in [HTMINBUTTON, HTMAXBUTTON, HTCLOSE].into_iter().enumerate()
                {
                    let mut point = windows::Win32::Foundation::POINT {
                        x: bounds.left
                            + (bounds.right - bounds.left)
                                * (i32::try_from(index).unwrap() * 2 + 1)
                                / 6,
                        y: i32::midpoint(bounds.top, bounds.bottom),
                    };
                    unsafe {
                        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(root, &raw mut point);
                    }
                    let coordinates =
                        LPARAM((point.y as isize) << 16 | (point.x as isize & 0xffff));
                    if unsafe { SendMessageW(root, WM_NCHITTEST, None, Some(coordinates)) }.0
                        != isize::try_from(expected).unwrap()
                    {
                        return Err("caption button lost native hit testing".into());
                    }
                }
                let mut drag = windows::Win32::Foundation::POINT { x: 400, y: 16 };
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::ClientToScreen(root, &raw mut drag);
                }
                let coordinates = LPARAM((drag.y as isize) << 16 | (drag.x as isize & 0xffff));
                if unsafe { SendMessageW(root, WM_NCHITTEST, None, Some(coordinates)) }.0
                    != isize::try_from(HTCAPTION).unwrap()
                {
                    return Err("caption drag area missing".into());
                }
            }
            Ok(Value::Null)
        })?;
        // Close only the disposable test fixture after all persistence checks.
        self.step(|h| {
            let state = unsafe { state_mut(h) }.unwrap();
            if let Some(editor) = &mut state.editor {
                editor.detach(h);
            }
            state.editor = None;
            state.accessibility.detach();
            state.home = false;
            state.phase = RecorderPhase::Ready;
            state.status = "选择录制来源后开始录制".into();
            state.refresh(h);
            workspace::switch(1);
            crate::platform::ui_preferences::set_theme(
                crate::platform::ui_preferences::ThemeMode::Dark,
            )?;
            Ok(Value::Null)
        })?;
        thread::sleep(Duration::from_millis(180));
        let shot = out.join("desktop-dark-recording.png");
        self.step(move |h| {
            let state = unsafe { state_mut(h) }.unwrap();
            let layout = ControlLayout::for_state(h, state);
            for id in [
                RecorderControlId::MonitorSource,
                RecorderControlId::WindowSource,
                RecorderControlId::RegionSource,
                RecorderControlId::CameraEnabled,
                RecorderControlId::Record,
            ] {
                let rect = layout.control_rect(id).0;
                if layout.hit_control(
                    i32::midpoint(rect.left, rect.right),
                    i32::midpoint(rect.top, rect.bottom),
                ) != Some(id)
                {
                    return Err(format!("recorder toolbar overlap: {id:?}"));
                }
            }
            capture_tools::review_chrome_png(workspace::root().unwrap(), &shot);
            unsafe {
                SetWindowPos(
                    workspace::root().unwrap(),
                    None,
                    40,
                    40,
                    960,
                    720,
                    SWP_NOZORDER,
                )
                .map_err(|e| e.to_string())?;
            }
            workspace::switch(0);
            Ok(Value::Null)
        })?;
        thread::sleep(Duration::from_millis(150));
        self.step(|_| {
            let panel = workspace::image_parent().unwrap();
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    panel,
                    WM_COMMAND,
                    Some(WPARAM(16)),
                    None,
                );
            }
            Ok(Value::Null)
        })?;
        let shot = out.join("desktop-compact-image.png");
        self.step(move |_| {
            capture_tools::review_chrome_png(workspace::root().unwrap(), &shot);
            Ok(Value::Null)
        })?;
        self.step(|h| {
            let state = unsafe { state_mut(h) }.unwrap();
            crate::platform::media_settings::record_async(
                h,
                state.recording_options.clone(),
                false,
            )?;
            Ok(Value::Null)
        })?;
        thread::sleep(Duration::from_millis(150));
        let shot = out.join("desktop-compact-record-settings.png");
        self.step(move |h| {
            use windows::Win32::UI::WindowsAndMessaging::{
                GetWindowRect, SB_BOTTOM, SendMessageW, WM_VSCROLL,
            };
            let state = unsafe { state_mut(h) }.unwrap();
            let layout = ControlLayout::for_state(h, state);
            for id in [
                RecorderControlId::MonitorSource,
                RecorderControlId::WindowSource,
                RecorderControlId::RegionSource,
                RecorderControlId::OpenProject,
                RecorderControlId::Diagnostics,
            ] {
                let rect = layout.control_rect(id).0;
                if layout.hit_control(
                    i32::midpoint(rect.left, rect.right),
                    i32::midpoint(rect.top, rect.bottom),
                ) != Some(id)
                {
                    return Err(format!("compact recorder controls overlap: {id:?}"));
                }
            }
            let form = workspace::review_form(1);
            unsafe {
                SendMessageW(form, WM_VSCROLL, Some(WPARAM(SB_BOTTOM.0 as usize)), None);
            }
            let button = unsafe { GetDlgItem(Some(form), 1) }.map_err(|e| e.to_string())?;
            let mut parent_rect = RECT::default();
            let mut button_rect = RECT::default();
            unsafe {
                GetWindowRect(form, &raw mut parent_rect).map_err(|e| e.to_string())?;
                GetWindowRect(button, &raw mut button_rect).map_err(|e| e.to_string())?;
            }
            if button_rect.top < parent_rect.top || button_rect.bottom > parent_rect.bottom {
                return Err("apply button not reachable by scrolling".into());
            }
            unsafe {
                SendMessageW(form, WM_COMMAND, Some(WPARAM(1)), None);
            }
            let feedback = unsafe { GetDlgItem(Some(form), 12) }.map_err(|e| e.to_string())?;
            if !ui::value(feedback).contains("已应用") {
                return Err("record settings did not apply".into());
            }
            capture_tools::review_chrome_png(workspace::root().unwrap(), &shot);
            Ok(Value::Null)
        })?;
        let top = self.step(|_| {
            let mut classes = Vec::<String>::new();
            unsafe {
                EnumWindows(
                    Some(collect),
                    LPARAM(std::ptr::from_mut(&mut classes) as isize),
                )
                .map_err(|e| e.to_string())?;
            }
            if classes.iter().any(|s| {
                [
                    "PanzoPicture",
                    "PanzoProperties",
                    "PanzoImageTask",
                    "PanzoCaptureHistory",
                    "PanzoRecorderWindow",
                ]
                .contains(&s.as_str())
            }) {
                return Err(format!("unexpected top-level tool window: {classes:?}"));
            }
            if classes
                .iter()
                .filter(|c| c.as_str() == "PanzoManagementWindow")
                .count()
                != 2
            {
                return Err(format!(
                    "settings/plugins should each have exactly one owned window: {classes:?}"
                ));
            }
            Ok(json!(classes))
        })?;
        Ok(
            json!({"passed":true,"plugin_ui_checked":plugin_checked,"editor_before":initial,"editor_after":after,"top_level_classes":top,"checks":["production host with three persistent workspaces and two owned management windows","native caption button hit testing and active/inactive colors","recorder hit targets, 960x720 image layout and scrollable settings apply","image annotations and undo survive switches","A background result remains with A while B stays selected","editable results survive repeated tab switches","dirty video generation/playhead retained without navigation","tray restore preserves editor","settings/plugins have unique user-opened windows; image/history/results remain in workspace"]}),
        )
    }
}
unsafe extern "system" fn collect(h: HWND, lp: LPARAM) -> windows::core::BOOL {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(h, Some(&raw mut pid));
    }
    if pid == std::process::id() {
        let mut c = [0u16; 128];
        let n = unsafe { GetClassNameW(h, &mut c) };
        unsafe { &mut *(lp.0 as *mut Vec<String>) }.push(String::from_utf16_lossy(
            &c[..usize::try_from(n).unwrap_or(0)],
        ));
    }
    windows::core::BOOL(1)
}
fn copy_fixture(source: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_fixture(&entry.path(), &dest.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), dest.join(entry.file_name())).unwrap();
        }
    }
}
#[test]
#[ignore = "requires desktop and PANZO_UI_REVIEW_PROJECT/PANZO_UI_REVIEW_DIR; isolated fixtures only"]
fn persistent_workspaces_keep_documents_and_job_ownership() {
    let source =
        PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").expect("review fixture"));
    let out = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").expect("review output"));
    let library = out.join(format!("workspace-review-{}", uuid::Uuid::new_v4()));
    let project = library.join("fixture.panzo");
    copy_fixture(&source, &project);
    let (tx, rx) = mpsc::channel();
    let driver_slot = Arc::new(Mutex::new(None));
    let slot = driver_slot.clone();
    let output = out.clone();
    workspace::enable_review();
    let result = RecorderWindow::run_internal_with_project(
        &library,
        Automation::None,
        None,
        Some(Box::new(move |hwnd| {
            MAILBOX.with(|m| *m.borrow_mut() = Some(rx));
            *slot.lock().unwrap() = Some(thread::spawn(move || {
                let d = Driver { hwnd, tx };
                let result = d.run(&output);
                fs::write(
                    output.join("workspace-review.json"),
                    serde_json::to_vec_pretty(&match &result {
                        Ok(v) => v.clone(),
                        Err(e) => json!({"passed":false,"error":e}),
                    })
                    .unwrap(),
                )
                .unwrap();
                let _ = d.step(|_| {
                    unsafe {
                        let _ = DestroyWindow(workspace::root().unwrap());
                    }
                    Ok(Value::Null)
                });
                result
            }));
        })),
        Some(project),
    );
    assert!(result.is_ok(), "host: {result:?}");
    let result = driver_slot.lock().unwrap().take().unwrap().join().unwrap();
    assert!(result.is_ok(), "review: {result:?}");
}

#[test]
#[ignore = "requires isolated PANZO_UI_REVIEW_DIR and desktop"]
fn empty_video_workspace_actions() {
    let out=PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
    assert!(PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).starts_with(&out));
    let library=out.join("empty-library");fs::create_dir_all(&library).unwrap();
    workspace::enable_review();
    crate::platform::ui_preferences::set_theme(crate::platform::ui_preferences::ThemeMode::Dark).unwrap();
    let report=RecorderWindow::run_internal(&library,Automation::None,None,Some(Box::new(move|raw| {
        use windows::Win32::UI::WindowsAndMessaging::*;
        let h=HWND(raw as *mut _);let root=workspace::window(h);workspace::switch(1);
        let end=Instant::now()+Duration::from_millis(120);
        while Instant::now()<end {
            let mut msg=MSG::default();
            unsafe { while PeekMessageW(&mut msg,None,0,0,PM_REMOVE).as_bool() {let _=TranslateMessage(&msg);DispatchMessageW(&msg);} }
            std::thread::sleep(Duration::from_millis(5));
        }
        let s=unsafe{state_mut(h)}.unwrap();assert!(s.home);assert!(s.editor.is_none());
        let c=ControlLayout::for_state(h,s);let r=client_rect(h);
        assert_eq!(c.source.0,RECT::default());assert_eq!(c.status,RECT::default());
        assert_eq!(c.control_rect(RecorderControlId::Record).0,RECT::default());
        assert_eq!(c.preview.0.left+c.preview.0.right,r.right);
        assert_eq!(c.record.0.left+c.record.0.right,r.right);
        assert!(!s.control_enabled(RecorderControlId::Preview));
        s.last_project=Some(out.join("recent-fixture.panzo"));
        assert!(s.control_enabled(RecorderControlId::Preview));
        s.last_project=None;
        unsafe { let _=windows::Win32::Graphics::Gdi::RedrawWindow(Some(root),None,None,windows::Win32::Graphics::Gdi::RDW_INVALIDATE|windows::Win32::Graphics::Gdi::RDW_ALLCHILDREN|windows::Win32::Graphics::Gdi::RDW_UPDATENOW); }
        capture_tools::review_window_png(root,&out.join("empty-workspace.png"));
        s.activate_control(h,RecorderControlId::NewRecording);
        assert!(crate::platform::recording_setup::is_open());assert!(!s.phase.is_active());assert!(s.home);
        let panel=crate::platform::recording_setup::review_handle();
        assert_eq!(ui::value(unsafe{GetDlgItem(Some(panel),10)}.unwrap()),"开始录制");
        unsafe { SendMessageW(h,crate::platform::recording_setup::MESSAGE,Some(WPARAM(3)),Some(LPARAM(60))); }
        assert!(matches!(unsafe{state_mut(h)}.unwrap().capture_source,RecordingSource::PrimaryMonitor));
        let end=Instant::now()+Duration::from_millis(100);
        while Instant::now()<end {
            let mut msg=MSG::default();
            unsafe { while PeekMessageW(&mut msg,None,0,0,PM_REMOVE).as_bool() {let _=TranslateMessage(&msg);DispatchMessageW(&msg);} }
            std::thread::sleep(Duration::from_millis(5));
        }
        capture_tools::review_window_png(root,&out.join("new-recording-setup.png"));
        crate::platform::recording_setup::close();
        let picture=out.join("resize-fixture.png");
        image::RgbaImage::from_pixel(960,540,image::Rgba([124,151,169,255])).save(&picture).unwrap();
        image_workspace::open_source(&picture,&out).unwrap();
        let end=Instant::now()+Duration::from_millis(200);
        while Instant::now()<end {
            let mut msg=MSG::default();
            unsafe { while PeekMessageW(&mut msg,None,0,0,PM_REMOVE).as_bool() {let _=TranslateMessage(&msg);DispatchMessageW(&msg);} }
            std::thread::sleep(Duration::from_millis(5));
        }
        image_workspace::review_panel_resize();
        capture_tools::review_window_png(root,&out.join("image-panel-resized.png"));
        let s=unsafe{state_mut(h)}.unwrap();assert!(s.home);assert!(!s.phase.is_active());
        s.open_last_close_at=Some(Instant::now()+Duration::from_millis(300));
        fs::write(out.join("review.txt"),"PASS: centered recent/new recording actions; no source card/start/footer on empty workspace; recent availability follows recent project; new recording opens source setup without recording; screen selection; cancel returns to empty state.\n").unwrap();
    }))).unwrap();
    assert!(report.graceful_close);
}
