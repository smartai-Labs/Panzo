//! Current-project media browser. One owned child, no second project model.
use crate::platform::{
    asset_index::{Asset, Kind},
    capture_tools::{self, Raster},
    desktop_ui as desk,
    editor_ui::{self, IconKind as I, TextStyle},
    workspace_ui as ui,
};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::*,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};
pub(super) struct ProjectBin {
    pub(super) hwnd: HWND,
}
impl Drop for ProjectBin {
    fn drop(&mut self) {
        unsafe {
            if let Some(s)=state(self.hwnd) { let _=DestroyWindow(s.tabs);let _=DestroyWindow(s.quality_host); }
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
#[derive(Clone, PartialEq)]
struct Item {
    id: Option<String>,
    name: String,
    detail: String,
    waveform: Vec<f32>,
    visual:bool,
    subtitle:bool,
}
struct State {
    owner: HWND,
    quality_host: HWND,
    quality: HWND,
    tabs: HWND,
    tab_buttons: [HWND;2],
    source_mode: bool,
    timeline_frames: Option<crate::platform::timeline_visual::Visuals>,
    visual_assets: Vec<panzo_core::timeline::VisualAsset>,
    loaded: Option<String>,
    root: PathBuf,
    source_item:Option<Item>,
    thumbs:std::collections::HashMap<String,Raster>,
    thumb_job:Option<Receiver<(String,Result<Raster,String>)>>,
    import: Option<crate::platform::project_assets::Job>,
    drag: Option<(String,i32,i32,bool)>,
    drag_card: Option<crate::platform::workbench_overlays::Overlay>,
    search: HWND,
    list: HWND,
    buttons: [HWND; 3],
    more: HWND,
    items: Vec<Item>,
    visible: Vec<usize>,
    revision: Option<u64>,
    hovered: Option<usize>,
    image: Option<Raster>,
    image_job: Option<Receiver<Result<Raster, String>>>,
}
unsafe fn state(h: HWND) -> Option<&'static mut State> {
    unsafe { (GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State).as_mut() }
}
pub(super) fn create(owner: HWND) -> Result<ProjectBin, String> {
    ui::register(w!("PanzoProjectBin"), Some(proc))?;
    let h = ui::panel(owner, w!("PanzoProjectBin"))?;
    desk::install_canvas(h);
    let search = ui::edit(h, "", 1, false)?;
    let cue = ui::wide("搜索当前项目");
    unsafe {
        SendMessageW(
            search,
            windows::Win32::UI::Controls::EM_SETCUEBANNER,
            Some(WPARAM(1)),
            Some(LPARAM(cue.as_ptr() as isize)),
        );
    }
    let list = ui::child(
        h,
        w!("LISTBOX"),
        "项目素材",
        2,
        WS_TABSTOP
            | WS_VSCROLL
            | WINDOW_STYLE(
                (LBS_NOTIFY | LBS_NOINTEGRALHEIGHT | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS) as u32,
            ),
    )?;
    desk::list(list);
    unsafe {
        let _ = windows::Win32::UI::Shell::SetWindowSubclass(list, Some(list_hover), 0x50415a80, 0);
    }
    let buttons = [
        ui::button(h, "导入素材", 3)?,
        ui::button(h, "录制", 4)?,
        ui::button(h, "从素材库添加", 5)?,
    ];
    for (b, i) in buttons.iter().zip([I::Add, I::RecordOutline, I::Folder]) {
        desk::style(*b, Some(i), true, false, true);
    }
    let more = ui::button(h, "更多素材操作", 6)?;
    desk::style(more, Some(I::More), true, false, true);
    ui::register(w!("PanzoPreviewTabs"), Some(tabs_proc))?;
    let tabs=ui::panel(owner,w!("PanzoPreviewTabs"))?;
    desk::install_canvas(tabs);
    let tab_buttons=[ui::button(tabs,"视频预览",1)?,ui::button(tabs,"素材预览",2)?];
    for btn in tab_buttons { desk::style(btn,None,false,false,false); }
    let quality_host=ui::panel(owner,w!("PanzoPreviewTabs"))?;
    desk::install_canvas(quality_host);
    let quality=ui::combo(quality_host,&["1/4","1/2","原画"],crate::platform::ui_preferences::preview_quality().index(),3)?;
    unsafe{SetWindowLongPtrW(quality_host,GWLP_USERDATA,h.0 as isize);}
    unsafe { SetWindowLongPtrW(tabs,GWLP_USERDATA,h.0 as isize); }
    let s = Box::new(State {
        more,
        owner,
        quality_host,quality,tabs,tab_buttons,source_mode:false,loaded:None,timeline_frames:None,visual_assets:Vec::new(),
        root:PathBuf::new(),source_item:None,thumbs:Default::default(),thumb_job:None,import:None,drag:None,drag_card:None,
        search,
        list,
        buttons,
        items: vec![],
        visible: vec![],
        revision: None,
        hovered: None,
        image: None,
        image_job: None,
    });
    unsafe {
        SetWindowLongPtrW(h, GWLP_USERDATA, Box::into_raw(s) as isize);
        SetTimer(Some(h),2,100,None);
    }
    Ok(ProjectBin { hwnd: h })
}
pub(super) fn update(
    bin: &ProjectBin,
    root: PathBuf,
    revision: u64,
    settings: &panzo_core::WorkbenchSettings,
) {
    let Some(s) = (unsafe { state(bin.hwnd) }) else {
        return;
    };
    if s.revision == Some(revision) {
        return;
    }
    if s.timeline_frames.is_none() || s.root!=root {
        let frames=crate::platform::timeline_visual::Visuals::thumbnails(root.clone());
        let owner=s.owner.0 as usize;
        frames.callback(std::sync::Arc::new(move||unsafe{let _=PostMessageW(Some(HWND(owner as *mut std::ffi::c_void)),WM_APP+82,WPARAM(0),LPARAM(0));}));
        s.timeline_frames=Some(frames);
    }
    s.visual_assets=settings.visual_assets.clone();
    s.root=root.clone();
    let first = s.revision.is_none();
    s.revision = Some(revision);
    let video = s.source_item.clone().unwrap_or_else(|| {
        let manifest = std::fs::read(root.join("project.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<panzo_core::ProjectManifest>(&bytes).ok());
        let fallback = root
            .file_stem()
            .map_or("项目视频".into(), |v| v.to_string_lossy().into_owned());
        let (name, detail) = manifest.map_or((fallback.clone(), "视频".into()), |m| {
            let name = m
                .capture
                .window_title
                .filter(|v| !v.trim().is_empty())
                .unwrap_or(fallback);
            let seconds = m.media.duration_tick.max(0) / m.timebase.ticks_per_second.max(1);
            (
                name,
                format!(
                    "{} · {} × {}",
                    duration(seconds as u64),
                    m.capture.content_size_px.width,
                    m.capture.content_size_px.height
                ),
            )
        });
        Item {
            id: None,
            name,
            detail,
            waveform: vec![],visual:true,subtitle:false,
        }
    });
    s.source_item=Some(video.clone());
    let mut items=if settings.source_asset_removed{Vec::new()}else{vec![video]};
    let mut paths = std::collections::HashSet::new();
    for c in &settings.audio_assets {
        if paths.insert(c.path.clone()) {
            items.push(Item {
                id: Some(c.id.clone()),
                name: c.name.clone(),
                detail: format!("{} · 48 kHz", duration(c.asset_frames / 48000)),
                waveform: c.waveform.clone(),visual:false,subtitle:false,
            });
        }
    }
    for a in settings.visual_assets.iter().filter(|a|a.id!="__source__") {items.push(Item{id:Some(a.id.clone()),name:a.name.clone(),detail:format!("{} · {} × {}",duration((a.duration.0/10_000_000)as u64),a.width,a.height),waveform:Vec::new(),visual:true,subtitle:false});}
    for a in &settings.subtitle_assets{items.push(Item{id:Some(a.id.clone()),name:a.name.clone(),detail:format!("{} 条字幕",a.cues.len()),waveform:Vec::new(),visual:false,subtitle:true});}
    let pending=settings.visual_assets.iter().filter(|a|a.id!="__source__"&&!s.thumbs.contains_key(&a.id)).map(|a|(a.id.clone(),Asset{path:root.join(&a.path),kind:match a.kind{panzo_core::timeline::VisualKind::Image=>Kind::Image,panzo_core::timeline::VisualKind::Gif=>Kind::Gif,_=>Kind::Video},modified:0,bytes:0,missing:false})).collect::<Vec<_>>();
    if !pending.is_empty()&&s.thumb_job.is_none(){let(tx,rx)=mpsc::channel();s.thumb_job=Some(rx);std::thread::spawn(move||{for(id,a)in pending{let r=capture_tools::asset_thumbnail(&a);if tx.send((id,r)).is_err(){break;}}});}
    if s.items != items {
        s.items = items;
        filter(s);
        layout(bin.hwnd, s);
        desk::invalidate(bin.hwnd);
    }
    if first {
        let (tx, rx) = mpsc::channel();
        s.image_job = Some(rx);
        unsafe {
            SetTimer(Some(bin.hwnd), 1, 160, None);
        }
        std::thread::spawn(move || {
            let _ = tx.send(capture_tools::asset_thumbnail(&Asset {
                path: root,
                kind: Kind::Project,
                modified: 0,
                bytes: 0,
                missing: false,
            }));
        });
    }
}
fn filter(s: &mut State) {
    let old_index = unsafe { SendMessageW(s.list, LB_GETCURSEL, None, None) }.0;
    let selected = usize::try_from(old_index)
        .ok()
        .and_then(|i| s.visible.get(i))
        .copied();
    let query = ui::value(s.search).to_lowercase();
    s.visible = s
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.name.to_lowercase().contains(&query))
        .map(|(i, _)| i)
        .collect();
    unsafe {
        SendMessageW(s.list, WM_SETREDRAW, Some(WPARAM(0)), None);
        SendMessageW(s.list, LB_RESETCONTENT, None, None);
        for i in &s.visible {
            let t = ui::wide(&s.items[*i].name);
            SendMessageW(
                s.list,
                LB_ADDSTRING,
                None,
                Some(LPARAM(t.as_ptr() as isize)),
            );
        }
        if !s.visible.is_empty() {
            SendMessageW(
                s.list,
                LB_SETCURSEL,
                Some(WPARAM(
                    selected
                        .and_then(|v| s.visible.iter().position(|i| *i == v))
                        .unwrap_or(0),
                )),
                None,
            );
        }
        SendMessageW(s.list, WM_SETREDRAW, Some(WPARAM(1)), None);
    }
    s.hovered = None;
    desk::invalidate(s.list);
}
fn layout(h: HWND, s: &State) {
    let _batch = ui::layout_batch(h);
    let r = ui::rect(h);
    let d = |n| ui::scale(h, n);
    ui::place(s.more, r.right - d(42), d(13), d(30), d(28));
    for (i, b) in s.buttons.iter().enumerate() {
        ui::place(*b, d(10 + 35 * i as i32), d(50), d(30), d(28));
    }
    ui::place(s.search, d(32), d(90), r.right - d(44), d(28));
    let ph=0;
    ui::place(
        s.list,
        d(10),
        d(128),
        r.right - d(20),
        (r.bottom - d(172) - ph)
            .min(d(61) * s.visible.len() as i32)
            .max(1),
    );
    unsafe {
        if SendMessageW(s.list, LB_GETITEMHEIGHT, Some(WPARAM(0)), None).0 == d(61) as isize {
            return;
        }
        SendMessageW(
            s.list,
            LB_SETITEMHEIGHT,
            Some(WPARAM(0)),
            Some(LPARAM(d(61) as isize)),
        );
    }
}
unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Some(s) = (unsafe { state(h) }) else {
        return unsafe { DefWindowProcW(h, m, w, l) };
    };
    match m {
        m if m==WM_APP+61 => {
            if s.import.is_none() {if let Some(path)=crate::platform::project_assets::choose(s.owner){s.import=Some(crate::platform::project_assets::Job::start(s.root.clone(),path));ui::text(s.buttons[0],"正在导入…");}}
        }
        WM_SIZE => layout(h, s),
        WM_ERASEBKGND => return LRESULT(1),
        WM_TIMER => {
            if w.0==2 {poll_import(h,s);let mut done=false;let mut changed=false;if let Some(rx)=&s.thumb_job{loop{match rx.try_recv(){Ok((id,Ok(r)))=>{s.thumbs.insert(id,r);changed=true;},Ok(_)=>{},Err(std::sync::mpsc::TryRecvError::Disconnected)=>{done=true;break;},Err(_)=>break}}}if done{s.thumb_job=None;}if changed{desk::invalidate(s.list);}return LRESULT(0);}
            if let Some(result) = s.image_job.as_ref().and_then(|rx| rx.try_recv().ok()) {
                s.image = result.ok();
                s.image_job = None;
                unsafe {
                    let _ = KillTimer(Some(h), 1);
                }
                desk::invalidate(s.list);
            }
        }
        WM_COMMAND => {
            let id = w.0 & 65535;
            let code = w.0 >> 16;
            if id == 1 && code == EN_CHANGE as usize {
                filter(s);
                layout(h, s);
            } else if id == 2 && code == LBN_SELCHANGE as usize {
                let i = unsafe { SendMessageW(s.list, LB_GETCURSEL, None, None) }.0 as usize;
                if let Some(item) = s.visible.get(i).and_then(|i| s.items.get(*i)).cloned() {
                    preview_item(s,&item);

                }
            } else if code == 0 {
                match id {
                    3 => unsafe {let _=PostMessageW(Some(h),WM_APP+61,WPARAM(0),LPARAM(0));},
                    4 => crate::platform::recorder_window::workspace_record_setup(s.owner),
                    6 => {
                        let t = editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
                        let rows = vec![
                            crate::platform::app_menu::Row::command(
                                1,
                                "导入音频",
                                I::Waveform,
                                true,
                            ),
                            crate::platform::app_menu::Row::command(2, "打开工程", I::Folder, true),
                            crate::platform::app_menu::Row::command(3, "删除所选素材", I::Delete, selected_audio(s).is_some()).shortcut_hint("Delete"),
                        ];
                        if let Ok(Some(id)) = crate::platform::app_menu::show_rows(
                            h,
                            desk::rect(
                                ui::rect(h).right - t.scale(38),
                                t.scale(8),
                                t.scale(26),
                                t.scale(26),
                            ),
                            t,
                            rows,
                            true,
                        ) {
                            if id == 1 {
                                unsafe {let _=PostMessageW(Some(h),WM_APP+61,WPARAM(0),LPARAM(0));}

                            } else if id == 3 {
                                delete_selected_asset(s);
                            } else {
                                crate::platform::recorder_window::workspace_open(s.owner, true);
                            }
                        }
                    }
                    5 => crate::platform::workspace::switch(2),
                    _ => {}
                }
            }
        }
        WM_DRAWITEM => {
            let item = unsafe { &*(l.0 as *const windows::Win32::UI::Controls::DRAWITEMSTRUCT) };
            if item.hwndItem == s.list {
                #[cfg(test)] REVIEW_PAINT.with(|v| {let mut n=v.get();n[0]+=1;if unsafe {GetObjectType(windows::Win32::Graphics::Gdi::HGDIOBJ(item.hDC.0))} != OBJ_MEMDC.0 as u32 {n[1]+=1;}v.set(n);});
                let t = editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
                let d = |n| t.scale(n);
                let mut r = item.rcItem;
                // Reserve the scrollbar outside the card, never on top of its border.
                if s.visible.len() as i32 * d(61) > ui::rect(s.list).bottom {
                    r.right -= d(12);
                }
                let selected = item.itemState.0 & windows::Win32::UI::Controls::ODS_SELECTED.0 != 0;
                editor_ui::fill_rect(item.hDC, &r, t.palette.panel);
                if selected {
                    editor_ui::rounded_surface(
                        item.hDC,
                        desk::rect(r.left, r.top, r.right - r.left, d(55)),
                        t.palette.raised,
                        t.palette.divider,
                        d(4),
                    );
                } else if s.hovered == Some(item.itemID as usize) {
                    editor_ui::rounded_surface(
                        item.hDC,
                        desk::rect(r.left, r.top, r.right - r.left, d(55)),
                        t.palette.hover,
                        t.palette.hover,
                        d(4),
                    );
                }
                if let Some(i) = s
                    .visible
                    .get(item.itemID as usize)
                    .and_then(|i| s.items.get(*i))
                {
                    let compact = ui::rect(h).right <= d(180);
                    let tw = if compact { 43 } else { 56 };
                    let th = if compact { 33 } else { 39 };
                    let thumb = desk::rect(r.left + d(8), r.top + d((55 - th) / 2), d(tw), d(th));
                    let text_x = thumb.right + d(9);
                    if let Some(image)=i.id.as_ref().and_then(|id|s.thumbs.get(id)){capture_tools::draw_raster(item.hDC,image,thumb);}
                    else if i.id.is_none() && s.image.is_some() {
                        capture_tools::draw_raster(item.hDC, s.image.as_ref().unwrap(), thumb);
                    } else {
                        editor_ui::rounded_surface(
                            item.hDC,
                            thumb,
                            desk::canvas(t),
                            desk::canvas(t),
                            d(2),
                        );
                        if i.subtitle {editor_ui::draw_icon(item.hDC,thumb,I::Subtitles,t.palette.text_muted);}
                        else if !i.visual {
                            // Match the prototype's small, top-aligned waveform, using source peaks.
                            let count = ((tw - 14) / 4).max(1) as usize;
                            for bar in 0..count {
                                let start = bar * i.waveform.len() / count;
                                let end = ((bar + 1) * i.waveform.len() / count)
                                    .max(start + 1)
                                    .min(i.waveform.len());
                                let peak = i
                                    .waveform
                                    .get(start..end)
                                    .unwrap_or(&[])
                                    .iter()
                                    .copied()
                                    .fold(0.0_f32, f32::max);
                                let height = (2.0 + peak.clamp(0.0, 1.0) * 11.0).round() as i32;
                                editor_ui::fill_rect(
                                    item.hDC,
                                    &desk::rect(
                                        thumb.left + d(7 + bar as i32 * 4),
                                        thumb.top + d((13 - height) / 2),
                                        d(2),
                                        d(height),
                                    ),
                                    wave_color(t),
                                );
                            }
                        } else {
                            editor_ui::draw_icon(
                                item.hDC,
                                desk::rect(thumb.left + d(17), thumb.top + d(10), d(20), d(20)),
                                I::Monitor,
                                t.palette.text_muted,
                            );
                        }
                    }
                    desk::text_style(
                        item.hDC,
                        desk::rect(
                            text_x,
                            r.top + d(8),
                            (r.right - text_x - d(8)).max(1),
                            d(17),
                        ),
                        &i.name,
                        t.palette.text,
                        t.dpi,
                        false,
                        TextStyle::Caption,
                    );
                    desk::text_style(
                        item.hDC,
                        desk::rect(
                            text_x,
                            r.top + d(28),
                            (r.right - text_x - d(8)).max(1),
                            d(15),
                        ),
                        &i.detail,
                        t.palette.text_muted,
                        t.dpi,
                        false,
                        TextStyle::Micro,
                    );
                }
                return LRESULT(1);
            }
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(h, &mut ps) };
            let r = ui::rect(h);
            let f = desk::BufferedFrame::new(dc, r);
            let t = editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
            let d = |n| t.scale(n);
            editor_ui::fill_rect(f.dc, &r, t.palette.panel);
            desk::text_style(
                f.dc,
                crate::platform::workbench_layout::panel_title(r, t),
                &format!("项目素材  {}", s.items.len()),
                t.palette.text_secondary,
                t.dpi,
                false,
                TextStyle::Caption,
            );
            editor_ui::fill_rect(
                f.dc,
                &desk::rect(d(12), d(90), d(20), d(28)),
                t.palette.raised,
            );
            editor_ui::draw_icon(
                f.dc,
                desk::rect(d(19), d(98), d(12), d(12)),
                I::Search,
                t.palette.text_muted,
            );
            desk::line(f.dc, desk::rect(0, r.bottom - d(34), r.right, 1), t);
            desk::text_style(
                f.dc,
                desk::rect(d(12), r.bottom - d(30), r.right - d(24), d(24)),
                &format!("当前项目 · {} 个文件", s.items.len()),
                t.palette.text_muted,
                t.dpi,
                false,
                TextStyle::Caption,
            );
            drop(f);
            unsafe {
                let _ = EndPaint(h, &ps);
            }
        }
        WM_NCDESTROY => unsafe {
            SetWindowLongPtrW(h, GWLP_USERDATA, 0);
            drop(Box::from_raw(std::ptr::from_mut(s)));
        },
        _ => return unsafe { DefWindowProcW(h, m, w, l) },
    }
    LRESULT(0)
}

fn duration(seconds: u64) -> String {
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    }
}
fn wave_color(t: editor_ui::EditorTheme) -> windows::Win32::Foundation::COLORREF {
    let a = t.palette.text_secondary.0;
    let b = desk::canvas(t).0;
    editor_ui::rgb(
        (((a & 255) + (b & 255)) / 2) as u8,
        ((((a >> 8) & 255) + ((b >> 8) & 255)) / 2) as u8,
        ((((a >> 16) & 255) + ((b >> 16) & 255)) / 2) as u8,
    )
}
unsafe extern "system" fn list_hover(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    use windows::Win32::UI::{
        Controls::WM_MOUSELEAVE,
        Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent},
        Shell::{DefSubclassProc, RemoveWindowSubclass},
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{SetCapture,ReleaseCapture};
    if m==WM_LBUTTONDOWN {
        let result=unsafe{DefSubclassProc(h,m,w,l)};
        if let Ok(parent)=unsafe{GetParent(h)} { if let Some(s)=unsafe{state(parent)} {
            let hit=unsafe{SendMessageW(h,LB_ITEMFROMPOINT,None,Some(l))}.0 as usize;
            if hit>>16==0 { if let Some(item)=s.visible.get(hit&65535).and_then(|i|s.items.get(*i)).cloned() { preview_item(s,&item); } }
        }}
        if let Ok(parent)=unsafe{GetParent(h)} {if let Some(s)=unsafe{state(parent)} {if let Some(id)=selected_audio(s){let mut p=windows::Win32::Foundation::POINT::default();unsafe{let _=GetCursorPos(&mut p);SetCapture(h);}s.drag=Some((id,p.x,p.y,false));}}}return result;
    }
    if m==WM_MOUSEMOVE {
        if let Some(s)=unsafe{GetParent(h)}.ok().and_then(|p|unsafe{state(p)}) {
            if let Some((id,x,y,active))=&mut s.drag {
                let mut p=windows::Win32::Foundation::POINT::default();unsafe{let _=GetCursorPos(&mut p);}
                *active|=(p.x-*x).abs()>ui::scale(h,5)||(p.y-*y).abs()>ui::scale(h,5);
                if *active {
                    if s.drag_card.is_none() {
                        let item=s.items.iter().find(|i|i.id.as_deref().unwrap_or("__source__")==id);
                        let title=item.map_or("素材",|i|i.name.as_str()).to_owned();
                        let image=s.thumbs.get(id).cloned().or_else(||if id=="__source__"{s.image.clone()}else{None});
                        s.drag_card=crate::platform::workbench_overlays::Overlay::card(s.owner,title,image).ok();
                    }
                    if let Some(card)=&s.drag_card {card.move_card(p);}
                    unsafe{let _=SetCursor(LoadCursorW(None,IDC_SIZEALL).ok());}
                    return LRESULT(0);
                }
            }
        }
    }
    if m==WM_LBUTTONUP {
        let drag=unsafe{GetParent(h)}.ok().and_then(|p|unsafe{state(p)}).and_then(|s|{s.drag_card=None;s.drag.take().map(|d|(s.owner,d))});
        unsafe{let _=ReleaseCapture();}let result=unsafe{DefSubclassProc(h,m,w,l)};
        if let Some((owner,(id,_,_,true)))=drag {let mut p=windows::Win32::Foundation::POINT::default();unsafe{let _=GetCursorPos(&mut p);}if let Err(e)=super::drop_project_asset(owner,&id,p){crate::platform::workspace::notice_error(&e);}}return result;
    }
    if m==WM_KEYDOWN && w.0==0x1b {unsafe{let _=ReleaseCapture();}return LRESULT(0);}
    if m==WM_CAPTURECHANGED||m==WM_CANCELMODE {if let Ok(parent)=unsafe{GetParent(h)} {if let Some(s)=unsafe{state(parent)} {s.drag=None;s.drag_card=None;}}}
    #[cfg(test)] REVIEW_PAINT.with(|v|{let mut n=v.get();if m==LB_SETITEMHEIGHT {n[2]+=1;}if m==WM_PAINT {n[3]+=1;}v.set(n);});
    if m == WM_KEYDOWN && w.0 == 0x2e {
        if l.0 & (1 << 30) != 0 {return LRESULT(0);}
        if let Ok(parent)=unsafe{GetParent(h)} {if let Some(s)=unsafe{state(parent)} {delete_selected_asset(s);}}
        return LRESULT(0);
    }
    if m == WM_CONTEXTMENU {
        if let Ok(parent)=unsafe{GetParent(h)} {
            if let Some(s)=unsafe{state(parent)} {
                let mut point=windows::Win32::Foundation::POINT{x:l.0 as i16 as i32,y:(l.0>>16) as i16 as i32};
                let keyboard=l.0 == -1;
                if !keyboard {
                    unsafe {let _=ScreenToClient(h,&mut point);}
                    let hit=unsafe{SendMessageW(h,LB_ITEMFROMPOINT,None,Some(LPARAM(((point.y as u32)<<16 | (point.x as u32 & 65535)) as isize)))}.0 as usize;
                    if hit>>16!=0 {return LRESULT(0);}
                    unsafe {SendMessageW(h,LB_SETCURSEL,Some(WPARAM(hit&65535)),None);}
                    desk::invalidate(h);
                }
                let anchor=if keyboard {desk::rect(10,128,40,24)}else{
                    unsafe {let _=ClientToScreen(h,&mut point);let _=ScreenToClient(parent,&mut point);}
                    desk::rect(point.x,point.y,1,1)
                };
                let t=editor_ui::EditorTheme::current(editor_ui::window_dpi(parent));
                let rows=vec![crate::platform::app_menu::Row::command(1,"删除素材",I::Delete,selected_audio(s).is_some()).shortcut_hint("Delete")];
                if let Ok(Some(1))=crate::platform::app_menu::show_rows(parent,anchor,t,rows,false) {delete_selected_asset(s);}
            }
        }
        return LRESULT(0);
    }
    if m == WM_MOUSEMOVE || m == WM_MOUSELEAVE {
        let hit = if m == WM_MOUSEMOVE {
            let value = unsafe { SendMessageW(h, LB_ITEMFROMPOINT, None, Some(l)) }.0 as usize;
            if value >> 16 == 0 {
                Some(value & 65535)
            } else {
                None
            }
        } else {
            None
        };
        if let Ok(parent) = unsafe { GetParent(h) } {
            if let Some(s) = unsafe { state(parent) } {
                if s.hovered != hit {
                    s.hovered = hit;
                    desk::invalidate(h);
                }
            }
        }
        if m == WM_MOUSEMOVE {
            let mut track = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: h,
                ..Default::default()
            };
            unsafe {
                let _ = TrackMouseEvent(&mut track);
            }
        }
    }
    if m == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(h, Some(list_hover), id);
        }
    }
    unsafe { DefSubclassProc(h, m, w, l) }
}

