//! v14 native layout and interaction acceptance on a disposable fixture/profile.
#[path="preview_quality_review.rs"]
mod preview_quality_review;
use super::*;
use crate::platform::{
    capture_tools, media_panel,
    ui_preferences::{self, ThemeMode},
    workspace, workspace_ui as ui,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetDlgItem, MSG, PM_REMOVE, PeekMessageW, SWP_NOMOVE, SendMessageW,
    TranslateMessage, WM_COMMAND,
};
fn pump() {
    let end = Instant::now() + Duration::from_millis(350);
    while Instant::now() < end {
        let mut msg = MSG::default();
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn select(h: HWND, id: ControlId) {
    let s = unsafe { state_mut(h) }.unwrap();
    s.activate_control(h, id);
    s.refresh(h, true);
    pump();
}
fn snap(root: HWND, out: &Path, name: &str) {
    pump();
    capture_tools::review_window_png(root, &out.join(format!("{name}.png")));

}
#[test]
#[ignore = "requires isolated native fixture, output and LOCALAPPDATA"]
fn workbench_v14_native_review() {
    assert!(unsafe { windows::Win32::UI::Input::Ime::ImmDisableIME(0) }.as_bool());
    let project = std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_PROJECT").unwrap());
    let out = std::path::PathBuf::from(std::env::var_os("PANZO_UI_REVIEW_DIR").unwrap());
    assert!(std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).starts_with(&out));
    let package = crate::platform::plugins::read_package(&std::path::PathBuf::from(
        std::env::var_os("PANZO_MEDIA_REVIEW_PACKAGE").unwrap(),
    ))
    .unwrap();
    crate::platform::plugins::Store::open(
        std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("Panzo/Plugins"),
    )
    .unwrap()
    .install(&package)
    .unwrap();
    let library = out.join("library");
    std::fs::create_dir_all(&library).unwrap();
    ui_preferences::set_library_directory(library.clone()).unwrap();
    crate::platform::asset_index::register(&library, &project).unwrap();
    workspace::enable_review();
    ui_preferences::set_theme(ThemeMode::Dark).unwrap();
    let report=PreviewWindow::run_internal_notifying(&project,Some(Duration::from_secs(150)),SmokeAction::None,Some(Box::new(move|raw|{
 let h=HWND(raw as *mut c_void);let root=workspace::window(h);
 unsafe{SetWindowPos(root,None,20,20,ui::scale(root,1440),ui::scale(root,900),SWP_NOZORDER|SWP_NOACTIVATE).unwrap();}pump();
 {let s=unsafe{state_mut(h)}.unwrap();s.theme.metrics.inspector_width=s.theme.scale(304);s.theme.metrics.timeline_height=s.theme.scale(282);s.selected_video=s.editor.video_edit().clips.first().map(|c|c.id.clone());s.refresh(h,true);}
 select(h,ControlId::VideoTab);snap(root,&out,"01-video-dark");
 if std::env::var_os("PANZO_PREVIEW_QUALITY_REVIEW").is_some() {
    preview_quality_review::run(h,root,&out);
    unsafe{state_mut(h)}.unwrap().close_at=Some(Instant::now()+Duration::from_millis(300));return;
 }
 if std::env::var_os("PANZO_MEDIA_POLISH_REVIEW").is_some() {
    media_polish_review::run(h,root,&out);
    unsafe{state_mut(h)}.unwrap().close_at=Some(Instant::now()+Duration::from_millis(300));return;
 }
 if std::env::var_os("PANZO_INSPECTOR_REVIEW").is_some() {
    inspector_settings_review(h,root,&out);
    unsafe{state_mut(h)}.unwrap().close_at=Some(Instant::now()+Duration::from_millis(300));return;
 }
 if std::env::var_os("PANZO_INTERACTION_REVIEW").is_some() {
    interaction_crop_review(h,root,&out);
    unsafe{state_mut(h)}.unwrap().close_at=Some(Instant::now()+Duration::from_millis(300));return;
 }
 if std::env::var_os("PANZO_PROJECT_ASSET_REVIEW").is_some() {
    project_assets_review(h,root,&out);reliability_review(h,root,&out);unsafe{state_mut(h)}.unwrap().close_at=Some(Instant::now()+Duration::from_millis(300));return;
 }
 if std::env::var_os("PANZO_MEDIA_FIXES_REVIEW").is_some() {
    media_fixes_review(h,root,&out);
    unsafe{state_mut(h)}.unwrap().close_at=Some(Instant::now()+Duration::from_millis(300));
    return;
 }
 if std::env::var_os("PANZO_PANEL_REVIEW").is_some() {
    splitter_component_review(h,root,&out);
    panel_resize_review(h,root,&out);
    continuous_panel_review(h,root,&out);
    unsafe{state_mut(h)}.unwrap().close_at=Some(Instant::now()+Duration::from_millis(300));
    return;
 }
 if std::env::var_os("PANZO_HOVER_REVIEW").is_some() {
    hover_review(h,root,&out);
    unsafe{state_mut(h)}.unwrap().close_at=Some(Instant::now()+Duration::from_millis(300));
    return;
 }
 {let s=unsafe{state_mut(h)}.unwrap();project_bin::review_cards(s.project_bin.as_ref().unwrap(),&out);}
 assert!(!unsafe{state_mut(h)}.unwrap().session.preview_muted());select(h,ControlId::PreviewMute);assert!(unsafe{state_mut(h)}.unwrap().session.preview_muted());select(h,ControlId::PreviewMute);
 let c=unsafe{state_mut(h)}.unwrap().control_layout(client_rect(h));assert_eq!(c.context_bar,RECT::default());assert!(c.project_bin.right>c.project_bin.left);assert_eq!(c.panel.bottom,c.splitter.top);assert_eq!(c.start_value.0,RECT::default());assert_eq!(c.end_value.0,RECT::default());assert!(c.video_scale.left>c.panel.left);
 select(h,ControlId::CanvasTab);snap(root,&out,"02-canvas-dark");
 select(h,ControlId::AudioTab);let audio=inspector_panel(h).unwrap();snap(root,&out,"03-audio-dark");
 select(h,ControlId::SubtitleTab);let subtitle=inspector_panel(h).unwrap();assert_ne!(audio,subtitle);snap(root,&out,"04-subtitles-dark");
 let cue=unsafe{state_mut(h)}.unwrap().editor.settings().subtitles.first().map(|c|c.id.clone());
 if let Some(cue)=cue{media_panel::select_item(h,true,&cue);let body=media_panel::review_body(inspector_panel(h).unwrap());ui::text(unsafe{GetDlgItem(Some(body),140)}.unwrap(),"v14 字幕校对");unsafe{SendMessageW(inspector_panel(h).unwrap(),WM_COMMAND,Some(WPARAM(50)),None);}assert_eq!(unsafe{state_mut(h)}.unwrap().editor.settings().subtitles.iter().find(|c|c.id==cue).unwrap().text,"v14 字幕校对");unsafe{state_mut(h)}.unwrap().undo();}
 let clip=unsafe{state_mut(h)}.unwrap().editor.settings().audio_clips.first().map(|c|c.id.clone());if let Some(id)=clip{media_panel::select_item(h,false,&id);pump();snap(root,&out,"05-audio-clip-dark");let panel=inspector_panel(h).unwrap();let body=media_panel::review_body(panel);ui::text(unsafe{GetDlgItem(Some(body),123)}.unwrap(),"50");unsafe{SendMessageW(panel,WM_COMMAND,Some(WPARAM(50)),None);}assert_eq!(unsafe{state_mut(h)}.unwrap().editor.settings().audio_clips.iter().find(|c|c.id==id).unwrap().gain,0.5);unsafe{state_mut(h)}.unwrap().undo();}
 select(h,ControlId::VideoTab);{let s=unsafe{state_mut(h)}.unwrap();s.theme.metrics.timeline_height=s.theme.scale(170);s.refresh(h,true);let c=s.control_layout(client_rect(h));assert_eq!(c.video_track.bottom-c.video_track.top,s.theme.scale(32));assert!(c.track_scroll_max>0);s.track_scroll=c.track_scroll_max;s.refresh(h,true);}
 snap(root,&out,"06-compact-tracks-dark");
 unsafe{SetWindowPos(root,None,0,0,ui::scale(root,976),ui::scale(root,752),SWP_NOMOVE|SWP_NOZORDER|SWP_NOACTIVATE).unwrap();}pump();select(h,ControlId::AudioTab);snap(root,&out,"07-compact-audio-dark");
 {let s=unsafe{state_mut(h)}.unwrap();let c=s.control_layout(client_rect(h));assert_eq!(c.project_bin.right-c.project_bin.left,0);}
 select(h,ControlId::Menu);assert!(unsafe{state_mut(h)}.unwrap().preview_surface.as_ref().unwrap().well.left>=212);snap(root,&out,"08-compact-bin-dark");select(h,ControlId::Menu);
 unsafe{SetWindowPos(root,None,20,20,ui::scale(root,1440),ui::scale(root,900),SWP_NOZORDER|SWP_NOACTIVATE).unwrap();}pump();
 ui_preferences::set_theme(ThemeMode::Light).unwrap();select(h,ControlId::SubtitleTab);snap(root,&out,"09-subtitles-light");
 ui_preferences::set_theme(ThemeMode::Dark).unwrap();
 if std::env::var_os("PANZO_VIDEO_ONLY").is_some() {
    {let s=unsafe{state_mut(h)}.unwrap();s.theme.metrics.timeline_height=s.theme.scale(282);s.track_scroll=0;}
    select(h,ControlId::CanvasTab);
    let before=unsafe{state_mut(h)}.unwrap().editor.settings().canvas.inset;
    {let s=unsafe{state_mut(h)}.unwrap();s.begin_inline_input(h,ControlId::Inset);let input=s.inline_input.as_ref().unwrap().hwnd;ui::text(input,"36");assert!(s.finish_inline_input(h,true));assert!((s.editor.settings().canvas.inset-36.0/f64::from(s.canvas_size().width.min(s.canvas_size().height))).abs()<1e-9);s.undo();assert!((s.editor.settings().canvas.inset-before).abs()<1e-9);}
    select(h,ControlId::BackgroundColor);snap(root,&out,"10-background-color");
    let pop=unsafe{windows::Win32::UI::WindowsAndMessaging::FindWindowW(w!("PanzoColorPopover"),None)}.unwrap();
    let hex=unsafe{GetDlgItem(Some(pop),10)}.unwrap();ui::text(hex,"#123456");unsafe{SendMessageW(pop,WM_COMMAND,Some(WPARAM(10|((windows::Win32::UI::WindowsAndMessaging::EN_CHANGE as usize)<<16))),None);SendMessageW(pop,WM_CLOSE,None,None);}
    pump();assert!(unsafe{state_mut(h)}.unwrap().editor.settings().background.color.eq_ignore_ascii_case("#123456"));unsafe{state_mut(h)}.unwrap().undo();
    select(h,ControlId::VideoTab);select(h,ControlId::SpeedSection);{let s=unsafe{state_mut(h)}.unwrap();s.inspector_scroll=s.control_layout(client_rect(h)).inspector_scroll_max;s.refresh(h,true);}snap(root,&out,"11-follow-expanded");select(h,ControlId::SpeedSection);
    select(h,ControlId::SubtitleTab);let cue=unsafe{state_mut(h)}.unwrap().editor.settings().subtitles.first().unwrap().id.clone();media_panel::select_item(h,true,&cue);let panel=inspector_panel(h).unwrap();pump();
    let body=media_panel::review_body(panel);let size=unsafe{GetDlgItem(Some(body),122)}.unwrap();ui::text(size,"24");unsafe{SendMessageW(panel,WM_COMMAND,Some(WPARAM(122|((windows::Win32::UI::WindowsAndMessaging::CBN_EDITCHANGE as usize)<<16))),None);}pump();
    assert!((unsafe{state_mut(h)}.unwrap().editor.settings().subtitle_style_for(unsafe{state_mut(h)}.unwrap().media_selected.as_ref().map_or("",|(_,id)|id)).font_size*media_canvas_height(h).unwrap() as f64-24.).abs()<0.01);
    unsafe{SendMessageW(panel,windows::Win32::UI::WindowsAndMessaging::WM_VSCROLL,Some(WPARAM(7)),None);}pump();
    snap(root,&out,"12-subtitle-style");unsafe{state_mut(h)}.unwrap().undo();
    select(h,ControlId::Export);snap(root,&out,"13-export-dark");unsafe{state_mut(h)}.unwrap().export_panel=None;

    crate::platform::recording_setup::open(h,"已选区域 · 1920 × 1080",0,&crate::platform::recorder::RecordingOptions::default()).unwrap();snap(root,&out,"14-recording-dark");capture_tools::review_window_png(crate::platform::recording_setup::review_handle(),&out.join("recording-panel.png"));crate::platform::recording_setup::close();
    ui_preferences::set_theme(ThemeMode::Light).unwrap();select(h,ControlId::CanvasTab);
    crate::platform::recording_setup::open(h,"显示器 1 · 2560 × 1440",2,&crate::platform::recorder::RecordingOptions::default()).unwrap();snap(root,&out,"15-recording-light");crate::platform::recording_setup::close();
    select(h,ControlId::Export);snap(root,&out,"16-export-light");unsafe{state_mut(h)}.unwrap().export_panel=None;
    {let s=unsafe{state_mut(h)}.unwrap();s.close_at=Some(Instant::now()+Duration::from_millis(300));}
    std::fs::write(out.join("review.txt"),"PASS: video panels, compact tracks, pixel inset/undo, live background colour/undo, subtitle pixel size, recording and export light/dark screenshots, material cards/overflow/hover/search. Disposable project; no microphone or network request.\n").unwrap();
    return;
 }
 let library=out.join("library");std::fs::create_dir_all(&library).unwrap();let mut img=image::RgbaImage::new(960,540);for(x,y,p)in img.enumerate_pixels_mut(){*p=image::Rgba([110+(x/12)as u8,120+(y/8)as u8,150,255]);}let path=library.join("layout-fixture.png");img.save(&path).unwrap();
 crate::platform::image_workspace::open_source(&path,&library).unwrap();crate::platform::image_workspace::open_source(&path,&library).unwrap();pump();assert_eq!(crate::platform::image_workspace::review()["documents"].as_array().unwrap().len(),1);workspace::switch(0);snap(root,&out,"10-image-dark");crate::platform::image_workspace::picture_command(1);snap(root,&out,"10b-image-rectangle-dark");
 {let (pic,_)=crate::platform::image_workspace::current_document().unwrap();let mut rows=Vec::new();for id in [38,50,54,100,101,110,111,112,113] {let c=unsafe{GetDlgItem(Some(pic),id)}.unwrap();let mut rect=RECT::default();unsafe{windows::Win32::UI::WindowsAndMessaging::GetWindowRect(c,&mut rect).unwrap();}let mut parent=RECT::default();unsafe{windows::Win32::UI::WindowsAndMessaging::GetWindowRect(pic,&mut parent).unwrap();}assert!(rect.left>=parent.left && rect.right<=parent.right,"control {id} left outside its resized parent: {rect:?} / {parent:?}");rows.push(format!("{id} visible={} rect={rect:?}",unsafe{windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(c)}.as_bool()));}std::fs::write(out.join("image-controls.txt"),rows.join("\n")).unwrap();}


 // Colour row: circles and custom swatch share an exact centre, including focus.
 {
 let (pic,_)=crate::platform::image_workspace::current_document().unwrap();
 let dot=unsafe{GetDlgItem(Some(pic),112)}.unwrap();let custom=unsafe{GetDlgItem(Some(pic),38)}.unwrap();
 let mut a=RECT::default();let mut b=RECT::default();unsafe{windows::Win32::UI::WindowsAndMessaging::GetWindowRect(dot,&mut a).unwrap();windows::Win32::UI::WindowsAndMessaging::GetWindowRect(custom,&mut b).unwrap();}
 assert_eq!(a.top+a.bottom,b.top+b.bottom,"colour row vertical centres");
 crate::platform::image_workspace::picture_command(112);
 unsafe{let _=windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(dot));}
 snap(root,&out,"10c-color-selected-focused-dark");
 }
 crate::platform::image_workspace::picture_command(36);snap(root,&out,"11-crop-dark");
 {
 let (pic,_)=crate::platform::image_workspace::current_document().unwrap();
 let original=capture_tools::workspace_metadata(pic).unwrap();
 let width=unsafe{GetDlgItem(Some(pic),80)}.unwrap();let height=unsafe{GetDlgItem(Some(pic),81)}.unwrap();
 ui::text(width,"720");ui::text(height,"405");
 unsafe{SendMessageW(pic,WM_COMMAND,Some(WPARAM(82)),None);}
 snap(root,&out,"11b-crop-inset-dark");
 unsafe{let _=windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(width));}
 snap(root,&out,"11e-number-focused-dark");

 ui_preferences::set_theme(ThemeMode::Light).unwrap();pump();snap(root,&out,"11c-crop-light");
 unsafe{SetWindowPos(root,None,20,20,ui::scale(root,960),ui::scale(root,720),SWP_NOZORDER|SWP_NOACTIVATE).unwrap();}pump();snap(root,&out,"11d-crop-compact-light");
 let apply=unsafe{GetDlgItem(Some(pic),61)}.unwrap();let cancel=unsafe{GetDlgItem(Some(pic),62)}.unwrap();
 assert_eq!(ui::value(apply),"应用裁剪");
 let mut a=RECT::default();let mut c=RECT::default();unsafe{windows::Win32::UI::WindowsAndMessaging::GetWindowRect(apply,&mut a).unwrap();windows::Win32::UI::WindowsAndMessaging::GetWindowRect(cancel,&mut c).unwrap();}
 assert!(a.right-a.left>c.right-c.left);assert!(a.left>c.right);assert_eq!(a.top,c.top);
 unsafe{SendMessageW(pic,WM_COMMAND,Some(WPARAM(61)),None);}
 let cropped=capture_tools::workspace_metadata(pic).unwrap();assert_eq!((cropped.0,cropped.1),(720,405));
 let image=workspace::image_parent().unwrap();unsafe{SendMessageW(image,WM_COMMAND,Some(WPARAM(35)),None);}
 let undone=capture_tools::workspace_metadata(pic).unwrap();assert_eq!((undone.0,undone.1),(original.0,original.1),"top toolbar undo restores image dimensions");
 // Save the disposable fixture so the exit check does not wait on a user prompt.
 unsafe{SendMessageW(pic,WM_COMMAND,Some(WPARAM(44)),None);}

 unsafe{SetWindowPos(root,None,20,20,ui::scale(root,1440),ui::scale(root,900),SWP_NOZORDER|SWP_NOACTIVATE).unwrap();}
 ui_preferences::set_theme(ThemeMode::Dark).unwrap();pump();
 }

 let image=workspace::image_parent().unwrap();unsafe{SendMessageW(image,WM_COMMAND,Some(WPARAM(15)),None);}snap(root,&out,"12-ocr-dark");
 crate::platform::collage_workspace::open(&library).unwrap();let mut photos=vec![path.clone()];for i in 2..=4{let target=library.join(format!("layout-{i}.png"));std::fs::copy(&path,&target).unwrap();photos.push(target);}crate::platform::collage_workspace::review_add(&photos);let end=Instant::now()+Duration::from_secs(20);while !crate::platform::collage_workspace::review_ready(){assert!(Instant::now()<end,"collage render timeout");pump();}snap(root,&out,"13-collage-dark");workspace::image_view(None);
 workspace::switch(2);pump();let assets=unsafe{windows::Win32::UI::WindowsAndMessaging::FindWindowExW(Some(root),None,windows::core::w!("PanzoCaptureHistory"),None)}.unwrap();unsafe{SendMessageW(assets,WM_COMMAND,Some(WPARAM(8)),None);}for _ in 0..5{pump();}snap(root,&out,"14-assets-dark");
 crate::platform::workspace_settings::general().unwrap();pump();snap(crate::platform::management_window::get(3).unwrap(),&out,"15-settings-dark");ui::show(crate::platform::management_window::get(3).unwrap(),false);
 crate::platform::workspace_settings::open().unwrap();pump();snap(crate::platform::management_window::get(3).unwrap(),&out,"16-vision-settings-dark");ui::show(crate::platform::management_window::get(3).unwrap(),false);
 crate::platform::speech_settings::open().unwrap();pump();snap(crate::platform::management_window::get(3).unwrap(),&out,"17-speech-settings-dark");ui::show(crate::platform::management_window::get(3).unwrap(),false);
 crate::platform::plugin_manager::open().unwrap();pump();snap(crate::platform::management_window::get(4).unwrap(),&out,"18-plugins-dark");ui::show(crate::platform::management_window::get(4).unwrap(),false);
 workspace::switch(1);select(h,ControlId::VideoTab);{let s=unsafe{state_mut(h)}.unwrap();s.theme.metrics.timeline_height=s.theme.scale(282);s.track_scroll=0;s.refresh(h,true);s.close_at=Some(Instant::now()+Duration::from_millis(300));}
 std::fs::write(out.join("review.txt"),"PASS: native v14 contexts, project bin, single inspector, clip range, audio/subtitle pages, subtitle apply/undo, compact tracks preserve height and scroll, compact bin overlay, image/crop/OCR/collage/assets screenshots. Isolated generated fixture; no microphone or AI network request.\n").unwrap();
 }))).unwrap();
    assert!(report.graceful_close);
    assert_eq!(report.media_errors, 0);
}

