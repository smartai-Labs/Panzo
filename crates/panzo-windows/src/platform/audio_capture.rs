//! WASAPI shared-mode system loopback and microphone capture. No external encoder is required.
use crate::platform::{
    apartment::WinRtApartment,
    audio::{AudioKind, AudioTrack, PcmWriter, RATE, frames_at},
    qpc::QpcSource,
    recorder::RecordingControl,
};
use panzo_core::TimeTick;
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use windows::Win32::{
    Media::Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR, AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_LOOPBACK,
        AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, DEVICE_STATE_ACTIVE, IAudioCaptureClient,
        IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, eCapture, eConsole,
        eRender,
    },
    System::Com::{CLSCTX_ALL, CoCreateInstance},
};
use windows::core::PCWSTR;
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AudioCaptureOptions {
    pub system: bool,
    pub microphone: bool,
    pub system_device: Option<String>,
    pub microphone_device: Option<String>,
}
pub(super) fn pcm_format() -> WAVEFORMATEX {
    WAVEFORMATEX {
        wFormatTag: 1,
        nChannels: 2,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * 4,
        nBlockAlign: 4,
        wBitsPerSample: 16,
        cbSize: 0,
    }
}
pub(super) fn open_client(capture: bool, device_id: Option<&str>) -> Result<IAudioClient, String> {
    unsafe {
        let devices: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let device = if let Some(id) = device_id {
            let wide = id.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            devices.GetDevice(PCWSTR(wide.as_ptr()))
        } else {
            devices.GetDefaultAudioEndpoint(if capture { eCapture } else { eRender }, eConsole)
        }
        .map_err(|e| e.to_string())?;
        device.Activate(CLSCTX_ALL, None).map_err(|e| e.to_string())
    }
}
pub fn probe(options: &AudioCaptureOptions) -> Result<String, String> {
    let _apartment = WinRtApartment::multi_threaded().map_err(|e| e.to_string())?;
    for (on, mic, id) in [
        (options.system, false, options.system_device.as_deref()),
        (
            options.microphone,
            true,
            options.microphone_device.as_deref(),
        ),
    ] {
        if on {
            let client = open_client(mic, id)?;
            initialize_capture(&client, mic)?;
        }
    }
    Ok("所选音频设备可用：48 kHz 立体声".into())
}
pub(super) fn initialize_capture(client: &IAudioClient, mic: bool) -> Result<(), String> {
    let format = pcm_format();
    let flags = AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
        | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY
        | if mic { 0 } else { AUDCLNT_STREAMFLAGS_LOOPBACK };
    unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            flags,
            1_000_000,
            0,
            &raw const format,
            None,
        )
    }
    .map_err(|e| format!("{}无法初始化：{e}", if mic { "麦克风" } else { "系统声音" }))
}
pub struct AudioCaptureService {
    workers: Vec<JoinHandle<Result<AudioTrack, String>>>,
    stop: Arc<AtomicBool>,
    duration: Arc<AtomicI64>,
    errors: Arc<Mutex<Vec<String>>>,
}
impl AudioCaptureService {
    pub fn start(root: &Path, control: &RecordingControl) -> Result<Self, String> {
        let mut service = Self {
            workers: Vec::new(),
            stop: Arc::new(AtomicBool::new(false)),
            duration: Arc::new(AtomicI64::new(0)),
            errors: Arc::default(),
        };
        for (enabled, kind, id) in [
            (
                control.options.audio.system,
                AudioKind::System,
                control.options.audio.system_device.clone(),
            ),
            (
                control.options.audio.microphone,
                AudioKind::Microphone,
                control.options.audio.microphone_device.clone(),
            ),
        ] {
            if !enabled {
                continue;
            }
            let root = root.to_path_buf();
            let control = control.clone();
            let stop = service.stop.clone();
            let duration = service.duration.clone();
            let errors = service.errors.clone();
            let (sender, receiver) = mpsc::sync_channel(1);
            service.workers.push(thread::spawn(move || {
                let _apartment = WinRtApartment::multi_threaded().map_err(|e| e.to_string())?;
                let setup = (|| {
                    let client = open_client(kind == AudioKind::Microphone, id.as_deref())?;
                    initialize_capture(&client, kind == AudioKind::Microphone)?;
                    let capture: IAudioCaptureClient =
                        unsafe { client.GetService() }.map_err(|e| e.to_string())?;
                    let writer = PcmWriter::create(&root, kind)?;
                    unsafe { client.Start() }.map_err(|e| e.to_string())?;
                    Ok::<_, String>((client, capture, writer))
                })();
                let _ = sender.send(setup.as_ref().map(|_| ()).map_err(Clone::clone));
                let (client, capture, mut writer) = setup?;
                let qpc = QpcSource::new().map_err(|e| e.to_string())?;
                while !stop.load(Ordering::Acquire) {
                    let result = drain(&capture, &control, &mut writer, kind, &qpc);
                    if let Err(error) = result {
                        errors
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(error);
                        control.request_stop();
                        break;
                    }
                    thread::sleep(Duration::from_millis(3));
                }
                unsafe {
                    let _ = client.Stop();
                }
                while !stop.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(3));
                }
                let _ = drain(&capture, &control, &mut writer, kind, &qpc);
                writer.finish(TimeTick(duration.load(Ordering::Acquire)))
            }));
            receiver
                .recv_timeout(Duration::from_secs(8))
                .map_err(|e| format!("音频设备启动超时：{e}"))??;
        }
        Ok(service)
    }
    pub fn finish(mut self, root: &Path, duration: TimeTick) -> Result<Vec<String>, String> {
        self.duration.store(duration.0, Ordering::Release);
        self.stop.store(true, Ordering::Release);
        let mut tracks = Vec::new();
        for worker in self.workers.drain(..) {
            tracks.push(worker.join().map_err(|_| "音频线程意外退出")??);
        }
        if !tracks.is_empty() {
            crate::platform::audio::AudioManifest::save(root, tracks)?;
        }
        Ok(self
            .errors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone())
    }
}
impl Drop for AudioCaptureService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
fn drain(
    capture: &IAudioCaptureClient,
    control: &RecordingControl,
    writer: &mut PcmWriter,
    kind: AudioKind,
    qpc: &QpcSource,
) -> Result<(), String> {
    while unsafe { capture.GetNextPacketSize() }.map_err(|e| format!("音频设备中断：{e}"))? > 0
    {
        let mut data = std::ptr::null_mut();
        let mut frames = 0;
        let mut flags = 0;
        let mut stamp = 0;
        unsafe {
            capture.GetBuffer(
                &raw mut data,
                &raw mut frames,
                &raw mut flags,
                None,
                Some(&raw mut stamp),
            )
        }
        .map_err(|e| e.to_string())?;
        let samples = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
            vec![0i16; frames as usize * 2]
        } else {
            let bytes = unsafe { std::slice::from_raw_parts(data, frames as usize * 4) };
            bytes
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>()
        };
        unsafe { capture.ReleaseBuffer(frames) }.map_err(|e| e.to_string())?;
        if flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR.0 as u32 != 0 {
            stamp = u64::try_from(
                (i128::from(qpc.now().map_err(|e| e.to_string())?) * 10_000_000)
                    / i128::from(qpc.frequency()),
            )
            .map_err(|_| "音频时钟无效")?;
            stamp = stamp.saturating_sub(u64::from(frames) * 10_000_000 / u64::from(RATE));
        }
        let peak = samples
            .iter()
            .map(|v| u32::from(v.unsigned_abs()))
            .max()
            .unwrap_or(0);
        control.levels[usize::from(kind == AudioKind::Microphone)].store(peak, Ordering::Relaxed);
        let session = control
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some(clock) = session.clock else {
            continue;
        };
        let mut start = None;
        let mut output = Vec::new();
        for (i, frame) in samples.chunks_exact(2).enumerate() {
            let wall = TimeTick(
                i64::try_from(stamp).map_err(|_| "音频时间戳超出范围")?
                    + crate::platform::audio::tick_at(i as u64).0
                    - clock.first_frame_system_relative_tick,
            );
            let mapped = session
                .timeline
                .map(wall)
                .map(|tick| frames_at(TimeTick(tick.0 + 104)));
            if let Some(position) = mapped {
                if start.is_some_and(|start| position != start + output.len() as u64 / 2) {
                    writer.write_at(start.unwrap(), &output)?;
                    output.clear();
                    start = None;
                }
                if start.is_none() {
                    start = Some(position);
                }
                output.extend_from_slice(frame);
            } else if let Some(position) = start.take() {
                writer.write_at(position, &output)?;
                output.clear();
            }
        }
        if let Some(start) = start {
            writer.write_at(start, &output)?;
        }
    }
    Ok(())
}
#[derive(Debug, Clone, Serialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}
pub fn devices(capture: bool) -> Result<Vec<AudioDevice>, String> {
    let _apartment = WinRtApartment::multi_threaded().map_err(|e| e.to_string())?;
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let default_id = enumerator
            .GetDefaultAudioEndpoint(if capture { eCapture } else { eRender }, eConsole)
            .ok()
            .and_then(|device| device.GetId().ok())
            .and_then(|id| {
                let text = id.to_string().ok();
                windows::Win32::System::Com::CoTaskMemFree(Some(id.0.cast()));
                text
            });
        let list = enumerator
            .EnumAudioEndpoints(
                if capture { eCapture } else { eRender },
                DEVICE_STATE_ACTIVE,
            )
            .map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for i in 0..list.GetCount().map_err(|e| e.to_string())? {
            let device = list.Item(i).map_err(|e| e.to_string())?;
            let id = device.GetId().map_err(|e| e.to_string())?;
            let value = id.to_string().map_err(|e| e.to_string());
            windows::Win32::System::Com::CoTaskMemFree(Some(id.0.cast()));
            let value = value?;
            let store = device
                .OpenPropertyStore(windows::Win32::System::Com::STGM_READ)
                .map_err(|e| e.to_string())?;
            let mut property = store
                .GetValue(&windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName)
                .map_err(|e| e.to_string())?;
            let label = windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc(
                &raw const property,
            );
            let name = if let Ok(label) = label {
                let text = label
                    .to_string()
                    .unwrap_or_else(|_| format!("设备 {}", i + 1));
                windows::Win32::System::Com::CoTaskMemFree(Some(label.0.cast()));
                text
            } else {
                format!("设备 {}", i + 1)
            };
            let _ =
                windows::Win32::System::Com::StructuredStorage::PropVariantClear(&raw mut property);
            result.push(AudioDevice {
                is_default: default_id.as_deref() == Some(value.as_str()),
                id: value,
                name: if name.trim().is_empty() {
                    format!("音频设备 {}", i + 1)
                } else {
                    name
                },
            });
        }
        Ok(result)
    }
}
pub fn measure(options: &AudioCaptureOptions) -> Result<String, String> {
    let _apartment = WinRtApartment::multi_threaded().map_err(|e| e.to_string())?;
    let mut inputs = Vec::new();
    for (on, mic, id) in [
        (options.system, false, options.system_device.as_deref()),
        (
            options.microphone,
            true,
            options.microphone_device.as_deref(),
        ),
    ] {
        if on {
            let client = open_client(mic, id)?;
            initialize_capture(&client, mic)?;
            let capture: IAudioCaptureClient =
                unsafe { client.GetService() }.map_err(|e| e.to_string())?;
            unsafe { client.Start() }.map_err(|e| e.to_string())?;
            inputs.push((client, capture, mic, 0u32));
        }
    }
    let until = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < until {
        for (_, capture, _, peak) in &mut inputs {
            while unsafe { capture.GetNextPacketSize() }.map_err(|e| e.to_string())? > 0 {
                let mut data = std::ptr::null_mut();
                let mut count = 0;
                let mut flags = 0;
                unsafe {
                    capture.GetBuffer(&raw mut data, &raw mut count, &raw mut flags, None, None)
                }
                .map_err(|e| e.to_string())?;
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 == 0 {
                    let bytes = unsafe { std::slice::from_raw_parts(data, count as usize * 4) };
                    *peak = (*peak).max(
                        bytes
                            .chunks_exact(2)
                            .map(|b| u32::from(i16::from_le_bytes([b[0], b[1]]).unsigned_abs()))
                            .max()
                            .unwrap_or(0),
                    );
                }
                unsafe { capture.ReleaseBuffer(count) }.map_err(|e| e.to_string())?;
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    let mut lines = Vec::new();
    for (client, _, mic, peak) in inputs {
        unsafe {
            let _ = client.Stop();
        }
        lines.push(format!(
            "{}峰值：{}%",
            if mic { "麦克风" } else { "系统声音" },
            peak * 100 / 32768
        ));
    }
    if lines.is_empty() {
        return Ok("没有启用音频来源".into());
    }
    Ok(format!(
        "{}\n0% 表示没有检测到声音；接近 100% 时建议降低输入音量。",
        lines.join("\n")
    ))
}
