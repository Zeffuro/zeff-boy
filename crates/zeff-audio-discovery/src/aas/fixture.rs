use super::*;

pub(super) const CONFIG: usize = 0x100;
pub(super) const PLAY: usize = 0x1800;
pub(super) const IRQ: usize = 0x1400;
pub(super) const MODULE: usize = 0x2000;
pub(super) const ROOT: usize = 0x8000;
pub(super) const SEQUENCE: usize = 0x9000;
pub(super) const PATTERNS: usize = 0xc000;

pub(super) fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
pub(super) fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn bl(bytes: &mut [u8], at: usize, target: usize) {
    let relative = target as i32 - at as i32 - 4;
    put16(bytes, at, 0xf000 | ((relative >> 12) as u16 & 0x7ff));
    put16(bytes, at + 2, 0xf800 | ((relative >> 1) as u16 & 0x7ff));
}
fn literal(bytes: &mut [u8], at: usize, slot: usize, value: u32) {
    let register = half(bytes, at).unwrap() & 0x700;
    let distance = slot - ((at + 4) & !3);
    assert!(distance.is_multiple_of(4) && distance < 1024);
    put16(bytes, at, 0x4800 | register | (distance / 4) as u16);
    put32(bytes, slot, value);
}

pub(crate) fn fixture(current: bool) -> Vec<u8> {
    let mut bytes = vec![0; 0x10000];
    let (
        config_len,
        update_call,
        irq_call,
        irq_pending,
        pending,
        config_call,
        config_state,
        play_state,
        sequence,
        patterns,
        sample_data,
        restart,
        channels,
    ) = if current {
        (
            0xfc, 0x26, 0xb6, 0xb0, 8, 0x7a, 0x7e, 0x13e, 0x7a, 0x88, 0xcc, 0xaa0, 16,
        )
    } else {
        (
            0x130, 0x22, 0xa6, 0xa0, 6, 0xca, 0xce, 0x56, 0x64, 0x6e, 0xce, 0xab0, 8,
        )
    };
    let update = CONFIG + config_len;
    let patterns_code = if current {
        [
            signatures::CONFIG_16,
            signatures::PLAY_16,
            signatures::STOP_16,
            signatures::UPDATE_16,
            signatures::IRQ_16,
            signatures::MOD_INTERRUPT_16,
            signatures::DO_CONFIG_16,
        ]
    } else {
        [
            signatures::CONFIG_8,
            signatures::PLAY_8,
            signatures::STOP_8,
            signatures::UPDATE_8,
            signatures::IRQ_8,
            signatures::MOD_INTERRUPT_8,
            signatures::DO_CONFIG_8,
        ]
    };
    for (at, pattern) in [CONFIG, PLAY, 0x1600, update, IRQ, MODULE, 0x1000]
        .into_iter()
        .zip(patterns_code)
    {
        for (index, &value) in pattern.iter().enumerate() {
            put16(&mut bytes, at + index * 2, value);
        }
    }
    let fast_irq = bytes[IRQ..IRQ + 48].to_vec();
    bytes[0x1300..0x1330].copy_from_slice(&fast_irq);
    bl(&mut bytes, CONFIG + config_call, 0x1000);
    bl(&mut bytes, update + update_call, MODULE);
    bl(&mut bytes, IRQ + irq_call, update);
    bl(&mut bytes, PLAY + 4, 0x1600);
    if current {
        bl(&mut bytes, 0x40, CONFIG);
    } else {
        put32(&mut bytes, 0x40, 0xe59f_c008);
        put32(&mut bytes, 0x44, 0xe3a0_0004);
        put32(&mut bytes, 0x48, 0xe1a0_e00f);
        put32(&mut bytes, 0x4c, 0xe12f_ff1c);
        put32(&mut bytes, 0x50, 0x0800_0001 + CONFIG as u32);
    }
    literal(
        &mut bytes,
        CONFIG + config_state,
        CONFIG + config_len - 4,
        0x0200_1000,
    );
    literal(
        &mut bytes,
        CONFIG + 0x18,
        CONFIG + config_len - 8,
        0x0800_4000,
    );
    for index in 0..8 {
        put32(
            &mut bytes,
            0x4000 + index * 4,
            0x0800_0000 + CONFIG as u32 + 0x20,
        );
    }
    literal(&mut bytes, PLAY + 8, PLAY + 0x200, 0x0200_1000);
    literal(&mut bytes, PLAY + play_state, PLAY + 0x204, 0x0200_1002);
    literal(
        &mut bytes,
        0x1600 + if current { 0xa } else { 0xc },
        0x1680,
        0x0200_1002,
    );
    literal(&mut bytes, MODULE + 0xc, MODULE + 0x180, 0x0200_1002);
    literal(&mut bytes, update + pending, update + 0x300, 0x0200_1004);
    literal(&mut bytes, IRQ + irq_pending, IRQ + 0x300, 0x0200_1004);
    for (at, slot, value) in [
        (PLAY + 0x18, PLAY + 0x208, ROOT),
        (PLAY + 0x22, PLAY + 0x20c, 0xb000),
        (PLAY + sequence, PLAY + 0x210, SEQUENCE),
        (PLAY + patterns, PLAY + 0x214, PATTERNS),
        (MODULE + 0xc2, MODULE + 0x184, ROOT + 4),
        (MODULE + sample_data, MODULE + 0x188, 0xb100),
        (MODULE + restart, MODULE + 0xe00, 0xb010),
    ] {
        literal(&mut bytes, at, slot, 0x0800_0000 + value as u32);
    }
    put32(&mut bytes, ROOT, 2);
    bytes[0xb000..0xb002].copy_from_slice(&[2, 1]);
    for song in 0..2 {
        let start = SEQUENCE + song * 128 * channels * 2;
        put16(&mut bytes, start, song as u16);
        put16(&mut bytes, start + 2, song as u16);
        put16(&mut bytes, start + channels * 2, u16::MAX);
        for sample in 0..31 {
            put16(
                &mut bytes,
                ROOT + 4 + (song * 31 + sample) * 12 + 4,
                u16::MAX,
            );
        }
        put16(&mut bytes, ROOT + 4 + song * 31 * 12 + 6, 2);
        bytes[ROOT + 4 + song * 31 * 12 + 9] = 64;
    }
    put32(&mut bytes, PATTERNS, (1 << 24) | (24 << 12));
    put32(&mut bytes, PATTERNS + 256, (1 << 24) | (36 << 12));
    bytes[0xb100..0xb104].copy_from_slice(&[0, 127, 0, 128]);
    bytes
}
