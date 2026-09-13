use super::profiles::{CUES, Profile};

pub(super) const PROFILE: Profile = Profile {
    id: "nes-native-synthetic",
    sources: &[
        (
            "a5e48e4edc37d6ab6b4e1a76a25abfcdcdfc10a2b2eeab5368a7fd1a1cdbc000",
            0x10010,
        ),
        (
            "b85ff9b19a281c1edd29bc6bbf84bfdf9b25a4077b96670ef4378a29413ef174",
            0x12010,
        ),
    ],
    cues: CUES,
    bootstrap: 0x847f,
    tables_end: 0xf046,
};

pub fn fixture_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x10010];
    bytes[..8].copy_from_slice(b"NES\x1a\x02\x04\x31\x00");
    let init = [
        0x85, 0x10, 0xa9, 0xbf, 0x8d, 0, 0x40, 0xa9, 0xe9, 0x8d, 2, 0x40, 0xa9, 2, 0x8d, 3, 0x40,
        0x60,
    ];
    let tick = [
        0xe6, 0x10, 0xa5, 0x10, 0x29, 0x0f, 0x09, 0xb0, 0x8d, 0, 0x40, 0x60,
    ];
    bytes[0x6c5c..0x6c5c + init.len()].copy_from_slice(&init);
    bytes[0x6d40..0x6d40 + tick.len()].copy_from_slice(&tick);
    for cue in CUES {
        let table = 0x700b + usize::from(cue.raw & 0x3f) * 3;
        for (channel, &(start, _)) in cue.streams.iter().enumerate() {
            let entry = table + channel * 3;
            bytes[entry] = channel as u8 * 4;
            bytes[entry + 1..entry + 3].copy_from_slice(&start.to_le_bytes());
            bytes[usize::from(start) - 0x8000 + 16] = 0xff;
        }
    }
    bytes
}

pub fn fixture_rom_pc10() -> Vec<u8> {
    let mut bytes = fixture_rom();
    bytes[7] = 2;
    bytes[10] = 0x30;
    bytes.resize(0x12010, 0);
    bytes
}
