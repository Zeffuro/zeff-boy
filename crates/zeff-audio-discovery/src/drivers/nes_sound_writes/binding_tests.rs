#[cfg(test)]
use std::sync::atomic::AtomicBool;

#[cfg(test)]
use crate::{Budget, ScanStop};

use super::record_tests::record_rom;

const SCHEDULER: u16 = 0xa000;
const RECORD_ENTRY: u16 = 0x8300;
const ENTRY: u16 = 0xa012;
const FETCH: u16 = 0xa066;
const DISPATCH: u16 = 0xa080;
const DISPATCH_TABLE: u16 = 0xa0c0;
const HANDLER: u16 = 0xa200;

pub fn synthetic_binding_rom() -> Vec<u8> {
    binding_rom(2, 0)
}

pub fn synthetic_head_edge_rom() -> Vec<u8> {
    head_edge_rom(2, 0)
}

pub(crate) fn head_edge_rom(banks: u8, relocation: u16) -> Vec<u8> {
    let mut rom = binding_rom(banks, relocation);
    let table = DISPATCH_TABLE + relocation;
    let handler = HANDLER + relocation;
    put(
        &mut rom,
        table + u16::from(0x93_u8.wrapping_mul(2)),
        &handler.to_le_bytes(),
    );
    put(
        &mut rom,
        handler,
        &[
            0xc8, 0xa2, 0, 0xb1, 0x60, 0x9d, 0xb1, 6, 0xc8, 0xe8, 0xe0, 8, 0xd0, 0xf5, 0x60,
        ],
    );
    for raw in 9_u16..=12 {
        for stream in 0..3 {
            put(
                &mut rom,
                0x9800 + relocation + raw * 32 + stream * 4,
                &[0x93],
            );
        }
    }
    rom
}

fn binding_rom(banks: u8, relocation: u16) -> Vec<u8> {
    let scheduler = SCHEDULER + relocation;
    let record_entry = RECORD_ENTRY + relocation;
    let entry = ENTRY + relocation;
    let fetch = FETCH + relocation;
    let dispatch = DISPATCH + relocation;
    let dispatch_table = DISPATCH_TABLE + relocation;
    let mut rom = record_rom(banks, relocation, 0x50, 0x0600);
    put(
        &mut rom,
        0x8000,
        &[
            0x20,
            0x00,
            0x81,
            0x20,
            record_entry as u8,
            (record_entry >> 8) as u8,
            0x20,
            scheduler as u8,
            (scheduler >> 8) as u8,
            0x60,
        ],
    );
    put(
        &mut rom,
        scheduler,
        &[
            0xa0,
            0,
            0xa2,
            0x80,
            0x20,
            entry as u8,
            (entry >> 8) as u8,
            0xa2,
            0x96,
            0xa0,
            1,
            0x20,
            entry as u8,
            (entry >> 8) as u8,
            0xa2,
            0xac,
            0xa0,
            2,
        ],
    );
    put(
        &mut rom,
        entry,
        &[
            0x84, 0xe1, 0xa9, 0, 0x9d, 0x13, 6, 0xbd, 0, 6, 0x85, 0x60, 0xbd, 1, 6, 0x85, 0x61,
            0x8a, 0x18, 0x69, 9, 0x7d, 2, 6, 0x85, 0xe3, 0x86, 0x73, 0xbd, 3, 6, 0xf0, 0x2d,
        ],
    );
    put(
        &mut rom,
        0xa060 + relocation,
        &[0xa9, 0, 0x9d, 0x15, 6, 0xa8],
    );
    put(
        &mut rom,
        fetch,
        &[
            0xb1,
            0x60,
            0x10,
            6,
            0xa6,
            0x73,
            0x20,
            dispatch as u8,
            (dispatch >> 8) as u8,
            0x60,
        ],
    );
    put(
        &mut rom,
        dispatch,
        &[
            0x0a,
            0xa8,
            0xb9,
            dispatch_table as u8,
            (dispatch_table >> 8) as u8,
            0x85,
            0x70,
            0xb9,
            (dispatch_table + 1) as u8,
            ((dispatch_table + 1) >> 8) as u8,
            0x85,
            0x71,
            0xa0,
            0,
            0xa6,
            0x73,
            0x6c,
            0x70,
            0,
        ],
    );
    put(&mut rom, dispatch_table, &[0, 0]);
    rom
}

