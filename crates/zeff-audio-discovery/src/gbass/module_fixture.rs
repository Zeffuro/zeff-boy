use super::{GbassModule, GbassSchedule, RomSpan, fixture, profiles::Profile};

const BASE: Profile = Profile {
    id: "gbass-authored-module-fixture-v1-0",
    sha256: "6200d18bdb53d51d4b3bb9faadb11d34889e5457eded20d9c582ca5d6b4450f7",
    rom_len: 0x10000,
    header: b"GBASS MODULE0001",
    schedule: GbassSchedule::VblankThenMain,
    module: Some(GbassModule {
        index: 0,
        source: source(0x1000, 0x4000),
        load_address: 0x0200_0000,
        configuration_address: 0x0200_0800,
        loader: source(0x700, 0x30),
        loader_handoff: source(0x204, 8),
    }),
    config: 0x1800,
    handoff: (0x1204, 8),
    init: (0x1300, 0x1c),
    start: (0x1340, 16),
    play: (0x1380, 36),
    vblank: (0x13c0, 2),
    update: (0x1400, 0x30),
    wrapper: (0x1400, 0x30),
    wait: (0x1440, 4),
    irq: (0x1500, 0x38),
    irq_address: 0x0200_0500,
    irq_table: (0x1600, 4),
    sample_data: (0x1b00, 8),
    instrument_data: (0x19a8, 8),
    channel_data: (0x1940, 16),
    title_data: (0x1920, 0x1c),
    sequences: (0x1a00, 8),
    ..fixture::STARTED_PROFILE
};

pub(super) const PROFILES: [Profile; 2] = [
    BASE,
    Profile {
        id: "gbass-authored-module-fixture-v1-1",
        module: Some(GbassModule {
            index: 1,
            source: source(0x5000, 0x4000),
            ..BASE.module.unwrap()
        }),
        config: 0x5800,
        handoff: (0x5204, 8),
        init: (0x5300, 0x1c),
        start: (0x5340, 16),
        play: (0x5380, 36),
        vblank: (0x53c0, 2),
        update: (0x5400, 0x30),
        wrapper: (0x5400, 0x30),
        wait: (0x5440, 4),
        irq: (0x5500, 0x38),
        irq_table: (0x5600, 4),
        sample_data: (0x5b00, 8),
        instrument_data: (0x59a8, 8),
        channel_data: (0x5940, 16),
        title_data: (0x5920, 0x1c),
        sequences: (0x5a00, 8),
        ..BASE
    },
];

pub fn fixture_rom_module() -> Vec<u8> {
    let mut bytes = fixture::fixture_rom_started();
    let mut module = bytes.clone();
    for word in module.as_chunks_mut::<4>().0 {
        let value = u32::from_le_bytes(*word);
        if (0x0800_0000..0x0800_4000).contains(&value) {
            word.copy_from_slice(&(value - 0x0600_0000).to_le_bytes());
        }
    }
    words(&mut module, 0x3c0, &[0x0000_4770, 0, 0]);
    words(
        &mut module,
        0x380,
        &[
            0x1809_0041,
            0x4a05_0089,
            0x1851_6892,
            0x6809_6849,
            0x6011_4a03,
            0x6050_2000,
            0x46c0_4770,
            0x0200_0800,
            0x0300_1400,
        ],
    );
    bytes.resize(BASE.rom_len, 0);
    bytes[0xa0..0xb0].copy_from_slice(BASE.header);
    words(
        &mut bytes,
        0x700,
        &[
            0x46c0_4778,
            0xe59f_101c,
            0xe581_7000,
            0xe59f_0018,
            0xe581_0004,
            0xe1a0_2126,
            0xe382_2484,
            0xe581_2008,
            0xe591_2008,
            0xe12f_ff10,
            0x0400_00d4,
            0x0200_0000,
        ],
    );
    bytes[0x1000..0x5000].copy_from_slice(&module);
    words(&mut module, 0xa00, &[0x87cf_87df, 0x877f_873f]);
    bytes[0x5000..0x9000].copy_from_slice(&module);
    bytes
}

const fn source(offset: u32, len: u32) -> RomSpan {
    RomSpan {
        effective_offset: offset,
        byte_len: len,
        canonical_cpu_address: 0x0800_0000 + offset,
    }
}

fn words(bytes: &mut [u8], offset: usize, values: &[u32]) {
    for (index, value) in values.iter().enumerate() {
        bytes[offset + index * 4..offset + index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
}
