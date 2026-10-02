use std::sync::atomic::AtomicBool;

use zeff_emu_common::system::System;

use super::*;

fn rom() -> Vec<u8> {
    let mut bytes = vec![0xc9; 0x8000];
    bytes[0x104..0x150].fill(0);
    code(&mut bytes, 0x100, &[0xc3, 0x50, 0x01]);
    checksum(&mut bytes);
    bytes
}

fn checksum(bytes: &mut [u8]) {
    bytes[0x14d] = bytes[0x134..0x14d]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_sub(*byte).wrapping_sub(1));
}

fn code(bytes: &mut [u8], address: usize, instructions: &[u8]) {
    bytes[address..address + instructions.len()].copy_from_slice(instructions);
}

fn inspect(
    bytes: &[u8],
    work: u64,
    capacity: usize,
    cancelled: bool,
) -> (Vec<DriverCandidate>, Result<(), ScanStop>) {
    let cancel = AtomicBool::new(cancelled);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: work,
    };
    let mut findings = Vec::new();
    let result = scan(bytes, &mut findings, &mut budget, capacity);
    (findings, result)
}

fn inventory(bytes: &[u8]) -> CodeInventory {
    let (findings, result) = inspect(bytes, 1_000_000, 8, false);
    result.unwrap();
    assert_eq!(findings.len(), 1);
    findings.into_iter().next().unwrap().code.unwrap()
}

#[test]
fn reverse_nested_call_paths_retain_every_writer_and_source_hash() {
    let mut bytes = rom();
    code(&mut bytes, 0x150, &[0xcd, 0, 2, 0xc9]);
    code(&mut bytes, 0x200, &[0xcd, 0, 3, 0xc9]);
    code(
        &mut bytes,
        0x300,
        &[0xe0, 0x12, 0xea, 0x30, 0xff, 0xe0, 0x12, 0xc9],
    );
    let (findings, result) = inspect(&bytes, 1_000_000, 8, false);
    result.unwrap();
    let candidate = &findings[0];
    assert_eq!(candidate.qualification, CandidateQualification::StaticCode);
    assert!(candidate.inventory.is_none());
    let code = candidate.code.as_ref().unwrap();
    assert_eq!(code.writes.len(), 3);
    assert_eq!(code.calls.len(), 6);
    assert_eq!(code.calls[0].target_cpu_address, 0x200);
    assert_eq!(code.calls[0].writer_cpu_address, 0x300);
    assert_eq!(code.calls[3].cpu_address, 0x200);
    for evidence in &candidate.evidence {
        let start = evidence.span.offset as usize;
        let end = start + evidence.span.byte_len as usize;
        assert_eq!(
            evidence.sha256,
            zeff_firmware::sha256_hex(&bytes[start..end])
        );
    }
    let json = serde_json::to_value(candidate).unwrap();
    assert!(json.get("id").is_none());
    assert!(json.get("capabilities").is_none());
}

#[test]
fn branches_returns_and_tail_jumps_preserve_possible_paths() {
    let mut bytes = rom();
    code(&mut bytes, 0x150, &[0xc4, 0, 2, 0xc9]);
    code(
        &mut bytes,
        0x200,
        &[0xc0, 0x20, 4, 0xe0, 0x10, 0x18, 0xf9, 0xc3, 0, 3],
    );
    code(&mut bytes, 0x300, &[0xea, 0x3f, 0xff, 0xd9]);
    let found = inventory(&bytes);
    assert_eq!(found.writes.len(), 2);
    assert_eq!(found.calls.len(), 2);
    assert_eq!(found.calls[1].writer_cpu_address, 0x300);
}

#[test]
fn rst_and_interrupt_roots_are_decoded_with_instruction_spans() {
    let mut bytes = rom();
    code(&mut bytes, 0x40, &[0xc3, 0, 2]);
    code(&mut bytes, 0x150, &[0xff, 0xc9]);
    code(&mut bytes, 0x38, &[0xe0, 0x26, 0xc9]);
    code(&mut bytes, 0x200, &[0xe0, 0x30, 0xd9]);
    let found = inventory(&bytes);
    assert_eq!(found.writes.len(), 2);
    assert_eq!(found.calls.len(), 1);
    assert_eq!(found.calls[0].target_cpu_address, 0x38);
    assert_eq!(found.calls[0].span.byte_len, 1);
}

