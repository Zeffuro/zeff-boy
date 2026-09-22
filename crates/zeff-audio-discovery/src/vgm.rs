use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::{bail, ensure};
use flate2::bufread::GzDecoder;
use serde::Serialize;

use super::{
    DetectorState, MAX_CANDIDATES, MAX_ROM_BYTES, MAX_SCAN_WORK, MediaIdentity, ScanLimits,
    ScanReport, ScanStatus, ScanStop, tracker::FileSpan,
};

pub mod capture;
pub mod playback;
mod structure;
use structure::{command, gd3};

pub const TICKS_PER_SECOND: u32 = 44_100;
const MAX_COMMANDS: u32 = 1_000_000;
const MAX_SAMPLES: u64 = 7_200 * TICKS_PER_SECOND as u64;
const MAX_GD3_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VgmEncoding {
    Raw,
    Gzip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VgmAddressSpace {
    SourceFile,
    DecompressedVgm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct VgmSpan {
    pub address_space: VgmAddressSpace,
    pub offset: u32,
    pub byte_len: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LogicalIdentity {
    pub address_space: VgmAddressSpace,
    pub byte_len: u32,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VgmChip {
    pub name: &'static str,
    pub raw_clock: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VgmWarning {
    EofMismatch { declared: u32, actual: u32 },
    TrailingBytes { byte_len: u32 },
    ReservedCommand { offset: u32, opcode: u8 },
    DeclaredSamplesMismatch { declared: u32, actual: u64 },
    DeclaredLoopSamplesMismatch { declared: u64, actual: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VgmLog {
    pub title: String,
    pub source: FileSpan,
    pub encoding: VgmEncoding,
    pub logical: LogicalIdentity,
    pub header: VgmSpan,
    pub commands: VgmSpan,
    pub gd3: Option<VgmSpan>,
    pub version: u32,
    pub declared_samples: u32,
    pub samples: u64,
    pub loop_offset: Option<u32>,
    pub loop_samples: u64,
    pub command_count: u32,
    pub chips: Vec<VgmChip>,
    pub command_histogram: BTreeMap<u8, u32>,
    pub warnings: Vec<VgmWarning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sn_playback: Option<playback::SnPlayback>,
}

pub fn scan(bytes: &[u8], limits: ScanLimits, cancel: &AtomicBool) -> ScanReport {
    let mut report = ScanReport::new(
        "standalone-vgm-structural",
        3,
        super::detectors::VGM,
        &[
            "VGM/VGZ logs are structurally preserved. Qualified SN76489 configurations support standalone playback; logs do not identify a native console driver or prove source PCM equivalence.",
            "Chip data block framing and ROM/RAM ranges are inspected. Compressed chip payloads are retained without decompression or playback validation.",
            "Gzip is accepted only as one CRC-valid member with no trailing bytes. Work units count parser steps; bounded hashing and gzip decoding are separate passes.",
            "VGM extra headers and undefined commands are unsupported. Known commands are gated to their supported version; each reserved opcode's first occurrence is warned and all occurrences are counted.",
            "Standalone playback is restricted to the reported SN76489 contract. Other chip clocks and commands remain structural inventory without playback validation.",
        ],
        MediaIdentity {
            system: "standalone_vgm",
            byte_len: bytes.len() as u64,
            sha256: (!cancel.load(Ordering::Relaxed) && bytes.len() <= MAX_ROM_BYTES)
                .then(|| const_hex::encode(zeff_firmware::sha256_bytes(bytes))),
        },
        limits,
    );
    if report.preflight(cancel).is_some() {
        return report;
    }
    let mut budget = Budget {
        cancel,
        remaining: limits.max_work,
    };
    let state = match inspect_with_budget(bytes, &mut budget) {
        Err(stop) => {
            report.status = ScanStatus::Incomplete(stop);
            DetectorState::Incomplete(stop)
        }
        Ok(None) => {
            report.status = ScanStatus::Unsupported;
            DetectorState::Unsupported
        }
        Ok(Some(_)) if limits.max_candidates == 0 => {
            report.status = ScanStatus::Incomplete(ScanStop::CandidateLimit);
            DetectorState::Incomplete(ScanStop::CandidateLimit)
        }
        Ok(Some(log)) => {
            report.vgm_logs.push(log);
            report.status = ScanStatus::Complete;
            DetectorState::Complete
        }
    };
    report.work_used = limits.max_work - budget.remaining;
    report.record_detector(
        super::detectors::VGM[0],
        state,
        report.vgm_logs.len(),
        report.work_used,
    );
    debug_assert_eq!(
        report.detector_outcomes.len(),
        report.applicable_detectors.len()
    );
    report
}

pub fn inspect(
    bytes: &[u8],
    limits: ScanLimits,
    cancel: &AtomicBool,
) -> Result<Option<VgmLog>, ScanStop> {
    if bytes.len() > MAX_ROM_BYTES {
        return Err(ScanStop::MediaLimit);
    }
    if limits.max_work > MAX_SCAN_WORK || limits.max_candidates > MAX_CANDIDATES {
        return Err(ScanStop::InvalidLimits);
    }
    let mut budget = Budget {
        cancel,
        remaining: limits.max_work,
    };
    inspect_with_budget(bytes, &mut budget)
}

fn inspect_with_budget(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<VgmLog>, ScanStop> {
    let (encoding, logical) = match logical(bytes, budget.cancel) {
        Ok(value) => value,
        Err(DecodeError::NotVgm) => return Ok(None),
        Err(DecodeError::Stop(stop)) => return Err(stop),
    };
    parse(&logical, bytes.len(), encoding, budget)
}

pub fn decode(bytes: &[u8], cancel: &AtomicBool) -> anyhow::Result<Vec<u8>> {
    match logical(bytes, cancel) {
        Ok((_, value)) => Ok(value),
        Err(DecodeError::NotVgm) => bail!("source is neither VGM nor a valid single-member VGZ"),
        Err(DecodeError::Stop(ScanStop::Cancelled)) => bail!("VGM decoding cancelled"),
        Err(DecodeError::Stop(_)) => bail!("VGM decoding exceeds its structural limits"),
    }
}

pub fn verify(bytes: &[u8], expected: &VgmLog, cancel: &AtomicBool) -> anyhow::Result<()> {
    let actual = inspect(
        bytes,
        ScanLimits {
            max_work: MAX_SCAN_WORK,
            max_candidates: 1,
        },
        cancel,
    )
    .map_err(|stop| anyhow::anyhow!("VGM verification stopped: {stop:?}"))?;
    ensure!(
        actual.as_ref() == Some(expected),
        "VGM source no longer matches its validated inventory"
    );
    Ok(())
}

enum DecodeError {
    NotVgm,
    Stop(ScanStop),
}

fn logical(bytes: &[u8], cancel: &AtomicBool) -> Result<(VgmEncoding, Vec<u8>), DecodeError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(DecodeError::Stop(ScanStop::Cancelled));
    }
    if bytes.len() > MAX_ROM_BYTES {
        return Err(DecodeError::Stop(ScanStop::MediaLimit));
    }
    if bytes.starts_with(b"Vgm ") {
        return Ok((VgmEncoding::Raw, bytes.to_vec()));
    }
    if !bytes.starts_with(&[0x1f, 0x8b]) {
        return Err(DecodeError::NotVgm);
    }
    let mut decoder = GzDecoder::new(Cursor::new(bytes));
    let mut out = Vec::new();
    let cap = bytes
        .len()
        .saturating_mul(512)
        .saturating_add(4096)
        .min(MAX_ROM_BYTES);
    let mut block = [0u8; 8192];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(DecodeError::Stop(ScanStop::Cancelled));
        }
        let read = decoder.read(&mut block).map_err(|_| DecodeError::NotVgm)?;
        if read == 0 {
            break;
        }
        if out.len().checked_add(read).is_none_or(|len| len > cap) {
            return Err(DecodeError::Stop(ScanStop::MediaLimit));
        }
        out.extend_from_slice(&block[..read]);
    }
    if decoder.get_ref().position() != bytes.len() as u64 || !out.starts_with(b"Vgm ") {
        return Err(DecodeError::NotVgm);
    }
    Ok((VgmEncoding::Gzip, out))
}

fn parse(
    data: &[u8],
    source_len: usize,
    encoding: VgmEncoding,
    budget: &mut Budget<'_>,
) -> Result<Option<VgmLog>, ScanStop> {
    let mut stopped = None;
    let result = (|| {
        data.get(..0x40)?;
        if data.get(..4)? != b"Vgm " {
            return None;
        }
        let version = u32le(data, 8)?;
        if !matches!(
            version,
            0x100 | 0x101 | 0x110 | 0x150 | 0x151 | 0x160 | 0x161 | 0x170 | 0x171
        ) {
            return None;
        }
        let eof = u32le(data, 4)?.checked_add(4)? as usize;
        if !(0x40..=data.len()).contains(&eof) {
            return None;
        }
        let data_start = if version < 0x150 || u32le(data, 0x34)? == 0 {
            0x40
        } else {
            0x34usize.checked_add(u32le(data, 0x34)? as usize)?
        };
        if !(0x40..=eof).contains(&data_start) {
            return None;
        }
        if version >= 0x170 && header_word(data, 0xbc, data_start) != 0 {
            return None;
        }
        let mut warnings = Vec::new();
        if eof != data.len() {
            warnings.push(VgmWarning::EofMismatch {
                declared: eof as u32,
                actual: data.len() as u32,
            });
            if data.len() > eof {
                warnings.push(VgmWarning::TrailingBytes {
                    byte_len: (data.len() - eof) as u32,
                });
            }
        }
        let mut chips = Vec::new();
        for &(name, offset, minimum) in CLOCKS {
            if version >= minimum && offset < data_start {
                let clock = header_word(data, offset, data_start);
                if clock & 0x3fff_ffff != 0 {
                    chips.push(VgmChip {
                        name,
                        raw_clock: clock,
                    });
                }
            }
        }
        let declared_samples = u32le(data, 0x18)?;
        let loop_offset = match u32le(data, 0x1c)? {
            0 => None,
            value => Some(value.checked_add(0x1c)?),
        };
        let mut loop_sample_start = None;
        let mut histogram = BTreeMap::new();
        let mut pos = data_start;
        let command_start = pos;
        let mut samples = 0u64;
        let mut command_count = 0u32;
        let command_end = loop {
            charge(budget, &mut stopped)?;
            if command_count == MAX_COMMANDS {
                stopped = Some(ScanStop::InventoryLimit);
                return None;
            }
            if pos >= eof {
                return None;
            }
            if loop_offset == Some(pos as u32) {
                loop_sample_start = Some(samples);
            }
            let opcode = *data.get(pos)?;
            *histogram.entry(opcode).or_insert(0) += 1;
            command_count += 1;
            let (len, wait, reserved) = command(opcode, data, pos, version)?;
            if reserved && histogram[&opcode] == 1 {
                warnings.push(VgmWarning::ReservedCommand {
                    offset: pos as u32,
                    opcode,
                });
            }
            if opcode == 0x66 {
                break pos + 1;
            }
            pos = pos.checked_add(len)?;
            if pos > eof {
                return None;
            }
            samples = samples.checked_add(wait)?;
            if samples > MAX_SAMPLES {
                stopped = Some(ScanStop::ValidationLimit);
                return None;
            }
        };
        if version <= 0x101 {
            let legacy_clock = u32le(data, 0x10)?;
            if !histogram.contains_key(&0x51) {
                chips.retain(|chip| chip.name != "ym2413");
            }
            for (name, used) in [
                (
                    "ym2612",
                    histogram.contains_key(&0x52) || histogram.contains_key(&0x53),
                ),
                ("ym2151", histogram.contains_key(&0x54)),
            ] {
                if used && legacy_clock & 0x3fff_ffff != 0 {
                    chips.push(VgmChip {
                        name,
                        raw_clock: legacy_clock,
                    });
                }
            }
        }
        let loop_samples = match loop_offset {
            Some(_) => {
                let actual = samples.checked_sub(loop_sample_start?)?;
                if actual == 0 {
                    return None;
                }
                if u32le(data, 0x20)? as u64 != actual {
                    warnings.push(VgmWarning::DeclaredLoopSamplesMismatch {
                        declared: u32le(data, 0x20)? as u64,
                        actual,
                    });
                }
                actual
            }
            None => {
                if u32le(data, 0x20)? != 0 {
                    return None;
                }
                0
            }
        };
        if declared_samples != samples as u32 {
            warnings.push(VgmWarning::DeclaredSamplesMismatch {
                declared: declared_samples,
                actual: samples,
            });
        }
        let (gd3, title) = gd3(data, eof, command_end, budget, &mut stopped, encoding)?;
        let used_end = gd3
            .as_ref()
            .map_or(command_end, |span| (span.offset + span.byte_len) as usize);
        if let Some(gd3) = &gd3
            && gd3.offset as usize > command_end
        {
            warnings.push(VgmWarning::TrailingBytes {
                byte_len: gd3.offset - command_end as u32,
            });
        }
        if eof > used_end {
            warnings.push(VgmWarning::TrailingBytes {
                byte_len: (eof - used_end) as u32,
            });
        }
        let mut log = VgmLog {
            title,
            source: FileSpan {
                offset: 0,
                byte_len: source_len as u32,
            },
            encoding,
            logical: LogicalIdentity {
                address_space: if encoding == VgmEncoding::Raw {
                    VgmAddressSpace::SourceFile
                } else {
                    VgmAddressSpace::DecompressedVgm
                },
                byte_len: data.len() as u32,
                sha256: const_hex::encode(zeff_firmware::sha256_bytes(data)),
            },
            header: span(encoding, 0, data_start),
            commands: span(encoding, command_start, command_end - command_start),
            gd3,
            version,
            declared_samples,
            samples,
            loop_offset,
            loop_samples,
            command_count,
            chips,
            command_histogram: histogram,
            warnings,
            sn_playback: None,
        };
        log.sn_playback = playback::capability(data, &log);
        Some(log)
    })();
    match stopped {
        Some(stop) => Err(stop),
        None => Ok(result),
    }
}

fn charge(budget: &mut Budget<'_>, stopped: &mut Option<ScanStop>) -> Option<()> {
    if let Err(stop) = budget.charge() {
        *stopped = Some(stop);
        None
    } else {
        Some(())
    }
}

fn span(encoding: VgmEncoding, offset: usize, byte_len: usize) -> VgmSpan {
    VgmSpan {
        address_space: if encoding == VgmEncoding::Raw {
            VgmAddressSpace::SourceFile
        } else {
            VgmAddressSpace::DecompressedVgm
        },
        offset: offset as u32,
        byte_len: byte_len as u32,
    }
}
fn u16le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn u32le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn header_word(bytes: &[u8], offset: usize, header_end: usize) -> u32 {
    u32::from_le_bytes(std::array::from_fn(|index| {
        if offset + index < header_end {
            bytes[offset + index]
        } else {
            0
        }
    }))
}

struct Budget<'a> {
    cancel: &'a AtomicBool,
    remaining: u64,
}
impl Budget<'_> {
    fn charge(&mut self) -> Result<(), ScanStop> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(ScanStop::Cancelled)
        } else if self.remaining == 0 {
            Err(ScanStop::WorkLimit)
        } else {
            self.remaining -= 1;
            Ok(())
        }
    }
}

