#[cfg(test)]
use std::sync::atomic::AtomicBool;

#[cfg(test)]
use crate::{Budget, ScanStop};

#[cfg(test)]
use super::super::CodePointerDisposition;

const CONSUMER: u16 = 0x8300;
const TABLE: u16 = 0x9200;
const WRITER: u16 = 0x8100;

pub fn synthetic_selector_rom() -> Vec<u8> {
    selector_rom(2, CONSUMER, TABLE, 0x50, 0x62, 0x73)
}

fn selector_rom(
    banks: u8,
    consumer: u16,
    table: u16,
    selector: u8,
    pointer: u8,
    header: u8,
) -> Vec<u8> {
    let mut rom = nrom(banks);
    code(
        &mut rom,
        0x8000,
        &[
            0x20,
            0,
            0x81,
            0x20,
            consumer as u8,
            (consumer >> 8) as u8,
            0x60,
        ],
    );
    code(&mut rom, WRITER, &[0x8d, 0, 0x40, 0x8d, 1, 0x40, 0x60]);
    code(
        &mut rom,
        consumer,
        &consumer_bytes(selector, pointer, header, [17, 18], 20, table),
    );
    for raw in 1_u8..20 {
        let target = match raw {
            15 => WRITER,
            16 => table,
            19 => 0x7000,
            _ => 0x9400 + u16::from(raw) * 4,
        };
        put_word(&mut rom, table + u16::from(raw - 1) * 2, target);
    }
    vectors(&mut rom, 0x8000, 0x8000, 0x8000);
    rom
}

