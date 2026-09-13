use std::sync::atomic::AtomicBool;

use super::super::GaxNativeLayout;
use super::*;

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn words(bytes: &mut [u8], at: usize, values: &[u32]) {
    for (i, &value) in values.iter().enumerate() {
        put32(bytes, at + i * 4, value);
    }
}
fn halves(bytes: &mut [u8], at: usize, values: &[u16]) {
    for (i, &value) in values.iter().enumerate() {
        put16(bytes, at + i * 2, value);
    }
}
fn call(bytes: &mut [u8], at: usize, target: usize) {
    let relative = (target as i32 - at as i32 - 4) as u32;
    put16(bytes, at, 0xf000 | ((relative >> 12) & 0x7ff) as u16);
    put16(bytes, at + 2, 0xf800 | ((relative >> 1) & 0x7ff) as u16);
}

fn fixture(modern: bool, copy_runtime: bool) -> Vec<u8> {
    let mut bytes = vec![0; 0x6000];
    put32(&mut bytes, 0, 0xea00_002e);
    bytes[0xb2] = 0x96;
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
    put32(&mut bytes, 0x210, 0x0300_7fa0);
    put32(&mut bytes, 0x214, 0x0300_7e00);
    halves(
        &mut bytes,
        0x800,
        &[
            0xb5f0, 0x4647, 0xb480, 0xb081, 0x1c06, 0x2e00, 0xd108, 0x4802, 0x4902,
        ],
    );
    call(&mut bytes, 0x812, 0x400);
    let size = if modern { 0x40 } else { 0x3c };
    halves(
        &mut bytes,
        0x820,
        &[
            0x1c34,
            0x2700 | size,
            0x2003,
            0x4030,
            0x2100 | (size - 4),
            0x1989,
            0x4688,
        ],
    );
    halves(
        &mut bytes,
        0x880,
        &[
            0x8130, 0x2001, 0x4240, 0x8170, 0x2000, 0x81b0, 0x3801, 0x8230, 0x8270, 0x2001, 0x4641,
            0x7008,
        ],
    );
    let mut init = profiles::INIT.to_vec();
    if modern {
        for i in [11, 17, 18, 24] {
            init[i] += 0x40;
        }
    }
    halves(&mut bytes, 0x1000, &init);
    put32(&mut bytes, 0x104c, 0x0200_1000);
    halves(&mut bytes, 0x2200, profiles::IRQ);
    put32(&mut bytes, 0x22f0, 0x0200_1000);
    let mut play = profiles::PLAY.to_vec();
    if modern {
        play[9] = 0x6b80;
    }
    halves(&mut bytes, 0x2400, &play);
    put32(&mut bytes, 0x2524, 0x0200_1000);
    call(&mut bytes, 0x13da, 0x400);
    call(&mut bytes, 0x16ec, 0x410);
    halves(&mut bytes, 0x400, &[0xdf06, 0x46f7]);
    halves(&mut bytes, 0x410, &[0xdf06, 0x46f7]);
    if copy_runtime {
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
            ],
        );
        words(
            &mut bytes,
            0x100,
            &[
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
        halves(&mut bytes, 0x400, &[0x4a01, 0x4710]);
        put32(&mut bytes, 0x408, 0x0300_0020);
        halves(&mut bytes, 0x410, &[0x2300, 0x469c, 0x4a01, 0x4710]);
        put32(&mut bytes, 0x41c, 0x0300_0030);
        words(
            &mut bytes,
            0x3020,
            &[0xe211_3102, 0x4261_1000, 0xe033_c040, 0x2260_0000],
        );
        words(
            &mut bytes,
            0x3030,
            &[0xe071_2fa0, 0x20c0_0f81, 0xe0a3_3003, 0xe071_2f20],
        );
    }
    halves(&mut bytes, 0x4000, &[1, 1, 1, 0, 256]);
    words(&mut bytes, 0x400c, &[0x0800_4800, 0x0800_4600, 0x0800_4700]);
    put16(&mut bytes, 0x4018, 22050);
    bytes[0x4800..0x4805].copy_from_slice(&[0, 60, 1, 0, 0]);
    let title = b"\"synthetic\" \xa9 test";
    bytes[0x4805..0x4805 + title.len()].copy_from_slice(title);
    let order = (0x4805 + title.len() + 3) & !3;
    put32(&mut bytes, 0x4020, 0x0800_0000 + order as u32);
    put32(&mut bytes, 0x4604, 0x0800_4640);
    bytes[0x4641] = 1;
    words(&mut bytes, 0x4708, &[0x0800_5000, 32]);
    bytes[0x5000..0x5020].fill(64);
    let version = b"GAX Sound Engine 3.05 (synthetic) \xa9 test\0";
    bytes[0x5800..0x5800 + version.len()].copy_from_slice(version);
    bytes
}

