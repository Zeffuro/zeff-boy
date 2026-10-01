use std::sync::atomic::AtomicBool;

use super::super::{ReadError, legacy_tests, sequence::Reader};
use super::*;

fn relocated() -> (Vec<u8>, FixedProfile) {
    let source = legacy_tests::synthetic_legacy_rom();
    let mut bytes = source.clone();
    bytes[0x44000..0x44800].fill(0);
    bytes[0x45800..0x46000].copy_from_slice(&source[0x44000..0x44800]);
    let mut profile = legacy_tests::PROFILE;
    profile.driver.segment = 0x4180;
    profile.driver.fixed += 0x1800;
    profile.driver.end += 0x1800;
    (bytes, profile)
}

#[test]
fn paragraph_mapping_preserves_logical_pointers_and_physical_spans() {
    let (bytes, profile) = relocated();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    assert_eq!(profile.driver.offset(0x4700), 0x45f00);
    assert_eq!(profile.driver.span().canonical_cpu_address, 0x45800);
    assert_eq!(track_count(&bytes, profile, 0), 4);
    let song = song(&bytes, profile, 0, 4, &mut budget).unwrap();
    assert_eq!(song.table_entry.effective_offset, 0x45e00);
    assert_eq!(song.table_entry.canonical_cpu_address, 0x45e00);
    assert_eq!(song.tracks.len(), 4);
    assert!(song.tracks.iter().all(|track| track.note_count > 0));
    assert!(song.mapped_spans.iter().all(|span| {
        span.effective_offset >= 0x45800
            && span.effective_offset + span.byte_len <= 0x46000
            && span.canonical_cpu_address == span.effective_offset
    }));
}

#[test]
fn paragraph_reads_stop_at_the_physical_bank_boundary() {
    let (mut bytes, profile) = relocated();
    bytes[0x4fffe..0x50002].copy_from_slice(&[0x34, 0x12, 0x78, 0x56]);
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut reader = Reader::legacy(&bytes, 4, profile.driver, &mut budget);
    assert_eq!(reader.word(0xe7fe).unwrap(), 0x1234);
    for at in [0xe7ff, 0xe800, 0xffff, usize::MAX] {
        assert!(matches!(reader.word(at), Err(ReadError::Invalid)));
    }
    assert_eq!(reader.mapped.len(), 2);
}

#[test]
fn paragraph_mapping_keeps_the_rom_bank_separate_from_the_cpu_window() {
    let (_, mut profile) = relocated();
    profile.driver.fixed = 0x1d7790;
    profile.driver.end = 0x1d8800;
    profile.driver.segment = 0xd080;
    assert_eq!(profile.driver.offset(0x6f90), 0x1d7790);
    assert_eq!(profile.driver.span().effective_offset, 0x1d7790);
    assert_eq!(profile.driver.span().canonical_cpu_address, 0xd7790);
}

#[test]
fn paragraph_quartets_preserve_permuted_slots_and_channels() {
    let (mut bytes, mut profile) = relocated();
    profile.driver.name = "ws-tose-fixed-paragraph-v2";
    let table = profile.driver.offset(profile.table);
    for (row, (slot, channel)) in [(5_u16, 2_u16), (6, 1), (4, 0), (7, 3)]
        .into_iter()
        .enumerate()
    {
        bytes[table + row * 6..table + row * 6 + 2].copy_from_slice(&(slot * 0x2a).to_le_bytes());
        bytes[table + row * 6 + 2..table + row * 6 + 4].copy_from_slice(&channel.to_le_bytes());
    }
    assert_eq!(track_count(&bytes, profile, 0), 4);
    let mut ordinary = profile;
    ordinary.driver.name = legacy_tests::PROFILE.driver.name;
    assert_eq!(track_count(&bytes, ordinary, 0), 1);

    for slot in [3_u16, 4, 8] {
        let mut changed = bytes.clone();
        changed[table + 18..table + 20].copy_from_slice(&(slot * 0x2a).to_le_bytes());
        assert_eq!(track_count(&changed, profile, 0), 1);
    }
    bytes[table + 20..table + 22].copy_from_slice(&0_u16.to_le_bytes());
    assert_eq!(track_count(&bytes, profile, 0), 1);
}
