#[cfg(test)]
use std::sync::atomic::AtomicBool;

#[cfg(test)]
use crate::{Budget, ScanStop};

#[cfg(test)]
use super::super::{CandidateQualification, EvidenceKind};

pub fn synthetic_rom() -> Vec<u8> {
    let mut rom = nrom(2);
    code(
        &mut rom,
        0x8000,
        &[
            0xa9, 1, 0x8d, 0, 0x40, 0x20, 0x10, 0x80, 0xd0, 3, 0x8c, 0x15, 0x40, 0x4c, 0x20, 0x80,
        ],
    );
    code(&mut rom, 0x8010, &[0x8e, 2, 0x40, 0x60]);
    code(&mut rom, 0x8020, &[0x8d, 3, 0x40, 0x4c, 0x20, 0x80]);
    vectors(&mut rom, 0x8000, 0x8000, 0x8000);
    rom
}

#[cfg(test)]
fn calls_rom(call_count: usize) -> Vec<u8> {
    let mut rom = nrom(2);
    let mut calls = Vec::with_capacity(call_count * 3 + 1);
    for _ in 0..call_count {
        calls.extend([0x20, 0, 0x82]);
    }
    calls.push(0x60);
    code(&mut rom, 0x8000, &calls);
    code(&mut rom, 0x8200, &[0x8d, 0, 0x40, 0x8d, 1, 0x40, 0x60]);
    vectors(&mut rom, 0x8000, 0x8000, 0x8000);
    rom
}

fn nrom(prg_banks: u8) -> Vec<u8> {
    let mut rom = vec![0; 16 + usize::from(prg_banks) * 0x4000];
    rom[..16].copy_from_slice(&[
        b'N', b'E', b'S', 0x1a, prg_banks, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
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
        let at = offset(rom, address);
        rom[at..at + 2].copy_from_slice(&target.to_le_bytes());
    }
}

#[cfg(test)]
fn scan(
    rom: &[u8],
    work: u64,
    capacity: usize,
    cancelled: bool,
) -> Result<Vec<super::DriverCandidate>, ScanStop> {
    let cancel = AtomicBool::new(cancelled);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: work,
    };
    let mut findings = Vec::new();
    super::scan(rom, &mut findings, &mut budget, capacity)?;
    Ok(findings)
}

#[test]
fn decoded_vector_flow_collects_direct_apu_writes_once_per_register() {
    let findings = scan(&synthetic_rom(), 100_000, 8, false).unwrap();
    assert_eq!(findings.len(), 1);
    let candidate = &findings[0];
    assert_eq!(candidate.family, "NES APU access");
    assert_eq!(candidate.variant, "nrom-vector-code");
    assert_eq!(candidate.qualification, CandidateQualification::StaticCode);
    assert_eq!(candidate.inventory, None);
    assert_eq!(candidate.fingerprint_revision, "nes-nrom-apu-writes/7");
    assert_eq!(
        candidate
            .evidence
            .iter()
            .filter(|evidence| evidence.kind == EvidenceKind::SoundRegisterWrite)
            .count(),
        4
    );
    assert!(
        candidate
            .evidence
            .iter()
            .any(|evidence| evidence.signature == "vector-reset")
    );
    let code = candidate.code.as_ref().expect("static code inventory");
    assert_eq!(
        code.writes
            .iter()
            .map(|write| (write.cpu_address, write.register))
            .collect::<Vec<_>>(),
        vec![
            (0x8002, 0x4000),
            (0x8010, 0x4002),
            (0x8020, 0x4003),
            (0x800a, 0x4015)
        ]
    );
    assert_eq!(code.calls.len(), 1);
    assert_eq!(code.calls[0].cpu_address, 0x8005);
    assert_eq!(code.calls[0].target_cpu_address, 0x8010);
    assert_eq!(code.calls[0].writer_cpu_address, 0x8010);
    assert_eq!(code.calls[0].span.byte_len, 3);
    assert_eq!(code.calls[0].writer_span.byte_len, 3);
    assert_eq!(
        candidate
            .evidence
            .iter()
            .filter(|evidence| evidence.kind == EvidenceKind::DirectCall)
            .map(|evidence| evidence.signature)
            .collect::<Vec<_>>(),
        vec!["decoded-apu-caller"]
    );
}

