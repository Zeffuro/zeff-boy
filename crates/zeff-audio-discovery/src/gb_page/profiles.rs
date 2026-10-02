use crate::{Budget, ScanStop};
pub(super) struct Selection {
    pub index: u16,
    pub entry: u16,
    pub bank: u16,
    pub module: u16,
    pub spans: &'static [(u16, u16, u16)],
}
pub(super) struct Profile {
    pub name: &'static str,
    pub hash: &'static str,
    pub setup: u16,
    pub stop: u16,
    pub start: u16,
    pub tick: u16,
    pub page: u16,
    pub wrapper: u16,
    pub double: bool,
    pub protected: &'static [(u16, u16)],
    pub selections: &'static [Selection],
}
const PROFILES: &[Profile] = &[
    Profile {
        protected: &[(0x26d5, 0x312b)],
        name: "page-four-channel-v1",
        hash: "02928ef4a0ce60b75078dc03ae72733d192d050b46d366aacd628b9e1971ee36",
        setup: 0x2edc,
        stop: 0x2f9b,
        start: 0x2f03,
        tick: 0x2fac,
        page: 0xcd00,
        wrapper: 0xc680,
        double: true,
        selections: &[
            Selection {
                index: 1,
                entry: 0x3110,
                bank: 3,
                module: 0x4000,
                spans: &[(0x0, 0x26d5, 0x0a3e), (0x3, 0x4000, 0x0290)],
            },
            Selection {
                index: 2,
                entry: 0x3113,
                bank: 3,
                module: 0x4290,
                spans: &[(0x0, 0x26d5, 0x0a41), (0x3, 0x4290, 0x039f)],
            },
            Selection {
                index: 3,
                entry: 0x3116,
                bank: 3,
                module: 0x462f,
                spans: &[(0x0, 0x26d5, 0x0a44), (0x3, 0x462f, 0x02c7)],
            },
            Selection {
                index: 4,
                entry: 0x3119,
                bank: 3,
                module: 0x48f6,
                spans: &[(0x0, 0x26d5, 0x0a47), (0x3, 0x48f6, 0x029c)],
            },
            Selection {
                index: 5,
                entry: 0x311c,
                bank: 3,
                module: 0x4b92,
                spans: &[(0x0, 0x26d5, 0x0a4a), (0x3, 0x4b92, 0x02b3)],
            },
            Selection {
                index: 6,
                entry: 0x311f,
                bank: 2,
                module: 0x5dc7,
                spans: &[(0x0, 0x26d5, 0x0a4d), (0x2, 0x5dc7, 0x023a)],
            },
            Selection {
                index: 7,
                entry: 0x3122,
                bank: 2,
                module: 0x6001,
                spans: &[(0x0, 0x26d5, 0x0a50), (0x2, 0x6001, 0x00c5)],
            },
            Selection {
                index: 8,
                entry: 0x3125,
                bank: 2,
                module: 0x60c6,
                spans: &[(0x0, 0x26d5, 0x0a53), (0x2, 0x60c6, 0x00d6)],
            },
            Selection {
                index: 9,
                entry: 0x3128,
                bank: 2,
                module: 0x619c,
                spans: &[(0x0, 0x26d5, 0x0a56), (0x2, 0x619c, 0x00dd)],
            },
        ],
    },
    Profile {
        protected: &[(0x04c4, 0x2f64)],
        name: "page-four-channel-v2",
        hash: "0430cbbcdd56b693d40ab7b424603c639d98d5ec97e91e17ce1ede766104e765",
        setup: 0x2e85,
        stop: 0x2f44,
        start: 0x2eac,
        tick: 0x2f55,
        page: 0xcd00,
        wrapper: 0xc8a7,
        double: true,
        selections: &[
            Selection {
                index: 1,
                entry: 0x04c4,
                bank: 13,
                module: 0x6d26,
                spans: &[(0x0, 0x04c4, 0x2aa0), (0xd, 0x6d26, 0x0b08)],
            },
            Selection {
                index: 2,
                entry: 0x04c7,
                bank: 14,
                module: 0x4000,
                spans: &[(0x0, 0x04c7, 0x2a9d), (0xe, 0x4000, 0x0b08)],
            },
            Selection {
                index: 3,
                entry: 0x04ca,
                bank: 14,
                module: 0x4b08,
                spans: &[(0x0, 0x04ca, 0x2a9a), (0xe, 0x4b08, 0x0b08)],
            },
            Selection {
                index: 4,
                entry: 0x04cd,
                bank: 15,
                module: 0x4a1f,
                spans: &[(0x0, 0x04cd, 0x2a97), (0xf, 0x4a1f, 0x0a08)],
            },
            Selection {
                index: 5,
                entry: 0x04d0,
                bank: 15,
                module: 0x5427,
                spans: &[(0x0, 0x04d0, 0x2a94), (0xf, 0x5427, 0x0a08)],
            },
            Selection {
                index: 6,
                entry: 0x04d3,
                bank: 15,
                module: 0x7235,
                spans: &[(0x0, 0x04d3, 0x2a91), (0xf, 0x7235, 0x0988)],
            },
            Selection {
                index: 7,
                entry: 0x04d6,
                bank: 16,
                module: 0x4000,
                spans: &[(0x0, 0x04d6, 0x2a8e), (0x10, 0x4000, 0x0988)],
            },
            Selection {
                index: 8,
                entry: 0x04d9,
                bank: 16,
                module: 0x4988,
                spans: &[(0x0, 0x04d9, 0x2a8b), (0x10, 0x4988, 0x0988)],
            },
            Selection {
                index: 9,
                entry: 0x04ee,
                bank: 18,
                module: 0x50c9,
                spans: &[(0x0, 0x04ee, 0x2a76), (0x12, 0x50c9, 0x0808)],
            },
            Selection {
                index: 10,
                entry: 0x04f1,
                bank: 18,
                module: 0x58d1,
                spans: &[(0x0, 0x04f1, 0x2a73), (0x12, 0x58d1, 0x0808)],
            },
            Selection {
                index: 11,
                entry: 0x04f4,
                bank: 18,
                module: 0x60d9,
                spans: &[(0x0, 0x04f4, 0x2a70), (0x12, 0x60d9, 0x0808)],
            },
            Selection {
                index: 12,
                entry: 0x0500,
                bank: 20,
                module: 0x73ef,
                spans: &[(0x0, 0x0500, 0x2a64), (0x14, 0x73ef, 0x0588)],
            },
            Selection {
                index: 13,
                entry: 0x04fa,
                bank: 19,
                module: 0x4000,
                spans: &[(0x0, 0x04fa, 0x2a6a), (0x13, 0x4000, 0x0708)],
            },
            Selection {
                index: 14,
                entry: 0x050c,
                bank: 24,
                module: 0x6b20,
                spans: &[(0x0, 0x050c, 0x2a58), (0x18, 0x6b20, 0x0388)],
            },
        ],
    },
    Profile {
        protected: &[(0x1bdf, 0x20be)],
        name: "page-four-channel-v3",
        hash: "3efc2e63eba0239925554afa45499964b7be7325bdd0fd6a1befe517cfd0bd4f",
        setup: 0x1bdf,
        stop: 0x1d9f,
        start: 0x1d9b,
        tick: 0x1d9d,
        page: 0xcb00,
        wrapper: 0x0000,
        double: false,
        selections: &[
            Selection {
                index: 1,
                entry: 0x1bf8,
                bank: 13,
                module: 0x4000,
                spans: &[(0x0, 0x1bdf, 0x04df), (0xd, 0x4001, 0x0a07)],
            },
            Selection {
                index: 2,
                entry: 0x1bfe,
                bank: 13,
                module: 0x4a08,
                spans: &[(0x0, 0x1bdf, 0x04df), (0xd, 0x4a09, 0x0a87)],
            },
            Selection {
                index: 3,
                entry: 0x1c05,
                bank: 13,
                module: 0x5490,
                spans: &[(0x0, 0x1bdf, 0x04df), (0xd, 0x5491, 0x0b07)],
            },
            Selection {
                index: 4,
                entry: 0x1c0c,
                bank: 13,
                module: 0x5f98,
                spans: &[(0x0, 0x1bdf, 0x04df), (0xd, 0x5f99, 0x0c07)],
            },
            Selection {
                index: 5,
                entry: 0x0e1e,
                bank: 14,
                module: 0x6fb9,
                spans: &[(0x0, 0x1bdf, 0x04df), (0xe, 0x6fba, 0x0b07)],
            },
        ],
    },
];
pub(super) fn named(name: &str) -> Option<&'static Profile> {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(p) = [&super::tests::PROFILE, &super::tests::NORMAL]
        .into_iter()
        .find(|p| p.name == name)
    {
        return Some(p);
    }
    PROFILES.iter().find(|p| p.name == name)
}
pub(super) fn recognized(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Option<&'static Profile>, ScanStop> {
    budget.charge()?;
    if !super::supports_cartridge(bytes) {
        return Ok(None);
    }
    let hash = zeff_firmware::sha256_hex(bytes);
    budget.charge()?;
    #[cfg(any(test, feature = "test-support"))]
    for (p, rom) in [
        (&super::tests::PROFILE, super::tests::synthetic_rom()),
        (&super::tests::NORMAL, super::tests::synthetic_normal_rom()),
    ] {
        if hash == zeff_firmware::sha256_hex(&rom) {
            return Ok(Some(p));
        }
    }
    Ok(PROFILES.iter().find(|p| p.hash == hash))
}
