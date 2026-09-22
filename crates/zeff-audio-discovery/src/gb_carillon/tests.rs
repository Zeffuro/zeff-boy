#[cfg(not(test))]
use super::profiles;
#[cfg(test)]
use super::*;

fn program() -> Vec<u8> {
    let mut data = vec![0; 0x600];
    data[..15].copy_from_slice(&[
        0xc3, 0x56, 0x40, 0xc3, 0x88, 0x40, 0xc3, 0xe0, 0x40, 0xc3, 0, 0x41, 0xc3, 0xe8, 0x40,
    ]);
    data[0x56..0x67].copy_from_slice(&[
        0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x77, 0xe0, 0x24, 0x3e, 0x11, 0xe0, 0x25, 0xaf, 0xea, 0xc0,
        0xc7, 0xc9,
    ]);
    data[0x88] = 0xc9;
    data[0xe0] = 0xc9;
    data[0xe8..0xf2].copy_from_slice(&[0x21, 0xf2, 0x40, 0x85, 0x6f, 0x7e, 0xea, 0xc5, 0xc7, 0xc9]);
    data[0xf2..0xfa].copy_from_slice(&[255, 127, 63, 191, 31, 95, 159, 223]);
    data[0x100..0x119].copy_from_slice(&[
        0xfa, 0xc0, 0xc7, 0xb7, 0xc0, 0x3c, 0xea, 0xc0, 0xc7, 0x3e, 0x80, 0xe0, 0x11, 0x3e, 0xf0,
        0xe0, 0x12, 0x3e, 0x40, 0xe0, 0x13, 0x3e, 0x87, 0xe0, 0x14,
    ]);
    data[0x119] = 0xc9;
    data
}

fn alternate_program() -> Vec<u8> {
    let mut data = program();
    data[..15].copy_from_slice(&[
        0xc3, 0x56, 0x40, 0xc3, 0x85, 0x40, 0xc3, 0xdf, 0x40, 0xc3, 0, 0x41, 0xc3, 0xe7, 0x40,
    ]);
    data[0x85..0x8b].copy_from_slice(&[0x3e, 0xff, 0xea, 0xd4, 0xc7, 0xc9]);
    data[0xdf] = 0xc9;
    data[0xe7..0xf1].copy_from_slice(&[0x21, 0xf1, 0x40, 0x85, 0x6f, 0x7e, 0xea, 0xc5, 0xc7, 0xc9]);
    data[0xf1..0xf9].copy_from_slice(&[255, 127, 63, 191, 31, 95, 159, 223]);
    data
}

pub(super) fn recognized(data: &[u8], profile: &str) -> bool {
    match profile {
        "carillon-cgb-v1" => data[..0x600] == program(),
        "carillon-cgb-v1-isolated" => data[..0x600] == alternate_program(),
        _ => false,
    }
}

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x8000];
    bytes[0x143] = 0xc0;
    bytes[0x147] = 0x19;
    bytes[0x4000..0x4600].copy_from_slice(&program());
    for (i, target) in profiles::COMMANDS.iter().enumerate() {
        bytes[0x46e0 + i * 2..0x46e2 + i * 2].copy_from_slice(&target.to_le_bytes());
    }
    bytes[0x4800] = 0xf1;
    bytes[0x4901] = 255;
    bytes[0x4f00] = 0x50;
    bytes[0x4f02] = 255;
    bytes[0x5000] = 48;
    let checksum = bytes[0x134..=0x14c]
        .iter()
        .fold(0_u8, |v, &b| v.wrapping_sub(b).wrapping_sub(1));
    bytes[0x14d] = checksum;
    bytes
}

fn synthetic_rom_alternate_in_bank(bank: usize) -> Vec<u8> {
    let len = (bank + 1).next_power_of_two() * 0x4000;
    let mut bytes = vec![0; len];
    bytes[0x143] = 0xc0;
    bytes[0x147] = 0x19;
    bytes[0x148] = (len / 0x8000).ilog2() as u8;
    bytes[bank * 0x4000..bank * 0x4000 + 0x600].copy_from_slice(&alternate_program());
    for (i, target) in profiles::COMMANDS.iter().enumerate() {
        bytes[bank * 0x4000 + 0x6e0 + i * 2..bank * 0x4000 + 0x6e2 + i * 2]
            .copy_from_slice(&target.to_le_bytes());
    }
    bytes[bank * 0x4000 + 0x800] = 0xf1;
    bytes[bank * 0x4000 + 0x901] = 255;
    bytes[bank * 0x4000 + 0xf00] = 0x50;
    bytes[bank * 0x4000 + 0xf02] = 255;
    bytes[bank * 0x4000 + 0x1000] = 48;
    let checksum = bytes[0x134..=0x14c]
        .iter()
        .fold(0_u8, |v, &b| v.wrapping_sub(b).wrapping_sub(1));
    bytes[0x14d] = checksum;
    bytes
}

pub fn synthetic_rom_alternate() -> Vec<u8> {
    synthetic_rom_alternate_in_bank(1)
}

#[cfg(test)]
fn songs(bytes: &[u8]) -> Vec<GbCarillonSong> {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut songs = Vec::new();
    scan(bytes, &mut songs, &mut budget, 20).unwrap();
    songs
}

