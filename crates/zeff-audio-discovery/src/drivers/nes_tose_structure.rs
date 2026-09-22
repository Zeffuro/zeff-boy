use crate::{Budget, RomSpan, ScanStop, tracker::FileSpan};

use super::candidates::{
    CandidateEvidence, CandidateQualification, DriverCandidate, EvidenceKind, SelectorHold,
    StructuralInventory, StructuralSelection,
};

mod layout;
mod mapped;
mod sequence;
#[cfg(any(test, feature = "test-support"))]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub use tests::synthetic_rom;

pub(crate) fn scan(
    bytes: &[u8],
    findings: &mut Vec<DriverCandidate>,
    budget: &mut Budget<'_>,
    capacity: usize,
) -> Result<(), ScanStop> {
    scan_with_span_limit(bytes, findings, budget, capacity, 8192)
}

fn scan_with_span_limit(
    bytes: &[u8],
    findings: &mut Vec<DriverCandidate>,
    budget: &mut Budget<'_>,
    capacity: usize,
    mut retained: usize,
) -> Result<(), ScanStop> {
    budget.charge()?;
    let Some((prg, mapper)) = prg(bytes) else {
        return Ok(());
    };
    let width = if mapper == 1 { 0x4000 } else { 0x2000 };
    let bases = if mapper == 1 {
        [0x8000, 0xc000]
    } else {
        [0x8000, 0xa000]
    };
    for (page, bank) in prg.chunks_exact(width).enumerate() {
        for cpu_base in bases {
            for at in 0..bank.len().saturating_sub(16) {
                if at % 64 == 0 {
                    budget.charge()?;
                }
                if bank.get(at..at + 3) != Some(&[0xa9, 0, 0x8d])
                    || !matches!(layout::word(bank, at + 3), 0x7a8 | 0x7c8 | 0x4a8)
                {
                    continue;
                }
                let Some(engine) = layout::engine(bank, at, cpu_base, budget)? else {
                    continue;
                };
                let Some(layout) = layout::resolve(bank, engine, budget)? else {
                    continue;
                };
                if findings.len() >= capacity {
                    return Err(ScanStop::CandidateLimit);
                }
                let offset = 16 + page * width;
                retain(
                    &mut retained,
                    4 + layout.engine.code_ranges.len()
                        + layout.engine.writers.len()
                        + usize::from(layout.engine.routing.is_some())
                        + usize::from(layout.selector_data.is_some()),
                )?;
                let inventory = inventory(bank, offset, &layout, budget, &mut retained)?;
                let mut evidence = Vec::new();
                for (signature, range) in [
                    ("closed-slot-init", layout.init),
                    ("closed-slot-selector", layout.selector),
                ]
                .into_iter()
                .chain(
                    layout
                        .engine
                        .code_ranges
                        .iter()
                        .map(|range| ("closed-slot-command-graph", *range)),
                ) {
                    budget.charge()?;
                    evidence.push(evidence_at(
                        bank,
                        offset,
                        signature,
                        EvidenceKind::InstructionBytes,
                        range,
                    ));
                }
                for writer in &layout.engine.writers {
                    budget.charge()?;
                    evidence.push(evidence_at(
                        bank,
                        offset,
                        "decoded-apu-writer",
                        EvidenceKind::SoundRegisterWrite,
                        (*writer, *writer + 3),
                    ));
                }
                if let Some(range) = layout.engine.routing {
                    budget.charge()?;
                    evidence.push(evidence_at(
                        bank,
                        offset,
                        "closed-slot-channel-routing",
                        EvidenceKind::DriverData,
                        range,
                    ));
                }
                if let Some(range) = layout.selector_data {
                    budget.charge()?;
                    evidence.push(evidence_at(
                        bank,
                        offset,
                        "closed-slot-selector-masks",
                        EvidenceKind::DriverData,
                        range,
                    ));
                }
                findings.push(DriverCandidate {
                    family: "TOSE closed-slot NES",
                    variant: match layout.engine.kind {
                        layout::Kind::Pointer => "mmc3-pointer-selector",
                        layout::Kind::IndexedPointer => "mmc3-indexed-pointer-selector",
                        layout::Kind::IndexedAbsolute => "mmc3-indexed-absolute-selector",
                        layout::Kind::AccumulatorPointer => "accumulator-pointer-selector",
                        layout::Kind::YPointerHelper => "y-pointer-with-mapped-helper",
                        layout::Kind::VariableSlotLimit => "variable-slot-limit-pointer",
                        #[cfg(any(test, feature = "test-support"))]
                        layout::Kind::Synthetic => "synthetic-closed-slot",
                    },
                    qualification: CandidateQualification::Structural,
                    fingerprint_source: "decoded 6502 command graph and selector ABI",
                    fingerprint_revision: "nes-closed-slot-structure/2",
                    evidence,
                    inventory: Some(inventory),
                    code: None,
                });
            }
        }
    }
    Ok(())
}

