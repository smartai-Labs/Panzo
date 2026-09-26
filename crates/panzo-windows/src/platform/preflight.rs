use crate::platform::apartment::WinRtApartment;
use crate::platform::d3d11::D3d11Device;
use crate::platform::media_foundation::EncoderProbe;
use crate::platform::qpc::{QpcSource, QpcSourceError};
use crate::platform::video_processor::VideoProcessorProbe;
use std::fmt;
use thiserror::Error;
use windows::Graphics::Capture::GraphicsCaptureSession;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityStatus {
    pub supported: bool,
    pub detail: String,
}

impl CapabilityStatus {
    fn supported(detail: impl Into<String>) -> Self {
        Self {
            supported: true,
            detail: detail.into(),
        }
    }

    fn unsupported(detail: impl Into<String>) -> Self {
        Self {
            supported: false,
            detail: detail.into(),
        }
    }

    fn from_windows_error(context: &str, error: &windows::core::Error) -> Self {
        Self::unsupported(format!("{context}: {error}"))
    }
}

impl fmt::Display for CapabilityStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} ({})",
            if self.supported {
                "supported"
            } else {
                "not supported"
            },
            self.detail
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightReport {
    pub qpc_frequency: i64,
    pub graphics_capture: CapabilityStatus,
    pub d3d11_hardware: CapabilityStatus,
    pub bgra_to_nv12: CapabilityStatus,
    pub hardware_h264: CapabilityStatus,
}

impl PreflightReport {
    pub fn collect() -> Result<Self, PreflightError> {
        let qpc = QpcSource::new()?;
        let graphics_capture = graphics_capture_status();
        let d3d11_hardware = match D3d11Device::create_hardware() {
            Ok(device) => CapabilityStatus::supported(format!(
                "D3D feature level 0x{:X}",
                device.feature_level().0
            )),
            Err(error) => {
                CapabilityStatus::from_windows_error("D3D11 device creation failed", &error)
            }
        };
        let hardware_h264 = match EncoderProbe::hardware_h264() {
            Ok(report) if report.hardware_h264_available() => {
                CapabilityStatus::supported(report.hardware_encoder_names.join(", "))
            }
            Ok(_) => CapabilityStatus::unsupported("no NV12-to-H.264 hardware MFT found"),
            Err(error) => CapabilityStatus::unsupported(error.to_string()),
        };
        let bgra_to_nv12 = match VideoProcessorProbe::bgra_to_nv12_1080p() {
            Ok(report) if report.is_supported() => {
                CapabilityStatus::supported("1920x1080 VideoProcessorBlt completed")
            }
            Ok(_) => CapabilityStatus::unsupported("BGRA input or NV12 output is unsupported"),
            Err(error) => CapabilityStatus::unsupported(error.to_string()),
        };

        Ok(Self {
            qpc_frequency: qpc.frequency(),
            graphics_capture,
            d3d11_hardware,
            bgra_to_nv12,
            hardware_h264,
        })
    }

    pub const fn is_supported(&self) -> bool {
        self.qpc_frequency > 0
            && self.graphics_capture.supported
            && self.d3d11_hardware.supported
            && self.bgra_to_nv12.supported
            && self.hardware_h264.supported
    }
}

impl fmt::Display for PreflightReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Panzo V0.1 preflight")?;
        writeln!(formatter, "  QPC frequency: {}", self.qpc_frequency)?;
        writeln!(
            formatter,
            "  Windows Graphics Capture: {}",
            self.graphics_capture
        )?;
        writeln!(formatter, "  D3D11 hardware: {}", self.d3d11_hardware)?;
        writeln!(formatter, "  BGRA to NV12: {}", self.bgra_to_nv12)?;
        writeln!(formatter, "  Hardware H.264: {}", self.hardware_h264)?;
        write!(
            formatter,
            "  Overall: {}",
            if self.is_supported() {
                "supported"
            } else {
                "not supported"
            }
        )
    }
}

fn graphics_capture_status() -> CapabilityStatus {
    let _apartment = match WinRtApartment::multi_threaded() {
        Ok(apartment) => apartment,
        Err(error) => {
            return CapabilityStatus::from_windows_error(
                "Windows Runtime initialization failed",
                &error,
            );
        }
    };
    match GraphicsCaptureSession::IsSupported() {
        Ok(true) => {
            CapabilityStatus::supported("GraphicsCaptureSession::IsSupported returned true")
        }
        Ok(false) => {
            CapabilityStatus::unsupported("GraphicsCaptureSession::IsSupported returned false")
        }
        Err(error) => CapabilityStatus::from_windows_error(
            "GraphicsCaptureSession::IsSupported failed",
            &error,
        ),
    }
}

#[derive(Debug, Error)]
pub enum PreflightError {
    #[error(transparent)]
    Qpc(#[from] QpcSourceError),
}
