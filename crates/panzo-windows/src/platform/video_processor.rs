use crate::platform::d3d11::D3d11Device;
use std::fmt;
use std::mem::ManuallyDrop;
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ, D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SUBRESOURCE_DATA, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT, D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
    D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoContext1,
    ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorOutputView,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::DXGI_ERROR_WAS_STILL_DRAWING;
use windows::core::Interface;

const GPU_READBACK_TIMEOUT: Duration = Duration::from_secs(2);
const GPU_MAP_POLL_INTERVAL: Duration = Duration::from_millis(1);

#[cfg(test)]
#[path = "video_color_tests.rs"]
mod color_tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoProcessorProbeReport {
    pub input_width: u32,
    pub input_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub bgra_input_supported: bool,
    pub nv12_output_supported: bool,
    pub blit_completed: bool,
}

impl VideoProcessorProbeReport {
    pub const fn is_supported(&self) -> bool {
        self.bgra_input_supported && self.nv12_output_supported && self.blit_completed
    }
}

impl fmt::Display for VideoProcessorProbeReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Panzo D3D11 video processor probe")?;
        writeln!(
            formatter,
            "  Conversion: BGRA {}x{} -> NV12 {}x{}",
            self.input_width, self.input_height, self.output_width, self.output_height
        )?;
        writeln!(
            formatter,
            "  BGRA input support: {}",
            self.bgra_input_supported
        )?;
        writeln!(
            formatter,
            "  NV12 output support: {}",
            self.nv12_output_supported
        )?;
        writeln!(
            formatter,
            "  VideoProcessorBlt completed: {}",
            self.blit_completed
        )?;
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

pub struct VideoProcessorProbe;

pub struct Nv12VideoProcessor {
    device: ID3D11Device,
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    output_view: ID3D11VideoProcessorOutputView,
    output_texture: ID3D11Texture2D,
    staging_texture: ID3D11Texture2D,
    immediate_context: ID3D11DeviceContext,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
}

impl Nv12VideoProcessor {
    #[allow(clippy::too_many_lines)]
    pub fn create(
        device: &D3d11Device,
        input_width: u32,
        input_height: u32,
    ) -> Result<Self, VideoProcessorProbeError> {
        Self::create_with_output(device, input_width, input_height, 1_920, 1_080)
    }

