use crate::platform::apartment::WinRtApartment;
use panzo_core::TimeTick;
use std::fmt;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use thiserror::Error;
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaBuffer, IMFMediaType, IMFSample, IMFSourceReader, IMFSourceReaderEx,
    MF_E_INVALIDINDEX, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READERF_ERROR, MF_VERSION, MF_XVP_DISABLE_FRC, MFCreateAttributes, MFCreateMediaType,
    MFCreateSourceReaderFromURL, MFMediaType_Video, MFSTARTUP_FULL, MFShutdown, MFStartup,
    MFT_CATEGORY_VIDEO_PROCESSOR, MFVideoFormat_RGB32,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::core::{GUID, Interface, PCWSTR};

const VIDEO_STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0.cast_unsigned();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedBgraFrame {
    pub native_size: Option<(u32, u32)>,
    pub pts: TimeTick,
    pub duration: Option<TimeTick>,
    pub width: u32,
    pub height: u32,
    /// Tightly packed, top-down BGRA8 pixels.
    pub pixels: std::sync::Arc<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedFrameMetadata {
    pub pts: TimeTick,
    pub duration: Option<TimeTick>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetFrameRead {
    Before(TimeTick),
    AtOrAfter(DecodedBgraFrame),
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderProbeReport {
    pub input_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub decoded_frames: u32,
    pub first_pts: TimeTick,
    pub last_pts: TimeTick,
    pub strictly_increasing_pts: bool,
    pub first_frame_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderSeekProbeReport {
    pub input_path: PathBuf,
    pub indexed_frames: usize,
    pub requested_tick: TimeTick,
    pub selected_frame_index: usize,
    pub selected_pts: TimeTick,
    pub first_after_seek_pts: TimeTick,
    pub decoded_after_seek: usize,
    pub selected_frame_bytes: usize,
    pub pixels_match_sequential: bool,
}

impl fmt::Display for DecoderSeekProbeReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Panzo Media Foundation random-seek probe")?;
        writeln!(formatter, "  Input: {}", self.input_path.display())?;
        writeln!(formatter, "  Indexed frames: {}", self.indexed_frames)?;
        writeln!(
            formatter,
            "  Requested tick: {}",
            self.requested_tick.as_i64()
        )?;
        writeln!(
            formatter,
            "  Selected frame: {} @ {}",
            self.selected_frame_index,
            self.selected_pts.as_i64()
        )?;
        writeln!(
            formatter,
            "  First PTS after seek: {}",
            self.first_after_seek_pts.as_i64()
        )?;
        writeln!(
            formatter,
            "  Frames decoded after seek: {}",
            self.decoded_after_seek
        )?;
        writeln!(
            formatter,
            "  Selected BGRA frame bytes: {}",
            self.selected_frame_bytes
        )?;
        write!(
            formatter,
            "  Pixels match sequential decode: {}",
            self.pixels_match_sequential
        )
    }
}

impl fmt::Display for DecoderProbeReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Panzo Media Foundation BGRA decoder probe")?;
        writeln!(formatter, "  Input: {}", self.input_path.display())?;
        writeln!(formatter, "  Frame size: {}x{}", self.width, self.height)?;
        writeln!(formatter, "  Decoded frames: {}", self.decoded_frames)?;
        writeln!(formatter, "  First PTS: {}", self.first_pts.as_i64())?;
        writeln!(formatter, "  Last PTS: {}", self.last_pts.as_i64())?;
        writeln!(
            formatter,
            "  Strictly increasing PTS: {}",
            self.strictly_increasing_pts
        )?;
        write!(
            formatter,
            "  First BGRA frame bytes: {}",
            self.first_frame_bytes
        )
    }
}

pub struct MfBgraDecoder {
    native_size: (u32, u32),
    preview_limit: Option<u32>,
    input_path: PathBuf,
    reader: Option<IMFSourceReader>,
    width: u32,
    height: u32,
    stride: i32,
    _media_foundation: MediaFoundationSession,
    _apartment: WinRtApartment,
}

