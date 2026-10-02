use super::profiles::{Profile, Selection};

const BASE: Profile = Profile {
    name: "timer-authored-mbc1",
    hash: "",
    init: 0x300,
    stop: 0x380,
    select: 0x384,
    tick: 0x3f2,
    shadow: 0xa9,
    compressed: false,
    selections: &[
        Selection {
            index: 1,
            entry: 0x502,
            module: 0x4000,
            spans: &[(0x300, 0x220), (0x4000, 0x20)],
        },
        Selection {
            index: 7,
            entry: 0x50e,
            module: 0x4800,
            spans: &[(0x300, 0x220), (0x4800, 0x20)],
        },
    ],
};

pub(super) const PROFILES: [Profile; 2] = [
    BASE,
    Profile {
        name: "timer-authored-mbc2",
        shadow: 0xae,
        ..BASE
    },
];

pub fn synthetic_rom() -> Vec<u8> {
    fixture(3)
}
pub fn synthetic_mbc2_rom() -> Vec<u8> {
    fixture(6)
}

fn fixture(mapper: u8) -> Vec<u8> {
    let mut bytes = vec![0; 0x8000];
    bytes[0x147] = mapper;
    let init = [
        0xaf, 0xea, 0, 0xce, 0xe0, 7, 0x3e, 0x2a, 0xe0, 6, 0xe0, 5, 0x3e, 4, 0xe0, 7, 0x3e, 0x80,
        0xe0, 0x26, 0x3e, 0x77, 0xe0, 0x24, 0x3e, 0x11, 0xe0, 0x25, 0x3e, 0x80, 0xe0, 0x11, 0x3e,
        0xf0, 0xe0, 0x12, 0xc9,
    ];
    bytes[0x300..0x300 + init.len()].copy_from_slice(&init);
    bytes[0x380] = 0xc9;
    let select = [
        0xea, 1, 0xce, 0x87, 0x21, 0, 5, 0x85, 0x6f, 0x5e, 0x23, 0x56, 0x1a, 0xe0, 0x13, 0x3e,
        0x87, 0xe0, 0x14, 0xc9,
    ];
    bytes[0x384..0x384 + select.len()].copy_from_slice(&select);
    let tick = [
        0x21, 0, 0xce, 0x34, 0x7e, 0xe6, 0x0f, 0xc6, 0x40, 0xe0, 0x13, 0x3e, 0x87, 0xe0, 0x14,
        0x7e, 0xe6, 0x0f, 0xc6, 0xe0, 0xe0, 6, 0xe0, 5, 0xc9,
    ];
    bytes[0x3f2..0x3f2 + tick.len()].copy_from_slice(&tick);
    bytes[0x502..0x504].copy_from_slice(&[0, 0x40]);
    bytes[0x50e..0x510].copy_from_slice(&[0, 0x48]);
    bytes[0x4000] = 0x40;
    bytes[0x4800] = 0x60;
    bytes[0x14d] = bytes[0x134..0x14d]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_sub(byte).wrapping_sub(1));
    bytes
}

#[cfg(test)]
fn songs(bytes: &[u8]) -> Vec<super::GbTimerSong> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 4_000_000,
    };
    let mut found = Vec::new();
    super::scan(bytes, &mut found, &mut budget, 100).unwrap();
    found
}

#[test]
fn sparse_domains_preserve_code_mapping_and_timer_isr() {
    for bytes in [synthetic_rom(), synthetic_mbc2_rom()] {
        let found = songs(&bytes);
        assert_eq!(
            found.iter().map(|song| song.index).collect::<Vec<_>>(),
            [1, 7]
        );
        for song in &found {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let prepared = super::prepare_rom(&bytes, song, &cancel).unwrap();
            assert_eq!(prepared.bytes[0x300..], bytes[0x300..]);
            assert_eq!(prepared.bytes[0x50..0x53], [0xc3, 0, 2]);
            assert_eq!(
                prepared.bytes[0x200..0x20c],
                [
                    0xf5, 0xc5, 0xd5, 0xe5, 0xcd, 0xf2, 3, 0xe1, 0xd1, 0xc1, 0xf1, 0xd9
                ]
            );
            let code = &prepared.bytes[0x150..prepared.wait_start as usize];
            assert!(
                code.starts_with(&[0xf3, 0x31, 0, 0xcf, 0xaf, 0xe0, 0xff, 0xe0, 0x0f, 0xe0, 7])
            );
            assert!(
                code.windows(5)
                    .any(|window| window == [0x3e, 1, 0xea, 0, 0x21])
            );
            let after = &prepared.bytes[prepared.wait_end as usize..0x200];
            assert!(after.starts_with(&[
                0xcd,
                0,
                3,
                0xcd,
                0x80,
                3,
                0x3e,
                song.index as u8,
                0xcd,
                0x84,
                3
            ]));
            assert_eq!(
                &after[11..22],
                &[
                    0xaf, 0xe0, 0x0f, 0x3e, 4, 0xe0, 0xff, 0xfb, 0x76, 0x18, 0xfd
                ]
            );
            assert_eq!(
                song.table_entry.effective_offset,
                0x500 + 2 * u32::from(song.index)
            );
            assert_eq!(
                song.mapped_spans[1].canonical_cpu_address,
                song.module_offset
            );
        }
    }
}

#[test]
fn whole_source_and_all_metadata_are_authenticated() {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    for bytes in [synthetic_rom(), synthetic_mbc2_rom()] {
        let selected = songs(&bytes).remove(0);
        for offset in [
            0x143, 0x147, 0x148, 0x149, 0x300, 0x384, 0x3f2, 0x502, 0x50e, 0x4000, 0x481f, 0x7fff,
        ] {
            let mut changed = bytes.clone();
            changed[offset] ^= 1;
            assert!(songs(&changed).is_empty());
            assert!(super::prepare_rom(&changed, &selected, &cancel).is_err());
        }
        for end in [0, 0x14f, 0x4000, 0x7fff] {
            assert!(songs(&bytes[..end]).is_empty());
        }
        let mut forged = vec![selected.clone(); 8];
        forged[0].index = 0;
        forged[1].index = 2;
        forged[2].compressed = true;
        forged[3].module_offset += 1;
        forged[4].table_entry.byte_len += 1;
        forged[5].mapped_spans.clear();
        forged[6].title.push('x');
        forged[7].warnings.clear();
        for song in forged {
            assert!(super::prepare_rom(&bytes, &song, &cancel).is_err());
        }
    }
}

#[test]
fn work_candidate_and_cancellation_limits_are_enforced() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let bytes = synthetic_rom();
    let cancel = AtomicBool::new(false);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 0,
    };
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 100),
        Err(crate::ScanStop::WorkLimit)
    );
    budget.remaining = 4_000_000;
    let mut found = Vec::new();
    assert_eq!(
        super::scan(&bytes, &mut found, &mut budget, 1),
        Err(crate::ScanStop::CandidateLimit)
    );
    assert_eq!(found.len(), 1);
    cancel.store(true, Ordering::Relaxed);
    assert!(super::prepare_rom(&bytes, &found[0], &cancel).is_err());
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 100),
        Err(crate::ScanStop::Cancelled)
    );
}