fn discover(bytes: &[u8]) -> Vec<GaxNativeSong> {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 8_000_000,
    };
    let mut songs = Vec::new();
    scan(bytes, &mut songs, &mut budget, 8, MAX_RETAINED_BYTES).unwrap();
    songs
}

#[test]
fn direct_headers_preserve_both_parameter_layouts() {
    for modern in [false, true] {
        let bytes = fixture(modern, false);
        let songs = discover(&bytes);
        assert_eq!(songs.len(), 1);
        assert_eq!(
            songs[0].native.layout,
            if modern {
                GaxNativeLayout::V3Modern
            } else {
                GaxNativeLayout::V3Legacy
            }
        );
        assert_eq!(songs[0].title, "synthetic");
        let prepared = driver::build(&bytes, &songs[0]).unwrap();
        let field: u32 = if modern { 0x34 } else { 0x30 };
        assert!(
            prepared[bytes.len()..]
                .windows(4)
                .any(|word| word == (0xe584_0000 | field).to_le_bytes())
        );
        assert!(prepared[4..bytes.len()] == bytes[4..]);
    }
}

#[test]
fn ram_division_requires_matching_crt_copy_and_safe_workspace() {
    let mut bytes = fixture(true, true);
    let songs = discover(&bytes);
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].native.ram_copies.len(), 2);
    assert_eq!(
        driver::workspace(&songs[0].native),
        Some((0x0300_0100, 0x7d00))
    );
    put32(&mut bytes, 0x3020, 0);
    assert!(discover(&bytes).is_empty());
    let mut bytes = fixture(true, true);
    put32(&mut bytes, 0x22c, 0x0300_0080);
    put32(&mut bytes, 0x230, 0x0300_0180);
    assert!(discover(&bytes).is_empty());
}

#[test]
fn workspace_uses_available_iwram_without_crossing_reserved_stacks() {
    let bytes = fixture(true, true);
    let mut native = discover(&bytes).remove(0).native;
    native.work_ram = 0x0300_0a3c;
    assert_eq!(driver::workspace(&native), Some((0x0300_0ad8, 0x7328)));
    native.work_ram = 0x0300_5d64;
    assert_eq!(driver::workspace(&native), Some((0x0300_5e00, 0x2000)));
    native.work_ram += 4;
    assert!(driver::workspace(&native).is_none());
    native.work_ram = 0x0200_1000;
    native.ram_copies[0].source.byte_len = 0x5e00;
    assert_eq!(driver::workspace(&native), Some((0x0300_5e00, 0x2000)));
    native.ram_copies[0].source.byte_len += 4;
    assert!(driver::workspace(&native).is_none());
}

#[test]
fn shared_state_literal_and_complete_song_ranges_are_required() {
    let mut bytes = fixture(false, false);
    put32(&mut bytes, 0x2524, 0x0200_2000);
    assert!(discover(&bytes).is_empty());
    let mut bytes = fixture(false, false);
    put32(&mut bytes, 0x470c, 0x2000);
    assert!(discover(&bytes).is_empty());
}

#[test]
fn native_instruments_allow_legacy_inline_fields() {
    let mut bytes = fixture(false, false);
    bytes[0x4651] = 12;
    put32(&mut bytes, 0x4654, 0x2121);
    assert_eq!(discover(&bytes).len(), 1);
}

#[test]
fn full_inventory_and_cancellation_stop_before_retention() {
    let bytes = fixture(false, false);
    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 8_000_000,
    };
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut budget, 8, MAX_RETAINED_BYTES),
        Err(ScanStop::Cancelled)
    );
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 8_000_000,
    };
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut budget, 0, MAX_RETAINED_BYTES),
        Err(ScanStop::CandidateLimit)
    );
    assert!(budget.remaining < 8_000_000);
    budget.remaining = 8_000_000;
    let mut limited = Vec::new();
    assert_eq!(
        scan(&bytes, &mut limited, &mut budget, 8, 1),
        Err(ScanStop::InventoryLimit)
    );
    assert!(limited.is_empty());
    budget.remaining = 8_000_000;
    assert_eq!(scan(&[], &mut limited, &mut budget, 0, 1), Ok(()));
}

