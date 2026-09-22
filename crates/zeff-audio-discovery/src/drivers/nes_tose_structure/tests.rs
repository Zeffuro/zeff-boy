use crate::{Budget, ScanStop};

use super::layout::{Engine, Layout};

const TICK: &[u8] = &[
    0xa9, 0, 0x8d, 0xa8, 7, 0xa9, 1, 0x8d, 0x15, 0x40, 0xa9, 0xbf, 0x99, 0, 0x40, 0xa9, 0, 0x99, 1,
    0x40, 0xa9, 0xaa, 0x99, 2, 0x40, 0xa9, 8, 0x99, 3, 0x40, 0x60,
];
const INIT: &[u8] = &[0xa9, 0, 0x8d, 0x15, 0x40, 0x60];

pub(super) fn engine_hash() -> &'static str {
    "7d086a8b0d189b19ee3ae259252409538555836a739f1b13fa27af4d2758ca81"
}

fn selector(table: u16) -> Vec<u8> {
    let [low, high] = table.to_le_bytes();
    vec![
        0xa5, 0xb2, 0xa0, 0, 0x84, 0xb2, 0x0a, 0x26, 0xb2, 0x0a, 0x26, 0xb2, 0x18, 0x69, low, 0x85,
        0xb1, 0xa9, high, 0x65, 0xb2, 0x85, 0xb2, 0xb1, 0xb1, 0xaa, 0xc8, 0xb1, 0xb1, 0x9d, 1, 7,
        0xc8, 0xb1, 0xb1, 0x9d, 2, 7, 0xc8, 0xb1, 0xb1, 0x9d, 3, 7, 0xa9, 0, 0x9d, 0, 7, 0x60,
    ]
}

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; 16 + 0x8000];
    bytes[..16].copy_from_slice(b"NES\x1a\x02\0\x40\0\0\0\0\0\0\0\0\0");
    let bank = &mut bytes[16..16 + 0x2000];
    bank[..TICK.len()].copy_from_slice(TICK);
    bank[0x80..0x8c].copy_from_slice(&[1, 2, 4, 8, 14, 13, 11, 7, 0, 1, 0x82, 0x43]);
    bank[0x100..0x100 + INIT.len()].copy_from_slice(INIT);
    let code = selector(0x8400);
    bank[0x110..0x110 + code.len()].copy_from_slice(&code);
    for channel in 0..4 {
        let at = 0x400 + channel * 4;
        let pointer = 0x8800 + channel as u16 * 16;
        bank[at..at + 4].copy_from_slice(&[
            84 + channel as u8 * 21,
            channel as u8,
            pointer as u8,
            (pointer >> 8) as u8,
        ]);
        let at = usize::from(pointer - 0x8000);
        bank[at..at + 7].copy_from_slice(&[1, 0, 0, 0, 1, 1, 0xff]);
    }
    bytes
}

pub(super) fn resolve(
    bank: &[u8],
    engine: Engine,
    budget: &mut Budget<'_>,
) -> Result<Option<Layout>, ScanStop> {
    budget.charge()?;
    let init = engine.tick + 0x100;
    let select = engine.tick + 0x110;
    let Some(code) = bank.get(select..select + 50) else {
        return Ok(None);
    };
    let table = u16::from_le_bytes([code[14], code[18]]);
    if bank.get(init..init + INIT.len()) != Some(INIT)
        || code != selector(table)
        || usize::from(table) < engine.cpu_base
        || (usize::from(table) - engine.cpu_base) + 1024 > bank.len()
    {
        return Ok(None);
    }
    Ok(Some(Layout {
        init: (init, init + INIT.len()),
        selector: (select, select + 50),
        table: usize::from(table) - engine.cpu_base,
        engine,
        selectors: 256,
        selector_data: None,
    }))
}

#[cfg(test)]
mod checks {
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::drivers::candidates::{DriverCandidate, SelectorHoldReason as Hold};