impl MfBgraDecoder {
    /// Read container timestamps without inserting a video decoder or RGB converter.
    /// <https://learn.microsoft.com/windows/win32/medfound/processing-media-data-with-the-source-reader>
    pub fn native_index(
        path: &Path,
        cancelled: impl Fn() -> bool,
    ) -> Result<Vec<DecodedFrameMetadata>, MediaDecoderError> {
        let _apartment = WinRtApartment::multi_threaded().map_err(MediaDecoderError::Apartment)?;
        let _mf = MediaFoundationSession::start()?;
        let attributes = create_attributes(1)?;
        let wide = wide_path(path);
        let reader = stage("MFCreateSourceReaderFromURL(index)", unsafe {
            MFCreateSourceReaderFromURL(PCWSTR(wide.as_ptr()), &attributes)
        })?;
        stage("index deselect streams", unsafe {
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0.cast_unsigned(), false)
        })?;
        stage("index select video", unsafe {
            reader.SetStreamSelection(VIDEO_STREAM, true)
        })?;
        let native = stage("index native media type", unsafe {
            reader.GetNativeMediaType(VIDEO_STREAM, 0)
        })?;
        let (width, height) = read_frame_size(&native)?;
        stage("index compressed output", unsafe {
            reader.SetCurrentMediaType(VIDEO_STREAM, None, &native)
        })?;
        let mut frames = Vec::new();
        loop {
            if cancelled() {
                return Err(MediaDecoderError::IndexCancelled);
            }
            let mut flags = 0;
            let mut timestamp = 0;
            let mut sample = None;
            stage("ReadSample(index)", unsafe {
                reader.ReadSample(
                    VIDEO_STREAM,
                    0,
                    None,
                    Some(&raw mut flags),
                    Some(&raw mut timestamp),
                    Some(&raw mut sample),
                )
            })?;
            if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
                return Err(MediaDecoderError::SourceReaderFlagError { flags });
            }
            if let Some(sample) = sample {
                frames.push(DecodedFrameMetadata {
                    pts: sample_time(&sample, timestamp)?,
                    duration: unsafe { sample.GetSampleDuration() }
                        .ok()
                        .filter(|value| *value > 0)
                        .map(TimeTick),
                    width,
                    height,
                });
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
        }
        // Compressed samples may arrive in decode order when an encoder uses B pictures.
        // The editor index is presentation order, matching the decoded source reader.
        frames.sort_by_key(|frame| frame.pts);
        Ok(frames)
    }

    pub fn open(input_path: impl AsRef<Path>) -> Result<Self, MediaDecoderError> {
        Self::open_with_preview_limit(input_path, None)
    }

    pub fn open_with_preview_limit(
        input_path: impl AsRef<Path>,
        longest_side: Option<u32>,
    ) -> Result<Self, MediaDecoderError> {
        let input_path = input_path.as_ref();
        if !input_path.is_file() {
            return Err(MediaDecoderError::MissingInput(input_path.into()));
        }

        let apartment = WinRtApartment::multi_threaded().map_err(MediaDecoderError::Apartment)?;
        let media_foundation = MediaFoundationSession::start()?;
        let attributes = create_attributes(2)?;
        set_u32(&attributes, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
        set_u32(
            &attributes,
            MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING,
            1,
        )?;
        let wide = wide_path(input_path);
        let reader = stage("MFCreateSourceReaderFromURL", unsafe {
            MFCreateSourceReaderFromURL(PCWSTR(wide.as_ptr()), &attributes)
        })?;

        stage("IMFSourceReader::SetStreamSelection(all=false)", unsafe {
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0.cast_unsigned(), false)
        })?;
        stage("IMFSourceReader::SetStreamSelection(video=true)", unsafe {
            reader.SetStreamSelection(VIDEO_STREAM, true)
        })?;
        let native_type = stage("IMFSourceReader::GetNativeMediaType", unsafe {
            reader.GetNativeMediaType(VIDEO_STREAM, 0)
        })?;
        let (visible_width, visible_height) = read_frame_size(&native_type)?;
        let (output_width, output_height) =
            preview_geometry(visible_width, visible_height, longest_side);
        let output_type = rgb32_media_type(output_width, output_height)?;
        stage("IMFSourceReader::SetCurrentMediaType(RGB32)", unsafe {
            reader.SetCurrentMediaType(VIDEO_STREAM, None, &output_type)
        })?;
        disable_frame_rate_conversion(&reader)?;
        let (width, height, stride) = read_output_geometry(&reader)?;

        Ok(Self {
            input_path: input_path.into(),
            native_size: (visible_width, visible_height),
            preview_limit: longest_side,
            reader: Some(reader),
            width,
            height,
            stride,
            _media_foundation: media_foundation,
            _apartment: apartment,
        })
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub fn input_path(&self) -> &Path {
        &self.input_path
    }

    /// Recover premature EOS or shifted output timestamps after repeated random seeks.
    /// The owner worker keeps the same resolution and hardware decode policy.
    pub fn reopen(&mut self) -> Result<(), MediaDecoderError> {
        let replacement = Self::open_with_preview_limit(&self.input_path, self.preview_limit)?;
        *self = replacement;
        Ok(())
    }

    pub fn read_next(&mut self) -> Result<Option<DecodedBgraFrame>, MediaDecoderError> {
        let Some(sample) = self.read_sample()? else {
            return Ok(None);
        };
        self.decode_sample(&sample).map(Some)
    }

    /// Advances one decoded sample toward `target_pts` without copying BGRA pixels for preroll
    /// samples. Interactive seek calls this with a small per-tick budget so the window thread
    /// remains responsive while preserving exact indexed-frame selection.
    pub fn read_next_toward(
        &mut self,
        target_pts: TimeTick,
    ) -> Result<TargetFrameRead, MediaDecoderError> {
        let Some(sample) = self.read_sample()? else {
            return Ok(TargetFrameRead::End);
        };
        if sample.pts < target_pts {
            return Ok(TargetFrameRead::Before(sample.pts));
        }
        self.decode_sample(&sample).map(TargetFrameRead::AtOrAfter)
    }

    fn decode_sample(&self, sample: &DecodedSample) -> Result<DecodedBgraFrame, MediaDecoderError> {
        let buffer = stage("IMFSample::ConvertToContiguousBuffer", unsafe {
            sample.sample.ConvertToContiguousBuffer()
        })?;
        let pixels = copy_top_down_bgra(&buffer, self.width, self.height, self.stride)?;
        Ok(DecodedBgraFrame {
            native_size: Some(self.native_size),
            pts: sample.pts,
            duration: sample.duration,
            width: self.width,
            height: self.height,
            pixels: std::sync::Arc::new(pixels),
        })
    }

    /// Reads only timing and geometry. This avoids copying decoded BGRA pixels and is used to
    /// build the lightweight source-frame index for interactive preview.
    pub fn read_next_metadata(
        &mut self,
    ) -> Result<Option<DecodedFrameMetadata>, MediaDecoderError> {
        Ok(self.read_sample()?.map(|sample| DecodedFrameMetadata {
            pts: sample.pts,
            duration: sample.duration,
            width: self.width,
            height: self.height,
        }))
    }

    /// Repositions the source reader. Media Foundation seeks to an implementation-defined
    /// decode point at or before the requested timestamp; callers must read forward to the
    /// exact indexed frame they need.
    pub fn seek(&mut self, source_tick: TimeTick) -> Result<(), MediaDecoderError> {
        if source_tick.as_i64() < 0 {
            return Err(MediaDecoderError::NegativeSeek(source_tick.as_i64()));
        }
        let reader = self
            .reader
            .as_ref()
            .ok_or(MediaDecoderError::ReaderClosed)?;
        let time_format = GUID::zeroed();
        let position = PROPVARIANT::from(source_tick.as_i64());
        stage("IMFSourceReader::SetCurrentPosition", unsafe {
            reader.SetCurrentPosition(&raw const time_format, &raw const position)
        })
    }

    fn read_sample(&mut self) -> Result<Option<DecodedSample>, MediaDecoderError> {
        let reader = self
            .reader
            .as_ref()
            .ok_or(MediaDecoderError::ReaderClosed)?;
        loop {
            let mut flags = 0_u32;
            let mut timestamp = 0_i64;
            let mut sample = None;
            stage("IMFSourceReader::ReadSample", unsafe {
                reader.ReadSample(
                    VIDEO_STREAM,
                    0,
                    None,
                    Some(&raw mut flags),
                    Some(&raw mut timestamp),
                    Some(&raw mut sample),
                )
            })?;

            if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
                return Err(MediaDecoderError::SourceReaderFlagError { flags });
            }
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                (self.width, self.height, self.stride) = read_output_geometry(reader)?;
            }
            if let Some(sample) = sample {
                let pts = sample_time(&sample, timestamp)?;
                let duration = unsafe { sample.GetSampleDuration() }
                    .ok()
                    .filter(|value| *value > 0)
                    .map(TimeTick);
                return Ok(Some(DecodedSample {
                    sample,
                    pts,
                    duration,
                }));
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(None);
            }
        }
    }
}

