use std::sync::atomic::AtomicBool;

use super::*;
const SELECTOR_TABLE_POINTER_OFFSET: usize = 40;

fn put_word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_half(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_bl(bytes: &mut [u8], offset: usize, target: usize) {
    let displacement = (target as i64 - offset as i64 - 4) as i32 as u32;
    put_half(
        bytes,
        offset,
        0xF000 | ((displacement >> 12) as u16 & 0x7FF),
    );
    put_half(
        bytes,
        offset + 2,
        0xF800 | ((displacement >> 1) as u16 & 0x7FF),
    );
}

fn put_selector(bytes: &mut [u8], offset: usize, player_table: usize, table: usize, target: usize) {
    bytes[offset..offset + SONG_SELECT_V2.len()].copy_from_slice(&SONG_SELECT_V2);
    put_bl(bytes, offset + 28, target);
    put_word(
        bytes,
        ((offset + 8) & !3) + 28,
        0x0800_0000 + player_table as u32,
    );
    put_word(bytes, ((offset + 10) & !3) + 32, 0x0800_0000 + table as u32);
}

fn put_entry(bytes: &mut [u8], offset: usize, header: u32, player: u16) {
    put_word(bytes, offset, header);
    put_half(bytes, offset + 4, player);
    put_half(bytes, offset + 6, player);
}

fn put_header(bytes: &mut [u8], offset: usize, track: usize) {
    bytes[offset] = 1;
    put_word(bytes, offset + 4, 0x0800_0600);
    put_word(bytes, offset + 8, 0x0800_0000 + track as u32);
    bytes[track..track + 7].copy_from_slice(&[0xBD, 0, 0xD0, 60, 100, 0x81, 0xB1]);
}

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x1000];
    put_word(&mut bytes, 0x100, 0x0097_F800);
    put_word(&mut bytes, 0x104, 2);
    put_word(&mut bytes, 0x108, 0x0800_02E8);
    bytes[0x110..0x112].copy_from_slice(&[0x00, 0xB5]);
    bytes[0x11C..0x11C + SONG_SELECT_V1.len()].copy_from_slice(&SONG_SELECT_V1);
    put_bl(&mut bytes, 0x11C + 28, 0x800);
    put_word(&mut bytes, 0x140, 0x0800_02E8);
    put_word(
        &mut bytes,
        0x11C + SELECTOR_TABLE_POINTER_OFFSET,
        0x0800_0300,
    );

    put_header(&mut bytes, 0x400, 0x500);
    put_header(&mut bytes, 0x480, 0x520);
    put_entry(&mut bytes, 0x300, 0x0800_0400, 1);
    put_entry(&mut bytes, 0x308, 0, 0);
    put_entry(&mut bytes, 0x310, 0x0800_0440, 0);
    put_entry(&mut bytes, 0x318, 0x0800_0480, 1);
    put_word(&mut bytes, 0x320, 0);
    put_word(&mut bytes, 0x324, 0x4000_0000);
    bytes[0x600] = 1;
    bytes
}

fn run(bytes: &[u8]) -> Result<Vec<SongTableInventory>, ScanStop> {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut inventories = Vec::new();
    discover(bytes, &mut inventories, &mut budget)?;
    Ok(inventories)
}

#[test]
fn selector_settings_literal_and_indexed_slots_are_preserved() {
    let inventories = run(&fixture()).unwrap();
    assert_eq!(inventories.len(), 1);
    let table = &inventories[0];
    assert_eq!(
        (table.selector.effective_offset, table.selector.byte_len),
        (0x11C, 44)
    );
    assert_eq!(
        (table.settings.effective_offset, table.settings.byte_len),
        (0x100, 12)
    );
    assert_eq!(
        (table.table.effective_offset, table.table.byte_len),
        (0x300, 32)
    );
    assert_eq!(
        table
            .entries
            .iter()
            .map(|entry| (entry.index, entry.kind))
            .collect::<Vec<_>>(),
        [
            (0, SongTableEntryKind::Song),
            (1, SongTableEntryKind::Null),
            (2, SongTableEntryKind::Placeholder),
            (3, SongTableEntryKind::Song),
        ]
    );
    assert_eq!(
        table.boundary,
        SongTableBoundary::NullTerminator {
            entry: RomSpan::new(0x320, 8)
        }
    );
}

