use super::{Profile, Selection};

pub(super) const PROFILE: Profile = Profile {
    name: "timer-paged-v2",
    hash: "ffeb6c8433f8d2bacceb833ec33d146ca0c2ddcd5980c2839a9c5416516c2c71",
    init: 0x1ae0,
    stop: 0x1b2e,
    select: 0x1b3e,
    tick: 0x1c8d,
    shadow: 0xae,
    compressed: false,
    selections: &[
        Selection {
            index: 1,
            entry: 0x3c002,
            module: 0x3c020,
            spans: &[(0x14de, 0xfbc), (0x3c000, 0x8b6)],
        },
        Selection {
            index: 2,
            entry: 0x3c004,
            module: 0x3c8b6,
            spans: &[(0x14de, 0xfa2), (0x3c000, 0xb36)],
        },
        Selection {
            index: 3,
            entry: 0x3c006,
            module: 0x3cb36,
            spans: &[(0x14de, 0xfa4), (0x3c000, 0xc89)],
        },
        Selection {
            index: 4,
            entry: 0x3c008,
            module: 0x3cc89,
            spans: &[(0x14de, 0xfc0), (0x3c000, 0x140b)],
        },
        Selection {
            index: 5,
            entry: 0x3c00a,
            module: 0x3d40b,
            spans: &[(0x14de, 0xfc2), (0x3c000, 0x1b9c)],
        },
        Selection {
            index: 6,
            entry: 0x3c00c,
            module: 0x3db9c,
            spans: &[(0x14de, 0xf94), (0x3c000, 0x1e2a)],
        },
        Selection {
            index: 7,
            entry: 0x3c00e,
            module: 0x3de2a,
            spans: &[(0x14de, 0xfb6), (0x3c000, 0x2208)],
        },
        Selection {
            index: 8,
            entry: 0x3c010,
            module: 0x3e208,
            spans: &[(0x14de, 0xf96), (0x3c000, 0x252a)],
        },
        Selection {
            index: 9,
            entry: 0x3c012,
            module: 0x3e52a,
            spans: &[(0x14de, 0xfba), (0x3c000, 0x2ce9)],
        },
        Selection {
            index: 10,
            entry: 0x3c014,
            module: 0x3ece9,
            spans: &[(0x14de, 0xf9e), (0x3c000, 0x2f75)],
        },
        Selection {
            index: 11,
            entry: 0x3c016,
            module: 0x3ef75,
            spans: &[(0x14de, 0xf96), (0x3c000, 0x3020)],
        },
        Selection {
            index: 12,
            entry: 0x3c018,
            module: 0x3f020,
            spans: &[(0x14de, 0xfb2), (0x3c000, 0x30fc)],
        },
        Selection {
            index: 13,
            entry: 0x3c01a,
            module: 0x3f0fc,
            spans: &[(0x14de, 0xf9e), (0x3c000, 0x3168)],
        },
        Selection {
            index: 14,
            entry: 0x3c01c,
            module: 0x3f168,
            spans: &[(0x14de, 0xfa4), (0x3c000, 0x3550)],
        },
        Selection {
            index: 15,
            entry: 0x3c01e,
            module: 0x3f550,
            spans: &[(0x14de, 0xfbc), (0x3c000, 0x3f39)],
        },
    ],
};
