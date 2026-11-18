fn store(program: &mut Vec<u16>, byte: bool, value: u16, address: u32) {
    program.extend([
        if byte { 0x13fc } else { 0x33fc },
        value,
        (address >> 16) as u16,
        address as u16,
    ]);
}

fn upload(program: &mut Vec<u16>, address: u16, bytes: &[u8]) {
    for (index, value) in bytes.iter().copied().enumerate() {
        store(
            program,
            true,
            u16::from(value),
            0xa00000 + u32::from(address) + index as u32,
        );
    }
}

fn sound_write(program: &mut Vec<u8>, address: u16, value: u8) {
    program.extend([0x3e, value, 0x32, address as u8, (address >> 8) as u8]);
}

pub(super) fn rom() -> Vec<u8> {
    let mut main = Vec::new();
    for (byte, value, address) in [
        (false, 0x8124, 0xc00004),
        (true, 0x8a, 0xc00011),
        (true, 8, 0xc00011),
        (true, 0x90, 0xc00011),
        (false, 0x100, 0xa11100),
        (false, 0x100, 0xa11200),
        (false, 0, 0xa11200),
        (false, 0x100, 0xa11200),
    ] {
        store(&mut main, byte, value, address);
    }
    main.extend([0x4e71; 64]);
    for (value, address) in [(0, 0xa04000), (0xff, 0xa04001)] {
        store(&mut main, true, value, address);
    }
    let mut sound = vec![0x31, 0xfd, 0x1f];
    for (address, value) in [
        (0x7f11, 0x85),
        (0x7f11, 6),
        (0x7f11, 0x90),
        (0x4002, 0xb6),
        (0x4001, 0xc0),
        (0x4000, 0x2b),
        (0x4003, 0x80),
        (0x4000, 0x2a),
        (0x4001, 0x9a),
    ] {
        sound_write(&mut sound, address, value);
    }
    for slot in [0, 4, 8, 12] {
        for (address, value) in [
            (0x30 + slot, 1),
            (0x40 + slot, 0x20),
            (0x50 + slot, 0x1f),
            (0x60 + slot, 0),
            (0x70 + slot, 0),
            (0x80 + slot, 0x0f),
        ] {
            sound_write(&mut sound, 0x4000, address);
            sound_write(&mut sound, 0x4001, value);
        }
    }
    for (address, value) in [
        (0xb0, 7),
        (0xb4, 0xc0),
        (0xa4, 0x22),
        (0xa0, 0x69),
        (0x28, 0xf0),
    ] {
        sound_write(&mut sound, 0x4000, address);
        sound_write(&mut sound, 0x4001, value);
    }
    sound.extend([0xed, 0x56, 0xfb, 0x76, 0x18, 0xfd]);
    let mut handler = vec![0x3a, 0, 0x40, 0x32, 1, 7, 0x3a, 0, 7, 0x3c, 0x32, 0, 7];
    sound_write(&mut handler, 0x4000, 0x2a);
    handler.extend([
        0x3a, 0, 7, 0xc6, 0x70, 0x32, 1, 0x40, 6, 0x40, 0x10, 0xfe, 0xfb, 0xed, 0x4d,
    ]);
    upload(&mut main, 0, &[0xc3, 0, 1]);
    upload(&mut main, 0x38, &handler);
    upload(&mut main, 0x100, &sound);
    upload(&mut main, 0x700, &[0, 0xff]);
    store(&mut main, false, 0, 0xa11100);
    main.extend([0x4e71, 0x60fc]);
    let mut rom = vec![0; 0x4000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, word) in main.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    rom
}
