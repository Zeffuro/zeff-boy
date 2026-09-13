use super::{half, signatures as sig};

pub(super) const INIT: usize = 0x1000;
pub(super) const SONG: usize = INIT + 0x358;
pub(super) const TABLE: usize = 0x3000;
pub(super) const BANK: usize = 0x3100;
pub(super) const SAMPLE: usize = 0x3200;

pub(super) fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

pub(super) fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub(super) fn bl(bytes: &mut [u8], at: usize, target: usize) {
    let relative = target as i32 - at as i32 - 4;
    put16(bytes, at, 0xf000 | ((relative >> 12) as u16 & 0x7ff));
    put16(bytes, at + 2, 0xf800 | ((relative >> 1) as u16 & 0x7ff));
}

fn literal(bytes: &mut [u8], at: usize, value: u32) {
    let instruction = half(bytes, at).unwrap();
    assert_eq!(instruction & 0xf800, 0x4800);
    put32(
        bytes,
        ((at + 4) & !3) + usize::from(instruction & 255) * 4,
        value,
    );
}

fn pattern(bytes: &mut [u8], at: usize, words: &[u16]) {
    for (index, &word) in words.iter().enumerate() {
        put16(bytes, at + index * 2, word);
    }
}

pub(super) fn build() -> Vec<u8> {
    let mut bytes = vec![0; 0x5000];
    let state = 0x0300_6000;
    let irq = INIT + 0x134;
    let update = INIT + 0x1dc;
    let render = update - 0x54;
    let play = SONG + 0x21c;
    for (at, words) in [
        (INIT, sig::CONTEXT_INIT),
        (SONG, sig::CONTEXT_SONG),
        (SONG + 0x34, sig::CONTEXT_ALLOCATE),
        (play, sig::CONTEXT_EFFECT),
        (update, sig::CONTEXT_UPDATE),
        (irq, sig::CONTEXT_IRQ_A),
        (render, sig::CONTEXT_RENDER),
    ] {
        pattern(&mut bytes, at, words);
    }
    literal(&mut bytes, INIT + 0x14, state);
    pattern(&mut bytes, INIT + 0x18, &[0x2090, 0x0040, 0x8398]);
    pattern(&mut bytes, INIT + 0x28, &[0x480c, 0x6298, 0x4d0c, 0x4c0d]);
    literal(&mut bytes, INIT + 0x28, 11025);
    literal(&mut bytes, INIT + 0x2c, 0x0800_22b4);
    literal(&mut bytes, INIT + 0x2e, 0x0800_2000);
    pattern(&mut bytes, play - 12, &[0x4901, 0x6809, 0x60c8, 0x4770]);
    literal(&mut bytes, play - 12, state);
    put16(&mut bytes, INIT + 0xbc, 0x4816);
    literal(&mut bytes, INIT + 0xbc, 0x0800_3100);
    bl(&mut bytes, INIT + 0xbe, play - 12);
    pattern(
        &mut bytes,
        INIT + 0xfa,
        &[
            0x490c, 0x480c, 0x6008, 0xb001, 0xbc08, 0x4698, 0xbcf0, 0xbc01, 0x4700,
        ],
    );
    literal(&mut bytes, INIT + 0xfa, 0x0400_0100);
    literal(&mut bytes, INIT + 0xfc, 0x0080_fa0f);
    literal(&mut bytes, play + 6, state);
    literal(&mut bytes, SONG + 0x3a, state);
    put16(&mut bytes, SONG + 4, 0x2a01);
    literal(&mut bytes, SONG + 8, 0x0800_3000);
    bl(&mut bytes, SONG + 0x16, SONG + 0x34);
    literal(&mut bytes, update + 0xa, state);
    literal(&mut bytes, update + 0x1a, 0x0400_0208);
    literal(&mut bytes, update + 0x20, 0x0400_0104);
    literal(&mut bytes, render + 0xa, state);
    literal(&mut bytes, irq + 2, 0x0400_00c4);
    literal(&mut bytes, irq + 8, 0x0400_00d0);
    literal(&mut bytes, irq + 0xc, state);
    put16(&mut bytes, 0x300, 0x2204);
    bl(&mut bytes, 0x302, INIT);
    for (index, opcode) in super::codec::MIXER.into_iter().enumerate() {
        let value = match index * 4 {
            0xec | 0x2a8 | 0x344 => state,
            0x2a0 => 0x0800_2400,
            0x2a4 => 0x0800_2a20,
            _ => opcode,
        };
        put32(&mut bytes, 0x2000 + index * 4, value);
    }
    put32(&mut bytes, BANK + 4, 2);
    put32(&mut bytes, BANK + 8, 0x0800_3110);
    put32(&mut bytes, BANK + 12, 0x0800_3110);
    put32(&mut bytes, 0x3110, 0x0800_3200);
    put32(&mut bytes, 0x3114, 0x0800_3600);
    for (index, at) in [SAMPLE, 0x3600].into_iter().enumerate() {
        put32(&mut bytes, at, 1);
        put32(&mut bytes, at + 4, 16);
        for offset in 0..16 {
            bytes[at + 16 + offset] = (offset * 7 + index * 13) as u8;
        }
    }
    for (index, at) in [0x4000, 0x4400].into_iter().enumerate() {
        put32(&mut bytes, at + 4, 160);
        for offset in 0..160 {
            bytes[at + 16 + offset] = (offset * 3 + index * 11) as u8;
        }
    }
    for (index, (order, blocks)) in [(0x4100, 0x4200), (0x4120, 0x4210)].into_iter().enumerate() {
        put32(&mut bytes, TABLE + index * 8, 0x0800_0000 + blocks as u32);
        put32(
            &mut bytes,
            TABLE + index * 8 + 4,
            0x0800_0000 + order as u32,
        );
        put32(&mut bytes, blocks, 0x0800_4000);
        put32(&mut bytes, blocks + 4, 0x0800_4400);
        put16(&mut bytes, order, 0);
        put16(&mut bytes, order + 2, if index == 0 { u16::MAX } else { 1 });
        put16(&mut bytes, order + 4, u16::MAX);
    }
    bytes
}