fn hover_review(h:HWND,root:HWND,out:&Path) {
    use windows::Win32::{Foundation::POINT,Graphics::Gdi::ClientToScreen,UI::{Input::KeyboardAndMouse::GetFocus,WindowsAndMessaging::*}};
    use crate::platform::hover_tip;
    let snapshot_tip=|tip:HWND,name:&str| {
        let r=ui::rect(tip);
        crate::platform::editor_ui::render_review_png(&out.join(name),r.right,r.bottom,|dc,_|{unsafe{SendMessageW(tip,WM_PRINTCLIENT,Some(WPARAM(dc.0 as usize)),None);}});
    };
    let hover=|owner:HWND,r:RECT| {
        pump();
        unsafe{SendMessageW(owner,windows::Win32::UI::Controls::WM_MOUSELEAVE,None,None);}
        unsafe{SendMessageW(owner,WM_MOUSEMOVE,Some(WPARAM(0)),Some(LPARAM((((r.top+r.bottom)/2)<<16 | (((r.left+r.right)/2)&65535)) as isize)));}
        // Deliver the real timer after its delay without moving the user's pointer.
        // Physical WM_MOUSELEAVE queued by unrelated input is drained before each case.
        std::thread::sleep(Duration::from_millis(470));
        if owner==h {unsafe{state_mut(h)}.unwrap().on_timer(h);}else{unsafe{SendMessageW(owner,WM_TIMER,Some(WPARAM(0x50415a81)),None);}}
        let tip=hover_tip::review_handle(owner).expect("hover timer must create popup");
        assert!(unsafe{IsWindowVisible(tip)}.as_bool(),"tip hidden for owner={owner:?} anchor={r:?} label={}",ui::value(tip));
        unsafe{let _=windows::Win32::Graphics::Gdi::RedrawWindow(Some(tip),None,None,windows::Win32::Graphics::Gdi::RDW_INVALIDATE|windows::Win32::Graphics::Gdi::RDW_UPDATENOW);}
        let mut a=r;let mut offset=POINT::default();unsafe{let _=ClientToScreen(owner,&mut offset);}a.left+=offset.x;a.right+=offset.x;a.top+=offset.y;a.bottom+=offset.y;
        let mut b=RECT::default();unsafe{GetWindowRect(tip,&mut b).unwrap();}
        assert!(b.bottom<a.top || b.left>a.right || b.right<a.left,"tip should be above/beside control, away from pointer: {a:?} {b:?}");
        assert_ne!(unsafe{GetWindowLongPtrW(tip,GWL_STYLE)}&WS_POPUP.0 as isize,0);
        assert_eq!(unsafe{SendMessageW(tip,WM_NCHITTEST,None,None)}.0,HTTRANSPARENT as isize);
        (tip,b)
    };
    let c=unsafe{state_mut(h)}.unwrap().control_layout(client_rect(h));let focus=unsafe{GetFocus()};
    let(tip,virtual_rect)=hover(h,c.new_recording.0);assert_eq!(unsafe{GetFocus()},focus);assert_eq!(ui::value(tip),ControlId::NewRecording.tooltip());
    snapshot_tip(tip,"tooltip-toolbar-dark.png");
    std::thread::sleep(Duration::from_millis(1100));
    unsafe { SendMessageW(tip,WM_TIMER,Some(WPARAM(1)),None); }
    assert!(unsafe{IsWindowVisible(tip)}.as_bool(), "stationary hover on an inactive window must survive the popup timer: {}",hover_tip::review_diagnostics(h));
    unsafe{SendMessageW(h,windows::Win32::UI::Controls::WM_MOUSELEAVE,None,None);}assert!(!unsafe{IsWindowVisible(tip)}.as_bool());
    let bin=unsafe{state_mut(h)}.unwrap().project_bin.as_ref().unwrap().hwnd;let record=unsafe{GetDlgItem(Some(bin),4)}.unwrap();
    let(native,native_rect)=hover(record,ui::rect(record));assert_eq!(native_rect.bottom-native_rect.top,virtual_rect.bottom-virtual_rect.top);assert_eq!(unsafe{GetFocus()},focus);
    snapshot_tip(native,"tooltip-native-dark.png");
    unsafe{SendMessageW(record,windows::Win32::UI::Controls::WM_MOUSELEAVE,None,None);}assert!(!unsafe{IsWindowVisible(native)}.as_bool());
    select(h,ControlId::InspectorToggle);let c=unsafe{state_mut(h)}.unwrap().control_layout(client_rect(h));assert_eq!(c.panel.right-c.panel.left,0);assert!(c.preview_fit.right+ui::scale(h,8)<=c.inspector_toggle.left);
    snap(root,out,"inspector-closed-buttons");let(tip,_)=hover(h,c.preview_fit.0);assert_eq!(ui::value(tip),ControlId::PreviewFit.tooltip());snapshot_tip(tip,"tooltip-fit-dark.png");
    unsafe{SendMessageW(h,windows::Win32::UI::Controls::WM_MOUSELEAVE,None,None);}let(tip,_)=hover(h,c.inspector_toggle.0);assert_eq!(ui::value(tip),ControlId::InspectorToggle.tooltip());snapshot_tip(tip,"tooltip-inspector-dark.png");
    unsafe{SendMessageW(h,windows::Win32::UI::Controls::WM_MOUSELEAVE,None,None);}select(h,ControlId::InspectorToggle);
    ui_preferences::set_theme(ThemeMode::Light).unwrap();select(h,ControlId::CanvasTab);let c=unsafe{state_mut(h)}.unwrap().control_layout(client_rect(h));let(tip,_)=hover(h,c.new_recording.0);snapshot_tip(tip,"tooltip-toolbar-light.png");
    unsafe{SendMessageW(h,windows::Win32::UI::Controls::WM_MOUSELEAVE,None,None);}let(tip,_)=hover(record,ui::rect(record));snapshot_tip(tip,"tooltip-native-light.png");
    unsafe{SendMessageW(record,windows::Win32::UI::Controls::WM_MOUSELEAVE,None,None);}
    let long=ui::button(bin,&"很长的素材路径：D:/示例项目/屏幕录制/产品说明与操作演示/第一部分视频文件.mp4".repeat(3),987).unwrap();ui::place(long,12,280,130,28);let(tip,r)=hover(long,ui::rect(long));snapshot_tip(tip,"tooltip-long-label.png");std::fs::write(out.join("long-label.txt"),format!("rect={r:?} text={} source={}",ui::value(tip),ui::value(long))).unwrap();assert!(r.bottom-r.top>native_rect.bottom-native_rect.top);unsafe{DestroyWindow(long).unwrap();}assert!(hover_tip::review_handle(long).is_none());assert!(!unsafe{IsWindow(Some(tip))}.as_bool());
    ui_preferences::set_theme(ThemeMode::Dark).unwrap();select(h,ControlId::VideoTab);
    std::fs::write(out.join("review.txt"),"PASS: painted/native controls use the same popup, message-driven hover timer, above/side placement, full labels, light/dark, click-through, no focus change, leave hides, long labels wrap, owner destruction releases popup, collapsed inspector buttons have distinct hit targets.\n").unwrap();
}