    #[allow(clippy::too_many_lines)]
    pub fn create_with_output(
        device: &D3d11Device,
        input_width: u32,
        input_height: u32,
        output_width: u32,
        output_height: u32,
    ) -> Result<Self, VideoProcessorProbeError> {
        if input_width == 0 || input_height == 0 {
            return Err(VideoProcessorProbeError::InvalidInputSize {
                width: input_width,
                height: input_height,
            });
        }
        if output_width == 0
            || output_height == 0
            || !output_width.is_multiple_of(2)
            || !output_height.is_multiple_of(2)
        {
            return Err(VideoProcessorProbeError::InvalidOutputSize {
                width: output_width,
                height: output_height,
            });
        }
        let video_device: ID3D11VideoDevice = stage(
            "ID3D11Device to ID3D11VideoDevice",
            device.native_device().cast(),
        )?;
        let video_context: ID3D11VideoContext = stage(
            "ID3D11DeviceContext to ID3D11VideoContext",
            device.immediate_context().cast(),
        )?;
        let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            },
            InputWidth: input_width,
            InputHeight: input_height,
            OutputFrameRate: DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            },
            OutputWidth: output_width,
            OutputHeight: output_height,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };
        let enumerator = stage("CreateVideoProcessorEnumerator", unsafe {
            video_device.CreateVideoProcessorEnumerator(&raw const content)
        })?;
        let bgra_flags = stage("CheckVideoProcessorFormat(BGRA)", unsafe {
            enumerator.CheckVideoProcessorFormat(DXGI_FORMAT_B8G8R8A8_UNORM)
        })?;
        let nv12_flags = stage("CheckVideoProcessorFormat(NV12)", unsafe {
            enumerator.CheckVideoProcessorFormat(DXGI_FORMAT_NV12)
        })?;
        if bgra_flags & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT.0 as u32 == 0
            || nv12_flags & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0 as u32 == 0
        {
            return Err(VideoProcessorProbeError::UnsupportedConversion);
        }
        let processor = stage("CreateVideoProcessor", unsafe {
            video_device.CreateVideoProcessor(&enumerator, 0)
        })?;
        let color_context: ID3D11VideoContext1 = stage(
            "ID3D11VideoContext to ID3D11VideoContext1",
            video_context.cast(),
        )?;
        // Screen/compositor pixels are full-range sRGB. Encode BT.709 YCbCr with
        // studio range; driver defaults can otherwise produce full-range BT.601
        // samples that an untagged HD decoder interprets as studio-range BT.709.
        // Keep these settings in sync with fmp4_writer::set_video_colorimetry.
        unsafe {
            color_context.VideoProcessorSetStreamColorSpace1(
                &processor,
                0,
                DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
            );
            color_context.VideoProcessorSetOutputColorSpace1(
                &processor,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
            );
            video_context.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);
        }
        let output_texture = create_texture(
            device.native_device(),
            output_width,
            output_height,
            DXGI_FORMAT_NV12,
        )?;
        let staging_texture = create_staging_texture(
            device.native_device(),
            output_width,
            output_height,
            DXGI_FORMAT_NV12,
        )?;
        let immediate_context = device.immediate_context().clone();
        let output_description = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut output_view = None;
        stage("CreateVideoProcessorOutputView", unsafe {
            video_device.CreateVideoProcessorOutputView(
                &output_texture,
                &enumerator,
                &raw const output_description,
                Some(&raw mut output_view),
            )
        })?;
        let output_view =
            output_view.ok_or(VideoProcessorProbeError::NullInterface("output video view"))?;
        Ok(Self {
            device: device.native_device().clone(),
            input_width,
            input_height,
            output_width,
            output_height,
            enumerator,
            processor,
            output_view,
            output_texture,
            staging_texture,
            immediate_context,
            video_device,
            video_context,
        })
    }

    pub fn convert(&self, input: &ID3D11Texture2D) -> Result<Vec<u8>, VideoProcessorProbeError> {
        self.convert_with_trace(input, |_| {})
    }

    pub fn convert_bgra_bytes(&self, bytes: &[u8]) -> Result<Vec<u8>, VideoProcessorProbeError> {
        let required = self
            .input_width
            .checked_mul(self.input_height)
            .and_then(|pixels| pixels.checked_mul(4))
            .map(|value| value as usize)
            .ok_or(VideoProcessorProbeError::InvalidInputSize {
                width: self.input_width,
                height: self.input_height,
            })?;
        if bytes.len() != required {
            return Err(VideoProcessorProbeError::InvalidBgraLength {
                actual: bytes.len(),
                required,
            });
        }
        let texture = create_bgra_texture_with_data(
            &self.device,
            self.input_width,
            self.input_height,
            bytes,
        )?;
        self.convert(&texture)
    }

    pub fn convert_with_trace(
        &self,
        input: &ID3D11Texture2D,
        mut trace: impl FnMut(&'static str),
    ) -> Result<Vec<u8>, VideoProcessorProbeError> {
        self.submit_with_trace(input, &mut trace)?;
        self.copy_completed_with_trace(&mut trace)
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn submit_with_trace(
        &self,
        input: &ID3D11Texture2D,
        mut trace: impl FnMut(&'static str),
    ) -> Result<(), VideoProcessorProbeError> {
        let mut input_description = D3D11_TEXTURE2D_DESC::default();
        unsafe { input.GetDesc(&raw mut input_description) };
        if input_description.Width != self.input_width
            || input_description.Height != self.input_height
            || input_description.Format != DXGI_FORMAT_B8G8R8A8_UNORM
        {
            return Err(VideoProcessorProbeError::UnexpectedInputTexture {
                width: input_description.Width,
                height: input_description.Height,
                format: input_description.Format.0,
            });
        }
        trace("input-texture-validated");
        let view_description = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut input_view = None;
        trace("input-view-create-begin");
        stage("CreateVideoProcessorInputView", unsafe {
            self.video_device.CreateVideoProcessorInputView(
                input,
                &self.enumerator,
                &raw const view_description,
                Some(&raw mut input_view),
            )
        })?;
        let input_view =
            input_view.ok_or(VideoProcessorProbeError::NullInterface("input video view"))?;
        trace("input-view-create-complete");
        let source_rect = RECT {
            left: 0,
            top: 0,
            right: self.input_width.cast_signed(),
            bottom: self.input_height.cast_signed(),
        };
        let destination_rect = RECT {
            left: 0,
            top: 0,
            right: self.output_width.cast_signed(),
            bottom: self.output_height.cast_signed(),
        };
        unsafe {
            self.video_context.VideoProcessorSetStreamSourceRect(
                &self.processor,
                0,
                true,
                Some(&raw const source_rect),
            );
            self.video_context.VideoProcessorSetStreamDestRect(
                &self.processor,
                0,
                true,
                Some(&raw const destination_rect),
            );
        }
        trace("stream-rectangles-set");
        let mut streams = [D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            pInputSurface: ManuallyDrop::new(Some(input_view)),
            ..Default::default()
        }];
        trace("video-processor-blit-begin");
        let blit_result = unsafe {
            self.video_context
                .VideoProcessorBlt(&self.processor, &self.output_view, 0, &streams)
        };
        unsafe { ManuallyDrop::drop(&mut streams[0].pInputSurface) };
        stage("VideoProcessorBlt(BGRA to NV12)", blit_result)?;
        trace("video-processor-blit-complete");
        trace("staging-copy-begin");
        unsafe {
            self.immediate_context
                .CopyResource(&self.staging_texture, &self.output_texture);
        }
        trace("staging-copy-issued");
        trace("gpu-flush-begin");
        unsafe { self.immediate_context.Flush() };
        trace("gpu-flush-complete");
        Ok(())
    }

    pub(crate) fn copy_completed_with_trace(
        &self,
        mut trace: impl FnMut(&'static str),
    ) -> Result<Vec<u8>, VideoProcessorProbeError> {
        trace("gpu-map-begin");
        let mapped = map_completed_nv12(&self.immediate_context, &self.staging_texture)?;
        trace("gpu-map-complete");
        trace("cpu-copy-begin");
        let result = copy_mapped_nv12(
            mapped.pData.cast_const().cast(),
            mapped.RowPitch,
            self.output_width,
            self.output_height,
        );
        unsafe { self.immediate_context.Unmap(&self.staging_texture, 0) };
        trace("cpu-copy-complete");
        result
    }
}

fn map_completed_nv12(
    context: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    texture: &ID3D11Texture2D,
) -> Result<D3D11_MAPPED_SUBRESOURCE, VideoProcessorProbeError> {
    let started = Instant::now();
    loop {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        match unsafe {
            context.Map(
                texture,
                0,
                D3D11_MAP_READ,
                D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                Some(&raw mut mapped),
            )
        } {
            Ok(()) => return Ok(mapped),
            Err(source) if source.code() == DXGI_ERROR_WAS_STILL_DRAWING => {
                if started.elapsed() >= GPU_READBACK_TIMEOUT {
                    return Err(VideoProcessorProbeError::GpuMapTimeout {
                        timeout_ms: GPU_READBACK_TIMEOUT.as_millis(),
                    });
                }
                thread::sleep(GPU_MAP_POLL_INTERVAL);
            }
            Err(source) => {
                return Err(VideoProcessorProbeError::WindowsStage {
                    stage: "ID3D11DeviceContext::Map(completed NV12, DO_NOT_WAIT)",
                    source,
                });
            }
        }
    }
}

fn copy_mapped_nv12(
    source: *const u8,
    row_pitch: u32,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, VideoProcessorProbeError> {
    if source.is_null() || row_pitch < width {
        return Err(VideoProcessorProbeError::InvalidMappedNv12 { row_pitch, width });
    }
    let width = width as usize;
    let height = height as usize;
    let row_pitch = row_pitch as usize;
    let mut output = vec![0_u8; width * height * 3 / 2];
    for row in 0..height {
        unsafe {
            std::ptr::copy_nonoverlapping(
                source.add(row * row_pitch),
                output.as_mut_ptr().add(row * width),
                width,
            );
        }
    }
    let source_uv = unsafe { source.add(row_pitch * height) };
    let output_uv_offset = width * height;
    for row in 0..height / 2 {
        unsafe {
            std::ptr::copy_nonoverlapping(
                source_uv.add(row * row_pitch),
                output.as_mut_ptr().add(output_uv_offset + row * width),
                width,
            );
        }
    }
    Ok(output)
}

impl VideoProcessorProbe {
    #[allow(clippy::too_many_lines)]
    pub fn bgra_to_nv12_1080p() -> Result<VideoProcessorProbeReport, VideoProcessorProbeError> {
        const WIDTH: u32 = 1_920;
        const HEIGHT: u32 = 1_080;
        let device = stage("D3D11 hardware device", D3d11Device::create_hardware())?;
        let video_device: ID3D11VideoDevice = stage(
            "ID3D11Device to ID3D11VideoDevice",
            device.native_device().cast(),
        )?;
        let video_context: ID3D11VideoContext = stage(
            "ID3D11DeviceContext to ID3D11VideoContext",
            device.immediate_context().cast(),
        )?;

        let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            },
            InputWidth: WIDTH,
            InputHeight: HEIGHT,
            OutputFrameRate: DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            },
            OutputWidth: WIDTH,
            OutputHeight: HEIGHT,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };
        let enumerator = stage("CreateVideoProcessorEnumerator", unsafe {
            video_device.CreateVideoProcessorEnumerator(&raw const content)
        })?;
        let bgra_flags = stage("CheckVideoProcessorFormat(BGRA)", unsafe {
            enumerator.CheckVideoProcessorFormat(DXGI_FORMAT_B8G8R8A8_UNORM)
        })?;
        let nv12_flags = stage("CheckVideoProcessorFormat(NV12)", unsafe {
            enumerator.CheckVideoProcessorFormat(DXGI_FORMAT_NV12)
        })?;
        let bgra_input_supported =
            bgra_flags & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT.0 as u32 != 0;
        let nv12_output_supported =
            nv12_flags & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0 as u32 != 0;
        if !bgra_input_supported || !nv12_output_supported {
            return Ok(VideoProcessorProbeReport {
                input_width: WIDTH,
                input_height: HEIGHT,
                output_width: WIDTH,
                output_height: HEIGHT,
                bgra_input_supported,
                nv12_output_supported,
                blit_completed: false,
            });
        }

        let processor = stage("CreateVideoProcessor", unsafe {
            video_device.CreateVideoProcessor(&enumerator, 0)
        })?;
        let input_texture = create_texture(
            device.native_device(),
            WIDTH,
            HEIGHT,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        )?;
        let output_texture =
            create_texture(device.native_device(), WIDTH, HEIGHT, DXGI_FORMAT_NV12)?;

        let input_description = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut input_view = None;
        stage("CreateVideoProcessorInputView", unsafe {
            video_device.CreateVideoProcessorInputView(
                &input_texture,
                &enumerator,
                &raw const input_description,
                Some(&raw mut input_view),
            )
        })?;
        let input_view = input_view
            .ok_or_else(|| VideoProcessorProbeError::NullInterface("input video view"))?;

        let output_description = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut output_view = None;
        stage("CreateVideoProcessorOutputView", unsafe {
            video_device.CreateVideoProcessorOutputView(
                &output_texture,
                &enumerator,
                &raw const output_description,
                Some(&raw mut output_view),
            )
        })?;
        let output_view = output_view
            .ok_or_else(|| VideoProcessorProbeError::NullInterface("output video view"))?;

        let rect = RECT {
            left: 0,
            top: 0,
            right: WIDTH.cast_signed(),
            bottom: HEIGHT.cast_signed(),
        };
        unsafe {
            video_context.VideoProcessorSetStreamSourceRect(
                &processor,
                0,
                true,
                Some(&raw const rect),
            );
            video_context.VideoProcessorSetStreamDestRect(
                &processor,
                0,
                true,
                Some(&raw const rect),
            );
        }

        let mut streams = [D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            pInputSurface: ManuallyDrop::new(Some(input_view)),
            ..Default::default()
        }];
        let blit_result =
            unsafe { video_context.VideoProcessorBlt(&processor, &output_view, 0, &streams) };
        unsafe { ManuallyDrop::drop(&mut streams[0].pInputSurface) };
        stage("VideoProcessorBlt(BGRA to NV12)", blit_result)?;
        unsafe { device.immediate_context().Flush() };

        Ok(VideoProcessorProbeReport {
            input_width: WIDTH,
            input_height: HEIGHT,
            output_width: WIDTH,
            output_height: HEIGHT,
            bgra_input_supported,
            nv12_output_supported,
            blit_completed: true,
        })
    }
}