/// The Source Reader's advanced video processor defaults to frame-rate conversion. For
/// VFR screen recordings this synthesizes a new timestamp grid after each seek, so a frame
/// indexed by a sequential scan may never appear at that PTS after a random seek. We need
/// color conversion only: keep source sample timing and frame identity unchanged.
fn disable_frame_rate_conversion(reader: &IMFSourceReader) -> Result<(), MediaDecoderError> {
    let reader: IMFSourceReaderEx = stage("IMFSourceReaderEx", reader.cast())?;
    for index in 0.. {
        let mut category = GUID::zeroed();
        let mut transform = None;
        let result = unsafe {
            reader.GetTransformForStream(
                VIDEO_STREAM,
                index,
                Some(&raw mut category),
                &raw mut transform,
            )
        };
        if result
            .as_ref()
            .is_err_and(|error| error.code() == MF_E_INVALIDINDEX)
        {
            return Ok(());
        }
        stage("IMFSourceReaderEx::GetTransformForStream", result)?;
        if category == MFT_CATEGORY_VIDEO_PROCESSOR {
            let transform = transform.ok_or(MediaDecoderError::NullInterface("IMFTransform"))?;
            let attributes = stage("video processor GetAttributes", unsafe {
                transform.GetAttributes()
            })?;
            set_u32(&attributes, MF_XVP_DISABLE_FRC, 1)?;
        }
    }
    unreachable!("transform index space exhausted")
}

