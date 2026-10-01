use std::sync::atomic::Ordering;

use super::{
    EntryPoint, FileSpan, MalformedInput, MusicRip, NsfeChunk, RipDetails, RipFormat, RipInspection,
};
use crate::{Budget, ScanStop};

const HEADER_LEN: usize = 0x80;
pub(super) const MAX_CHUNKS: usize = 4096;

pub(crate) fn inspect(bytes: &[u8], budget: &mut Budget<'_>) -> Result<RipInspection, ScanStop> {
    if !bytes.starts_with(b"NESM\x1a\x02") {
        return Ok(RipInspection::Unsupported);
    }
    if bytes.len() < HEADER_LEN {
        return Ok(RipInspection::Malformed(MalformedInput::TruncatedHeader));
    }
    let song_count = bytes[6];
    let first_song = bytes[7];
    if song_count == 0 {
        return Ok(RipInspection::Malformed(MalformedInput::InvalidSongCount));
    }
    if !(1..=song_count).contains(&first_song) {
        return Ok(RipInspection::Malformed(MalformedInput::InvalidFirstSong));
    }
    let raw_flags = bytes[0x7c];
    if raw_flags & 0x0f != 0 {
        return Ok(RipInspection::Unsupported);
    }
    let declared_program_bytes = u32::from_le_bytes([bytes[0x7d], bytes[0x7e], bytes[0x7f], 0]);
    let available = (bytes.len() - HEADER_LEN) as u32;
    let program_len = if declared_program_bytes == 0 {
        available
    } else {
        declared_program_bytes
    };
    if program_len > available {
        return Ok(RipInspection::Malformed(
            MalformedInput::ProgramLengthExceedsSource,
        ));
    }
    if program_len == 0 {
        return Ok(RipInspection::Unsupported);
    }
    let program = span(HEADER_LEN, program_len);
    let opaque_metadata = (program_len < available).then_some(span(
        HEADER_LEN + program_len as usize,
        available - program_len,
    ));
    if raw_flags & 0x80 != 0 && opaque_metadata.is_none() {
        return Ok(RipInspection::Unsupported);
    }
    let mut warnings = Vec::new();
    let title = super::structure::text(bytes, 0x0e, "title", RipFormat::Nsf, &mut warnings);
    let author = super::structure::text(bytes, 0x2e, "author", RipFormat::Nsf, &mut warnings);
    let copyright = super::structure::text(bytes, 0x4e, "copyright", RipFormat::Nsf, &mut warnings);
    budget.charge()?;
    let metadata = match opaque_metadata {
        Some(metadata) => match metadata_chunks(bytes, metadata, budget)? {
            Ok(chunks) => chunks,
            Err(result) => return Ok(result),
        },
        None => Vec::new(),
    };
    if raw_flags & 0x80 != 0 && metadata.is_empty() {
        return Ok(RipInspection::Unsupported);
    }
    budget.charge()?;
    let sha256 = zeff_firmware::sha256_hex(bytes);
    if budget.cancel.load(Ordering::Relaxed) {
        return Err(ScanStop::Cancelled);
    }
    let rates = metadata
        .iter()
        .find(|chunk| chunk.id == *b"RATE")
        .map(|chunk| {
            let at = chunk.payload.offset as usize;
            let raw = &bytes[at..at + chunk.payload.byte_len as usize];
            [
                Some(word(raw, 0)),
                (raw.len() >= 4).then(|| word(raw, 2)),
                (raw.len() == 6).then(|| word(raw, 4)),
            ]
        });
    let [
        rate_ntsc_period_us,
        rate_pal_period_us,
        rate_dendy_period_us,
    ] = rates.unwrap_or([None; 3]);
    Ok(RipInspection::Match(Box::new(MusicRip {
        format: RipFormat::Nsf,
        version: Some(2),
        title,
        author,
        copyright,
        song_count: Some(song_count),
        first_song: Some(first_song),
        source: span(0, bytes.len() as u32),
        sha256,
        header: span(0, HEADER_LEN as u32),
        program,
        opaque_metadata,
        load_address: Some(word(bytes, 8)),
        init: Some(EntryPoint {
            cpu_address: word(bytes, 10),
            initial_source_offset: None,
        }),
        play: Some(EntryPoint {
            cpu_address: word(bytes, 12),
            initial_source_offset: None,
        }),
        details: RipDetails::Nsf2 {
            raw_flags,
            irq_enabled: raw_flags & 0x10 != 0,
            init_non_returning: raw_flags & 0x20 != 0,
            play_suppressed: raw_flags & 0x40 != 0,
            metadata_required: raw_flags & 0x80 != 0,
            declared_program_bytes,
            header_ntsc_period_us: word(bytes, 0x6e),
            header_pal_period_us: word(bytes, 0x78),
            header_region_bits: bytes[0x7a],
            header_expansion_bits: bytes[0x7b],
            initial_banks: bytes[0x70..0x78]
                .try_into()
                .expect("validated NSF2 header width"),
            metadata,
            rate_ntsc_period_us,
            rate_pal_period_us,
            rate_dendy_period_us,
        },
        warnings,
    })))
}

