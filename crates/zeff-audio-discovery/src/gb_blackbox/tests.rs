use super::profiles::{Profile, Selection};

pub(super) const PROFILE: Profile = Profile {
    name: "blackbox-authored-cgb",
    hash: "",
    init: 0x300,
    selections: &[
        Selection {
            index: 2,
            bank: 1,
            module: 0x4000,
            initial: 0,
            restart: 1,
            entry_bank: 0,
            entry: 0x500,
            entry_len: 2,
            data_len: 0x480,
        },
        Selection {
            index: 7,
            bank: 1,
            module: 0x4800,
            initial: 4,
            restart: 4,
            entry_bank: 0,
            entry: 0x502,
            entry_len: 2,
            data_len: 0x480,
        },
    ],
};

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x8000];
    bytes[0x143] = 0xc0;
    bytes[0x147] = 0x19;
    bytes[0x300..0x31d].copy_from_slice(&[
        0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x77, 0xe0, 0x24, 0x3e, 0x11, 0xe0, 0x25, 0x3e, 0x80, 0xe0,
        0x11, 0x3e, 0xf0, 0xe0, 0x12, 0x3e, 0x40, 0xe0, 0x13, 0x3e, 0x87, 0xe0, 0x14, 0xc9,
    ]);
    bytes[0x386] = 0xc9;
    bytes[0x500..0x504].copy_from_slice(&[0, 0x40, 0, 0x48]);
    for at in [0x4000, 0x4800] {
        bytes[at..at + 3].copy_from_slice(&[4, 0x77, 0xff]);
        bytes[at + 0x400..at + 0x402].copy_from_slice(&[24, 1]);
    }
    bytes[0x14d] = bytes[0x134..0x14d]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_sub(byte).wrapping_sub(1));
    bytes
}

#[cfg(test)]
fn songs(bytes: &[u8]) -> Vec<super::GbBlackBoxSong> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 4_000_000,
    };
    let mut songs = Vec::new();
    super::scan(bytes, &mut songs, &mut budget, 100).unwrap();
    songs
}

#[test]
fn sparse_contexts_preserve_start_restart_and_native_code() {
    let bytes = synthetic_rom();
    let found = songs(&bytes);
    assert_eq!(found.len(), 2);
    assert_eq!(
        found.iter().map(|song| song.index).collect::<Vec<_>>(),
        [2, 7]
    );
    assert_eq!((found[0].initial_order, found[0].loop_order), (0, 1));
    assert_eq!((found[1].initial_order, found[1].loop_order), (4, 4));
    let cancel = std::sync::atomic::AtomicBool::new(false);
    for song in &found {
        let prepared = super::prepare_rom(&bytes, song, &cancel).unwrap();
        assert_eq!(prepared.bytes[0x300..], bytes[0x300..]);
        assert_eq!(
            prepared.timing,
            crate::gb_music::native::GbBankedTiming::CgbDouble
        );
        let start = song.module_address.to_le_bytes();
        assert!(prepared.bytes[0x150..0x200].windows(9).any(|code| code
            == [
                0x21,
                start[0],
                start[1],
                0x06,
                song.loop_order,
                0x0e,
                0,
                0x3e,
                song.initial_order
            ]));
    }
}

#[test]
fn source_authentication_rejects_changed_code_tables_and_unvisited_data() {
    let bytes = synthetic_rom();
    let selected = songs(&bytes).remove(0);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    for offset in [0x143, 0x148, 0x300, 0x386, 0x500, 0x4000, 0x4400, 0x7fff] {
        let mut altered = bytes.clone();
        altered[offset] ^= 1;
        assert!(songs(&altered).is_empty());
        assert!(super::prepare_rom(&altered, &selected, &cancel).is_err());
    }
    assert!(songs(&bytes[..0x7fff]).is_empty());
}

#[test]
fn forged_selection_metadata_and_limits_are_refused() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let bytes = synthetic_rom();
    let selected = songs(&bytes).remove(0);
    let cancel = AtomicBool::new(false);
    let mut variants = vec![selected.clone(); 6];
    variants[0].index = 3;
    variants[1].bank = 2;
    variants[2].module_address += 1;
    variants[3].initial_order += 1;
    variants[4].loop_order += 1;
    variants[5].mapped_spans.clear();
    for forged in variants {
        assert!(super::prepare_rom(&bytes, &forged, &cancel).is_err());
    }
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 0,
    };
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 100),
        Err(crate::ScanStop::WorkLimit)
    );
    budget.remaining = 4_000_000;
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 0),
        Err(crate::ScanStop::CandidateLimit)
    );
    cancel.store(true, Ordering::Relaxed);
    assert!(super::prepare_rom(&bytes, &selected, &cancel).is_err());
}
