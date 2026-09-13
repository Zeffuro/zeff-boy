use super::{fixture, profiles::Profile, separate::ConfigurationLayout};

pub(super) const PROFILE: Profile = Profile {
    id: "gbass-authored-split-instruments-fixture",
    sha256: "ff3f56c1fde49ab0e535026dcabe426484baffbad899ffcbd83df84e19d93449",
    header: b"GBASS SPLIT00001",
    layout: ConfigurationLayout::SeparatePcm,
    play: (0x700, 0x28),
    ..fixture::STARTED_PROFILE
};

pub fn fixture_rom_separate() -> Vec<u8> {
    let mut bytes = fixture::fixture_rom_started();
    bytes[0xa0..0xb0].copy_from_slice(PROFILE.header);
    let configuration = [
        2,
        2,
        0x0800_0900,
        0,
        0x0800_09a0,
        1,
        0x0800_09a0,
        0x0800_09a4,
        1,
        0x0800_0980,
        1,
        0x0300_1400,
        0x0300_1500,
        0x0300_1600,
        1,
        0x0300_1700,
        0x0800_09b0,
        0x0300_1800,
        0x0300_1900,
        0x0300_1a00,
        0x0300_1b00,
    ];
    let player = [
        0x1c04_b510,
        0x6948_4906,
        0x60d0_4a06,
        0x6110_6988,
        0x6150_69c8,
        0xf7ff_1c20,
        0xbc10_fe33,
        0x4700_bc01,
        0x0800_0800,
        0x0300_1400,
    ];
    for (offset, words) in [
        (0x800, configuration.as_slice()),
        (0x700, player.as_slice()),
    ] {
        for (index, word) in words.iter().enumerate() {
            bytes[offset + index * 4..offset + index * 4 + 4]
                .copy_from_slice(&u32::to_le_bytes(*word));
        }
    }
    bytes
}
