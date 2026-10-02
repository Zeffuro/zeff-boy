use super::profiles::{Profile, Selection};

pub(super) const PROFILE: Profile = Profile {
    name: "resident-authored-frame",
    hash: "",
    init: 0x300,
    start: 0x380,
    tick: 0x400,
    table: 0x500,
    bank: 1,
    resident_len: 0x20b,
    timer_modulo: None,
    selections: &[
        Selection {
            index: 2,
            header: 0x4000,
            data_len: 0x20,
        },
        Selection {
            index: 8,
            header: 0x4100,
            data_len: 0x20,
        },
    ],
};

pub(super) const TIMER_PROFILE: Profile = Profile {
    name: "resident-authored-timer",
    timer_modulo: Some(0xb8),
    ..PROFILE
};

pub fn synthetic_rom() -> Vec<u8> {
    fixture(false)
}

pub fn synthetic_timer_rom() -> Vec<u8> {
    fixture(true)
}

fn fixture(timer: bool) -> Vec<u8> {
    let mut bytes = vec![0; 0x8000];
    bytes[0x134] = u8::from(timer);
    bytes[0x147] = 1;
    let init = [
        0xaf, 0x21, 0, 0xce, 0x06, 0xd2, 0x22, 0x05, 0x20, 0xfc, 0x3e, 0x80, 0xe0, 0x26, 0x3e,
        0x77, 0xe0, 0x24, 0x3e, 0x11, 0xe0, 0x25, 0x3e, 0x80, 0xe0, 0x11, 0x3e, 0xf0, 0xe0, 0x12,
        0xc9,
    ];
    bytes[0x300..0x300 + init.len()].copy_from_slice(&init);
    let start = [
        0xf5, 0xcd, 0, 3, 0xf1, 0xea, 0, 0xce, 0x21, 0, 5, 0x85, 0x6f, 0x5e, 0x23, 0x56, 0x1a,
        0xea, 2, 0xce, 0xe0, 0x13, 0x3e, 0x87, 0xe0, 0x14, 0xc9,
    ];
    bytes[0x380..0x380 + start.len()].copy_from_slice(&start);
    let tick = [
        0x21, 0x10, 0xce, 0x34, 0x7e, 0xe6, 0x0f, 0x87, 0x87, 0x21, 2, 0xce, 0x86, 0xe0, 0x13,
        0x3e, 0x87, 0xe0, 0x14, 0xc9,
    ];
    bytes[0x400..0x400 + tick.len()].copy_from_slice(&tick);
    bytes[0x502..0x504].copy_from_slice(&[0, 0x40]);
    bytes[0x508..0x50a].copy_from_slice(&[0, 0x41]);
    bytes[0x4000] = 0x40;
    bytes[0x4100] = 0x60;
    bytes[0x14d] = bytes[0x134..0x14d]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_sub(byte).wrapping_sub(1));
    bytes
}

