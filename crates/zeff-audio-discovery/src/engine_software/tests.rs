use std::sync::atomic::AtomicBool;

use super::*;
use crate::{Budget, RomSpan, ScanStop};

const BASE: usize = 0x40;

fn put_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

use crate::test_support::engine_software::fixture;

fn scanned(bytes: &[u8]) -> Vec<EngineSoftwareSong> {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut songs = Vec::new();
    scan(bytes, &mut songs, &mut budget, 16).unwrap();
    songs
}

fn aliased_song_bank(count: u8) -> Vec<u8> {
    let instrument = 4 + usize::from(count) * 4;
    let song = instrument + INSTRUMENT_HEADER_LEN;
    let pattern = song + 12;
    let mut bytes = vec![0; pattern + 2 * (4 + 4096 * 4)];
    put_u16(&mut bytes, 0, BANK_ID);
    bytes[2] = 1;
    bytes[3] = count;
    for index in 0..usize::from(count) {
        put_u32(&mut bytes, 4 + index * 4, song as u32);
    }
    for envelope in [instrument + 20, instrument + 72] {
        bytes[envelope + 1..envelope + 4].fill(255);
    }
    bytes[song..song + 6].copy_from_slice(&[32, 1, 0, 2, 6, 125]);
    for at in [pattern, pattern + 4 + 4096 * 4] {
        put_u16(&mut bytes, at, 4096);
    }
    bytes
}

#[test]
fn transient_graph_budget_is_shared_across_aliased_song_entries() {
    let mut bytes = fixture();
    bytes.extend(aliased_song_bank(60));
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: crate::MAX_SCAN_WORK,
    };
    let mut songs = Vec::new();
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget, 256),
        Err(ScanStop::InventoryLimit)
    );
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].bank.effective_offset, BASE as u32);
    assert!(budget.remaining > crate::MAX_SCAN_WORK - 1_000_000);
}

#[test]
fn a_full_candidate_inventory_stops_before_decoding_a_bank() {
    let bytes = aliased_song_bank(60);
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: crate::MAX_SCAN_WORK,
    };
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut budget, 0),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(budget.remaining, crate::MAX_SCAN_WORK);
}

#[test]
fn inventory_limit_accounts_for_the_output_vector_allocation() {
    let bytes = fixture();
    let expected = scanned(&bytes);
    let limit = retained_owned_bytes(&expected);
    let cancel = AtomicBool::new(false);
    for (budget_bytes, expected_count) in [(limit - 1, 0), (limit, 1)] {
        let mut songs = Vec::new();
        let status = scan_with_inventory_limit(
            &bytes,
            &mut songs,
            &mut Budget {
                cancel: &cancel,
                remaining: 100_000,
            },
            16,
            budget_bytes,
        );
        assert_eq!(songs.len(), expected_count);
        assert!(retained_owned_bytes(&songs) <= budget_bytes);
        assert_eq!(
            status,
            if expected_count == 0 {
                Err(ScanStop::InventoryLimit)
            } else {
                Ok(())
            }
        );
    }
}

#[test]
fn retains_full_bounded_graph_and_zero_row_pointer() {
    let bytes = fixture();
    let songs = scanned(&bytes);
    assert_eq!(songs.len(), 1);
    let song = &songs[0];
    assert_eq!(song.header, RomSpan::new(0xc8, 8));
    assert_eq!(song.bank, RomSpan::new(BASE, 136));
    assert_eq!((song.index, song.channels), (0, 2));
    assert!(song.warnings.is_empty(), "{:?}", song.warnings);
    assert!(song.mapped_spans.contains(&RomSpan::new(0xd4, 4)));
    assert!(song.mapped_spans.contains(&RomSpan::new(0xd8, 8)));
    assert!(song.mapped_spans.contains(&RomSpan::new(0xe0, 4)));
    assert!(song.mapped_spans.iter().all(|span| {
        bytes
            .get(span.effective_offset as usize..(span.effective_offset + span.byte_len) as usize)
            .is_some()
    }));

    let xm = to_xm(&bytes, song, &AtomicBool::new(false)).unwrap();
    assert!(xm.starts_with(b"Extended Module: "));
    assert_eq!(u16::from_le_bytes(xm[341..343].try_into().unwrap()), 2);
    assert_eq!(&xm[345..350], &[0x80, 0x80, 0x83, 49, 1]);
}

#[test]
fn zero_pattern_row_count_is_not_treated_as_an_empty_row() {
    let mut bytes = fixture();
    put_u16(&mut bytes, 0xd4, 0);
    assert!(scanned(&bytes).is_empty());
}

#[test]
fn unreachable_zero_patterns_retain_raw_headers_and_remap_xm_orders() {
    let bytes = crate::test_support::engine_software::unreachable_zero_pattern();
    let song = scanned(&bytes).remove(0);
    assert!(song.mapped_spans.contains(&RomSpan::new(0xdc, 4)));
    assert!(
        song.warnings
            .iter()
            .any(|warning| warning.contains("unreachable"))
    );
    let cancel = AtomicBool::new(false);
    let bank = parse_bank(
        &bytes,
        BASE,
        &mut ExportMeter {
            cancel: &cancel,
            remaining: 100_000,
        },
    )
    .unwrap();
    let xm = project::project_xm(&bytes, &bank, &bank.songs[0], &song, &cancel).unwrap();
    assert_eq!(xm.orders, [0, 1]);
    assert_eq!(xm.restart, 1);
    assert_eq!(xm.patterns.len(), 2);
    assert_eq!(
        (
            xm.patterns[0].cells[0].effect,
            xm.patterns[0].cells[0].parameter
        ),
        (0x0b, 1)
    );
    assert_eq!(
        (
            xm.patterns[1].cells[0].effect,
            xm.patterns[1].cells[0].parameter
        ),
        (0x0b, 0)
    );
    assert!(to_xm(&bytes, &song, &cancel).is_ok());
}

#[test]
fn malformed_or_truncated_headers_do_not_retain_a_candidate() {
    let mut bytes = fixture();
    bytes.truncate(BASE + 7);
    assert!(scanned(&bytes).is_empty());
    let mut bytes = fixture();
    put_u32(&mut bytes, BASE + 4, u32::MAX);
    assert!(scanned(&bytes).is_empty());
    let mut bytes = fixture();
    bytes[0xe3] = 2;
    assert!(scanned(&bytes).is_empty());
}

#[test]
fn provenance_and_cancellation_are_checked_before_xm_export() {
    let bytes = fixture();
    let song = scanned(&bytes).remove(0);
    let mut forged = song.clone();
    forged.title.push('!');
    assert!(to_xm(&bytes, &forged, &AtomicBool::new(false)).is_err());
    assert!(to_xm(&bytes, &song, &AtomicBool::new(true)).is_err());
}

#[test]
fn scan_preserves_partial_results_at_candidate_and_work_limits() {
    let bytes = fixture();
    let cancel = AtomicBool::new(false);
    let mut songs = Vec::new();
    assert_eq!(
        scan(
            &bytes,
            &mut songs,
            &mut Budget {
                cancel: &cancel,
                remaining: 100_000,
            },
            0,
        ),
        Err(ScanStop::CandidateLimit)
    );
    assert!(songs.is_empty());
    assert_eq!(
        scan(
            &bytes,
            &mut songs,
            &mut Budget {
                cancel: &cancel,
                remaining: 1,
            },
            16,
        ),
        Err(ScanStop::WorkLimit)
    );
}
