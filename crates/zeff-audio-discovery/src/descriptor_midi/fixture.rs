use super::{half, signatures as sig};

pub(super) const SELECTOR: usize = 0x4000;
pub(super) const HANDOFF: usize = 0x1010;
pub(super) const ROOT: usize = 0x6000;
pub(super) const BANK: usize = 0x6240;
pub(super) const INSTRUMENT: usize = 0x6400;
pub(super) const SAMPLE: usize = 0x6500;
pub(super) const DESCRIPTOR: usize = 0x6800;
pub(super) const MIDI: usize = 0x7000;

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
    let slot = ((at + 4) & !3) + usize::from(instruction & 255) * 4;
    put32(bytes, slot, value);
}

pub(super) fn rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x8000];
    let init = SELECTOR + 0x1248;
    let play = SELECTOR - 0x1c0;
    let update = SELECTOR + 0xc48;
    let configure = SELECTOR + 0xe18;
    for (at, pattern) in [
        (SELECTOR, sig::SELECTOR),
        (init, sig::INIT),
        (play, sig::PLAY),
        (update, sig::UPDATE),
        (update + 0x38, sig::UPDATE_PLAYERS),
        (SELECTOR - 0x2238, sig::DMA),
        (configure, sig::CONFIGURE),
        (SELECTOR + 0x798, sig::EVENT),
        (SELECTOR - 0xe58, sig::VOICE),
        (SELECTOR - 0x20f0, sig::SAMPLE),
    ] {
        for (index, &value) in pattern.iter().enumerate() {
            put16(&mut bytes, at + index * 2, value);
        }
    }
    bl(&mut bytes, SELECTOR + 0x1c, play);
    bl(&mut bytes, init + 0x22, SELECTOR - 0x1f6c);
    bl(&mut bytes, init + 0x50, SELECTOR - 0x1538);
    bl(&mut bytes, play + 0x48, SELECTOR - 0x1538);
    literal(&mut bytes, SELECTOR + 4, 0x0800_6100);
    literal(&mut bytes, SELECTOR + 6, 0x0800_6000);
    literal(&mut bytes, init + 0x34, 0x0800_6048);
    literal(&mut bytes, update + 0x38, 0x0800_6024);
    literal(&mut bytes, update + 0xa4, 0x0800_6018);
    literal(&mut bytes, play + 0x4c, 0x0800_6200);
    literal(&mut bytes, configure + 2, 0x0300_6800);
    literal(&mut bytes, init + 0x8c, 0x0300_6800);
    put16(&mut bytes, play + 0x16e, 0x4812);
    put16(&mut bytes, play + 0x17a, 0x4810);
    literal(&mut bytes, play + 0x16e, 0x0800_6f40);
    literal(&mut bytes, play + 0x17a, 0x0800_6f44);
    bytes[0x6f40] = b'[';
    bytes[0x6f44] = b']';
    bl(&mut bytes, HANDOFF - 16, init);
    for (index, value) in [0x2023, 0x2102, 0x2202, 0x2304].into_iter().enumerate() {
        put16(&mut bytes, HANDOFF - 12 + index * 2, value);
    }
    bl(&mut bytes, HANDOFF - 4, configure);
    for (index, value) in [0x4927, 0x2008, 0x8008, 0x4927, 0x4a27, 0x1c10]
        .into_iter()
        .enumerate()
    {
        put16(&mut bytes, HANDOFF + index * 2, value);
    }
    literal(&mut bytes, HANDOFF, 0x0400_0004);
    literal(&mut bytes, HANDOFF + 6, 0x0400_0200);
    literal(&mut bytes, HANDOFF + 8, 0x2401);
    put32(&mut bytes, 0x6018, 8);
    put32(&mut bytes, 0x60fc, 9);
    for index in 0..9 {
        let state = 0x0300_1000 + index as u32 * 48;
        let at = 0x6048 + index * 20;
        put16(&mut bytes, at, index as u16 | (4 << 5));
        put32(&mut bytes, at + 4, 0x0300_2000 + index as u32 * 128);
        put32(&mut bytes, at + 8, 0x0300_5000 + index as u32 * 40);
        put32(&mut bytes, at + 12, 0x0300_4000 + index as u32 * 144);
        put32(&mut bytes, at + 16, state);
        put32(&mut bytes, 0x6024 + index * 4, state);
        put32(&mut bytes, 0x6100 + index * 12, state);
    }
    put32(&mut bytes, 0x6200, 0x0800_6240);
    put32(&mut bytes, BANK, 0x0800_6400);
    put32(&mut bytes, BANK + 4, 0x0800_6440);
    bytes[INSTRUMENT] = b'A';
    put32(&mut bytes, INSTRUMENT + 4, 0x0800_6500);
    bytes[0x6440] = b'P';
    bytes[0x6460] = 2;
    put32(&mut bytes, 0x6444, 0x0800_6680);
    for (index, value) in [16, 13_379, 60, 0, 0, 0x0800_6600].into_iter().enumerate() {
        put32(&mut bytes, SAMPLE + index * 4, value);
    }
    bytes[0x6600..0x6610].copy_from_slice(&[
        0, 32, 64, 96, 127, 96, 64, 32, 0, 224, 192, 160, 128, 160, 192, 224,
    ]);
    bytes[0x6680..0x6690].copy_from_slice(&[
        1, 35, 69, 103, 137, 171, 205, 239, 254, 220, 186, 152, 118, 84, 50, 16,
    ]);
    for (entry, raw, player, program) in [(0, 0, 0, 0), (1, 2, 1, 1)] {
        let descriptor = DESCRIPTOR + entry * 20;
        let midi = MIDI + entry * 0x100;
        let name = 0x6f00 + entry * 0x20;
        put32(&mut bytes, ROOT + raw * 8, 0x0800_0000 + descriptor as u32);
        put32(&mut bytes, ROOT + raw * 8 + 4, player);
        put32(&mut bytes, descriptor, 0x0800_0000 + midi as u32);
        put32(&mut bytes, descriptor + 4, player | (100 << 15));
        put32(&mut bytes, descriptor + 8, 255);
        put32(&mut bytes, descriptor + 12, 0x0800_0000 + name as u32);
        put32(&mut bytes, descriptor + 16, raw as u32);
        let title = if entry == 0 {
            b"Synthetic PCM".as_slice()
        } else {
            b"Synthetic wave".as_slice()
        };
        bytes[name..name + title.len()].copy_from_slice(title);
        let smf = midi_file(program, 100);
        bytes[midi..midi + smf.len()].copy_from_slice(&smf);
    }
    bytes
}

pub(super) fn midi_file(program: u8, velocity: u8) -> Vec<u8> {
    midi_tracks(&[
        vec![0, 255, 81, 3, 7, 161, 32, 48, 255, 47, 0],
        vec![
            0, 0xc7, program, 0, 0x97, 60, velocity, 48, 0x87, 60, 0, 0, 255, 47, 0,
        ],
    ])
}

pub(super) fn midi_tracks(tracks: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = b"MThd\0\0\0\x06\0\x01".to_vec();
    bytes.extend_from_slice(&(tracks.len() as u16).to_be_bytes());
    bytes.extend_from_slice(&24u16.to_be_bytes());
    for track in tracks {
        bytes.extend_from_slice(b"MTrk");
        bytes.extend_from_slice(&(track.len() as u32).to_be_bytes());
        bytes.extend_from_slice(track);
    }
    bytes
}