#[test]
fn calls_follow_local_paths_without_entering_nested_callees() {
    let mut rom = nrom(2);
    code(&mut rom, 0x8000, &[0x20, 0x10, 0x80, 0x8d, 0, 0x40, 0x60]);
    code(&mut rom, 0x8010, &[0x20, 0x20, 0x80, 0x60]);
    code(&mut rom, 0x8020, &[0x8d, 1, 0x40, 0x60]);
    vectors(&mut rom, 0x8000, 0x8000, 0x8000);

    let findings = scan(&rom, 100_000, 8, false).unwrap();
    let calls = &findings[0].code.as_ref().unwrap().calls;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].cpu_address, 0x8010);
    assert_eq!(calls[0].target_cpu_address, 0x8020);
    assert_eq!(calls[0].writer_cpu_address, 0x8020);
}

#[test]
fn calls_choose_lowest_reachable_retained_writer_and_stay_cpu_ordered() {
    let mut rom = nrom(2);
    code(
        &mut rom,
        0x8000,
        &[0x20, 0x30, 0x80, 0x20, 0x40, 0x80, 0x60],
    );
    code(&mut rom, 0x8012, &[0x8d, 0, 0x40, 0x60]);
    code(&mut rom, 0x8030, &[0xd0, 0xe0, 0x8d, 1, 0x40, 0x60]);
    code(&mut rom, 0x8040, &[0x8d, 2, 0x40, 0x60]);
    vectors(&mut rom, 0x8000, 0x8000, 0x8000);

    let findings = scan(&rom, 100_000, 8, false).unwrap();
    let calls = &findings[0].code.as_ref().unwrap().calls;
    assert_eq!(
        calls
            .iter()
            .map(|call| (
                call.cpu_address,
                call.target_cpu_address,
                call.writer_cpu_address
            ))
            .collect::<Vec<_>>(),
        vec![(0x8000, 0x8030, 0x8012), (0x8003, 0x8040, 0x8040)]
    );
}

#[test]
fn nrom_alias_call_sites_remain_distinct_cpu_locations() {
    let mut rom = nrom(1);
    code(&mut rom, 0x8000, &[0x20, 0x10, 0x80, 0x60]);
    code(&mut rom, 0x8010, &[0x8d, 0, 0x40, 0x8d, 1, 0x40, 0x60]);
    vectors(&mut rom, 0xc000, 0x8000, 0xc000);

    let findings = scan(&rom, 100_000, 8, false).unwrap();
    let calls = &findings[0].code.as_ref().unwrap().calls;
    assert_eq!(
        calls
            .iter()
            .map(|call| call.cpu_address)
            .collect::<Vec<_>>(),
        vec![0x8000, 0xc000]
    );
    assert!(calls.iter().all(|call| call.writer_cpu_address == 0x8010));
}

