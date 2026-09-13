use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use super::tracker::FileSpan;
use super::{
    Budget, MAX_CANDIDATES, MAX_ROM_BYTES, MAX_SCAN_WORK, MalformedInput, ScanLimits, ScanStop,
};

mod scan;
mod structure;
pub use scan::scan;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RipFormat {
    Gbs,
    Nsf,
}

impl RipFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Gbs => "gbs",
            Self::Nsf => "nsf",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Gbs => "Game Boy GBS",
            Self::Nsf => "NES NSF",
        }
    }

    pub fn system_id(self) -> &'static str {
        match self {
            Self::Gbs => "standalone_gbs",
            Self::Nsf => "standalone_nsf",
        }
    }

    pub fn detector_id(self) -> &'static str {
        match self {
            Self::Gbs => "gbs-container",
            Self::Nsf => "nsf-container",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MusicRip {
    pub format: RipFormat,
    pub version: u8,
    pub title: String,
    pub author: String,
    pub copyright: String,
    pub song_count: u8,
    pub first_song: u8,
    pub source: FileSpan,
    pub sha256: String,
    pub header: FileSpan,
    pub program: FileSpan,
    pub opaque_metadata: Option<FileSpan>,
    pub load_address: u16,
    pub init: EntryPoint,
    pub play: EntryPoint,
    pub details: RipDetails,
    pub warnings: Vec<RipWarning>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct EntryPoint {
    pub cpu_address: u16,
    pub initial_source_offset: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RipDetails {
    Gbs {
        stack_pointer: u16,
        timer_modulo: u8,
        timer_control: u8,
        double_speed: bool,
        initial_play_rate_hz: Option<Rate>,
        logical_page_count: u32,
        leading_padding: u32,
        page_size: u32,
    },
    Nsf {
        ntsc_period_us: u16,
        pal_period_us: u16,
        region_bits: u8,
        region: NsfRegion,
        expansion_bits: u8,
        expansion_chips: Vec<&'static str>,
        initial_banks: [u8; 8],
        banking_enabled: bool,
        bank_count: Option<u32>,
        leading_padding: u32,
        bank_size: u32,
        nsf2_flags: u8,
        declared_program_bytes: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Rate {
    pub numerator: u32,
    pub denominator: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NsfRegion {
    Ntsc,
    Pal,
    DualNtscPreferred,
    DualPalPreferred,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RipWarning {
    ReservedBits {
        field: &'static str,
        value: u8,
    },
    NonAsciiText {
        field: &'static str,
    },
    ControlText {
        field: &'static str,
    },
    NonzeroTextPadding {
        field: &'static str,
    },
    UnterminatedText {
        field: &'static str,
    },
    UnbackedEntry {
        entry: &'static str,
        cpu_address: u16,
    },
    UnspecifiedPeriod {
        region: &'static str,
    },
    MultipleExpansionChips,
    ExpansionCompatibility {
        chip: &'static str,
    },
    FdsMapping,
    LowFdsAddress {
        entry: &'static str,
        cpu_address: u16,
    },
    ProgramExceedsLinearMemory {
        byte_len: u32,
    },
    PageIndexExceedsByte {
        pages: u32,
    },
    InitialBankBeyondProgram {
        slot: u8,
        bank: u8,
        pages: u32,
    },
    CustomInterruptVectors,
    UnbackedInterruptVectors,
    Nsf2FeatureFlags {
        value: u8,
    },
    OpaqueMetadata {
        byte_len: u32,
    },
}

pub enum RipInspection {
    Match(Box<MusicRip>),
    Unsupported,
    Malformed(MalformedInput),
}

pub fn inspect(
    bytes: &[u8],
    format: RipFormat,
    limits: ScanLimits,
    cancel: &AtomicBool,
) -> Result<Option<MusicRip>, ScanStop> {
    if cancel.load(Ordering::Relaxed) {
        return Err(ScanStop::Cancelled);
    }
    if bytes.len() > MAX_ROM_BYTES {
        return Err(ScanStop::MediaLimit);
    }
    if limits.max_work > MAX_SCAN_WORK || limits.max_candidates > MAX_CANDIDATES {
        return Err(ScanStop::InvalidLimits);
    }
    Ok(
        match inspect_with_budget(
            bytes,
            format,
            &mut Budget {
                cancel,
                remaining: limits.max_work,
            },
        )? {
            RipInspection::Match(rip) => Some(*rip),
            RipInspection::Unsupported | RipInspection::Malformed(_) => None,
        },
    )
}

pub(crate) fn inspect_with_budget(
    bytes: &[u8],
    format: RipFormat,
    budget: &mut Budget<'_>,
) -> Result<RipInspection, ScanStop> {
    budget.charge()?;
    let (header_len, version_at, songs_at, text_at) = match format {
        RipFormat::Gbs if bytes.starts_with(b"GBS") => (0x70, 3, 4, 0x10),
        RipFormat::Nsf if bytes.starts_with(b"NESM\x1a") => (0x80, 5, 6, 0x0e),
        _ => return Ok(RipInspection::Unsupported),
    };
    if bytes.len() <= version_at {
        return Ok(RipInspection::Malformed(MalformedInput::TruncatedHeader));
    }
    if bytes[version_at] != 1 {
        return Ok(RipInspection::Unsupported);
    }
    if bytes.len() < header_len {
        return Ok(RipInspection::Malformed(MalformedInput::TruncatedHeader));
    }
    if bytes.len() == header_len {
        return Ok(RipInspection::Malformed(MalformedInput::EmptyProgram));
    }
    let song_count = bytes[songs_at];
    let first_song = bytes[songs_at + 1];
    if song_count == 0 {
        return Ok(RipInspection::Malformed(MalformedInput::InvalidSongCount));
    }
    if !(1..=song_count).contains(&first_song) {
        return Ok(RipInspection::Malformed(MalformedInput::InvalidFirstSong));
    }
    let mut warnings = Vec::new();
    let title = structure::text(bytes, text_at, "title", format, &mut warnings);
    let author = structure::text(bytes, text_at + 32, "author", format, &mut warnings);
    let copyright = structure::text(bytes, text_at + 64, "copyright", format, &mut warnings);
    budget.charge()?;
    let (program, opaque_metadata, load_address, init, play, details) =
        match structure::header(bytes, format, &mut warnings) {
            Ok(header) => header,
            Err(reason) => return Ok(RipInspection::Malformed(reason)),
        };
    budget.charge()?;
    let sha256 = zeff_firmware::sha256_hex(bytes);
    if budget.cancel.load(Ordering::Relaxed) {
        return Err(ScanStop::Cancelled);
    }
    Ok(RipInspection::Match(Box::new(MusicRip {
        format,
        version: 1,
        title,
        author,
        copyright,
        song_count,
        first_song,
        source: FileSpan {
            offset: 0,
            byte_len: bytes.len() as u32,
        },
        sha256,
        header: FileSpan {
            offset: 0,
            byte_len: header_len as u32,
        },
        program,
        opaque_metadata,
        load_address,
        init,
        play,
        details,
        warnings,
    })))
}

pub fn verify(bytes: &[u8], expected: &MusicRip, cancel: &AtomicBool) -> anyhow::Result<()> {
    let actual = inspect(bytes, expected.format, ScanLimits::default(), cancel)
        .map_err(|stop| anyhow::anyhow!("music-rip verification stopped: {stop:?}"))?;
    anyhow::ensure!(
        actual.as_ref() == Some(expected),
        "music-rip source no longer matches its validated inventory"
    );
    Ok(())
}
