use std::sync::atomic::AtomicBool;

use super::*;

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn arm_words(bytes: &mut [u8], at: usize, pattern: &[u32]) {
    for (i, &value) in pattern.iter().enumerate() {
        put32(bytes, at + i * 4, value);
    }
}

fn thumb_words(bytes: &mut [u8], at: usize, pattern: &[u16]) {
    for (i, &value) in pattern.iter().enumerate() {
        put16(bytes, at + i * 2, value);
    }
}

fn thumb_literal(bytes: &mut [u8], at: usize, address: usize, value: u32) {
    let register = half(bytes, at).unwrap() & 0x700;
    put16(
        bytes,
        at,
        0x4800 | register | ((address - ((at + 4) & !3)) / 4) as u16,
    );
    put32(bytes, address, value);
}

fn call(bytes: &mut [u8], at: usize, target: usize) {
    let relative = (target as i32 - at as i32 - 4) as u32;
    put16(bytes, at, 0xf000 | ((relative >> 12) & 0x7ff) as u16);
    put16(bytes, at + 2, 0xf800 | ((relative >> 1) & 0x7ff) as u16);
}

fn fixture(branch_to_crt: bool) -> Vec<u8> {
    let mut bytes = vec![0; 0x5000];
    let crt = if branch_to_crt {
        put32(&mut bytes, 0xc0, 0xea00_0000);
        0xc8
    } else {
        0xc0
    };
    arm_words(
        &mut bytes,
        crt,
        &[
            0xe3a0_0012,
            0xe129_f000,
            0xe59f_d028,
            0xe3a0_001f,
            0xe129_f000,
            0xe59f_d018,
        ],
    );
    put32(&mut bytes, crt + 0x38, 0x0300_7f00);
    put32(&mut bytes, crt + 0x34, 0x0300_7e00);
    bytes
}

fn add_main(bytes: &mut [u8], main: usize) {
    thumb_words(bytes, main, MAIN_PREFIX);
    thumb_words(bytes, main + 0xc6, CONFIGURE);
    thumb_words(bytes, main + 0x178, CALL_INIT);
    let mut next = main + 0x100;
    for (i, &instruction) in MAIN_PREFIX.iter().enumerate() {
        if instruction & 0xf800 == 0x4800 {
            thumb_literal(bytes, main + i * 2, next, 0);
            next += 4;
        }
    }
    for (offset, address, value) in [
        (0xc8, 0x160, 9000),
        (0xe4, 0x164, ROM_BASE + 0x2800),
        (0x178, 0x1b0, 0x0300_0030),
        (0x184, 0x1b4, ROM_BASE + 0x1301),
        (0x186, 0x1b8, ROM_BASE + 0x1401),
    ] {
        thumb_literal(bytes, main + offset, main + address, value);
    }
    for (offset, value) in [
        (0x10, 0x0400_00d4),
        (0x1c, 0x8501_0000),
        (0x2c, 0x8500_1f80),
        (0x40, 0x8100_c000),
        (0x50, 0x8100_0200),
        (0x6c, ROM_BASE + 0x2800),
        (0x70, 0x0200_0000),
        (0x74, 0x8000_0004),
        (0x7a, ROM_BASE + 0x2808),
        (0x7e, 0x0300_4000),
        (0x82, 0x8000_0004),
    ] {
        let (_, witness) = literal(bytes, main + offset).unwrap();
        put32(bytes, witness.effective_offset as usize, value);
    }
    call(bytes, main + 0xee, 0x1100);
    call(bytes, main + 0x180, 0x1200);
    call(bytes, main + 0x18c, 0x1000);
    thumb_words(bytes, 0x1100, signatures::ESTIMATE[0].value);
    thumb_words(bytes, 0x1200, signatures::ASSIGN[0].value);
}

fn recover_fixture(bytes: &[u8], boot: usize) -> Option<Setup> {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    recover(bytes, 0, 0x1000, ROM_BASE + boot as u32, &mut budget).unwrap()
}