fn create_texture(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    width: u32,
    height: u32,
    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
) -> Result<ID3D11Texture2D, VideoProcessorProbeError> {
    let description = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    stage("ID3D11Device::CreateTexture2D", unsafe {
        device.CreateTexture2D(&raw const description, None, Some(&raw mut texture))
    })?;
    texture.ok_or_else(|| VideoProcessorProbeError::NullInterface("D3D11 texture"))
}

fn create_bgra_texture_with_data(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    bytes: &[u8],
) -> Result<ID3D11Texture2D, VideoProcessorProbeError> {
    let description = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let initial = D3D11_SUBRESOURCE_DATA {
        pSysMem: bytes.as_ptr().cast(),
        SysMemPitch: width.saturating_mul(4),
        SysMemSlicePitch: 0,
    };
    let mut texture = None;
    stage("ID3D11Device::CreateTexture2D(BGRA data)", unsafe {
        device.CreateTexture2D(
            &raw const description,
            Some(&raw const initial),
            Some(&raw mut texture),
        )
    })?;
    texture.ok_or(VideoProcessorProbeError::NullInterface(
        "BGRA input texture",
    ))
}

fn create_staging_texture(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    width: u32,
    height: u32,
    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
) -> Result<ID3D11Texture2D, VideoProcessorProbeError> {
    let description = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut texture = None;
    stage("CreateTexture2D(staging)", unsafe {
        device.CreateTexture2D(&raw const description, None, Some(&raw mut texture))
    })?;
    texture.ok_or(VideoProcessorProbeError::NullInterface(
        "staging NV12 texture",
    ))
}