#[test]
fn halfword_aligned_selector_reads_the_actual_pc_relative_table_literal() {
    let mut bytes = fixture();
    bytes[0x11C..0x148].fill(0);
    bytes[0x11E..0x11E + SONG_SELECT_V1.len()].copy_from_slice(&SONG_SELECT_V1);
    put_bl(&mut bytes, 0x11E + 28, 0x800);
    put_word(&mut bytes, 0x140, 0x0800_02E8);
    put_word(&mut bytes, 0x148, 0x0800_0300);
    assert_eq!(word(&bytes, 0x11E + 40), Some(0x0300_0000));
    let inventories = run(&bytes).unwrap();
    assert_eq!(inventories.len(), 1);
    assert_eq!(inventories[0].selector, RomSpan::new(0x11E, 46));
    assert_eq!(inventories[0].table.effective_offset, 0x300);
    put_word(&mut bytes, 0x148, 0x0800_0380);
    assert!(run(&bytes).unwrap().is_empty());
}

#[test]
fn both_selector_versions_require_matching_literal_and_settings() {
    let mut v2 = fixture();
    v2[0x11C..0x11C + SONG_SELECT_V2.len()].copy_from_slice(&SONG_SELECT_V2);
    assert_eq!(run(&v2).unwrap().len(), 1);

    let mut odd_selector = fixture();
    odd_selector[0x11C..0x14B].fill(0);
    odd_selector[0x11D..0x11D + SONG_SELECT_V1.len()].copy_from_slice(&SONG_SELECT_V1);
    put_word(
        &mut odd_selector,
        0x11D + SELECTOR_TABLE_POINTER_OFFSET,
        0x0800_0300,
    );
    assert!(run(&odd_selector).unwrap().is_empty());

    for (offset, value) in [
        (0x11C + SELECTOR_TABLE_POINTER_OFFSET, 0x0800_0380),
        (0x100, 0),
        (0x104, 256),
        (0x108, 0x0200_02E8),
    ] {
        let mut invalid = fixture();
        put_word(&mut invalid, offset, value);
        assert!(run(&invalid).unwrap().is_empty(), "field {offset:x}");
    }
}

#[test]
fn truncated_header_references_are_retained_as_unresolved() {
    let mut bytes = fixture();
    put_word(&mut bytes, 0x484, 0x0800_0FFC);
    let inventories = run(&bytes).unwrap();
    assert_eq!(
        inventories[0].entries[3].kind,
        SongTableEntryKind::Unresolved
    );

    put_word(&mut bytes, 0x318, 0x0900_0000);
    let inventories = run(&bytes).unwrap();
    assert_eq!(inventories[0].entries.len(), 3);
    assert_eq!(
        inventories[0].boundary,
        SongTableBoundary::InvalidHeaderPointer {
            entry: RomSpan::new(0x318, 8),
            address: 0x0900_0000,
        }
    );
}

#[test]
fn linked_header_indices_and_placeholder_aliases_remain_distinct() {
    let mut bytes = fixture();
    put_entry(&mut bytes, 0x320, 0x0800_0440, 0);
    put_entry(&mut bytes, 0x328, 0x0800_0400, 1);
    put_word(&mut bytes, 0x330, 0);
    put_word(&mut bytes, 0x334, 0x4000_0000);

    let inventories = run(&bytes).unwrap();
    let table = &inventories[0];
    assert_eq!(
        table
            .entries
            .iter()
            .filter(|entry| entry.header_address == 0x0800_0400)
            .map(|entry| entry.index)
            .collect::<Vec<_>>(),
        [0, 5]
    );
    assert_eq!(
        table
            .entries
            .iter()
            .filter(|entry| entry.kind == SongTableEntryKind::Placeholder)
            .map(|entry| (entry.index, entry.header_address))
            .collect::<Vec<_>>(),
        [(2, 0x0800_0440), (4, 0x0800_0440)]
    );
    assert_eq!(table.entries[1].kind, SongTableEntryKind::Null);
    for max_candidates in [1, 1024] {
        let report = crate::scan(
            zeff_emu_common::system::System::Gba,
            &bytes,
            crate::ScanLimits {
                max_candidates,
                ..Default::default()
            },
            &AtomicBool::new(false),
        );
        assert_eq!(
            report.candidates.len(),
            if max_candidates == 1 { 1 } else { 2 }
        );
        assert_eq!(
            report.candidates[0]
                .table_entries
                .iter()
                .map(|entry| entry.index)
                .collect::<Vec<_>>(),
            [0, 5]
        );
        assert!(report.candidates[0].evidence.song_table_verified);
        assert!(report.candidates[0].evidence.engine_signature_verified);
        if max_candidates == 1 {
            assert_eq!(
                report.status,
                crate::ScanStatus::Incomplete(ScanStop::CandidateLimit)
            );
        } else {
            assert_eq!(report.status, crate::ScanStatus::Complete);
            assert_eq!(report.candidates[1].table_entries[0].index, 3);
        }
    }
}

