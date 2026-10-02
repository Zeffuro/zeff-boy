use super::profiles::Profile;

pub(super) const PROFILE: Profile = Profile {
    name: "cache-authored-startup",
    hash: "",
    index: 4,
    bank: 1,
    api: 0x4000,
    caller: 0x1a2,
    entry: 0x150,
    vblank: 0x300,
    reserve: 0x3f00,
    flag: 0xdffe,
    spans: &[(0x40, 3), (0x100, 0xa8), (0x300, 12), (0x4000, 0xa0)],
};

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x8000];
    bytes[0x147] = 1;
    bytes[0x100..0x104].copy_from_slice(&[0, 0xc3, 0x50, 1]);
    bytes[0x40..0x43].copy_from_slice(&[0xc3, 0, 3]);
    let startup = [
        0xf3, 0x31, 0, 0xdf, 0xaf, 0x21, 0, 0xc1, 0x06, 0x10, 0x22, 0x05, 0x20, 0xfc, 0x3e, 0x37,
        0xea, 6, 0xc1, 0x3e, 1, 0xea, 0, 0x20, 0x3e, 0x91, 0xe0, 0x40, 0xaf, 0xe0, 0x0f, 0x3e, 1,
        0xe0, 0xff, 0xfb, 0xfa, 0x0f, 0xc1, 0xfe, 3, 0x38, 0xf9, 0xc3, 0xa0, 1,
    ];
    bytes[0x150..0x150 + startup.len()].copy_from_slice(&startup);
    bytes[0x1a0..0x1a8].copy_from_slice(&[0x3e, 4, 0xcd, 0, 0x40, 0x76, 0x18, 0xfd]);
    bytes[0x300..0x30c].copy_from_slice(&[
        0xf5, 0xe5, 0x21, 0x0f, 0xc1, 0x34, 0xcd, 3, 0x40, 0xe1, 0xf1, 0xd9,
    ]);
    bytes[0x4000..0x4006].copy_from_slice(&[0xc3, 0x20, 0x40, 0xc3, 0x80, 0x40]);
    let select = [
        0xea, 1, 0xc1, 0x3e, 1, 0xea, 0, 0xc1, 0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x77, 0xe0, 0x24,
        0x3e, 0x11, 0xe0, 0x25, 0x3e, 0x80, 0xe0, 0x11, 0x3e, 0xf0, 0xe0, 0x12, 0xfa, 6, 0xc1,
        0xe0, 0x13, 0x3e, 0x87, 0xe0, 0x14, 0xc9,
    ];
    bytes[0x4020..0x4020 + select.len()].copy_from_slice(&select);
    let tick = [
        0xfa, 0, 0xc1, 0xb7, 0xc8, 0xfa, 2, 0xc1, 0x3c, 0xea, 2, 0xc1, 0xe6, 0x1f, 0x21, 6, 0xc1,
        0x86, 0xe0, 0x13, 0x3e, 0x87, 0xe0, 0x14, 0xc9,
    ];
    bytes[0x4080..0x4080 + tick.len()].copy_from_slice(&tick);
    bytes[0x14d] = bytes[0x134..0x14d]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_sub(byte).wrapping_sub(1));
    bytes
}

#[cfg(test)]
fn songs(bytes: &[u8]) -> Vec<super::GbCacheSong> {
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
fn original_startup_and_native_services_are_preserved() {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let bytes = synthetic_rom();
    let selected = songs(&bytes).remove(0);
    let prepared = super::prepare_rom(&bytes, &selected, &cancel).unwrap();
    assert_eq!(prepared.wait_start, 0x3f1c);
    assert_eq!(prepared.wait_end, 0x3f22);
    assert_eq!(
        prepared.timing,
        crate::gb_music::native::GbBankedTiming::Dmg
    );
    assert_eq!(prepared.bytes[0x1a2..0x1a5], [0xc3, 9, 0x3f]);
    assert_eq!(prepared.bytes[0x40..0x43], [0xc3, 0x3b, 0x3f]);
    for (offset, (&source, &output)) in bytes.iter().zip(&prepared.bytes).enumerate() {
        if ![
            (0x40..0x43),
            (0x100..0x103),
            (0x1a2..0x1a5),
            (0x3f00..0x3f54),
        ]
        .iter()
        .any(|range| range.contains(&offset))
        {
            assert_eq!(source, output, "offset {offset:04x}");
        }
    }
}

#[test]
fn complete_source_identity_and_metadata_are_required() {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let bytes = synthetic_rom();
    let selected = songs(&bytes).remove(0);
    for offset in [
        0x40, 0x100, 0x143, 0x147, 0x148, 0x149, 0x150, 0x1a2, 0x300, 0x3f00, 0x4000, 0x7fff,
    ] {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert!(songs(&changed).is_empty());
        assert!(super::prepare_rom(&changed, &selected, &cancel).is_err());
    }
    for end in [0, 0x14f, 0x4000, 0x7fff] {
        assert!(songs(&bytes[..end]).is_empty());
    }
    let mut forged = vec![selected.clone(); 11];
    forged[0].index += 1;
    forged[1].bank += 1;
    forged[2].header_address += 1;
    forged[3].table_entry.effective_offset += 1;
    forged[4].table_entry.byte_len += 1;
    forged[5].mapped_spans.clear();
    forged[6].title.push('x');
    forged[7].warnings.clear();
    forged[8].profile = "unknown";
    forged[9].mapped_spans[0].canonical_cpu_address += 1;
    forged[10].table_entry.canonical_cpu_address += 1;
    for song in forged {
        assert!(super::prepare_rom(&bytes, &song, &cancel).is_err());
    }
}

#[test]
fn cancellation_and_limits_prevent_admission_and_preparation() {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let bytes = synthetic_rom();
    let selected = songs(&bytes).remove(0);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 0,
    };
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 1),
        Err(crate::ScanStop::WorkLimit)
    );
    budget.remaining = 4_000_000;
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 0),
        Err(crate::ScanStop::CandidateLimit)
    );
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 1),
        Err(crate::ScanStop::Cancelled)
    );
    assert!(super::prepare_rom(&bytes, &selected, &cancel).is_err());
}
