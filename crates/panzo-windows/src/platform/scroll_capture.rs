//! Modeless scrolling capture. Capture-excluded controls, conservative overlap checks and manual correction.
use super::{
    HWND, LPARAM, LRESULT, PCWSTR, POINT, Path, PathBuf, Raster, RefCell, Surface, WPARAM, w, wide,
};
use crate::platform::image_ops::ScrollDocument;
use panzo_core::PhysicalRect;
use windows::Win32::{
    Graphics::Gdi::{BitBlt, CAPTUREBLT, GetDC, ReleaseDC, SRCCOPY},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
        },
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, ES_NUMBER, GA_ROOT, GWLP_USERDATA,
            GetAncestor, GetWindowLongPtrW, GetWindowTextW, HMENU, IDC_ARROW, IDYES, IsWindow,
            KillTimer, LoadCursorW, MB_DEFBUTTON2, MB_YESNO, RegisterClassW, SW_HIDE, SW_SHOW,
            SW_SHOWNOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SendMessageW, SetCursorPos,
            SetForegroundWindow, SetTimer, SetWindowDisplayAffinity, SetWindowLongPtrW,
            SetWindowPos, SetWindowTextW, ShowWindow, WDA_EXCLUDEFROMCAPTURE, WINDOW_EX_STYLE,
            WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_NCDESTROY, WM_SETFONT, WM_TIMER, WNDCLASSW,
            WS_BORDER, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
            WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WindowFromPoint,
        },
    },
};
thread_local! {static WINDOWS:RefCell<Vec<HWND>>=const{RefCell::new(Vec::new())};}
struct State {
    owner: HWND,
    target: HWND,
    region: PhysicalRect,
    library: PathBuf,
    document: ScrollDocument,
    pending: Option<Raster>,
    status: HWND,
    overlap: HWND,
    auto_button: HWND,
    automatic: bool,
    waiting: bool,
}
fn text(hwnd: HWND, value: &str) {
    let value = wide(value);
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(value.as_ptr()));
    }
}
fn grab(region: PhysicalRect) -> Result<Raster, String> {
    let surface = Surface::new(region.width, region.height)?;
    let dc = unsafe { GetDC(None) };
    if dc.is_invalid() {
        return Err("桌面暂不可捕获".into());
    }
    let result = unsafe {
        BitBlt(
            surface.dc,
            0,
            0,
            region.width as i32,
            region.height as i32,
            Some(dc),
            region.x,
            region.y,
            SRCCOPY | CAPTUREBLT,
        )
    };
    unsafe {
        ReleaseDC(None, dc);
    }
    result.map_err(|e| e.to_string())?;
    Ok(surface.raster(region.width, region.height))
}
pub(super) fn start(owner: HWND, region: PhysicalRect, library: &Path) -> Result<(), String> {
    if !WINDOWS.with(|v| v.borrow().is_empty()) {
        return Err("已有长截图任务，请先完成或关闭".into());
    }
    if region.width < 48 || region.height < 64 {
        return Err("请选择至少 48×64 像素的滚动内容区域".into());
    }
    unsafe {
        let _ = ShowWindow(crate::platform::workspace::window(owner), SW_HIDE);
        let _ = windows::Win32::Graphics::Dwm::DwmFlush();
    }
    let result = (|| -> Result<HWND, String> {
        let center = POINT {
            x: region.x + region.width as i32 / 2,
            y: region.y + region.height as i32 / 2,
        };
        let target = unsafe { GetAncestor(WindowFromPoint(center), GA_ROOT) };
        let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
        unsafe {
            RegisterClassW(&WNDCLASSW {
                hInstance: module.into(),
                lpszClassName: w!("PanzoScrollCapture"),
                lpfnWndProc: Some(procedure),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(
                    (windows::Win32::Graphics::Gdi::COLOR_WINDOW.0 + 1) as *mut _,
                ),
                ..Default::default()
            });
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                w!("PanzoScrollCapture"),
                w!("Panzo · 滚动长截图"),
                WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN,
                20,
                20,
                680,
                222,
                None,
                None,
                Some(module.into()),
                None,
            )
        }
        .map_err(|e| e.to_string())?;
        crate::platform::desktop_ui::install(hwnd);
        super::editor_ui::apply_window_chrome(hwnd);
        let setup = (|| -> Result<State, String> {
            unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) }
                .map_err(|_| "系统不能从截图中排除控制条，已停止创建长截图")?;
            let scale = |n| super::editor_ui::scale_dip(n, super::editor_ui::window_dpi(hwnd));
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    scale(680),
                    scale(222),
                    SWP_NOMOVE | SWP_NOZORDER,
                );
            }
            let child = |class: PCWSTR, label: &str, id, x, y, width, height, style| {
                let label = wide(label);
                let h = unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        class,
                        PCWSTR(label.as_ptr()),
                        WS_CHILD | WS_VISIBLE | style,
                        scale(x),
                        scale(y),
                        scale(width),
                        scale(height),
                        Some(hwnd),
                        Some(HMENU(id as *mut _)),
                        Some(module.into()),
                        None,
                    )
                }
                .map_err(|e| e.to_string())?;
                unsafe {
                    SendMessageW(
                        h,
                        WM_SETFONT,
                        Some(WPARAM(
                            super::editor_ui::font_handle(
                                super::TextStyle::Body,
                                super::editor_ui::window_dpi(hwnd),
                            )
                            .0 as usize,
                        )),
                        Some(LPARAM(1)),
                    );
                }
                if class == w!("BUTTON") {
                    crate::platform::desktop_ui::button(h);
                }
                if class == w!("EDIT") {
                    crate::platform::desktop_ui::edit(h);
                }
                Ok::<_, String>(h)
            };
            let status = child(
                w!("STATIC"),
                "先在内容窗口向下滚动一段，再采集下一屏。",
                0,
                12,
                10,
                644,
                48,
                WINDOW_STYLE(0),
            )?;
            child(w!("BUTTON"), "采集下一屏", 1, 12, 65, 124, 32, WS_TABSTOP)?;
            let auto_button = child(
                w!("BUTTON"),
                "自动下滚采集",
                2,
                146,
                65,
                140,
                32,
                WS_TABSTOP,
            )?;
            child(w!("BUTTON"), "撤回上一屏", 3, 296, 65, 124, 32, WS_TABSTOP)?;
            child(w!("BUTTON"), "预览长图", 4, 430, 65, 104, 32, WS_TABSTOP)?;
            child(w!("BUTTON"), "完成", 5, 544, 65, 106, 32, WS_TABSTOP)?;
            child(
                w!("STATIC"),
                "手动重叠高度（像素）",
                0,
                12,
                112,
                180,
                28,
                WINDOW_STYLE(0),
            )?;
            let overlap = child(
                w!("EDIT"),
                "",
                6,
                194,
                110,
                110,
                30,
                WS_TABSTOP | WS_BORDER | WINDOW_STYLE(ES_NUMBER as u32),
            )?;
            child(w!("BUTTON"), "手动拼接", 7, 316, 110, 110, 30, WS_TABSTOP)?;
            child(
                w!("STATIC"),
                "自动模式会移鼠标到选区并滚动；固定页眉请排除在选区外。",
                0,
                12,
                150,
                644,
                28,
                WINDOW_STYLE(0),
            )?;
            let mut document = ScrollDocument::default();
            document.append(grab(region)?, None)?;
            Ok(State {
                owner,
                target,
                region,
                library: library.into(),
                document,
                pending: None,
                status,
                overlap,
                auto_button,
                automatic: false,
                waiting: false,
            })
        })();
        match setup {
            Ok(state) => {
                let ptr = Box::into_raw(Box::new(state));
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                }
                WINDOWS.with(|v| v.borrow_mut().push(hwnd));
                Ok(hwnd)
            }
            Err(e) => {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                Err(e)
            }
        }
    })();
    if let Err(e) = result {
        unsafe {
            let _ = ShowWindow(crate::platform::workspace::window(owner), SW_SHOW);
        }
        return Err(e);
    }
    Ok(())
}
impl State {
    fn stop_auto(&mut self, hwnd: HWND) {
        self.automatic = false;
        self.waiting = false;
        unsafe {
            let _ = KillTimer(Some(hwnd), 1);
        }
        text(self.auto_button, "自动下滚采集");
    }
    fn capture(&mut self, hwnd: HWND, manual: bool) -> Result<(), String> {
        let overlap = if manual {
            let mut value = [0u16; 32];
            let n = unsafe { GetWindowTextW(self.overlap, &mut value) }.max(0) as usize;
            Some(
                String::from_utf16_lossy(&value[..n])
                    .parse::<u32>()
                    .map_err(|_| "请输入与上一屏重叠的高度")?,
            )
        } else {
            None
        };
        let image = if manual {
            self.pending.take().map_or_else(|| grab(self.region), Ok)?
        } else {
            grab(self.region)?
        };
        if let Err(e) = self.document.append(image.clone(), overlap) {
            self.pending = Some(image);
            self.stop_auto(hwnd);
            return Err(e);
        }
        self.pending = None;
        text(
            self.status,
            &format!(
                "已拼接 {} 屏，{}×{} 像素。继续滚动，或预览后完成。",
                self.document.frames.len(),
                self.region.width,
                self.document.steps.iter().sum::<u32>()
            ),
        );
        Ok(())
    }
    fn command(&mut self, hwnd: HWND, id: usize) -> Result<bool, String> {
        match id {
            1 => {
                self.stop_auto(hwnd);
                self.capture(hwnd, false)?;
            }
            2 => {
                if self.automatic {
                    self.stop_auto(hwnd);
                } else {
                    self.automatic = true;
                    self.waiting = false;
                    text(self.auto_button, "暂停自动采集");
                    unsafe {
                        SetTimer(Some(hwnd), 1, 650, None);
                    }
                }
            }
            3 => {
                self.stop_auto(hwnd);
                self.document.undo();
                self.pending = None;
                text(
                    self.status,
                    &format!("已撤回，剩余 {} 屏", self.document.frames.len()),
                );
            }
            4 | 5 => {
                self.stop_auto(hwnd);
                if id == 5 && self.pending.is_some() {
                    let answer = unsafe {
                        crate::platform::app_dialog::message(
                            Some(hwnd),
                            w!("还有一屏未拼接成功。是否忽略它并完成已拼接的内容？"),
                            w!("完成长截图"),
                            MB_YESNO | MB_DEFBUTTON2,
                        )
                    };
                    if answer != IDYES {
                        return Ok(false);
                    }
                }
                let image = self.document.compose()?;
                super::remember_capture(&image)?;
                super::show_image(image, &self.library, false)?;
                if id == 5 {
                    return Ok(true);
                }
            }
            7 => {
                self.stop_auto(hwnd);
                self.capture(hwnd, true)?;
            }
            _ => {}
        }
        Ok(false)
    }
}
pub(super) fn close_all() {
    for hwnd in WINDOWS.with(|v| v.borrow().clone()) {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}
unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut State;
    if let Some(state) = unsafe { ptr.as_mut() } {
        match msg {
            WM_COMMAND => {
                match state.command(hwnd, wp.0 & 0xffff) {
                    Ok(true) => {
                        unsafe {
                            let _ = DestroyWindow(hwnd);
                        }
                        return LRESULT(0);
                    }
                    Ok(false) => {}
                    Err(e) => text(state.status, &e),
                }
                return LRESULT(0);
            }
            WM_TIMER if state.automatic => {
                if state.waiting {
                    state.waiting = false;
                    if let Err(e) = state.capture(hwnd, false) {
                        text(state.status, &e);
                    }
                } else {
                    if !unsafe { IsWindow(Some(state.target)) }.as_bool() {
                        state.stop_auto(hwnd);
                        text(state.status, "内容窗口已关闭，请完成已有截图");
                        return LRESULT(0);
                    }
                    unsafe {
                        let _ = SetForegroundWindow(state.target);
                        let _ = SetCursorPos(
                            state.region.x + state.region.width as i32 / 2,
                            state.region.y + state.region.height as i32 / 2,
                        );
                    }
                    let at = POINT {
                        x: state.region.x + state.region.width as i32 / 2,
                        y: state.region.y + state.region.height as i32 / 2,
                    };
                    if unsafe { GetAncestor(WindowFromPoint(at), GA_ROOT) } != state.target {
                        state.stop_auto(hwnd);
                        text(
                            state.status,
                            "选区中心被窗口遮挡，请移开控制条或其他窗口后重试",
                        );
                        return LRESULT(0);
                    }
                    let input = INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: MOUSEINPUT {
                                mouseData: (-240i32).cast_unsigned(),
                                dwFlags: MOUSEEVENTF_WHEEL,
                                ..Default::default()
                            },
                        },
                    };
                    if unsafe { SendInput(&[input], size_of::<INPUT>() as i32) } == 0 {
                        state.stop_auto(hwnd);
                        text(state.status, "无法自动滚动，请手动滚动后采集");
                    } else {
                        state.waiting = true;
                    }
                }
                return LRESULT(0);
            }
            WM_CLOSE => {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                return LRESULT(0);
            }
            WM_NCDESTROY => {
                state.stop_auto(hwnd);
                unsafe {
                    let _ = ShowWindow(crate::platform::workspace::window(state.owner), SW_SHOW);
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    drop(Box::from_raw(ptr));
                }
                WINDOWS.with(|v| v.borrow_mut().retain(|h| *h != hwnd));
            }
            _ => {}
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

#[cfg(test)]
pub(super) fn review(owner: HWND, library: &Path, root: &Path) {
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT, UpdateWindow},
    };
    struct Page {
        image: Raster,
        offset: u32,
    }
    unsafe extern "system" fn page_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Page;
        if let Some(page) = unsafe { ptr.as_mut() } {
            match msg {
                WM_PAINT => {
                    let mut ps = PAINTSTRUCT::default();
                    let dc = unsafe { BeginPaint(hwnd, &raw mut ps) };
                    let image = page
                        .image
                        .crop(PhysicalRect {
                            x: 0,
                            y: page.offset as i32,
                            width: 320,
                            height: 240,
                        })
                        .unwrap();
                    super::draw_raster(
                        dc,
                        &image,
                        RECT {
                            left: 0,
                            top: 0,
                            right: 320,
                            bottom: 240,
                        },
                    );
                    unsafe {
                        let _ = EndPaint(hwnd, &raw const ps);
                    }
                    return LRESULT(0);
                }
                WM_MOUSEWHEEL => {
                    page.offset = (page.offset + 120).min(240);
                    super::invalidate(hwnd);
                    return LRESULT(0);
                }
                _ => {}
            }
        }
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }
    let mut image = Raster {
        width: 320,
        height: 480,
        pixels: vec![255; 320 * 480 * 4],
    };
    for y in 0..480 {
        for x in 0..320 {
            let value = ((y * 31 + x * 17 + (x * y) % 83) % 255) as u8;
            image.pixel(x, y, [value, value / 2, 255 - value, 255]);
        }
    }
    let module = unsafe { GetModuleHandleW(None) }.unwrap();
    unsafe {
        RegisterClassW(&WNDCLASSW {
            hInstance: module.into(),
            lpszClassName: w!("PanzoScrollReview"),
            lpfnWndProc: Some(page_proc),
            ..Default::default()
        });
    }
    let page = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST,
            w!("PanzoScrollReview"),
            w!("Synthetic scrolling content"),
            WS_POPUP | WS_VISIBLE,
            980,
            370,
            320,
            240,
            None,
            None,
            Some(module.into()),
            None,
        )
    }
    .unwrap();
    let ptr = Box::into_raw(Box::new(Page {
        image: image.clone(),
        offset: 0,
    }));
    unsafe {
        SetWindowLongPtrW(page, GWLP_USERDATA, ptr as isize);
    }
    super::review_present(page);
    start(
        owner,
        PhysicalRect {
            x: 980,
            y: 370,
            width: 320,
            height: 240,
        },
        library,
    )
    .unwrap();
    let hwnd = WINDOWS.with(|v| v.borrow()[0]);
    let state = unsafe { &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State) };
    assert_eq!(
        state.document.frames[0].pixels,
        image
            .crop(PhysicalRect {
                x: 0,
                y: 0,
                width: 320,
                height: 240
            })
            .unwrap()
            .pixels
    );
    let mut cursor = POINT::default();
    unsafe {
        let _ = GetCursorPos(&raw mut cursor);
    }
    state.command(hwnd, 2).unwrap();
    unsafe {
        let _ = KillTimer(Some(hwnd), 1);
    }
    // Exercise the same SendInput -> repaint -> settled capture sequence as the timer.
    for _ in 0..2 {
        unsafe {
            SendMessageW(hwnd, WM_TIMER, Some(WPARAM(1)), None);
        }
        let until = std::time::Instant::now() + std::time::Duration::from_millis(400);
        while std::time::Instant::now() < until {
            let mut msg = MSG::default();
            unsafe {
                while PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&raw const msg);
                    DispatchMessageW(&raw const msg);
                }
                let _ = UpdateWindow(page);
                let _ = windows::Win32::Graphics::Dwm::DwmFlush();
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        unsafe {
            SendMessageW(hwnd, WM_TIMER, Some(WPARAM(1)), None);
        }
    }
    let state = unsafe { &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State) };
    state.stop_auto(hwnd);
    let mut status = [0u16; 2048];
    let n = unsafe { GetWindowTextW(state.status, &mut status) }.max(0) as usize;
    assert_eq!(
        state.document.frames.len(),
        3,
        "page offset {}, target {:?}, status {}",
        unsafe { (*ptr).offset },
        state.target,
        String::from_utf16_lossy(&status[..n])
    );
    let result = state.document.compose().unwrap();
    assert_eq!(result.pixels, image.pixels);
    result.save(&root.join("long-screenshot.png")).unwrap();
    assert!(state.capture(hwnd, false).is_err());
    assert!(state.pending.is_some());
    assert_eq!(state.document.frames.len(), 3);
    text(state.overlap, "120");
    state.command(hwnd, 7).unwrap();
    assert!(state.pending.is_none());
    assert_eq!(state.document.compose().unwrap().height, 600);
    state.command(hwnd, 3).unwrap();
    assert_eq!(state.document.compose().unwrap().pixels, image.pixels);
    // Excluded controls can be painted from their client DC for review without disabling exclusion.
    let mut rect = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &raw mut rect);
    }
    crate::platform::editor_ui::render_review_png(
        &root.join("scroll-controls.png"),
        rect.right,
        rect.bottom,
        |dc, _| unsafe {
            SendMessageW(
                hwnd,
                WM_PRINT,
                Some(WPARAM(dc.0 as usize)),
                Some(LPARAM(
                    (PRF_CLIENT | PRF_CHILDREN | PRF_ERASEBKGND) as isize,
                )),
            );
        },
    );
    unsafe {
        let _ = DestroyWindow(hwnd);
        let _ = DestroyWindow(page);
        drop(Box::from_raw(ptr));
        let _ = SetCursorPos(cursor.x, cursor.y);
    }
    std::fs::write(root.join("scroll-proof.json"),serde_json::to_vec_pretty(&serde_json::json!({"automaticScreens":3,"outputWidth":320,"outputHeight":480,"pixelsEqualGeneratedPage":true,"duplicateRejected":true,"manualOverlapApplied":true,"undoRestoredExactPixels":true})).unwrap()).unwrap();
}