struct DecodedSample {
    sample: IMFSample,
    pts: TimeTick,
    duration: Option<TimeTick>,
}

impl Drop for MfBgraDecoder {
    fn drop(&mut self) {
        self.reader.take();
    }
}

pub struct DecoderProbe;

impl DecoderProbe {
    pub fn inspect(
        input_path: impl AsRef<Path>,
        maximum_frames: u32,
    ) -> Result<DecoderProbeReport, MediaDecoderError> {
        if maximum_frames == 0 {
            return Err(MediaDecoderError::InvalidProbeFrameCount);
        }
        let mut decoder = MfBgraDecoder::open(input_path)?;
        let mut decoded_frames = 0_u32;
        let mut first_pts = None;
        let mut last_pts = None;
        let mut strictly_increasing_pts = true;
        let mut first_frame_bytes = 0_usize;

        while decoded_frames < maximum_frames {
            let Some(frame) = decoder.read_next()? else {
                break;
            };
            if decoded_frames == 0 {
                first_pts = Some(frame.pts);
                first_frame_bytes = frame.pixels.len();
            }
            if last_pts.is_some_and(|previous| frame.pts <= previous) {
                strictly_increasing_pts = false;
            }
            last_pts = Some(frame.pts);
            decoded_frames += 1;
        }
        if decoded_frames == 0 {
            return Err(MediaDecoderError::NoVideoFrames);
        }

        let (Some(first_pts), Some(last_pts)) = (first_pts, last_pts) else {
            return Err(MediaDecoderError::NoVideoFrames);
        };
        Ok(DecoderProbeReport {
            input_path: decoder.input_path.clone(),
            width: decoder.width,
            height: decoder.height,
            decoded_frames,
            first_pts,
            last_pts,
            strictly_increasing_pts,
            first_frame_bytes,
        })
    }

