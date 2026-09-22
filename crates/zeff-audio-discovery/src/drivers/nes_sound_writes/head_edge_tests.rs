use std::sync::atomic::AtomicBool;

use crate::{Budget, ScanStop, drivers::CodeConditionalHeadCommandEdge};

use super::binding_tests::{head_edge_rom, synthetic_head_edge_rom};

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

fn edges(rom: &[u8]) -> Vec<CodeConditionalHeadCommandEdge> {
    scan(rom, 100_000, false).unwrap()[0]
        .code
        .as_ref()
        .unwrap()
        .selector_consumers
        .iter()
        .flat_map(|consumer| &consumer.records)
        .flat_map(|record| &record.streams)
        .filter_map(|stream| stream.conditional_head_command_edge.clone())
        .collect()
}

fn binding_count(rom: &[u8]) -> usize {
    scan(rom, 1_000_000, false).unwrap()[0]
        .code
        .as_ref()
        .unwrap()
        .selector_consumers
        .iter()
        .flat_map(|consumer| &consumer.records)
        .flat_map(|record| &record.streams)
        .filter(|stream| stream.fetch_binding.is_some())
        .count()
}

#[test]
fn immutable_heads_select_the_fixed_duration_handler() {
    for (rom, handler, row) in [
        (synthetic_head_edge_rom(), 0xa200, 16 + 0x20e6),
        (head_edge_rom(1, 0x4000), 0xe200, 16 + 0x20e6),
    ] {
        let found = edges(&rom);
        assert_eq!(found.len(), 12);
        for edge in found {
            assert_eq!(edge.head_byte, 0x93);
            assert_eq!(edge.head_span.byte_len, 1);
            assert_eq!(edge.dispatch_row_span.offset, row);
            assert_eq!(edge.dispatch_row_span.byte_len, 2);
            assert_eq!(edge.handler_cpu_address, handler);
            assert_eq!(edge.handler_span.byte_len, 15);
            assert_eq!(edge.handler_span.offset, 16 + 0x2200);
            assert_eq!(edge.evidence.len(), 4);
            for span in [edge.head_span, edge.dispatch_row_span, edge.handler_span] {
                assert!(edge.evidence.iter().any(|evidence| evidence.span == span));
            }
            assert_eq!(edge.operand_count, 8);
            assert_eq!(
                (edge.destination_start, edge.destination_end_inclusive),
                (0x6b1, 0x6b8)
            );
            for evidence in &edge.evidence {
                let start = evidence.span.offset as usize;
                assert_eq!(
                    evidence.sha256,
                    zeff_firmware::sha256_hex(&rom[start..start + evidence.span.byte_len as usize])
                );
            }
        }
    }
}

#[test]
fn edge_mutations_hold_without_removing_bindings() {
    for (address, value, expected, bindings) in [
        (0x9920_u16, 0x13, 11, 12),
        (0xa0e6, 1, 0, 12),
        (0xa200, 0xea, 0, 12),
        (0xa204, 0x61, 0, 12),
        (0xa206, 0xb2, 0, 12),
        (0xa20b, 9, 0, 12),
        (0xa20d, 0, 0, 12),
        (0xa20e, 0xea, 0, 12),
    ] {
        let mut rom = synthetic_head_edge_rom();
        rom[16 + usize::from(address - 0x8000)] = value;
        let findings = scan(&rom, 100_000, false).unwrap();
        let code = findings[0].code.as_ref().unwrap();
        assert_eq!(
            code.selector_consumers
                .iter()
                .flat_map(|consumer| &consumer.records)
                .flat_map(|record| &record.streams)
                .filter(|stream| stream.fetch_binding.is_some())
                .count(),
            bindings
        );
        assert_eq!(
            code.selector_consumers
                .iter()
                .flat_map(|consumer| &consumer.records)
                .flat_map(|record| &record.streams)
                .filter(|stream| stream.conditional_head_command_edge.is_some())
                .count(),
            expected,
            "{address:04x}"
        );
    }
}

#[test]
fn head_edge_work_and_cancellation_are_atomic() {
    let rom = synthetic_head_edge_rom();
    assert_eq!(scan(&rom, 0, false), Err(ScanStop::WorkLimit));
    assert_eq!(scan(&rom, 100_000, true), Err(ScanStop::Cancelled));
}

fn put(rom: &mut [u8], address: u16, bytes: &[u8]) {
    let offset = 16 + (usize::from(address) - 0x8000) % (usize::from(rom[4]) * 0x4000);
    rom[offset..offset + bytes.len()].copy_from_slice(bytes);
}

