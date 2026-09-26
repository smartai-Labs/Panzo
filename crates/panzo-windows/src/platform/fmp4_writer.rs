use crate::platform::apartment::WinRtApartment;
use panzo_core::TICKS_PER_SECOND;
use std::fmt;
use std::fs;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::Instant;
use thiserror::Error;
#[path = "export_timing.rs"]
mod export_timing;
pub(crate) use export_timing::normalize_export_timing;
use windows::Win32::Media::MediaFoundation::{
    CODECAPI_AVEncCommonRateControlMode, CODECAPI_AVEncMPVGOPSize, IMFAttributes, IMFByteStream,
    IMFMediaBuffer, IMFMediaSink, IMFMediaType, IMFSinkWriter, IMFSinkWriterEx,
    MF_ACCESSMODE_WRITE, MF_FILEFLAGS_NONE, MF_MPEG4SINK_MIN_FRAGMENT_DURATION, MF_MT_AVG_BITRATE,
    MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_MPEG2_PROFILE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_MT_TRANSFER_FUNCTION,
    MF_MT_VIDEO_CHROMA_SITING, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES, MF_MT_YUV_MATRIX,
    MF_OPENMODE_DELETE_IF_EXIST, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
    MF_SINK_WRITER_DISABLE_THROTTLING, MF_VERSION, MFCreateAttributes, MFCreateFMPEG4MediaSink,
    MFCreateFile, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFCreateSinkWriterFromMediaSink, MFMediaType_Video, MFNominalRange_16_235, MFSTARTUP_FULL,
    MFShutdown, MFStartup, MFT_ENUM_HARDWARE_URL_Attribute, MFT_FRIENDLY_NAME_Attribute,
    MFVideoChromaSubsampling_MPEG2, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFVideoInterlace_Progressive, MFVideoPrimaries_BT709, MFVideoTransFunc_709,
    MFVideoTransferMatrix_BT709, eAVEncCommonRateControlMode_UnconstrainedVBR,
    eAVEncH264VProfile_High,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::{GUID, Interface, PCWSTR, PWSTR};

const WIDTH: u32 = 1_920;
const HEIGHT: u32 = 1_080;
const FRAMES_PER_SECOND: u32 = 60;
const TARGET_BITRATE: u32 = 20_000_000;
const MAX_SCREEN_CAPTURE_BITRATE: u32 = 80_000_000;
const EDITOR_GOP_SECONDS: u32 = 1;
const GOP_FRAMES: u32 = FRAMES_PER_SECOND * EDITOR_GOP_SECONDS;
const FRAGMENT_DURATION_TICK: u64 = 2 * TICKS_PER_SECOND as u64;
#[cfg(test)]
const NV12_FRAME_BYTES: u32 = WIDTH * HEIGHT * 3 / 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nv12VideoConfig {
    pub width: u32,
    pub height: u32,
    pub frames_per_second: u32,
    pub target_bitrate: u32,
    pub gop_frames: u32,
}

impl Nv12VideoConfig {
    pub const fn hd1080p60() -> Self {
        Self {
            width: WIDTH,
            height: HEIGHT,
            frames_per_second: FRAMES_PER_SECOND,
            target_bitrate: TARGET_BITRATE,
            gop_frames: GOP_FRAMES,
        }
    }

    pub fn native_screen_60(width: u32, height: u32) -> Result<Self, Fmp4ProbeError> {
        validate_video_dimensions(width, height)?;
        let source_pixels = u64::from(width) * u64::from(height);
        let hd_pixels = u64::from(WIDTH) * u64::from(HEIGHT);
        let scaled = u64::from(TARGET_BITRATE)
            .saturating_mul(source_pixels)
            .div_ceil(hd_pixels);
        let target_bitrate = u32::try_from(scaled.clamp(
            u64::from(TARGET_BITRATE),
            u64::from(MAX_SCREEN_CAPTURE_BITRATE),
        ))
        .unwrap_or(MAX_SCREEN_CAPTURE_BITRATE);
        Ok(Self {
            width,
            height,
            frames_per_second: FRAMES_PER_SECOND,
            target_bitrate,
            gop_frames: GOP_FRAMES,
        })
    }

    fn validate(self) -> Result<Self, Fmp4ProbeError> {
        validate_video_dimensions(self.width, self.height)?;
        if self.frames_per_second == 0 || self.target_bitrate == 0 || self.gop_frames == 0 {
            return Err(Fmp4ProbeError::InvalidEncodingConfig {
                frames_per_second: self.frames_per_second,
                target_bitrate: self.target_bitrate,
            });
        }
        Ok(self)
    }

    fn frame_bytes(self) -> Result<usize, Fmp4ProbeError> {
        nv12_frame_bytes(self.width, self.height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fmp4ProbeReport {
    pub output_path: PathBuf,
    pub frames_written: u32,
    pub duration_tick: i64,
    pub output_bytes: u64,
    pub fragment_count: usize,
    pub encoder_name: String,
    pub hardware_encoder: bool,
    pub elapsed_ms: u128,
    pub finalized: bool,
}

impl fmt::Display for Fmp4ProbeReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Panzo Media Foundation fMP4 writer probe")?;
        writeln!(formatter, "  Output: {}", self.output_path.display())?;
        writeln!(formatter, "  Frames: {}", self.frames_written)?;
        writeln!(formatter, "  Duration tick: {}", self.duration_tick)?;
        writeln!(formatter, "  Output bytes: {}", self.output_bytes)?;
        writeln!(formatter, "  fMP4 fragments: {}", self.fragment_count)?;
        writeln!(
            formatter,
            "  Encoder: {} ({})",
            self.encoder_name,
            if self.hardware_encoder {
                "hardware"
            } else {
                "not identified as hardware"
            }
        )?;
        writeln!(formatter, "  Finalized: {}", self.finalized)?;
        write!(formatter, "  Elapsed: {} ms", self.elapsed_ms)
    }
}

pub struct Fmp4WriterProbe;

pub struct Nv12Fmp4Writer {
    output_path: PathBuf,
    _apartment: WinRtApartment,
    _media_foundation: MediaFoundationSession,
    _byte_stream: IMFByteStream,
    media_sink: MediaSinkSession,
    writer: Option<IMFSinkWriter>,
    encoder: EncoderIdentity,
    config: Nv12VideoConfig,
    frame_bytes: usize,
    frames_written: u32,
    last_sample_end_tick: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fmp4FinalizeReport {
    pub output_path: PathBuf,
    pub frames_written: u32,
    pub duration_tick: i64,
    pub output_bytes: u64,
    pub inspection: Fmp4Inspection,
    pub encoder_name: String,
    pub hardware_encoder: bool,
}

impl Nv12Fmp4Writer {
    pub fn create(output_path: impl AsRef<Path>) -> Result<Self, Fmp4ProbeError> {
        Self::create_with_config(output_path, Nv12VideoConfig::hd1080p60())
    }

    pub fn create_with_config(
        output_path: impl AsRef<Path>,
        config: Nv12VideoConfig,
    ) -> Result<Self, Fmp4ProbeError> {
        let output_path = output_path.as_ref();
        validate_output_path(output_path)?;
        let config = config.validate()?;
        let frame_bytes = config.frame_bytes()?;
        let apartment = WinRtApartment::multi_threaded().map_err(Fmp4ProbeError::Apartment)?;
        let media_foundation = MediaFoundationSession::start()?;
        let output_type = output_media_type(config)?;
        let input_type = input_media_type(config)?;
        let byte_stream = stage("MFCreateFile", unsafe {
            let wide = wide_path(output_path);
            MFCreateFile(
                MF_ACCESSMODE_WRITE,
                MF_OPENMODE_DELETE_IF_EXIST,
                MF_FILEFLAGS_NONE,
                PCWSTR(wide.as_ptr()),
            )
        })?;
        let media_sink = stage("MFCreateFMPEG4MediaSink", unsafe {
            MFCreateFMPEG4MediaSink(&byte_stream, &output_type, None::<&IMFMediaType>)
        })?;
        let media_sink = MediaSinkSession::new(media_sink);
        let writer_attributes = create_attributes(2)?;
        set_u32(
            &writer_attributes,
            MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
            1,
        )?;
        set_u32(&writer_attributes, MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;
        let writer = stage("MFCreateSinkWriterFromMediaSink", unsafe {
            MFCreateSinkWriterFromMediaSink(media_sink.get(), &writer_attributes)
        })?;
        let encoder_attributes = create_attributes(2)?;
        set_u32(
            &encoder_attributes,
            CODECAPI_AVEncCommonRateControlMode,
            eAVEncCommonRateControlMode_UnconstrainedVBR.0 as u32,
        )?;
        set_u32(
            &encoder_attributes,
            CODECAPI_AVEncMPVGOPSize,
            config.gop_frames,
        )?;
        stage("IMFSinkWriter::SetInputMediaType", unsafe {
            writer.SetInputMediaType(0, &input_type, &encoder_attributes)
        })?;
        let encoder = inspect_encoder(&writer)?;
        stage("IMFSinkWriter::BeginWriting", unsafe {
            writer.BeginWriting()
        })?;

        Ok(Self {
            output_path: output_path.into(),
            _apartment: apartment,
            _media_foundation: media_foundation,
            _byte_stream: byte_stream,
            media_sink,
            writer: Some(writer),
            encoder,
            config,
            frame_bytes,
            frames_written: 0,
            last_sample_end_tick: 0,
        })
    }

    pub fn write_nv12(
        &mut self,
        bytes: &[u8],
        sample_time: i64,
        sample_duration: i64,
    ) -> Result<(), Fmp4ProbeError> {
        if bytes.len() != self.frame_bytes {
            return Err(Fmp4ProbeError::InvalidNv12Length {
                actual: bytes.len(),
                required: self.frame_bytes,
            });
        }
        if sample_duration <= 0
            || sample_time < 0
            || (self.frames_written > 0 && sample_time < self.last_sample_end_tick)
        {
            return Err(Fmp4ProbeError::InvalidSampleTiming {
                sample_time,
                sample_duration,
                previous_end: self.last_sample_end_tick,
            });
        }
        let sample = nv12_sample(bytes, sample_time, sample_duration, self.frame_bytes)?;
        let writer = self
            .writer
            .as_ref()
            .ok_or(Fmp4ProbeError::WriterAlreadyFinalized)?;
        stage("IMFSinkWriter::WriteSample", unsafe {
            writer.WriteSample(0, &sample)
        })?;
        self.frames_written += 1;
        self.last_sample_end_tick = sample_time.saturating_add(sample_duration);
        Ok(())
    }

    pub fn encoder_name(&self) -> &str {
        &self.encoder.name
    }

    pub const fn hardware_encoder(&self) -> bool {
        self.encoder.hardware
    }

    pub const fn frames_written(&self) -> u32 {
        self.frames_written
    }

    pub const fn config(&self) -> Nv12VideoConfig {
        self.config
    }

    pub fn finish(mut self) -> Result<Fmp4FinalizeReport, Fmp4ProbeError> {
        let writer = self
            .writer
            .take()
            .ok_or(Fmp4ProbeError::WriterAlreadyFinalized)?;
        stage("IMFSinkWriter::NotifyEndOfSegment", unsafe {
            writer.NotifyEndOfSegment(0)
        })?;
        stage("IMFSinkWriter::Finalize", unsafe { writer.Finalize() })?;
        drop(writer);
        self.media_sink.shutdown()?;
        let output_bytes = fs::metadata(&self.output_path)
            .map_err(|source| Fmp4ProbeError::OutputMetadata {
                path: self.output_path.clone(),
                source,
            })?
            .len();
        let inspection = inspect_fmp4(&self.output_path)?;
        Ok(Fmp4FinalizeReport {
            output_path: self.output_path.clone(),
            frames_written: self.frames_written,
            duration_tick: self.last_sample_end_tick,
            output_bytes,
            inspection,
            encoder_name: self.encoder.name.clone(),
            hardware_encoder: self.encoder.hardware,
        })
    }
}

fn validate_output_path(output_path: &Path) -> Result<(), Fmp4ProbeError> {
    if output_path.extension().and_then(|value| value.to_str()) != Some("mp4") {
        return Err(Fmp4ProbeError::InvalidOutputExtension(output_path.into()));
    }
    let parent = output_path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(Fmp4ProbeError::MissingOutputDirectory(parent.into()));
    }
    Ok(())
}

fn validate_video_dimensions(width: u32, height: u32) -> Result<(), Fmp4ProbeError> {
    if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err(Fmp4ProbeError::InvalidVideoDimensions { width, height });
    }
    nv12_frame_bytes(width, height)?;
    Ok(())
}

fn nv12_frame_bytes(width: u32, height: u32) -> Result<usize, Fmp4ProbeError> {
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(3))
        .map(|bytes| bytes / 2)
        .ok_or(Fmp4ProbeError::FrameSizeOverflow { width, height })?;
    if bytes > u64::from(u32::MAX) {
        return Err(Fmp4ProbeError::FrameSizeOverflow { width, height });
    }
    usize::try_from(bytes).map_err(|_| Fmp4ProbeError::FrameSizeOverflow { width, height })
}

impl Fmp4WriterProbe {
    pub fn write_black_video(
        output_path: impl AsRef<Path>,
        duration_seconds: u32,
    ) -> Result<Fmp4ProbeReport, Fmp4ProbeError> {
        if duration_seconds == 0 {
            return Err(Fmp4ProbeError::InvalidDuration);
        }
        let output_path = output_path.as_ref();
        if output_path.extension().and_then(|value| value.to_str()) != Some("mp4") {
            return Err(Fmp4ProbeError::InvalidOutputExtension(output_path.into()));
        }
        let parent = output_path
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !parent.is_dir() {
            return Err(Fmp4ProbeError::MissingOutputDirectory(parent.into()));
        }

        let _apartment = WinRtApartment::multi_threaded().map_err(Fmp4ProbeError::Apartment)?;
        let _media_foundation = MediaFoundationSession::start()?;
        let config = Nv12VideoConfig::hd1080p60();
        let output_type = output_media_type(config)?;
        let input_type = input_media_type(config)?;
        let byte_stream = stage("MFCreateFile", unsafe {
            let wide = wide_path(output_path);
            MFCreateFile(
                MF_ACCESSMODE_WRITE,
                MF_OPENMODE_DELETE_IF_EXIST,
                MF_FILEFLAGS_NONE,
                PCWSTR(wide.as_ptr()),
            )
        })?;
        let media_sink = stage("MFCreateFMPEG4MediaSink", unsafe {
            MFCreateFMPEG4MediaSink(&byte_stream, &output_type, None::<&IMFMediaType>)
        })?;
        let mut media_sink = MediaSinkSession::new(media_sink);
        let writer_attributes = create_attributes(2)?;
        set_u32(
            &writer_attributes,
            MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
            1,
        )?;
        set_u32(&writer_attributes, MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;
        let writer = stage("MFCreateSinkWriterFromMediaSink", unsafe {
            MFCreateSinkWriterFromMediaSink(media_sink.get(), &writer_attributes)
        })?;
        let encoder_attributes = create_attributes(2)?;
        set_u32(
            &encoder_attributes,
            CODECAPI_AVEncCommonRateControlMode,
            eAVEncCommonRateControlMode_UnconstrainedVBR.0 as u32,
        )?;
        set_u32(&encoder_attributes, CODECAPI_AVEncMPVGOPSize, GOP_FRAMES)?;
        stage("IMFSinkWriter::SetInputMediaType", unsafe {
            writer.SetInputMediaType(0, &input_type, &encoder_attributes)
        })?;
        let encoder = inspect_encoder(&writer)?;
        stage("IMFSinkWriter::BeginWriting", unsafe {
            writer.BeginWriting()
        })?;

        let frame_count = duration_seconds.saturating_mul(FRAMES_PER_SECOND);
        let started = Instant::now();
        for frame_index in 0..frame_count {
            let start_tick = frame_tick(frame_index);
            let end_tick = frame_tick(frame_index + 1);
            let sample = black_nv12_sample(config, start_tick, end_tick - start_tick)?;
            stage("IMFSinkWriter::WriteSample", unsafe {
                writer.WriteSample(0, &sample)
            })?;
        }
        stage("IMFSinkWriter::NotifyEndOfSegment", unsafe {
            writer.NotifyEndOfSegment(0)
        })?;
        stage("IMFSinkWriter::Finalize", unsafe { writer.Finalize() })?;
        drop(writer);
        media_sink.shutdown()?;
        drop(byte_stream);
        let elapsed_ms = started.elapsed().as_millis();
        let output_bytes = fs::metadata(output_path)
            .map_err(|source| Fmp4ProbeError::OutputMetadata {
                path: output_path.into(),
                source,
            })?
            .len();
        let inspection = inspect_fmp4(output_path)?;

        Ok(Fmp4ProbeReport {
            output_path: output_path.into(),
            frames_written: frame_count,
            duration_tick: frame_tick(frame_count),
            output_bytes,
            fragment_count: inspection.fragment_count,
            encoder_name: encoder.name,
            hardware_encoder: encoder.hardware,
            elapsed_ms,
            finalized: true,
        })
    }
}

#[derive(Debug)]
struct EncoderIdentity {
    name: String,
    hardware: bool,
}

fn inspect_encoder(writer: &IMFSinkWriter) -> Result<EncoderIdentity, Fmp4ProbeError> {
    let writer_ex: IMFSinkWriterEx = stage("IMFSinkWriter to IMFSinkWriterEx", writer.cast())?;
    let mut category = GUID::zeroed();
    let mut transform = None;
    stage("IMFSinkWriterEx::GetTransformForStream", unsafe {
        writer_ex.GetTransformForStream(0, 0, Some(&raw mut category), &raw mut transform)
    })?;
    let transform = transform.ok_or(Fmp4ProbeError::NullInterface("encoder IMFTransform"))?;
    let attributes = stage("IMFTransform::GetAttributes", unsafe {
        transform.GetAttributes()
    })?;
    let name = allocated_string(&attributes, MFT_FRIENDLY_NAME_Attribute)
        .unwrap_or_else(|| "unnamed Media Foundation transform".into());
    let hardware = allocated_string(&attributes, MFT_ENUM_HARDWARE_URL_Attribute).is_some();
    Ok(EncoderIdentity { name, hardware })
}

fn allocated_string(attributes: &IMFAttributes, key: GUID) -> Option<String> {
    let mut value = PWSTR::null();
    let mut length = 0;
    if unsafe { attributes.GetAllocatedString(&raw const key, &raw mut value, &raw mut length) }
        .is_err()
    {
        return None;
    }
    let result = if length == 0 || value.is_null() {
        String::new()
    } else {
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(value.0, length as usize) })
    };
    if !value.is_null() {
        unsafe { CoTaskMemFree(Some(value.0.cast())) };
    }
    Some(result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fmp4Inspection {
    pub output_bytes: u64,
    pub retained_bytes: u64,
    pub trailing_bytes: u64,
    pub fragment_count: usize,
    pub media_data_boxes: usize,
    pub media_timescale: u32,
    pub last_video_end_tick: i64,
}

pub fn inspect_fmp4(path: impl AsRef<Path>) -> Result<Fmp4Inspection, Fmp4ProbeError> {
    let path = path.as_ref();
    let file = fs::File::open(path).map_err(|source| Fmp4ProbeError::OutputRead {
        path: path.into(),
        source,
    })?;
    let length = file.metadata()?.len();
    let mut reader = BufReader::new(file);
    let result = inspect_fmp4_reader(&mut reader, length)?;
    if reader.get_ref().metadata()?.len() != length {
        return Err(Fmp4ProbeError::InvalidMediaTiming);
    }
    Ok(result)
}

// The same metadata limit is used by export timing normalization. Encoded mdat
// payloads are never buffered, and fragment metadata is discarded after each pair.
const MAX_INSPECTION_METADATA_BYTES: u64 = 16 * 1024 * 1024;

fn inspect_fmp4_reader(
    reader: &mut (impl Read + Seek),
    length: u64,
) -> Result<Fmp4Inspection, Fmp4ProbeError> {
    let mut position = 0_u64;
    let mut ftyp = false;
    let mut moov_header = None;
    // Find the last complete moov first, retaining compatibility with files whose
    // movie metadata follows fragments, without retaining every fragment offset.
    while let Some(header) = read_top_level_header(reader, position, length)? {
        match &header.kind {
            b"ftyp" => ftyp = true,
            b"moov" => moov_header = Some(header),
            _ => {}
        }
        position = header.end;
    }
    let Some(moov_header) = moov_header else {
        return Err(Fmp4ProbeError::NotFragmentedMp4 {
            ftyp,
            moov: false,
            fragments: 0,
            media_data: 0,
        });
    };
    let moov = read_box_metadata(reader, moov_header)?;
    let media_timescale = find_media_timescale(&moov, 0, moov.len())?;
    let default_durations = find_default_sample_durations(&moov, 0, moov.len())?;
    drop(moov);

    position = 0;
    let mut fragments = 0usize;
    let mut media_data = 0usize;
    let mut retained_bytes = 0_u64;
    let mut pending_moof = None;
    let mut last_video_end = 0_i128;
    let mut track_decode_times = Vec::new();
    while let Some(header) = read_top_level_header(reader, position, length)? {
        match &header.kind {
            b"moof" => {
                if pending_moof.is_some() {
                    return Err(Fmp4ProbeError::InvalidMediaTiming);
                }
                pending_moof = Some(header);
            }
            b"mdat" => {
                if let Some(moof) = pending_moof.take() {
                    let metadata = read_box_metadata(reader, moof)?;
                    // Commit timing only once the complete mdat is within the file.
                    last_video_end = last_video_end.max(inspect_moof_timing(
                        &metadata,
                        0,
                        metadata.len(),
                        &default_durations,
                        &mut track_decode_times,
                    )?);
                    fragments += 1;
                    media_data += 1;
                }
            }
            _ => {}
        }
        position = header.end;
        if pending_moof.is_none() {
            retained_bytes = position;
        }
    }

    if !ftyp || fragments == 0 || media_data == 0 {
        return Err(Fmp4ProbeError::NotFragmentedMp4 {
            ftyp,
            moov: true,
            fragments,
            media_data,
        });
    }
    let last_video_end_tick =
        i64::try_from(last_video_end * i128::from(TICKS_PER_SECOND) / i128::from(media_timescale))
            .map_err(|_| Fmp4ProbeError::InvalidMediaTiming)?;

    Ok(Fmp4Inspection {
        output_bytes: length,
        retained_bytes,
        trailing_bytes: length - retained_bytes,
        fragment_count: fragments,
        media_data_boxes: media_data,
        media_timescale,
        last_video_end_tick,
    })
}

#[derive(Clone, Copy)]
struct StreamBoxHeader {
    kind: [u8; 4],
    payload_start: u64,
    end: u64,
}

fn read_top_level_header(
    reader: &mut (impl Read + Seek),
    position: u64,
    length: u64,
) -> Result<Option<StreamBoxHeader>, Fmp4ProbeError> {
    let remaining = length.saturating_sub(position);
    if remaining < 8 {
        return Ok(None);
    }
    reader.seek(SeekFrom::Start(position))?;
    let mut bytes = [0_u8; 16];
    reader.read_exact(&mut bytes[..8])?;
    let size32 = u32::from_be_bytes(bytes[..4].try_into().unwrap());
    let (size, header_size) = match size32 {
        0 => (remaining, 8),
        1 => {
            if remaining < 16 {
                return Ok(None);
            }
            reader.read_exact(&mut bytes[8..])?;
            (u64::from_be_bytes(bytes[8..].try_into().unwrap()), 16)
        }
        value => (u64::from(value), 8),
    };
    if size < header_size {
        return Err(Fmp4ProbeError::InvalidMediaTiming);
    }
    if size > remaining {
        return Ok(None);
    } // An interrupted final box.
    Ok(Some(StreamBoxHeader {
        kind: bytes[4..8].try_into().unwrap(),
        payload_start: position + header_size,
        end: position + size,
    }))
}

fn read_box_metadata(
    reader: &mut (impl Read + Seek),
    header: StreamBoxHeader,
) -> Result<Vec<u8>, Fmp4ProbeError> {
    let length = header.end - header.payload_start;
    if length > MAX_INSPECTION_METADATA_BYTES {
        return Err(Fmp4ProbeError::MetadataTooLarge(length));
    }
    let mut bytes =
        vec![0; usize::try_from(length).map_err(|_| Fmp4ProbeError::InvalidMediaTiming)?];
    reader.seek(SeekFrom::Start(header.payload_start))?;
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[derive(Debug, Clone, Copy)]
struct Mp4BoxHeader {
    kind: [u8; 4],
    payload_start: usize,
    end: usize,
}

fn parse_box_header(bytes: &[u8], start: usize, limit: usize) -> Option<Mp4BoxHeader> {
    if start.checked_add(8)? > limit {
        return None;
    }
    let size32 = u32::from_be_bytes(bytes.get(start..start + 4)?.try_into().ok()?);
    let kind = bytes.get(start + 4..start + 8)?.try_into().ok()?;
    let (size, header_size) = if size32 == 1 {
        if start.checked_add(16)? > limit {
            return None;
        }
        (
            usize::try_from(u64::from_be_bytes(
                bytes.get(start + 8..start + 16)?.try_into().ok()?,
            ))
            .ok()?,
            16,
        )
    } else if size32 == 0 {
        (limit - start, 8)
    } else {
        (usize::try_from(size32).ok()?, 8)
    };
    if size < header_size {
        return None;
    }
    let end = start.checked_add(size)?;
    if end > limit {
        return None;
    }
    Some(Mp4BoxHeader {
        kind,
        payload_start: start + header_size,
        end,
    })
}

fn child_boxes(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Result<Vec<Mp4BoxHeader>, Fmp4ProbeError> {
    let mut position = start;
    let mut boxes = Vec::new();
    while position < end {
        let header = parse_box_header(bytes, position, end)
            .ok_or(Fmp4ProbeError::MalformedContainer { offset: position })?;
        position = header.end;
        boxes.push(header);
    }
    Ok(boxes)
}

fn find_media_timescale(
    bytes: &[u8],
    moov_start: usize,
    moov_end: usize,
) -> Result<u32, Fmp4ProbeError> {
    for trak in child_boxes(bytes, moov_start, moov_end)?
        .into_iter()
        .filter(|header| &header.kind == b"trak")
    {
        for mdia in child_boxes(bytes, trak.payload_start, trak.end)?
            .into_iter()
            .filter(|header| &header.kind == b"mdia")
        {
            for mdhd in child_boxes(bytes, mdia.payload_start, mdia.end)?
                .into_iter()
                .filter(|header| &header.kind == b"mdhd")
            {
                let payload = &bytes[mdhd.payload_start..mdhd.end];
                let offset = match payload.first() {
                    Some(0) => 12,
                    Some(1) => 20,
                    _ => return Err(Fmp4ProbeError::InvalidMediaTiming),
                };
                let timescale = read_u32(payload, offset)?;
                if timescale > 0 {
                    return Ok(timescale);
                }
            }
        }
    }
    Err(Fmp4ProbeError::InvalidMediaTiming)
}

fn find_default_sample_durations(
    bytes: &[u8],
    moov_start: usize,
    moov_end: usize,
) -> Result<Vec<(u32, u32)>, Fmp4ProbeError> {
    let mut result = Vec::new();
    for mvex in child_boxes(bytes, moov_start, moov_end)?
        .into_iter()
        .filter(|header| &header.kind == b"mvex")
    {
        for trex in child_boxes(bytes, mvex.payload_start, mvex.end)?
            .into_iter()
            .filter(|header| &header.kind == b"trex")
        {
            let payload = &bytes[trex.payload_start..trex.end];
            result.push((read_u32(payload, 4)?, read_u32(payload, 12)?));
        }
    }
    Ok(result)
}

fn inspect_moof_timing(
    bytes: &[u8],
    moof_start: usize,
    moof_end: usize,
    trex_durations: &[(u32, u32)],
    track_decode_times: &mut Vec<(u32, i128)>,
) -> Result<i128, Fmp4ProbeError> {
    let mut maximum_end = 0_i128;
    for traf in child_boxes(bytes, moof_start, moof_end)?
        .into_iter()
        .filter(|header| &header.kind == b"traf")
    {
        let children = child_boxes(bytes, traf.payload_start, traf.end)?;
        let tfhd = children
            .iter()
            .find(|header| &header.kind == b"tfhd")
            .ok_or(Fmp4ProbeError::InvalidMediaTiming)?;
        let tfhd_payload = &bytes[tfhd.payload_start..tfhd.end];
        let flags = read_full_box_flags(tfhd_payload)?;
        let track_id = read_u32(tfhd_payload, 4)?;
        let mut tfhd_offset = 8;
        if flags & 0x0000_0001 != 0 {
            tfhd_offset += 8;
        }
        if flags & 0x0000_0002 != 0 {
            tfhd_offset += 4;
        }
        let default_duration = if flags & 0x0000_0008 != 0 {
            read_u32(tfhd_payload, tfhd_offset)?
        } else {
            trex_durations
                .iter()
                .find_map(|(id, duration)| (*id == track_id).then_some(*duration))
                .ok_or(Fmp4ProbeError::InvalidMediaTiming)?
        };
        let mut decode_time =
            if let Some(tfdt) = children.iter().find(|header| &header.kind == b"tfdt") {
                let tfdt_payload = &bytes[tfdt.payload_start..tfdt.end];
                match tfdt_payload.first() {
                    Some(0) => i128::from(read_u32(tfdt_payload, 4)?),
                    Some(1) => i128::from(read_u64(tfdt_payload, 4)?),
                    _ => return Err(Fmp4ProbeError::InvalidMediaTiming),
                }
            } else {
                track_decode_times
                    .iter()
                    .find_map(|(id, time)| (*id == track_id).then_some(*time))
                    .unwrap_or(0)
            };
        for trun in children.iter().filter(|header| &header.kind == b"trun") {
            let (end, maximum) = inspect_run_timing(
                &bytes[trun.payload_start..trun.end],
                default_duration,
                decode_time,
            )?;
            decode_time = end;
            maximum_end = maximum_end.max(maximum);
        }
        if let Some((_, time)) = track_decode_times
            .iter_mut()
            .find(|(id, _)| *id == track_id)
        {
            *time = decode_time;
        } else {
            if track_decode_times.len() >= 1_024 {
                return Err(Fmp4ProbeError::InvalidMediaTiming);
            }
            track_decode_times.push((track_id, decode_time));
        }
    }
    Ok(maximum_end)
}

fn inspect_run_timing(
    payload: &[u8],
    default_duration: u32,
    mut decode_time: i128,
) -> Result<(i128, i128), Fmp4ProbeError> {
    let mut maximum_end = 0_i128;
    let version = *payload.first().ok_or(Fmp4ProbeError::InvalidMediaTiming)?;
    if version > 1 {
        return Err(Fmp4ProbeError::InvalidMediaTiming);
    }
    let trun_flags = read_full_box_flags(payload)?;
    let sample_count = read_u32(payload, 4)?;
    let mut offset = 8_usize;
    if trun_flags & 0x0000_0001 != 0 {
        offset += 4;
    }
    if trun_flags & 0x0000_0004 != 0 {
        offset += 4;
    }
    let per_sample_bytes = [0x100, 0x200, 0x400, 0x800]
        .into_iter()
        .filter(|flag| trun_flags & flag != 0)
        .count()
        * 4;
    let required = usize::try_from(sample_count)
        .ok()
        .and_then(|count| count.checked_mul(per_sample_bytes))
        .and_then(|bytes| offset.checked_add(bytes))
        .ok_or(Fmp4ProbeError::InvalidMediaTiming)?;
    if required > payload.len() {
        return Err(Fmp4ProbeError::InvalidMediaTiming);
    }
    if sample_count == 0 {
        return Ok((decode_time, 0));
    }
    if per_sample_bytes == 0 {
        // A compact constant-duration run may declare billions of samples.
        // Compute its end directly instead of spending time per sample.
        decode_time += i128::from(sample_count) * i128::from(default_duration);
        return Ok((decode_time, decode_time));
    }
    for _ in 0..sample_count {
        let duration = if trun_flags & 0x0000_0100 != 0 {
            let value = read_u32(payload, offset)?;
            offset += 4;
            value
        } else {
            default_duration
        };
        if trun_flags & 0x0000_0200 != 0 {
            offset += 4;
        }
        if trun_flags & 0x0000_0400 != 0 {
            offset += 4;
        }
        let composition_offset = if trun_flags & 0x0000_0800 != 0 {
            let raw = read_u32(payload, offset)?;
            offset += 4;
            if version == 1 {
                i128::from(i32::from_be_bytes(raw.to_be_bytes()))
            } else {
                i128::from(raw)
            }
        } else {
            0
        };
        maximum_end = maximum_end.max(decode_time + composition_offset + i128::from(duration));
        decode_time += i128::from(duration);
    }
    Ok((decode_time, maximum_end))
}

fn read_full_box_flags(payload: &[u8]) -> Result<u32, Fmp4ProbeError> {
    let bytes: [u8; 4] = payload
        .get(0..4)
        .ok_or(Fmp4ProbeError::InvalidMediaTiming)?
        .try_into()
        .map_err(|_| Fmp4ProbeError::InvalidMediaTiming)?;
    Ok(u32::from_be_bytes(bytes) & 0x00ff_ffff)
}

fn read_u32(payload: &[u8], offset: usize) -> Result<u32, Fmp4ProbeError> {
    payload
        .get(offset..offset + 4)
        .ok_or(Fmp4ProbeError::InvalidMediaTiming)?
        .try_into()
        .map(u32::from_be_bytes)
        .map_err(|_| Fmp4ProbeError::InvalidMediaTiming)
}

fn read_u64(payload: &[u8], offset: usize) -> Result<u64, Fmp4ProbeError> {
    payload
        .get(offset..offset + 8)
        .ok_or(Fmp4ProbeError::InvalidMediaTiming)?
        .try_into()
        .map(u64::from_be_bytes)
        .map_err(|_| Fmp4ProbeError::InvalidMediaTiming)
}

fn output_media_type(config: Nv12VideoConfig) -> Result<IMFMediaType, Fmp4ProbeError> {
    let media_type = stage("MFCreateMediaType(output)", unsafe { MFCreateMediaType() })?;
    set_guid(&media_type, MF_MT_MAJOR_TYPE, MFMediaType_Video)?;
    set_guid(&media_type, MF_MT_SUBTYPE, MFVideoFormat_H264)?;
    set_u32(&media_type, MF_MT_AVG_BITRATE, config.target_bitrate)?;
    set_u32(
        &media_type,
        MF_MT_INTERLACE_MODE,
        MFVideoInterlace_Progressive.0 as u32,
    )?;
    set_u32(
        &media_type,
        MF_MT_MPEG2_PROFILE,
        eAVEncH264VProfile_High.0 as u32,
    )?;
    set_u64(
        &media_type,
        MF_MPEG4SINK_MIN_FRAGMENT_DURATION,
        FRAGMENT_DURATION_TICK,
    )?;
    set_video_geometry(&media_type, config)?;
    set_video_colorimetry(&media_type)?;
    Ok(media_type)
}

fn input_media_type(config: Nv12VideoConfig) -> Result<IMFMediaType, Fmp4ProbeError> {
    let media_type = stage("MFCreateMediaType(input)", unsafe { MFCreateMediaType() })?;
    set_guid(&media_type, MF_MT_MAJOR_TYPE, MFMediaType_Video)?;
    set_guid(&media_type, MF_MT_SUBTYPE, MFVideoFormat_NV12)?;
    set_u32(
        &media_type,
        MF_MT_INTERLACE_MODE,
        MFVideoInterlace_Progressive.0 as u32,
    )?;
    set_video_geometry(&media_type, config)?;
    set_video_colorimetry(&media_type)?;
    Ok(media_type)
}

fn set_video_colorimetry(media_type: &IMFMediaType) -> Result<(), Fmp4ProbeError> {
    // Nv12VideoProcessor converts screen pixels to SDR studio-range BT.709.
    // Tag both raw input and H.264 output so playback
    // uses the same matrix/range instead of inferring them from frame dimensions.
    for (key, value) in [
        (MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0),
        (MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0),
        (MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0),
        (MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0),
        (MF_MT_VIDEO_CHROMA_SITING, MFVideoChromaSubsampling_MPEG2.0),
    ] {
        set_u32(media_type, key, value.cast_unsigned())?;
    }
    Ok(())
}

fn set_video_geometry(
    media_type: &IMFMediaType,
    config: Nv12VideoConfig,
) -> Result<(), Fmp4ProbeError> {
    set_u64(
        media_type,
        MF_MT_FRAME_SIZE,
        pack_pair(config.width, config.height),
    )?;
    set_u64(
        media_type,
        MF_MT_FRAME_RATE,
        pack_pair(config.frames_per_second, 1),
    )?;
    set_u64(media_type, MF_MT_PIXEL_ASPECT_RATIO, pack_pair(1, 1))
}

fn create_attributes(capacity: u32) -> Result<IMFAttributes, Fmp4ProbeError> {
    let mut attributes = None;
    stage("MFCreateAttributes", unsafe {
        MFCreateAttributes(&raw mut attributes, capacity)
    })?;
    attributes.ok_or(Fmp4ProbeError::NullInterface("IMFAttributes"))
}

fn black_nv12_sample(
    config: Nv12VideoConfig,
    sample_time: i64,
    sample_duration: i64,
) -> Result<windows::Win32::Media::MediaFoundation::IMFSample, Fmp4ProbeError> {
    let frame_bytes = config.frame_bytes()?;
    let buffer = stage("MFCreateMemoryBuffer", unsafe {
        MFCreateMemoryBuffer(u32::try_from(frame_bytes).map_err(|_| {
            Fmp4ProbeError::FrameSizeOverflow {
                width: config.width,
                height: config.height,
            }
        })?)
    })?;
    fill_black_nv12(&buffer, config)?;
    stage("IMFMediaBuffer::SetCurrentLength", unsafe {
        buffer.SetCurrentLength(u32::try_from(frame_bytes).map_err(|_| {
            Fmp4ProbeError::FrameSizeOverflow {
                width: config.width,
                height: config.height,
            }
        })?)
    })?;
    let sample = stage("MFCreateSample", unsafe { MFCreateSample() })?;
    stage("IMFSample::AddBuffer", unsafe { sample.AddBuffer(&buffer) })?;
    stage("IMFSample::SetSampleTime", unsafe {
        sample.SetSampleTime(sample_time)
    })?;
    stage("IMFSample::SetSampleDuration", unsafe {
        sample.SetSampleDuration(sample_duration)
    })?;
    Ok(sample)
}

fn nv12_sample(
    bytes: &[u8],
    sample_time: i64,
    sample_duration: i64,
    frame_bytes: usize,
) -> Result<windows::Win32::Media::MediaFoundation::IMFSample, Fmp4ProbeError> {
    let frame_bytes_u32 =
        u32::try_from(frame_bytes).map_err(|_| Fmp4ProbeError::InvalidNv12Length {
            actual: frame_bytes,
            required: u32::MAX as usize,
        })?;
    let buffer = stage("MFCreateMemoryBuffer", unsafe {
        MFCreateMemoryBuffer(frame_bytes_u32)
    })?;
    let mut destination = ptr::null_mut();
    let mut capacity = 0;
    stage("IMFMediaBuffer::Lock", unsafe {
        buffer.Lock(&raw mut destination, Some(&raw mut capacity), None)
    })?;
    if destination.is_null() || capacity < frame_bytes_u32 {
        let _ = unsafe { buffer.Unlock() };
        return Err(Fmp4ProbeError::InvalidBuffer {
            capacity,
            required: frame_bytes_u32,
        });
    }
    unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len()) };
    stage("IMFMediaBuffer::Unlock", unsafe { buffer.Unlock() })?;
    stage("IMFMediaBuffer::SetCurrentLength", unsafe {
        buffer.SetCurrentLength(frame_bytes_u32)
    })?;
    let sample = stage("MFCreateSample", unsafe { MFCreateSample() })?;
    stage("IMFSample::AddBuffer", unsafe { sample.AddBuffer(&buffer) })?;
    stage("IMFSample::SetSampleTime", unsafe {
        sample.SetSampleTime(sample_time)
    })?;
    stage("IMFSample::SetSampleDuration", unsafe {
        sample.SetSampleDuration(sample_duration)
    })?;
    Ok(sample)
}

fn fill_black_nv12(buffer: &IMFMediaBuffer, config: Nv12VideoConfig) -> Result<(), Fmp4ProbeError> {
    let frame_bytes = config.frame_bytes()?;
    let frame_bytes_u32 =
        u32::try_from(frame_bytes).map_err(|_| Fmp4ProbeError::FrameSizeOverflow {
            width: config.width,
            height: config.height,
        })?;
    let mut data = ptr::null_mut();
    let mut capacity = 0;
    stage("IMFMediaBuffer::Lock", unsafe {
        buffer.Lock(&raw mut data, Some(&raw mut capacity), None)
    })?;
    if data.is_null() || capacity < frame_bytes_u32 {
        let _ = unsafe { buffer.Unlock() };
        return Err(Fmp4ProbeError::InvalidBuffer {
            capacity,
            required: frame_bytes_u32,
        });
    }
    let luma_bytes =
        usize::try_from(u64::from(config.width) * u64::from(config.height)).map_err(|_| {
            Fmp4ProbeError::FrameSizeOverflow {
                width: config.width,
                height: config.height,
            }
        })?;
    unsafe {
        ptr::write_bytes(data, 16, luma_bytes);
        ptr::write_bytes(data.add(luma_bytes), 128, frame_bytes - luma_bytes);
    }
    stage("IMFMediaBuffer::Unlock", unsafe { buffer.Unlock() })
}

fn set_guid(attributes: &IMFAttributes, key: GUID, value: GUID) -> Result<(), Fmp4ProbeError> {
    stage("IMFAttributes::SetGUID", unsafe {
        attributes.SetGUID(&raw const key, &raw const value)
    })
}

fn set_u32(attributes: &IMFAttributes, key: GUID, value: u32) -> Result<(), Fmp4ProbeError> {
    stage("IMFAttributes::SetUINT32", unsafe {
        attributes.SetUINT32(&raw const key, value)
    })
}

fn set_u64(attributes: &IMFAttributes, key: GUID, value: u64) -> Result<(), Fmp4ProbeError> {
    stage("IMFAttributes::SetUINT64", unsafe {
        attributes.SetUINT64(&raw const key, value)
    })
}

const fn pack_pair(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

fn frame_tick(frame_index: u32) -> i64 {
    i64::from(frame_index) * TICKS_PER_SECOND / i64::from(FRAMES_PER_SECOND)
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn stage<T>(stage: &'static str, result: windows::core::Result<T>) -> Result<T, Fmp4ProbeError> {
    result.map_err(|source| Fmp4ProbeError::WindowsStage { stage, source })
}

struct MediaFoundationSession;

impl MediaFoundationSession {
    fn start() -> Result<Self, Fmp4ProbeError> {
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

struct MediaSinkSession(Option<IMFMediaSink>);

impl MediaSinkSession {
    fn new(media_sink: IMFMediaSink) -> Self {
        Self(Some(media_sink))
    }

    fn get(&self) -> &IMFMediaSink {
        self.0.as_ref().expect("media sink is active")
    }

    fn shutdown(&mut self) -> Result<(), Fmp4ProbeError> {
        if let Some(media_sink) = self.0.take() {
            stage("IMFMediaSink::Shutdown", unsafe { media_sink.Shutdown() })?;
        }
        Ok(())
    }
}

impl Drop for MediaSinkSession {
    fn drop(&mut self) {
        if let Some(media_sink) = self.0.take() {
            let _ = unsafe { media_sink.Shutdown() };
        }
    }
}

#[derive(Debug, Error)]
pub enum Fmp4ProbeError {
    #[error("MP4 metadata box exceeds the 16 MiB inspection budget: {0} bytes")]
    MetadataTooLarge(u64),
    #[error("export timing finalization failed: {0}")]
    ExportTimingIo(#[from] std::io::Error),
    #[error("duration must be at least one second")]
    InvalidDuration,
    #[error("fMP4 output path must end in .mp4: {0}")]
    InvalidOutputExtension(PathBuf),
    #[error("fMP4 output directory does not exist: {0}")]
    MissingOutputDirectory(PathBuf),
    #[error("NV12 video dimensions must be non-zero and even, got {width}x{height}")]
    InvalidVideoDimensions { width: u32, height: u32 },
    #[error("NV12 frame size overflows the Media Foundation buffer for {width}x{height}")]
    FrameSizeOverflow { width: u32, height: u32 },
    #[error(
        "encoding FPS and target bitrate must be positive, got {frames_per_second} FPS and {target_bitrate} bps"
    )]
    InvalidEncodingConfig {
        frames_per_second: u32,
        target_bitrate: u32,
    },
    #[error("Windows Runtime apartment initialization failed: {0}")]
    Apartment(windows::core::Error),
    #[error("Media Foundation stage '{stage}' failed: {source}")]
    WindowsStage {
        stage: &'static str,
        source: windows::core::Error,
    },
    #[error("Media Foundation returned a null {0} interface")]
    NullInterface(&'static str),
    #[error("Media Foundation buffer has capacity {capacity}, required {required}")]
    InvalidBuffer { capacity: u32, required: u32 },
    #[error("NV12 sample has {actual} bytes, required {required}")]
    InvalidNv12Length { actual: usize, required: usize },
    #[error(
        "invalid media sample timing: time={sample_time}, duration={sample_duration}, previousEnd={previous_end}"
    )]
    InvalidSampleTiming {
        sample_time: i64,
        sample_duration: i64,
        previous_end: i64,
    },
    #[error("fMP4 writer was already finalized")]
    WriterAlreadyFinalized,
    #[error("could not inspect fMP4 output {path}: {source}")]
    OutputMetadata {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not read fMP4 output {path}: {source}")]
    OutputRead {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("malformed MP4 box at byte offset {offset}")]
    MalformedContainer { offset: usize },
    #[error("fMP4 media timing metadata is missing or invalid")]
    InvalidMediaTiming,
    #[error(
        "output is not fragmented MP4 (ftyp={ftyp}, moov={moov}, moof={fragments}, mdat={media_data})"
    )]
    NotFragmentedMp4 {
        ftyp: bool,
        moov: bool,
        fragments: usize,
        media_data: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use panzo_core::{TimeTick, VideoFrameRateDecision, VideoFrameRateLimiter};

    fn boxed(kind: [u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut bytes = u32::try_from(payload.len() + 8)
            .unwrap()
            .to_be_bytes()
            .to_vec();
        bytes.extend(kind);
        bytes.extend(payload);
        bytes
    }

    fn inspection_fixture(sample_count: u32) -> (Vec<u8>, Vec<u8>) {
        let mut mdhd = vec![0; 12];
        mdhd.extend(1000_u32.to_be_bytes());
        let track_box = boxed(*b"trak", &boxed(*b"mdia", &boxed(*b"mdhd", &mdhd)));
        let mut trex = vec![0; 4];
        for value in [1_u32, 1, 1000, 0, 0] {
            trex.extend(value.to_be_bytes());
        }
        let mut moov = track_box;
        moov.extend(boxed(*b"mvex", &boxed(*b"trex", &trex)));
        let mut prefix = boxed(*b"ftyp", b"isom");
        prefix.extend(boxed(*b"moov", &moov));
        let mut tfhd = vec![0; 4];
        tfhd.extend(1_u32.to_be_bytes());
        let mut trun = vec![0; 4];
        trun.extend(sample_count.to_be_bytes());
        let mut traf = boxed(*b"tfhd", &tfhd);
        traf.extend(boxed(*b"trun", &trun));
        (prefix, boxed(*b"moof", &boxed(*b"traf", &traf)))
    }

    #[test]
    fn streaming_inspection_skips_multi_gigabyte_payloads() {
        struct HeaderOnlyReader {
            bytes: Vec<u8>,
            position: u64,
            reads: usize,
        }
        impl Read for HeaderOnlyReader {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                let start = usize::try_from(self.position).unwrap();
                assert!(
                    start < self.bytes.len(),
                    "attempted to read encoded payload"
                );
                let count = buffer.len().min(self.bytes.len() - start);
                buffer[..count].copy_from_slice(&self.bytes[start..start + count]);
                self.position += count as u64;
                self.reads += count;
                Ok(count)
            }
        }
        impl Seek for HeaderOnlyReader {
            fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
                let SeekFrom::Start(position) = position else {
                    panic!("absolute seeks expected");
                };
                self.position = position;
                Ok(position)
            }
        }
        let (mut bytes, moof) = inspection_fixture(1);
        bytes.extend(moof);
        let mdat_size = 5 * 1024 * 1024 * 1024_u64;
        let length = bytes.len() as u64 + mdat_size;
        bytes.extend(1_u32.to_be_bytes());
        bytes.extend(b"mdat");
        bytes.extend(mdat_size.to_be_bytes());
        let mut reader = HeaderOnlyReader {
            bytes,
            position: 0,
            reads: 0,
        };
        let report = inspect_fmp4_reader(&mut reader, length).unwrap();
        assert_eq!(report.retained_bytes, length);
        assert_eq!(report.last_video_end_tick, TICKS_PER_SECOND);
        assert_eq!(report.fragment_count, 1);
        assert!(reader.reads < 1024);
    }

    #[test]
    fn streaming_inspection_excludes_unfinished_fragment_timing() {
        let (mut bytes, moof) = inspection_fixture(1);
        bytes.extend(&moof);
        bytes.extend(boxed(*b"mdat", &[0; 4]));
        let retained = bytes.len() as u64;
        bytes.extend(&moof);
        bytes.extend(100_u32.to_be_bytes());
        bytes.extend(b"mdat");
        bytes.extend([0; 3]);
        let length = bytes.len() as u64;
        let report = inspect_fmp4_reader(&mut std::io::Cursor::new(bytes), length).unwrap();
        assert_eq!(report.retained_bytes, retained);
        assert_eq!(report.trailing_bytes, length - retained);
        assert_eq!(report.fragment_count, 1);
        assert_eq!(report.last_video_end_tick, TICKS_PER_SECOND);
    }

    #[test]
    fn streaming_inspection_limits_metadata_and_validates_sample_fields() {
        let mut empty = std::io::Cursor::new(Vec::<u8>::new());
        assert!(matches!(
            read_box_metadata(
                &mut empty,
                StreamBoxHeader {
                    kind: *b"moov",
                    payload_start: 0,
                    end: MAX_INSPECTION_METADATA_BYTES + 1,
                }
            ),
            Err(Fmp4ProbeError::MetadataTooLarge(_))
        ));
        let (mut bytes, mut moof) = inspection_fixture(2);
        // Mark durations as present, but retain the short trun payload.
        let flags_offset = moof.windows(4).position(|bytes| bytes == b"trun").unwrap() + 4;
        moof[flags_offset + 2] = 1;
        bytes.extend(moof);
        bytes.extend(boxed(*b"mdat", &[0; 4]));
        let length = bytes.len() as u64;
        assert!(matches!(
            inspect_fmp4_reader(&mut std::io::Cursor::new(bytes), length),
            Err(Fmp4ProbeError::InvalidMediaTiming)
        ));
    }

    #[test]
    fn streaming_inspection_handles_compact_long_runs_without_per_sample_work() {
        let (mut bytes, moof) = inspection_fixture(u32::MAX);
        bytes.extend(moof);
        bytes.extend(boxed(*b"mdat", &[]));
        let length = bytes.len() as u64;
        let report = inspect_fmp4_reader(&mut std::io::Cursor::new(bytes), length).unwrap();
        assert_eq!(
            report.last_video_end_tick,
            i64::from(u32::MAX) * TICKS_PER_SECOND
        );
    }

    #[test]
    fn streaming_inspection_preserves_signed_offsets_and_empty_runs() {
        let mut run = vec![1, 0, 9, 0]; // duration + signed composition offset
        run.extend(2_u32.to_be_bytes());
        for value in [
            1000_u32,
            u32::from_be_bytes((-500_i32).to_be_bytes()),
            500,
            200,
        ] {
            run.extend(value.to_be_bytes());
        }
        assert_eq!(inspect_run_timing(&run, 1, 4000).unwrap(), (5500, 5700));
        assert_eq!(inspect_run_timing(&[0; 8], 1000, 9000).unwrap(), (9000, 0));
    }

    #[test]
    fn frame_time_accumulation_is_exact_at_one_second() {
        assert_eq!(frame_tick(60), TICKS_PER_SECOND);
        assert_eq!(frame_tick(1), 166_666);
        assert_eq!(frame_tick(2), 333_333);
    }

    #[test]
    fn media_foundation_pairs_use_high_low_layout() {
        assert_eq!(pack_pair(1_920, 1_080), 0x0000_0780_0000_0438);
    }

    #[test]
    fn native_screen_config_scales_bitrate_and_frame_size() {
        let config = Nv12VideoConfig::native_screen_60(2_560, 1_440).unwrap();
        assert_eq!(config.target_bitrate, 35_555_556);
        assert_eq!(config.frame_bytes().unwrap(), 5_529_600);
        assert!(matches!(
            Nv12VideoConfig::native_screen_60(1_919, 1_080),
            Err(Fmp4ProbeError::InvalidVideoDimensions { .. })
        ));
    }

    #[test]
    fn rejects_non_mp4_box_data() {
        let path =
            std::env::temp_dir().join(format!("panzo-invalid-fmp4-{}.mp4", std::process::id()));
        fs::write(&path, b"not an mp4").unwrap();
        assert!(matches!(
            inspect_fmp4(&path),
            Err(Fmp4ProbeError::MalformedContainer { .. } | Fmp4ProbeError::NotFragmentedMp4 { .. })
        ));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn reusable_writer_accepts_timestamped_nv12_samples() {
        let path = std::env::temp_dir().join(format!(
            "panzo-reusable-fmp4-{}-{}.mp4",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let mut frame = vec![128_u8; NV12_FRAME_BYTES as usize];
        frame[..(WIDTH * HEIGHT) as usize].fill(16);
        let mut writer = Nv12Fmp4Writer::create(&path).unwrap();
        writer.write_nv12(&frame, 0, 5_000_000).unwrap();
        writer.write_nv12(&frame, 5_000_000, 5_000_000).unwrap();
        let report = writer.finish().unwrap();
        assert_eq!(report.frames_written, 2);
        assert_eq!(report.inspection.last_video_end_tick, TICKS_PER_SECOND);
        assert!(report.hardware_encoder);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn reusable_writer_accepts_native_1440p_samples() {
        let path = std::env::temp_dir().join(format!(
            "panzo-native-1440p-fmp4-{}-{}.mp4",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let config = Nv12VideoConfig::native_screen_60(2_560, 1_440).unwrap();
        let mut frame = vec![128_u8; config.frame_bytes().unwrap()];
        frame[..2_560 * 1_440].fill(16);
        let mut writer = Nv12Fmp4Writer::create_with_config(&path, config).unwrap();
        assert_eq!(writer.config(), config);
        writer.write_nv12(&frame, 0, 5_000_000).unwrap();
        writer.write_nv12(&frame, 5_000_000, 5_000_000).unwrap();
        let report = writer.finish().unwrap();
        assert_eq!(report.frames_written, 2);
        assert!(report.hardware_encoder);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn reusable_writer_accepts_five_seconds_of_rate_limited_100_hz_vfr_samples() {
        let path = std::env::temp_dir().join(format!(
            "panzo-vfr-fmp4-{}-{}.mp4",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let mut frame = vec![128_u8; NV12_FRAME_BYTES as usize];
        frame[..(WIDTH * HEIGHT) as usize].fill(16);
        let mut limiter = VideoFrameRateLimiter::new(60).unwrap();
        let selected_ticks: Vec<_> = (0..500_i64)
            .map(|frame_index| TimeTick(frame_index * TICKS_PER_SECOND / 100))
            .filter(|tick| {
                matches!(
                    limiter.observe(*tick).unwrap(),
                    VideoFrameRateDecision::Accept(_)
                )
            })
            .collect();
        assert_eq!(selected_ticks.len(), 300);

        let mut writer = Nv12Fmp4Writer::create(&path).unwrap();
        for (index, tick) in selected_ticks.iter().enumerate() {
            let end_tick = selected_ticks
                .get(index + 1)
                .copied()
                .unwrap_or(TimeTick(5 * TICKS_PER_SECOND));
            writer
                .write_nv12(&frame, tick.0, end_tick.0 - tick.0)
                .unwrap();
        }
        let report = writer.finish().unwrap();
        assert_eq!(report.frames_written, 300);
        assert_eq!(report.inspection.last_video_end_tick, 5 * TICKS_PER_SECOND);
        assert!(report.hardware_encoder);
        fs::remove_file(path).unwrap();
    }
}