#[test]
fn per_table_slot_limit_is_reported_before_unbounded_enumeration() {
    let mut bytes = fixture();
    bytes.resize(0xA000, 0);
    put_header(&mut bytes, 0x8800, 0x8A00);
    put_word(&mut bytes, 0x8804, 0x0800_8900);
    bytes[0x8900] = 1;
    for index in 0..MAX_ENTRY_SLOTS_PER_TABLE {
        put_entry(&mut bytes, 0x300 + index * 8, 0x0800_8800, 0);
    }
    assert_eq!(run(&bytes), Err(ScanStop::InventoryLimit));
}

#[test]
fn work_limit_and_cancellation_are_not_partial_success() {
    let bytes = fixture();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1,
    };
    assert_eq!(
        discover(&bytes, &mut Vec::new(), &mut budget),
        Err(ScanStop::WorkLimit)
    );

    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    assert_eq!(
        discover(&bytes, &mut Vec::new(), &mut budget),
        Err(ScanStop::Cancelled)
    );
}

fn song_id_fixture(pool: usize, selector: usize) -> Vec<u8> {
    let mut bytes = vec![0; 0x6000];
    for (field, value) in [
        (0x00, 0x0300_0100),
        (0x04, 0x0400_0200),
        (0x08, 0x0400_0084),
        (0x0C, 0x0400_0082),
        (0x10, 0xA90E),
        (0x14, 0x0400_0089),
        (0x18, 0x0400_0063),
        (0x1C, 0x0400_0080),
        (0x58, 2),
        (0x5C, 0x0193_F600),
        (0x68, 0x0400_00D4),
        (0x6C, 0x0800_0900),
    ] {
        put_word(&mut bytes, pool + field, value);
    }
    for (index, field) in [0x20, 0x30, 0x40].into_iter().enumerate() {
        put_word(&mut bytes, pool + field, 0x0300_0200 + index as u32 * 4);
        put_word(
            &mut bytes,
            pool + field + 4,
            0x0300_1001 + index as u32 * 0x100,
        );
        put_word(
            &mut bytes,
            pool + field + 8,
            0x0800_4001 + index as u32 * 0x100,
        );
        put_word(&mut bytes, pool + field + 12, 0x8000_0020);
    }
    for index in 0..2 {
        let player = 0x900 + index * 12;
        put_word(&mut bytes, player, 0x0300_2000 + index as u32 * 0x100);
        put_word(&mut bytes, player + 4, 0x0300_3000 + index as u32 * 0x100);
        put_half(&mut bytes, player + 8, 2);
        put_half(&mut bytes, player + 10, 1);
    }
    put_selector(&mut bytes, selector, 0x900, 0x918, 0x5100);
    put_header(&mut bytes, 0x1200, 0x1800);
    bytes[0x1201] = 7;
    put_header(&mut bytes, 0x1400, 0x1900);
    bytes[0x1401] = 9;
    bytes[0x600] = 1;
    put_entry(&mut bytes, 0x918, 0x0800_1200, 0);
    put_entry(&mut bytes, 0x920, 0x0800_1600, 1);
    put_entry(&mut bytes, 0x928, 0x0800_1400, 1);
    // A matching-shaped header outside the verified table must stay excluded.
    put_header(&mut bytes, 0x1A00, 0x1B00);
    bytes[0x1A01] = 12;
    bytes
}

