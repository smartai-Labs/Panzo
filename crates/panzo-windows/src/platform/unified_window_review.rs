//! Opt-in integration regression using the production host and its single message loop.
//! Only the command mailbox and modal-answer injection are test hooks. No recording starts.

use super::*;
use std::cell::RefCell;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread::{self, JoinHandle};
use windows::Win32::UI::Accessibility::{IInvokeProvider, UIA_E_ELEMENTNOTAVAILABLE};
use windows::Win32::UI::WindowsAndMessaging::{IsWindow, PostMessageW, SendMessageW, WM_APP};

const WM_REVIEW: u32 = WM_APP + 0x620;
const ROUND_TRIPS: usize = 20;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
const TRANSITION_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ReviewEditorSnapshot {
    pub(crate) generation: usize,
    pub(crate) project_tick: i64,
    pub(crate) dirty: bool,
    pub(crate) navigation_pending: bool,
    pub(crate) saving: bool,
    pub(crate) playing: bool,
    pub(crate) rendered_frames: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ReviewLeaveAnswer {
    Discard,
    Cancel,
}

pub(crate) struct ReviewPendingSave {
    pub(crate) complete: SyncSender<crate::platform::editor_tasks::ReviewSaveResult>,
    pub(crate) snapshot: crate::platform::editor::ProjectEditorSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    hwnd: usize,
    host: usize,
    thread: u32,
    project: Option<PathBuf>,
    editor: Option<ReviewEditorSnapshot>,
    transitioning: bool,
    recording_started: bool,
}

enum ReviewCommand {
    Snapshot,
    Navigate(NavigationIntent),
    DirtyNavigate(NavigationIntent, ReviewLeaveAnswer),
    RetainProvider,
    InvokeRetainedProvider,
    StaleCallbacks(usize),
    BlockedNavigationInputs,
    BeginSaveNavigation,
    EditDuringSave,
    CompleteSave { fail: bool },
    Escape,
}

struct Envelope {
    command: ReviewCommand,
    reply: SyncSender<Result<Snapshot, String>>,
}

thread_local! {
    // Commands own their payloads: a timed-out sender never leaves a dangling LPARAM.
    static MAILBOX: RefCell<Option<Receiver<Envelope>>> = const { RefCell::new(None) };
    static RETAINED_PROVIDER: RefCell<Option<IInvokeProvider>> = const { RefCell::new(None) };
    static PENDING_SAVE: RefCell<Option<ReviewPendingSave>> = const { RefCell::new(None) };
}

pub(super) fn handle_message(
    hwnd: HWND,
    message: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> Option<LRESULT> {
    if message != WM_REVIEW {
        return None;
    }
    let envelope = MAILBOX.with(|mailbox| {
        mailbox
            .borrow()
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
    });
    if let Some(envelope) = envelope {
        let result = execute(hwnd, envelope.command).and_then(|()| snapshot(hwnd));
        let _ = envelope.reply.send(result);
    }
    Some(LRESULT(0))
}

fn snapshot(hwnd: HWND) -> Result<Snapshot, String> {
    let state = unsafe { state_mut(hwnd) }.ok_or("host no longer exists")?;
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() || state.hwnd != Some(hwnd) {
        return Err("the original host HWND is no longer live".into());
    }
    Ok(Snapshot {
        hwnd: hwnd.0 as usize,
        host: std::ptr::from_ref(state) as usize,
        thread: unsafe { windows::Win32::System::Threading::GetCurrentThreadId() },
        project: state
            .editor
            .as_ref()
            .map(|editor| editor.project_root().to_path_buf()),
        editor: state.editor.as_ref().map(|editor| editor.review_snapshot()),
        transitioning: state.transition.is_some() || state.requested_project.is_some(),
        recording_started: state.telemetry.recording_started.load(Ordering::Relaxed),
    })
}

fn execute(hwnd: HWND, command: ReviewCommand) -> Result<(), String> {
    match command {
        ReviewCommand::Snapshot => (),
        ReviewCommand::Navigate(intent) => navigate(hwnd, intent)?,
        ReviewCommand::DirtyNavigate(intent, answer) => {
            let state = unsafe { state_mut(hwnd) }.ok_or("host disappeared")?;
            state
                .editor
                .as_mut()
                .ok_or("expected an editor")?
                .review_dirty_navigation(hwnd, intent, answer)?;
        }
        ReviewCommand::RetainProvider => {
            let state = unsafe { state_mut(hwnd) }.ok_or("host disappeared")?;
            let provider = if let Some(editor) = &state.editor {
                editor.review_invoke_provider()
            } else {
                state
                    .accessibility
                    .review_invoke_provider(RecorderControlId::Preview as u32 + 1)
            }
            .map_err(|error| error.to_string())?;
            RETAINED_PROVIDER.with(|retained| *retained.borrow_mut() = Some(provider));
        }
        ReviewCommand::InvokeRetainedProvider => {
            let before = snapshot(hwnd)?;
            let result = RETAINED_PROVIDER.with(|retained| {
                let retained = retained.borrow();
                let provider = retained.as_ref().ok_or("no retained UIA provider")?;
                match unsafe { provider.Invoke() } {
                    Err(error) if error.code().0.cast_unsigned() == UIA_E_ELEMENTNOTAVAILABLE => {
                        Ok(())
                    }
                    Err(error) => Err(format!(
                        "retained provider returned the wrong error: {error}"
                    )),
                    Ok(()) => Err("a detached page's UIA provider still accepts Invoke".into()),
                }
            });
            result?;
            ensure_same_snapshot(&before, &snapshot(hwnd)?, "retained UIA provider")?;
        }
        ReviewCommand::StaleCallbacks(generation) => {
            let before = snapshot(hwnd)?;
            // Synchronous dispatch on the UI thread prevents an unrelated timer/render
            // completion from contaminating the exact before/after assertion. No host
            // state borrow spans these calls, and no second message loop is introduced.
            for message in [WM_APP + 1, WM_APP + 2, WM_APP + 3] {
                unsafe {
                    let _ = SendMessageW(hwnd, message, Some(WPARAM(generation)), Some(LPARAM(0)));
                }
            }
            ensure_same_snapshot(&before, &snapshot(hwnd)?, "stale render callbacks")?;
        }
        command => execute_save_review(hwnd, &command)?,
    }
    Ok(())
}

fn execute_save_review(hwnd: HWND, command: &ReviewCommand) -> Result<(), String> {
    if matches!(command, ReviewCommand::Escape) {
        unsafe {
            let _ = SendMessageW(
                hwnd,
                WM_KEYDOWN,
                Some(WPARAM(usize::from(VK_ESCAPE.0))),
                Some(LPARAM(0)),
            );
        }
        return Ok(());
    }
    let state = unsafe { state_mut(hwnd) }.ok_or("host disappeared")?;
    let editor = state
        .editor
        .as_mut()
        .ok_or("save review requires an editor")?;
    match command {
        ReviewCommand::BlockedNavigationInputs => editor.review_blocked_navigation_inputs(hwnd)?,
        ReviewCommand::BeginSaveNavigation => {
            if PENDING_SAVE.with(|slot| slot.borrow().is_some()) {
                return Err("a controlled save is already pending".into());
            }
            let pending = editor.review_begin_save_navigation(hwnd)?;
            PENDING_SAVE.with(|slot| *slot.borrow_mut() = Some(pending));
        }
        ReviewCommand::EditDuringSave => editor.review_edit_during_save()?,
        ReviewCommand::CompleteSave { fail } => {
            let mut pending = PENDING_SAVE
                .with(|slot| slot.borrow_mut().take())
                .ok_or("no controlled save result to release")?;
            let result = if *fail {
                Err("review injected save failure".into())
            } else {
                pending
                    .snapshot
                    .save(panzo_core::CameraEditKind::Update, None)
                    .map(|report| (pending.snapshot, report))
                    .map_err(|error| error.to_string())
            };
            pending
                .complete
                .send(result)
                .map_err(|error| error.to_string())?;
            editor.review_poll_save(hwnd);
        }
        _ => return Err("not a save review command".into()),
    }
    Ok(())
}

fn navigate(hwnd: HWND, intent: NavigationIntent) -> Result<(), String> {
    let state = unsafe { state_mut(hwnd) }.ok_or("host disappeared")?;
    if let Some(editor) = &mut state.editor {
        editor.review_request_navigation(hwnd, intent);
    } else {
        match intent {
            NavigationIntent::OpenProject(project) => {
                state.requested_project = Some(project);
                state.post_navigation();
            }
            NavigationIntent::Quit => state.on_close(hwnd),
            NavigationIntent::NewRecording => return Err("already in recording preparation".into()),
        }
    }
    Ok(())
}

fn ensure_same_snapshot(before: &Snapshot, after: &Snapshot, reason: &str) -> Result<(), String> {
    if before == after {
        Ok(())
    } else {
        Err(format!(
            "{reason} changed session state: before={before:?}, after={after:?}"
        ))
    }
}

struct Driver {
    hwnd: usize,
    commands: SyncSender<Envelope>,
    initial: Option<Snapshot>,
    checks: Vec<String>,
    completed_round_trips: usize,
}

impl Driver {
    fn command(&self, command: ReviewCommand) -> Result<Snapshot, String> {
        let (reply, response) = sync_channel(1);
        self.commands
            .send(Envelope { command, reply })
            .map_err(|error| error.to_string())?;
        unsafe {
            PostMessageW(
                Some(HWND(self.hwnd as *mut _)),
                WM_REVIEW,
                WPARAM(0),
                LPARAM(0),
            )
        }
        .map_err(|error| error.to_string())?;
        let snapshot = response.recv_timeout(COMMAND_TIMEOUT).map_err(|error| {
            format!("host command did not complete within {COMMAND_TIMEOUT:?}: {error}")
        })??;
        if let Some(initial) = &self.initial {
            if (snapshot.hwnd, snapshot.host, snapshot.thread)
                != (initial.hwnd, initial.host, initial.thread)
            {
                return Err(format!("navigation replaced the host: {snapshot:?}"));
            }
            if snapshot.recording_started {
                return Err("navigation unexpectedly started recording".into());
            }
        }
        Ok(snapshot)
    }

    fn wait_for(
        &self,
        description: &str,
        predicate: impl Fn(&Snapshot) -> bool,
    ) -> Result<Snapshot, String> {
        let deadline = Instant::now() + TRANSITION_TIMEOUT;
        loop {
            let current = self.command(ReviewCommand::Snapshot)?;
            if predicate(&current) {
                return Ok(current);
            }
            if Instant::now() >= deadline {
                return Err(format!("timeout waiting for {description}: {current:?}"));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn open(&self, project: &Path) -> Result<Snapshot, String> {
        self.command(ReviewCommand::Navigate(NavigationIntent::OpenProject(
            project.into(),
        )))?;
        self.wait_for("project ready", |current| {
            current.project.as_deref() == Some(project)
                && !current.transitioning
                && current
                    .editor
                    .as_ref()
                    .is_some_and(|editor| !editor.navigation_pending)
        })
    }

    fn recorder(&self) -> Result<Snapshot, String> {
        self.wait_for("recording preparation on the same host", |current| {
            current.editor.is_none() && !current.transitioning
        })
    }

    fn check_invalid_project(&mut self, project: &Path, active: &Snapshot) -> Result<(), String> {
        self.command(ReviewCommand::Navigate(NavigationIntent::OpenProject(
            project.with_file_name("intentionally-missing.panzo"),
        )))?;
        let after = self.wait_for("failed load leaves the old session active", |current| {
            !current.transitioning
                && current
                    .editor
                    .as_ref()
                    .is_some_and(|editor| !editor.navigation_pending)
        })?;
        if after.project != active.project
            || after.editor.as_ref().map(|value| value.generation)
                != active.editor.as_ref().map(|value| value.generation)
        {
            return Err(format!("failed load discarded the old session: {after:?}"));
        }
        self.checks
            .push("invalid project retained the same session generation and project".into());
        Ok(())
    }

    fn check_dirty_cancel(&mut self, active: &Snapshot) -> Result<(), String> {
        self.command(ReviewCommand::DirtyNavigate(
            NavigationIntent::NewRecording,
            ReviewLeaveAnswer::Cancel,
        ))?;
        let after = self.command(ReviewCommand::Snapshot)?;
        let editor = after.editor.as_ref().ok_or("Cancel discarded the editor")?;
        if !editor.dirty
            || editor.navigation_pending
            || after.transitioning
            || after.project != active.project
            || Some(editor.generation) != active.editor.as_ref().map(|value| value.generation)
        {
            return Err(format!(
                "dirty Cancel did not retain the current unsaved session: {after:?}"
            ));
        }
        self.checks
            .push("dirty Cancel retained unsaved edits and cancelled navigation".into());
        Ok(())
    }

    fn check_save_navigation(&mut self, project: &Path) -> Result<(), String> {
        let active = self.open(project)?;
        let generation = active
            .editor
            .as_ref()
            .ok_or("missing save editor")?
            .generation;
        self.command(ReviewCommand::BlockedNavigationInputs)?;
        self.checks.push("authorized navigation blocked wheel and already queued UIA SetRange; both unlocked positive controls changed the model".into());

        let waiting = self.command(ReviewCommand::BeginSaveNavigation)?;
        Self::expect_save_state(&waiting, generation, (true, true, true), "pending save")?;
        let cancelled = self.command(ReviewCommand::Escape)?;
        Self::expect_save_state(
            &cancelled,
            generation,
            (true, false, true),
            "Esc during save",
        )?;
        self.command(ReviewCommand::CompleteSave { fail: false })?;
        let complete = self.command(ReviewCommand::Snapshot)?;
        Self::expect_save_state(
            &complete,
            generation,
            (false, false, false),
            "save completed after Esc",
        )?;
        self.checks.push("Esc cancelled pending navigation while preserving the save; real save completion retained the same session".into());

        let saved_files = fixture_edit_bytes(project);
        self.command(ReviewCommand::BeginSaveNavigation)?;
        self.command(ReviewCommand::CompleteSave { fail: true })?;
        let failed = self.command(ReviewCommand::Snapshot)?;
        Self::expect_save_state(&failed, generation, (true, false, false), "save failed")?;
        if fixture_edit_bytes(project) != saved_files {
            return Err("a failed save changed persisted project files".into());
        }
        self.checks.push("injected save failure retained dirty edits/session and cancelled navigation; persisted files unchanged".into());

        self.command(ReviewCommand::BeginSaveNavigation)?;
        self.command(ReviewCommand::EditDuringSave)?;
        self.command(ReviewCommand::CompleteSave { fail: false })?;
        let newer_edit = self.command(ReviewCommand::Snapshot)?;
        Self::expect_save_state(
            &newer_edit,
            generation,
            (true, false, false),
            "saved snapshot predates another edit",
        )?;
        self.checks.push(
            "a newer edit survived real old-snapshot save completion and prevented navigation"
                .into(),
        );

        self.command(ReviewCommand::BeginSaveNavigation)?;
        self.command(ReviewCommand::CompleteSave { fail: false })?;
        self.recorder()?;
        self.checks.push(
            "positive control: successful save with no later edit completed pending navigation"
                .into(),
        );
        Ok(())
    }

    fn expect_save_state(
        current: &Snapshot,
        generation: usize,
        expected: (bool, bool, bool),
        stage: &str,
    ) -> Result<(), String> {
        let editor = current
            .editor
            .as_ref()
            .ok_or_else(|| format!("{stage}: editor was discarded"))?;
        if current.transitioning
            || editor.generation != generation
            || (editor.dirty, editor.navigation_pending, editor.saving) != expected
        {
            return Err(format!(
                "{stage}: unexpected save/navigation state {current:?}; expected {expected:?}"
            ));
        }
        Ok(())
    }

    fn run(&mut self, project: &Path, save_project: &Path) -> Result<(), String> {
        self.initial = Some(self.command(ReviewCommand::Snapshot)?);
        self.command(ReviewCommand::RetainProvider)?;
        let mut old_generation = None;
        for round in 0..ROUND_TRIPS {
            let active = self.open(project)?;
            if round == 0 {
                self.command(ReviewCommand::InvokeRetainedProvider)?;
                self.check_invalid_project(project, &active)?;
                self.check_dirty_cancel(&active)?;
            }
            if let Some(previous) = old_generation {
                let current = active.editor.as_ref().ok_or("editor missing")?.generation;
                if current == previous {
                    return Err("a replacement editor reused the previous generation".into());
                }
                self.command(ReviewCommand::StaleCallbacks(previous))?;
                self.command(ReviewCommand::InvokeRetainedProvider)?;
            }
            self.command(ReviewCommand::RetainProvider)?;
            old_generation = active.editor.as_ref().map(|editor| editor.generation);
            if round == 0 {
                self.command(ReviewCommand::DirtyNavigate(
                    NavigationIntent::NewRecording,
                    ReviewLeaveAnswer::Discard,
                ))?;
            } else {
                self.command(ReviewCommand::Navigate(NavigationIntent::NewRecording))?;
            }
            self.recorder()?;
            self.command(ReviewCommand::InvokeRetainedProvider)?;
            self.command(ReviewCommand::StaleCallbacks(
                old_generation.ok_or("no prior generation")?,
            ))?;
            self.checks.push(format!(
                "round {}: same HWND/host/thread, open and return, stale callbacks/UIA rejected",
                round + 1
            ));
            self.completed_round_trips += 1;
        }
        self.check_save_navigation(save_project)?;
        self.command(ReviewCommand::Navigate(NavigationIntent::Quit))?;
        Ok(())
    }
}

fn copy_fixture(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let destination = target.join(entry.file_name());
        if kind.is_symlink() {
            return Err(std::io::Error::other(
                "review fixture must not contain symlinks",
            ));
        }
        if kind.is_dir() {
            copy_fixture(&entry.path(), &destination)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

fn fixture_edit_bytes(project: &Path) -> Vec<Vec<u8>> {
    ["project.json", "tracks/camera.json", "edit/workbench.json"]
        .into_iter()
        .map(|path| fs::read(project.join(path)).unwrap())
        .collect()
}

#[test]
#[ignore = "requires a desktop and PANZO_UI_REVIEW_PROJECT/PANZO_UI_REVIEW_DIR; no recording"]
fn real_host_survives_twenty_content_round_trips() {
    let source =
        PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").expect("review source project"));
    let output =
        PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").expect("review output directory"))
            .join(format!("unified-window-{}", uuid::Uuid::new_v4()));
    let library = output.join("library");
    let project = library.join("fixture.panzo");
    let save_project = library.join("save-fixture.panzo");
    copy_fixture(&source, &project).expect("copy isolated review fixture");
    copy_fixture(&source, &save_project).expect("copy isolated save review fixture");
    let original_edits = fixture_edit_bytes(&project);
    let (commands, mailbox) = sync_channel(1);
    let driver: Arc<Mutex<Option<JoinHandle<_>>>> = Arc::default();
    let driver_slot = Arc::clone(&driver);
    let driver_output = output.clone();
    let report = RecorderWindow::run_internal(
        &library,
        Automation::None,
        None,
        Some(Box::new(move |hwnd| {
            MAILBOX.with(|slot| *slot.borrow_mut() = Some(mailbox));
            let handle = thread::spawn(move || {
                let mut driver = Driver {
                    hwnd,
                    commands,
                    initial: None,
                    checks: Vec::new(),
                    completed_round_trips: 0,
                };
                let result = driver.run(&project, &save_project);
                // Even a regression must let the production loop return for a useful failure.
                // This message only exists to end a failed test; success uses normal Quit.
                if result.is_err() {
                    unsafe {
                        let _ = PostMessageW(
                            Some(HWND(hwnd as *mut _)),
                            WM_QUIT_WINDOW,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }
                let log = serde_json::json!({
                "passed": result.is_ok(), "required_round_trips": ROUND_TRIPS,
                "completed_round_trips": driver.completed_round_trips,
                    "host_hwnd": hwnd, "checks": driver.checks,
                    "error": result.as_ref().err(),
                    "scope": "real single host message loop; internal navigation/UIA hooks; no recording"
                });
                fs::write(
                    driver_output.join("navigation-review.json"),
                    serde_json::to_vec_pretty(&log).unwrap(),
                )
                .expect("write navigation review report");
                result
            });
            *driver_slot.lock().unwrap() = Some(handle);
        })),
    );
    MAILBOX.with(|slot| *slot.borrow_mut() = None);
    RETAINED_PROVIDER.with(|slot| *slot.borrow_mut() = None);
    PENDING_SAVE.with(|slot| *slot.borrow_mut() = None);
    if let Some(driver) = driver.lock().unwrap().take() {
        driver
            .join()
            .expect("review driver thread panicked")
            .expect("navigation regression");
    }
    let report = report.expect("production host failed");
    assert!(report.window_created && report.graceful_close, "{report:?}");
    assert!(
        !report.recording_started && report.recording.is_none(),
        "{report:?}"
    );
    assert_eq!(
        fixture_edit_bytes(&library.join("fixture.panzo")),
        original_edits,
        "discarding an unsaved edit must not change persisted project files"
    );
    eprintln!("single-window regression artifacts: {}", output.display());
}
