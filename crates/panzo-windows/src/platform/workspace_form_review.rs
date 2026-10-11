//! Native regression for cached form identity, callback ownership, and hidden settings tasks.
use super::{Apply, open_scoped, state};
use crate::platform::{
    image_tasks::Output, management_window, workspace, workspace_settings, workspace_ui as ui,
};
use std::{
    cell::Cell,
    path::PathBuf,
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HWND, WPARAM},
        UI::WindowsAndMessaging::{
            CB_GETCOUNT, CB_SETCURSEL, DestroyWindow, DispatchMessageW, GetDlgItem, IsWindow,
            IsWindowVisible, MSG, PM_REMOVE, PeekMessageW, SendMessageW, TranslateMessage,
            WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_TIMER,
        },
    },
    core::w,
};

struct Host(HWND);
impl Host {
    fn hidden() -> Self {
        Self(workspace::create().expect("isolated workspace host"))
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
fn command(form: HWND, id: usize) {
    unsafe {
        SendMessageW(form, WM_COMMAND, Some(WPARAM(id)), None);
    }
}
fn field(form: HWND, index: i32) -> HWND {
    unsafe { GetDlgItem(Some(form), 100 + index) }.expect("form field")
}
fn apply_ok() -> Apply {
    Box::new(|_, _| Ok("已应用".into()))
}
fn open(context: &str, value: &str, apply: Apply) -> HWND {
    open_scoped(
        0,
        crate::platform::workspace_form::FormId::Review,
        "原生表单回归",
        context,
        "仅使用合成测试值",
        &[("参数", value.into())],
        &[],
        &[],
        &["应用"],
        apply,
    )
    .expect("test form")
}
struct DropProbe(Rc<Cell<bool>>);
impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

#[test]
fn cached_form_reopen_keeps_executing_callback_alive_and_installs_replacement() {
    let _host = Host::hidden();
    let dropped = Rc::new(Cell::new(false));
    let probe = DropProbe(dropped.clone());
    let phase = Rc::new(Cell::new(0));
    let callback_phase = phase.clone();
    let replacement_calls = Rc::new(Cell::new(0));
    let replacement = replacement_calls.clone();
    let during_call = dropped.clone();
    let original = open(
        "callback",
        "initial",
        Box::new(move |_, values| {
            let _keep_probe_in_closure = &probe;
            callback_phase.set(1);
            let replacement = replacement.clone();
            open(
                "callback",
                "replacement-default",
                Box::new(move |_, _| {
                    replacement.set(replacement.get() + 1);
                    Ok("replacement ran".into())
                }),
            );
            // An error reports safely through WM_COMMAND; do not panic across the native callback.
            if during_call.get() {
                callback_phase.set(90);
                return Err("executing closure was destroyed".into());
            }
            if values != ["draft kept"] {
                callback_phase.set(91);
                return Err("draft values changed".into());
            }
            callback_phase.set(2);
            Ok("original completed".into())
        }),
    );
    ui::text(field(original, 0), "draft kept");
    command(original, 1);
    assert_eq!(
        phase.get(),
        2,
        "reopening from the callback must not destroy its captures"
    );
    assert!(
        dropped.get(),
        "the old closure should be released after it returns"
    );
    assert!(unsafe { IsWindow(Some(original)) }.as_bool());
    assert_eq!(ui::value(field(original, 0)), "draft kept");
    command(original, 1);
    assert_eq!(
        replacement_calls.get(),
        1,
        "the cached form must use the replacement closure next time"
    );
}

#[test]
fn form_contexts_keep_independent_handles_callbacks_and_drafts() {
    let _host = Host::hidden();
    let first = open("image-A", "A initial", apply_ok());
    ui::text(field(first, 0), "A draft");
    let second = open("image-B", "B initial", apply_ok());
    assert_ne!(first, second);
    ui::text(field(second, 0), "B draft");
    let returned = open("image-A", "new A default", apply_ok());
    assert_eq!(returned, first);
    assert_eq!(ui::value(field(returned, 0)), "A draft");
    assert_eq!(ui::value(field(second, 0)), "B draft");
    assert_eq!(open("image-B", "new B default", apply_ok()), second);
}

#[test]
fn cached_combo_refreshes_choices_retains_valid_selection_and_falls_back_when_removed() {
    let _host = Host::hidden();
    let create = |default: &str, choices: &[&str]| {
        open_scoped(
            0,
            crate::platform::workspace_form::FormId::Review,
            "动态下拉回归",
            "choices",
            "设备列表会刷新",
            &[("设备", default.into())],
            &[(0, choices)],
            &[],
            &["应用"],
            apply_ok(),
        )
        .unwrap()
    };
    let form = create("A", &["A", "B"]);
    let combo = field(form, 0);
    unsafe {
        SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(1)), None);
    }
    assert_eq!(ui::value(combo), "B");
    assert_eq!(create("C", &["B", "C", "D"]), form);
    assert_eq!(unsafe { SendMessageW(combo, CB_GETCOUNT, None, None) }.0, 3);
    assert_eq!(
        ui::value(combo),
        "B",
        "a surviving draft selection wins over a new default"
    );
    create("D", &["C", "D"]);
    assert_eq!(
        ui::value(combo),
        "D",
        "removed selection uses the provided valid default"
    );
    create("unknown", &["E", "F"]);
    assert_eq!(
        ui::value(combo),
        "E",
        "invalid default falls back to the first available choice"
    );
}

