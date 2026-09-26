use crate::platform::apartment::WinRtApartment;
use crate::platform::d3d11::D3d11Device;
use std::fmt;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use thiserror::Error;
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;
use windows::core::{IInspectable, factory};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrameInfo {
    pub system_relative_tick: i64,
    pub width: i32,
    pub height: i32,
    pub has_surface: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureProbeReport {
    pub requested_frames: usize,
    pub frames: Vec<CapturedFrameInfo>,
    pub cursor_capture_enabled: bool,
    pub monitor_rect: [i32; 4],
    pub elapsed_ms: u128,
}

impl CaptureProbeReport {
    pub fn timestamps_are_monotonic(&self) -> bool {
        self.frames
            .windows(2)
            .all(|pair| pair[0].system_relative_tick < pair[1].system_relative_tick)
    }
}

impl fmt::Display for CaptureProbeReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Panzo WGC capture probe")?;
        writeln!(
            formatter,
            "  Frames: {}/{}",
            self.frames.len(),
            self.requested_frames
        )?;
        if let Some(first) = self.frames.first() {
            writeln!(formatter, "  Content: {}x{}", first.width, first.height)?;
            writeln!(
                formatter,
                "  First SystemRelativeTime: {}",
                first.system_relative_tick
            )?;
        }
        writeln!(
            formatter,
            "  Cursor captured in source: {}",
            self.cursor_capture_enabled
        )?;
        writeln!(
            formatter,
            "  Monitor rect: ({}, {})-({}, {})",
            self.monitor_rect[0], self.monitor_rect[1], self.monitor_rect[2], self.monitor_rect[3]
        )?;
        writeln!(
            formatter,
            "  Timestamps monotonic: {}",
            self.timestamps_are_monotonic()
        )?;
        write!(formatter, "  Elapsed: {} ms", self.elapsed_ms)
    }
}

pub struct CaptureProbe;

impl CaptureProbe {
    pub fn primary_monitor(
        requested_frames: usize,
        timeout: Duration,
    ) -> Result<CaptureProbeReport, CaptureProbeError> {
        if requested_frames == 0 {
            return Err(CaptureProbeError::InvalidFrameCount);
        }
        let _apartment = windows_stage(
            "RoInitialize(RO_INIT_MULTITHREADED)",
            WinRtApartment::multi_threaded(),
        )?;
        let d3d = windows_stage("D3D11 device creation", D3d11Device::create_hardware())?;
        let monitor = windows_stage("primary monitor lookup", primary_monitor())?;
        let item = primary_monitor_item(monitor)?;
        let size = windows_stage("GraphicsCaptureItem::Size", item.Size())?;
        if size.Width <= 0 || size.Height <= 0 {
            return Err(CaptureProbeError::InvalidCaptureSize {
                width: size.Width,
                height: size.Height,
            });
        }

        let frame_pool = windows_stage(
            "Direct3D11CaptureFramePool::CreateFreeThreaded",
            Direct3D11CaptureFramePool::CreateFreeThreaded(
                d3d.winrt_device(),
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                3,
                size,
            ),
        )?;
        let session = windows_stage(
            "Direct3D11CaptureFramePool::CreateCaptureSession",
            frame_pool.CreateCaptureSession(&item),
        )?;
        windows_stage(
            "GraphicsCaptureSession::SetIsCursorCaptureEnabled",
            session.SetIsCursorCaptureEnabled(false),
        )?;
        let cursor_capture_enabled = windows_stage(
            "GraphicsCaptureSession::IsCursorCaptureEnabled",
            session.IsCursorCaptureEnabled(),
        )?;

        let (sender, receiver) = mpsc::channel();
        let handler =
            TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(move |pool, _| {
                let frame = pool.ok()?.TryGetNextFrame()?;
                let time = frame.SystemRelativeTime()?.Duration;
                let content_size = frame.ContentSize()?;
                let has_surface = frame.Surface().is_ok();
                let _ = sender.send(CapturedFrameInfo {
                    system_relative_tick: time,
                    width: content_size.Width,
                    height: content_size.Height,
                    has_surface,
                });
                frame.Close()?;
                Ok(())
            });
        let token = windows_stage(
            "Direct3D11CaptureFramePool::FrameArrived",
            frame_pool.FrameArrived(&handler),
        )?;

        let started = Instant::now();
        windows_stage(
            "GraphicsCaptureSession::StartCapture",
            session.StartCapture(),
        )?;
        let result = receive_frames(&receiver, requested_frames, timeout);
        let elapsed_ms = started.elapsed().as_millis();

        let _ = frame_pool.RemoveFrameArrived(token);
        let _ = session.Close();
        let _ = frame_pool.Close();

        let frames = result?;
        if frames.iter().any(|frame| !frame.has_surface) {
            return Err(CaptureProbeError::MissingSurface);
        }

        Ok(CaptureProbeReport {
            requested_frames,
            frames,
            cursor_capture_enabled,
            monitor_rect: monitor.rect,
            elapsed_ms,
        })
    }
}

