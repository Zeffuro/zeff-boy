use zeff_emu_common::system::System;

use super::{
    RomSpan, SegaPsgRegion,
    profiles::{HeaderLayout, Profile, SupplementalSelector},
};

pub(super) const PROFILE: Profile = Profile {
    name: "synthetic-sega-psg-native",
    sha256: "de2a15e68cacbe1256bcb36e8e56f29b189c8d8dd76eb5268cfa5434eb1ea735",
    rom_len: 0x10000,
    system: System::Sms,
    region: SegaPsgRegion::Export,
    audio_offset: 0x8000,
    driver_address: 0x4000,
    init_address: 0x4050,
    table: 0x4100,
    song_count: 1,
    frame_divider: 1,
    audio_byte_len: 0x8000,
    header_layout: HeaderLayout::FourByteChannels,
    rejected_selectors: &[],
    supplemental_selectors: &[],
};

pub(super) const SIX_BYTE_PROFILE: Profile = Profile {
    name: "synthetic-sega-psg-native-six-byte",
    sha256: "2f04c4f2b7afcc4e544d18fe0f50da986e5cebdfc08d63cf8f4dd3fa029b7cb1",
    system: System::Gg,
    header_layout: HeaderLayout::SixByteChannels,
    ..PROFILE
};

pub(super) const SUPPLEMENTAL_PROFILE: Profile = Profile {
    name: "synthetic-sega-psg-supplemental",
    sha256: "b8a3886f49a2f4f0975fa39500529eacdd1e17723a966810051447628b61e9d5",
    supplemental_selectors: &[SupplementalSelector {
        raw_index: 0x81,
        spans: &[
            RomSpan {
                effective_offset: 0x311,
                byte_len: 1,
                canonical_cpu_address: 0x311,
            },
            RomSpan {
                effective_offset: 0x504,
                byte_len: 1,
                canonical_cpu_address: 0x504,
            },
        ],
    }],
    ..SIX_BYTE_PROFILE
};

pub fn fixture_rom_supplemental() -> Vec<u8> {
    let mut bytes = fixture_rom_six_byte();
    bytes.copy_within(0x8050..0x8061, 0x805a);
    bytes[0x8050..0x805a].copy_from_slice(&[0x3a, 0x11, 3, 0xd3, 0x7f, 0x3a, 4, 5, 0xd3, 0x7f]);
    bytes[0x311] = 0x9f;
    bytes[0x504] = 0xbf;
    bytes
}

pub fn fixture_rom() -> Vec<u8> {
    let mut bytes = vec![0; PROFILE.rom_len];
    bytes[0x7ff0..0x7ff8].copy_from_slice(b"TMR SEGA");
    bytes[0x7fff] = 0x4e;
    let driver = [
        0x3a, 4, 0xde, 0xfe, 0x81, 0xc0, 0xaf, 0x32, 4, 0xde, 0x2a, 0, 0x41, 0x23, 0x23, 0x4e,
        0x23, 0x23, 0x23, 0x23, 0x5e, 0x23, 0x56, 0x23, 0x23, 0x23, 0xe5, 6, 3, 0x1a, 0xd3, 0x7f,
        0x13, 0x10, 0xfa, 0xe1, 0x0d, 0x20, 0xed, 0xc9,
    ];
    bytes[0x8000..0x8000 + driver.len()].copy_from_slice(&driver);
    let init = [
        0x3e, 0x9f, 0xd3, 0x7f, 0x3e, 0xbf, 0xd3, 0x7f, 0x3e, 0xdf, 0xd3, 0x7f, 0x3e, 0xff, 0xd3,
        0x7f, 0xc9,
    ];
    bytes[0x8050..0x8050 + init.len()].copy_from_slice(&init);
    bytes[0x8100..0x8102].copy_from_slice(&0x4140_u16.to_le_bytes());
    bytes[0x8140..0x8146].copy_from_slice(&[0, 0, 3, 0, 1, 3]);
    for channel in 0..3_u16 {
        let address = 0x4200 + channel * 0x10;
        let entry = 0x8146 + usize::from(channel) * 4;
        bytes[entry..entry + 2].copy_from_slice(&address.to_le_bytes());
        let at = usize::from(address) + 0x4000;
        bytes[at..at + 3].copy_from_slice(&[
            0x84 + (channel as u8) * 0x20,
            0x10,
            0x96 + (channel as u8) * 0x20,
        ]);
    }
    bytes
}

pub fn fixture_rom_six_byte() -> Vec<u8> {
    let mut bytes = fixture_rom();
    bytes[0x7fff] = 0x7e;
    let driver = [
        0x3a, 4, 0xde, 0xfe, 0x81, 0xc0, 0xaf, 0x32, 4, 0xde, 0x2a, 0, 0x41, 0x23, 0x23, 0x23,
        0x4e, 0x23, 0x23, 0x23, 0x5e, 0x23, 0x56, 0x23, 0x23, 0x23, 0x23, 0x23, 0xe5, 6, 3, 0x1a,
        0xd3, 0x7f, 0x13, 0x10, 0xfa, 0xe1, 0x0d, 0x20, 0xeb, 0xc9,
    ];
    bytes[0x8000..0x8000 + driver.len()].copy_from_slice(&driver);
    bytes[0x8140..0x8180].fill(0);
    bytes[0x8140..0x8146].copy_from_slice(&[0, 0, 0, 3, 1, 3]);
    for channel in 0..3_u16 {
        let entry = 0x8146 + usize::from(channel) * 6;
        let address = 0x4200 + channel * 0x10;
        bytes[entry..entry + 2].copy_from_slice(&address.to_le_bytes());
        bytes[entry + 2..entry + 6].copy_from_slice(&[
            12 + channel as u8,
            3 + channel as u8,
            0x80 + channel as u8,
            0x90 + channel as u8,
        ]);
    }
    bytes
}