fn arm_load(bytes: &mut [u8], at: usize, register: u32, slot: usize, value: u32) {
    put32(
        bytes,
        at,
        0xe59f_0000 | register << 12 | (slot - at - 8) as u32,
    );
    put32(bytes, slot, value);
}

#[test]
fn alternate_dma_crt_forms_retain_only_bounded_copies() {
    for peripheral in [false, true] {
        let mut bytes = fixture(true, true);
        bytes[0xe4..0x180].fill(0);
        let mut slot = 0x260;
        let mut load = |bytes: &mut [u8], at, register, value| {
            arm_load(bytes, at, register, slot, value);
            slot += 4;
        };
        if peripheral {
            words(
                &mut bytes,
                0xe4,
                &[
                    0xe3a0_c301,
                    0xe3a0_0901,
                    0xe380_0014,
                    0xe3a0_1f81,
                    0xe18c_00b1,
                ],
            );
            let at = 0xf8;
            for (offset, word) in [
                (4, 0xe58c_00d4),
                (12, 0xe58c_00d8),
                (20, 0xe041_3000),
                (24, 0xe3a0_1321),
                (28, 0xe181_0143),
                (32, 0xe58c_00dc),
                (40, 0xe080_0003),
                (44, 0xe58c_00d4),
                (52, 0xe58c_00d8),
                (60, 0xe041_0000),
                (64, 0xe3a0_1321),
                (68, 0xe181_0140),
                (72, 0xe58c_00dc),
            ] {
                put32(&mut bytes, at + offset, word);
            }
            for (offset, register, value) in [
                (0, 0, 0x0800_3000),
                (8, 0, 0x0300_0000),
                (16, 1, 0x0300_0100),
                (36, 0, 0x0800_3000),
                (48, 0, 0x0200_0000),
                (56, 1, 0x0200_0100),
            ] {
                load(&mut bytes, at + offset, register, value);
            }
        } else {
            let at = 0xe4;
            for (offset, word) in [
                (8, 0xe582_0000),
                (16, 0xe582_0004),
                (24, 0xe041_3000),
                (28, 0xe3a0_1321),
                (32, 0xe181_0143),
                (36, 0xe582_0008),
                (48, 0xe080_0003),
                (52, 0xe582_0000),
                (60, 0xe582_0004),
                (68, 0xe041_0000),
                (72, 0xe3a0_1321),
                (76, 0xe181_0140),
                (80, 0xe582_0008),
            ] {
                put32(&mut bytes, at + offset, word);
            }
            for (offset, register, value) in [
                (0, 2, 0x0400_00d4),
                (4, 0, 0x0800_3000),
                (12, 0, 0x0300_0000),
                (20, 1, 0x0300_0100),
                (40, 2, 0x0400_00d4),
                (44, 0, 0x0800_3000),
                (56, 0, 0x0200_0000),
                (64, 1, 0x0200_0100),
            ] {
                load(&mut bytes, at + offset, register, value);
            }
        }
        let songs = discover(&bytes);
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].native.ram_copies.len(), 2);
        let mut native = songs[0].native.clone();
        native.ram_copies[0].source.byte_len = 0x7e00;
        assert!(driver::workspace(&native).is_none());
    }
}

#[test]
fn copied_images_bind_each_song_to_one_driver() {
    let mut bytes = fixture(false, false);
    let mut second = fixture(true, false);
    for at in (0..second.len()).step_by(4) {
        let value = u32::from_le_bytes(second[at..at + 4].try_into().unwrap());
        if (0x0800_0000..0x0a00_0000).contains(&value) {
            put32(&mut second, at, value + 0x6000);
        }
    }
    second[0xbd] = 0u8.wrapping_sub(
        second[0xa0..0xbd]
            .iter()
            .fold(0x19u8, |sum, value| sum.wrapping_add(*value)),
    );
    bytes.extend(second);
    let songs = discover(&bytes);
    assert_eq!(songs.len(), 2);
    assert_eq!(songs[0].native.init.source.effective_offset, 0x1000);
    assert_eq!(songs[1].native.init.source.effective_offset, 0x7000);
    let mut missing_version = bytes.clone();
    missing_version[0xb800..0xb880].fill(0);
    assert_eq!(discover(&missing_version).len(), 1);
    bytes[0x60b2] = 0;
    assert!(discover(&bytes).is_empty());
}
