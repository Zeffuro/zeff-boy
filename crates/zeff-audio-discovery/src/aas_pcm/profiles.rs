#[derive(Clone, Copy)]
pub(super) struct Profile {
    pub id: &'static str,
    pub sha256: &'static str,
    pub rom_len: usize,
    pub header: &'static [u8; 16],
    pub count: u16,
    pub table: usize,
    pub bank: usize,
    pub channel_zero: (u16, u16),
    pub handoff: (usize, usize),
    pub init: (usize, usize),
    pub play: (usize, usize),
    pub update: (usize, usize),
    pub timer1_irq: (usize, usize),
    pub irq_handler: (usize, usize),
    pub irq_address: u32,
    pub vblank_slot: u32,
    pub binding: (usize, usize),
    pub support: &'static [(usize, usize)],
}

pub(super) fn all() -> impl Iterator<Item = &'static Profile> {
    let profiles = PROFILES.iter();
    #[cfg(any(test, feature = "test-support"))]
    let profiles = profiles.chain([&super::fixture::PROFILE]);
    profiles
}

const PROFILES: &[Profile] = &[
    Profile {
        id: "aas-pcm-native-v1-01",
        sha256: "9a49fa18c9f8e76bfeeda3447ddccb7f57ec43e9ed1a70e318334bbf5c30571a",
        rom_len: 0x400000,
        header: b"MADDEN 2006\0B6ME",
        count: 46,
        table: 0x38a860,
        bank: 0xbb660,
        channel_zero: (35, 44),
        handoff: (0x100bc, 8),
        init: (0x16100, 0x34),
        play: (0xb9f08, 0x174),
        update: (0xb7478, 0xed2),
        timer1_irq: (0xb7378, 0x100),
        irq_handler: (0x1b0, 0x110),
        irq_address: 0x0300_51d0,
        vblank_slot: 0x0300_52ec,
        binding: (0x15df4, 0x1b4),
        support: &[
            (0xb6b80, 0x4ae0),
            (0x3eba00, 0xd0),
            (0x3ebe80, 2),
            (0x3ec480, 2),
        ],
    },
    Profile {
        id: "aas-pcm-native-v1-02",
        sha256: "c2b1cb5acda7b2afbacc982ecd2ed41c462a220133c9f994f19c0762c360aaee",
        rom_len: 0x400000,
        header: b"MADDEN 2007\0B7ME",
        count: 49,
        table: 0x38a584,
        bank: 0xbfce0,
        channel_zero: (35, 45),
        handoff: (0x11874, 8),
        init: (0x17af4, 0x34),
        play: (0xbe58c, 0x174),
        update: (0xbbafc, 0xed2),
        timer1_irq: (0xbb9fc, 0x100),
        irq_handler: (0x1b0, 0x110),
        irq_address: 0x0300_51fc,
        vblank_slot: 0x0300_5318,
        binding: (0x177c8, 0x1cc),
        support: &[
            (0xbb204, 0x4adc),
            (0x3ec448, 0xd0),
            (0x3ec8c8, 2),
            (0x3ecec8, 2),
        ],
    },
];