fn stage<T>(
    stage: &'static str,
    result: windows::core::Result<T>,
) -> Result<T, VideoProcessorProbeError> {
    result.map_err(|source| VideoProcessorProbeError::WindowsStage { stage, source })
}

#[derive(Debug, Error)]
pub enum VideoProcessorProbeError {
    #[error("D3D11 video processor stage '{stage}' failed: {source}")]
    WindowsStage {
        stage: &'static str,
        source: windows::core::Error,
    },
    #[error("D3D11 returned a null {0} interface")]
    NullInterface(&'static str),
    #[error("video processor input size must be non-zero, got {width}x{height}")]
    InvalidInputSize { width: u32, height: u32 },
    #[error("NV12 output size must be non-zero and even, got {width}x{height}")]
    InvalidOutputSize { width: u32, height: u32 },
    #[error("D3D11 video processor does not support BGRA to NV12 conversion")]
    UnsupportedConversion,
    #[error("unexpected input texture {width}x{height}, DXGI format {format}")]
    UnexpectedInputTexture {
        width: u32,
        height: u32,
        format: i32,
    },
    #[error("BGRA input length {actual} does not equal required {required}")]
    InvalidBgraLength { actual: usize, required: usize },
    #[error("mapped NV12 row pitch {row_pitch} is smaller than width {width}")]
    InvalidMappedNv12 { row_pitch: u32, width: u32 },
    #[error("D3D11 NV12 staging texture remained busy during Map for {timeout_ms} ms")]
    GpuMapTimeout { timeout_ms: u128 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reusable_processor_returns_packed_1080p_nv12() {
        let device = D3d11Device::create_hardware().unwrap();
        assert!(device.multithread_protected());
        let input = create_texture(
            device.native_device(),
            1_920,
            1_080,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        )
        .unwrap();
        let processor = Nv12VideoProcessor::create(&device, 1_920, 1_080).unwrap();
        let output = processor.convert(&input).unwrap();
        assert_eq!(output.len(), 1_920 * 1_080 * 3 / 2);
    }

    #[test]
    fn processor_preserves_native_1440p_when_requested() {
        let device = D3d11Device::create_hardware().unwrap();
        let input = create_texture(
            device.native_device(),
            2_560,
            1_440,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        )
        .unwrap();
        let processor =
            Nv12VideoProcessor::create_with_output(&device, 2_560, 1_440, 2_560, 1_440).unwrap();
        let output = processor.convert(&input).unwrap();
        assert_eq!(output.len(), 2_560 * 1_440 * 3 / 2);
    }

    #[test]
    fn reusable_processor_converts_five_seconds_of_1440p_source_frames() {
        let device = D3d11Device::create_hardware().unwrap();
        let input = create_texture(
            device.native_device(),
            2_560,
            1_440,
            DXGI_FORMAT_B8G8R8A8_UNORM,
        )
        .unwrap();
        let processor = Nv12VideoProcessor::create(&device, 2_560, 1_440).unwrap();
        for _ in 0..300 {
            let output = processor.convert(&input).unwrap();
            assert_eq!(output.len(), 1_920 * 1_080 * 3 / 2);
        }
    }
}
