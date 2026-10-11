//! Lightweight, non-activating overlays. They never decode media or capture input.
use super::{workspace_ui as ui,desktop_ui as desk,editor_ui as theme,capture_tools::{self,Raster}};
use std::{sync::{Arc,atomic::{AtomicBool,Ordering}},time::Instant};
use windows::{core::w,Win32::{Foundation::{HWND,LPARAM,LRESULT,WPARAM,POINT},Graphics::Gdi::*,UI::WindowsAndMessaging::*}};
pub(crate) struct Overlay(HWND);
struct State { title:String, image:Option<Raster>, countdown:Option<(u8,Instant)>, cancel:Option<Arc<AtomicBool>> }
impl Drop for Overlay {fn drop(&mut self){unsafe{let _=DestroyWindow(self.0);}}}
impl Overlay {
    #[cfg(test)] pub(crate) fn review_handle(&self)->HWND{self.0}
    fn create(owner:HWND,state:State,popup:bool)->Result<Self,String>{
        ui::register(w!("PanzoWorkbenchOverlay"),Some(proc))?;
        let h=unsafe{CreateWindowExW(
            if popup {WS_EX_NOACTIVATE|WS_EX_TOOLWINDOW|WS_EX_TRANSPARENT|WS_EX_LAYERED}else{WS_EX_NOACTIVATE},
            w!("PanzoWorkbenchOverlay"),w!(""),if popup {WS_POPUP}else{WS_CHILD},0,0,1,1,Some(owner),None,None,None
        )}.map_err(|e|e.to_string())?;
        unsafe{SetWindowLongPtrW(h,GWLP_USERDATA,Box::into_raw(Box::new(state)) as isize);}
        if popup {unsafe{let _=SetLayeredWindowAttributes(h,windows::Win32::Foundation::COLORREF(0),224,LWA_ALPHA);}}
        Ok(Self(h))
    }
    pub(crate) fn card(owner:HWND,title:String,image:Option<Raster>)->Result<Self,String>{
        Self::create(owner,State{title,image,countdown:None,cancel:None},true)
    }
    pub(crate) fn move_card(&self,p:POINT){
        let d=|x|ui::scale(self.0,x);
        unsafe{let _=SetWindowPos(self.0,Some(HWND_TOP),p.x+d(18),p.y+d(18),d(226),d(62),SWP_NOACTIVATE|SWP_SHOWWINDOW);}
    }
    pub(crate) fn countdown(owner:HWND,cancel:Arc<AtomicBool>)->Result<Self,String>{
        let this=Self::create(owner,State{title:"准备录制旁白".into(),image:None,countdown:Some((3,Instant::now())),cancel:Some(cancel)},false)?;
        unsafe{SetTimer(Some(this.0),1,33,None);}
        this.count(3);Ok(this)
    }
    pub(crate) fn count(&self,n:u8){
        if let Some(s)=unsafe{state(self.0)} {s.countdown=Some((n,Instant::now()));}
        self.place_countdown();desk::invalidate(self.0);
    }
    fn place_countdown(&self){
        let Ok(owner)=(unsafe{GetParent(self.0)}) else{return;};
        let r=super::preview_window::preview_area(owner).unwrap_or_else(||ui::rect(owner));
        let d=|x|ui::scale(owner,x);let width=d(208);let height=d(192);
        unsafe{let _=SetWindowPos(self.0,Some(HWND_TOP),(r.left+r.right-width)/2,(r.top+r.bottom-height)/2,width,height,SWP_NOACTIVATE|SWP_SHOWWINDOW);}
    }
}
unsafe fn state(h:HWND)->Option<&'static mut State>{unsafe{(GetWindowLongPtrW(h,GWLP_USERDATA) as *mut State).as_mut()}}
unsafe extern "system" fn proc(h:HWND,m:u32,w:WPARAM,l:LPARAM)->LRESULT{
    let Some(s)=(unsafe{state(h)})else{return unsafe{DefWindowProcW(h,m,w,l)};};
    match m {
        WM_MOUSEACTIVATE=>return LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST if s.countdown.is_none()=>return LRESULT(HTTRANSPARENT as isize),
        WM_TIMER=>{desk::invalidate(h);return LRESULT(0);}
        WM_LBUTTONUP=>{if let Some(c)=&s.cancel {c.store(true,Ordering::Release);ui::show(h,false);}return LRESULT(0);}
        WM_ERASEBKGND=>return LRESULT(1),
        WM_PAINT=>{
            let mut ps=PAINTSTRUCT::default();let dc=unsafe{BeginPaint(h,&mut ps)};let r=ui::rect(h);
            let t=theme::EditorTheme::current(theme::window_dpi(h));let d=|x|t.scale(x);
            let frame=desk::BufferedFrame::new(dc,r);
            theme::fill_rect(frame.dc,&r,t.palette.window);
            theme::rounded_surface(frame.dc,r,t.palette.raised,t.palette.raised,d(12));
            if let Some((n,started))=s.countdown {
                desk::text(frame.dc,desk::rect(d(12),d(12),r.right-d(24),d(24)),&s.title,t.palette.text_secondary,t.dpi,true);
                let cx=r.right/2;let cy=d(91);let radius=d(38) as f64;
                let progress=1.0-started.elapsed().as_secs_f64().min(1.0);
                let points=(0..=(progress*72.0).ceil() as usize).map(|i|{
                    let a=-std::f64::consts::FRAC_PI_2+i as f64*std::f64::consts::TAU/72.0;
                    POINT{x:cx+(a.cos()*radius) as i32,y:cy+(a.sin()*radius) as i32}
                }).collect::<Vec<_>>();
                let pen=unsafe{CreatePen(PS_SOLID,d(3),t.palette.accent)};let old=unsafe{SelectObject(frame.dc,pen.into())};
                if points.len()>1 {unsafe{let _=Polyline(frame.dc,&points);}}
                unsafe{SelectObject(frame.dc,old);let _=DeleteObject(pen.into());}
                desk::text_style(frame.dc,desk::rect(cx-d(30),cy-d(30),d(60),d(60)),&n.to_string(),t.palette.text,t.dpi,true,theme::TextStyle::Hero);
                desk::text(frame.dc,desk::rect(d(12),d(148),r.right-d(24),d(28)),"取消录制",t.palette.text_muted,t.dpi,true);
            } else {
                let thumb=desk::rect(d(10),d(10),d(56),d(42));
                if let Some(image)=&s.image {capture_tools::draw_raster(frame.dc,image,thumb);}
                else {theme::draw_icon(frame.dc,thumb,theme::IconKind::Music,t.palette.accent);}
                desk::text(frame.dc,desk::rect(d(76),d(9),r.right-d(86),d(24)),&s.title,t.palette.text,t.dpi,false);
                desk::text_style(frame.dc,desk::rect(d(76),d(33),r.right-d(86),d(20)),"拖入对应轨道",t.palette.text_secondary,t.dpi,false,theme::TextStyle::Caption);
            }
            drop(frame);unsafe{let _=EndPaint(h,&ps);}return LRESULT(0);
        }
        WM_NCDESTROY=>{unsafe{SetWindowLongPtrW(h,GWLP_USERDATA,0);drop(Box::from_raw(s));}return LRESULT(0);}
        _=>{}
    }
    unsafe{DefWindowProcW(h,m,w,l)}
}