#[test]
fn immediate_operands_reads_indirect_stores_and_unreachable_bytes_are_not_writers() {
    let mut bytes = rom();
    code(
        &mut bytes,
        0x150,
        &[
            0x01, 0xe0, 0x12, 0x3e, 0xea, 0xcb, 0xe0, 0xf0, 0x12, 0xfa, 0x12, 0xff, 0xe2, 0x77,
            0x08, 0x12, 0xff, 0xe0, 0x27, 0xe0, 0x2f, 0xe9,
        ],
    );
    code(&mut bytes, 0x200, &[0xe0, 0x12, 0xc9]);
    code(&mut bytes, 0x166, &[0xe0, 0x12, 0xc9]);
    assert!(inspect(&bytes, 1_000_000, 8, false).0.is_empty());
}

#[test]
fn unconditional_return_does_not_reach_a_later_writer() {
    let mut bytes = rom();
    code(&mut bytes, 0x150, &[0xcd, 0, 2, 0xe0, 0x10, 0xc9]);
    code(&mut bytes, 0x200, &[0xc9, 0xe0, 0x11]);
    let found = inventory(&bytes);
    assert_eq!(found.writes.len(), 1);
    assert!(found.calls.is_empty());
}

#[test]
fn recursive_call_cycle_without_writer_does_not_create_a_link() {
    let mut bytes = rom();
    code(&mut bytes, 0x150, &[0xcd, 0, 2, 0xe0, 0x10, 0xc9]);
    code(&mut bytes, 0x200, &[0xcd, 0, 2, 0xc9]);
    assert!(inventory(&bytes).calls.is_empty());
    code(&mut bytes, 0x203, &[0xe0, 0x12, 0xc9]);
    assert_eq!(inventory(&bytes).calls.len(), 2);
}

#[test]
fn invalid_conflicting_truncated_or_header_code_suppresses_entire_candidate() {
    for (address, instructions) in [
        (0x150, vec![0xe0, 0x12, 0xd3]),
        (0x150, vec![0xe0, 0x12, 0x20, 0xfd, 0xc9]),
        (0x150, vec![0xe0, 0x12, 0xc3, 0xff, 0x7f]),
        (0x150, vec![0xe0, 0x12, 0xc3, 4, 1]),
        (0x150, vec![0xe0, 0x12, 0x10, 1]),
    ] {
        let mut bytes = rom();
        code(&mut bytes, address, &instructions);
        bytes[0x7fff] = 0xea;
        let (findings, result) = inspect(&bytes, 1_000_000, 8, false);
        assert_eq!(result, Ok(()));
        assert!(findings.is_empty());
    }
}

#[test]
fn opcode_widths_cover_all_256_base_opcodes() {
    let expected = [
        "1311112131111121",
        "2311112121111121",
        "2311112121111121",
        "2311112121111121",
        "1111111111111111",
        "1111111111111111",
        "1111111111111111",
        "1111111111111111",
        "1111111111111111",
        "1111111111111111",
        "1111111111111111",
        "1111111111111111",
        "1133312111323321",
        "1130312111303021",
        "2110012121300021",
        "2111012121310021",
    ];
    for opcode in 0..=255u16 {
        let digit = expected[usize::from(opcode >> 4)].as_bytes()[usize::from(opcode & 15)] - b'0';
        assert_eq!(
            decoder::opcode_length(opcode as u8),
            (digit != 0).then_some(digit),
            "{opcode:02x}"
        );
    }
}

#[test]
fn mbc0_upper_window_is_immutable_but_mapper_upper_windows_stay_unmapped() {
    let mut bytes = rom();
    code(&mut bytes, 0x150, &[0xcd, 0, 0x40, 0xc9]);
    code(&mut bytes, 0x4000, &[0xe0, 0x12, 0xc9]);
    assert_eq!(inventory(&bytes).writes[0].span.offset, 0x4000);
    for mapper in [0x01, 0x05, 0x11, 0x19] {
        bytes[0x147] = mapper;
        checksum(&mut bytes);
        assert!(inspect(&bytes, 1_000_000, 8, false).0.is_empty());
        code(&mut bytes, 0x150, &[0xe0, 0x12, 0xc9]);
        let (findings, result) = inspect(&bytes, 1_000_000, 8, false);
        result.unwrap();
        assert_eq!(findings[0].variant, "required-bank-zero-vector-code");
        assert_eq!(
            findings[0].evidence[0].signature,
            "required-lower-window-bank-zero"
        );
        code(&mut bytes, 0x150, &[0xcd, 0, 0x40, 0xc9]);
    }
}

