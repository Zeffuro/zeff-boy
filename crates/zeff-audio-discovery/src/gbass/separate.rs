use super::{GbassSchedule, GbassStartupGuard, profiles::Profile, word};

#[derive(Clone, Copy)]
pub(super) enum ConfigurationLayout {
    Unified,
    SeparatePcm,
}

impl ConfigurationLayout {
    pub fn byte_len(self) -> usize {
        match self {
            Self::Unified => 80,
            Self::SeparatePcm => 84,
        }
    }

    pub fn table_shift(self, bytes: &[u8], config: usize) -> Option<usize> {
        match self {
            Self::Unified => Some(0),
            Self::SeparatePcm => {
                // This revision has an empty PSG table preceding its PCM instruments.
                (word(bytes, config + 12)? == 0
                    && word(bytes, config + 16)? == word(bytes, config + 24)?)
                .then_some(8)
            }
        }
    }
}

pub(super) const PROFILES: &[Profile] = &[Profile {
    id: "gbass-native-split-instruments-01",
    sha256: "506a3b9d2f5ed6af8ee3150172b8b467fee34389c1ba6bbff0e81bbab5b9adb4",
    rom_len: 0x400000,
    header: b"DAVID BECKHAABQE",
    schedule: GbassSchedule::VblankIrq,
    hardware_started_before_handoff: true,
    guards: &[
        GbassStartupGuard {
            address: 0x0200_2c04,
            byte_len: 1,
            value: 0,
        },
        GbassStartupGuard {
            address: 0x0200_080c,
            byte_len: 1,
            value: 0,
        },
        GbassStartupGuard {
            address: 0x0200_4183,
            byte_len: 1,
            value: 0,
        },
        GbassStartupGuard {
            address: 0x0200_145f,
            byte_len: 1,
            value: 0,
        },
    ],
    config: 0x219438,
    layout: ConfigurationLayout::SeparatePcm,
    song_count: 7,
    instruments: 65,
    samples: 52,
    sample_steps: &[0x1_0000],
    sample_flags: &[0],
    bank: None,
    module: None,
    partial_warning: None,
    handoff: (0x626, 10),
    init: (0x15720, 0xf4),
    start: (0x15944, 0x74),
    play: (0x15888, 0xb0),
    vblank: (0x152c0, 0x30),
    update: (0x14fb4, 0x30c),
    wrapper: (0x690, 0x104),
    wait: (0x834, 0x14),
    irq: (0xfc, 0xcc),
    irq_address: 0x0800_00fc,
    irq_table: (0x17a2c, 0x38),
    irq_table_address: 0x0300_0000,
    state_address: 0x0200_46e0,
    sample_data: (0x1bdbec, 0x52a6b),
    instrument_data: (0x210c7d, 0x437),
    channel_data: (0x21114c, 0x1c0),
    title_data: (0x211108, 0x44),
    sequences: (0x21130c, 0x8128),
}];
