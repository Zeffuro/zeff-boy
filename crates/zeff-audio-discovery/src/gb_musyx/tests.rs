use super::*;

pub(super) static PROFILE: profiles::Profile = profiles::Profile {
    name: "gb-musyx-synthetic-v1",
    start: 0x4000,
    len: 0x100,
    bank_literal: None,
    prefix: [0xaf, 0xea, 0x20, 0xdf, 0xea, 0x21, 0xdf, 0x3e],
    hash: "0b20462ea6d0a88f9fbdc6c8b20a99ce511b935c5b9cb3f6ef5f6e7ce4b49144",
    bank0: 0x3000,
    bank0_len: 16,
    bank0_hashes: &["290fc84595c38cccf2538c970acf3d9535809cee638723857a45f4a221011e77"],
    init: 0x4000,
    handle: 0x4040,
    start_song: 0x4080,
    sample: 0x3000,
    current_bank: 0xfffe,
};

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x10000];
    bytes[0x100..0x103].copy_from_slice(&[0xc3, 0x00, 0x01]);
    bytes[0x143] = 0x80;
    bytes[0x147] = 0x19;
    bytes[0x148] = 1;
    bytes[0x3000] = 0xc9;
    bytes[0x8000..0x8011].copy_from_slice(&[
        175, 234, 32, 223, 234, 33, 223, 62, 128, 224, 38, 62, 119, 224, 36, 201, 0,
    ]);
    bytes[0x8040..0x804a].copy_from_slice(&[250, 32, 223, 60, 234, 32, 223, 224, 19, 201]);
    bytes[0x8080..0x8098].copy_from_slice(&[
        234, 33, 223, 62, 17, 224, 37, 62, 128, 224, 17, 62, 240, 224, 18, 62, 32, 224, 19, 62,
        135, 224, 20, 201,
    ]);
    put_le(&mut bytes, 0x8100, 0x70);
    put_le(&mut bytes, 0x8102, 0x70);
    put_le(&mut bytes, 0x8105, 0x70);
    bytes[0x8107] = 1;
    put_le(&mut bytes, 0x810c, 0x80);
    bytes[0x810e] = 2;
    for (index, relative) in [8, 0x21, 0x41, 0x51].into_iter().enumerate() {
        put_le(&mut bytes, 0x810f + index * 2, relative);
    }
    bytes[0x8117..0x811f].copy_from_slice(&[0x0c, 1, 4, 0, 0, 0, 12, 0]);
    bytes[0x8130..0x813a].copy_from_slice(&[0x0e, 2, 0x21, 0, 4, 0, 0, 0, 12, 0]);
    put_le(&mut bytes, 0x8170, 0x4600);
    put_le(&mut bytes, 0x8172, 1);
    bytes[0x8174] = 1;
    bytes[0x8180..0x8186].copy_from_slice(&[1, 0, 0x40, 1, 0, 0x40]);
    bytes[0xc005] = 1;
    put_be(&mut bytes, 0xc084, 6);
    put_be(&mut bytes, 0xc086, 34);
    put_be(&mut bytes, 0xc088, 120);
    put_be(&mut bytes, 0xc08a, 14);
    bytes[0xc092..0xc09c].copy_from_slice(&[0, 0, 0, 0, 12, 0xfe, 0, 0, 0, 0]);
    put_be(&mut bytes, 0xc0a6, 36);
    bytes[0xc0a8..0xc0af].copy_from_slice(&[0x10, 0, 60, 12, 0xf0, 0, 255]);
    bytes
}

fn put_le(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put_be(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

#[cfg(test)]
fn inventory(bytes: &[u8]) -> Vec<GbMusyxSong> {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    let mut songs = Vec::new();
    scan(bytes, &mut songs, &mut budget, 100).unwrap();
    songs
}

#[test]
fn recognized_driver_binds_selectors_and_revalidates_inventory() {
    let bytes = synthetic_rom();
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 2);
    assert_eq!(
        songs[0].tracks,
        vec![GbMusyxTrack {
            number: 1,
            note_count: 1
        }]
    );
    assert_eq!(songs[0].bank, 3);
    assert_eq!(songs[0].header.effective_offset, 0xc000);
    let cancel = AtomicBool::new(false);
    validate_song(&bytes, &songs[0], &cancel).unwrap();
    let mut changed = songs[0].clone();
    changed.tracks[0].note_count += 1;
    assert!(validate_song(&bytes, &changed, &cancel).is_err());
    changed = songs[0].clone();
    changed.profile = "unrecognized";
    assert!(validate_song(&bytes, &changed, &cancel).is_err());
    assert!(validate_song(&bytes, &songs[0], &AtomicBool::new(true)).is_err());
}

#[test]
fn invalid_earlier_project_does_not_hide_a_later_qualified_bank() {
    let mut bytes = synthetic_rom();
    bytes.copy_within(0x8000..0xc000, 0x4000);
    bytes[0x410f] = 1;
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 2);
    validate_song(&bytes, &songs[0], &AtomicBool::new(false)).unwrap();
}