fn capacity_rom(bound: u8) -> Vec<u8> {
    let mut rom = synthetic_head_edge_rom();
    put(&mut rom, 0x8320, &[bound]);
    for raw in (1_u16..u16::from(bound)).filter(|raw| !matches!(*raw, 17 | 18)) {
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
        for offset in [0, 4, 8] {
            put(&mut rom, stream + offset, &[0x93]);
        }
    }
    rom
}

#[test]
fn forty_eight_edges_publish_and_the_next_bound_field_is_atomic() {
    let complete = scan(&capacity_rom(19), 1_000_000, false).unwrap();
    assert_eq!(
        complete[0]
            .code
            .as_ref()
            .unwrap()
            .selector_consumers
            .iter()
            .flat_map(|consumer| &consumer.records)
            .flat_map(|record| &record.streams)
            .filter(|stream| stream.conditional_head_command_edge.is_some())
            .count(),
        48
    );
    assert_eq!(
        scan(&capacity_rom(20), 1_000_000, false),
        Err(ScanStop::InventoryLimit)
    );
}

#[test]
fn protected_rows_handlers_and_state_aliases_hold_edges() {
    for table in [0xffe0_u16, 0x91da, 0x946a, 0x98fa, 0x9fec] {
        let mut rom = synthetic_head_edge_rom();
        put(&mut rom, 0xa083, &[table as u8]);
        put(&mut rom, 0xa084, &[(table >> 8) as u8]);
        put(&mut rom, 0xa088, &[table.wrapping_add(1) as u8]);
        put(&mut rom, 0xa089, &[(table.wrapping_add(1) >> 8) as u8]);
        assert_eq!(binding_count(&rom), 12, "{table:04x}");
        assert!(edges(&rom).is_empty(), "{table:04x}");
    }
    let mut seam = head_edge_rom(1, 0x4000);
    for (address, value) in [
        (0xe083, 0xd9),
        (0xe084, 0xbf),
        (0xe088, 0xda),
        (0xe089, 0xbf),
    ] {
        put(&mut seam, address, &[value]);
    }
    assert_eq!(binding_count(&seam), 12);
    assert!(edges(&seam).is_empty());
    for handler in [0x9920_u16, 0x9490, 0x9200, 0xa013] {
        let mut rom = synthetic_head_edge_rom();
        put(&mut rom, 0xa0e6, &handler.to_le_bytes());
        assert_eq!(binding_count(&rom), 12, "{handler:04x}");
        assert!(edges(&rom).is_empty(), "{handler:04x}");
    }
    let mut alias = synthetic_head_edge_rom();
    for (address, value) in [(0xa00f, 0xb1), (0x8424, 0xb1), (0x842a, 0xb2)] {
        put(&mut alias, address, &[value]);
    }
    assert_eq!(binding_count(&alias), 12);
    assert!(edges(&alias).is_empty());
}

#[test]
fn a_valid_handler_cannot_own_retained_stream_data() {
    let mut rom = synthetic_head_edge_rom();
    let handler = rom[16 + 0x2200..16 + 0x220f].to_vec();
    put(&mut rom, 0x9920, &handler);
    put(&mut rom, 0xa0e6, &0x9920_u16.to_le_bytes());
    assert_eq!(binding_count(&rom), 12);
    assert!(edges(&rom).is_empty());
}

#[test]
fn graph_owned_handler_requires_consistent_instruction_boundaries() {
    let mut rom = synthetic_head_edge_rom();
    put(&mut rom, 0x8009, &[0x20, 0x00, 0xa2, 0x60]);
    assert_eq!(edges(&rom).len(), 12);
    put(&mut rom, 0x8009, &[0x20, 0xff, 0xa1, 0x60]);
    put(&mut rom, 0xa1ff, &[0xad]);
    assert_eq!(binding_count(&rom), 12);
    assert!(edges(&rom).is_empty());
}

#[test]
fn last_unit_of_head_edge_work_keeps_publication_atomic() {
    let rom = synthetic_head_edge_rom();
    let cancel = AtomicBool::new(false);
    let mut complete_budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    let mut complete = Vec::new();
    super::scan(&rom, &mut complete, &mut complete_budget, 8).unwrap();
    let used = 1_000_000 - complete_budget.remaining;
    let mut exhausted_budget = Budget {
        cancel: &cancel,
        remaining: used - 1,
    };
    let mut partial = Vec::new();
    assert_eq!(
        super::scan(&rom, &mut partial, &mut exhausted_budget, 8),
        Err(ScanStop::WorkLimit)
    );
    assert!(partial.is_empty());
}