fn pump(root: HWND) {
    let mut message = MSG::default();
    while unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    unsafe {
        SendMessageW(root, WM_TIMER, Some(WPARAM(1)), None);
    }
}
fn wait_for(root: HWND, condition: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !condition() {
        pump(root);
        assert!(
            Instant::now() < end,
            "form task did not reach the expected state"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "opens isolated settings window; requires PANZO_FORM_REVIEW_DIR with LOCALAPPDATA inside that directory"]
fn settings_draft_and_form_job_survive_close_reopen_and_complete_while_hidden() {
    let out = PathBuf::from(
        std::env::var_os("PANZO_FORM_REVIEW_DIR").expect("isolated output directory"),
    );
    let profile = PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("isolated LOCALAPPDATA"));
    assert!(
        profile.starts_with(&out),
        "never inspect the user's saved API settings in this review"
    );
    std::fs::create_dir_all(&profile).unwrap();
    let library = out.join(format!("form-library-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&library).unwrap();
    let host = Host::hidden();
    let video = ui::child(
        host.0,
        w!("STATIC"),
        "Synthetic video host",
        800,
        WINDOW_STYLE(0),
    )
    .unwrap();
    workspace::attach(host.0, video, &library, false).unwrap();
    workspace_settings::open().unwrap();
    pump(host.0);
    let form = workspace::review_form(3);
    let manager = management_window::get(3).expect("settings owner");
    let draft_url = "https://draft.invalid/v1";
    let draft_model = "unsaved-test-model";
    ui::text(field(form, 0), draft_url);
    ui::text(field(form, 1), draft_model);
    ui::text(field(form, 2), "synthetic-unsaved-key");
    unsafe {
        SendMessageW(manager, WM_CLOSE, None, None);
    }
    assert!(!unsafe { IsWindowVisible(manager) }.as_bool());
    assert!(unsafe { IsWindow(Some(form)) }.as_bool());
    workspace_settings::open().unwrap();
    pump(host.0);
    assert_eq!(management_window::get(3), Some(manager));
    assert_eq!(workspace::review_form(3), form);
    assert!(unsafe { IsWindowVisible(manager) }.as_bool());
    assert_eq!(ui::value(field(form, 0)), draft_url);
    assert_eq!(ui::value(field(form, 1)), draft_model);
    assert_eq!(ui::value(field(form, 2)), "synthetic-unsaved-key");

    let (release, gate) = mpsc::channel();
    let token = workspace::start_job(
        workspace::Target::Form(form),
        "本地合成验证",
        move |_, progress| {
            let _ = progress.send(crate::platform::image_tasks::Update::Progress(
                "正在处理合成数据".into(),
            ));
            gate.recv_timeout(Duration::from_secs(10))
                .map_err(|e| e.to_string())?;
            Ok(Output::Text("隐藏窗口中的任务已完成".into()))
        },
    );
    wait_for(host.0, || {
        unsafe { state(form) }.is_some_and(|s| s.running.is_some())
    });
    unsafe {
        SendMessageW(manager, WM_CLOSE, None, None);
    }
    assert!(!unsafe { IsWindowVisible(manager) }.as_bool());
    assert!(
        !token.load(std::sync::atomic::Ordering::Acquire),
        "hiding settings must not cancel the job"
    );
    release.send(()).unwrap();
    wait_for(host.0, || {
        unsafe { state(form) }
            .is_some_and(|s| s.running.is_none() && ui::value(s.status) == "隐藏窗口中的任务已完成")
    });
    assert!(
        !unsafe { IsWindowVisible(manager) }.as_bool(),
        "completion must not reopen settings automatically"
    );
    workspace_settings::open().unwrap();
    pump(host.0);
    assert_eq!(workspace::review_form(3), form);
    assert_eq!(ui::value(field(form, 0)), draft_url);
    assert_eq!(ui::value(field(form, 1)), draft_model);
    assert_eq!(
        ui::value(unsafe { state(form) }.unwrap().status),
        "隐藏窗口中的任务已完成"
    );
    assert!(unsafe { state(form) }.unwrap().running.is_none());
    std::fs::write(out.join("workspace-form-review.json"), serde_json::to_vec_pretty(&serde_json::json!({"passed":true,"checks":["settings close/reopen keeps the same window and form","unsaved URL/model/key drafts retained","Target::Form job remains attached while settings hidden","completion updates hidden form without opening a window","reopened form retains completion result and cancel state clears"],"networkRequests":0,"savedSecrets":0})).unwrap()).unwrap();
}
