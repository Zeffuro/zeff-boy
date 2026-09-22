#[cfg(test)]
use std::sync::atomic::AtomicBool;

#[cfg(test)]
use crate::{Budget, ScanStop, drivers::CodeCommandDispatch};

const ROOT: u16 = 0x8000;
const WRITER: u16 = 0x8100;
const FETCH: u16 = 0x8200;
const SECOND_FETCH: u16 = 0x8220;
const DISPATCH: u16 = 0x8400;
const TABLE: u16 = 0x8600;

pub fn synthetic_dispatch_rom() -> Vec<u8> {
    dispatch_rom(2, 0, 0x62, 0x70, 0x73)
}

fn dispatch_rom(
    banks: u8,
    relocation: u16,
    source_pointer: u8,
    target_pointer: u8,
    saved_index: u8,
) -> Vec<u8> {
    let fetch = FETCH + relocation;
    let second_fetch = SECOND_FETCH + relocation;
    let dispatch = DISPATCH + relocation;
    let table = TABLE + relocation;
    let mut rom = nrom(banks);
    code(
        &mut rom,
        ROOT,
        &[
            0x20,
            WRITER as u8,
            (WRITER >> 8) as u8,
            0x20,
            fetch as u8,
            (fetch >> 8) as u8,
            0x20,
            second_fetch as u8,
            (second_fetch >> 8) as u8,
            0x60,
        ],
    );
    code(&mut rom, WRITER, &[0x8d, 0, 0x40, 0x8d, 1, 0x40, 0x60]);
    code(
        &mut rom,
        fetch,
        &[
            0xb1,
            source_pointer,
            0x10,
            8,
            0x8d,
            0,
            6,
            0xa6,
            saved_index,
            0x20,
            dispatch as u8,
            (dispatch >> 8) as u8,
            0x60,
        ],
    );
    code(
        &mut rom,
        second_fetch,
        &[
            0xb1,
            source_pointer,
            0x10,
            5,
            0xa6,
            saved_index,
            0x20,
            dispatch as u8,
            (dispatch >> 8) as u8,
            0x60,
        ],
    );
    code(
        &mut rom,
        dispatch,
        &[
            0x0a,
            0xa8,
            0xb9,
            table as u8,
            (table >> 8) as u8,
            0x85,
            target_pointer,
            0xb9,
            (table + 1) as u8,
            ((table + 1) >> 8) as u8,
            0x85,
            target_pointer + 1,
            0xa0,
            0,
            0xa6,
            saved_index,
            0x6c,
            target_pointer,
            0,
        ],
    );
    code(&mut rom, table, &[0, 0]);
    vectors(&mut rom, ROOT, ROOT, ROOT);
    rom
}