    pub fn inspect_seek(
        input_path: impl AsRef<Path>,
        requested_tick: TimeTick,
    ) -> Result<DecoderSeekProbeReport, MediaDecoderError> {
        if requested_tick < TimeTick::ZERO {
            return Err(MediaDecoderError::NegativeSeek(requested_tick.as_i64()));
        }
        let input_path = input_path.as_ref();
        let mut index_decoder = MfBgraDecoder::open(input_path)?;
        let mut presentation_ticks = Vec::new();
        let mut reference_sample = None;
        while let Some(frame) = index_decoder.read_sample()? {
            if let Some(&previous) = presentation_ticks.last()
                && frame.pts <= previous
            {
                return Err(MediaDecoderError::NonMonotonicIndex {
                    previous,
                    current: frame.pts,
                });
            }
            presentation_ticks.push(frame.pts);
            if reference_sample.is_none() || frame.pts <= requested_tick {
                reference_sample = Some(frame);
            }
        }
        if presentation_ticks.is_empty() {
            return Err(MediaDecoderError::NoVideoFrames);
        }
        let upper = presentation_ticks.partition_point(|candidate| *candidate <= requested_tick);
        let selected_frame_index = upper.saturating_sub(1);
        let selected_pts = presentation_ticks[selected_frame_index];
        let reference = index_decoder
            .decode_sample(&reference_sample.ok_or(MediaDecoderError::NoVideoFrames)?)?;
        // Source Reader seeks are implementation-defined and some hardware-backed paths can
        // return the first sample *after* a non-keyframe target. Seek from a bounded indexed
        // preroll and decode forward so the exact VFR frame remains deterministic.
        let seek_anchor_index = selected_frame_index.saturating_sub(120);
        let seek_anchor_pts = presentation_ticks[seek_anchor_index];

        let mut seek_decoder = MfBgraDecoder::open(input_path)?;
        seek_decoder.seek(seek_anchor_pts)?;
        let mut decoded_after_seek = 0_usize;
        let mut first_after_seek_pts = None;
        let selected = loop {
            let frame = seek_decoder
                .read_next()?
                .ok_or(MediaDecoderError::SeekTargetNotDecoded(selected_pts))?;
            decoded_after_seek += 1;
            first_after_seek_pts.get_or_insert(frame.pts);
            if frame.pts >= selected_pts {
                break frame;
            }
        };
        if selected.pts != selected_pts {
            return Err(MediaDecoderError::SeekSkippedTarget {
                requested: selected_pts,
                decoded: selected.pts,
            });
        }
        if selected.pixels != reference.pixels {
            return Err(MediaDecoderError::SeekPixelMismatch(selected_pts));
        }

        let first_after_seek_pts =
            first_after_seek_pts.ok_or(MediaDecoderError::SeekTargetNotDecoded(selected_pts))?;
        Ok(DecoderSeekProbeReport {
            input_path: input_path.into(),
            indexed_frames: presentation_ticks.len(),
            requested_tick,
            selected_frame_index,
            selected_pts,
            first_after_seek_pts,
            decoded_after_seek,
            selected_frame_bytes: selected.pixels.len(),
            pixels_match_sequential: true,
        })
    }
}

fn sample_time(
    sample: &windows::Win32::Media::MediaFoundation::IMFSample,
    fallback_timestamp: i64,
) -> Result<TimeTick, MediaDecoderError> {
    let pts = unsafe { sample.GetSampleTime() }.unwrap_or(fallback_timestamp);
    if pts < 0 {
        return Err(MediaDecoderError::NegativeSampleTime(pts));
    }
    Ok(TimeTick(pts))
}