fn panel_resize_review(h:HWND,root:HWND,out:&Path) {
    let s=unsafe{state_mut(h)}.unwrap();
    let original=s.control_layout(client_rect(h));
    let revision=s.editor.change_revision();
    let title_refresh=s.last_title_refresh;
    let preview=s.preview_surface.as_ref().unwrap().bounds;
    let x=(original.project_bin_splitter.left+original.project_bin_splitter.right)/2;
    let y=original.project_bin.top+s.theme.scale(200);
    s.on_mouse_down(h,x,y);
    assert!(s.project_bin_resize.is_some());
    for delta in 1..=80 { s.on_mouse_move(h,x+s.theme.scale(delta),y); }
    assert_eq!(s.preview_surface.as_ref().unwrap().bounds,preview,"mousemove burst must not resize GPU surface before frame commit");
    assert!(s.pending_panel_layout.is_some());
    let start=Instant::now();s.flush_panel_layout(h);let elapsed=start.elapsed();
    assert!(s.pending_panel_layout.is_none());
    let wider=s.control_layout(client_rect(h));
    assert!(wider.project_bin.right>original.project_bin.right);
    assert_eq!(wider.timeline_panel,original.timeline_panel);
    assert_eq!(wider.panel,original.panel);
    assert_eq!(s.last_title_refresh,title_refresh,"layout does not rebuild title/accessibility");
    assert_eq!(s.editor.change_revision(),revision);
    s.on_mouse_up(h,x+s.theme.scale(80),y);
    assert!(s.project_bin_resize.is_none());
    let committed=s.project_bin_width_dip;
    let c=s.control_layout(client_rect(h));let x=(c.project_bin_splitter.left+c.project_bin_splitter.right)/2;
    s.on_mouse_down(h,x,y);s.on_mouse_move(h,x+s.theme.scale(100),y);s.flush_panel_layout(h);s.cancel_gesture(h);
    assert_eq!(s.project_bin_width_dip,committed);
    for timeline in [false,true] {
        let c=s.control_layout(client_rect(h));
        let (x,y)=if timeline {(s.theme.scale(400),(c.splitter.top+c.splitter.bottom)/2)} else {((c.inspector_splitter.left+c.inspector_splitter.right)/2,c.panel.top+s.theme.scale(100))};
        s.on_mouse_down(h,x,y);assert!(s.panel_resizing());
        let before=s.preview_surface.as_ref().unwrap().bounds;
        let title=s.last_title_refresh;
        for delta in 1..=40 { s.on_mouse_move(h,x-if timeline{0}else{s.theme.scale(delta)},y-if timeline{s.theme.scale(delta)}else{0}); }
        assert_eq!(s.preview_surface.as_ref().unwrap().bounds,before);
        s.flush_panel_layout(h);assert_eq!(s.last_title_refresh,title);
        s.on_mouse_up(h,x-if timeline{0}else{s.theme.scale(40)},y-if timeline{s.theme.scale(40)}else{0});
        assert!(!s.panel_resizing());
    }
    assert_eq!(s.editor.change_revision(),revision);
    snap(root,out,"resized-panels");
    std::fs::write(out.join("review.txt"),format!("PASS: material/inspector/timeline splitters; 80 mouse moves coalesced into one layout commit ({elapsed:?}); unrelated panel geometry/title/revision unchanged; final release commit; cancellation restores width.\n")).unwrap();
}