#[cfg(test)]
pub(super) fn review_cards(bin: &ProjectBin, out: &std::path::Path) {
    fn snapshot(h: HWND, out: &std::path::Path, name: &str) {
        unsafe {
            let _ = RedrawWindow(
                Some(h),
                None,
                None,
                RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN,
            );
        }
        let until = std::time::Instant::now() + std::time::Duration::from_millis(100);
        while std::time::Instant::now() < until {
            let mut msg = MSG::default();
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        capture_tools::review_window_png(h, &out.join(name));
    }
    let h = bin.hwnd;
    let s = unsafe { state(h) }.unwrap();
    let original = s.items.clone();
    snapshot(h, out, "materials-normal.png");
    for n in 0..12 {
        let mut row = original.last().unwrap().clone();
        row.id = Some(format!("review-{n}"));
        row.name = format!("音频素材 {n:02}.wav");
        s.items.push(row);
    }
    filter(s);
    layout(h, s);
    let t = editor_ui::EditorTheme::current(editor_ui::window_dpi(h));
    assert!(s.visible.len() as i32 * t.scale(61) > ui::rect(s.list).bottom);
    unsafe {
        SendMessageW(s.list, LB_SETCURSEL, Some(WPARAM(1)), None);
    }
    s.hovered = Some(2);
    desk::invalidate(s.list);
    snapshot(h, out, "materials-scroll-hover.png");
    // Search shrinks the native list; the former scrollbar must leave no painted strip.
    ui::text(s.search, "音频素材 03");
    assert_eq!(s.visible.len(), 1);
    snapshot(h, out, "materials-search.png");
    ui::text(s.search, "");
    s.items = original;
    filter(s);
    layout(h, s);
    desk::invalidate(h);
}

#[cfg(test)]
thread_local! { static REVIEW_PAINT: std::cell::Cell<[u64;4]> = const {std::cell::Cell::new([0;4])}; }
#[cfg(test)]
pub(super) fn review_paint_counts(reset:bool)->[u64;4] { REVIEW_PAINT.with(|v|{let result=v.get();if reset {v.set([0;4]);}result}) }

fn selected_audio(s:&State)->Option<String> {
    let i=unsafe{SendMessageW(s.list,LB_GETCURSEL,None,None)}.0;
    usize::try_from(i).ok().and_then(|i|s.visible.get(i)).and_then(|i|s.items.get(*i)).map(|item|item.id.clone().unwrap_or_else(||"__source__".into()))
}
fn delete_selected_asset(s:&State) {
    let result=if let Some(id)=selected_audio(s) {
        super::remove_project_audio_asset(s.owner,&id)
    } else {Err("当前工程的主视频不能移除；旁白与音乐素材可删除".into())};
    if let Err(error)=result {crate::platform::workspace::notice_error(&error);}
}

fn preview_item(s:&mut State,item:&Item) {
    s.loaded=Some(item.id.clone().unwrap_or_else(||"__source__".into()));
    unsafe{let _=PostMessageW(Some(s.owner),WM_APP+81,WPARAM(1),LPARAM(0));}
}
pub(super) fn selected_preview_asset(bin:&ProjectBin)->Option<String>{unsafe{state(bin.hwnd)}.and_then(|s|s.loaded.clone())}
fn poll_import(_h:HWND,s:&mut State){
 let result=s.import.as_ref().and_then(|j|j.result.try_recv().ok());let Some(result)=result else{return;};s.import=None;ui::text(s.buttons[0],"导入素材");
 let result=result.and_then(|mut staged|{let(root,expected,_,_)=super::media_snapshot(s.owner)?;if root!=s.root{return Err("工程已切换，素材保留在原工程目录".into());}
 let mut next=expected.clone();match staged.asset.take().ok_or("素材导入结果为空")?{crate::platform::project_assets::Imported::Audio(a)=>next.audio_assets.push(a),crate::platform::project_assets::Imported::Visual(a)=>next.visual_assets.push(a),crate::platform::project_assets::Imported::Subtitle(a)=>next.subtitle_assets.push(a)};
 super::apply_workspace_media(s.owner,&root,&expected,next)?;staged.commit();Ok(())});
 if let Err(e)=result{crate::platform::workspace::notice_error(&e);}
}

#[cfg(test)] pub(super) fn review_import_click(bin:&ProjectBin,path:PathBuf){
 crate::platform::project_assets::review_choose(path);let s=unsafe{state(bin.hwnd)}.unwrap();let b=s.buttons[0];
 unsafe{let _=windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(s.search));SendMessageW(b,WM_LBUTTONDOWN,Some(WPARAM(1)),Some(LPARAM(5|(5<<16))));SendMessageW(b,WM_LBUTTONUP,Some(WPARAM(0)),Some(LPARAM(5|(5<<16))));}
}
#[cfg(test)] pub(super) fn review_preview_asset(bin:&ProjectBin,id:&str)->HWND{let s=unsafe{state(bin.hwnd)}.unwrap();let item=s.items.iter().find(|a|a.id.as_deref().unwrap_or("__source__")==id).unwrap().clone();preview_item(s,&item);s.owner}

