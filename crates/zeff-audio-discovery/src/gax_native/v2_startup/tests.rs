use std::sync::atomic::AtomicBool;

use super::*;

fn words(bytes: &mut [u8], at: usize, values: &[u32]) {
    for (index, value) in values.iter().enumerate() {
        bytes[at + index * 4..at + index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
}

fn halves(bytes: &mut [u8], at: usize, values: &[u16]) {
    for (index, value) in values.iter().enumerate() {
        bytes[at + index * 2..at + index * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
}

fn thumb_bl(bytes: &mut [u8], at: usize, target: usize) {
    let relative = (target as i32 - at as i32 - 4) as u32;
    let high = 0xf000u16 | ((relative >> 12) & 0x7ff) as u16;
    let low = 0xf800u16 | ((relative >> 1) & 0x7ff) as u16;
    halves(bytes, at, &[high, low]);
}

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x4000];
    words(
        &mut bytes,
        0xc0,
        &[
            0xe3a0_0012,
            0xe129_f000,
            0xe59f_d140,
            0xe3a0_001f,
            0xe129_f000,
            0xe59f_d138,
        ],
    );
    words(&mut bytes, 0x210, &[0x0300_7fa0, 0x0300_7e00]);
    words(
        &mut bytes,
        0xe8,
        &[
            0xe3a0_c301,
            0xe3a0_0014,
            0xe380_0901,
            0xe3a0_1f81,
            0xe18c_00b1,
            0xe28c_c0d4,
            0xe59f_0118,
            0xe59f_1118,
            0xe59f_3118,
            0xe043_3001,
            0xe3a0_2321,
            0xe182_2143,
            0xe88c_0007,
            0xe080_0003,
            0xe59f_1104,
            0xe59f_3104,
            0xe043_3001,
            0xe3a0_2321,
            0xe182_2143,
            0xe88c_0007,
        ],
    );
    words(
        &mut bytes,
        0x220,
        &[
            0x0800_3000,
            0x0300_0000,
            0x0300_0100,
            0x0200_0000,
            0x0200_0100,
        ],
    );
    let signature = crate::gax_native::INIT_SIGNATURES[2].bytes;
    bytes[0x1000..0x1000 + signature.len()].copy_from_slice(signature);
    for offset in [0x2bc, 0x31c, 0x58c] {
        let at = 0x1000 + offset;
        thumb_bl(&mut bytes, at, 0x400);
    }
    words(&mut bytes, 0x400, &[0x4710_4a01, 0, 0x0300_0020]);
    words(
        &mut bytes,
        0x3020,
        &[0xe211_3102, 0x4261_1000, 0xe033_c040, 0x2260_0000],
    );
    bytes
}

fn rom_division_fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x4000];
    let signature = crate::gax_native::INIT_SIGNATURES[2].bytes;
    bytes[0x1000..0x1000 + signature.len()].copy_from_slice(signature);
    halves(
        &mut bytes,
        0x3000,
        &[
            0x2900, 0xd041, 0xb410, 0x1c04, 0x404c, 0x46a4, 0x2301, 0x2200, 0x2900, 0xd500, 0x4249,
            0x2800, 0xd500, 0x4240, 0x4288, 0xd32c,
        ],
    );
    halves(
        &mut bytes,
        0x3100,
        &[
            0x2900, 0xd034, 0x2301, 0x2200, 0xb410, 0x4288, 0xd32c, 0x2401, 0x0724, 0x42a1, 0xd204,
            0x4281, 0xd202, 0x0109, 0x011b, 0xe7f8,
        ],
    );
    for (offset, target) in [(0x2f8, 0x3000), (0x35c, 0x3000), (0x5d0, 0x3100)] {
        thumb_bl(&mut bytes, 0x1000 + offset, target);
    }
    bytes
}

fn inspected(bytes: &[u8]) -> Option<startup::Setup> {
    inspect(
        bytes,
        GaxNativeEntry {
            source: RomSpan::new(0x1000, 20),
            cpu_address: 0x0800_1001,
        },
        &mut Budget {
            cancel: &AtomicBool::new(false),
            remaining: 1_000_000,
        },
    )
    .unwrap()
}

#[test]
fn division_calls_require_their_original_bounded_startup_code() {
    let bytes = fixture();
    let setup = inspected(&bytes).unwrap();
    assert_eq!(setup.copies.len(), 2);
    assert_eq!(setup.copies[0].source, RomSpan::new(0x3000, 0x100));
    assert!(
        setup
            .spans
            .iter()
            .all(|span| span.effective_offset + span.byte_len <= bytes.len() as u32)
    );
    for at in [0xc0, 0x100, 0x408, 0x12bc, 0x131c, 0x158c, 0x3020] {
        let mut changed = bytes.clone();
        words(&mut changed, at, &[0]);
        assert!(
            inspected(&changed).is_none(),
            "accepted missing witness at {at:x}"
        );
    }
    let mut direct = bytes;
    words(&mut direct, 0x400, &[0x46f7_df06]);
    words(&mut direct, 0xc0, &[0]);
    assert!(inspected(&direct).unwrap().copies.is_empty());
}

#[test]
fn rom_division_layout_requires_three_ordered_original_bodies() {
    let bytes = rom_division_fixture();
    let setup = inspected(&bytes).unwrap();
    assert!(setup.copies.is_empty());
    assert_eq!(
        setup.spans,
        vec![
            RomSpan::new(0x12f8, 4),
            RomSpan::new(0x3000, 32),
            RomSpan::new(0x135c, 4),
            RomSpan::new(0x3000, 32),
            RomSpan::new(0x15d0, 4),
            RomSpan::new(0x3100, 32),
        ]
    );
    for at in [0x12f8, 0x135c, 0x15d0, 0x3004, 0x3104] {
        let mut changed = bytes.clone();
        halves(&mut changed, at, &[0]);
        assert!(
            inspected(&changed).is_none(),
            "accepted a damaged ROM-only division witness at {at:x}"
        );
    }
}
