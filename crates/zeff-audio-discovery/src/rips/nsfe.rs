use super::{
    EntryPoint, FileSpan, MalformedInput, MusicRip, NsfeChunk, RipDetails, RipFormat, RipInspection,
};
use crate::{Budget, ScanStop};

pub(super) const MAX_CHUNKS: usize = 4096;

pub(crate) fn inspect(bytes: &[u8], budget: &mut Budget<'_>) -> Result<RipInspection, ScanStop> {
    if !bytes.starts_with(b"NSFE") {
        return Ok(RipInspection::Unsupported);
    }
    let mut at = 4;
    let mut chunks = Vec::new();
    let mut info = None;
    let mut data = None;
    let mut rate = None;
    let mut bank = None;
    while at < bytes.len() {
        if bytes.len() - at < 8 {
            return Ok(RipInspection::Malformed(MalformedInput::TruncatedChunk));
        }
        budget.charge()?;
        if chunks.len() == MAX_CHUNKS {
            return Err(ScanStop::InventoryLimit);
        }
        let payload_len = u32::from_le_bytes(bytes[at..at + 4].try_into().expect("chunk length"));
        let payload_at = at + 8;
        let Some(end) = payload_at.checked_add(payload_len as usize) else {
            return Ok(RipInspection::Malformed(MalformedInput::TruncatedChunk));
        };
        if end > bytes.len() {
            return Ok(RipInspection::Malformed(MalformedInput::TruncatedChunk));
        }
        let id: [u8; 4] = bytes[at + 4..payload_at].try_into().expect("chunk id");
        let chunk = NsfeChunk {
            id,
            header: span(at, 8),
            payload: span(payload_at, payload_len),
        };
        let payload = &bytes[payload_at..end];
        match &id {
            b"INFO" => {
                if info.is_some() {
                    return Ok(RipInspection::Malformed(MalformedInput::DuplicateChunk));
                }
                if data.is_some() {
                    return Ok(RipInspection::Malformed(MalformedInput::InvalidChunkOrder));
                }
                if payload.len() < 9 {
                    return Ok(RipInspection::Malformed(MalformedInput::InvalidChunkSize));
                }
                charge_decoded(payload.len(), budget)?;
                let song_count = payload[8];
                let raw_start_song = payload.get(9).copied().unwrap_or(0);
                if song_count == 0 {
                    return Ok(RipInspection::Malformed(MalformedInput::InvalidSongCount));
                }
                if raw_start_song >= song_count {
                    return Ok(RipInspection::Malformed(MalformedInput::InvalidFirstSong));
                }
                info = Some((chunk.header, payload, song_count, raw_start_song));
            }
            b"DATA" => {
                if data.is_some() {
                    return Ok(RipInspection::Malformed(MalformedInput::DuplicateChunk));
                }
                if info.is_none() {
                    return Ok(RipInspection::Malformed(MalformedInput::InvalidChunkOrder));
                }
                if payload.is_empty() {
                    return Ok(RipInspection::Unsupported);
                }
                data = Some((chunk.header, chunk.payload));
            }
            b"RATE" => {
                if rate.is_some() {
                    return Ok(RipInspection::Unsupported);
                }
                if !matches!(payload.len(), 2 | 4 | 6) {
                    return Ok(RipInspection::Unsupported);
                }
                charge_decoded(payload.len(), budget)?;
                let values = [
                    Some(period(payload, 0)),
                    (payload.len() >= 4).then(|| period(payload, 2)),
                    (payload.len() == 6).then(|| period(payload, 4)),
                ];
                if values.into_iter().flatten().any(|value| value == 0) {
                    return Ok(RipInspection::Unsupported);
                }
                rate = Some(values);
            }
            b"BANK" => {
                if bank.is_some() {
                    return Ok(RipInspection::Unsupported);
                }
                charge_decoded(payload.len(), budget)?;
                bank = Some((chunk.payload, payload));
            }
            b"NEND" => {
                if !payload.is_empty() || end != bytes.len() {
                    return Ok(RipInspection::Unsupported);
                }
                chunks.push(chunk);
                break;
            }
            b"NSF2" | b"VRC7" => return Ok(RipInspection::Unsupported),
            _ if id[0].is_ascii_uppercase() => return Ok(RipInspection::Unsupported),
            _ => {}
        }
        chunks.push(chunk);
        at = end;
    }
    let Some((info_header, info_payload, song_count, raw_start_song)) = info else {
        return Ok(RipInspection::Malformed(
            MalformedInput::MissingRequiredChunk,
        ));
    };
    let Some((data_header, program)) = data else {
        return Ok(RipInspection::Malformed(
            MalformedInput::MissingRequiredChunk,
        ));
    };
    if !chunks.last().is_some_and(|chunk| chunk.id == *b"NEND") {
        return Ok(RipInspection::Malformed(
            MalformedInput::MissingRequiredChunk,
        ));
    }
    budget.charge()?;
    let sha256 = zeff_firmware::sha256_hex(bytes);
    if budget.cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(ScanStop::Cancelled);
    }
    let mut initial_banks = [0; 8];
    let banking_enabled = bank.is_some();
    let bank_payload = bank.map(|(payload, raw)| {
        let copied = raw.len().min(initial_banks.len());
        initial_banks[..copied].copy_from_slice(&raw[..copied]);
        payload
    });
    let [ntsc_period_us, pal_period_us, dendy_period_us] = rate.unwrap_or([None; 3]);
    Ok(RipInspection::Match(Box::new(MusicRip {
        format: RipFormat::Nsfe,
        version: None,
        title: String::new(),
        author: String::new(),
        copyright: String::new(),
        song_count,
        first_song: raw_start_song + 1,
        source: span(0, bytes.len() as u32),
        sha256,
        header: span(0, 4),
        program,
        opaque_metadata: None,
        load_address: period(info_payload, 0),
        init: EntryPoint {
            cpu_address: period(info_payload, 2),
            initial_source_offset: None,
        },
        play: EntryPoint {
            cpu_address: period(info_payload, 4),
            initial_source_offset: None,
        },
        details: RipDetails::Nsfe {
            chunks,
            info_header,
            data_header,
            raw_start_song,
            info_region_bits: info_payload[6],
            info_expansion_bits: info_payload[7],
            ntsc_period_us,
            pal_period_us,
            dendy_period_us,
            bank_payload,
            initial_banks,
            banking_enabled,
        },
        warnings: Vec::new(),
    })))
}

fn period(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().expect("validated NSFe field"))
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