pub(super) fn import_path(bin:&ProjectBin,path:PathBuf)->Result<(),String>{let s=unsafe{state(bin.hwnd)}.ok_or("素材面板已关闭")?;if s.import.is_some(){return Err("请等待当前素材导入完成".into());}s.import=Some(crate::platform::project_assets::Job::start(s.root.clone(),path));ui::text(s.buttons[0],"正在导入…");Ok(())}

pub(super) fn draw_track_thumbnail(bin:&ProjectBin,dc:HDC,clip:&panzo_core::timeline::VisualClip,range:(panzo_core::TimeTick,panzo_core::TimeTick),r:windows::Win32::Foundation::RECT,theme:editor_ui::EditorTheme) {
 if r.right<=r.left||r.bottom<=r.top{return;}
 let Some(s)=(unsafe{state(bin.hwnd)})else{return;};let Some(asset)=s.visual_assets.iter().find(|a|a.id==clip.asset_id).cloned()else{return;};
 let Some(frames)=s.timeline_frames.as_mut()else{return;};frames.bound_cache(256);
 let w=(((r.bottom-r.top)as i64*asset.width as i64/asset.height.max(1)as i64).max(1)as i32).max(theme.scale(24));
 let saved=unsafe{SaveDC(dc)};unsafe{IntersectClipRect(dc,r.left,r.top,r.right,r.bottom);}
 let mut x=r.left;let mut i=0;while x<r.right {
  let fraction=f64::from(x-r.left)/f64::from((r.right-r.left).max(1));
  let tick=panzo_core::TimeTick(range.0.0+((range.1.0-range.0.0)as f64*fraction)as i64);
  if let Ok(Some(f))=frames.frame(&format!("{}:{i}",clip.id),&asset,tick){let image=Raster{width:f.width,height:f.height,pixels:f.pixels.as_ref().clone()};capture_tools::draw_raster(dc,&image,windows::Win32::Foundation::RECT{left:x,right:x+w,..r});}
  x+=w+theme.scale(5);i+=1;
 }
 unsafe{let _=RestoreDC(dc,saved);}
}