pub(super) fn global() -> Vec<u8> {
    let source = build();
    let mut bytes = vec![0; source.len()];
    bytes[0x3000..].copy_from_slice(&source[0x3000..]);
    put32(&mut bytes, 0, 0xea00_002e);
    for (index, word) in super::startup::CRT.into_iter().enumerate() {
        put32(&mut bytes, 0xc0 + index * 4, word);
    }
    for (at, value) in [
        (0x130, 0x0300_7f00),
        (0x134, 0x0300_7fa0),
        (0x26c, 0x0300_7ffc),
        (0x270, 0x0300_6100),
        (0x274, 0x0300_6000),
        (0x278, 0x0800_4800),
        (0x27c, 0x0800_02fd),
    ] {
        put32(&mut bytes, at, value);
    }
    put16(&mut bytes, 0x2fc, 0xb500);
    let state = 0x0300_6000;
    let init = 0x1000;
    let update = init + 0x2a4;
    let render = update - 0x48;
    let irq = init + 0x228;
    let set = init + 0x5d4;
    let song = set + 0x88;
    let play = song + 0x54;
    for (at, words) in [
        (init, sig::GLOBAL_INIT_A),
        (update, sig::GLOBAL_UPDATE),
        (render, sig::GLOBAL_RENDER),
        (irq, sig::GLOBAL_IRQ_A),
        (song, sig::GLOBAL_SONG),
        (play, sig::GLOBAL_EFFECT_A),
    ] {
        pattern(&mut bytes, at, words);
    }
    pattern(&mut bytes, set, &[0x4901, 0x6008, 0x4770, 0]);
    for at in [set, song + 4, play + 6] {
        literal(&mut bytes, at, state);
    }
    literal(&mut bytes, init + 0x22, state + 0x2c);
    literal(&mut bytes, update + 0xa, state + 0x10);
    literal(&mut bytes, update + 0xe, state - 0x10);
    literal(&mut bytes, update + 0x12, state + 8);
    literal(&mut bytes, update + 0x1a, state + 0x2c);
    literal(&mut bytes, render + 0xa, 0x0800_4a00);
    for (at, value) in [
        (irq, 0x0400_00c4),
        (irq + 6, 0x0400_00d0),
        (irq + 0xa, 0x0400_0104),
        (irq + 0xe, state + 4),
    ] {
        literal(&mut bytes, at, value);
    }
    pattern(&mut bytes, 0x300, &[0x48ff, 0x2102]);
    literal(&mut bytes, 0x300, 11025);
    bl(&mut bytes, 0x304, init);
    put16(&mut bytes, 0x308, 0x48ff);
    literal(&mut bytes, 0x308, 0x0800_3100);
    bl(&mut bytes, 0x30a, set);
    bytes
}
