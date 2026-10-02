use super::profiles::{Profile, Selection};

pub(super) const PROFILE: Profile = Profile {
    name: "timed-authored-frame",
    hash: "",
    init: 0x300,
    stop: 0x340,
    start: 0x380,
    tick: 0x400,
    second_tick: 0x480,
    shadow: 0xce20,
    selections: &[
        Selection {
            index: 4,
            bank: 1,
            address: 0x4000,
            caller_bank: 0,
            caller: 0x500,
            spans: &[(0, 0x300, 0x188), (1, 0x4000, 0x118)],
        },
        Selection {
            index: 9,
            bank: 1,
            address: 0x4100,
            caller_bank: 0,
            caller: 0x508,
            spans: &[(0, 0x300, 0x188), (1, 0x4000, 0x118)],
        },
    ],
};

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x8000];
    bytes[0x143] = 0x80;
    bytes[0x147] = 0x19;
    let init = [
        0xaf, 0x21, 0, 0xce, 0x06, 0x20, 0x22, 0x05, 0x20, 0xfc, 0x3e, 0x80, 0xe0, 0x26, 0x3e,
        0x77, 0xe0, 0x24, 0x3e, 0x11, 0xe0, 0x25, 0x3e, 0x80, 0xe0, 0x11, 0x3e, 0xf0, 0xe0, 0x12,
        0xc9,
    ];
    bytes[0x300..0x300 + init.len()].copy_from_slice(&init);
    bytes[0x340] = 0xc9;
    let start = [
        0xea, 0x20, 0xce, 0xea, 0, 0x20, 0x7b, 0xea, 3, 0xce, 0x7a, 0xea, 4, 0xce, 0xaf, 0xea, 1,
        0xce, 0xea, 2, 0xce, 0xc9,
    ];
    bytes[0x380..0x380 + start.len()].copy_from_slice(&start);
    let tick = TICK;
    bytes[0x400..0x400 + tick.len()].copy_from_slice(tick);
    bytes[0x480] = 0xc9;
    bytes[0x500..0x508].copy_from_slice(&[0x3e, 1, 0x11, 0, 0x40, 0xcd, 0x80, 3]);
    bytes[0x508..0x510].copy_from_slice(&[0x3e, 1, 0x11, 0, 0x41, 0xcd, 0x80, 3]);
    let stream = [
        0, 0, 0x10, 0x40, 0x87, 1, 0, 0x40, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe,
        0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10, 1, 2, 0x10, 0x60, 0x87, 1, 5, 0,
    ];
    bytes[0x4000..0x4000 + stream.len()].copy_from_slice(&stream);
    bytes[0x4100..0x410a].copy_from_slice(&[0, 0, 0x10, 0x20, 0x87, 0, 3, 0x10, 0x50, 0x87]);
    bytes[0x410a..0x410d].copy_from_slice(&[0, 6, 0]);
    bytes[0x14d] = bytes[0x134..0x14d]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_sub(byte).wrapping_sub(1));
    bytes
}

const TICK: &[u8] = &[
    0xfa, 0x1, 0xce, 0x47, 0xfa, 0x2, 0xce, 0x4f, 0xfa, 0x3, 0xce, 0x6f, 0xfa, 0x4, 0xce, 0x67,
    0x2a, 0x90, 0xda, 0x23, 0x4, 0xc2, 0x64, 0x4, 0x2a, 0x91, 0xda, 0x24, 0x4, 0xca, 0x24, 0x4,
    0xc3, 0x64, 0x4, 0x23, 0x2a, 0xfe, 0x10, 0xc2, 0x33, 0x4, 0x2a, 0xe0, 0x13, 0x2a, 0xe0, 0x14,
    0xc3, 0x59, 0x4, 0xfe, 0x40, 0xc2, 0x4c, 0x4, 0x11, 0x30, 0xff, 0xc5, 0x6, 0x10, 0xaf, 0xe0,
    0x1a, 0x2a, 0x12, 0x13, 0x5, 0xc2, 0x41, 0x4, 0xc1, 0xc3, 0x59, 0x4, 0xaf, 0xea, 0x1, 0xce,
    0xea, 0x2, 0xce, 0x1, 0x0, 0x0, 0x21, 0x0, 0x40, 0x7d, 0xea, 0x3, 0xce, 0x7c, 0xea, 0x4, 0xce,
    0xc3, 0x10, 0x4, 0x3, 0x78, 0xea, 0x1, 0xce, 0x79, 0xea, 0x2, 0xce, 0xc9,
];

#[cfg(test)]
fn songs(bytes: &[u8]) -> Vec<super::GbTimedSong> {
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
fn sparse_original_callers_and_wrapper_are_preserved() {
    let bytes = synthetic_rom();
    let found = songs(&bytes);
    assert_eq!(
        found.iter().map(|song| song.index).collect::<Vec<_>>(),
        [4, 9]
    );
    for song in &found {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let prepared = super::prepare_rom(&bytes, song, &cancel).unwrap();
        assert_eq!(
            prepared.timing,
            crate::gb_music::native::GbBankedTiming::Dmg
        );
        assert_eq!(
            &prepared.bytes[0x200..0x20f],
            &[
                0xf5, 0xc5, 0xd5, 0xe5, 0xcd, 0, 4, 0xcd, 0x80, 4, 0xe1, 0xd1, 0xc1, 0xf1, 0xd9
            ]
        );
        assert_eq!(prepared.bytes[0x300..], bytes[0x300..]);
        let caller = song.table_entry.effective_offset as usize;
        assert_eq!(
            &bytes[caller..caller + 8],
            &[
                0x3e,
                1,
                0x11,
                0,
                (song.header_address >> 8) as u8,
                0xcd,
                0x80,
                3
            ]
        );
    }
}

#[test]
fn full_source_and_complete_metadata_are_authenticated() {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let bytes = synthetic_rom();
    let selected = songs(&bytes).remove(0);
    for offset in [
        0x143, 0x147, 0x148, 0x149, 0x300, 0x340, 0x380, 0x400, 0x480, 0x500, 0x4000, 0x7fff,
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
    forged[1].bank = 2;
    forged[2].header_address += 1;
    forged[3].table_entry.effective_offset += 1;
    forged[4].mapped_spans.clear();
    forged[5].title.push('x');
    forged[6].warnings.clear();
    forged[7].profile = "unknown";
    for song in forged {
        assert!(super::prepare_rom(&bytes, &song, &cancel).is_err());
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