#[cfg(test)]
fn songs(bytes: &[u8]) -> Vec<super::GbResidentSong> {
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
fn sparse_byte_offsets_preserve_source_and_exact_inventory() {
    for bytes in [synthetic_rom(), synthetic_timer_rom()] {
        let found = songs(&bytes);
        assert_eq!(
            found.iter().map(|song| song.index).collect::<Vec<_>>(),
            [2, 8]
        );
        for song in &found {
            assert_eq!(song.bank, 1);
            assert_eq!(
                song.table_entry.effective_offset,
                0x500 + u32::from(song.index)
            );
            assert_eq!(song.table_entry.byte_len, 2);
            let at = song.table_entry.effective_offset as usize;
            assert_eq!(
                u16::from_le_bytes([bytes[at], bytes[at + 1]]),
                song.header_address
            );
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let prepared = super::prepare_rom(&bytes, song, &cancel).unwrap();
            assert_eq!(prepared.bytes[0x300..], bytes[0x300..]);
            assert_eq!(
                prepared.timing,
                crate::gb_music::native::GbBankedTiming::Dmg
            );
            assert_eq!(
                (prepared.ready_address, prepared.ready_value),
                (0xfffc, 0xa5)
            );
            assert_eq!((prepared.ack_address, prepared.ack_value), (0xfffd, 0x5a));
            assert_eq!(
                prepared.bytes[0x200..0x20c],
                [
                    0xf5, 0xc5, 0xd5, 0xe5, 0xcd, 0, 4, 0xe1, 0xd1, 0xc1, 0xf1, 0xd9
                ]
            );
        }
    }
}

#[test]
fn startup_preserves_disabled_interrupts_mapper_and_source_clock_order() {
    for bytes in [synthetic_rom(), synthetic_timer_rom()] {
        let selected = songs(&bytes).remove(0);
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let prepared = super::prepare_rom(&bytes, &selected, &cancel).unwrap();
        let code = &prepared.bytes[0x150..0x200];
        assert!(code.starts_with(&[
            0xf3, 0x31, 0, 0xcf, 0xaf, 0xe0, 0xff, 0xe0, 0x0f, 0xe0, 0x07
        ]));
        assert_eq!(
            code[11..22],
            [0xea, 0, 0x60, 0xea, 0, 0x40, 0x3e, 1, 0xea, 0, 0x20]
        );
        assert_eq!(code[22..25], [0xcd, 0, 3]);
        assert_eq!(
            prepared.bytes[prepared.wait_start as usize..prepared.wait_end as usize],
            [0xf0, 0xfd, 0xfe, 0x5a, 0x20, 0xfa]
        );
        let after_ack = &prepared.bytes[prepared.wait_end as usize..0x200];
        assert!(after_ack.starts_with(&[0x3e, 2, 0xcd, 0x80, 3]));
        if selected.timer_modulo.is_some() {
            assert_eq!(prepared.bytes[0x50..0x53], [0xc3, 0, 2]);
            assert_eq!(
                after_ack[5..21],
                [
                    0xaf, 0xe0, 4, 0x3e, 0xb8, 0xe0, 5, 0xe0, 6, 0xaf, 0xe0, 7, 0x3e, 4, 0xe0, 7
                ]
            );
            assert_eq!(
                after_ack[21..32],
                [
                    0xaf, 0xe0, 0x0f, 0x3e, 4, 0xe0, 0xff, 0xfb, 0x76, 0x18, 0xfd
                ]
            );
        } else {
            assert_eq!(prepared.bytes[0x40..0x43], [0xc3, 0, 2]);
            assert_eq!(
                after_ack[5..16],
                [
                    0xaf, 0xe0, 0x0f, 0x3e, 1, 0xe0, 0xff, 0xfb, 0x76, 0x18, 0xfd
                ]
            );
        }
    }
}

#[test]
fn source_authentication_rejects_changed_code_tables_and_unvisited_data() {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    for bytes in [synthetic_rom(), synthetic_timer_rom()] {
        let selected = songs(&bytes).remove(0);
        for offset in [
            0x134, 0x143, 0x147, 0x148, 0x300, 0x380, 0x400, 0x502, 0x508, 0x4000, 0x411f, 0x7fff,
        ] {
            let mut altered = bytes.clone();
            altered[offset] ^= 1;
            assert!(songs(&altered).is_empty());
            assert!(super::prepare_rom(&altered, &selected, &cancel).is_err());
        }
        for end in [0, 0x14f, 0x4000, 0x7fff] {
            assert!(songs(&bytes[..end]).is_empty());
        }
        assert!(songs(&bytes.repeat(2)).is_empty());
    }
}

#[test]
fn forged_selection_metadata_and_limits_are_refused() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let bytes = synthetic_rom();
    let selected = songs(&bytes).remove(0);
    let cancel = AtomicBool::new(false);
    let mut variants = vec![selected.clone(); 9];
    variants[0].index = 0;
    variants[1].index = 3;
    variants[2].bank = 2;
    variants[3].header_address += 1;
    variants[4].timer_modulo = Some(0xb8);
    variants[5].table_entry.byte_len += 1;
    variants[6].mapped_spans.clear();
    variants[7].title.push('x');
    variants[8].warnings.clear();
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
    let mut limited = Vec::new();
    assert_eq!(
        super::scan(&bytes, &mut limited, &mut budget, 1),
        Err(crate::ScanStop::CandidateLimit)
    );
    assert_eq!(limited, vec![selected.clone()]);
    budget.remaining = 4_000_000;
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 0),
        Err(crate::ScanStop::CandidateLimit)
    );
    cancel.store(true, Ordering::Relaxed);
    assert!(super::prepare_rom(&bytes, &selected, &cancel).is_err());
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 100),
        Err(crate::ScanStop::Cancelled)
    );
}
