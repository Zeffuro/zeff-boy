use serde::Serialize;

use super::{
    GbassSchedule, GbassStartupGuard, RomSpan, contains, intersects, profiles::Profile, span,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GbassModule {
    pub index: u16,
    pub source: RomSpan,
    pub load_address: u32,
    pub configuration_address: u32,
    pub loader: RomSpan,
    pub loader_handoff: RomSpan,
}

impl GbassModule {
    pub(super) fn runtime_address(self, source: RomSpan) -> Option<u32> {
        contains(self.source, source)
            .then(|| self.load_address + source.effective_offset - self.source.effective_offset)
    }

    pub(super) fn source_span(self, bytes: &[u8], address: u32, len: usize) -> Option<RomSpan> {
        let offset = address.checked_sub(self.load_address)? as usize;
        if offset.checked_add(len)? > self.source.byte_len as usize {
            return None;
        }
        span(bytes, self.source.effective_offset as usize + offset, len)
    }
}

pub(super) fn validate(bytes: &[u8], profile: &Profile, root: RomSpan) -> Option<()> {
    let Some(module) = profile.module else {
        return Some(());
    };
    if profile.bank.is_some()
        || module.load_address != 0x0200_0000
        || module.source.byte_len > 0x40000
        || module.runtime_address(root)? != module.configuration_address
    {
        return None;
    }
    for source in [module.source, module.loader, module.loader_handoff] {
        if span(
            bytes,
            source.effective_offset as usize,
            source.byte_len as usize,
        )? != source
        {
            return None;
        }
    }
    if intersects(module.source, module.loader)
        || intersects(module.source, module.loader_handoff)
        || intersects(module.loader, module.loader_handoff)
    {
        return None;
    }
    for (offset, len) in [
        profile.handoff,
        profile.init,
        profile.start,
        profile.play,
        profile.vblank,
        profile.update,
        profile.wrapper,
        profile.wait,
        profile.irq,
        profile.irq_table,
    ] {
        module.runtime_address(span(bytes, offset, len)?)?;
    }
    Some(())
}

const fn rom_span(offset: u32, len: u32) -> RomSpan {
    RomSpan {
        effective_offset: offset,
        byte_len: len,
        canonical_cpu_address: 0x0800_0000 + offset,
    }
}

const BASE: Profile = Profile {
    id: "gbass-native-v1-05-module-0",
    sha256: "db34774ab36489f8a663033dd29698567615d13623f37ac0b61065cf24c372ea",
    rom_len: 0x400000,
    header: b"RR GO PARTY\0AR5E",
    schedule: GbassSchedule::VblankThenMain,
    hardware_started_before_handoff: true,
    song_count: 2,
    instruments: 31,
    samples: 3,
    layout: super::separate::ConfigurationLayout::Unified,
    sample_steps: &[0x1_0000],
    sample_flags: &[0],
    bank: None,
    partial_warning: None,
    state_address: 0x0300_05b4,
    irq_address: 0x0200_003c,
    module: Some(GbassModule {
        index: 0,
        source: rom_span(0x293e80, 0x1efbc),
        load_address: 0x0200_0000,
        configuration_address: 0x0201ef6c,
        loader: rom_span(0x54a0, 0x3e),
        loader_handoff: rom_span(0x4c6, 10),
    }),
    guards: &[
        GbassStartupGuard {
            address: 0x0201f0da,
            byte_len: 2,
            value: 1,
        },
        GbassStartupGuard {
            address: 0x0201f014,
            byte_len: 4,
            value: 2,
        },
        GbassStartupGuard {
            address: 0x0201efd8,
            byte_len: 2,
            value: 0,
        },
        GbassStartupGuard {
            address: 0x0201f020,
            byte_len: 2,
            value: 1,
        },
        GbassStartupGuard {
            address: 0x02021988,
            byte_len: 2,
            value: 0,
        },
    ],
    config: 0x2b2dec,
    handoff: (0x29ee94, 0x8),
    init: (0x2a7170, 0x10),
    start: (0x2a77e8, 0x10),
    play: (0x2a403c, 0x5c),
    vblank: (0x2a6e04, 0x34),
    update: (0x2a6758, 0x6ac),
    wrapper: (0x2a3fec, 0x28),
    wait: (0x2a923c, 0x8),
    irq: (0x293ebc, 0x114),
    irq_table: (0x2a9bbc, 0x3c),
    irq_table_address: 0x0202223c,
    sample_data: (0x2acc58, 0x3b0a),
    instrument_data: (0x2b0847, 0x3b1),
    channel_data: (0x2b0c24, 0x80),
    title_data: (0x2b0c10, 0x12),
    sequences: (0x2b0ca4, 0x2144),
};

pub(super) const PROFILES: &[Profile] = &[
    BASE,
    Profile {
        id: "gbass-native-v1-05-module-1",
        module: Some(GbassModule {
            index: 1,
            source: rom_span(0x2b2e40, 0x1f11c),
            load_address: 0x0200_0000,
            configuration_address: 0x0201f0cc,
            loader: rom_span(0x54a0, 0x3e),
            loader_handoff: rom_span(0x4c6, 10),
        }),
        guards: &[
            GbassStartupGuard {
                address: 0x0201f23a,
                byte_len: 2,
                value: 1,
            },
            GbassStartupGuard {
                address: 0x0201f174,
                byte_len: 4,
                value: 2,
            },
            GbassStartupGuard {
                address: 0x0201f138,
                byte_len: 2,
                value: 0,
            },
            GbassStartupGuard {
                address: 0x0201f180,
                byte_len: 2,
                value: 1,
            },
            GbassStartupGuard {
                address: 0x02021c84,
                byte_len: 2,
                value: 0,
            },
        ],
        config: 0x2d1f0c,
        handoff: (0x2bdc28, 0x8),
        init: (0x2c5f04, 0x10),
        start: (0x2c657c, 0x10),
        play: (0x2c2dd0, 0x5c),
        vblank: (0x2c5b98, 0x34),
        update: (0x2c54ec, 0x6ac),
        wrapper: (0x2c2d80, 0x28),
        wait: (0x2c7fd0, 0x8),
        irq: (0x2b2e7c, 0x114),
        irq_table: (0x2c8cdc, 0x3c),
        irq_table_address: 0x02022538,
        sample_data: (0x2cbd78, 0x3b0a),
        instrument_data: (0x2cf967, 0x3b1),
        channel_data: (0x2cfd44, 0x80),
        title_data: (0x2cfd30, 0x12),
        sequences: (0x2cfdc4, 0x2144),
        ..BASE
    },
    Profile {
        id: "gbass-native-v1-05-module-2",
        module: Some(GbassModule {
            index: 2,
            source: rom_span(0x2d1f60, 0x220a4),
            load_address: 0x0200_0000,
            configuration_address: 0x02022054,
            loader: rom_span(0x54a0, 0x3e),
            loader_handoff: rom_span(0x4c6, 10),
        }),
        guards: &[
            GbassStartupGuard {
                address: 0x020221c2,
                byte_len: 2,
                value: 1,
            },
            GbassStartupGuard {
                address: 0x020220fc,
                byte_len: 4,
                value: 2,
            },
            GbassStartupGuard {
                address: 0x020220c0,
                byte_len: 2,
                value: 0,
            },
            GbassStartupGuard {
                address: 0x02022108,
                byte_len: 2,
                value: 1,
            },
            GbassStartupGuard {
                address: 0x02024a70,
                byte_len: 2,
                value: 0,
            },
        ],
        config: 0x2f3fb4,
        handoff: (0x2e007c, 0x8),
        init: (0x2e8358, 0x10),
        start: (0x2e89d0, 0x10),
        play: (0x2e5224, 0x5c),
        vblank: (0x2e7fec, 0x34),
        update: (0x2e7940, 0x6ac),
        wrapper: (0x2e51d4, 0x28),
        wait: (0x2ea424, 0x8),
        irq: (0x2d1f9c, 0x114),
        irq_table: (0x2ead84, 0x3c),
        irq_table_address: 0x02025324,
        sample_data: (0x2ede20, 0x3b0a),
        instrument_data: (0x2f1a0f, 0x3b1),
        channel_data: (0x2f1dec, 0x80),
        title_data: (0x2f1dd8, 0x12),
        sequences: (0x2f1e6c, 0x2144),
        ..BASE
    },
    Profile {
        id: "gbass-native-v1-05-module-3",
        module: Some(GbassModule {
            index: 3,
            source: rom_span(0x2f4020, 0x1f588),
            load_address: 0x0200_0000,
            configuration_address: 0x0201f538,
            loader: rom_span(0x54a0, 0x3e),
            loader_handoff: rom_span(0x4c6, 10),
        }),
        guards: &[
            GbassStartupGuard {
                address: 0x0201f6a6,
                byte_len: 2,
                value: 1,
            },
            GbassStartupGuard {
                address: 0x0201f5e0,
                byte_len: 4,
                value: 2,
            },
            GbassStartupGuard {
                address: 0x0201f5a4,
                byte_len: 2,
                value: 0,
            },
            GbassStartupGuard {
                address: 0x0201f5ec,
                byte_len: 2,
                value: 1,
            },
            GbassStartupGuard {
                address: 0x02021f54,
                byte_len: 2,
                value: 0,
            },
        ],
        config: 0x313558,
        handoff: (0x2ff600, 0x8),
        init: (0x3078dc, 0x10),
        start: (0x307f54, 0x10),
        play: (0x3047a8, 0x5c),
        vblank: (0x307570, 0x34),
        update: (0x306ec4, 0x6ac),
        wrapper: (0x304758, 0x28),
        wait: (0x3099a8, 0x8),
        irq: (0x2f405c, 0x114),
        irq_table: (0x30a328, 0x3c),
        irq_table_address: 0x02022808,
        sample_data: (0x30d3c4, 0x3b0a),
        instrument_data: (0x310fb3, 0x3b1),
        channel_data: (0x311390, 0x80),
        title_data: (0x31137c, 0x12),
        sequences: (0x311410, 0x2144),
        ..BASE
    },
    Profile {
        id: "gbass-native-v1-05-module-4",
        module: Some(GbassModule {
            index: 4,
            source: rom_span(0x3135c0, 0x20860),
            load_address: 0x0200_0000,
            configuration_address: 0x02020810,
            loader: rom_span(0x54a0, 0x3e),
            loader_handoff: rom_span(0x4c6, 10),
        }),
        guards: &[
            GbassStartupGuard {
                address: 0x0202097e,
                byte_len: 2,
                value: 1,
            },
            GbassStartupGuard {
                address: 0x020208b8,
                byte_len: 4,
                value: 2,
            },
            GbassStartupGuard {
                address: 0x0202087c,
                byte_len: 2,
                value: 0,
            },
            GbassStartupGuard {
                address: 0x020208c4,
                byte_len: 2,
                value: 1,
            },
            GbassStartupGuard {
                address: 0x0202322c,
                byte_len: 2,
                value: 0,
            },
        ],
        config: 0x333dd0,
        handoff: (0x31fe78, 0x8),
        init: (0x328154, 0x10),
        start: (0x3287cc, 0x10),
        play: (0x325020, 0x5c),
        vblank: (0x327de8, 0x34),
        update: (0x32773c, 0x6ac),
        wrapper: (0x324fd0, 0x28),
        wait: (0x32a220, 0x8),
        irq: (0x3135fc, 0x114),
        irq_table: (0x32aba0, 0x3c),
        irq_table_address: 0x02023ae0,
        sample_data: (0x32dc3c, 0x3b0a),
        instrument_data: (0x33182b, 0x3b1),
        channel_data: (0x331c08, 0x80),
        title_data: (0x331bf4, 0x12),
        sequences: (0x331c88, 0x2144),
        ..BASE
    },
];
