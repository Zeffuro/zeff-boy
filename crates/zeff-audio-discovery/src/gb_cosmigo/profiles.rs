use crate::{Budget, ScanStop};

#[derive(Clone, Copy)]
pub(super) struct Profile {
    pub(super) name: &'static str,
    hash: &'static str,
    prefix: [u8; 12],
    pub(super) separate_start: bool,
}

#[derive(Clone, Copy)]
pub(super) struct Recognition {
    pub(super) bank: u16,
    pub(super) profile: &'static Profile,
}

const STANDARD_PREFIX: [u8; 12] = [
    0xc3, 0x32, 0x40, 0xc3, 0xe0, 0x40, 0xc3, 0x8b, 0x40, 0xc3, 0x16, 0x40,
];

const PROFILES: [Profile; 3] = [
    Profile {
        name: "cosmigo-four-channel-v1",
        hash: "5fadd498031e003f6f02a69f92b35d5cb6332ff7bc05e1addfe24cd704568317",
        prefix: STANDARD_PREFIX,
        separate_start: false,
    },
    Profile {
        name: "cosmigo-four-channel-v2",
        hash: "df49f1486d6012787877bb714cab6a3bee9879812fb96c632ce4bdd6627e79eb",
        prefix: STANDARD_PREFIX,
        separate_start: false,
    },
    Profile {
        name: "cosmigo-four-channel-split-start",
        hash: "16710421bfa4e655bae25848aaabdc44e1e272172cfa19473ff8958dc14b72fd",
        prefix: [
            0xc3, 0x32, 0x40, 0xc3, 0xed, 0x40, 0xc3, 0x98, 0x40, 0xc3, 0x16, 0x40,
        ],
        separate_start: true,
    },
];

pub(super) fn named(name: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.name == name)
}

pub(super) fn recognized(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Vec<Recognition>, ScanStop> {
    let mut found = Vec::new();
    if !super::supports_cartridge(bytes) {
        return Ok(found);
    }
    for (bank, data) in bytes.as_chunks::<0x4000>().0.iter().enumerate().skip(1) {
        budget.charge()?;
        if data[0] != 0xc3 || data[3] != 0xc3 {
            continue;
        }
        let hash = zeff_firmware::sha256_hex(&data[0x16..0x1000]);
        for profile in &PROFILES {
            let matches = hash == profile.hash;
            #[cfg(any(test, feature = "test-support"))]
            let matches = matches || super::tests::recognized(data, profile.name);
            if data[..12] == profile.prefix && matches {
                found.push(Recognition {
                    bank: bank as u16,
                    profile,
                });
            }
        }
    }
    Ok(found)
}