fn continuous_panel_review(h:HWND,root:HWND,out:&Path) {
    unsafe extern "system" fn paint_child(child:HWND,_:LPARAM)->windows::core::BOOL {
        unsafe {let _=UpdateWindow(child);} windows::core::BOOL(1)
    }
    fn paint(h:HWND) { unsafe {let _=UpdateWindow(h);let _=windows::Win32::UI::WindowsAndMessaging::EnumChildWindows(Some(h),Some(paint_child),LPARAM(0));} }
    let mut report=String::new();
    for kind in [0,1,2,3,4] {
        if kind==3 {select(h,ControlId::AudioTab);}
        if kind==4 {select(h,ControlId::SubtitleTab);}
        paint(h);
        let s=unsafe{state_mut(h)}.unwrap();
        let c=s.control_layout(client_rect(h));
        let (x,y)=match kind {0=>((c.project_bin_splitter.left+c.project_bin_splitter.right)/2,c.project_bin.top+100),2=>(400,(c.splitter.top+c.splitter.bottom)/2),_=>((c.inspector_splitter.left+c.inspector_splitter.right)/2,c.panel.top+100)};
        s.on_mouse_down(h,x,y);assert!(s.panel_resizing());
        project_bin::review_paint_counts(true);
        let presentations=s.session.compositor_cache_stats().presentations;
        let revision=s.editor.change_revision();
        let mut timings=Vec::new();
        for frame in 0..60 {
            let delta=if frame<30 {frame*2+2}else{(60-frame)*2};
            let (mx,my)=if kind==2 {(x,y-delta)}else{(x+if kind==0{delta}else{-delta},y)};
            let start=Instant::now();
            s.on_mouse_move(h,mx,my);s.flush_panel_layout(h);paint(h);
            timings.push(start.elapsed().as_micros());
            std::thread::sleep(Duration::from_millis(16));
        }
        let counts=project_bin::review_paint_counts(false);
        let extra=s.session.compositor_cache_stats().presentations-presentations;
        timings.sort();
        report.push_str(&format!("kind={kind}: frames=60 median={}us p95={}us max={}us draws={} unbuffered={} height_sets={} list_paints={} extra_presentations={extra}\n",timings[30],timings[57],timings[59],counts[0],counts[1],counts[2],counts[3]));
        std::fs::write(out.join("continuous-drag.txt"),&report).unwrap();
        assert_eq!(counts[1],0,"material rows must paint into memory DC");
        assert_eq!(counts[2],0,"no redundant row height resets");
        assert_eq!(extra,0,"paused video must not recompose while resizing");
        assert_eq!(revision,s.editor.change_revision());
        s.on_mouse_up(h,x,y);
        snap(root,out,&format!("continuous-{kind}"));
    }
}

fn media_fixes_review(h:HWND,root:HWND,out:&Path) {
    use windows::Win32::UI::WindowsAndMessaging::*;
    use crate::platform::media_jobs::{Event,ResultData};
    select(h,ControlId::AudioTab);
    let panel=inspector_panel(h).unwrap();let body=media_panel::review_body(panel);
    let label=unsafe{GetDlgItem(Some(body),74)}.unwrap();let combo=unsafe{GetDlgItem(Some(body),153)}.unwrap();
    let mut a=RECT::default();let mut b=RECT::default();unsafe{GetWindowRect(label,&mut a).unwrap();GetWindowRect(combo,&mut b).unwrap();}
    assert!((a.top+a.bottom-b.top-b.bottom).abs()<=2,"label/combo vertical centres: {a:?} {b:?}");
    assert_ne!(unsafe{GetWindowLongPtrW(label,GWL_STYLE)} & 0x200,0);
    let state=unsafe{state_mut(h)}.unwrap();let original=state.editor.settings().clone();let revision=state.editor.change_revision();
    state.activate_control(h,ControlId::TimelineZoomOut);state.refresh(h,true);
    assert!((state.timeline.zoom_fraction()).abs()<1e-9);
    let c=state.control_layout(client_rect(h));let width=c.video_track.right-c.video_track.left;
    assert!((state.timeline.tick_to_offset(state.editor.project_duration(),width)-width/2).abs()<=1);
    let fixture=state.editor.project_root().to_path_buf();
    let job=out.join("synthetic-capture");std::fs::create_dir(&job).unwrap();
    let pcm=job.join("audio.pcm");std::fs::write(&pcm,[]).unwrap();
    let tx=media_panel::review_live_recording(panel,&job,TimeTick(10_000_000));
    for index in 0..3 {
        use std::io::Write;
        let sample=if index==1 {[0,0,0,0]}else{[0,0,0,64]};
        std::fs::OpenOptions::new().append(true).open(&pcm).unwrap().write_all(&sample.repeat(48000)).unwrap();
        std::fs::write(job.join("meter"),format!("{} 50",(index+1)*48000)).unwrap();
        if index==1 {ui::show(panel,false);}
        unsafe{SendMessageW(panel,WM_TIMER,Some(WPARAM(1)),None);}
        let live=state.recording_preview.as_ref().unwrap();
        assert_eq!(live.frames,(index+1)*48000);assert_eq!(live.peaks.len(),(index+1) as usize*10);
        assert_eq!(state.editor.change_revision(),revision,"recording preview must not edit the project");
        if index==1 {assert_eq!(live.peaks[10],0.0);ui::show(panel,true);}
        snap(root,out,&format!("live-narration-{index}"));
    }
    unsafe{SendMessageW(panel,WM_COMMAND,Some(WPARAM(32)),None);}assert!(job.join("stop").exists());
    assert!(state.recording_preview.is_some(),"retain live clip until capture finishes");
    let relative="media/audio-clips/11111111-2222-4333-8444-555555555555.pcm";std::fs::copy(&pcm,fixture.join(relative)).unwrap();
    let clip=panzo_core::media_edit::AudioClip{speed_percent:100,id:"live-narration-test".into(),name:"旁白实时验收".into(),path:relative.into(),asset_frames:144000,start:TimeTick(10_000_000),source_in:TimeTick::ZERO,source_out:TimeTick(30_000_000),role:panzo_core::media_edit::AudioRole::Narration,gain:1.0,muted:false,fade_in:TimeTick::ZERO,fade_out:TimeTick::ZERO,waveform:vec![0.5;30]};
    tx.send(Event::Done(Ok(ResultData::Audio(clip.clone())))).unwrap();unsafe{SendMessageW(panel,WM_TIMER,Some(WPARAM(1)),None);}
    assert!(state.recording_preview.is_none());assert_eq!(state.editor.settings().audio_clips.len(),original.audio_clips.len()+1);
    state.undo();assert_eq!(state.editor.settings(),&original,"one undo removes recording");state.redo();
    let mut next=state.editor.settings().clone();let mut duplicate=clip.clone();duplicate.id="live-narration-duplicate".into();duplicate.start=TimeTick(20_000_000);next.audio_clips.push(duplicate);
    state.editor.replace_media_settings(next).unwrap();state.apply_editor_state().unwrap();state.refresh(h,true);
    state.editor.write_recovery_draft().unwrap();
    let mut recovered=crate::platform::editor::ProjectEditorSession::open(&fixture).unwrap();
    assert!(recovered.restore_recovery_draft().unwrap());
    assert_eq!(recovered.settings(),state.editor.settings(),"legacy saved project can restore asset-aware draft");
    state.editor.clear_recovery_draft().unwrap();
    let before_delete=state.editor.settings().clone();
    let bin=state.project_bin.as_ref().unwrap().hwnd;
    let search=unsafe{GetDlgItem(Some(bin),1)}.unwrap();let list=unsafe{GetDlgItem(Some(bin),2)}.unwrap();
    ui::text(search,"旁白实时验收");assert_eq!(unsafe{SendMessageW(list,LB_GETCOUNT,None,None)}.0,1);
    let msg=MSG{hwnd:list,message:WM_KEYDOWN,wParam:WPARAM(0x2e),..Default::default()};
    assert!(!crate::platform::shortcuts::dialog_message(&msg),"global Delete must yield to material list");
    unsafe{SendMessageW(list,WM_KEYDOWN,Some(WPARAM(0x2e)),None);}
    assert_eq!(state.editor.settings(),&before_delete,"used asset must not be deleted");
    for id in ["live-narration-test","live-narration-duplicate"] {
        state.media_selected=Some((false,id.into()));state.delete_media();state.refresh(h,true);
    }
    assert!(!state.editor.settings().audio_clips.iter().any(|v|v.path==relative));
    assert_eq!(unsafe{SendMessageW(list,LB_GETCOUNT,None,None)}.0,1,"unused asset must remain in bin");
    state.editor.save(CameraEditKind::Update,None).unwrap();
    let reopened=crate::platform::editor::ProjectEditorSession::open(&fixture).unwrap();
    assert!(reopened.settings().audio_assets.iter().any(|v|v.path==relative));
    assert!(!reopened.settings().audio_clips.iter().any(|v|v.path==relative));
    let before_delete=state.editor.settings().clone();
    unsafe{SendMessageW(list,WM_KEYDOWN,Some(WPARAM(0x2e)),None);}
    assert!(!state.editor.settings().audio_assets.iter().any(|v|v.path==relative));
    assert_eq!(unsafe{SendMessageW(list,LB_GETCOUNT,None,None)}.0,0);assert!(fixture.join(relative).exists());
    state.undo();assert_eq!(state.editor.settings(),&before_delete);state.redo();state.undo();
    ui::text(search,"");state.refresh(h,true);
    snap(root,out,"half-width-and-aligned-audio");
    for canceled in [true,false] {
        let job=out.join(if canceled{"capture-cancel"}else{"capture-error"});std::fs::create_dir(&job).unwrap();
        std::fs::write(job.join("audio.pcm"),[0,0,0,64].repeat(4800)).unwrap();
        let tx=media_panel::review_live_recording(panel,&job,TimeTick::ZERO);unsafe{SendMessageW(panel,WM_TIMER,Some(WPARAM(1)),None);}
        let revision=state.editor.change_revision();assert!(state.recording_preview.is_some());
        if canceled {unsafe{SendMessageW(panel,WM_COMMAND,Some(WPARAM(37)),None);}}
        tx.send(Event::Done(Err("录音测试结束".into()))).unwrap();unsafe{SendMessageW(panel,WM_TIMER,Some(WPARAM(1)),None);}
        assert!(state.recording_preview.is_none());assert_eq!(state.editor.change_revision(),revision);
    }
    std::fs::write(out.join("review.txt"),"PASS: synthetic PCM grows live narration with actual peaks and silent interval; hidden inspector continues updates; stop retains preview until final commit; one undo removes recording; cancel/error clean preview without edit; used asset deletion is blocked; deleting track clips preserves unused bin asset through save/reopen; deleting unused searched asset preserves PCM; undo/redo restore; Delete routing; half-width overview and audio label alignment. No microphone capture.\n").unwrap();
}