#[test]
fn call_inventory_and_search_limits_are_explicit() {
    let under_limit = calls_rom(64);
    let findings = scan(&under_limit, 100_000, 8, false).unwrap();
    assert_eq!(findings[0].code.as_ref().unwrap().calls.len(), 64);
    assert_eq!(
        findings[0]
            .evidence
            .iter()
            .filter(|evidence| evidence.kind == EvidenceKind::DirectCall)
            .count(),
        64
    );
    assert_eq!(findings[0].evidence.len(), 64 + 3 + 2);
    assert_eq!(
        scan(&calls_rom(65), 100_000, 8, false),
        Err(ScanStop::InventoryLimit)
    );
    assert_eq!(scan(&under_limit, 80, 8, false), Err(ScanStop::WorkLimit));

    let mut old_non_candidate = calls_rom(65);
    code(&mut old_non_candidate, 0x8203, &[0]);
    assert!(
        scan(&old_non_candidate, 100_000, 8, false)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn raw_bytes_indexed_writes_and_illegal_apu_addresses_do_not_become_evidence() {
    let mut rom = nrom(2);
    code(
        &mut rom,
        0x8000,
        &[
            0x8d, 0, 0x40, 0x8d, 1, 0x40, 0x8d, 0x14, 0x40, 0x8d, 0x16, 0x40, 0x8d, 0x18, 0x40,
            0x9d, 2, 0x40, 0x60,
        ],
    );
    code(&mut rom, 0x8100, &[0x8d, 4, 0x40, 0x8d, 5, 0x40]);
    vectors(&mut rom, 0x8000, 0x8000, 0x8000);
    let findings = scan(&rom, 100_000, 8, false).unwrap();
    assert_eq!(findings.len(), 1);
    let writes = findings[0]
        .evidence
        .iter()
        .filter(|evidence| evidence.kind == EvidenceKind::SoundRegisterWrite)
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 2);
    for evidence in writes {
        let start = evidence.span.offset as usize;
        assert_eq!(
            &rom[start..start + 3],
            if rom[start + 1] == 0 {
                &[0x8d, 0, 0x40]
            } else {
                &[0x8d, 1, 0x40]
            }
        );
    }
}

#[test]
fn vector_rooted_cycles_terminate_and_raw_data_stays_unqualified() {
    let mut cycle = nrom(2);
    code(&mut cycle, 0x8000, &[0x8d, 0, 0x40, 0x4c, 0, 0x80]);
    code(&mut cycle, 0x8010, &[0x8d, 1, 0x40, 0x4c, 0x10, 0x80]);
    vectors(&mut cycle, 0x8010, 0x8000, 0x8000);
    assert_eq!(scan(&cycle, 100_000, 8, false).unwrap().len(), 1);

    let mut data = nrom(2);
    code(&mut data, 0x8000, &[0x60]);
    code(&mut data, 0x8100, &[0x8d, 0, 0x40, 0x8d, 1, 0x40]);
    vectors(&mut data, 0x8000, 0x8000, 0x8000);
    assert!(scan(&data, 100_000, 8, false).unwrap().is_empty());
}

#[test]
fn overlapping_or_unsupported_paths_and_bad_nrom_mappings_are_rejected() {
    let mut overlap = synthetic_rom();
    code(&mut overlap, 0x8000, &[0x20, 1, 0x80]);
    assert!(scan(&overlap, 100_000, 8, false).unwrap().is_empty());
    let mut unsupported = synthetic_rom();
    code(&mut unsupported, 0x8000, &[0x02]);
    assert!(scan(&unsupported, 100_000, 8, false).unwrap().is_empty());
    for (index, value) in [(4, 3), (6, 4), (6, 8), (6, 0x10), (7, 8), (8, 1), (15, 1)] {
        let mut rom = synthetic_rom();
        rom[index] = value;
        assert!(scan(&rom, 100_000, 8, false).unwrap().is_empty());
    }
    let mut truncated = synthetic_rom();
    truncated.pop();
    assert!(scan(&truncated, 100_000, 8, false).unwrap().is_empty());
}

#[test]
fn nrom_16k_mirroring_and_global_stops_are_bounded() {
    let mut mirrored = nrom(1);
    code(&mut mirrored, 0xc000, &[0x8d, 0, 0x40, 0x8d, 1, 0x40, 0x60]);
    vectors(&mut mirrored, 0xc000, 0xc000, 0xc000);
    assert_eq!(scan(&mirrored, 100_000, 8, false).unwrap().len(), 1);

    assert_eq!(
        scan(&synthetic_rom(), 0, 8, false),
        Err(ScanStop::WorkLimit)
    );
    assert_eq!(
        scan(&synthetic_rom(), 100_000, 8, true),
        Err(ScanStop::Cancelled)
    );
    assert_eq!(
        scan(&synthetic_rom(), 100_000, 0, false),
        Err(ScanStop::CandidateLimit)
    );
}
