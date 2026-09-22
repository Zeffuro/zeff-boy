use super::Profile;
use crate::MediaIdentity;

pub(super) const PROFILE_ID: &str = "gb-native-cgb-timer-banked-synthetic";
pub(super) const PROFILE: Profile = Profile {
    id: PROFILE_ID,
    source: "synthetic",
    audio_hash: "synthetic",
    byte_len: 0x20_0000,
    table: 0x4500,
    queue: 0x1000,
    init: 0x1020,
    timer_start: 0x1040,
    wrapper_end: 0x1140,
    music_selectors: &[1, 2],
    effect_range: Some((0x4c, 0x4d)),
    empty_effects: &[],
};

pub fn fixture_rom() -> Vec<u8> {
    fixture(0x20_0000, 6)
}
pub fn fixture_rom_small() -> Vec<u8> {
    fixture(0x10_0000, 5)
}

pub(super) fn matches(bytes: &[u8]) -> bool {
    match bytes.len() {
        0x20_0000 => bytes == fixture_rom(),
        0x10_0000 => bytes == fixture_rom_small(),
        _ => false,
    }
}

pub(super) fn source_matches(media: &MediaIdentity) -> bool {
    let bytes = match media.byte_len {
        0x20_0000 => fixture_rom(),
        0x10_0000 => fixture_rom_small(),
        _ => return false,
    };
    media.sha256.as_deref() == Some(zeff_firmware::sha256_hex(&bytes).as_str())
}

fn put(bytes: &mut [u8], address: usize, code: &[u8]) {
    bytes[address..address + code.len()].copy_from_slice(code);
}

fn fixture(len: usize, size: u8) -> Vec<u8> {
    let mut bytes = vec![0xff; len];
    bytes[0x134..0x150].fill(0);
    put(&mut bytes, 0x134, b"TIMER BANKED FIX");
    bytes[0x143] = 0xc0;
    put(&mut bytes, 0x147, &[0x1b, size, 2]);
    put(&mut bytes, 0x100, &[0xc3, 0x50, 1]);
    put(&mut bytes, 0x50, &[0xc3, 0, 0x11]);
    put(&mut bytes, 0x1000, &[0xea, 0, 0xc0, 0xc9]);
    put(
        &mut bytes,
        0x1020,
        &[
            0xaf, 0xe0, 0xdc, 0x3e, 0x80, 0xe0, 0x26, 0xe0, 0x11, 0x3e, 0x77, 0xe0, 0x24, 0x3e,
            0x11, 0xe0, 0x25, 0x3e, 0xf0, 0xe0, 0x12, 0xc9,
        ],
    );
    put(
        &mut bytes,
        0x1040,
        &[
            0xaf, 0xe0, 7, 0x3e, 0x77, 0xe0, 5, 0xe0, 6, 0x3e, 4, 0xe0, 7, 0xc9,
        ],
    );
    put(
        &mut bytes,
        0x1100,
        &[
            0xf5, 0xf0, 0xdc, 0x3d, 0xe0, 0xdc, 0x20, 9, 0x3e, 7, 0xe0, 0xdc, 0xf0, 6, 0x3d, 0xe0,
            5, 0xfa, 0, 0xc0, 0xc6, 0x20, 0xe0, 0x13, 0x3e, 0x87, 0xe0, 0x14, 0xf1, 0xd9,
        ],
    );
    let bank = 0x39 * 0x4000;
    put(
        &mut bytes,
        bank + 0x500,
        &[0, 0, 0, 0, 0x20, 0x45, 1, 0x40, 0x45],
    );
    put(&mut bytes, bank + 0x500 + 0x4c * 3, &[1, 0, 0x46]);
    put(&mut bytes, bank + 0x500 + 0x4d * 3, &[2, 0x20, 0x46]);
    for (header, stream_bank, base) in [(0x4520, 0x39_u8, 0x4560_u16), (0x4540, 0x3a, 0x4580)] {
        let offset = bank + header - 0x4000;
        for (slot, channel) in [0, 1, 4, 6].into_iter().enumerate() {
            bytes[offset + slot * 3] = channel;
            let pointer = base + slot as u16 * 4;
            put(&mut bytes, offset + slot * 3 + 1, &pointer.to_le_bytes());
            let stream = usize::from(stream_bank) * 0x4000 + usize::from(pointer) - 0x4000;
            put(&mut bytes, stream, &[0x20 + slot as u8, 4, 0xff]);
        }
        bytes[offset + 12] = 0xff;
    }
    put(
        &mut bytes,
        bank + 0x600,
        &[0x12, 0x80, 0x46, 0xb5, 0x90, 0x46, 0xff],
    );
    put(
        &mut bytes,
        bank + 0x620,
        &[0x73, 0x80, 0x46, 0xe7, 0x90, 0x46, 0x25, 0xa0, 0x46, 0xff],
    );
    for (stream_bank, pointer, note) in [
        (0x3a_u8, 0x4680_u16, 0x30),
        (0x3a, 0x4690, 0x31),
        (0x3b, 0x4680, 0x32),
        (0x3b, 0x4690, 0x33),
        (0x3b, 0x46a0, 0x34),
    ] {
        let stream = usize::from(stream_bank) * 0x4000 + usize::from(pointer) - 0x4000;
        put(&mut bytes, stream, &[note, 4, 0xff]);
    }
    bytes[0x14d] = bytes[0x134..0x14d]
        .iter()
        .fold(0_u8, |sum, byte| sum.wrapping_sub(*byte).wrapping_sub(1));
    bytes
}