#[test]
fn shared_start_aliases_are_one_validated_entry() {
    let bytes = synthetic_rom();
    let found = songs(&bytes);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].aliases, [1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(found[0].tracks[0].note_count, 1);
    assert_eq!(found[0].order_address, 0x4f00);
    validate_song(&bytes, &found[0], &AtomicBool::new(false)).unwrap();
    let prepared = prepare_rom(&bytes, &found[0], &AtomicBool::new(false)).unwrap();
    assert_eq!(
        prepared.timing,
        crate::gb_music::native::GbBankedTiming::CgbDouble
    );
    assert_eq!(&prepared.bytes[0x4000..], &bytes[0x4000..]);
}

#[test]
fn changed_contract_and_forged_inventory_reject() {
    let bytes = synthetic_rom();
    let found = songs(&bytes).remove(0);
    let cancel = AtomicBool::new(false);
    for offset in [0x4140, 0x46e0, 0x4f00] {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert!(validate_song(&changed, &found, &cancel).is_err());
    }
    let mut forged = found;
    forged.aliases.clear();
    assert!(prepare_rom(&bytes, &forged, &cancel).is_err());
}

#[test]
fn immediate_cycles_samples_and_non_rom_patterns_reject() {
    for fixture in [synthetic_rom(), synthetic_rom_alternate()] {
        for (offset, value) in [(0x4901, 1), (0x4f00, 0x80), (0x5004, 255)] {
            let mut bytes = fixture.clone();
            bytes[offset] = value;
            assert!(songs(&bytes).is_empty());
        }
        let mut bytes = fixture;
        bytes[0x4f00] = 0;
        assert!(songs(&bytes).is_empty());
    }
}

#[test]
fn independent_starts_stay_distinct_and_budget_is_shared() {
    let mut bytes = synthetic_rom();
    bytes[0x4f80] = 0x51;
    bytes[0x4f82] = 255;
    bytes[0x5100] = 50;
    assert_eq!(songs(&bytes).len(), 2);
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut found = Vec::new();
    assert_eq!(
        scan(&bytes, &mut found, &mut budget, 1),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(found.len(), 1);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 0,
    };
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut budget, 20),
        Err(ScanStop::WorkLimit)
    );
    assert!(validate_song(&bytes, &found[0], &AtomicBool::new(true)).is_err());
}

#[test]
fn alternate_profile_isolated_image_preserves_its_source_bank() {
    let bytes = synthetic_rom_alternate_in_bank(3);
    let original = bytes.clone();
    let found = songs(&bytes);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].profile, "carillon-cgb-v1-isolated");
    assert_eq!(found[0].bank, 3);
    assert_eq!(found[0].table_entry.canonical_cpu_address, 0x40f1);
    assert_eq!(found[0].aliases, [1, 2, 3, 4, 5, 6, 7]);
    let prepared = prepare_rom(&bytes, &found[0], &AtomicBool::new(false)).unwrap();
    assert_eq!(prepared.bytes.len(), 0x8000);
    assert_eq!(prepared.bytes[0x143], 0xc0);
    assert_eq!(&prepared.bytes[0x147..0x14a], &[0x19, 0, 0]);
    assert_eq!(&prepared.bytes[0x4000..], &original[3 * 0x4000..4 * 0x4000]);
    assert_eq!(bytes, original);
    assert_eq!(
        prepared.timing,
        crate::gb_music::native::GbBankedTiming::CgbDouble
    );
}

#[test]
fn alternate_profile_accepts_only_its_special_source_headers() {
    for mapper in [0x97, 0x99] {
        let mut bytes = synthetic_rom_alternate();
        bytes[0x147] = mapper;
        bytes[0x148] = 7;
        assert!(!supports_cartridge(&bytes));
        assert_eq!(songs(&bytes).len(), 1);
    }
    let mut old = synthetic_rom();
    for mapper in [0x97, 0x99] {
        old[0x147] = mapper;
        assert!(songs(&old).is_empty());
    }
    let mut truncated = synthetic_rom_alternate();
    truncated.truncate(0x7000);
    assert!(songs(&truncated).is_empty());
    for (offset, value) in [(0x143, 0), (0x147, 0)] {
        let mut bytes = synthetic_rom_alternate();
        bytes[offset] = value;
        assert!(songs(&bytes).is_empty());
    }
    let mut partial_tail = synthetic_rom_alternate();
    partial_tail[0x147] = 0x97;
    partial_tail.push(0);
    assert!(songs(&partial_tail).is_empty());
    let mut oversized = vec![0; 0x80_4000];
    oversized[..0x8000].copy_from_slice(&synthetic_rom_alternate());
    oversized[0x147] = 0x99;
    assert!(songs(&oversized).is_empty());
}

#[test]
fn alternate_revalidation_and_profile_forgery_reject() {
    let bytes = synthetic_rom_alternate();
    let found = songs(&bytes).remove(0);
    let cancel = AtomicBool::new(false);
    validate_song(&bytes, &found, &cancel).unwrap();
    let mut forged = found.clone();
    forged.profile = "carillon-cgb-v1";
    assert!(prepare_rom(&bytes, &forged, &cancel).is_err());
    for offset in [0x4004, 0x40f1, 0x46e0] {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert!(validate_song(&changed, &found, &cancel).is_err());
    }
}