#[test]
fn relocated_song_id_pool_and_selector_authorize_only_linked_metadata_headers() {
    for (pool, selector) in [(0x100, 0x3000), (0x200, 0x3202)] {
        let bytes = song_id_fixture(pool, selector);
        let tables = run(&bytes).unwrap();
        assert_eq!(tables.len(), 1);
        let table = &tables[0];
        assert_eq!(table.dialect, SongDialect::SongIdHeader);
        assert_eq!(table.settings, RomSpan::new(pool, SONG_ID_SETTINGS_LEN));
        assert_eq!(
            table.settings_fields,
            SettingsFields {
                sound_mode: RomSpan::new(pool + 0x5C, 4),
                player_count: RomSpan::new(pool + 0x58, 4),
                player_table_pointer: RomSpan::new(pool + 0x6C, 4),
            }
        );
        assert_eq!(
            table
                .entries
                .iter()
                .map(|entry| entry.kind)
                .collect::<Vec<_>>(),
            [
                SongTableEntryKind::Song,
                SongTableEntryKind::Placeholder,
                SongTableEntryKind::Song
            ]
        );
        let scan = crate::scan(
            zeff_emu_common::system::System::Gba,
            &bytes,
            Default::default(),
            &AtomicBool::new(false),
        );
        assert_eq!(scan.candidates.len(), 2);
        assert!(
            scan.candidates
                .iter()
                .all(|candidate| candidate.evidence.song_table_verified)
        );
        assert_eq!(
            scan.candidates
                .iter()
                .map(|candidate| candidate.header.effective_offset)
                .collect::<Vec<_>>(),
            [0x1200, 0x1400]
        );
    }
}

#[test]
fn song_id_evidence_rejects_malformed_registers_copies_players_and_literals() {
    for (offset, value) in [
        (0x104, 0x0400_0204),
        (0x168, 0x0400_00D8),
        (0x100, 0x0800_0100),
        (0x124, 0x0200_1001),
        (0x128, 0x0800_5FF1),
        (0x12C, 0x8400_0020),
        (0x158, 0),
        (0x158, u32::MAX),
        (0x15C, 0x0493_F600),
        (0x16C, 0x0800_0920),
        (0x900, 0x0400_2000),
        (0x908, 25),
        (0x3024, 0x0800_0910),
        (0x3028, 0x0800_0930),
    ] {
        let mut bytes = song_id_fixture(0x100, 0x3000);
        put_word(&mut bytes, offset, value);
        assert!(
            run(&bytes).unwrap().is_empty(),
            "accepted malformed field {offset:x}"
        );
        let scan = crate::scan(
            zeff_emu_common::system::System::Gba,
            &bytes,
            Default::default(),
            &AtomicBool::new(false),
        );
        assert!(
            scan.candidates.is_empty(),
            "unverified metadata headers at {offset:x}"
        );
    }
}

#[test]
fn selector_branch_requires_both_thumb_halves_and_a_bounded_target() {
    let mut bytes = song_id_fixture(0x100, 0x3000);
    for target in [0x1000, 0x5100] {
        put_bl(&mut bytes, 0x301C, target);
        assert_eq!(thumb_bl_target(&bytes, 0x301C), Some(target));
        assert_eq!(run(&bytes).unwrap().len(), 1);
    }
    for (offset, value) in [(0x301C, 0xE000), (0x301E, 0xE800)] {
        let mut invalid = bytes.clone();
        put_half(&mut invalid, offset, value);
        assert!(run(&invalid).unwrap().is_empty());
    }
    put_bl(&mut bytes, 0x301C, 0x6000);
    assert!(run(&bytes).unwrap().is_empty());
    let mut stock = fixture();
    stock[0x401] = 3;
    let tables = run(&stock).unwrap();
    assert_eq!(tables[0].dialect, SongDialect::Mp2k);
    assert_eq!(tables[0].entries[0].kind, SongTableEntryKind::Unresolved);
}
