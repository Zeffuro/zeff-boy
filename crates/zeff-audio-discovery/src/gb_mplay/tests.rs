use super::profiles::{OrderLayout, Profile};

pub(super) const PROFILE: Profile = Profile {
    name: "mplay-authored",
    bank: 1,
    tick: 0x3000,
    tick_end: 0x3001,
    tone: 0x4100,
    wram_bank: 1,
    selector_count: 2,
    instruments: 0x5200,
    instruments_end: 0x5208,
    arp: 0x5300,
    duty: 0x5320,
    wave: 0x5340,
    wave_end: 0x5350,
    order: 0x5400,
    order_end: 0x5404,
    layout: OrderLayout::Indexed {
        selectors: 0x5600,
        groups: 0x5602,
        banks: 0x5612,
        speed: 0x5500,
    },
    fixed_hash: "",
    bank_hash: "",
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

pub(super) fn recognized(bytes: &[u8]) -> bool {
    bytes.len() >= 0x8000 && bytes[0x3000] == 0xc9 && bytes[0x4000..0x4100] == program()
}

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x8000];
    bytes[0x143] = 0xc0;
    bytes[0x147] = 0x19;
    bytes[0x3000] = 0xc9;
    bytes[0x4000..0x4100].copy_from_slice(&program());
    bytes[0x5200..0x5208].copy_from_slice(&[0xf0, 0, 1, 0x11, 8, 0, 1, 0]);
    bytes[0x5300..0x5302].copy_from_slice(&[0, 254]);
    bytes[0x5320..0x5322].copy_from_slice(&[0, 254]);
    bytes[0x5400..0x5404].copy_from_slice(&[0, 255, 1, 254]);
    bytes[0x5500] = 2;
    bytes[0x5600..0x5602].copy_from_slice(&[0, 2]);
    for group in 0..2 {
        for channel in 0..4 {
            let pointer = 0x5700 + group * 0x40 + channel * 8;
            let at = 0x5602 + group * 8 + channel * 2;
            bytes[at..at + 2].copy_from_slice(&(pointer as u16).to_le_bytes());
            bytes[pointer..pointer + 5].copy_from_slice(&[0x30, 24, 0, 0, 255]);
        }
    }
    bytes[0x14d] = bytes[0x134..=0x14c]
        .iter()
        .fold(0_u8, |v, &b| v.wrapping_sub(b).wrapping_sub(1));
    bytes
}

#[cfg(test)]
fn songs(bytes: &[u8]) -> Vec<super::GbMplaySong> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 4_000_000,
    };
    let mut result = Vec::new();
    super::scan(bytes, &mut result, &mut budget, 100).unwrap();
    result
}

#[test]
fn inventories_loop_and_stop_and_revalidates_source() {
    let mut bytes = synthetic_rom();
    let songs = songs(&bytes);
    assert_eq!(songs.len(), 2);
    assert!(
        songs
            .iter()
            .all(|s| s.tracks.iter().all(|t| t.note_count > 0))
    );
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let prepared = super::prepare_rom(&bytes, &songs[0], &cancel).unwrap();
    assert_eq!(prepared.bytes[0x4000..], bytes[0x4000..]);
    assert_eq!(prepared.bytes[0x204..0x207], [0xcd, 0, 0x30]);
    bytes[0x4000] ^= 1;
    assert!(super::validate_song(&bytes, &songs[0], &cancel).is_err());
}

#[test]
fn rejects_bank_escapes_uninitialized_instruments_and_unsafe_effects() {
    for (address, patch) in [
        (0x5612, &[255][..]),
        (0x5700, &[0x10, 24, 0, 255][..]),
        (0x5700, &[0x75, 24, 0, 0, 0, 255][..]),
        (0x5700, &[0x7f, 24, 0, 1, 0, 255][..]),
        (0x5700, &[0x30, 72, 0, 0, 255][..]),
        (0x5602, &[255, 127][..]),
        (0x5602, &[0, 64][..]),
    ] {
        let mut bytes = synthetic_rom();
        bytes[address..address + patch.len()].copy_from_slice(patch);
        assert!(
            !songs(&bytes).iter().any(|s| s.index == 0),
            "{address:x}: {patch:?}"
        );
    }
}

