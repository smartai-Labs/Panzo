//! Unified inspector regression: disposable project and profile, no microphone/API requests.
use super::*;
use crate::platform::{
    capture_tools, media_panel,
    ui_preferences::{self, ThemeMode},
    workspace, workspace_ui as ui,
};
use std::path::PathBuf;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetDlgItem, GetParent, GetWindowRect, IsWindowVisible, MSG, PM_REMOVE,
    PeekMessageW, TranslateMessage, WM_COMMAND, WM_VSCROLL,
};
use windows::Win32::UI::WindowsAndMessaging::{SWP_NOMOVE, SendMessageW};
fn pump() {
    let until = Instant::now() + Duration::from_millis(180);
    while Instant::now() < until {
        let mut m = MSG::default();
        unsafe {
            while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn click(hwnd: HWND, id: ControlId) {
    let state = unsafe { state_mut(hwnd) }.unwrap();
    let r = state.control_layout(client_rect(hwnd)).control_rect(id);
    assert!(r.right > r.left);
    let x = (r.left + r.right) / 2;
    let y = (r.top + r.bottom) / 2;
    state.on_mouse_down(hwnd, x, y);
    state.on_mouse_up(hwnd, x, y);
    pump();
}
fn check(hwnd: HWND) -> ControlLayout {
    let s = unsafe { state_mut(hwnd) }.unwrap();
    let c = s.control_layout(client_rect(hwnd));
    let root = workspace::window(hwnd);
    assert_eq!(
        client_rect(root).right,
        client_rect(hwnd).right,
        "timeline host keeps full workspace width"
    );
    assert!(c.timeline_panel.right > c.panel.right - s.theme.scale(2));
    assert_eq!(c.panel.bottom, c.splitter.top);
    assert!(c.inspector_splitter.right > c.inspector_splitter.left);
    if let Some(panel) = s.inspector_panel {
        assert_eq!(unsafe { GetParent(panel) }.unwrap(), hwnd);
        let mut r = RECT::default();
        unsafe {
            GetWindowRect(panel, &mut r).unwrap();
        }
        let mut a = POINT {
            x: r.left,
            y: r.top,
        };
        let mut b = POINT {
            x: r.right,
            y: r.bottom,
        };
        unsafe {
            let _ = ScreenToClient(hwnd, &mut a);
            let _ = ScreenToClient(hwnd, &mut b);
        }
        assert_eq!(
            RECT {
                left: a.x,
                top: a.y,
                right: b.x,
                bottom: b.y
            },
            c.inspector_content()
        );
        assert_eq!(c.control_rect(ControlId::CropLeft).0, RECT::default());
        assert!(!s.control_enabled(ControlId::CropLeft));
    }
    assert_eq!(s.preview_surface.as_ref().unwrap().well, c.preview_well);
    c
}
fn check_actions(panel: HWND, subtitle: bool) {
    let body = media_panel::review_body(panel);
    let mut bounds = RECT::default();
    unsafe {
        GetWindowRect(panel, &mut bounds).unwrap();
    }
    for id in if subtitle { vec![31, 32, 50] } else { vec![31] } {
        let control = unsafe { GetDlgItem(Some(panel), id) }.unwrap();
        assert!(unsafe { IsWindowVisible(control) }.as_bool());
        assert_eq!(
            unsafe { GetParent(control) }.unwrap(),
            panel,
            "primary actions are outside the scroll viewport"
        );
        let mut rect = RECT::default();
        unsafe {
            GetWindowRect(control, &mut rect).unwrap();
        }
        assert!(
            rect.left >= bounds.left
                && rect.right <= bounds.right
                && rect.top >= bounds.top
                && rect.bottom <= bounds.bottom,
            "primary action {id} is clipped"
        );
    }
    assert!(unsafe { IsWindowVisible(body) }.as_bool());
}
#[test]
#[ignore = "requires isolated synthetic project/profile and native review directory"]
fn unified_inspector_native_review() {
    assert!(unsafe { windows::Win32::UI::Input::Ime::ImmDisableIME(0) }.as_bool());
    let project = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
    let out = PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
    assert!(PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).starts_with(&out));
    let package = crate::platform::plugins::read_package(&PathBuf::from(
        std::env::var_os("PANZO_MEDIA_REVIEW_PACKAGE").unwrap(),
    ))
    .unwrap();
    crate::platform::plugins::Store::open(
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("Panzo/Plugins"),
    )
    .unwrap()
    .install(&package)
    .unwrap();
    workspace::enable_review();
    let report=PreviewWindow::run_internal_notifying(&project,Some(Duration::from_secs(60)),SmokeAction::None,Some(Box::new(move |raw|{
        let h=HWND(raw as *mut c_void); let root=workspace::window(h);
        let s=unsafe{state_mut(h)}.unwrap();let original=s.editor.settings().clone();
        s.theme.metrics.inspector_width=s.theme.scale(360);s.theme.metrics.timeline_height=s.theme.scale(350);s.refresh(h,true);
        let mut media=None;
        for (theme,name) in [(ThemeMode::Dark,"dark"),(ThemeMode::Light,"light")] {
            ui_preferences::set_theme(theme).unwrap();pump();
            for (id,label) in [(ControlId::CanvasTab,"canvas"),(ControlId::VideoTab,"video"),(ControlId::AudioTab,"audio"),(ControlId::SubtitleTab,"subtitles")] {
                let before=check(h);
                click(h,id);
                let c=check(h);let s=unsafe{state_mut(h)}.unwrap();
                assert!(s.inspector_tab_selected(id));
                assert_eq!(c.panel,before.panel);assert_eq!(c.timeline_panel,before.timeline_panel);
                if matches!(id,ControlId::AudioTab|ControlId::SubtitleTab) {
                    let p=s.inspector_panel.unwrap();
                    if let Some(old)=media {assert_eq!(p,old);} else {media=Some(p);}
                    assert!(unsafe{IsWindowVisible(p)}.as_bool());
                    assert!(unsafe{GetDlgItem(Some(p),20)}.is_err(),"no duplicate tab strip");
                    let (total,page,_)=crate::platform::desktop_ui::panel_scroll_info(p);
                    assert!(total>page,"content scrolls within the upper inspector");
                    check_actions(p, id==ControlId::SubtitleTab);
                    let control=unsafe{GetDlgItem(Some(p),if id==ControlId::SubtitleTab {50}else{31})}.unwrap();
                    let mut before=RECT::default();unsafe{GetWindowRect(control,&mut before).unwrap();SendMessageW(p,WM_VSCROLL,Some(WPARAM(7)),None);}
                    let mut after=RECT::default();unsafe{GetWindowRect(control,&mut after).unwrap();}assert_eq!(before,after,"actions stay fixed while content scrolls");
                    unsafe{SendMessageW(p,WM_VSCROLL,Some(WPARAM(6)),None);}
                } else if let Some(p)=media {assert!(!unsafe{IsWindowVisible(p)}.as_bool());}
                let path=out.join(format!("{label}-{name}.png"));
                capture_tools::review_window_png(root,&path);
                let mut origin=POINT::default();unsafe{let _=windows::Win32::Graphics::Gdi::ClientToScreen(h,&mut origin);let _=ScreenToClient(root,&mut origin);}
                let x=origin.x+(c.inspector_splitter.left+c.inspector_splitter.right)/2;
                let y=origin.y+c.inspector_splitter.top+s.theme.scale(20);
                let pixels=image::open(path).unwrap().to_rgb8();let color=s.theme.palette.divider.0;
                assert_eq!(pixels.get_pixel(x as u32,y as u32).0,[color as u8,(color>>8)as u8,(color>>16)as u8],"visible common separator on {label}");
            }
        }
        // Both draggable boundaries also place the plugin page through the same layout.
        click(h,ControlId::AudioTab);let c=check(h);let s=unsafe{state_mut(h)}.unwrap();
        let x=(c.inspector_splitter.left+c.inspector_splitter.right)/2;let y=c.inspector_splitter.top+80;
        s.on_mouse_down(h,x,y);assert!(s.inspector_resize.is_some());s.on_mouse_move(h,x-50,y);s.on_mouse_up(h,x-50,y);pump();
        let wide=check(h);assert!(wide.panel.right-wide.panel.left>c.panel.right-c.panel.left);assert_eq!(wide.timeline_panel,c.timeline_panel);
        let y=(wide.splitter.top+wide.splitter.bottom)/2;let s=unsafe{state_mut(h)}.unwrap();
        s.on_mouse_down(h,200,y);assert!(s.timeline_resize.is_some());s.on_mouse_move(h,200,y-80);s.on_mouse_up(h,200,y-80);pump();
        let tall=check(h);assert!(tall.panel.bottom<wide.panel.bottom);
        click(h,ControlId::InspectorToggle);assert!(!unsafe{IsWindowVisible(media.unwrap())}.as_bool());
        click(h,ControlId::InspectorToggle);let reopened=check(h);assert_eq!(reopened.panel,tall.panel);
        // Hidden content retains job ownership and continues receiving results.
        let (cancel,tx)=media_panel::review_pending_job(media.unwrap());
        click(h,ControlId::CanvasTab);assert!(!cancel.load(Ordering::Acquire));
        tx.send(crate::platform::media_jobs::Event::Progress("后台任务仍在运行".into())).unwrap();pump();
        assert_eq!(ui::value(unsafe{GetDlgItem(Some(media.unwrap()),51)}.unwrap()),"后台任务仍在运行");
        click(h,ControlId::AudioTab);assert_eq!(unsafe{state_mut(h)}.unwrap().inspector_panel,media);assert!(!cancel.load(Ordering::Acquire));
        tx.send(crate::platform::media_jobs::Event::Done(Err("测试任务已结束".into()))).unwrap();pump();
        let stop=out.join("synthetic-recording-stop");
        let tx=media_panel::review_recording(media.unwrap(),stop.clone());
        click(h,ControlId::CanvasTab);
        unsafe{SendMessageW(root,WM_COMMAND,Some(WPARAM(100)),None);}pump();
        assert!(unsafe{IsWindowVisible(GetDlgItem(Some(root),117).unwrap())}.as_bool(),"hidden task remains discoverable");
        assert!(unsafe{IsWindowVisible(GetDlgItem(Some(root),118).unwrap())}.as_bool(),"stop remains available outside audio workspace");
        unsafe{SendMessageW(root,WM_COMMAND,Some(WPARAM(118)),None);}assert!(stop.exists());
        tx.send(crate::platform::media_jobs::Event::Done(Err("测试录音已结束".into()))).unwrap();pump();
        unsafe{SendMessageW(root,WM_COMMAND,Some(WPARAM(101)),None);}pump();
        // Track properties are another content page in the same host, not a second sidebar.
        let audio=original.audio_clips.first().unwrap().id.clone();
        unsafe{state_mut(h)}.unwrap().media_audio_properties(h,&audio).unwrap();pump();
        let properties=inspector_panel(h).unwrap();assert_ne!(Some(properties),media);check(h);
        assert!(!unsafe{IsWindowVisible(media.unwrap())}.as_bool());
        ui::text(unsafe{GetDlgItem(Some(properties),103)}.unwrap(),"73");
        unsafe{SendMessageW(properties,windows::Win32::UI::WindowsAndMessaging::WM_COMMAND,Some(WPARAM(1)),None);}
        assert_eq!(unsafe{state_mut(h)}.unwrap().editor.settings().audio_clips.iter().find(|clip|clip.id==audio).unwrap().gain,0.73);
        unsafe{state_mut(h)}.unwrap().undo();
        workspace::hide_form(properties);pump();assert!(inspector_panel(h).is_none());check(h);
        click(h,ControlId::SubtitleTab);
        let cue=original.subtitles.first().unwrap().id.clone();
        media_panel::select_item(h,true,&cue);pump();
        let p=inspector_panel(h).unwrap();
        ui::text(unsafe{GetDlgItem(Some(media_panel::review_body(p)),140)}.unwrap(),"统一侧栏中的字幕编辑");
        unsafe{SendMessageW(p,windows::Win32::UI::WindowsAndMessaging::WM_COMMAND,Some(WPARAM(50)),None);}
        assert_eq!(unsafe{state_mut(h)}.unwrap().editor.settings().subtitles.iter().find(|item|item.id==cue).unwrap().text,"统一侧栏中的字幕编辑");
        unsafe{state_mut(h)}.unwrap().undo();pump();
        unsafe {SetWindowPos(root,None,0,0,1000,740,SWP_NOMOVE|SWP_NOZORDER|SWP_NOACTIVATE).unwrap();}pump();check(h);check_actions(inspector_panel(h).unwrap(),true);
        capture_tools::review_window_png(root,&out.join("compact-subtitles.png"));
        let s=unsafe{state_mut(h)}.unwrap();assert_eq!(s.editor.settings(),&original);assert!(!s.editor.is_dirty());
        s.close_at=Some(Instant::now()+Duration::from_millis(200));
        std::fs::write(out.join("review.txt"),"PASS: one inspector host for canvas/video/audio/subtitles/track properties; common visible separator; full-width timeline; panel bottom follows timeline splitter; vertical and horizontal drag; collapse/reopen and cached panel reuse; audio property apply and subtitle text edit with undo; covered controls disabled; hidden mock job retained and polled; fixed primary actions stay fully visible while scrolling; global task return/stop remains available across workspaces (synthetic stop signal); dark/light and compact native windows; no microphone capture or AI request; project unchanged.\n").unwrap();
    }))).unwrap();
    assert!(report.graceful_close);
    assert_eq!(report.media_errors, 0);
}
