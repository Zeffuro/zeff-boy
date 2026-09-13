use super::profiles::Profile;

pub(super) const PROFILE: Profile = Profile {
    id: "gbass-authored-fixture-v1",
    sha256: "48c4c85cec9bc7ee8a9e70fc3d5c0eaab93755fb562cc8b31f0c5f04c3d6fbc0",
    rom_len: 0x4000,
    header: b"GBASS FIXTURE001",
    schedule: super::GbassSchedule::VblankThenMain,
    hardware_started_before_handoff: false,
    guards: &[],
    config: 0x800,
    layout: super::separate::ConfigurationLayout::Unified,
    song_count: 2,
    instruments: 1,
    samples: 1,
    sample_steps: &[0x1_0000],
    sample_flags: &[0],
    bank: None,
    module: None,
    partial_warning: None,
    handoff: (0x200, 8),
    init: (0x300, 0x1c),
    start: (0x340, 2),
    play: (0x380, 0x20),
    vblank: (0x3c0, 2),
    update: (0x400, 0x30),
    wrapper: (0x400, 0x30),
    wait: (0x440, 4),
    irq: (0x500, 0x38),
    irq_address: 0x0800_0500,
    irq_table: (0x600, 4),
    irq_table_address: 0x0300_1200,
    state_address: 0x0300_1400,
    sample_data: (0xb00, 8),
    instrument_data: (0x9a8, 8),
    channel_data: (0x940, 16),
    title_data: (0x920, 0x1c),
    sequences: (0xa00, 8),
};

pub(super) const IRQ_PROFILE: Profile = Profile {
    id: "gbass-authored-irq-fixture-v1",
    sha256: "d34c294e6265e53b49af9c9b818dea3ee896a675927b0a758c93a173fe6eb84e",
    header: b"GBASS IRQ FIX001",
    schedule: super::GbassSchedule::VblankIrq,
    vblank: (0x3c0, 10),
    wrapper: (0x3c0, 10),
    ..PROFILE
};

pub(super) const STARTED_PROFILE: Profile = Profile {
    id: "gbass-authored-started-fixture-v1",
    sha256: "78939c61c0456d3562fefef5144d9b8f5ebd93b579b47bff434ce0b071ec074f",
    header: b"GBASS START 0001",
    hardware_started_before_handoff: true,
    sample_steps: &[0x1_0000, 0x2_0000, 0x2_031a],
    handoff: (0x204, 8),
    start: (0x340, 16),
    ..IRQ_PROFILE
};

pub(super) const PARTIAL_PROFILE: Profile = Profile {
    id: "gbass-authored-partial-fixture-v1",
    sha256: "53a99f9b2ae056f047b2002ec3cbcedba99b52ca6214b9c2daa81f6e5e89a5d9",
    header: b"GBASS PART 00001",
    partial_warning: Some("An additional authored music module is unqualified."),
    play: (0x700, 14),
    ..STARTED_PROFILE
};

const BANKED_PROFILE: Profile = Profile {
    id: "gbass-authored-banked-fixture-v1-0",
    sha256: "e958e1a5b9edb11aa020e8532c30d6611402014ab829472b77b134c5d47c0e41",
    header: b"GBASS BANK 00001",
    bank: Some(super::GbassBankSelector {
        index: 0,
        table: super::RomSpan {
            effective_offset: 0xc80,
            byte_len: 8,
            canonical_cpu_address: 0x0800_0c80,
        },
        configuration_address: 0x0300_1500,
    }),
    play: (0x700, 0x40),
    ..STARTED_PROFILE
};

pub(super) const BANKED_PROFILES: [Profile; 2] = [
    BANKED_PROFILE,
    Profile {
        id: "gbass-authored-banked-fixture-v1-1",
        config: 0xc00,
        bank: Some(super::GbassBankSelector {
            index: 1,
            table: super::RomSpan {
                effective_offset: 0xc80,
                byte_len: 8,
                canonical_cpu_address: 0x0800_0c80,
            },
            configuration_address: 0x0300_1500,
        }),
        channel_data: (0xd40, 16),
        title_data: (0xd20, 0x1d),
        sequences: (0xe00, 8),
        ..BANKED_PROFILE
    },
];