pub(super) fn busy(bin: &ProjectBin) -> bool { unsafe { state(bin.hwnd) }.is_some_and(|s| s.import.is_some()) }

pub(super) fn source_mode(bin:&ProjectBin)->bool {unsafe{state(bin.hwnd)}.is_some_and(|s|s.source_mode)}
pub(super) fn select_preview(bin:&ProjectBin,source:bool) {
 if let Some(s)=unsafe{state(bin.hwnd)} { s.source_mode=source; }
}
pub(super) fn layout_preview(bin:&ProjectBin,r:windows::Win32::Foundation::RECT,q:windows::Win32::Foundation::RECT,quality:crate::platform::preview_quality::PreviewQuality) {
    let Some(s)=(unsafe{state(bin.hwnd)})else{return;};
    let d=|n|ui::scale(s.owner,n);let width=d(184).min((r.right-r.left).max(0)/3);
    let height=d(30);let inset=d(2);
    ui::place(s.quality_host,q.left,q.top,q.right-q.left,q.bottom-q.top);
    ui::place(s.quality,0,0,q.right-q.left,q.bottom-q.top);ui::show(s.quality_host,true);
    unsafe {
        // Refresh must not reset an open dropdown while the user is choosing.
        if SendMessageW(s.quality,CB_GETDROPPEDSTATE,None,None).0==0
            && ui::selection(s.quality)!=quality.index() {
            SendMessageW(s.quality,CB_SETCURSEL,Some(WPARAM(quality.index())),None);
        }
    }
    ui::place(s.tabs,r.left+d(10),r.top+((r.bottom-r.top)-height)/2,width,height);
    ui::show(s.tabs,true);
    for (i,b) in s.tab_buttons.iter().enumerate() {
        let tw=(width-inset*2)/2;
        ui::place(*b,inset+i as i32*tw,inset,tw,height-inset*2);
        desk::selected(*b,s.source_mode==(i==1));
    }
}
unsafe extern "system" fn tabs_proc(h:HWND,m:u32,w:WPARAM,l:LPARAM)->LRESULT {
 if m==WM_PAINT {let mut ps=PAINTSTRUCT::default();let dc=unsafe{BeginPaint(h,&mut ps)};let theme=editor_ui::EditorTheme::current(editor_ui::window_dpi(h));editor_ui::fill_rect(dc,&ui::rect(h),theme.palette.panel);editor_ui::rounded_surface(dc,ui::rect(h),theme.palette.raised,theme.palette.raised,theme.scale(4));unsafe{let _=EndPaint(h,&ps);}return LRESULT(0);}
 if m==WM_COMMAND && w.0&0xffff==3 && w.0>>16==CBN_SELCHANGE as usize {
  let bin=HWND(unsafe{GetWindowLongPtrW(h,GWLP_USERDATA)} as *mut std::ffi::c_void);
  if let Some(s)=unsafe{state(bin)} {let index=ui::selection(s.quality);unsafe{let _=PostMessageW(Some(s.owner),WM_APP+83,WPARAM(index),LPARAM(0));}}
  return LRESULT(0);
 }
 if m==WM_COMMAND && w.0>>16==0 {
  let bin=HWND(unsafe{GetWindowLongPtrW(h,GWLP_USERDATA)} as *mut std::ffi::c_void);
  if let Some(s)=unsafe{state(bin)} {if w.0==2 && s.loaded.is_none(){let i=unsafe{SendMessageW(s.list,LB_GETCURSEL,None,None)}.0 as usize;if let Some(item)=s.visible.get(i).and_then(|i|s.items.get(*i)).cloned(){preview_item(s,&item);}}
   unsafe{let _=PostMessageW(Some(s.owner),WM_APP+81,WPARAM(usize::from(w.0==2)),LPARAM(0));}
  }return LRESULT(0);
 }
 unsafe{DefWindowProcW(h,m,w,l)}
}