    fn scan(bytes: &[u8]) -> Vec<DriverCandidate> {
        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: 2_000_000,
        };
        let mut findings = Vec::new();
        super::super::scan(bytes, &mut findings, &mut budget, 4096).unwrap();
        findings
    }

    fn layout(bytes: &[u8]) -> Layout {
        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: 100_000,
        };
        let bank = &bytes[16..16 + 0x2000];
        let engine = super::super::layout::engine(bank, 0, 0x8000, &mut budget)
            .unwrap()
            .unwrap();
        resolve(bank, engine, &mut budget).unwrap().unwrap()
    }

    fn walk(
        bytes: &[u8],
        pointer: usize,
    ) -> Result<crate::drivers::candidates::StructuralTrack, Hold> {
        let layout = layout(bytes);
        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: 100_000,
        };
        super::super::sequence::walk(
            &bytes[16..16 + 0x2000],
            16,
            &layout,
            pointer,
            0,
            &mut budget,
        )
        .unwrap()
    }

    #[test]
    fn discovers_four_channels_without_native_identity() {
        let bytes = synthetic_rom();
        let findings = scan(&bytes);
        assert_eq!(findings.len(), 1);
        let inventory = findings[0].inventory.as_ref().unwrap();
        assert_eq!(inventory.inspected_selectors, 253);
        assert_eq!(inventory.entries.len(), 1);
        assert!(inventory.held.is_empty());
        assert_eq!(inventory.entries[0].raw_selector, 0);
        assert_eq!(inventory.entries[0].slot_base, 84);
        assert!(
            inventory.entries[0]
                .tracks
                .iter()
                .all(|track| track.note_count == 1)
        );
        assert!(findings[0].evidence.iter().any(|evidence| evidence.kind
            == crate::drivers::candidates::EvidenceKind::SoundRegisterWrite));
    }

    #[test]
    fn mapped_windows_keep_physical_provenance_and_full_width_sequences() {
        for (mapper, base, width, sequence_at) in [
            (1, 0x8000_u16, 0x4000, 0x2800),
            (1, 0xc000, 0x4000, 0x2800),
            (4, 0xa000, 0x2000, 0x1800),
        ] {
            let original = synthetic_rom();
            let mut bytes = original.clone();
            bytes[6] = mapper << 4;
            bytes[16..].fill(0);
            let offset = 16 + width;
            let bank = &mut bytes[offset..offset + width];
            bank[..0x1400].copy_from_slice(&original[16..16 + 0x1400]);
            let code = selector(base + 0x400);
            bank[0x110..0x110 + code.len()].copy_from_slice(&code);
            for channel in 0..4 {
                let at = sequence_at + channel * 16;
                let pointer = base + at as u16;
                bank[0x402 + channel * 4..0x404 + channel * 4]
                    .copy_from_slice(&pointer.to_le_bytes());
                bank[at..at + 7].copy_from_slice(&[1, 0, 0, 0, 1, 1, 0xff]);
            }
            let findings = scan(&bytes);
            assert_eq!(findings.len(), 1);
            let inventory = findings[0].inventory.as_ref().unwrap();
            assert_eq!(
                inventory.mapped_window.canonical_cpu_address,
                u32::from(base)
            );
            assert_eq!(inventory.mapped_window.byte_len, width as u32);
            assert_eq!(inventory.mapped_window.effective_offset, offset as u32);
            assert_eq!(inventory.entries.len(), 1);
            assert_eq!(
                inventory.entries[0].tracks[0].source_spans[0].offset,
                (offset + sequence_at) as u32
            );
            bytes[offset + 0x402..offset + 0x404]
                .copy_from_slice(&(base + (width - 1) as u16).to_le_bytes());
            let findings = scan(&bytes);
            let inventory = findings[0].inventory.as_ref().unwrap();
            assert!(inventory.entries.is_empty());
            assert_eq!(inventory.held[0].reason, Hold::SequenceOutOfRange);
        }
    }

    #[test]
    fn data_edits_are_freshly_walked_and_unrelated_prg_is_ignored() {
        let mut bytes = synthetic_rom();
        bytes[16 + 0x4000] = 0xff;
        bytes[16 + 0x806..16 + 0x809].copy_from_slice(&[2, 1, 0xff]);
        let findings = scan(&bytes);
        assert_eq!(
            findings[0].inventory.as_ref().unwrap().entries[0].tracks[0].note_count,
            2
        );
    }

    #[test]
    fn decoder_rejects_changed_semantics_raw_writers_and_unmapped_calls() {
        let mut routing = synthetic_rom();
        routing[16 + 0x8b] ^= 1;
        assert!(scan(&routing).is_empty());
        for change in [0xad, 0x20, 0x6c, 0] {
            let mut bytes = synthetic_rom();
            bytes[16 + 7] = change;
            assert!(scan(&bytes).is_empty());
        }
        let mut bytes = synthetic_rom();
        bytes[16..16 + TICK.len()].fill(0);
        bytes[16 + 0x900..16 + 0x900 + TICK.len()].copy_from_slice(TICK);
        assert!(scan(&bytes).is_empty());
    }

    #[test]
    fn supports_physical_and_cpu_relocation_of_proven_shape() {
        let bytes = synthetic_rom();
        let baseline = scan(&bytes);
        let mut moved = bytes.clone();
        moved[16..16 + 0x2000].fill(0);
        moved[16 + 0x2000..16 + 0x4000].copy_from_slice(&bytes[16..16 + 0x2000]);
        let findings = scan(&moved);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0]
                .inventory
                .as_ref()
                .unwrap()
                .mapped_window
                .effective_offset,
            16 + 0x2000
        );
        let mut moved = bytes.clone();
        moved[16..16 + 0x2000].fill(0);
        moved[16 + 0x200..16 + 0x1600].copy_from_slice(&bytes[16..16 + 0x1400]);
        let code = selector(0x8600);
        moved[16 + 0x310..16 + 0x310 + code.len()].copy_from_slice(&code);
        for channel in 0..4 {
            moved[16 + 0x603 + channel * 4] += 2;
        }
        let findings = scan(&moved);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].inventory.as_ref().unwrap().entries[0].tracks[0].note_count,
            baseline[0].inventory.as_ref().unwrap().entries[0].tracks[0].note_count
        );
    }

    #[test]
    fn all_header_mapping_claims_and_length_are_checked() {
        for (at, value) in [(6, 0x44), (6, 0x20), (7, 8), (8, 1), (15, 1), (4, 1)] {
            let mut bytes = synthetic_rom();
            bytes[at] = value;
            assert!(scan(&bytes).is_empty());
        }
        let mut bytes = synthetic_rom();
        bytes.pop();
        assert!(scan(&bytes).is_empty());
    }

    #[test]
    fn ordinary_mapper_register_width_bounds_prg_pages() {
        for (mapper, maximum) in [(1, 16), (4, 32)] {
            let mut bytes = vec![0; 16 + (maximum + 1) * 0x4000];
            bytes[..4].copy_from_slice(b"NES\x1a");
            bytes[6] = mapper << 4;
            bytes[4] = (maximum + 1) as u8;
            assert!(super::super::prg(&bytes).is_none());
            bytes[4] = maximum as u8;
            bytes.truncate(16 + maximum * 0x4000);
            assert!(super::super::prg(&bytes).is_some());
        }
    }

    #[test]
    fn graph_holds_bad_sequences_and_accepts_yielding_recurrence() {
        for (data, expected) in [
            (vec![0xad, 0], Hold::PointerRebase),
            (vec![0xca, 1], Hold::UnsupportedCommand),
            (vec![0xb0, 2], Hold::NonYieldingLoop),
            ([0xa0, 0].repeat(16), Hold::CommandBatchLimit),
        ] {
            let mut bytes = synthetic_rom();
            bytes[16 + 0x804..16 + 0x804 + data.len()].copy_from_slice(&data);
            assert_eq!(walk(&bytes, 0x8800), Err(expected));
        }
        let mut bytes = synthetic_rom();
        bytes[16 + 0x804..16 + 0x80a].copy_from_slice(&[0xfd, 0, 1, 1, 0xb0, 0]);
        assert_eq!(walk(&bytes, 0x8800).unwrap().note_count, 1);
        assert_eq!(walk(&bytes, 0x9fff), Err(Hold::SequenceOutOfRange));
        assert_eq!(walk(&bytes, 0x8110), Err(Hold::SourceOverlap));
        assert_eq!(walk(&bytes, 0x700), Err(Hold::SequenceOutOfRange));
        bytes[16 + 0x804..16 + 0x806].copy_from_slice(&[0xb0, 255]);
        bytes[16 + 0x9fe..16 + 0xa00].copy_from_slice(&[1, 1]);
        assert_eq!(walk(&bytes, 0x8800), Err(Hold::Restart));
    }

    #[test]
    fn descriptors_cannot_parse_themselves_as_note_streams() {
        let mut bytes = synthetic_rom();
        bytes[16 + 0x402..16 + 0x404].copy_from_slice(&0x8400_u16.to_le_bytes());
        let findings = scan(&bytes);
        let inventory = findings[0].inventory.as_ref().unwrap();
        assert!(inventory.entries.is_empty());
        assert_eq!(inventory.held[0].reason, Hold::SourceOverlap);
        bytes[16 + 0x400..16 + 0x410].fill(0);
        let findings = scan(&bytes);
        let inventory = findings[0].inventory.as_ref().unwrap();
        assert!(inventory.entries.is_empty());
        assert!(inventory.held.is_empty());
    }

    #[test]
    fn high_selectors_and_slot_runs_are_bounded_by_abi() {
        let mut bytes = synthetic_rom();
        let descriptors = bytes[16 + 0x400..16 + 0x410].to_vec();
        bytes[16 + 0x400..16 + 0x410].fill(0);
        bytes[16 + 0x600..16 + 0x610].copy_from_slice(&descriptors);
        let findings = scan(&bytes);
        assert_eq!(
            findings[0].inventory.as_ref().unwrap().entries[0].raw_selector,
            128
        );
        let mut layout = layout(&bytes);
        layout.selectors = 128;
        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: 100_000,
        };
        let result =
            super::super::inventory(&bytes[16..16 + 0x2000], 16, &layout, &mut budget, &mut 8192)
                .unwrap();
        assert!(result.entries.is_empty());
        for channel in 0..4 {
            bytes[16 + 0x600 + channel * 4] = 63 + channel as u8 * 21;
        }
        assert_eq!(
            scan(&bytes)[0].inventory.as_ref().unwrap().entries[0].slot_base,
            63
        );
        bytes[16 + 0x600] = 85;
        assert!(
            scan(&bytes)[0]
                .inventory
                .as_ref()
                .unwrap()
                .entries
                .is_empty()
        );
    }

    #[test]
    fn four_descriptor_groups_stop_at_the_input_domain_boundary() {
        for (inputs, raw, accepted) in [
            (128, 124, true),
            (128, 125, false),
            (256, 252, true),
            (256, 253, false),
        ] {
            let mut bytes = synthetic_rom();
            let descriptors = bytes[16 + 0x400..16 + 0x410].to_vec();
            bytes[16 + 0x400..16 + 0x410].fill(0);
            let at = 16 + 0x400 + usize::from(raw) * 4;
            bytes[at..at + 16].copy_from_slice(&descriptors);
            let mut layout = layout(&bytes);
            layout.selectors = inputs;
            let cancel = AtomicBool::new(false);
            let mut budget = Budget {
                cancel: &cancel,
                remaining: 1_000_000,
            };
            let result = super::super::inventory(
                &bytes[16..16 + 0x2000],
                16,
                &layout,
                &mut budget,
                &mut 8192,
            )
            .unwrap();
            assert_eq!(
                (result.selector_input_count, result.inspected_selectors),
                (inputs, inputs - 3)
            );
            assert_eq!(
                result.entries.iter().any(|entry| entry.raw_selector == raw),
                accepted
            );
            assert!(
                result
                    .entries
                    .iter()
                    .all(|entry| entry.raw_selector < inputs - 3)
            );
        }
    }

    #[test]
    fn budget_and_cancellation_never_publish_partial_inventory() {
        let bytes = synthetic_rom();
        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: 20,
        };
        let mut findings = Vec::new();
        assert!(super::super::scan(&bytes, &mut findings, &mut budget, 10).is_err());
        assert!(findings.is_empty());
        let cancel = AtomicBool::new(true);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: 100_000,
        };
        assert_eq!(
            super::super::scan(&bytes, &mut findings, &mut budget, 10),
            Err(ScanStop::Cancelled)
        );
        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: 100_000,
        };
        assert_eq!(
            super::super::scan(&bytes, &mut findings, &mut budget, 0),
            Err(ScanStop::CandidateLimit)
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn fragmented_graphs_and_repeated_banks_have_a_global_output_bound() {
        let mut bytes = synthetic_rom();
        for channel in 0..4 {
            let start = 16 + 0x800 + channel * 16;
            bytes[start + 4..start + 8].copy_from_slice(&[1, 1, 0xb0, 100]);
            bytes[start + 200] = 0xff;
        }
        let original = bytes[16..16 + 0x2000].to_vec();
        bytes[16 + 0x2000..16 + 0x4000].copy_from_slice(&original);
        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: 1_000_000,
        };
        let mut findings = Vec::new();
        let result = super::super::scan_with_span_limit(&bytes, &mut findings, &mut budget, 10, 25);
        assert_eq!(result, Err(ScanStop::InventoryLimit));
        assert_eq!(findings.len(), 1);
        let spans = &findings[0].inventory.as_ref().unwrap().entries[0].tracks[0].source_spans;
        assert_eq!(spans.len(), 2);
        assert!(spans[0].offset + spans[0].byte_len < spans[1].offset);
    }
}
