use super::profiles::Profile;

pub(super) const PROFILE: Profile = Profile {
    name: "channel-authored-timer",
    hash: "",
    index: 4,
    table: 0x1c022,
    header: 0x1c040,
    clear: 0x300,
    init: 0x4100,
    stop: 0x4140,
    select: 0x380,
    shadow: 0xc103,
    lock: 0xc106,
    rearm: 0x6b5,
    owarai: false,
    spans: &[
        (0x50, 3),
        (0x9c, 42),
        (0x300, 0x180),
        (0x6b5, 23),
        (0x8000, 0x40),
        (0x1c000, 0x150),
    ],
};

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x20000];
    bytes[0x143] = 0x80;
    bytes[0x147] = 0x19;
    bytes[0x148] = 2;
    bytes[0x50..0x53].copy_from_slice(&[0xc3, 0x9c, 0]);
    let clear = [
        0xaf, 0xea, 6, 0xc1, 0x21, 0, 0xc2, 0x06, 0, 0x22, 0x05, 0x20, 0xfc, 0xc9,
    ];
    bytes[0x300..0x300 + clear.len()].copy_from_slice(&clear);
    let generator = [
        0x21, 0x80, 0xcb, 0x36, 0x21, 0x23, 0x36, 0x80, 0x23, 0x36, 0xc2, 0x23, 0x36, 0x34, 0x23,
        0x36, 0xc9, 0xc9,
    ];
    bytes[0x8000..0x8000 + generator.len()].copy_from_slice(&generator);
    let init = [
        0x3e, 0xf8, 0xea, 0xcb, 0xc2, 0x3e, 0xde, 0xea, 0xcc, 0xc2, 0x3e, 0x80, 0xe0, 0x26, 0x3e,
        0x77, 0xe0, 0x24, 0x3e, 0x11, 0xe0, 0x25, 0x3e, 0x80, 0xe0, 0x11, 0x3e, 0xf0, 0xe0, 0x12,
        0xc9,
    ];
    bytes[0x1c100..0x1c100 + init.len()].copy_from_slice(&init);
    bytes[0x1c140] = 0xc9;
    let start = [0xea, 0x81, 0xc2, 0xc9];
    bytes[0x380..0x380 + start.len()].copy_from_slice(&start);
    let tick = [
        0xcd, 0x80, 0xcb, 0xfa, 0x80, 0xc2, 0xe0, 0x13, 0x3e, 0x87, 0xe0, 0x14, 0xc9,
    ];
    bytes[0x450..0x450 + tick.len()].copy_from_slice(&tick);
    let irq = [
        0xf5, 0xc5, 0xd5, 0xe5, 0x21, 6, 0xc1, 0xcb, 0xc6, 0xfa, 3, 0xc1, 0xf5, 0x3e, 7, 0xea, 0,
        0x20, 0xea, 3, 0xc1, 0xcd, 0x50, 4, 0xf1, 0xea, 0, 0x20, 0xea, 3, 0xc1, 0x21, 6, 0xc1,
        0xcb, 0x86, 0xe1, 0xd1, 0xc1, 0xf1, 0xd9,
    ];
    bytes[0x9c..0x9c + irq.len()].copy_from_slice(&irq);
    let rearm = [
        0x21, 0x0f, 0xff, 0xcb, 0x96, 0x21, 0xff, 0xff, 0xcb, 0xd6, 0x21, 0xcb, 0xc2, 0x2a, 0xe0,
        5, 0x7e, 0xe0, 6, 0x3e, 4, 0xe0, 7,
    ];
    bytes[0x6b5..0x6cc].copy_from_slice(&rearm);
    bytes[0x1c022..0x1c024].copy_from_slice(&[0x40, 0x40]);
    bytes[0x1c040] = 4;
    bytes[0x14d] = bytes[0x134..0x14d]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_sub(byte).wrapping_sub(1));
    bytes
}

#[cfg(test)]
fn songs(bytes: &[u8]) -> Vec<super::GbChannelSong> {
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
fn source_identity_and_entire_selection_are_required() {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let bytes = synthetic_rom();
    let selected = songs(&bytes).remove(0);
    let prepared = super::prepare_rom(&bytes, &selected, &cancel).unwrap();
    assert_eq!(prepared.bytes[0x200..], bytes[0x200..]);
    assert_eq!(prepared.bytes[0x50..0x53], bytes[0x50..0x53]);
    assert_eq!(prepared.bytes[0x9c..0xc6], bytes[0x9c..0xc6]);
    assert_eq!(
        prepared.timing,
        crate::gb_music::native::GbBankedTiming::Dmg
    );
    for offset in [
        0x143, 0x147, 0x148, 0x149, 0x50, 0x9c, 0x300, 0x380, 0x450, 0x6b5, 0x8000, 0x1c022,
        0x1ffff,
    ] {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert!(songs(&changed).is_empty());
        assert!(super::prepare_rom(&changed, &selected, &cancel).is_err());
    }
    for end in [0, 0x14f, 0x4000, 0x1ffff] {
        assert!(songs(&bytes[..end]).is_empty());
    }
    let mut forged = vec![selected.clone(); 9];
    forged[0].index += 1;
    forged[1].bank += 1;
    forged[2].header_address += 1;
    forged[3].table_entry.effective_offset += 1;
    forged[4].mapped_spans.clear();
    forged[5].title.push('x');
    forged[6].warnings.clear();
    forged[7].profile = "unknown";
    forged[8].mapped_spans[0].canonical_cpu_address += 1;
    for song in forged {
        assert!(super::prepare_rom(&bytes, &song, &cancel).is_err());
    }
}

#[test]
fn limits_stop_before_admission() {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let bytes = synthetic_rom();
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
}