fn nrom(banks: u8) -> Vec<u8> {
    let mut rom = vec![0; 16 + usize::from(banks) * 0x4000];
    rom[..16].copy_from_slice(&[
        b'N', b'E', b'S', 0x1a, banks, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    rom
}

fn offset(rom: &[u8], address: u16) -> usize {
    let prg_len = usize::from(rom[4]) * 0x4000;
    16 + if prg_len == 0x4000 {
        (usize::from(address) - 0x8000) & 0x3fff
    } else {
        usize::from(address) - 0x8000
    }
}

fn code(rom: &mut [u8], address: u16, bytes: &[u8]) {
    let at = offset(rom, address);
    rom[at..at + bytes.len()].copy_from_slice(bytes);
}

fn vectors(rom: &mut [u8], nmi: u16, reset: u16, irq: u16) {
    for (address, target) in [(0xfffa, nmi), (0xfffc, reset), (0xfffe, irq)] {
        code(rom, address, &target.to_le_bytes());
    }
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

#[cfg(test)]
fn dispatches(rom: &[u8]) -> Vec<CodeCommandDispatch> {
    scan(rom, 100_000, false).unwrap()[0]
        .code
        .as_ref()
        .unwrap()
        .command_dispatches
        .clone()
}

#[test]
fn guarded_fetches_share_one_operand_normalized_dispatch() {
    let rom = synthetic_dispatch_rom();
    let found = dispatches(&rom);
    assert_eq!(found.len(), 1);
    let dispatch = &found[0];
    assert_eq!(dispatch.entry_cpu_address, DISPATCH);
    assert_eq!(
        dispatch.entry_span.offset,
        16 + u32::from(DISPATCH - 0x8000)
    );
    assert_eq!(dispatch.entry_span.byte_len, 19);
    assert_eq!(dispatch.table_cpu_address, TABLE);
    assert_eq!(dispatch.target_pointer_address, 0x70);
    assert_eq!(dispatch.saved_index_address, 0x73);
    assert_eq!(dispatch.fetches.len(), 2);
    assert_eq!(
        dispatch
            .fetches
            .iter()
            .map(|fetch| (
                fetch.cpu_address,
                fetch.call_cpu_address,
                fetch.event_cpu_address
            ))
            .collect::<Vec<_>>(),
        vec![
            (FETCH, FETCH + 9, FETCH + 12),
            (SECOND_FETCH, SECOND_FETCH + 6, SECOND_FETCH + 9)
        ]
    );
    for fetch in &dispatch.fetches {
        assert_eq!(fetch.source_pointer_address, 0x62);
        assert_eq!(fetch.audio_call.cpu_address, ROOT);
        assert_eq!(fetch.audio_call.target_cpu_address, WRITER);
        assert_eq!(fetch.audio_call.writer_cpu_address, WRITER);
    }
    for evidence in &dispatch.evidence {
        let start = evidence.span.offset as usize;
        assert_eq!(
            evidence.sha256,
            zeff_firmware::sha256_hex(&rom[start..start + evidence.span.byte_len as usize])
        );
    }
}

#[test]
fn relocation_and_16k_mirroring_preserve_cpu_and_source_spans() {
    let rom = dispatch_rom(1, 0x4000, 0x2a, 0x38, 0x49);
    let found = dispatches(&rom);
    let dispatch = &found[0];
    assert_eq!(dispatch.entry_cpu_address, DISPATCH + 0x4000);
    assert_eq!(
        dispatch.entry_span.offset,
        16 + u32::from(DISPATCH - 0x8000)
    );
    assert_eq!(dispatch.table_cpu_address, TABLE + 0x4000);
    assert_eq!(dispatch.fetches[0].cpu_address, FETCH + 0x4000);
    assert_eq!(dispatch.fetches[0].source_pointer_address, 0x2a);
    assert_eq!(dispatch.target_pointer_address, 0x38);
    assert_eq!(dispatch.saved_index_address, 0x49);
}

#[test]
fn malformed_guard_dispatch_and_anchor_hold_all_dispatch_evidence() {
    for (address, value) in [
        (FETCH + 1, 0x70),
        (FETCH + 2, 0x30),
        (FETCH + 7, 0xa5),
        (FETCH + 8, 0x74),
    ] {
        let mut rom = synthetic_dispatch_rom();
        code(&mut rom, address, &[value]);
        let found = dispatches(&rom);
        assert_eq!(found.len(), 1, "mutation at {address:04x}");
        assert_eq!(found[0].fetches.len(), 1, "mutation at {address:04x}");
        assert_eq!(found[0].fetches[0].cpu_address, SECOND_FETCH);
    }

    for (address, value) in [
        (DISPATCH + 7, 0xbd),
        (DISPATCH + 11, 0x72),
        (DISPATCH + 15, 0x70),
        (DISPATCH + 17, 0x71),
    ] {
        let mut rom = synthetic_dispatch_rom();
        code(&mut rom, address, &[value]);
        assert!(dispatches(&rom).is_empty(), "mutation at {address:04x}");
    }

    let mut unanchored = synthetic_dispatch_rom();
    code(
        &mut unanchored,
        ROOT,
        &[
            0x8d,
            0,
            0x40,
            0x8d,
            1,
            0x40,
            0x20,
            FETCH as u8,
            (FETCH >> 8) as u8,
            0x60,
        ],
    );
    assert!(dispatches(&unanchored).is_empty());
}

#[test]
fn dispatch_scans_charge_work_and_do_not_publish_partial_candidates() {
    let rom = synthetic_dispatch_rom();
    assert_eq!(scan(&rom, 0, false), Err(ScanStop::WorkLimit));
    assert_eq!(scan(&rom, 100_000, true), Err(ScanStop::Cancelled));
}