#[test]
fn code_witnesses_are_exact_but_unrelated_bytes_and_bank_relocation_are_portable() {
    let bytes = synthetic_rom();
    for at in [0x8000, 0x80ff, 0x3001] {
        let mut changed = bytes.clone();
        changed[at] ^= 1;
        assert!(inventory(&changed).is_empty());
    }
    let mut changed = bytes.clone();
    changed[0x3f00] = 123;
    assert_eq!(inventory(&changed), inventory(&bytes));
    changed.copy_within(0x8000..0xc000, 0x4000);
    changed.copy_within(0xc000..0x10000, 0x8000);
    let relocated = inventory(&changed);
    assert_eq!(relocated.len(), 2);
    assert_eq!(relocated[0].bank, 2);
    assert_eq!(relocated[0].table_entry.effective_offset, 0x4180);
}

#[test]
fn wrong_hardware_truncation_and_invalid_pointers_are_rejected() {
    let bytes = synthetic_rom();
    for (at, value) in [
        (0x143, 0),
        (0x147, 1),
        (0x148, 8),
        (0x810f, 1),
        (0x8171, 0x80),
        (0x8170, 1),
        (0xc08b, 1),
        (0xc0a7, 1),
    ] {
        let mut changed = bytes.clone();
        changed[at] = value;
        assert!(inventory(&changed).is_empty(), "offset {at:x}");
    }
    let mut changed = bytes.clone();
    changed[0x8180] = 255;
    let songs = inventory(&changed);
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].index, 1);
    assert!(inventory(&bytes[..0xffff]).is_empty());
}

#[test]
fn macro_control_flow_cannot_escape_enter_operands_or_run_without_yield() {
    for code in [
        &[6, 0, 9][..],
        &[6, 0, 8],
        &[6, 0xff, 0xff],
        &[0x0a, 3, 0],
        &[0x1a, 8, 0],
        &[0x12, 3, 0],
        &[0x0f, 0, 0],
        &[0x1c],
    ] {
        let mut bytes = synthetic_rom();
        bytes[0x8117..0x8117 + code.len()].copy_from_slice(code);
        assert!(inventory(&bytes).is_empty(), "macro {code:?}");
    }
    let mut bytes = synthetic_rom();
    bytes[0x8117..0x811f].copy_from_slice(&[4, 0, 0, 0, 0, 6, 0, 8]);
    assert_eq!(inventory(&bytes).len(), 2);
}

#[test]
fn loop_program_state_closes_over_samples_without_double_counting_notes() {
    let mut bytes = synthetic_rom();
    bytes[0xc0a8..0xc0b2].copy_from_slice(&[0x10, 0, 60, 12, 0, 0, 0x81, 0xf0, 0, 255]);
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 2);
    assert_eq!(songs[0].tracks[0].note_count, 1);
    assert!(
        songs[0]
            .mapped_spans
            .iter()
            .any(|span| span.effective_offset == 0x8600 && span.byte_len == 16)
    );
    bytes[0x8133] = 1;
    assert!(inventory(&bytes).is_empty());
}

#[test]
fn control_only_selector_is_not_counted_as_a_song() {
    let mut bytes = synthetic_rom();
    bytes[0xc0a8..0xc0ae].copy_from_slice(&[0, 0, 0x80, 0xf0, 0, 255]);
    assert!(inventory(&bytes).is_empty());
}

#[test]
fn last_note_consumes_both_pitch_operands() {
    let mut bytes = synthetic_rom();
    bytes[0x8117..0x811b].copy_from_slice(&[0x14, 0, 0xff, 0]);
    assert_eq!(inventory(&bytes).len(), 2);
    bytes[0x811a..0x811d].copy_from_slice(&[6, 0, 10]);
    assert!(inventory(&bytes).is_empty());
}

