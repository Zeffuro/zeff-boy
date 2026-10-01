use std::sync::atomic::Ordering;

use super::{FileSpan, MusicRip, RipDetails, RipFormat, RipInspection};
use crate::{Budget, ScanStop};

const TRAILER_BYTES: usize = 32;
const ALIGNMENT: usize = 0x1_0000;

pub(crate) fn inspect(bytes: &[u8], budget: &mut Budget<'_>) -> Result<RipInspection, ScanStop> {
    if bytes.len() < ALIGNMENT || !bytes.len().is_multiple_of(ALIGNMENT) {
        return Ok(RipInspection::Unsupported);
    }
    let trailer = bytes.len() - TRAILER_BYTES;
    if &bytes[trailer..trailer + 4] != b"WSRF" {
        return Ok(RipInspection::Unsupported);
    }
    budget.charge()?;
    let sha256 = zeff_firmware::sha256_hex(bytes);
    if budget.cancel.load(Ordering::Relaxed) {
        return Err(ScanStop::Cancelled);
    }
    Ok(RipInspection::Match(Box::new(MusicRip {
        format: RipFormat::Wsr,
        version: None,
        title: String::new(),
        author: String::new(),
        copyright: String::new(),
        song_count: None,
        first_song: None,
        source: span(0, bytes.len() as u32),
        sha256,
        header: span(trailer, TRAILER_BYTES as u32),
        program: span(0, trailer as u32),
        opaque_metadata: None,
        load_address: None,
        init: None,
        play: None,
        details: RipDetails::Wsr {
            raw_byte_4: bytes[trailer + 4],
            raw_start_song: bytes[trailer + 5],
            opaque_trailer: bytes[trailer + 6..trailer + 16]
                .try_into()
                .expect("WSR trailer width"),
            reset_entry: span(trailer + 16, 6),
            cartridge_footer: span(trailer + 22, 10),
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