fn offset(rom: &[u8], address: u16) -> usize {
    16 + (usize::from(address) - 0x8000) % (usize::from(rom[4]) * 0x4000)
}

fn put(rom: &mut [u8], address: u16, bytes: &[u8]) {
    let at = offset(rom, address);
    rom[at..at + bytes.len()].copy_from_slice(bytes);
}

#[cfg(test)]
fn scan(rom: &[u8], work: u64, cancelled: bool) -> Result<Vec<super::DriverCandidate>, ScanStop> {
    let cancel = AtomicBool::new(cancelled);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: work,
    };
    let mut findings = Vec::new();
    super::scan(rom, &mut findings, &mut budget, 8)?;
    Ok(findings)
}

#[test]
fn triple_records_bind_to_the_guarded_fetch_state() {
    let rom = synthetic_binding_rom();
    let findings = scan(&rom, 100_000, false).unwrap();
    let code = findings[0].code.as_ref().unwrap();
    assert_eq!(code.command_dispatches.len(), 1);
    assert_eq!(code.command_dispatches[0].fetches.len(), 1);
    let streams = code.selector_consumers[0]
        .records
        .iter()
        .filter(|record| record.header & 0x80 != 0)
        .flat_map(|record| record.streams.iter())
        .collect::<Vec<_>>();
    assert_eq!(streams.len(), 12);
    assert_eq!(
        streams
            .iter()
            .filter(|stream| stream.fetch_binding.is_some())
            .count(),
        12
    );
    for (index, stream) in streams.iter().enumerate() {
        let binding = stream.fetch_binding.as_ref().unwrap();
        assert_eq!(
            binding.state_pointer_address,
            [0x680, 0x696, 0x6ac][index % 3]
        );
        assert_eq!(binding.scheduler_cpu_address, SCHEDULER);
        assert_eq!(binding.scheduler_span.byte_len, 18);
        assert_eq!(binding.consumer_entry_cpu_address, ENTRY);
        assert_eq!(binding.consumer_entry_span.byte_len, 33);
        assert_eq!(binding.fetch_cpu_address, FETCH);
        assert_eq!(binding.fetch_span.byte_len, 9);
        assert_eq!(binding.evidence.len(), 5);
        for evidence in &binding.evidence {
            let start = evidence.span.offset as usize;
            assert_eq!(
                evidence.sha256,
                zeff_firmware::sha256_hex(&rom[start..start + evidence.span.byte_len as usize])
            );
        }
    }
}