fn project_assets_review(h:HWND,root:HWND,out:&Path){
 use crate::platform::{project_assets,media_decoder::MfBgraDecoder,export::{ProjectExporter,ExportRequest,ExportSnapshot,ExportOptions,ExportPreset}};
 use panzo_core::timeline::TrackKind;
 use windows::Win32::UI::WindowsAndMessaging::*;
 let s=unsafe{state_mut(h)}.unwrap();let fixture=s.editor.project_root().to_path_buf();
 let mut clean=s.editor.settings().clone();clean.audio_assets.clear();clean.audio_clips.clear();clean.subtitles.clear();clean.visual_assets.clear();clean.visual_clips.clear();clean.subtitle_assets.clear();clean.subtitle_links.clear();clean.clip_tracks.clear();clean.annotations.clear();clean.follow_positions.clear();clean.tracks=panzo_core::timeline::default_tracks();clean.source_video_enabled=true;clean.source_asset_removed=false;clean.audio.enabled=true;clean.audio.source_muted=true;
 let mut video=panzo_core::VideoEdit::full(s.editor.duration()).unwrap();let id=video.clips[0].id.clone();video.trim(&id,TimeTick::ZERO,TimeTick(10_000_000)).unwrap();clean.video=Some(video);s.editor.replace_media_settings(clean).unwrap();s.apply_editor_state().unwrap();s.theme.metrics.timeline_height=s.theme.scale(365);s.refresh(h,true);pump();
 let wav=out.join("music-3-seconds.wav");let frames=48000u32*3;let bytes=frames*4;let mut data=Vec::new();data.extend(b"RIFF");data.extend((36+bytes).to_le_bytes());data.extend(b"WAVEfmt ");data.extend(16u32.to_le_bytes());data.extend(1u16.to_le_bytes());data.extend(2u16.to_le_bytes());data.extend(48000u32.to_le_bytes());data.extend(192000u32.to_le_bytes());data.extend(4u16.to_le_bytes());data.extend(16u16.to_le_bytes());data.extend(b"data");data.extend(bytes.to_le_bytes());for i in 0..frames{let v=if i%240<120{9000i16}else{-9000i16};data.extend(v.to_le_bytes());data.extend(v.to_le_bytes());}std::fs::write(&wav,data).unwrap();
 project_bin::review_import_click(s.project_bin.as_ref().unwrap(),wav);let deadline=Instant::now()+Duration::from_secs(15);while s.editor.settings().audio_assets.is_empty()&&Instant::now()<deadline{pump();}
 assert_eq!(s.editor.settings().audio_assets.len(),1,"one mouse click imports exactly one asset");assert!(s.editor.settings().audio_clips.is_empty());assert_eq!(s.editor.project_duration(),TimeTick(10_000_000));
 let asset=s.editor.settings().audio_assets[0].clone();assert_eq!(asset.asset_frames,144000);let preview=project_bin::review_preview_asset(s.project_bin.as_ref().unwrap(),&asset.id);pump();assert!(unsafe{state_mut(preview)}.unwrap().session.is_asset_preview());snap(root,out,"asset-preview-before-drop");
 let c=s.control_layout(client_rect(h));let r=c.media_tracks[1];let mut point=POINT{x:r.left,y:(r.top+r.bottom)/2};unsafe{windows::Win32::Graphics::Gdi::ClientToScreen(h,&mut point).ok().unwrap();}drop_project_asset(h,&asset.id,point).unwrap();pump();assert_eq!(s.editor.project_duration(),TimeTick(30_000_000));assert_eq!(s.session.snapshot().duration_tick,TimeTick(30_000_000));assert!(remove_project_audio_asset(h,&asset.id).is_err());
 s.session.seek(TimeTick(20_000_000)).unwrap();let frame=s.session.render_current().unwrap();assert!(frame.composited.pixels.chunks_exact(4).all(|p|p[..3].iter().all(|v|*v<3)),"uncovered tail must be black");
 let mut mix=crate::platform::audio::AudioMixer::with_clips(&fixture,&s.editor.settings().mixing_clips()).unwrap();let samples=mix.render(0,480,TimeTick(20_000_000),&s.editor.video_edit(),s.editor.settings().audio).unwrap();assert!(samples.iter().any(|s|s.abs()>8000),"music must continue beyond video end");drop(mix);
 let output=out.join("black-tail-with-music.mp4");let mut exported=s.editor.settings().clone();exported.canvas.size=Some(panzo_core::CanvasSize{width:640,height:360});
 let report=ProjectExporter::export_snapshot_with_options(&ExportRequest{project_root:fixture.clone(),output_path:output.clone(),maximum_duration:None},&ExportSnapshot{camera:s.editor.camera().clone(),settings:exported},ExportPreset::Native,ExportOptions{fps:30,..Default::default()},&AtomicBool::new(false),|_,_|{}).unwrap();assert_eq!(report.duration_tick,TimeTick(30_000_000));assert_eq!(report.frames_written,90);
 let mut decoder=MfBgraDecoder::open(&output).unwrap();let mut tail=0;while let Some(f)=decoder.read_next().unwrap(){if f.pts>=TimeTick(20_000_000){assert!(f.pixels.chunks_exact(4).all(|p|p[..3].iter().all(|v|*v<8)));tail+=1;}}assert!(tail>=28);drop(decoder);
 let mut audio=crate::platform::audio_decode::AudioDecoder::open(&output).unwrap().unwrap();let mut peak=0i16;while let Some(p)=audio.read().unwrap(){if p.tick>=TimeTick(20_000_000){peak=peak.max(p.samples.iter().map(|v|v.abs()).max().unwrap_or(0));}}assert!(peak>5000);drop(audio);
 s.timeline.set_zoom_fraction(TimeTick::ZERO,0.0);s.refresh(h,true);snap(root,out,"music-extends-project-black-tail");
 let png=out.join("green-picture.png");image::RgbaImage::from_pixel(160,90,image::Rgba([20,220,40,255])).save(&png).unwrap();let job=project_assets::Job::start(fixture.clone(),png);let mut visual=loop{if let Ok(r)=job.result.try_recv(){break r.unwrap();}pump();};let project_assets::Imported::Visual(a)=visual.asset.take().unwrap() else{panic!("image import")};visual.commit();let image_id=a.id.clone();let mut next=s.editor.settings().clone();next.visual_assets.push(a);let track=next.add_track(TrackKind::Video).unwrap();let image_clip=next.insert_visual_asset(&image_id,&track,TimeTick(20_000_000)).unwrap();next.visual_clips.last_mut().unwrap().source_out=TimeTick(10_000_000);s.editor.replace_media_settings(next).unwrap();s.apply_editor_state().unwrap();s.media_selected=Some((false,image_clip.clone()));s.select_video_properties();s.refresh(h,true);pump();assert!(inspector_panel(h).is_none());assert_eq!(s.selected_video.as_deref(),Some(image_clip.as_str()));
 let until=Instant::now()+Duration::from_secs(5);let mut green=false;while Instant::now()<until{pump();let f=s.session.render_current().unwrap();let i=((f.composited.height/2*f.composited.width+f.composited.width/2)*4)as usize;if f.composited.pixels[i+1]>180{green=true;break;}}assert!(green,"placed image must replace black tail asynchronously");snap(root,out,"placed-image-video-properties");
 let crop_before=s.editor.settings().clone();s.crop_locked=false;s.apply_widget_range(h,ControlId::CropLeft,10.0);assert_eq!(s.selected_video.as_deref(),Some(image_clip.as_str()));assert_eq!(s.widget_value(ControlId::CropLeft),Some(10.));s.toggle_crop_editing();let overlay=s.crop_overlay(&s.frame).unwrap();assert!(overlay[2]-overlay[0]<0.5,"imported small image crop overlay must use its own geometry");s.toggle_crop_editing();s.undo();assert_eq!(s.editor.settings(),&crop_before);s.refresh(h,true);

 assert!(remove_project_audio_asset(h,&image_id).is_err());let before=s.editor.settings().clone();let mut next=before.clone();next.remove_track(&track).unwrap();s.editor.replace_media_settings(next).unwrap();s.apply_editor_state().unwrap();remove_project_audio_asset(h,&image_id).unwrap();s.undo();s.undo();assert_eq!(s.editor.settings(),&before);
 select(h,ControlId::AudioPanel);let panel=inspector_panel(h).unwrap();let body=media_panel::review_body(panel);assert!(!unsafe{IsWindowVisible(GetDlgItem(Some(body),74).unwrap())}.as_bool());assert!(!unsafe{IsWindowVisible(GetDlgItem(Some(body),30).unwrap())}.as_bool());
 let count=s.editor.settings().audio_clips.len();let count_assets=s.editor.settings().audio_assets.len();let(_,tx)=media_panel::review_pending_job(panel);let mut recorded=s.editor.settings().audio_clips[0].clone();recorded.id=uuid::Uuid::new_v4().to_string();recorded.path=format!("media/audio-clips/{}.pcm",recorded.id);recorded.name="recorded asset".into();std::fs::copy(fixture.join(&asset.path),fixture.join(&recorded.path)).unwrap();tx.send(crate::platform::media_jobs::Event::Done(Ok(crate::platform::media_jobs::ResultData::Audio(recorded)))).unwrap();unsafe{SendMessageW(panel,WM_TIMER,Some(WPARAM(1)),None);}assert_eq!(s.editor.settings().audio_clips.len(),count);assert_eq!(s.editor.settings().audio_assets.len(),count_assets+1);assert!(s.recording_preview.is_none());

 // Cross-track drags are preview-only until release, snap to the playhead, and undo once.
 let before_drag=s.editor.settings().clone();let mut next=before_drag.clone();let music_track=next.add_track(TrackKind::Music).unwrap();s.editor.replace_media_settings(next).unwrap();s.apply_editor_state().unwrap();s.timeline.set_zoom_fraction(TimeTick::ZERO,0.0);s.track_scroll=0;s.session.seek(TimeTick(5_000_000)).unwrap();s.refresh(h,true);
 let c=s.control_layout(client_rect(h));let r=c.media_tracks[1];let x=timeline_x(r,TimeTick(10_000_000),s.timeline);let y=(r.top+r.bottom)/2;assert!(s.media_down(h,x,y));let original=s.editor.settings().clone();s.track_scroll=c.track_scroll_max;let c=s.control_layout(client_rect(h));let row=(0..35).find(|i|s.row_track(*i).is_some_and(|t|t.id==music_track)).unwrap();let target=c.media_tracks[row];let target_y=(target.top+target.bottom)/2;let delta=s.timeline.tick_to_offset(TimeTick(5_000_000),c.ruler.right-c.ruler.left)-s.timeline.tick_to_offset(TimeTick::ZERO,c.ruler.right-c.ruler.left);
 s.media_move(h,x+delta+2,target_y);assert_eq!(s.editor.settings(),&original,"drag must not rebuild editor state");assert_eq!(s.snap_guide,Some(TimeTick(5_000_000)));s.media_up(h,x+delta+2,target_y);assert_eq!(s.editor.settings().audio_clips[0].start,TimeTick(5_000_000));assert_eq!(s.editor.settings().audio_track(&s.editor.settings().audio_clips[0]),music_track);snap(root,out,"cross-track-snapped-audio");s.undo();assert_eq!(s.editor.settings(),&original);
 // A drop on an incompatible track cancels without altering anything.
 s.track_scroll=0;let c=s.control_layout(client_rect(h));let r=c.media_tracks[1];let x=timeline_x(r,TimeTick(10_000_000),s.timeline);let y=(r.top+r.bottom)/2;let before=s.editor.settings().clone();assert!(s.media_down(h,x,y));let bad=c.media_tracks[2];s.media_move(h,x+30,(bad.top+bad.bottom)/2);s.media_up(h,x+30,(bad.top+bad.bottom)/2);assert_eq!(s.editor.settings(),&before);
 assert!(s.media_down(h,x,y));s.media_move(h,x+40,y);s.cancel_gesture(h);assert_eq!(s.editor.settings(),&before);
 // Existing source video moves freely too; the gap is black and its source mapping survives.
 let before=s.editor.settings().clone();s.track_scroll=0;let c=s.control_layout(client_rect(h));let r=c.video_track;let x=timeline_x(r,TimeTick(2_500_000),s.timeline);let y=(r.top+r.bottom)/2;let source_id=s.editor.video_edit().clips[0].id.clone();s.begin_source_move(h,x,y,source_id.clone(),TimeTick::ZERO,TimeTick(10_000_000));s.track_scroll=c.track_scroll_max;let c=s.control_layout(client_rect(h));let row=(0..35).find(|i|s.row_track(*i).is_some_and(|t|t.id==track)).unwrap();let r=c.media_tracks[row];let target_y=(r.top+r.bottom)/2;let delta=s.timeline.tick_to_offset(TimeTick(30_000_000),c.ruler.right-c.ruler.left)-s.timeline.tick_to_offset(TimeTick::ZERO,c.ruler.right-c.ruler.left);s.media_move(h,x+delta,target_y);s.media_up(h,x+delta,target_y);
 assert!(!s.editor.settings().source_video_enabled);let moved=s.editor.settings().visual_clips.iter().find(|v|v.id==source_id).unwrap();assert!((moved.start.0-30_000_000).abs()<50000);assert_eq!(moved.track_id,track);let start=moved.start;
 s.session.seek(TimeTick(5_000_000)).unwrap();let gap=s.session.render_current().unwrap();assert!(gap.composited.pixels.chunks_exact(4).all(|p|p[..3].iter().all(|v|*v<3)));
 s.seek_absolute(TimeTick(start.0+5_000_000));pump();let until=Instant::now()+Duration::from_secs(6);let mut visible=false;while Instant::now()<until{pump();let f=s.session.render_current().unwrap();if f.composited.pixels.chunks_exact(4).any(|p|p[0]>100&&p[1]>50){visible=true;break;}}assert!(visible,"moved original source must render");assert_eq!(s.editor.settings().source_time_at(s.editor.duration(),TimeTick(start.0+5_000_000)),Some(TimeTick(5_000_000)));snap(root,out,"original-video-free-move");
 let output=out.join("freely-moved-source.mp4");let mut settings=s.editor.settings().clone();settings.canvas.size=Some(panzo_core::CanvasSize{width:640,height:360});let report=ProjectExporter::export_snapshot_with_options(&ExportRequest{project_root:fixture.clone(),output_path:output.clone(),maximum_duration:None},&ExportSnapshot{camera:s.editor.camera().clone(),settings},ExportPreset::Native,ExportOptions{fps:30,..Default::default()},&AtomicBool::new(false),|_,_|{}).unwrap();assert!(report.duration_tick>=TimeTick(39_900_000));let mut decoder=MfBgraDecoder::open(&output).unwrap();let(mut gap_frames,mut moved_frames)=(0,0);while let Some(f)=decoder.read_next().unwrap(){if f.pts>=TimeTick(2_000_000)&&f.pts<TimeTick(8_000_000){assert!(f.pixels.chunks_exact(4).all(|p|p[..3].iter().all(|v|*v<8)));gap_frames+=1;}if f.pts>=TimeTick(start.0+2_000_000)&&f.pts<TimeTick(start.0+8_000_000){assert!(f.pixels.chunks_exact(4).any(|p|p[0]>90&&p[1]>40));moved_frames+=1;}}assert!(gap_frames>=15&&moved_frames>=15);drop(decoder);s.undo();assert_eq!(s.editor.settings(),&before);
 // Follow clips also have independent project-time positions and compatible tracks.
 let before=s.editor.settings().clone();let mut next=before.clone();let follow_track=next.add_track(TrackKind::Follow).unwrap();s.editor.replace_media_settings(next).unwrap();s.apply_editor_state().unwrap();s.track_scroll=0;s.refresh(h,true);let c=s.control_layout(client_rect(h));let segment=s.display_segments()[0].clone();let x=timeline_x(c.camera_track,TimeTick((segment.start_tick.0+segment.end_tick.0)/2),s.timeline);let y=(c.camera_track.top+c.camera_track.bottom)/2;assert!(s.media_down(h,x,y));s.track_scroll=c.track_scroll_max;let c=s.control_layout(client_rect(h));let row=(0..35).find(|i|s.row_track(*i).is_some_and(|t|t.id==follow_track)).unwrap();let r=c.media_tracks[row];let y=(r.top+r.bottom)/2;let delta=s.timeline.tick_to_offset(TimeTick(40_000_000),c.ruler.right-c.ruler.left);s.media_move(h,x+delta,y);s.media_up(h,x+delta,y);assert_eq!(s.editor.settings().clip_tracks.get(&segment.id),Some(&follow_track));assert!(s.editor.settings().follow_positions.get(&segment.id).unwrap().0>=TimeTick(39_000_000));s.undo();s.undo();assert_eq!(s.editor.settings(),&before);
 // Subtitle import is a reusable source asset until placed on a subtitle track.
 let srt=out.join("subtitle.srt");std::fs::write(&srt,"1\n00:00:00,000 --> 00:00:01,000\nSubtitle source\n").unwrap();let job=project_assets::Job::start(fixture.clone(),srt);let mut imported=loop{if let Ok(r)=job.result.try_recv(){break r.unwrap();}pump();};let project_assets::Imported::Subtitle(sub)=imported.asset.take().unwrap() else{panic!("subtitle import")};let mut next=s.editor.settings().clone();imported.commit();let sid=sub.id.clone();next.subtitle_assets.push(sub);s.editor.replace_media_settings(next).unwrap();s.apply_editor_state().unwrap();assert!(s.editor.settings().subtitles.is_empty());let mut next=s.editor.settings().clone();next.insert_subtitle_asset(&sid,"subtitle",TimeTick(10_000_000)).unwrap();s.editor.replace_media_settings(next).unwrap();s.apply_editor_state().unwrap();assert!(remove_project_audio_asset(h,&sid).is_err());
 s.editor.write_recovery_draft().unwrap();let mut restored=crate::platform::editor::ProjectEditorSession::open(&fixture).unwrap();assert!(restored.restore_recovery_draft().unwrap());assert_eq!(restored.settings(),s.editor.settings());s.editor.clear_recovery_draft().unwrap();s.editor.save(CameraEditKind::Update,None).unwrap();let restored=crate::platform::editor::ProjectEditorSession::open(&fixture).unwrap();assert_eq!(restored.settings(),s.editor.settings());
 snap(root,out,"recording-assets-and-tracks");std::fs::write(out.join("project-assets-review.txt"),"PASS: one-click import enters bin only; independent PCM preview; typed drop extends project; black tail retains music in preview and exported MP4; actual decoded tail frame/audio verified; image worker renders placed asset; selected video properties; referenced asset deletion blocked; track deletion releases references; undo restores; recording enters bin only; removed import/role controls hidden; save/reopen and recovery retain tracks/assets. Cross-track moves, snapped playhead, cancel and invalid track rejection, free source move with decoded export gap/source verification, independent follow movement, SRT bin import. Synthetic audio; no microphone capture.\n").unwrap();
}