const CLOCKS: &[(&str, usize, u32)] = &[
    ("sn76489", 0x0c, 0x100),
    ("ym2413", 0x10, 0x100),
    ("ym2612", 0x2c, 0x110),
    ("ym2151", 0x30, 0x110),
    ("sega_pcm", 0x38, 0x151),
    ("rf5c68", 0x40, 0x151),
    ("ym2203", 0x44, 0x151),
    ("ym2608", 0x48, 0x151),
    ("ym2610", 0x4c, 0x151),
    ("ym3812", 0x50, 0x151),
    ("ym3526", 0x54, 0x151),
    ("y8950", 0x58, 0x151),
    ("ymf262", 0x5c, 0x151),
    ("ymf278b", 0x60, 0x151),
    ("ymf271", 0x64, 0x151),
    ("ymz280b", 0x68, 0x151),
    ("rf5c164", 0x6c, 0x151),
    ("pwm", 0x70, 0x151),
    ("ay8910", 0x74, 0x151),
    ("gameboy_dmg", 0x80, 0x161),
    ("nes_apu", 0x84, 0x161),
    ("multipcm", 0x88, 0x161),
    ("upd7759", 0x8c, 0x161),
    ("okim6258", 0x90, 0x161),
    ("okim6295", 0x98, 0x161),
    ("k051649", 0x9c, 0x161),
    ("k054539", 0xa0, 0x161),
    ("huc6280", 0xa4, 0x161),
    ("c140", 0xa8, 0x161),
    ("k053260", 0xac, 0x161),
    ("pokey", 0xb0, 0x161),
    ("qsound", 0xb4, 0x161),
    ("scsp", 0xb8, 0x171),
    ("wonderswan", 0xc0, 0x171),
    ("vsu", 0xc4, 0x171),
    ("saa1099", 0xc8, 0x171),
    ("es5503", 0xcc, 0x171),
    ("es5506", 0xd0, 0x171),
    ("x1_010", 0xd8, 0x171),
    ("c352", 0xdc, 0x171),
    ("ga20", 0xe0, 0x171),
];

#[cfg(test)]
mod tests;