fn receive_frames(
    receiver: &mpsc::Receiver<CapturedFrameInfo>,
    requested_frames: usize,
    timeout: Duration,
) -> Result<Vec<CapturedFrameInfo>, CaptureProbeError> {
    let deadline = Instant::now() + timeout;
    let mut frames = Vec::with_capacity(requested_frames);

    while frames.len() < requested_frames {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok(frame) => frames.push(frame),
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(CaptureProbeError::FrameChannelDisconnected);
            }
        }
    }

    if frames.len() < requested_frames {
        return Err(CaptureProbeError::TimedOut {
            requested: requested_frames,
            received: frames.len(),
        });
    }
    Ok(frames)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PrimaryMonitor {
    pub(crate) handle: HMONITOR,
    pub(crate) rect: [i32; 4],
}

pub(crate) fn primary_monitor() -> windows::core::Result<PrimaryMonitor> {
    let desktop = unsafe { GetDesktopWindow() };
    let monitor = unsafe { MonitorFromWindow(desktop, MONITOR_DEFAULTTOPRIMARY) };
    if monitor.is_invalid() {
        return Err(windows::core::Error::from_thread());
    }

    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &raw mut info) }.as_bool() {
        return Err(windows::core::Error::from_thread());
    }

    Ok(PrimaryMonitor {
        handle: monitor,
        rect: [
            info.rcMonitor.left,
            info.rcMonitor.top,
            info.rcMonitor.right,
            info.rcMonitor.bottom,
        ],
    })
}

pub(crate) fn primary_monitor_item(
    monitor: PrimaryMonitor,
) -> Result<GraphicsCaptureItem, CaptureProbeError> {
    let interop = windows_stage(
        "GraphicsCaptureItem interop factory",
        factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>(),
    )?;
    unsafe {
        interop
            .CreateForMonitor::<GraphicsCaptureItem>(monitor.handle)
            .map_err(|source| CaptureProbeError::CaptureItemForMonitor {
                monitor_handle: monitor.handle.0 as usize,
                rect: monitor.rect,
                source,
            })
    }
}

fn windows_stage<T>(
    stage: &'static str,
    result: windows::core::Result<T>,
) -> Result<T, CaptureProbeError> {
    result.map_err(|source| CaptureProbeError::WindowsStage { stage, source })
}

#[derive(Debug, Error)]
pub enum CaptureProbeError {
    #[error("Windows capture stage '{stage}' failed: {source}")]
    WindowsStage {
        stage: &'static str,
        source: windows::core::Error,
    },
    #[error(
        "creating a capture item for monitor 0x{monitor_handle:X} at ({}, {})-({}, {}) failed: {source}",
        rect[0], rect[1], rect[2], rect[3]
    )]
    CaptureItemForMonitor {
        monitor_handle: usize,
        rect: [i32; 4],
        #[source]
        source: windows::core::Error,
    },
    #[error("requested frame count must be greater than zero")]
    InvalidFrameCount,
    #[error("capture item returned invalid size {width}x{height}")]
    InvalidCaptureSize { width: i32, height: i32 },
    #[error("capture frame did not expose a Direct3D surface")]
    MissingSurface,
    #[error("capture callback channel disconnected")]
    FrameChannelDisconnected,
    #[error("capture timed out after receiving {received}/{requested} frames")]
    TimedOut { requested: usize, received: usize },
}