fn reliability_review(h:HWND,root:HWND,out:&Path) {
    use windows::Win32::UI::WindowsAndMessaging::*;
    let s=unsafe{state_mut(h)}.unwrap();
    s.select_preview_source(false).unwrap();s.refresh(h,true);
    for (w,height) in [(640,360),(360,640),(640,640),(1280,720)] {
        let mut next=s.editor.settings().clone();next.canvas.size=Some(panzo_core::CanvasSize{width:w,height});
        s.editor.replace_media_settings(next).unwrap();s.apply_editor_state().unwrap();s.refresh(h,true);pump();
        assert!(unsafe{IsWindowVisible(s.preview_surface.as_ref().unwrap().hwnd)}.as_bool(),"resized GPU child must be visible");
        assert_eq!((s.frame.evaluation.output.width,s.frame.evaluation.output.height),(w,height));
    }
    snap(root,out,"resized-video-preview");
    let asset=s.editor.settings().audio_assets[0].id.clone();project_bin::review_preview_asset(s.project_bin.as_ref().unwrap(),&asset);pump();
    let mut next=s.editor.settings().clone();next.canvas.size=Some(panzo_core::CanvasSize{width:640,height:720});s.editor.replace_media_settings(next).unwrap();s.apply_editor_state().unwrap();s.refresh(h,true);pump();
    assert!(!project_bin::source_mode(s.project_bin.as_ref().unwrap()));assert!(unsafe{IsWindowVisible(s.preview_surface.as_ref().unwrap().hwnd)}.as_bool(),"both preview modes share the GPU child");
    s.select_preview_source(false).unwrap();s.refresh(h,true);

    // A project-time cue may extend the source; adding and editing stays immediate.
    let tick=TimeTick(s.editor.duration().0+10_000_000);let mut next=s.editor.settings().clone();
    next.subtitles.push(panzo_core::media_edit::SubtitleCue{id:"duration-extension".into(),start:tick,end:TimeTick(tick.0+50_000_000),text:"extended".into(),anchor:panzo_core::media_edit::TimeAnchor::Project});
    s.editor.replace_media_settings(next).unwrap();s.apply_editor_state().unwrap();s.seek_absolute(tick);pump();
    select(h,ControlId::SubtitleTab);let panel=inspector_panel(h).unwrap();unsafe{SendMessageW(panel,WM_COMMAND,Some(WPARAM(31)),None);}pump();
    let body=media_panel::review_body(panel);let input=unsafe{GetDlgItem(Some(body),140)}.unwrap();let id=s.editor.settings().subtitles.last().unwrap().id.clone();
    assert!(s.editor.settings().subtitles.last().unwrap().start>=tick);
    let type_text=|text:&str|{let text=ui::wide(text);unsafe{SendMessageW(input,0xb1,Some(WPARAM(0)),Some(LPARAM(-1)));SendMessageW(input,0xc2,Some(WPARAM(1)),Some(LPARAM(text.as_ptr() as isize)));}};
    type_text("即时字幕，无需应用");assert_eq!(s.editor.settings().subtitles.iter().find(|c|c.id==id).unwrap().text,"即时字幕，无需应用");
    type_text("");assert!(s.editor.settings().subtitles.iter().find(|c|c.id==id).unwrap().text.is_empty());
    type_text("字幕编辑测试");unsafe{SendMessageW(input,0xb1,Some(WPARAM(2)),Some(LPARAM(2)));}pump();
    let selection=unsafe{SendMessageW(input,0xb0,None,None)}.0;assert_eq!(selection & 65535,2,"timer must preserve text caret");
    let size=unsafe{GetDlgItem(Some(body),122)}.unwrap();ui::text(size,"8");unsafe{SendMessageW(panel,WM_COMMAND,Some(WPARAM(122 | ((EN_CHANGE as usize)<<16))),Some(LPARAM(size.0 as isize)));}
    assert!((s.editor.settings().subtitle_style.font_size*720.-8.).abs()<0.01);
    assert!(!unsafe{IsWindowVisible(GetDlgItem(Some(panel),50).unwrap())}.as_bool(),"subtitle apply must be hidden");
    snap(root,out,"live-subtitles");
    let (_,job)=media_panel::review_pending_job(panel);let before=s.editor.settings().subtitles.clone();let mut next=s.editor.settings().clone();next.tracks[0].muted=!next.tracks[0].muted;s.editor.replace_media_settings(next).unwrap();s.apply_editor_state().unwrap();
    job.send(crate::platform::media_jobs::Event::Done(Ok(crate::platform::media_jobs::ResultData::Subtitles(vec![panzo_core::media_edit::SubtitleCue{id:"stale".into(),start:TimeTick::ZERO,end:TimeTick(10_000_000),text:"stale result".into(),anchor:panzo_core::media_edit::TimeAnchor::Project}],false)))).unwrap();
    unsafe{SendMessageW(panel,WM_TIMER,Some(WPARAM(1)),None);}assert_eq!(s.editor.settings().subtitles,before,"track edits must reject stale AI results");
    assert!(std::fs::read_dir(s.editor.project_root().join("media")).unwrap().filter_map(Result::ok).any(|e|e.file_name().to_string_lossy().starts_with("generated-")));
    s.add_manual_segment();assert!(s.selected_segment.as_ref().is_some_and(|id|s.editor.settings().follow_positions.get(id).is_some_and(|r|r.0==tick)));
    s.editor.save(CameraEditKind::Update,None).unwrap();
    std::fs::write(out.join("reliability-review.txt"),"PASS: resized GPU preview remains visible across landscape/portrait/square; source/video tabs share central viewport; subtitles add beyond original source; empty and nonempty text update immediately; timer preserves caret; 8 px font accepted; no Apply button; track edit rejects stale AI result and preserves SRT; follow adds beyond original source. No external AI service or real microphone used.\n").unwrap();
}