#[test]
fn mirrored_16k_mapping_keeps_relocated_binding_addresses_and_source_spans() {
    let rom = binding_rom(1, 0x4000);
    let findings = scan(&rom, 100_000, false).unwrap();
    let code = findings[0].code.as_ref().unwrap();
    let bindings = code.selector_consumers[0]
        .records
        .iter()
        .flat_map(|record| record.streams.iter())
        .filter_map(|stream| stream.fetch_binding.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(bindings.len(), 12);
    for binding in bindings {
        assert_eq!(binding.scheduler_cpu_address, SCHEDULER + 0x4000);
        assert_eq!(binding.consumer_entry_cpu_address, ENTRY + 0x4000);
        assert_eq!(binding.fetch_cpu_address, FETCH + 0x4000);
        assert_eq!(
            binding.scheduler_span.offset,
            16 + u32::from(SCHEDULER - 0x8000)
        );
    }
}

#[test]
fn slot_mutation_holds_only_the_matching_triple_field() {
    let mut rom = synthetic_binding_rom();
    put(&mut rom, SCHEDULER + 3, &[0x60]);
    let findings = scan(&rom, 100_000, false).unwrap();
    let code = findings[0].code.as_ref().unwrap();
    assert_eq!(code.selector_consumers[0].records.len(), 16);
    let bound = code.selector_consumers[0]
        .records
        .iter()
        .flat_map(|record| record.streams.iter())
        .filter(|stream| stream.fetch_binding.is_some())
        .count();
    assert_eq!(bound, 8);
}

#[test]
fn bridge_mutations_hold_bindings_without_removing_records() {
    for (address, value) in [
        (ENTRY + 8, 1),
        (ENTRY + 26, 0x85),
        (ENTRY + 32, 0),
        (0xa065, 0xaa),
        (FETCH + 1, 0x62),
        (DISPATCH + 15, 0x74),
        (ENTRY + 6, 0xff),
        (0x8407, 0x80),
    ] {
        let mut rom = synthetic_binding_rom();
        put(&mut rom, address, &[value]);
        let findings = scan(&rom, 100_000, false).unwrap();
        let code = findings[0].code.as_ref().unwrap();
        assert_eq!(
            code.selector_consumers[0].records.len(),
            16,
            "mutation {address:04x}"
        );
        assert_eq!(
            code.selector_consumers[0]
                .records
                .iter()
                .flat_map(|record| record.streams.iter())
                .filter(|stream| stream.fetch_binding.is_some())
                .count(),
            0,
            "mutation {address:04x}"
        );
    }
}

#[test]
fn stack_and_cross_slot_state_aliases_hold_every_binding() {
    let mut stack = synthetic_binding_rom();
    for address in [
        ENTRY + 6,
        ENTRY + 9,
        ENTRY + 14,
        ENTRY + 23,
        ENTRY + 30,
        0xa064,
        0x840d,
        0x8413,
        0x8419,
        0x841f,
        0x8425,
        0x842b,
    ] {
        put(&mut stack, address, &[1]);
    }
    let mut overlap = synthetic_binding_rom();
    put(&mut overlap, SCHEDULER + 8, &[0x82]);
    for rom in [stack, overlap] {
        let findings = scan(&rom, 100_000, false).unwrap();
        let code = findings[0].code.as_ref().unwrap();
        assert_eq!(code.selector_consumers[0].records.len(), 16);
        assert!(
            code.selector_consumers[0]
                .records
                .iter()
                .flat_map(|record| record.streams.iter())
                .all(|stream| stream.fetch_binding.is_none())
        );
    }
}

#[test]
fn binding_work_and_cancellation_never_publish_candidates() {
    let rom = synthetic_binding_rom();
    assert_eq!(scan(&rom, 0, false), Err(ScanStop::WorkLimit));
    assert_eq!(scan(&rom, 100_000, true), Err(ScanStop::Cancelled));
}

#[test]
fn scheduler_and_reload_mutations_preserve_records_without_bindings() {
    for patches in [
        vec![(SCHEDULER + 10, 2)],
        vec![(SCHEDULER + 5, 6), (SCHEDULER + 6, 0x81)],
        vec![
            (SCHEDULER + 5, 6),
            (SCHEDULER + 6, 0x81),
            (SCHEDULER + 12, 6),
            (SCHEDULER + 13, 0x81),
        ],
        vec![(ENTRY + 16, 0x62)],
    ] {
        let mut rom = synthetic_binding_rom();
        for (address, value) in patches {
            put(&mut rom, address, &[value]);
        }
        let findings = scan(&rom, 100_000, false).unwrap();
        let records = &findings[0].code.as_ref().unwrap().selector_consumers[0].records;
        assert_eq!(records.len(), 16);
        assert!(
            records
                .iter()
                .flat_map(|record| &record.streams)
                .all(|stream| stream.fetch_binding.is_none())
        );
    }
}

#[test]
fn held_stream_targets_keep_their_dispositions_and_other_bindings() {
    use crate::drivers::CodePointerDisposition;
    for (target, disposition) in [
        (0x8100_u16, CodePointerDisposition::DecodedCode),
        (0x9200, CodePointerDisposition::PointerTable),
        (0x94a0, CodePointerDisposition::RecordPrefix),
        (0x7000, CodePointerDisposition::Unmapped),
    ] {
        let mut rom = synthetic_binding_rom();
        put(&mut rom, 0x9492, &target.to_le_bytes());
        let findings = scan(&rom, 100_000, false).unwrap();
        let records = &findings[0].code.as_ref().unwrap().selector_consumers[0].records;
        let stream = &records
            .iter()
            .find(|record| record.raw_selector == 9)
            .unwrap()
            .streams[0];
        assert_eq!(stream.disposition, disposition);
        assert!(stream.fetch_binding.is_none());
        assert_eq!(
            records
                .iter()
                .flat_map(|record| &record.streams)
                .filter(|stream| stream.fetch_binding.is_some())
                .count(),
            11
        );
    }
}

#[test]
fn wrapping_unrelated_pattern_keeps_valid_bindings() {
    let mut rom = synthetic_binding_rom();
    put(&mut rom, 0xfffe, &[0xa0, 0xc0]);
    put(&mut rom, 0xc0a0, &[0x20, 0xfe, 0xff, 0x60]);
    let findings = scan(&rom, 100_000, false).unwrap();
    let code = findings[0].code.as_ref().unwrap();
    assert_eq!(
        code.selector_consumers[0]
            .records
            .iter()
            .flat_map(|record| &record.streams)
            .filter(|stream| stream.fetch_binding.is_some())
            .count(),
        12
    );
}

#[test]
fn late_binding_work_exhaustion_does_not_publish_a_candidate() {
    let rom = synthetic_binding_rom();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut findings = Vec::new();
    super::scan(&rom, &mut findings, &mut budget, 8).unwrap();
    let used = 100_000 - budget.remaining;
    let mut exhausted = Budget {
        cancel: &cancel,
        remaining: used - 1,
    };
    let mut partial = Vec::new();
    assert_eq!(
        super::scan(&rom, &mut partial, &mut exhausted, 8),
        Err(ScanStop::WorkLimit)
    );
    assert!(partial.is_empty());
}

#[test]
fn binding_limit_does_not_publish_a_partial_candidate() {
    let mut rom = synthetic_binding_rom();
    put(&mut rom, 0x8320, &[21]);
    for raw in (1_u16..21).filter(|raw| !matches!(*raw, 17 | 18)) {
        let record = 0x9400 + raw * 16;
        let stream = 0x9800 + raw * 32;
        put(&mut rom, 0x9200 + (raw - 1) * 2, &record.to_le_bytes());
        put(
            &mut rom,
            record,
            &[
                0xb4,
                0xf1,
                stream as u8,
                (stream >> 8) as u8,
                (stream + 4) as u8,
                ((stream + 4) >> 8) as u8,
                (stream + 8) as u8,
                ((stream + 8) >> 8) as u8,
            ],
        );
    }
    put(&mut rom, 0x8320, &[19]);
    let complete = scan(&rom, 100_000, false).unwrap();
    assert_eq!(
        complete[0].code.as_ref().unwrap().selector_consumers[0]
            .records
            .iter()
            .flat_map(|record| &record.streams)
            .filter(|stream| stream.fetch_binding.is_some())
            .count(),
        48
    );
    put(&mut rom, 0x8320, &[20]);
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut findings = Vec::new();
    assert_eq!(
        super::scan(&rom, &mut findings, &mut budget, 8),
        Err(ScanStop::InventoryLimit)
    );
    assert!(findings.is_empty());
}