#[test]
fn default_macro_is_direct_while_program_changes_use_the_lookup_table() {
    let mut bytes = synthetic_rom();
    bytes[0xc000] = 1;
    bytes[0xc005] = 0;
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 2);
    assert!(
        songs[0]
            .mapped_spans
            .iter()
            .any(|span| span.effective_offset == 0x8600)
    );
    bytes[0xc0a8..0xc0b2].copy_from_slice(&[0, 0, 0x81, 0x10, 0, 60, 12, 0xf0, 0, 255]);
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 2);
    assert!(
        !songs[0]
            .mapped_spans
            .iter()
            .any(|span| span.effective_offset == 0x8600)
    );
}

#[cfg(test)]
fn adsr_fixture(voice: u8, table: usize) -> Vec<u8> {
    let mut bytes = synthetic_rom();
    bytes[0xc000] = 2;
    bytes[0x8150..0x815a].copy_from_slice(&[0x0e, voice, 0x0f, 0, 4, 0, 0, 0, 12, 0]);
    bytes.copy_within(0x8170..0x8176, table + 16);
    bytes.copy_within(0x8180..0x8186, table + 32);
    put_le(&mut bytes, 0x8100, (table - 0x8100) as u16);
    put_le(&mut bytes, 0x8102, (table + 16 - 0x8100) as u16);
    put_le(&mut bytes, 0x8105, (table + 16 - 0x8100) as u16);
    put_le(&mut bytes, 0x810c, (table + 32 - 0x8100) as u16);
    put_le(&mut bytes, table, 2);
    bytes[table + 2..table + 9].copy_from_slice(&[1, 0, 255, 255, 10, 255, 254]);
    bytes
}

#[test]
fn keyoff_maps_only_the_derived_zero_and_composed_adsr_aliases() {
    let songs = inventory(&adsr_fixture(2, 0x8164));
    assert_eq!(songs.len(), 2);
    assert!(
        songs[0]
            .mapped_spans
            .iter()
            .any(|span| span.effective_offset == 1 && span.byte_len == 4)
    );
    let songs = inventory(&adsr_fixture(1, 0x8164));
    assert_eq!(songs.len(), 2);
    for offset in [0xa801, 0xaa01] {
        assert!(
            songs[0]
                .mapped_spans
                .iter()
                .any(|span| span.effective_offset == offset && span.byte_len == 4)
        );
    }
    assert!(inventory(&adsr_fixture(1, 0x81e0)).is_empty());
    assert!(inventory(&adsr_fixture(1, 0x81fe)).is_empty());
}

#[test]
fn candidate_work_and_cancellation_limits_remain_explicit() {
    let bytes = synthetic_rom();
    let cancel = AtomicBool::new(false);
    let mut songs = Vec::new();
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget, 1),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(songs.len(), 1);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1,
    };
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut budget, 100),
        Err(ScanStop::WorkLimit)
    );
    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut budget, 100),
        Err(ScanStop::Cancelled)
    );
}

#[test]
fn native_rom_preserves_source_and_hands_off_before_song_selection() {
    let bytes = synthetic_rom();
    let songs = inventory(&bytes);
    let prepared = prepare_rom(&bytes, &songs[1], &AtomicBool::new(false)).unwrap();
    assert_eq!(prepared.bytes.len(), bytes.len());
    for (at, (original, patched)) in bytes.iter().zip(&prepared.bytes).enumerate() {
        if !((0x40..0x43).contains(&at)
            || (0x50..0x53).contains(&at)
            || (0x100..0x103).contains(&at)
            || (0x150..0x280).contains(&at))
        {
            assert_eq!(original, patched, "preserved source at {at:x}");
        }
    }
    assert!(prepared.wait_start >= 0x150 && prepared.wait_end < 0x200);
    let start = usize::from(prepared.wait_end);
    assert_eq!(
        &prepared.bytes[start..start + 5],
        &[0x3e, 1, 0xcd, 0x80, 0x40]
    );
    assert_eq!(
        (prepared.ready_address, prepared.ready_value),
        (0xfffc, 0xa5)
    );
    assert_eq!((prepared.ack_address, prepared.ack_value), (0xfffb, 0x5a));
}