/// Use the button's native mouse path, not direct activate_control / BM_CLICK.
fn click_control(h: HWND, id: ControlId) {
    let r=unsafe{state_mut(h)}.unwrap().control_layout(client_rect(h)).control_rect(id).0;
    assert!(r.right>r.left && r.bottom>r.top,"visible control {id:?}");
    let x=(r.left+r.right)/2;let y=(r.top+r.bottom)/2;
    let point=LPARAM((x as u16 as usize | ((y as u16 as usize)<<16)) as isize);
    unsafe {SendMessageW(h,WM_LBUTTONDOWN,Some(WPARAM(1)),Some(point));}
    pump(); // Allow focus-loss commits and layout work before releasing the mouse.
    unsafe {SendMessageW(h,WM_LBUTTONUP,Some(WPARAM(0)),Some(point));}
    pump();
}

fn interaction_crop_review(h:HWND,root:HWND,out:&Path) {
    project_bin::review_first_click(unsafe{state_mut(h)}.unwrap().project_bin.as_ref().unwrap());
    // Existing inline values commit without eating the first click on another tab.
    click_control(h,ControlId::CanvasTab);
    {let s=unsafe{state_mut(h)}.unwrap();s.begin_inline_input(h,ControlId::Inset);let input=s.inline_input.as_ref().unwrap().hwnd;ui::text(input,"25");}
    click_control(h,ControlId::VideoTab);
    assert!(unsafe{state_mut(h)}.unwrap().inspector_video_tab);
    assert!(unsafe{state_mut(h)}.unwrap().inline_input.is_none());
    // Test a native child from inline numeric input as well as root/search/button focus.
    project_bin::review_first_click(unsafe{state_mut(h)}.unwrap().project_bin.as_ref().unwrap());
    {
        let s=unsafe{state_mut(h)}.unwrap();
        s.theme.metrics.timeline_height=s.theme.scale(170);
        s.inspector_scroll=0;s.video_collapsed=[true,false];s.refresh(h,true);
    }
    snap(root,out,"follow-no-overlap");
    // Free starts crop editing immediately and switches source preview back to video.
    {let s=unsafe{state_mut(h)}.unwrap();project_bin::select_preview(s.project_bin.as_ref().unwrap(),true);s.refresh(h,true);}
    click_control(h,ControlId::CropLock);
    {let s=unsafe{state_mut(h)}.unwrap();assert!(s.crop_editing);assert!(!s.crop_locked);assert!(!project_bin::source_mode(s.project_bin.as_ref().unwrap()));assert!(s.crop_overlay(&s.frame).is_some());}
    snap(root,out,"free-crop-overlay");
    crop_drag_review(h);
    let retained=unsafe{state_mut(h)}.unwrap().selected_crop().unwrap();
    click_control(h,ControlId::CropEdit);assert!(!unsafe{state_mut(h)}.unwrap().crop_editing);
    click_control(h,ControlId::CropLock);assert_eq!(unsafe{state_mut(h)}.unwrap().selected_crop(),Some(retained));
    // Ratio mode still allows movement/resize and remains selected after translation.
    click_control(h,ControlId::CropSquare);
    {let s=unsafe{state_mut(h)}.unwrap();assert!(s.crop_locked&&s.crop_editing);}
    crop_drag_review(h);
    snap(root,out,"locked-crop-moved");
    click_control(h,ControlId::CropReset);
    assert_eq!(unsafe{state_mut(h)}.unwrap().selected_crop(),Some(panzo_core::VideoCrop::default()));
    click_control(h,ControlId::CropEdit);
    // GPU child and pointer geometry stay aligned after preview zoom and project aspect changes.
    select(h,ControlId::CanvasTab);select(h,ControlId::CanvasWide);select(h,ControlId::VideoTab);
    click_control(h,ControlId::CropLock);
    {let s=unsafe{state_mut(h)}.unwrap();
        s.set_selected_crop(panzo_core::VideoCrop{left:200,top:200,right:200,bottom:200}).unwrap();
        s.apply_editor_state().unwrap();s.preview_view.zoom=1.35;s.refresh_preview_view(h);}
    pump();
    crop_drag_review(h);
    snap(root,out,"zoomed-free-crop");
    // Imported clips use their own source size and transform, including locked ratios.
    let imported=unsafe{state_mut(h)}.unwrap().editor.settings().visual_clips.iter().find(|c|c.asset_id!="__source__").map(|c|c.id.clone()).expect("fixture imported clip");
    {let s=unsafe{state_mut(h)}.unwrap();s.set_crop_editing(false);s.preview_view=preview_view::PreviewView::default();
        s.media_selected=Some((false,imported.clone()));s.select_video_properties();s.inspector_scroll=0;s.refresh(h,true);}
    click_control(h,ControlId::CropSquare);
    {let s=unsafe{state_mut(h)}.unwrap();assert_eq!(s.selected_video.as_deref(),Some(imported.as_str()));
        let c=s.selected_crop().unwrap();let (w,v)=s.selected_video_size();assert!((f64::from(w)*c.width()/(f64::from(v)*c.height())-1.).abs()<0.015);}
    snap(root,out,"imported-locked-crop");
    click_control(h,ControlId::CropEdit);
    // The actual file dialog entry is also tested without intercepting BN_CLICKED.
    let file=out.join("single-click-import.png");image::RgbaImage::from_pixel(40,30,image::Rgba([20,160,80,255])).save(&file).unwrap();
    {let s=unsafe{state_mut(h)}.unwrap();project_bin::review_import_click(s.project_bin.as_ref().unwrap(),file);}
    for _ in 0..20 {pump();if !project_bin::busy(unsafe{state_mut(h)}.unwrap().project_bin.as_ref().unwrap()){break;}}
    assert!(unsafe{state_mut(h)}.unwrap().editor.settings().visual_assets.iter().any(|a|a.name.contains("single-click-import")));
    std::fs::write(out.join("interaction-review.txt"),"PASS: first native click from canvas/search/button focus issues exactly one command; capture retained; inline edit commits and first tab click executes; one-click import enters project; free crop opens video preview; edge resize/interior move/undo/re-entry/reset/locked ratio/zoom/changed canvas; drag does not refresh project bin. No microphone or network.\n").unwrap();
}