// Exercise native button processing, including focus transferred from the canvas.
#[cfg(test)]
pub(super) fn review_first_click(bin: &ProjectBin) {
    use windows::Win32::UI::{Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass}, Input::KeyboardAndMouse::{GetCapture, SetFocus}};
    unsafe extern "system" fn count(h:HWND,m:u32,w:WPARAM,l:LPARAM,_:usize,data:usize)->LRESULT {
        if m==WM_COMMAND && w.0>>16==0 && l.0!=0 {
            unsafe { *(data as *mut usize)+=1; }
            return LRESULT(0);
        }
        unsafe { DefSubclassProc(h,m,w,l) }
    }
    let s=unsafe {state(bin.hwnd)}.unwrap();
    let (owner,search,buttons)=(s.owner,s.search,s.buttons);
    let mut clicks=0usize;
    unsafe { assert!(SetWindowSubclass(bin.hwnd,Some(count),0x50415aee,(&raw mut clicks) as usize).as_bool()); }
    struct Hook(HWND);
    impl Drop for Hook { fn drop(&mut self) { unsafe { let _=RemoveWindowSubclass(self.0,Some(count),0x50415aee); } } }
    let _hook=Hook(bin.hwnd);
    for b in buttons.into_iter().chain([s.more]) {
        for focus in [owner,search,b] {
            unsafe {
                let _=SetFocus(Some(focus));
                SendMessageW(b,WM_LBUTTONDOWN,Some(WPARAM(1)),Some(LPARAM(5|(5<<16))));
                assert_eq!(GetCapture(),b,"first press must retain capture after canvas/field focus transfer");
                let before=clicks;
                SendMessageW(b,WM_LBUTTONUP,Some(WPARAM(0)),Some(LPARAM(5|(5<<16))));
                assert_eq!(clicks,before+1,"one press/release must issue exactly one BN_CLICKED");
            }
        }
    }
}

