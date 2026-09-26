//! Finalize our single-video fMP4 exports with explicit fragment decode times.
//! MF's implicit-time fragments can lose their final partial GOP in the Windows reader.
//! Copy encoded payloads unchanged; recording/recovery never passes through this module.
use super::{
    Fmp4ProbeError, child_boxes, find_default_sample_durations, inspect_moof_timing,
    read_full_box_flags, read_u32, read_u64,
};
use std::{
    fs,
    io::{self, BufReader, BufWriter, Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) fn normalize_export_timing(
    input: &Path,
    output: &Path,
    cancel: &AtomicBool,
) -> Result<(), Fmp4ProbeError> {
    let source = fs::File::open(input)?;
    let length = source.metadata()?.len();
    let mut reader = BufReader::new(source);
    let mut writer = BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?,
    );
    let mut position = 0_u64;
    let mut written = 0_u64;
    let mut defaults = Vec::new();
    let mut decode_times = Vec::new();
    let mut buffer = vec![0_u8; 128 * 1024];
    while position < length {
        check_cancel(cancel)?;
        let mut header = [0_u8; 16];
        reader.read_exact(&mut header[..8])?;
        let size32 = read_u32(&header, 0)?;
        let (size, header_size) = match size32 {
            0 => (length - position, 8),
            1 => {
                reader.read_exact(&mut header[8..])?;
                (read_u64(&header, 8)?, 16)
            }
            size => (u64::from(size), 8),
        };
        if size < header_size as u64 || size > length - position {
            return Err(Fmp4ProbeError::InvalidMediaTiming);
        }
        let kind = &header[4..8];
        if kind == b"moov" || kind == b"moof" {
            // Only metadata is buffered; large encoded mdat payloads stream through.
            if size > 16 * 1024 * 1024 || header_size != 8 {
                return Err(Fmp4ProbeError::InvalidMediaTiming);
            }
            let mut metadata =
                vec![0; usize::try_from(size).map_err(|_| Fmp4ProbeError::InvalidMediaTiming)?];
            metadata[..8].copy_from_slice(&header[..8]);
            reader.read_exact(&mut metadata[8..])?;
            if kind == b"moov" {
                defaults = find_default_sample_durations(&metadata, 8, metadata.len())?;
            } else {
                explicit_fragment_time(
                    &mut metadata,
                    &defaults,
                    &mut decode_times,
                    i128::from(written) - i128::from(position),
                )?;
            }
            writer.write_all(&metadata)?;
            written += metadata.len() as u64;
        } else {
            // Optional random-access footer contains old byte offsets. Our explicit
            // tfdt timestamps support fragment seeking without this optional index.
            let keep = kind != b"mfra";
            if keep {
                writer.write_all(&header[..header_size])?;
                written += size;
            }
            let mut remaining = size - header_size as u64;
            while remaining > 0 {
                check_cancel(cancel)?;
                let chunk = usize::try_from(remaining.min(buffer.len() as u64)).unwrap();
                reader.read_exact(&mut buffer[..chunk])?;
                if keep {
                    writer.write_all(&buffer[..chunk])?;
                }
                remaining -= chunk as u64;
            }
        }
        position += size;
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

fn check_cancel(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "export cancelled",
        ))
    } else {
        Ok(())
    }
}

fn explicit_fragment_time(
    bytes: &mut Vec<u8>,
    defaults: &[(u32, u32)],
    decode_times: &mut Vec<(u32, i128)>,
    shift: i128,
) -> Result<(), Fmp4ProbeError> {
    let tracks: Vec<_> = child_boxes(bytes, 8, bytes.len())?
        .into_iter()
        .filter(|header| &header.kind == b"traf")
        .collect();
    if tracks.len() != 1 {
        return Err(Fmp4ProbeError::InvalidMediaTiming);
    }
    let traf = tracks[0];
    let children = child_boxes(bytes, traf.payload_start, traf.end)?;
    let tfhd = children
        .iter()
        .find(|header| &header.kind == b"tfhd")
        .ok_or(Fmp4ProbeError::InvalidMediaTiming)?;
    let payload = &bytes[tfhd.payload_start..tfhd.end];
    // This helper only accepts the absolute-offset form written by our MF sink.
    if read_full_box_flags(payload)? & 1 == 0 {
        return Err(Fmp4ProbeError::InvalidMediaTiming);
    }
    let track = read_u32(payload, 4)?;
    let base = read_u64(payload, 8)?;
    let start = decode_times
        .iter()
        .find_map(|(id, time)| (*id == track).then_some(*time))
        .unwrap_or(0);
    let insert = !children.iter().any(|header| &header.kind == b"tfdt");
    inspect_moof_timing(bytes, 8, bytes.len(), defaults, decode_times)?;
    let added = if insert { 20 } else { 0 };
    let base = u64::try_from(i128::from(base) + shift + added)
        .map_err(|_| Fmp4ProbeError::InvalidMediaTiming)?;
    bytes[tfhd.payload_start + 8..tfhd.payload_start + 16].copy_from_slice(&base.to_be_bytes());
    if insert {
        let time = u64::try_from(start).map_err(|_| Fmp4ProbeError::InvalidMediaTiming)?;
        let mut tfdt = Vec::from(20_u32.to_be_bytes());
        tfdt.extend_from_slice(b"tfdt");
        tfdt.extend_from_slice(&[1, 0, 0, 0]);
        tfdt.extend_from_slice(&time.to_be_bytes());
        let traf_start = traf.payload_start - 8;
        let traf_size = u32::try_from(traf.end - traf_start + 20)
            .map_err(|_| Fmp4ProbeError::InvalidMediaTiming)?;
        let moof_size =
            u32::try_from(bytes.len() + 20).map_err(|_| Fmp4ProbeError::InvalidMediaTiming)?;
        bytes[..4].copy_from_slice(&moof_size.to_be_bytes());
        bytes[traf_start..traf_start + 4].copy_from_slice(&traf_size.to_be_bytes());
        bytes.splice(tfhd.end..tfhd.end, tfdt);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn boxed(kind: [u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut bytes = u32::try_from(payload.len() + 8)
            .unwrap()
            .to_be_bytes()
            .to_vec();
        bytes.extend_from_slice(&kind);
        bytes.extend_from_slice(payload);
        bytes
    }
    #[test]
    fn explicit_times_preserve_variable_duration_offsets_and_existing_timestamps() {
        let mut header = vec![0, 0, 0, 1];
        header.extend_from_slice(&1_u32.to_be_bytes());
        header.extend_from_slice(&1000_u64.to_be_bytes());
        let mut run = vec![0, 0, 1, 0];
        for value in [2_u32, 1000, 1500] {
            run.extend_from_slice(&value.to_be_bytes());
        }
        let mut children = boxed(*b"tfhd", &header);
        children.extend(boxed(*b"trun", &run));
        let original = boxed(*b"moof", &boxed(*b"traf", &children));
        let mut first = original.clone();
        let mut times = Vec::new();
        explicit_fragment_time(&mut first, &[(1, 1000)], &mut times, 0).unwrap();
        assert_eq!(times, [(1, 2500)]);
        assert_eq!(read_u64(&first, 32).unwrap(), 1020);
        let mut second = original;
        explicit_fragment_time(&mut second, &[(1, 1000)], &mut times, 20).unwrap();
        assert_eq!(times, [(1, 5000)]);
        assert_eq!(read_u64(&second, 52).unwrap(), 2500);
        let before = second.clone();
        explicit_fragment_time(&mut second, &[(1, 1000)], &mut Vec::new(), 0).unwrap();
        assert_eq!(second, before);
    }
}
