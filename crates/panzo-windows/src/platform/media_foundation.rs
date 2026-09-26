use std::fmt;
use std::ptr;
use thiserror::Error;
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, MF_VERSION, MFMediaType_Video, MFSTARTUP_FULL, MFShutdown, MFStartup,
    MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_FRIENDLY_NAME_Attribute, MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoFormat_H264,
    MFVideoFormat_NV12,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::PWSTR;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderProbeReport {
    pub input_subtype: &'static str,
    pub output_subtype: &'static str,
    pub hardware_encoder_names: Vec<String>,
}

impl EncoderProbeReport {
    pub fn hardware_h264_available(&self) -> bool {
        !self.hardware_encoder_names.is_empty()
    }
}

impl fmt::Display for EncoderProbeReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Panzo Media Foundation encoder probe")?;
        writeln!(
            formatter,
            "  Media types: {} -> {}",
            self.input_subtype, self.output_subtype
        )?;
        writeln!(
            formatter,
            "  Hardware H.264 encoder count: {}",
            self.hardware_encoder_names.len()
        )?;
        for name in &self.hardware_encoder_names {
            writeln!(formatter, "  - {name}")?;
        }
        write!(
            formatter,
            "  Overall: {}",
            if self.hardware_h264_available() {
                "supported"
            } else {
                "not supported"
            }
        )
    }
}

pub struct EncoderProbe;

impl EncoderProbe {
    pub fn hardware_h264() -> Result<EncoderProbeReport, EncoderProbeError> {
        let _media_foundation = MediaFoundation::start()?;
        let input = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_NV12,
        };
        let output = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };
        let mut activations: *mut Option<IMFActivate> = ptr::null_mut();
        let mut count = 0;

        unsafe {
            MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
                Some(&raw const input),
                Some(&raw const output),
                &raw mut activations,
                &raw mut count,
            )
        }
        .map_err(EncoderProbeError::Enumerate)?;

        if count > 0 && activations.is_null() {
            return Err(EncoderProbeError::InvalidActivationArray { count });
        }
        let activation_array = ActivationArray { activations, count };
        let mut names = Vec::with_capacity(count as usize);
        for activation in activation_array.into_activations() {
            names.push(friendly_name(&activation)?);
        }

        Ok(EncoderProbeReport {
            input_subtype: "NV12",
            output_subtype: "H.264",
            hardware_encoder_names: names,
        })
    }
}

struct MediaFoundation;

impl MediaFoundation {
    fn start() -> Result<Self, EncoderProbeError> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.map_err(EncoderProbeError::Startup)?;
        Ok(Self)
    }
}

impl Drop for MediaFoundation {
    fn drop(&mut self) {
        let _ = unsafe { MFShutdown() };
    }
}

struct ActivationArray {
    activations: *mut Option<IMFActivate>,
    count: u32,
}

impl ActivationArray {
    fn into_activations(self) -> Vec<IMFActivate> {
        let mut values = Vec::with_capacity(self.count as usize);
        for index in 0..self.count as usize {
            let value = unsafe { ptr::read(self.activations.add(index)) };
            if let Some(value) = value {
                values.push(value);
            }
        }
        values
    }
}

impl Drop for ActivationArray {
    fn drop(&mut self) {
        if !self.activations.is_null() {
            unsafe { CoTaskMemFree(Some(self.activations.cast())) };
        }
    }
}

fn friendly_name(activation: &IMFActivate) -> Result<String, EncoderProbeError> {
    let mut value = PWSTR::null();
    let mut length = 0;
    let attribute = MFT_FRIENDLY_NAME_Attribute;
    unsafe { activation.GetAllocatedString(&raw const attribute, &raw mut value, &raw mut length) }
        .map_err(EncoderProbeError::FriendlyName)?;

    let name = if length == 0 || value.is_null() {
        "<unnamed hardware encoder>".into()
    } else {
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(value.0, length as usize) })
    };
    if !value.is_null() {
        unsafe { CoTaskMemFree(Some(value.0.cast())) };
    }
    Ok(name)
}

#[derive(Debug, Error)]
pub enum EncoderProbeError {
    #[error("Media Foundation startup failed: {0}")]
    Startup(windows::core::Error),
    #[error("hardware NV12-to-H.264 encoder enumeration failed: {0}")]
    Enumerate(windows::core::Error),
    #[error("Media Foundation returned {count} encoders with a null activation array")]
    InvalidActivationArray { count: u32 },
    #[error("reading an encoder friendly name failed: {0}")]
    FriendlyName(windows::core::Error),
}