fn copy_top_down_bgra(
    buffer: &IMFMediaBuffer,
    width: u32,
    height: u32,
    stride: i32,
) -> Result<Vec<u8>, MediaDecoderError> {
    let row_bytes = width
        .checked_mul(4)
        .ok_or(MediaDecoderError::InvalidOutputGeometry { width, height })?;
    let stride_bytes = stride.unsigned_abs();
    if stride_bytes < row_bytes {
        return Err(MediaDecoderError::InvalidStride { stride, row_bytes });
    }
    let required = stride_bytes
        .checked_mul(height)
        .ok_or(MediaDecoderError::InvalidOutputGeometry { width, height })?;
    let tight_length = row_bytes
        .checked_mul(height)
        .ok_or(MediaDecoderError::InvalidOutputGeometry { width, height })?;
    let mut data = ptr::null_mut();
    let mut current_length = 0_u32;
    stage("IMFMediaBuffer::Lock(decoded RGB32)", unsafe {
        buffer.Lock(&raw mut data, None, Some(&raw mut current_length))
    })?;
    if data.is_null() || current_length < required {
        let _ = unsafe { buffer.Unlock() };
        return Err(MediaDecoderError::InvalidDecodedBuffer {
            current_length,
            required,
        });
    }

    let mut pixels = vec![0_u8; tight_length as usize];
    for output_row in 0..height {
        let input_row = if stride < 0 {
            height - 1 - output_row
        } else {
            output_row
        };
        let source_offset = input_row as usize * stride_bytes as usize;
        let destination_offset = output_row as usize * row_bytes as usize;
        unsafe {
            ptr::copy_nonoverlapping(
                data.add(source_offset),
                pixels.as_mut_ptr().add(destination_offset),
                row_bytes as usize,
            );
        }
    }
    stage("IMFMediaBuffer::Unlock(decoded RGB32)", unsafe {
        buffer.Unlock()
    })?;
    Ok(pixels)
}

fn read_output_geometry(reader: &IMFSourceReader) -> Result<(u32, u32, i32), MediaDecoderError> {
    let media_type = stage("IMFSourceReader::GetCurrentMediaType", unsafe {
        reader.GetCurrentMediaType(VIDEO_STREAM)
    })?;
    let (width, height) = read_frame_size(&media_type)?;
    let stride_key = MF_MT_DEFAULT_STRIDE;
    let fallback_stride = i32::try_from(width.saturating_mul(4))
        .map_err(|_| MediaDecoderError::InvalidOutputGeometry { width, height })?;
    let stride = unsafe { media_type.GetUINT32(&raw const stride_key) }
        .map_or_else(|_| fallback_stride, u32::cast_signed);
    if stride == 0 {
        return Err(MediaDecoderError::InvalidStride {
            stride,
            row_bytes: width.saturating_mul(4),
        });
    }
    Ok((width, height, stride))
}

fn read_frame_size(media_type: &IMFMediaType) -> Result<(u32, u32), MediaDecoderError> {
    let packed_size = get_u64(media_type, MF_MT_FRAME_SIZE)?;
    let width = (packed_size >> 32) as u32;
    let height = packed_size as u32;
    if width == 0 || height == 0 {
        return Err(MediaDecoderError::InvalidOutputGeometry { width, height });
    }
    Ok((width, height))
}

fn preview_geometry(width: u32, height: u32, longest_side: Option<u32>) -> (u32, u32) {
    let Some(limit) = longest_side else {
        return (width, height);
    };
    let longest = width.max(height);
    if longest <= limit.max(2) {
        return (width, height);
    }
    let even = |dimension| {
        let rounded = (u64::from(dimension) * u64::from(limit.max(2)) + u64::from(longest) / 2)
            / u64::from(longest);
        ((rounded as u32) & !1).max(2)
    };
    (even(width), even(height))
}

#[cfg(test)]
mod preview_geometry_tests {
    use super::preview_geometry;
    #[test]
    fn only_explicit_scrub_proxy_reduces_dimensions() {
        assert_eq!(preview_geometry(2560, 1440, None), (2560, 1440));
        assert_eq!(preview_geometry(2560, 1440, Some(960)), (960, 540));
        assert_eq!(preview_geometry(480, 270, Some(960)), (480, 270));
        assert_eq!(preview_geometry(1080, 1920, Some(960)), (540, 960));
    }
}

fn rgb32_media_type(width: u32, height: u32) -> Result<IMFMediaType, MediaDecoderError> {
    let media_type = stage("MFCreateMediaType(RGB32)", unsafe { MFCreateMediaType() })?;
    set_guid(&media_type, MF_MT_MAJOR_TYPE, MFMediaType_Video)?;
    set_guid(&media_type, MF_MT_SUBTYPE, MFVideoFormat_RGB32)?;
    set_u64(&media_type, MF_MT_FRAME_SIZE, pack_pair(width, height))?;
    Ok(media_type)
}