#[test]
fn external_launcher_requires_original_crt_and_complete_signature() {
    for (branch, pattern) in [
        (false, DISPLAY_LOADER),
        (true, PALETTE_LOADER),
        (false, THUMB_INTRO_LOADER),
        (true, PACKED_INTRO_LOADER),
    ] {
        let mut bytes = fixture(branch);
        arm_words(&mut bytes, 0x4000, pattern);
        let setup = recover_fixture(&bytes, 0x4000).unwrap();
        assert_eq!(setup.boot_entry, ROM_BASE + 0xc0);
        assert_eq!(setup.configuration_entry, None);
        let end = 0x4000 + pattern.len() * 4;
        let bounded = recover_fixture(&bytes[..end], 0x4000).unwrap();
        assert!(
            bounded
                .spans
                .iter()
                .all(|span| span.effective_offset as usize + span.byte_len as usize <= end)
        );
        bytes[0x4000 + (pattern.len() - 1) * 4] ^= 1;
        assert!(recover_fixture(&bytes, 0x4000).is_none());
        arm_words(&mut bytes, 0x4000, pattern);
        put32(&mut bytes, if branch { 0x100 } else { 0xf8 }, 0x0400_0000);
        assert!(recover_fixture(&bytes, 0x4000).is_none());
    }
}

#[test]
fn original_crt_branch_stays_within_the_header_startup_region() {
    let mut bytes = fixture(false);
    bytes.copy_within(0xc0..0x100, 0xe0);
    put32(&mut bytes, 0xc0, 0xea00_0006);
    arm_words(&mut bytes, 0x4000, PACKED_INTRO_LOADER);
    let setup = recover_fixture(&bytes, 0x4000).unwrap();
    assert_eq!(setup.boot_entry, ROM_BASE + 0xc0);
    for target in [0x80, 0x400] {
        bytes.copy_within(0xe0..0x120, target);
        let relative = (target as i32 - 0xc0 - 8) / 4;
        put32(
            &mut bytes,
            0xc0,
            0xea00_0000 | (relative as u32 & 0x00ff_ffff),
        );
        assert!(recover_fixture(&bytes, 0x4000).is_none());
    }
}

#[test]
fn ram_launcher_requires_unique_constructor_and_bounded_dma_sources() {
    let mut bytes = fixture(false);
    arm_words(&mut bytes, 0xe4, &[0xe59f_1158, 0xe1a0_e00f, 0xe12f_ff11]);
    put32(&mut bytes, 0x244, ROM_BASE + 0x4000);
    arm_words(&mut bytes, 0x4000, RAM_LOADER);
    add_main(&mut bytes, 0x274);
    let setup = recover_fixture(&bytes, 0xc0).unwrap();
    assert_eq!(
        setup.configuration_entry,
        Some(RomSpan::new(0x274, MAIN_LEN))
    );
    assert!(setup.spans.contains(&RomSpan::new(0x2800, 8)));
    let (_, control) = literal(&bytes, 0x274 + 0x74).unwrap();
    put32(&mut bytes, control.effective_offset as usize, 0x8400_0004);
    assert!(recover_fixture(&bytes, 0xc0).is_none());
    put32(&mut bytes, control.effective_offset as usize, 0x8000_0004);
    let (_, source) = literal(&bytes, 0x274 + 0x6c).unwrap();
    put32(
        &mut bytes,
        source.effective_offset as usize,
        ROM_BASE + 0x4ffc,
    );
    assert!(recover_fixture(&bytes, 0xc0).is_none());
    put32(
        &mut bytes,
        source.effective_offset as usize,
        ROM_BASE + 0x2800,
    );
    add_main(&mut bytes, 0x800);
    assert!(recover_fixture(&bytes, 0xc0).is_none());
}

#[test]
fn changed_native_init_call_does_not_select_a_configuration_main() {
    let mut bytes = fixture(false);
    add_main(&mut bytes, 0x274);
    assert!(configuration_main(&bytes, 0x274, 0x1000).is_some());
    call(&mut bytes, 0x274 + 0x18c, 0x1002);
    assert!(configuration_main(&bytes, 0x274, 0x1000).is_none());
}
