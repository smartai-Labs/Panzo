//! Interactive proof using only the application's own generated image.
use super::*;
#[test]
#[ignore = "requires desktop/GPU and new PANZO_STAGE_C_NATIVE_DIR; no audio is recorded"]
fn stage_c_pause_hud_and_camera() {
    let directory = PathBuf::from(
        std::env::var_os("PANZO_STAGE_C_NATIVE_DIR").expect("isolated output directory"),
    );
    fs::create_dir_all(&directory).unwrap();
    let library = directory.join("library");
    fs::create_dir_all(&library).unwrap();
    let (sender, receiver) = mpsc::channel();
    RecorderWindow::run_internal(&library,Automation::None,None,Some(Box::new(move|raw|{
 let hwnd=HWND(raw as *mut std::ffi::c_void);let state=unsafe{state_mut(hwnd)}.unwrap();
 let region=crate::platform::capture_tools::review_picture_flow(hwnd,&state.library_root,&directory);
 crate::platform::export_window::review_format_controls(hwnd,&directory);
 let mut control=RecordingControl::default();control.camera_enabled=true;control.options.fps=30;
 state.hud=Some(crate::platform::recording_hud::RecordingHud::new(hwnd,control.clone()).unwrap());
 state.control=Some(control.clone());
 std::thread::spawn(move||{
 let result=std::panic::catch_unwind(||{
 let project=directory.join("pause.panzo");let target=project.clone();let worker_control=control.clone();let started=Instant::now();
 let worker=std::thread::spawn(move||CaptureRecorder::record_with_control(&target,Duration::from_millis(1500),&worker_control,&RecordingSource::Region(region)));
 while !control.recording_started(){assert!(started.elapsed()<Duration::from_secs(15));std::thread::sleep(Duration::from_millis(10));}
 std::thread::sleep(Duration::from_millis(300));assert!(control.toggle_pause().unwrap());let frozen=control.elapsed();control.set_camera_enabled(false).unwrap();std::thread::sleep(Duration::from_millis(650));assert_eq!(frozen,control.elapsed());assert!(!control.toggle_pause().unwrap());std::thread::sleep(Duration::from_millis(350));control.set_camera_enabled(true).unwrap();
 let report=worker.join().unwrap().unwrap();assert!(started.elapsed()>=Duration::from_millis(2100));assert!((report.duration_tick-15_000_000).abs()<2_000_000,"duration {}",report.duration_tick);
 let layout=ProjectLayout::open(&project).unwrap();assert_eq!(layout.load_manifest().unwrap().capture.target_fps,30);
 let editor=crate::platform::editor::ProjectEditorSession::open(&project).unwrap();let policy=editor.settings().automatic_camera.as_ref().unwrap();assert!(!policy.at(frozen).enabled);assert!(policy.at(panzo_core::TimeTick(report.duration_tick-1)).enabled);
 let mut decoder=crate::platform::media_decoder::MfBgraDecoder::open(project.join("media/screen.mp4")).unwrap();let mut ticks=Vec::new();while let Some(frame)=decoder.read_next().unwrap(){ticks.push(frame.pts.0);}assert!(ticks.windows(2).all(|pair|pair[1]>pair[0]&&pair[1]-pair[0]<2_000_000));
 let devices=crate::platform::audio_capture::devices(true);let probe=crate::platform::audio_capture::probe(&crate::platform::audio_capture::AudioCaptureOptions{system:true,..Default::default()});
 fs::write(directory.join("pause-proof.json"),serde_json::to_vec_pretty(&serde_json::json!({"videoFrames":ticks.len(),"durationTick":report.duration_tick,"pauseRemovedMs":650,"frozenTick":frozen,"camera":policy,"microphones":devices,"systemInitialization":probe})).unwrap()).unwrap();
 });let _=sender.send(result.map_err(|_|"stage C assertions failed"));unsafe{let _=windows::Win32::UI::WindowsAndMessaging::PostMessageW(Some(HWND(raw as *mut std::ffi::c_void)),WM_CLOSE,WPARAM(0),LPARAM(0));}
 });
 }))).unwrap();
    receiver
        .recv_timeout(Duration::from_mins(2))
        .unwrap()
        .unwrap();
}
