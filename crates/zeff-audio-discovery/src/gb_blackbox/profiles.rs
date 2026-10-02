use crate::{Budget, ScanStop};

pub(super) struct Selection {
    pub index: u16,
    pub bank: u16,
    pub module: u16,
    pub initial: u8,
    pub restart: u8,
    pub entry_bank: u16,
    pub entry: u16,
    pub entry_len: u16,
    pub data_len: u16,
}

pub(super) struct Profile {
    pub name: &'static str,
    pub hash: &'static str,
    pub init: u16,
    pub selections: &'static [Selection],
}

const PROFILES: [Profile; 3] = [
    Profile {
        name: "blackbox-four-channel-v1",
        hash: "2079bf9fc139ba2ef43d7a9a72ae284b8900880a5be94d7852deec45d69aaf63",
        init: 0x386c,
        selections: &[
            Selection {
                index: 1,
                bank: 14,
                module: 0x6000,
                initial: 0,
                restart: 0,
                entry_bank: 16,
                entry: 0x4003,
                entry_len: 3,
                data_len: 0x1d80,
            },
            Selection {
                index: 2,
                bank: 15,
                module: 0x4000,
                initial: 0,
                restart: 0,
                entry_bank: 16,
                entry: 0x4006,
                entry_len: 3,
                data_len: 0x1c80,
            },
            Selection {
                index: 3,
                bank: 15,
                module: 0x6000,
                initial: 0,
                restart: 0,
                entry_bank: 16,
                entry: 0x4009,
                entry_len: 3,
                data_len: 0x1f00,
            },
        ],
    },
    Profile {
        name: "blackbox-four-channel-v2",
        hash: "7027441b7ead92dcb5e931c8a3ac5608611717e2dafdab4741dca108e1a33027",
        init: 0x3c7a,
        selections: &[
            Selection {
                index: 1,
                bank: 31,
                module: 0x4bc3,
                initial: 0,
                restart: 0,
                entry_bank: 0,
                entry: 0x2b6e,
                entry_len: 2,
                data_len: 0x09ff,
            },
            Selection {
                index: 2,
                bank: 32,
                module: 0x4000,
                initial: 0,
                restart: 0,
                entry_bank: 0,
                entry: 0x2b70,
                entry_len: 2,
                data_len: 0x077f,
            },
            Selection {
                index: 3,
                bank: 32,
                module: 0x6000,
                initial: 0,
                restart: 0,
                entry_bank: 0,
                entry: 0x2b72,
                entry_len: 2,
                data_len: 0x087f,
            },
            Selection {
                index: 4,
                bank: 33,
                module: 0x4000,
                initial: 0,
                restart: 0,
                entry_bank: 0,
                entry: 0x2b74,
                entry_len: 2,
                data_len: 0x06ff,
            },
            Selection {
                index: 5,
                bank: 33,
                module: 0x6000,
                initial: 0,
                restart: 0,
                entry_bank: 0,
                entry: 0x2b76,
                entry_len: 2,
                data_len: 0x06ff,
            },
            Selection {
                index: 6,
                bank: 34,
                module: 0x4000,
                initial: 0,
                restart: 0,
                entry_bank: 0,
                entry: 0x2b78,
                entry_len: 2,
                data_len: 0x0600,
            },
        ],
    },
    Profile {
        name: "blackbox-four-channel-v3",
        hash: "37cd42559ea48780f707e44d3ce44c2ab2c2506034151826561df635aa2fea28",
        init: 0x3198,
        selections: &[
            Selection {
                index: 0,
                bank: 19,
                module: 0x4000,
                initial: 13,
                restart: 13,
                entry_bank: 0,
                entry: 0x2de7,
                entry_len: 29,
                data_len: 0x1300,
            },
            Selection {
                index: 1,
                bank: 19,
                module: 0x4000,
                initial: 4,
                restart: 4,
                entry_bank: 0,
                entry: 0x2e04,
                entry_len: 29,
                data_len: 0x0900,
            },
            Selection {
                index: 2,
                bank: 19,
                module: 0x4000,
                initial: 0,
                restart: 1,
                entry_bank: 0,
                entry: 0x2e21,
                entry_len: 29,
                data_len: 0x1280,
            },
        ],
    },
];

pub(super) fn named(name: &str) -> Option<&'static Profile> {
    #[cfg(any(test, feature = "test-support"))]
    if name == super::tests::PROFILE.name {
        return Some(&super::tests::PROFILE);
    }
    PROFILES.iter().find(|profile| profile.name == name)
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
    if hash == zeff_firmware::sha256_hex(&super::tests::synthetic_rom()) {
        return Ok(Some(&super::tests::PROFILE));
    }
    Ok(PROFILES.iter().find(|profile| profile.hash == hash))
}