pub fn fixture_rom_banked() -> Vec<u8> {
    let mut bytes = fixture_rom_started();
    bytes[0xa0..0xb0].copy_from_slice(BANKED_PROFILES[0].header);
    write_words(
        &mut bytes,
        0x204,
        &[0x2100_2000, 0xfa7a_f000, 0xf918_f000, 0x46c0_e7fc],
    );
    write_words(
        &mut bytes,
        0x700,
        &[
            0x0089_b530,
            0x5852_4a0b,
            0x2414_4b0b,
            0xc320_ca20,
            0xd1fb_3c01,
            0x6892_4a08,
            0x1809_0041,
            0x1889_0089,
            0x6809_6849,
            0x6011_4a05,
            0x6050_2000,
            0xbc01_bc30,
            0x46c0_4700,
            0x0800_0c80,
            0x0300_1500,
            0x0300_1400,
        ],
    );
    bytes.copy_within(0x800..0x850, 0xc00);
    write_words(&mut bytes, 0xc08, &[0x0800_0d00]);
    write_words(&mut bytes, 0xc80, &[0x0800_0800, 0x0800_0c00]);
    write_words(
        &mut bytes,
        0xd00,
        &[1, 0x0800_0d40, 0x0800_0d20, 1, 0x0800_0d48, 0x0800_0d30],
    );
    bytes[0xd20..0xd2c].copy_from_slice(b"Bank Two One");
    bytes[0xd30..0xd3c].copy_from_slice(b"Bank Two Two");
    write_words(
        &mut bytes,
        0xd40,
        &[0x0800_0e00, 0x0100_0000, 0x0800_0e04, 0x0100_0000],
    );
    write_words(&mut bytes, 0xe00, &[0x87cf_87df, 0x877f_873f]);
    bytes
}

pub fn fixture_rom_partial() -> Vec<u8> {
    let mut bytes = fixture_rom_started();
    bytes[0xa0..0xb0].copy_from_slice(PARTIAL_PROFILE.header);
    write_words(&mut bytes, 0x206, &[0xfa7b_f000]);
    write_words(&mut bytes, 0x700, &[0xf7ff_b500, 0xf7ff_fe3d, 0xbc01_fe1b]);
    bytes[0x70c..0x70e].copy_from_slice(&0x4700_u16.to_le_bytes());
    write_words(&mut bytes, 0xc00, &[0x0200_2000, 0x0000_0001]);
    bytes
}

pub fn fixture_rom_started() -> Vec<u8> {
    let mut bytes = fixture_rom_irq();
    bytes[0xa0..0xb0].copy_from_slice(STARTED_PROFILE.header);
    write_words(
        &mut bytes,
        0x340,
        &[0x6801_4802, 0x6001_3101, 0x46c0_4770, 0x0300_1408],
    );
    write_words(&mut bytes, 0x984, &[0x2_031a]);
    bytes
}

pub fn fixture_rom_irq() -> Vec<u8> {
    let mut bytes = fixture_rom();
    bytes[0xa0..0xb0].copy_from_slice(IRQ_PROFILE.header);
    write_words(&mut bytes, 0x3c0, &[0xf000_b500, 0xbc01_f81d]);
    bytes[0x3c8..0x3ca].copy_from_slice(&0x4700_u16.to_le_bytes());
    bytes[0x20e..0x214].copy_from_slice(&[0xfc, 0xe7, 0xc0, 0x46, 0xc0, 0x46]);
    bytes
}