fn prg(bytes: &[u8]) -> Option<(&[u8], u8)> {
    let header = bytes.get(..16)?;
    if &header[..4] != b"NES\x1a"
        || header[4] < 2
        || !matches!(header[6] & 0xfc, 0x10 | 0x40)
        || header[7] != 0
        || header[8..].iter().any(|value| *value != 0)
    {
        return None;
    }
    let mapper = header[6] >> 4;
    let max_banks = if mapper == 1 { 16 } else { 32 };
    if header[4] > max_banks {
        return None;
    }
    let prg_len = usize::from(header[4]) * 0x4000;
    let expected = 16 + prg_len + usize::from(header[5]) * 0x2000;
    (bytes.len() == expected).then(|| (&bytes[16..16 + prg_len], mapper))
}

fn evidence_at(
    bank: &[u8],
    offset: usize,
    signature: &'static str,
    kind: EvidenceKind,
    range: (usize, usize),
) -> CandidateEvidence {
    CandidateEvidence {
        signature,
        kind,
        span: FileSpan {
            offset: (offset + range.0) as u32,
            byte_len: (range.1 - range.0) as u32,
        },
        sha256: zeff_firmware::sha256_hex(&bank[range.0..range.1]),
    }
}

fn inventory(
    bank: &[u8],
    offset: usize,
    layout: &layout::Layout,
    budget: &mut Budget<'_>,
    retained: &mut usize,
) -> Result<StructuralInventory, ScanStop> {
    let mut entries = Vec::new();
    let mut held = Vec::new();
    for raw_selector in 0..layout.selectors - 3 {
        budget.charge()?;
        let at = layout.table + usize::from(raw_selector) * 4;
        let descriptor = &bank[at..at + 16];
        if (at..at + 16).any(|address| layout.is_code(address)) {
            continue;
        }
        let base = descriptor[0];
        if base > 84 || !base.is_multiple_of(21) {
            continue;
        }
        let mut channels = 0_u8;
        if descriptor
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .any(|(index, entry)| {
                let invalid = entry[0] != base + index as u8 * 21
                    || entry[1] > 3
                    || channels & (1 << entry[1]) != 0;
                if entry[1] <= 3 {
                    channels |= 1 << entry[1];
                }
                invalid
            })
        {
            continue;
        }
        let mut tracks = Vec::new();
        let mut failure = None;
        let mut track_spans = 0;
        for entry in descriptor.as_chunks::<4>().0 {
            let pointer = layout::word(entry, 2);
            match sequence::walk(bank, offset, layout, pointer, entry[1], budget)? {
                Ok(track) => {
                    if track.source_spans.iter().any(|span| {
                        (span.offset as usize) < offset + at + 16
                            && span.offset as usize + span.byte_len as usize > offset + at
                    }) {
                        failure = Some(super::candidates::SelectorHoldReason::SourceOverlap);
                        break;
                    }
                    retain(retained, track.source_spans.len())?;
                    track_spans += track.source_spans.len();
                    tracks.push(track);
                }
                Err(reason) => {
                    failure = Some(reason);
                    break;
                }
            }
        }
        if failure.is_none() && tracks.iter().all(|track| track.note_count == 0) {
            failure = Some(super::candidates::SelectorHoldReason::NoNotes);
        }
        if let Some(reason) = failure {
            *retained += track_spans;
            retain(retained, 1)?;
            held.push(SelectorHold {
                raw_selector,
                reason,
            });
        } else {
            retain(retained, 1)?;
            entries.push(StructuralSelection {
                raw_selector,
                slot_base: base,
                descriptor: FileSpan {
                    offset: (offset + at) as u32,
                    byte_len: 16,
                },
                tracks,
            });
        }
    }
    Ok(StructuralInventory {
        mapped_window: RomSpan {
            effective_offset: offset as u32,
            byte_len: bank.len() as u32,
            canonical_cpu_address: layout.engine.cpu_base as u32,
        },
        descriptor_probe: FileSpan {
            offset: (offset + layout.table) as u32,
            byte_len: u32::from(layout.selectors) * 4,
        },
        selector_input_count: layout.selectors,
        inspected_selectors: layout.selectors - 3,
        entries,
        held,
    })
}

fn retain(remaining: &mut usize, count: usize) -> Result<(), ScanStop> {
    *remaining = remaining
        .checked_sub(count)
        .ok_or(ScanStop::InventoryLimit)?;
    Ok(())
}