#[cfg(test)] pub(super) fn review_revision(bin:&ProjectBin)->Option<u64>{unsafe{state(bin.hwnd)}.unwrap().revision}

#[cfg(test)] pub(super) fn review_drag_card(bin:&ProjectBin,out:&std::path::Path) {
    let s=unsafe{state(bin.hwnd)}.unwrap();
    let card=s.drag_card.as_ref().expect("drag card remains until release");
    capture_tools::review_window_png(card.review_handle(),&out.join("drag-card.png"));
}

#[cfg(test)]
pub(super) fn review_select_quality(bin:&ProjectBin,q:crate::platform::preview_quality::PreviewQuality) {
    let s=unsafe{state(bin.hwnd)}.unwrap();
    let field=ui::rect(s.quality);let host=ui::rect(s.quality_host);
    assert_eq!(field.bottom,host.bottom,"quality text must fit the visible field");
    assert!(field.right>=ui::scale(s.owner,60));
    unsafe {
        SendMessageW(s.quality,CB_SHOWDROPDOWN,Some(WPARAM(1)),None);
        assert_ne!(SendMessageW(s.quality,CB_GETDROPPEDSTATE,None,None).0,0);
        assert_eq!(SendMessageW(s.quality,CB_GETCOUNT,None,None).0,3);
        SendMessageW(s.quality,CB_SETCURSEL,Some(WPARAM(q.index())),None);
        SendMessageW(s.quality_host,WM_COMMAND,Some(WPARAM(3|((CBN_SELCHANGE as usize)<<16))),Some(LPARAM(s.quality.0 as isize)));
        SendMessageW(s.quality,CB_SHOWDROPDOWN,Some(WPARAM(0)),None);
    }
}