#[test]
fn malformed_headers_sizes_and_unsupported_mappers_are_rejected() {
    let mut good = rom();
    code(&mut good, 0x150, &[0xe0, 0x12, 0xc9]);
    for (offset, value) in [
        (0x147, 0xfc),
        (0x148, 1),
        (0x149, 6),
        (0x149, 3),
        (0x143, 1),
    ] {
        let mut bytes = good.clone();
        bytes[offset] = value;
        checksum(&mut bytes);
        assert!(inspect(&bytes, 1_000_000, 8, false).0.is_empty());
    }
    good[0x14d] ^= 1;
    assert!(inspect(&good, 1_000_000, 8, false).0.is_empty());
    good.truncate(0x150);
    assert!(inspect(&good, 1_000_000, 8, false).0.is_empty());
}

#[test]
fn work_cancellation_and_candidate_stops_leave_no_partial_inventory() {
    let mut bytes = rom();
    code(&mut bytes, 0x150, &[0xe0, 0x12, 0xc9]);
    for (work, capacity, cancelled, stop) in [
        (0, 8, false, ScanStop::WorkLimit),
        (10, 8, false, ScanStop::WorkLimit),
        (1_000_000, 0, false, ScanStop::CandidateLimit),
        (1_000_000, 8, true, ScanStop::Cancelled),
    ] {
        let (findings, result) = inspect(&bytes, work, capacity, cancelled);
        assert_eq!(result, Err(stop));
        assert!(findings.is_empty());
    }
}

#[test]
fn graph_writer_call_site_and_reverse_link_caps_report_explicit_stops() {
    let cases = [
        (vec![0; MAX_NODES + 1], ScanStop::ValidationLimit),
        (
            [[0xe0, 0x12].repeat(MAX_WRITERS + 1), vec![0xc9]].concat(),
            ScanStop::InventoryLimit,
        ),
        (
            [[0xcd, 0, 0x30].repeat(MAX_CALL_SITES + 1), vec![0xc9]].concat(),
            ScanStop::InventoryLimit,
        ),
    ];
    for (instructions, stop) in cases {
        let mut bytes = rom();
        code(&mut bytes, 0x150, &instructions);
        assert_eq!(
            inspect(&bytes, 1_000_000, 8, false),
            (Vec::new(), Err(stop))
        );
    }
    let mut bytes = rom();
    let instructions = [[0xcd, 0, 0x30].repeat(65), vec![0xc9]].concat();
    code(&mut bytes, 0x150, &instructions);
    code(
        &mut bytes,
        0x3000,
        &[0xe0, 0x10, 0xe0, 0x11, 0xe0, 0x12, 0xe0, 0x13, 0xc9],
    );
    assert_eq!(
        inspect(&bytes, 1_000_000, 8, false),
        (Vec::new(), Err(ScanStop::InventoryLimit))
    );
}

#[test]
fn standalone_evidence_does_not_admit_catalog_playback_or_export() {
    let mut bytes = rom();
    code(&mut bytes, 0x150, &[0xe0, 0x12, 0xc9]);
    let cancel = AtomicBool::new(false);
    let evidence = crate::drivers::scan(System::Gb, &bytes, crate::ScanLimits::default(), &cancel);
    assert_eq!(evidence.status, crate::ScanStatus::Complete);
    assert_eq!(evidence.driver_candidates.len(), 1);
    assert_eq!(evidence.detector_version, 3);
    let report = crate::scan(System::Gb, &bytes, crate::ScanLimits::default(), &cancel);
    assert_eq!(report.song_count(), 0);
    assert!(report.driver_candidates.is_empty());
    #[cfg(not(target_arch = "wasm32"))]
    assert_eq!(report.catalog().count(), 0);
}
