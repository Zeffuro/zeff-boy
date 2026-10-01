use crate::{Budget, ScanStop};

pub(super) struct Check {
    pub(super) bank: Option<u16>,
    pub(super) start: u16,
    pub(super) end: u16,
    pub(super) hash: &'static str,
}

#[derive(Clone, Copy)]
pub(super) struct Profile {
    pub(super) name: &'static str,
    pub(super) init: u16,
    pub(super) tick: u16,
    pub(super) state_start: u16,
    pub(super) state_end: u16,
    pub(super) sfx_flag: u16,
    pub(super) initial_mask: Option<u16>,
    pub(super) double_speed: bool,
    pub(super) wram_bank: u8,
    pub(super) data_start: u16,
    pub(super) extra_banks: &'static [u16],
    pub(super) checks: &'static [Check],
}

#[derive(Clone, Copy)]
pub(super) struct Recognition {
    pub(super) bank: u16,
    pub(super) profile: &'static Profile,
}

const PROFILES: [Profile; 5] = [
    Profile {
        name: "imed-fixed-v1",
        init: 0x367d,
        tick: 0x3750,
        state_start: 0xdf54,
        state_end: 0xdfb1,
        sfx_flag: 0xdfb1,
        initial_mask: None,
        double_speed: false,
        wram_bank: 1,
        data_start: 0x4000,
        extra_banks: &[10],
        checks: &[
            Check {
                bank: Some(0),
                start: 0x3668,
                end: 0x3ee1,
                hash: "a3bd8ab41ca20681c284dd07627bb40fbffa6eeed82bbc9f8c1829cfc08183ce",
            },
            Check {
                bank: Some(10),
                start: 0x6edc,
                end: 0x6f02,
                hash: "7c98eaa06f05fd54659cdb1343b0847e03a5680a4325bce4bab9fc5a37318c46",
            },
        ],
    },
    Profile {
        name: "imed-banked-v2",
        init: 0x4050,
        tick: 0x4174,
        state_start: 0xd020,
        state_end: 0xd081,
        sfx_flag: 0xd000,
        initial_mask: None,
        double_speed: true,
        wram_bank: 4,
        data_start: 0x4fc0,
        extra_banks: &[],
        checks: &[Check {
            bank: None,
            start: 0x4000,
            end: 0x4fc0,
            hash: "8bf985f08ca813f1becdf6eabe7e12ccb7ec18bb4285a0b65643f53ccc717f1f",
        }],
    },
    Profile {
        name: "imed-banked-v3",
        init: 0x403b,
        tick: 0x415f,
        state_start: 0xdf20,
        state_end: 0xdf81,
        sfx_flag: 0xdf00,
        initial_mask: Some(0xdf72),
        double_speed: true,
        wram_bank: 7,
        data_start: 0x501e,
        extra_banks: &[],
        checks: &[Check {
            bank: None,
            start: 0x4000,
            end: 0x501e,
            hash: "2306e7d85d1137cdef272e9c2be08e43c03777b15f288dd636d8b29bd1180e6a",
        }],
    },
    Profile {
        name: "imed-banked-v4",
        init: 0x4032,
        tick: 0x40f2,
        state_start: 0xd020,
        state_end: 0xd0c7,
        sfx_flag: 0xd000,
        initial_mask: None,
        double_speed: true,
        wram_bank: 2,
        data_start: 0x4880,
        extra_banks: &[],
        checks: &[
            Check {
                bank: None,
                start: 0x4004,
                end: 0x4880,
                hash: "151c68ae3e8d53d03e9bb5e7dcbe986a6a731f11a0a37cbcf0dff10d79601278",
            },
            Check {
                bank: None,
                start: 0x6a69,
                end: 0x6a8f,
                hash: "275eb043faf0ad98b739c6af5be0a80ac699ff7123f5fecacfc746b8f43a46cb",
            },
            Check {
                bank: None,
                start: 0x6cc4,
                end: 0x6d34,
                hash: "94f2c58a5926458fe90e2b9e5fb6d38862b43e46bb563d35bb2465b601f74c6f",
            },
        ],
    },
    Profile {
        name: "imed-banked-v5",
        init: 0x403c,
        tick: 0x416d,
        state_start: 0xdf20,
        state_end: 0xdf81,
        sfx_flag: 0xdf00,
        initial_mask: None,
        double_speed: true,
        wram_bank: 7,
        data_start: 0x4ed3,
        extra_banks: &[],
        checks: &[Check {
            bank: None,
            start: 0x4001,
            end: 0x4ed3,
            hash: "6f25777f74aef342634067951ed87bf17f5b18cef4a8b2be6fdfc9fb0dbe3fb7",
        }],
    },
];

pub(super) fn named(name: &str) -> Option<&'static Profile> {
    #[cfg(any(test, feature = "test-support"))]
    if name == super::tests::PROFILE.name {
        return Some(&super::tests::PROFILE);
    }
    #[cfg(any(test, feature = "test-support"))]
    if name == super::tests::DMG_PROFILE.name {
        return Some(&super::tests::DMG_PROFILE);
    }
    PROFILES.iter().find(|p| p.name == name)
}

pub(super) fn recognized(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Vec<Recognition>, ScanStop> {
    let mut result = Vec::new();
    if !super::supports_cartridge(bytes) {
        return Ok(result);
    }
    #[cfg(any(test, feature = "test-support"))]
    if let Some(profile) = super::tests::recognized(bytes) {
        result.push(Recognition { bank: 1, profile });
    }
    let banks = (0x8000usize << bytes[0x148]) / 0x4000;
    for profile in &PROFILES {
        if profile.double_speed != matches!(bytes[0x143], 0x80 | 0xc0)
            || (!profile.double_speed && (!matches!(bytes[0x147], 1..=3) || bytes[0x148] > 4))
            || (profile.double_speed && !(0x19..=0x1e).contains(&bytes[0x147]))
        {
            continue;
        }
        for bank in 1..banks.min(256) {
            budget.charge()?;
            let valid = profile.checks.iter().all(|check| {
                let bank = usize::from(check.bank.unwrap_or(bank as u16));
                let start =
                    bank * 0x4000 + usize::from(check.start) - if bank == 0 { 0 } else { 0x4000 };
                bytes
                    .get(start..start + usize::from(check.end - check.start))
                    .is_some_and(|data| zeff_firmware::sha256_hex(data) == check.hash)
            });
            if valid {
                result.push(Recognition {
                    bank: bank as u16,
                    profile,
                });
            }
        }
    }
    Ok(result)
}

pub(super) fn modules(
    bytes: &[u8],
    recognized: Recognition,
    budget: &mut Budget<'_>,
) -> Result<Vec<u16>, ScanStop> {
    let base = usize::from(recognized.bank) * 0x4000;
    let start = usize::from(recognized.profile.data_start - 0x4000);
    let mut result = Vec::new();
    for (offset, window) in bytes[base + start..base + 0x4000].windows(8).enumerate() {
        budget.charge()?;
        if window == b"IMEDGBoy" {
            result.push((0x4000 + start + offset) as u16);
        }
    }
    Ok(result)
}