fn metadata_chunks(
    bytes: &[u8],
    metadata: FileSpan,
    budget: &mut Budget<'_>,
) -> Result<Result<Vec<NsfeChunk>, RipInspection>, ScanStop> {
    let start = metadata.offset as usize;
    let end = start + metadata.byte_len as usize;
    if bytes[start..end].starts_with(b"NSFE") {
        return Ok(Err(RipInspection::Malformed(
            MalformedInput::InvalidChunkOrder,
        )));
    }
    let mut at = start;
    let mut chunks = Vec::new();
    let mut rate = false;
    while at < end {
        if end - at < 8 {
            return Ok(Err(RipInspection::Malformed(
                MalformedInput::TruncatedChunk,
            )));
        }
        budget.charge()?;
        if chunks.len() == MAX_CHUNKS {
            return Err(ScanStop::InventoryLimit);
        }
        let payload_len = u32::from_le_bytes(bytes[at..at + 4].try_into().expect("chunk length"));
        let payload_at = at + 8;
        let Some(chunk_end) = payload_at.checked_add(payload_len as usize) else {
            return Ok(Err(RipInspection::Malformed(
                MalformedInput::TruncatedChunk,
            )));
        };
        if chunk_end > end {
            return Ok(Err(RipInspection::Malformed(
                MalformedInput::TruncatedChunk,
            )));
        }
        let id: [u8; 4] = bytes[at + 4..payload_at].try_into().expect("chunk id");
        let chunk = NsfeChunk {
            id,
            header: span(at, 8),
            payload: span(payload_at, payload_len),
        };
        let payload = &bytes[payload_at..chunk_end];
        match &id {
            b"INFO" | b"DATA" | b"BANK" | b"NSF2" => {
                return Ok(Err(RipInspection::Malformed(
                    MalformedInput::InvalidChunkOrder,
                )));
            }
            b"RATE" => {
                if rate || !matches!(payload.len(), 2 | 4 | 6) {
                    return Ok(Err(RipInspection::Unsupported));
                }
                charge_decoded(payload.len(), budget)?;
                if payload.as_chunks::<2>().0.contains(&[0, 0]) {
                    return Ok(Err(RipInspection::Unsupported));
                }
                rate = true;
            }
            b"NEND" => {
                if !payload.is_empty() || chunk_end != end {
                    return Ok(Err(RipInspection::Unsupported));
                }
                chunks.push(chunk);
                return Ok(Ok(chunks));
            }
            b"VRC7" => return Ok(Err(RipInspection::Unsupported)),
            _ if id[0].is_ascii_uppercase() => return Ok(Err(RipInspection::Unsupported)),
            _ => {}
        }
        chunks.push(chunk);
        at = chunk_end;
    }
    Ok(Ok(chunks))
}

fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().expect("validated NSF2 field"))
}

fn span(offset: usize, byte_len: u32) -> FileSpan {
    FileSpan {
        offset: offset as u32,
        byte_len,
    }
}

fn charge_decoded(byte_len: usize, budget: &mut Budget<'_>) -> Result<(), ScanStop> {
    for _ in 0..byte_len.div_ceil(256) {
        budget.charge()?;
    }
    Ok(())
}