fn create_attributes(capacity: u32) -> Result<IMFAttributes, MediaDecoderError> {
    let mut attributes = None;
    stage("MFCreateAttributes(source reader)", unsafe {
        MFCreateAttributes(&raw mut attributes, capacity)
    })?;
    attributes.ok_or(MediaDecoderError::NullInterface("IMFAttributes"))
}

fn set_guid(attributes: &IMFAttributes, key: GUID, value: GUID) -> Result<(), MediaDecoderError> {
    stage("IMFAttributes::SetGUID", unsafe {
        attributes.SetGUID(&raw const key, &raw const value)
    })
}

fn set_u32(attributes: &IMFAttributes, key: GUID, value: u32) -> Result<(), MediaDecoderError> {
    stage("IMFAttributes::SetUINT32", unsafe {
        attributes.SetUINT32(&raw const key, value)
    })
}

fn get_u64(attributes: &IMFAttributes, key: GUID) -> Result<u64, MediaDecoderError> {
    stage("IMFAttributes::GetUINT64", unsafe {
        attributes.GetUINT64(&raw const key)
    })
}

fn set_u64(attributes: &IMFAttributes, key: GUID, value: u64) -> Result<(), MediaDecoderError> {
    stage("IMFAttributes::SetUINT64", unsafe {
        attributes.SetUINT64(&raw const key, value)
    })
}

const fn pack_pair(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn stage<T>(
    stage_name: &'static str,
    result: windows::core::Result<T>,
) -> Result<T, MediaDecoderError> {
    result.map_err(|source| MediaDecoderError::WindowsStage {
        stage: stage_name,
        source,
    })
}

struct MediaFoundationSession;

impl MediaFoundationSession {
    fn start() -> Result<Self, MediaDecoderError> {
        stage("MFStartup", unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_FULL)
        })?;
        Ok(Self)
    }
}

impl Drop for MediaFoundationSession {
    fn drop(&mut self) {
        let _ = unsafe { MFShutdown() };
    }
}

#[derive(Debug, Error)]
pub enum MediaDecoderError {
    #[error("索引准备已取消")]
    IndexCancelled,
    #[error("input MP4 does not exist or is not a file: {0}")]
    MissingInput(PathBuf),
    #[error("WinRT apartment initialization failed: {0}")]
    Apartment(windows::core::Error),
    #[error("Media Foundation stage '{stage}' failed: {source}")]
    WindowsStage {
        stage: &'static str,
        #[source]
        source: windows::core::Error,
    },
    #[error("Media Foundation returned a null {0} interface")]
    NullInterface(&'static str),
    #[error("decoder output has invalid geometry {width}x{height}")]
    InvalidOutputGeometry { width: u32, height: u32 },
    #[error("decoder output stride {stride} is smaller than its {row_bytes}-byte row")]
    InvalidStride { stride: i32, row_bytes: u32 },
    #[error("decoded buffer length {current_length} is smaller than required {required}")]
    InvalidDecodedBuffer { current_length: u32, required: u32 },
    #[error("source reader reported an error flag (0x{flags:08x})")]
    SourceReaderFlagError { flags: u32 },
    #[error("decoded sample has a negative PTS: {0}")]
    NegativeSampleTime(i64),
    #[error("decoder seek position must not be negative: {0}")]
    NegativeSeek(i64),
    #[error("decoder source reader is closed")]
    ReaderClosed,
    #[error("decoder probe frame count must be positive")]
    InvalidProbeFrameCount,
    #[error("input contains no decodable video frames")]
    NoVideoFrames,
    #[error("decoded PTS index moved backwards or duplicated from {previous:?} to {current:?}")]
    NonMonotonicIndex {
        previous: TimeTick,
        current: TimeTick,
    },
    #[error("decoder ended before seek target {0:?} was decoded")]
    SeekTargetNotDecoded(TimeTick),
    #[error("decoder skipped indexed seek target {requested:?} and returned {decoded:?}")]
    SeekSkippedTarget {
        requested: TimeTick,
        decoded: TimeTick,
    },
    #[error("random seek pixels differ from sequential decode at {0:?}")]
    SeekPixelMismatch(TimeTick),
}
