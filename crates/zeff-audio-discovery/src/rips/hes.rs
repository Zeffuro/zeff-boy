use std::sync::atomic::Ordering;

use super::{EntryPoint, FileSpan, MalformedInput, MusicRip, RipDetails, RipFormat, RipInspection};
use crate::{Budget, ScanStop};

const HEADER_LEN: usize = 0x20;
const ROM_BYTES: u64 = 0x10_0000;

pub(crate) fn inspect(bytes: &[u8], budget: &mut Budget<'_>) -> Result<RipInspection, ScanStop> {
    if !bytes.starts_with(b"HESM") {
        return Ok(RipInspection::Unsupported);
    }
    if bytes.len() < HEADER_LEN {
        return Ok(RipInspection::Malformed(MalformedInput::TruncatedHeader));
    }
    if bytes[4] != 0 || &bytes[16..20] != b"DATA" {
        return Ok(RipInspection::Unsupported);
    }
    let program_len = u32::from_le_bytes(bytes[20..24].try_into().expect("HES header width"));
    let program_end = u64::from(HEADER_LEN as u32) + u64::from(program_len);
    if program_end > bytes.len() as u64 {
        return Ok(RipInspection::Malformed(
            MalformedInput::ProgramLengthExceedsSource,
        ));
    }
    if program_len == 0 || bytes[28..32] != [0; 4] || program_end != bytes.len() as u64 {
        return Ok(RipInspection::Unsupported);
    }
    let physical_load_address =
        u32::from_le_bytes(bytes[24..28].try_into().expect("HES header width"));
    let load = u64::from(physical_load_address);
    if load >= ROM_BYTES || u64::from(program_len) > ROM_BYTES - load {
        return Ok(RipInspection::Unsupported);
    }
    budget.charge()?;
    let sha256 = zeff_firmware::sha256_hex(bytes);
    if budget.cancel.load(Ordering::Relaxed) {
        return Err(ScanStop::Cancelled);
    }
    Ok(RipInspection::Match(Box::new(MusicRip {
        format: RipFormat::Hes,
        version: Some(0),
        title: String::new(),
        author: String::new(),
        copyright: String::new(),
        song_count: None,
        first_song: None,
        source: span(0, bytes.len() as u32),
        sha256,
        header: span(0, HEADER_LEN as u32),
        program: span(HEADER_LEN, program_len),
        opaque_metadata: None,
        load_address: None,
        init: Some(EntryPoint {
            cpu_address: u16::from_le_bytes(bytes[6..8].try_into().expect("HES header width")),
            initial_source_offset: None,
        }),
        play: None,
        details: RipDetails::Hes {
            raw_start_song: bytes[5],
            initial_mprs: bytes[8..16].try_into().expect("HES header width"),
            data_header: span(16, 16),
            physical_load_address,
            reserved: bytes[28..32].try_into().expect("HES header width"),
        },
        warnings: Vec::new(),
    })))
}

fn span(offset: usize, byte_len: u32) -> FileSpan {
    FileSpan {
        offset: offset as u32,
        byte_len,
    }
}