fn consumer_bytes(
    selector: u8,
    pointer: u8,
    header: u8,
    controls: [u8; 2],
    bound: u8,
    table: u16,
) -> [u8; 56] {
    let table_high = table + 1;
    [
        0xa6,
        selector,
        0xd0,
        1,
        0x60,
        0xe0,
        controls[0],
        0xd0,
        4,
        0xa9,
        0x80,
        0x30,
        6,
        0xe0,
        controls[1],
        0xd0,
        10,
        0xa9,
        0x40,
        0x8d,
        0,
        6,
        0xa9,
        0,
        0x85,
        selector,
        0x60,
        0xa6,
        selector,
        0x30,
        5,
        0xe0,
        bound,
        0x90,
        1,
        0x60,
        0xca,
        0x8a,
        0x0a,
        0xa8,
        0xb9,
        table as u8,
        (table >> 8) as u8,
        0x85,
        pointer,
        0xb9,
        table_high as u8,
        (table_high >> 8) as u8,
        0x85,
        pointer + 1,
        0xa0,
        0,
        0xb1,
        pointer,
        0x85,
        header,
    ]
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

fn put_word(rom: &mut [u8], address: u16, value: u16) {
    code(rom, address, &value.to_le_bytes());
}

fn vectors(rom: &mut [u8], nmi: u16, reset: u16, irq: u16) {
    for (address, target) in [(0xfffa, nmi), (0xfffc, reset), (0xfffe, irq)] {
        put_word(rom, address, target);
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
fn consumer(rom: &[u8]) -> Option<super::super::CodeSelectorConsumer> {
    scan(rom, 100_000, false)
        .unwrap()
        .first()?
        .code
        .as_ref()?
        .selector_consumers
        .first()
        .cloned()
}

#[test]
fn decoded_selector_consumer_reports_only_bounded_positive_rows() {
    let rom = synthetic_selector_rom();
    let found = consumer(&rom).expect("decoded consumer");
    assert_eq!(found.audio_call.cpu_address, 0x8000);
    assert_eq!(found.audio_call.target_cpu_address, WRITER);
    assert_eq!(found.audio_call.span.offset, 16);
    assert_eq!(found.audio_call.writer_cpu_address, WRITER);
    assert_eq!(found.audio_call.writer_span.offset, 16 + 0x100);
    assert_eq!(found.call_cpu_address, 0x8003);
    assert_eq!(found.entry_cpu_address, CONSUMER);
    assert_eq!(
        found.entry_span.offset,
        16 + usize::from(CONSUMER - 0x8000) as u32
    );
    assert_eq!(found.entry_span.byte_len, 56);
    assert_eq!(found.call_span.offset, 19);
    assert_eq!(found.call_span.byte_len, 3);
    assert_eq!(found.selector_address, 0x50);
    assert_eq!(found.upper_bound_exclusive, 20);
    assert_eq!(found.pointer_address, 0x62);
    assert_eq!(found.header_address, 0x73);
    assert_eq!(found.table_cpu_address, TABLE);
    assert_eq!(
        found.pointer_aperture.offset,
        16 + usize::from(TABLE - 0x8000) as u32
    );
    assert_eq!(found.pointer_aperture.byte_len, 38);
    assert_eq!(found.control_address, 0x0600);
    assert_eq!(
        found
            .controls
            .iter()
            .map(|control| (control.raw_selector, control.value))
            .collect::<Vec<_>>(),
        vec![(17, 0x80), (18, 0x40)]
    );
    assert_eq!(
        found
            .pointers
            .iter()
            .map(|row| row.raw_selector)
            .collect::<Vec<_>>(),
        (1..=16).chain(std::iter::once(19)).collect::<Vec<_>>()
    );
    assert!(
        found
            .pointers
            .iter()
            .filter(|row| row.raw_selector <= 14)
            .all(|row| row.disposition == CodePointerDisposition::Unparsed)
    );
    assert_eq!(found.pointers[0].entry_span.offset, 16 + 0x1200);
    assert_eq!(found.pointers[0].entry_span.byte_len, 2);
    assert_eq!(found.pointers[0].target_cpu_address, 0x9404);
    assert_eq!(found.pointers[0].target_span.unwrap().offset, 16 + 0x1404);
    assert_eq!(
        found.pointers[14].disposition,
        CodePointerDisposition::DecodedCode
    );
    assert_eq!(
        found.pointers[15].disposition,
        CodePointerDisposition::PointerTable
    );
    let unmapped = found.pointers.last().unwrap();
    assert_eq!(unmapped.raw_selector, 19);
    assert_eq!(unmapped.disposition, CodePointerDisposition::Unmapped);
    assert_eq!(unmapped.target_span, None);
}

#[test]
fn renamed_relocated_16k_consumer_keeps_cpu_and_source_addresses_exact() {
    let rom = selector_rom(1, 0x9300, 0xb200, 0x19, 0x2a, 0x3b);
    let found = consumer(&rom).expect("mirrored consumer");
    assert_eq!(found.entry_cpu_address, 0x9300);
    assert_eq!(found.entry_span.offset, 16 + 0x1300);
    assert_eq!(found.call_cpu_address, 0x8003);
    assert_eq!(found.selector_address, 0x19);
    assert_eq!(found.pointer_address, 0x2a);
    assert_eq!(found.header_address, 0x3b);
    assert_eq!(found.table_cpu_address, 0xb200);
    assert_eq!(found.pointer_aperture.offset, 16 + 0x3200);
}

#[test]
fn selector_consumer_requires_the_exact_decoded_shape_and_association() {
    for (at, value) in [
        (3, 2),
        (8, 3),
        (12, 5),
        (16, 9),
        (32, 18),
        (47, 0),
        (49, 0x64),
    ] {
        let mut rom = synthetic_selector_rom();
        let entry = offset(&rom, CONSUMER);
        rom[entry + at] = value;
        assert!(consumer(&rom).is_none(), "mutation at consumer byte {at}");
    }

    let mut unreachable = synthetic_selector_rom();
    code(&mut unreachable, 0x8003, &[0x60]);
    assert!(consumer(&unreachable).is_none());

    let mut not_adjacent = synthetic_selector_rom();
    code(&mut not_adjacent, 0x8003, &[0xea, 0x20, 0, 0x83, 0x60]);
    assert!(consumer(&not_adjacent).is_none());

    let mut naked_tail = synthetic_selector_rom();
    code(&mut naked_tail, 0x8003, &[0x20, 0x1b, 0x83, 0x60]);
    assert!(consumer(&naked_tail).is_none());

    let mut aperture_overlaps_code = synthetic_selector_rom();
    let entry = offset(&aperture_overlaps_code, CONSUMER);
    aperture_overlaps_code[entry + 41..entry + 43].copy_from_slice(&WRITER.to_le_bytes());
    aperture_overlaps_code[entry + 46..entry + 48].copy_from_slice(&(WRITER + 1).to_le_bytes());
    assert!(consumer(&aperture_overlaps_code).is_none());
}

#[test]
fn selector_scan_stops_for_budget_cancellation_and_consumer_inventory_limit() {
    assert_eq!(
        scan(&synthetic_selector_rom(), 0, false),
        Err(ScanStop::WorkLimit)
    );
    assert_eq!(
        scan(&synthetic_selector_rom(), 100_000, true),
        Err(ScanStop::Cancelled)
    );
    assert_eq!(
        scan(&many_consumers_rom(17), 100_000, false),
        Err(ScanStop::InventoryLimit)
    );
    assert_eq!(
        scan(&many_consumers_rom(16), 100_000, false).unwrap()[0]
            .code
            .as_ref()
            .unwrap()
            .selector_consumers
            .len(),
        16
    );
}

#[cfg(test)]
fn many_consumers_rom(count: usize) -> Vec<u8> {
    let mut rom = nrom(2);
    let mut root = Vec::with_capacity(count * 6 + 1);
    for index in 0..count {
        let entry = 0x8400 + u16::try_from(index).unwrap() * 0x80;
        root.extend([0x20, 0, 0x82, 0x20, entry as u8, (entry >> 8) as u8]);
        let table = 0xb000 + u16::try_from(index).unwrap() * 0x30;
        code(
            &mut rom,
            entry,
            &consumer_bytes(0x50, 0x62, 0x73, [2, 3], 4, table),
        );
        put_word(&mut rom, table, 0x9600);
        put_word(&mut rom, table + 2, 0x9604);
        put_word(&mut rom, table + 4, 0x9608);
    }
    root.push(0x60);
    code(&mut rom, 0x8000, &root);
    code(&mut rom, 0x8200, &[0x8d, 0, 0x40, 0x8d, 1, 0x40, 0x60]);
    vectors(&mut rom, 0x8000, 0x8000, 0x8000);
    rom
}
