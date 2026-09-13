use super::*;
use crate::test_support::tracker::{it_fixture, s3m_fixture};
use std::sync::atomic::AtomicBool;

fn detected(bytes: &[u8]) -> Vec<EmbeddedModule> {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: super::super::MAX_SCAN_WORK,
    };
    let mut modules = Vec::new();
    scan(bytes, &mut modules, &mut budget, 10).unwrap();
    modules
}

#[test]
fn s3m_pattern_length_includes_its_length_word_at_the_exact_file_end() {
    let mut bytes = s3m_fixture();
    let payload = bytes[258..325].to_vec();
    bytes.resize(416 + 69, 0);
    bytes[99..101].copy_from_slice(&26u16.to_le_bytes());
    bytes[416..418].copy_from_slice(&69u16.to_le_bytes());
    bytes[418..].copy_from_slice(&payload);
    assert_eq!(detected(&bytes)[0].span.byte_len, 485);
    bytes.extend_from_slice(&[0xa5, 0x5a]);
    assert_eq!(detected(&bytes)[0].span.byte_len, 485);
    bytes[416..418].copy_from_slice(&1u16.to_le_bytes());
    assert!(detected(&bytes).is_empty());
}

#[test]
fn tracker_notes_must_refer_to_the_sample_on_the_played_channel_and_key() {
    let mut s3m = s3m_fixture();
    s3m[258..265].copy_from_slice(&[0x20, 0x40, 0, 0x21, 0xfe, 1, 0]);
    s3m[256..258].copy_from_slice(&72u16.to_le_bytes());
    assert!(detected(&s3m).is_empty());
    let mut it = it_fixture();
    it[512 + 65] = 1;
    it[512 + 64 + 48 * 2 + 1] = 0;
    assert!(detected(&it).is_empty());
}

#[test]
fn s3m_sparse_channels_keep_their_indices_and_pan_tables_have_a_checked_extent() {
    let mut bytes = s3m_fixture();
    bytes[95] = 15;
    assert_eq!(detected(&bytes)[0].channels, 32);
    bytes[53] = 0xfc;
    // The optional pan table ends at 133 and must not overlap the instrument at 128.
    assert!(detected(&bytes).is_empty());
}

#[test]
fn it_message_extent_is_retained_and_malformed_or_overlapping_ranges_are_rejected() {
    let mut bytes = it_fixture();
    bytes[46..48].copy_from_slice(&1u16.to_le_bytes());
    bytes[54..56].copy_from_slice(&4u16.to_le_bytes());
    bytes[56..60].copy_from_slice(&1316u32.to_le_bytes());
    bytes.extend_from_slice(b"abc\0");
    assert_eq!(detected(&bytes)[0].span.byte_len, 1320);
    assert!(detected(&bytes[..1319]).is_empty());
    bytes[56..60].copy_from_slice(&1300u32.to_le_bytes());
    assert!(detected(&bytes).is_empty());
    bytes[56..60].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(detected(&bytes).is_empty());
}

#[test]
fn it_history_and_embedded_midi_config_are_bounded_before_object_offsets() {
    let mut bytes = it_fixture();
    bytes[46..48].copy_from_slice(&2u16.to_le_bytes());
    bytes[205..207].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(detected(&bytes)[0].span.byte_len, 1316);
    bytes[205..207].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(detected(&bytes).is_empty());
    bytes[46..48].copy_from_slice(&8u16.to_le_bytes());
    assert!(detected(&bytes).is_empty());

    let source = it_fixture();
    let shift = 4896;
    let mut configured = source[..205].to_vec();
    configured.resize(205 + shift, 0);
    configured.extend_from_slice(&source[205..]);
    configured[46..48].copy_from_slice(&8u16.to_le_bytes());
    for at in [193, 197, 201] {
        let value = u32::from_le_bytes(source[at..at + 4].try_into().unwrap()) + shift as u32;
        configured[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    configured[1272 + shift..1276 + shift].copy_from_slice(&(1300u32 + shift as u32).to_le_bytes());
    assert_eq!(
        detected(&configured)[0].span.byte_len as usize,
        configured.len()
    );
}

#[test]
fn unreferenced_patterns_do_not_supply_a_playable_note_witness() {
    let mut it = it_fixture();
    it[38..40].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(detected(&it)[0].patterns, 2);
    it[192] = 1;
    assert!(detected(&it).is_empty());
    let mut s3m = s3m_fixture();
    s3m[36..38].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(detected(&s3m)[0].patterns, 2);
    s3m[96] = 1;
    assert!(detected(&s3m).is_empty());
}

#[test]
fn empty_slots_do_not_hide_later_playable_instruments_and_samples() {
    let mut it = it_fixture();
    it[34..38].copy_from_slice(&[2, 0, 2, 0]);
    for (offset, value) in [(193, 0u32), (197, 512), (201, 0), (205, 1200), (209, 1088)] {
        it[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    it[1099] = 2;
    it[512 + 64 + 48 * 2 + 1] = 2;
    assert_eq!(detected(&it)[0].samples, 1);
    it[512 + 64 + 48 * 2 + 1] = 1;
    assert!(detected(&it).is_empty());

    let mut s3m = s3m_fixture();
    s3m[34..36].copy_from_slice(&2u16.to_le_bytes());
    s3m[97..103].copy_from_slice(&[26, 0, 8, 0, 16, 0]);
    s3m[260] = 2;
    s3m.resize(416 + 80, 0);
    assert_eq!(detected(&s3m)[0].samples, 1);
    assert_eq!(detected(&s3m)[0].span.byte_len, 496);
    s3m[260] = 1;
    assert!(detected(&s3m).is_empty());
}
