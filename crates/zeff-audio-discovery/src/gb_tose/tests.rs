use super::*;

pub(super) static PROFILE: profiles::Profile = profiles::Profile {
    name: "gb-tose-synthetic-v1",
    prefix: &[0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x77],
    len: 0x100,
    relocations: &[],
    table_operand: 0xf0,
    hash: "07d61067f30c606407b7de6d28adab4af3985fef389d0946c94bc4d7c84eadbc",
    selector: 0x40,
    tick: 0x80,
};

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x10000];
    bytes[0x100..0x103].copy_from_slice(&[0xc3, 0, 1]);
    bytes[0x147] = 1;
    bytes[0x148] = 1;
    bytes[0x1000..0x1011].copy_from_slice(&[
        0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x77, 0xe0, 0x24, 0xaf, 0xe0, 0x25, 0xea, 0x97, 0xdd, 0xc9,
        0, 0,
    ]);
    bytes[0x1040..0x1058].copy_from_slice(&[
        0xea, 0x9e, 0xdd, 0x3e, 0x11, 0xe0, 0x25, 0x3e, 0x80, 0xe0, 0x11, 0x3e, 0xf0, 0xe0, 0x12,
        0x3e, 0x20, 0xe0, 0x13, 0x3e, 0x87, 0xe0, 0x14, 0xc9,
    ]);
    bytes[0x1080..0x1088].copy_from_slice(&[0x21, 0x9c, 0xdd, 0x34, 0x7e, 0xe0, 0x13, 0xc9]);
    bytes[0x10f0..0x10f2].copy_from_slice(&0x4000u16.to_le_bytes());
    for channel in 0..4 {
        let at = 0x8000 + channel * 4;
        bytes[at] = (channel as u8 + 2) * 25;
        bytes[at + 1] = channel as u8;
        bytes[at + 2..at + 4].copy_from_slice(&(0x4040u16 + channel as u16 * 16).to_le_bytes());
        let sequence = 0x8040 + channel * 16;
        bytes[sequence..sequence + 7].copy_from_slice(&[0, 0, 15, 0, 0x40, 4, 0xff]);
    }
    bytes
}

#[cfg(test)]
fn inventory(bytes: &[u8]) -> Vec<GbToseSong> {
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
fn executable_inventory_revalidates_while_unrelated_bytes_remain_portable() {
    let bytes = synthetic_rom();
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].bank, 2);
    assert_eq!(serde_json::to_value(songs[0].hardware).unwrap(), "dmg");
    assert!(songs[0].tracks.iter().all(|track| track.note_count == 1));
    validate_song(&bytes, &songs[0], &AtomicBool::new(false)).unwrap();
    let mut changed = bytes.clone();
    changed[0x3fff] = 1;
    assert_eq!(inventory(&changed), songs);
    changed[0x1084] ^= 1;
    assert!(inventory(&changed).is_empty());
    assert!(validate_song(&changed, &songs[0], &AtomicBool::new(false)).is_err());
    let mut stale = songs[0].clone();
    stale.bank = 0;
    assert!(validate_song(&bytes, &stale, &AtomicBool::new(false)).is_err());
    stale = songs[0].clone();
    stale.tracks[0].note_count = 2;
    assert!(validate_song(&bytes, &stale, &AtomicBool::new(false)).is_err());
    assert!(validate_song(&bytes, &songs[0], &AtomicBool::new(true)).is_err());
}

#[test]
fn bank_and_table_relocation_preserves_qualified_selectors() {
    let mut bytes = synthetic_rom();
    bytes.copy_within(0x8000..0xc000, 0xc000);
    assert_eq!(inventory(&bytes).len(), 2);
    bytes[0x10f0..0x10f2].copy_from_slice(&0x4100u16.to_le_bytes());
    bytes.copy_within(0x8000..0x8010, 0x8100);
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].table_entry.effective_offset, 0x8100);
}

#[test]
fn invalid_channel_groups_headers_and_hardware_are_rejected() {
    for (at, value) in [
        (0x143, 0x80),
        (0x147, 0x19),
        (0x148, 6),
        (0x8000, 0),
        (0x8005, 3),
        (0x8003, 0x80),
        (0x8002, 0xff),
        (0x8003, 0x7f),
    ] {
        let mut bytes = synthetic_rom();
        bytes[at] = value;
        assert!(inventory(&bytes).is_empty(), "offset {at:x}");
    }
    assert!(inventory(&synthetic_rom()[..0xffff]).is_empty());
}

#[test]
fn sequences_must_yield_stay_in_bank_and_keep_repeat_targets_out_of_headers() {
    for code in [&[0xb0, 2][..], &[0xfd, 0, 0xb0, 0], &[0xb0, 0]] {
        let mut bytes = synthetic_rom();
        bytes[0x8044..0x8044 + code.len()].copy_from_slice(code);
        assert!(inventory(&bytes).is_empty(), "code {code:?}");
    }
    let mut bytes = synthetic_rom();
    bytes[0x8044..0x804a].copy_from_slice(&[0x40, 4, 0xb2, 2, 0xff, 0]);
    assert_eq!(inventory(&bytes)[0].tracks[0].note_count, 3);
    bytes[0x8044..0x8048].copy_from_slice(&[0x40, 0, 0xb0, 2]);
    assert!(!inventory(&bytes).is_empty());
    for channel in 0..4 {
        bytes[0x8044 + channel * 16] = 0xff;
    }
    assert!(inventory(&bytes).is_empty());
}

#[test]
fn prepared_program_uses_dmg_banking_and_reserved_handshake_bytes() {
    let bytes = synthetic_rom();
    let song = &inventory(&bytes)[0];
    let prepared = prepare_rom(&bytes, song, &AtomicBool::new(false)).unwrap();
    assert_eq!(prepared.ready_address, 0xff81);
    assert_eq!(prepared.ack_address, 0xff80);
    assert_eq!(&prepared.bytes[0x1000..0x1100], &bytes[0x1000..0x1100]);
    assert_eq!(&prepared.bytes[0x8000..0x8080], &bytes[0x8000..0x8080]);
    assert_eq!(&prepared.bytes[0x40..0x43], &[0xc3, 0, 2]);
}

#[test]
fn repeated_multicart_headers_cannot_change_the_qualified_bank_mapping() {
    let mut bytes = synthetic_rom();
    bytes.resize(0x100000, 0);
    bytes[0x148] = 5;
    bytes[0x104..0x134].fill(1);
    assert!(supports_cartridge(&bytes));
    assert_eq!(inventory(&bytes).len(), 1);
    for bank in [16, 32, 48] {
        bytes.copy_within(0x104..0x134, bank * 0x4000 + 0x104);
    }
    assert!(!supports_cartridge(&bytes));
    assert!(inventory(&bytes).is_empty());
}