fn crop_drag_review(h:HWND) {
    let s=unsafe{state_mut(h)}.unwrap();
    let before=s.selected_crop().unwrap();
    let overlay=s.crop_overlay(&s.frame).unwrap();let video=s.preview_rect(client_rect(h));
    let px=|u:f64| video.left+(f64::from(video.right-video.left)*u).round() as i32;
    let py=|v:f64| video.top+(f64::from(video.bottom-video.top)*v).round() as i32;
    // Drag the upper-left handle inward. Overlay coordinates and hit coordinates must agree.
    let x=px(overlay[0]);let y=py(overlay[1]);
    assert_eq!(s.crop_hit(client_rect(h),x,y),Some((-1,-1)),"overlay={overlay:?} video={video:?} point={x},{y} well={:?} crop={before:?}",s.control_layout(client_rect(h)).preview_well);
    let bin_revision=project_bin::review_revision(s.project_bin.as_ref().unwrap());
    s.on_mouse_down(h,x,y);assert!(s.crop_drag.is_some(),"preview hit must start crop, not pan");
    s.on_mouse_move(h,x+12,y+10);
    assert_eq!(project_bin::review_revision(s.project_bin.as_ref().unwrap()),bin_revision,"drag must not rebuild the project bin");
    assert_ne!(s.selected_crop(),Some(before));
    s.finish_crop_drag(h,false);
    let resized=s.selected_crop().unwrap();
    let overlay=s.crop_overlay(&s.frame).unwrap();
    let x=px((overlay[0]+overlay[2])/2.);let y=py((overlay[1]+overlay[3])/2.);
    assert_eq!(s.crop_hit(client_rect(h),x,y),Some((0,0)));
    assert!(s.begin_crop_drag(h,x,y));s.update_crop_drag(h,x-5,y-5);s.finish_crop_drag(h,false);
    let moved=s.selected_crop().unwrap();assert_ne!(moved,resized);
    assert!((moved.width()-resized.width()).abs()<0.003);
    assert!((moved.height()-resized.height()).abs()<0.003);
    s.undo();assert_eq!(s.selected_crop(),Some(resized));
    s.undo();assert_eq!(s.selected_crop(),Some(before));
    // Keep a cropped result for exit/re-entry checks.
    let overlay=s.crop_overlay(&s.frame).unwrap();
    assert!(s.begin_crop_drag(h,px(overlay[0]),py(overlay[1])));
    s.update_crop_drag(h,px(overlay[0])+8,py(overlay[1])+8);s.finish_crop_drag(h,false);
}

fn inspector_settings_review(h:HWND,root:HWND,out:&Path) {
    // Background loading uses the real asynchronous task and normal poll/apply path.
    let background=out.join("background-fixture.png");
    image::RgbaImage::from_pixel(640,360,image::Rgba([30,100,170,255])).save(&background).unwrap();
    select(h,ControlId::CanvasTab);
    {let s=unsafe{state_mut(h)}.unwrap();
        s.editor.set_canvas_size(Some(panzo_core::CanvasSize{width:1280,height:720})).unwrap();
        s.editor.set_canvas_inset(50.0/720.0).unwrap();s.session.seek(TimeTick::ZERO).unwrap();
        s.apply_editor_state().unwrap();s.canvas_advanced=false;
        s.begin_inline_input(h,ControlId::Inset);
        assert_eq!(s.inline_input.as_ref().unwrap().value(),"50");s.finish_inline_input(h,false);
        s.background_task=Some(crate::platform::editor_tasks::BackgroundTask::start(s.editor.background_snapshot(),Some(background)).unwrap());
    }
    for _ in 0..30 {pump();if unsafe{state_mut(h)}.unwrap().background_task.is_none(){break;}}
    {let s=unsafe{state_mut(h)}.unwrap();
        assert!(s.background_task.is_none());assert!(s.session.background_preview().is_some());
        assert_eq!(s.editor.settings().background.kind,panzo_core::BackgroundKind::Image);
        let c=s.control_layout(client_rect(h));assert!(c.background_file.bottom<c.inset.top);assert!(c.background_image.bottom<c.background_file.top);assert_eq!(c.radius.0,RECT::default());
        let pixels=s.session.compose_prepared(&s.frame).unwrap().composited.pixels;
        assert!(pixels[0].abs_diff(170)<2&&pixels[1].abs_diff(100)<2&&pixels[2].abs_diff(30)<2,"image background must reach GPU output");
    }
    snap(root,out,"image-background-in-background-group");
    click_control(h,ControlId::Background);pump();
    {let s=unsafe{state_mut(h)}.unwrap();assert_eq!(s.editor.settings().background.kind,panzo_core::BackgroundKind::Solid);assert!(s.session.background_preview().is_none());}
    snap(root,out,"solid-background-segments");
    click_control(h,ControlId::BackgroundImage);pump();
    {let s=unsafe{state_mut(h)}.unwrap();assert_eq!(s.editor.settings().background.kind,panzo_core::BackgroundKind::Image);assert!(s.session.background_preview().is_some());s.undo();}
    pump();assert_eq!(unsafe{state_mut(h)}.unwrap().editor.settings().background.kind,panzo_core::BackgroundKind::Solid);
    unsafe{state_mut(h)}.unwrap().redo();pump();
    click_control(h,ControlId::VideoTab);
    {let s=unsafe{state_mut(h)}.unwrap();
        let c=s.control_layout(client_rect(h));assert_eq!(c.selected_info,RECT::default());assert_eq!(c.start_value.0,RECT::default());assert_eq!(c.end_value.0,RECT::default());
        s.begin_inline_input(h,ControlId::VideoScale);assert_eq!(s.inline_input.as_ref().unwrap().value(),"100.0");
        ui::text(s.inline_input.as_ref().unwrap().hwnd,"125.5");assert!(s.finish_inline_input(h,true));
        assert_eq!(s.widget_value(ControlId::VideoScale),Some(125.5));
        assert!((s.frame.evaluation.camera_transform.content_scale-1.255).abs()<1e-9);
        s.undo();assert_eq!(s.widget_value(ControlId::VideoScale),Some(100.0));s.redo();
        s.apply_widget_range(h,ControlId::VideoScale,75.0);
        let r=s.control_layout(client_rect(h)).video_scale;
        assert!(s.begin_property_drag(h,ControlId::VideoScale,r.left+(r.right-r.left)/2,r.bottom-s.theme.scale(8)));
        s.on_mouse_up(h,r.left+(r.right-r.left)/2,r.bottom-s.theme.scale(8));
        assert!(s.widget_value(ControlId::VideoScale).unwrap()>190.0);
        s.undo();assert_eq!(s.widget_value(ControlId::VideoScale),Some(75.0));
    }
    snap(root,out,"video-speed-scale-segments");
    click_control(h,ControlId::CropLock);crop_drag_review(h);snap(root,out,"scaled-crop-overlay");
    click_control(h,ControlId::CropEdit);
    {let s=unsafe{state_mut(h)}.unwrap();s.editor.save(CameraEditKind::Update,None).unwrap();
        let reopened=crate::platform::editor::ProjectEditorSession::open(s.project_root()).unwrap();
        assert_eq!(reopened.settings().background,s.editor.settings().background);assert_eq!(reopened.video_edit(),s.editor.video_edit());
    }
    ui_preferences::set_theme(ThemeMode::Light).unwrap();pump();snap(root,out,"video-light");
    std::fs::write(out.join("inspector-review.txt"),"PASS: async image background appears in GPU output; solid/image switch on first click, undo/redo and save; image picker independent of advanced group; removed hidden trim hit areas; integer px input; decimal clip scale input and drag, undo, saved scale and scaled crop handles; light/dark screenshots. Isolated fixture only.\n").unwrap();
}

#[path="workbench_polish_review.rs"]
mod media_polish_review;

/// Exercise shared hover/capture cancellation and title placement in real panes.
fn splitter_component_review(h:HWND,root:HWND,out:&Path) {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture,SetCapture};
    let s=unsafe{state_mut(h)}.unwrap();
    let revision=s.editor.change_revision();
    for (id,splitter) in s.control_layout(client_rect(h)).splitters() {
        let r=splitter.bounds;let x=(r.left+r.right)/2;let y=(r.top+r.bottom)/2;
        s.on_mouse_move(h,x,y);
        assert_eq!(s.splitter_hover,Some(id));
        assert_eq!(s.cursor_at(client_rect(h),x,y),splitter.cursor());
        let before=s.control_layout(client_rect(h));
        s.on_mouse_down(h,x,y);
        assert_eq!(unsafe{GetCapture()},h);
        s.on_mouse_move(h,x+40,y-40);s.flush_panel_layout(h);
        // Another native control takes capture: restore size and clear drag.
        unsafe{SetCapture(root);}
        assert!(!s.panel_resizing());
        assert_eq!(s.control_layout(client_rect(h)).project_bin,before.project_bin);
        assert_eq!(s.control_layout(client_rect(h)).panel,before.panel);
        assert_eq!(s.control_layout(client_rect(h)).timeline_panel,before.timeline_panel);
        crate::platform::desktop_ui::release_capture_owned_by(root);
    }
    s.on_mouse_move(h,-1,-1);assert!(s.splitter_hover.is_none());
    assert_eq!(s.editor.change_revision(),revision);
    for mode in [ThemeMode::Dark,ThemeMode::Light] {
        ui_preferences::set_theme(mode).unwrap();
        for tab in [ControlId::CanvasTab,ControlId::AudioTab,ControlId::SubtitleTab] {
            select(h,tab);
            let s=unsafe{state_mut(h)}.unwrap();let c=s.control_layout(client_rect(h));
            let a=crate::platform::workbench_layout::panel_title(c.project_bin,s.theme);
            let b=crate::platform::workbench_layout::panel_title(c.panel,s.theme);
            assert_eq!((a.top,a.bottom),(b.top,b.bottom));
            assert_eq!((c.project_bin.top,c.project_bin.bottom),(c.panel.top,c.panel.bottom));
            assert!(b.bottom<c.inspector_tabs[0].top);
            if let Some(panel)=inspector_panel(h) {
                let mut actual=RECT::default();
                unsafe {GetWindowRect(panel,&mut actual).unwrap();}
                let mut origin=POINT{x:actual.left,y:actual.top};
                unsafe {let _=windows::Win32::Graphics::Gdi::ScreenToClient(h,&mut origin);}
                assert_eq!(origin.y,c.inspector_body_top);
            }
            snap(root,out,&format!("aligned-{mode:?}-{tab:?}"));
        }
    }
    ui_preferences::set_theme(ThemeMode::Dark).unwrap();select(h,ControlId::VideoTab);
    std::fs::write(out.join("splitter-component.txt"),"PASS: all divider hover/cursors/capture-loss cancellation; aligned titles, tabs and native body boundaries in canvas/audio/subtitle pages; dark/light.\n").unwrap();
}
