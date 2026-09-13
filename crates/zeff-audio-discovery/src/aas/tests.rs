use super::fixture::*;
use super::*;
use crate::gba_bootstrap;

fn discover(bytes: &[u8], limit: usize) -> (Vec<AasSong>, Result<(), ScanStop>) {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let mut songs = Vec::new();
    let result = scan(bytes, &mut songs, &mut budget, limit);
    (songs, result)
}

#[test]
fn both_native_abis_bind_tables_and_prepare_selected_driver() {
    for current in [false, true] {
        let bytes = fixture(current);
        let original = bytes.clone();
        let (songs, result) = discover(&bytes, 8);
        assert_eq!(result, Ok(()));
        assert_eq!(songs.len(), 2);
        assert_eq!(songs[0].channels, 2);
        assert_eq!(songs[0].notes, 1);
        assert_eq!(songs[0].patterns, 1);
        assert_eq!(songs[0].instruments, 1);
        assert_eq!(songs[0].samples, 1);
        assert_eq!(songs[0].native.max_channels, if current { 16 } else { 8 });
        let ready = prepare_rom(&bytes, &songs[1], &AtomicBool::new(false)).unwrap();
        assert_eq!(bytes, original);
        assert_eq!(&ready.bytes[..CONFIG], &bytes[..CONFIG]);
        assert_eq!(
            &ready.bytes[CONFIG + 12..bytes.len()],
            &bytes[CONFIG + 12..]
        );
        assert_eq!(
            word(&ready.bytes, CONFIG + 8),
            Some(0x0800_0000 + bytes.len() as u32)
        );
        let wait = ready.wait_loop.effective_offset as usize;
        assert_eq!(word(&ready.bytes, wait), Some(0xe590_1004));
        assert_eq!(word(&ready.bytes, wait + 4), Some(0xe351_0001));
        assert!(
            ready.bytes[bytes.len()..]
                .windows(4)
                .any(|b| b == gba_bootstrap::READY_VALUE.to_le_bytes())
        );
    }
}

#[test]
fn invalid_notes_samples_and_orders_do_not_become_songs() {
    for (at, value) in [
        (PATTERNS, 61 << 12),
        (ROOT + 4, u32::MAX),
        (SEQUENCE, 0xffff_ffff),
    ] {
        let mut bytes = fixture(true);
        put32(&mut bytes, at, value);
        let (songs, result) = discover(&bytes, 8);
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].index, 1);
        assert_eq!(result, Err(ScanStop::ValidationLimit));
    }
}

#[test]
fn driver_requires_shared_state_call_chain_and_caller() {
    for at in [PLAY + 4, IRQ + 0xb6, PLAY + 0x200, 0x40] {
        let mut bytes = fixture(true);
        put32(&mut bytes, at, 0);
        assert!(discover(&bytes, 8).0.is_empty());
    }
}

#[test]
fn candidate_capacity_only_stops_an_admissible_song() {
    assert_eq!(discover(&[0; 1024], 0).1, Ok(()));
    assert_eq!(discover(&fixture(true), 0).1, Err(ScanStop::CandidateLimit));
    let (songs, result) = discover(&fixture(true), 1);
    assert_eq!(songs.len(), 1);
    assert_eq!(result, Err(ScanStop::CandidateLimit));
    let mut songs = songs;
    let original = songs.clone();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1024,
    };
    assert_eq!(scan(&[0; 1024], &mut songs, &mut budget, 1), Ok(()));
    assert_eq!(songs, original);
}

#[test]
fn cancellation_work_and_owned_memory_are_bounded() {
    let bytes = fixture(false);
    for (cancelled, work, expected) in [
        (true, MAX_VALIDATION_WORK, ScanStop::Cancelled),
        (false, 1, ScanStop::WorkLimit),
    ] {
        let cancel = AtomicBool::new(cancelled);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: work,
        };
        assert_eq!(scan(&bytes, &mut Vec::new(), &mut budget, 8), Err(expected));
    }
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    assert_eq!(
        scan_with_retained_limit(&bytes, &mut Vec::new(), &mut budget, 8, 0),
        Err(ScanStop::InventoryLimit)
    );
}

#[test]
fn stale_native_metadata_and_full_rom_placement_are_rejected() {
    let bytes = fixture(true);
    let (mut songs, _) = discover(&bytes, 8);
    songs[0].index = 999;
    assert!(prepare_rom(&bytes, &songs[0], &AtomicBool::new(false)).is_err());
    assert!(bootstrap::placement(&vec![0; MAX_ROM_BYTES]).is_none());
}
