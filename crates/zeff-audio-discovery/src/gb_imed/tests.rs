use super::profiles::Profile;

pub(super) const PROFILE: Profile = Profile {
    name: "imed-authored-cgb",
    init: 0x4000,
    tick: 0x4100,
    state_start: 0xc100,
    state_end: 0xc160,
    sfx_flag: 0xc000,
    initial_mask: Some(0xc152),
    double_speed: true,
    wram_bank: 1,
    data_start: 0x5000,
    extra_banks: &[],
    checks: &[],
};

pub(super) const DMG_PROFILE: Profile = Profile {
    name: "imed-authored-dmg",
    double_speed: false,
    initial_mask: None,
    ..PROFILE
};

fn program() -> Vec<u8> {
    let mut bytes = vec![0; 0x100];
    let code = [
        0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x77, 0xe0, 0x24, 0x3e, 0x11, 0xe0, 0x25, 0x3e, 0x80, 0xe0,
        0x11, 0x3e, 0xf0, 0xe0, 0x12, 0x3e, 0x40, 0xe0, 0x13, 0x3e, 0x87, 0xe0, 0x14, 0xc9,
    ];
    bytes[..code.len()].copy_from_slice(&code);
    bytes
}

pub(super) fn recognized(bytes: &[u8]) -> Option<&'static Profile> {
    if bytes.len() < 0x8000 || bytes[0x4000..0x4100] != program() || bytes[0x4100] != 0xc9 {
        return None;
    }
    Some(if matches!(bytes[0x143], 0x80 | 0xc0) {
        &PROFILE
    } else {
        &DMG_PROFILE
    })
}

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x8000];
    bytes[0x143] = 0xc0;
    bytes[0x147] = 0x19;
    bytes[0x4000..0x4100].copy_from_slice(&program());
    bytes[0x4100] = 0xc9;
    for at in [0x5000, 0x5200] {
        bytes[at..at + 8].copy_from_slice(b"IMEDGBoy");
        for (i, word) in [20u16, 24, 158, 162, 166, 182].iter().enumerate() {
            bytes[at + 8 + i * 2..at + 10 + i * 2].copy_from_slice(&word.to_le_bytes());
        }
        bytes[at + 20..at + 28].copy_from_slice(&[0, 255, 1, 0, 4, 0, 134, 0]);
        bytes[at + 28..at + 32].copy_from_slice(&[0xc0, 0, 24, 1]);
        bytes[at + 158..at + 162].copy_from_slice(&[0, 0x80, 0xf0, 0x11]);
        bytes[at + 162..at + 166].copy_from_slice(&[0, 0x20, 0, 0x44]);
    }
    bytes[0x14d] = bytes[0x134..=0x14c]
        .iter()
        .fold(0u8, |sum, &b| sum.wrapping_sub(b).wrapping_sub(1));
    bytes
}

#[cfg(test)]
fn songs(bytes: &[u8]) -> Vec<super::GbImedSong> {
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
fn discovers_modules_and_preserves_both_hardware_contracts() {
    for dmg in [false, true] {
        let mut bytes = synthetic_rom();
        if dmg {
            bytes[0x143] = 0;
            bytes[0x147] = 1;
        }
        let found = songs(&bytes);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].module_address, 0x5000);
        assert_eq!(found[1].module_address, 0x5200);
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let prepared = super::prepare_rom(&bytes, &found[0], &cancel).unwrap();
        assert_eq!(prepared.bytes[0x4000..], bytes[0x4000..]);
        assert_eq!(
            prepared.bytes[0x150..0x200]
                .windows(6)
                .any(|code| code == [0x3e, 0xff, 0xea, 0x52, 0xc1, 0xaf]),
            !dmg
        );
        assert_eq!(
            prepared.timing,
            if dmg {
                crate::gb_music::native::GbBankedTiming::Dmg
            } else {
                crate::gb_music::native::GbBankedTiming::CgbDouble
            }
        );
        bytes[0x4000] ^= 1;
        assert!(super::validate_song(&bytes, &found[0], &cancel).is_err());
    }
}

#[test]
fn rejects_invalid_headers_patterns_and_instrument_reads() {
    for (offset, patch) in [
        (8, &[21][..]),
        (20, &[128][..]),
        (22, &[129][..]),
        (24, &[6][..]),
        (26, &[135][..]),
        (28, &[0xff, 0xff][..]),
        (30, &[97][..]),
        (31, &[2][..]),
        (160, &[0][..]),
        (18, &[170][..]),
    ] {
        let mut bytes = synthetic_rom();
        bytes[0x5000 + offset..0x5000 + offset + patch.len()].copy_from_slice(patch);
        assert!(
            !songs(&bytes).iter().any(|s| s.module_address == 0x5000),
            "{offset}: {patch:?}"
        );
        assert!(songs(&bytes).iter().any(|s| s.module_address == 0x5200));
    }
}

#[test]
fn truncated_inputs_and_invalid_roots_fail_closed() {
    let bytes = synthetic_rom();
    for end in [0, 0x14f, 0x4000, 0x5008, 0x5080, 0x7fff] {
        assert!(songs(&bytes[..end]).is_empty());
    }
    let mut song = songs(&bytes).remove(0);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    song.index = 1;
    assert!(super::prepare_rom(&bytes, &song, &cancel).is_err());
    song.index = 0;
    song.module_address = 0x7fff;
    assert!(super::prepare_rom(&bytes, &song, &cancel).is_err());
}

#[test]
fn exact_mirrors_are_canonicalized_but_mismatched_copies_reject() {
    let bytes = synthetic_rom();
    let mut mirror = bytes.repeat(4);
    assert_eq!(songs(&mirror), songs(&bytes));
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let prepared = super::prepare_rom(&mirror, &songs(&mirror)[0], &cancel).unwrap();
    assert_eq!(prepared.bytes.len(), bytes.len());
    mirror[0x9000] = 1;
    assert!(songs(&mirror).is_empty());
}

#[test]
fn scan_limits_and_changed_asset_inventory_are_rejected() {
    let bytes = synthetic_rom();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 0,
    };
    assert!(super::scan(&bytes, &mut Vec::new(), &mut budget, 100).is_err());
    budget.remaining = 4_000_000;
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 0),
        Err(crate::ScanStop::CandidateLimit)
    );
    let mut song = songs(&bytes).remove(0);
    song.mapped_spans.clear();
    assert!(super::prepare_rom(&bytes, &song, &cancel).is_err());
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(super::scan(&bytes, &mut Vec::new(), &mut budget, 100).is_err());
}