#[test]
fn rejects_immediate_envelope_cycles_and_generated_waves() {
    for (address, value) in [
        (0x5300, 255),
        (0x5320, 255),
        (0x5201, 128),
        (0x5300, 100),
        (0x5320, 1),
    ] {
        let mut bytes = synthetic_rom();
        bytes[address] = value;
        assert!(songs(&bytes).is_empty(), "{address:x}={value}");
    }
}

#[test]
fn selected_orders_cannot_cross_into_another_selection() {
    let mut bytes = synthetic_rom();
    bytes[0x5401] = 0;
    assert!(!songs(&bytes).iter().any(|s| s.index == 0));
    assert!(songs(&bytes).iter().any(|s| s.index == 1));
}

#[test]
fn instruments_must_reset_active_arpeggio_positions() {
    let mut bytes = synthetic_rom();
    bytes[0x5401] = 254;
    for channel in 0..4 {
        let at = 0x5700 + channel * 8;
        bytes[at..at + 7].copy_from_slice(&[0x20, 0, 0, 0x10, 24, 0, 255]);
    }
    assert!(songs(&bytes).iter().any(|s| s.index == 0));
    bytes[0x5700..0x5708].copy_from_slice(&[0x30, 24, 0, 0, 0x20, 0, 0, 255]);
    assert!(!songs(&bytes).iter().any(|s| s.index == 0));
    for effect in [0, 3] {
        let mut bytes = synthetic_rom();
        for channel in 0..4 {
            let at = 0x5800 + channel * 16;
            let pointer = 0x5602 + channel * 2;
            bytes[pointer..pointer + 2].copy_from_slice(&(at as u16).to_le_bytes());
            bytes[at..at + 10].copy_from_slice(&[0x70 | effect, 24, 0, 0, 0, 0x30, 24, 0, 0, 255]);
        }
        assert_eq!(songs(&bytes).iter().any(|s| s.index == 0), effect == 0);
    }
}

#[test]
fn indirect_tables_preserve_selection_boundaries() {
    static INDIRECT: Profile = Profile {
        layout: OrderLayout::Indirect {
            selectors: [0x5620, 0x5624, 0x5628],
        },
        ..PROFILE
    };
    let mut bytes = synthetic_rom();
    for (address, value) in [
        (0x5620, 0x5400u16),
        (0x5622, 0x5402),
        (0x5624, 0x5602),
        (0x5626, 0x5602),
        (0x5628, 0x5612),
        (0x562a, 0x5612),
    ] {
        bytes[address..address + 2].copy_from_slice(&value.to_le_bytes());
    }
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 4_000_000,
    };
    let recognized = super::profiles::Recognition {
        bank: 1,
        profile: &INDIRECT,
    };
    for index in 0..2 {
        let song = super::sequence::song(&bytes, recognized, index, &mut budget).unwrap();
        assert_eq!(song.order_address, 0x5400 + index * 2);
        assert_eq!(
            song.table_entry.canonical_cpu_address,
            0x5620 + u32::from(index) * 2
        );
    }
    bytes[0x5401] = 0;
    assert!(super::sequence::song(&bytes, recognized, 0, &mut budget).is_err());
    assert!(super::sequence::song(&bytes, recognized, 1, &mut budget).is_ok());
    bytes[0x562a..0x562c].copy_from_slice(&0x5613u16.to_le_bytes());
    assert!(super::sequence::song(&bytes, recognized, 1, &mut budget).is_err());
}

#[test]
fn scan_limits_and_forged_inventory_fail_closed() {
    let bytes = synthetic_rom();
    let song = songs(&bytes).remove(0);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut forged = song;
    forged.mapped_spans.clear();
    assert!(super::prepare_rom(&bytes, &forged, &cancel).is_err());
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 0,
    };
    assert!(super::scan(&bytes, &mut Vec::new(), &mut budget, 100).is_err());
    budget.remaining = 1_000_000;
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 0),
        Err(crate::ScanStop::CandidateLimit)
    );
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(super::scan(&bytes, &mut Vec::new(), &mut budget, 100).is_err());
}
