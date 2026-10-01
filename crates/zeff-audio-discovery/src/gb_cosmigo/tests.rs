#[cfg(test)]
use std::sync::atomic::AtomicBool;

fn program(split: bool) -> Vec<u8> {
    let mut data = vec![0; 0x1000];
    data[..12].copy_from_slice(if split {
        &[
            0xc3, 0x32, 0x40, 0xc3, 0xed, 0x40, 0xc3, 0x98, 0x40, 0xc3, 0x16, 0x40,
        ]
    } else {
        &[
            0xc3, 0x32, 0x40, 0xc3, 0xe0, 0x40, 0xc3, 0x8b, 0x40, 0xc3, 0x16, 0x40,
        ]
    });
    data[0x16] = 0xc9;
    let start = if split { 0x56 } else { 0x32 };
    data[0x32] = 0xc9;
    if split {
        data[0x32..0x37].copy_from_slice(&[0x3e, 0x80, 0xe0, 0x26, 0xc9]);
    }
    let init = [
        0x3e, 0x77, 0xe0, 0x24, 0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x11, 0xe0, 0x25, 0x3e, 0x80, 0xe0,
        0x11, 0x3e, 0xf0, 0xe0, 0x12, 0x3e, 0x40, 0xe0, 0x13, 0x3e, 0x87, 0xe0, 0x14, 0xc9,
    ];
    data[start..start + init.len()].copy_from_slice(&init);
    data[0xe0] = 0xc9;
    data[0xed] = 0xc9;
    data
}

pub(super) fn recognized(data: &[u8], name: &str) -> bool {
    let split = match name {
        "cosmigo-four-channel-v1" => false,
        "cosmigo-four-channel-split-start" => true,
        _ => return false,
    };
    data[0x16..0x1000] == program(split)[0x16..]
}

fn fixture(split: bool) -> Vec<u8> {
    let mut bytes = vec![0; 0x8000];
    bytes[0x143] = 0xc0;
    bytes[0x147] = 0x19;
    bytes[0x4000..0x5000].copy_from_slice(&program(split));
    for (i, address) in [0x524e_u16, 0x5246, 0x5200, 0x5202, 0x5206]
        .into_iter()
        .enumerate()
    {
        bytes[0x400c + i * 2..0x400e + i * 2].copy_from_slice(&address.to_le_bytes());
    }
    bytes[0x5000..0x500a].copy_from_slice(&[0, 0, 1, 0, 2, 0, 3, 255, 0, 0]);
    for channel in 0..4 {
        let pattern = 0x5100 + channel * 6;
        bytes[pattern..pattern + 6].copy_from_slice(&[1, 0, 2, 0, 0, 254]);
        bytes[0x5246 + channel * 2..0x5248 + channel * 2]
            .copy_from_slice(&(pattern as u16).to_le_bytes());
    }
    bytes[0x5202..0x5206].copy_from_slice(&[0xf0, 0, 1, 254]);
    bytes[0x524e..0x5253].copy_from_slice(&[0, 0x50, 2, 2, 7]);
    bytes[0x14d] = bytes[0x134..=0x14c]
        .iter()
        .fold(0_u8, |v, &b| v.wrapping_sub(b).wrapping_sub(1));
    bytes
}

pub fn synthetic_rom() -> Vec<u8> {
    fixture(false)
}

#[cfg(test)]
fn songs(bytes: &[u8]) -> Vec<super::GbCosmigoSong> {
    let cancel = AtomicBool::new(false);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 2_000_000,
    };
    let mut result = Vec::new();
    super::scan(bytes, &mut result, &mut budget, 1024).unwrap();
    result
}

#[test]
fn authenticates_code_and_revalidates_inventory() {
    let mut bytes = synthetic_rom();
    let found = songs(&bytes);
    assert_eq!(found.len(), 1);
    assert!(found[0].tracks.iter().all(|t| t.note_count > 0));
    let cancel = AtomicBool::new(false);
    super::validate_song(&bytes, &found[0], &cancel).unwrap();
    bytes[0x4100] ^= 1;
    assert!(songs(&bytes).is_empty());
    assert!(super::prepare_rom(&bytes, &found[0], &cancel).is_err());
}

#[test]
fn rejects_uninitialized_envelopes_and_pitch_escape() {
    for (at, value) in [
        (0x5102, 253),
        (0x5102, 254),
        (0x5102, 130),
        (0x5100, 129),
        (0x5203, 84),
        (0x5202, 0xf4),
    ] {
        let mut bytes = synthetic_rom();
        bytes[at] = value;
        assert!(songs(&bytes).is_empty(), "{at:x}={value:x}");
    }
}

#[test]
fn rejects_order_cycles_and_cross_region_reads() {
    for (at, patch) in [
        (0x5000, &[255, 0, 0][..]),
        (0x5007, &[255, 0xff, 0xff][..]),
        (0x5246, &[0xff, 0x7f][..]),
        (0x5200, &[0x3f, 0xff][..]),
        (0x5205, &[255, 0, 0][..]),
        (0x524e, &[0, 0x50, 0, 2, 7][..]),
        (0x524e, &[0, 0x50, 2, 0, 7][..]),
        (0x524e, &[0, 0x50, 2, 2, 8][..]),
    ] {
        let mut bytes = synthetic_rom();
        bytes[at..at + patch.len()].copy_from_slice(patch);
        assert!(songs(&bytes).is_empty(), "{at:x}: {patch:?}");
    }
}

#[test]
fn selector_byte_domain_is_independent_of_padding() {
    let mut bytes = synthetic_rom();
    let entry = bytes[0x524e..0x5253].to_vec();
    bytes[0x524e + 255 * 5..0x5253 + 255 * 5].copy_from_slice(&entry);
    assert_eq!(
        songs(&bytes).iter().map(|s| s.index).collect::<Vec<_>>(),
        [0, 255]
    );
}

#[test]
fn requires_notes_and_valid_cartridge() {
    let mut bytes = synthetic_rom();
    bytes[0x5000] = 254;
    assert!(songs(&bytes).is_empty());
    for (at, value) in [(0x143, 0), (0x147, 1), (0x148, 5)] {
        let mut bytes = synthetic_rom();
        bytes[at] = value;
        assert!(songs(&bytes).is_empty());
    }
}

#[test]
fn bounded_scan_and_cancellation_propagate() {
    let bytes = synthetic_rom();
    let cancel = AtomicBool::new(false);
    let mut result = Vec::new();
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 1,
    };
    assert!(super::scan(&bytes, &mut result, &mut budget, 100).is_err());
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    assert_eq!(
        super::scan(&bytes, &mut result, &mut budget, 0),
        Err(crate::ScanStop::CandidateLimit)
    );
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(super::scan(&bytes, &mut result, &mut budget, 100).is_err());
}

#[test]
fn prepares_both_start_contracts_without_changing_payload() {
    for split in [false, true] {
        let bytes = fixture(split);
        let song = songs(&bytes).remove(0);
        let prepared = super::prepare_rom(&bytes, &song, &AtomicBool::new(false)).unwrap();
        assert_eq!(&prepared.bytes[0x4000..], &bytes[0x4000..]);
        assert_eq!(prepared.bytes.len(), 0x8000);
        assert_eq!(prepared.bytes[0x204..0x207], [0xcd, 3, 0x40]);
        let mut wrong = song;
        wrong.index = 256;
        assert!(super::prepare_rom(&bytes, &wrong, &AtomicBool::new(false)).is_err());
    }
}