pub fn fixture_rom() -> Vec<u8> {
    let mut bytes = vec![0; PROFILE.rom_len];
    bytes[0xa0..0xb0].copy_from_slice(PROFILE.header);
    bytes[0xb2] = 0x96;
    write_words(&mut bytes, 0x0, &[0xea00002e]);
    write_words(
        &mut bytes,
        0xc0,
        &[
            0xe3a0c0d2, 0xe121f00c, 0xe59fd070, 0xe3a0c0df, 0xe121f00c, 0xe59fd068, 0xe59f0068,
            0xe59f1068, 0xe5801000, 0xe59f0064, 0xe5901000, 0xe59f0060, 0xe5801000, 0xe59f005c,
            0xe59f105c, 0xe1c010b0, 0xe59f0058, 0xe59f1058, 0xe1c010b0, 0xe59f0054, 0xe59f1054,
            0xe1c010b0, 0xe59f0050, 0xe59f1050, 0xe1c010b0, 0xe59fc04c, 0xe1a0e00f, 0xe12fff1c,
            0xe3a0c01f, 0xe121f00c, 0xe59fc03c, 0xe12fff1c, 0x03007fa0, 0x03007e00, 0x03007ffc,
            0x08000500, 0x08000600, 0x03001200, 0x04000200, 0x00000001, 0x04000202, 0x00003fff,
            0x04000004, 0x00000008, 0x04000208, 0x00000001, 0x08000301, 0x08000201,
        ],
    );
    write_words(
        &mut bytes,
        0x200,
        &[0xf89ef000, 0xf0002000, 0xf000f8bb, 0xf000f919, 0xe7faf8f7],
    );
    write_words(
        &mut bytes,
        0x300,
        &[
            0x20804903, 0x49037008, 0x80084803, 0x46c04770, 0x04000084, 0x04000080, 0x00001177,
        ],
    );
    write_words(&mut bytes, 0x340, &[0x00004770]);
    write_words(
        &mut bytes,
        0x380,
        &[
            0x18090041, 0x4a040089, 0x68491851, 0x4a036809, 0x20006011, 0x47706050, 0x08000900,
            0x03001400,
        ],
    );
    write_words(&mut bytes, 0x3c0, &[0x00004770]);
    write_words(
        &mut bytes,
        0x400,
        &[
            0x680a4907, 0x23026848, 0x60484058, 0x4a065a10, 0x80134b06, 0x80104a03, 0x46c04770,
            0x46c046c0, 0x03001400, 0x04000064, 0x04000062, 0x0000f080,
        ],
    );
    write_words(&mut bytes, 0x440, &[0x4770df05]);
    write_words(
        &mut bytes,
        0x500,
        &[
            0xe59f3024, 0xe5932000, 0xe0021822, 0xe1c310b2, 0xe59f2018, 0xe1d200b0, 0xe1800001,
            0xe1c200b0, 0xe59f000c, 0xe5900000, 0xe12fff10, 0x04000200, 0x03007ff8, 0x03001200,
        ],
    );
    write_words(&mut bytes, 0x600, &[0x080003c1]);
    write_words(
        &mut bytes,
        0x800,
        &[
            0x00000002, 0x00000002, 0x08000900, 0x00000001, 0x080009a0, 0x080009a4, 0x00000001,
            0x08000980, 0x00000004, 0x03001400, 0x03001500, 0x03001600, 0x00000002, 0x03001700,
            0x080009b0, 0x03001800, 0x03001900, 0x03001a00, 0x03001b00, 0x0000000f,
        ],
    );
    write_words(
        &mut bytes,
        0x900,
        &[
            0x00000001, 0x08000940, 0x08000920, 0x00000001, 0x08000948, 0x08000930,
        ],
    );
    write_words(
        &mut bytes,
        0x940,
        &[0x08000a00, 0x01000000, 0x08000a04, 0x01000000],
    );
    write_words(
        &mut bytes,
        0x980,
        &[
            0x08000b00, 0x00010000, 0x00000000, 0x00080000, 0x00000000, 0x00080000,
        ],
    );
    write_words(
        &mut bytes,
        0x9a0,
        &[0x080009a8, 0x00000004, 0x03020100, 0x07060504, 0x00000101],
    );
    write_words(&mut bytes, 0xa00, &[0x879f87bf, 0x871f875f]);
    write_words(&mut bytes, 0xb00, &[0x60402000, 0xa0c0e000]);
    bytes[0x920..0x92c].copy_from_slice(b"Fixture One\0");
    bytes[0x930..0x93c].copy_from_slice(b"Fixture Two\0");
    bytes
}

fn write_words(bytes: &mut [u8], offset: usize, words: &[u32]) {
    for (index, word) in words.iter().enumerate() {
        bytes[offset + index * 4..offset + index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
}
