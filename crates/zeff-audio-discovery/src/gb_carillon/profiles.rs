use crate::{Budget, ScanStop};

const OLD_CODE_HASH: &str = "a9a5262449482a854f3951788c48dec01ec81b666f8cbbb5c24c3e00ca40601f";
const ISOLATED_CODE_HASH: &str = "77b814837b7484182a96462e269e69989b6c0fc04c9df0cdf71e9838d9fdd4fe";
pub(super) const COMMANDS: [u16; 16] = [
    0x4559, 0x454a, 0x4551, 0x44fc, 0x4509, 0x4514, 0x4523, 0x4535, 0x4542, 0x4559, 0x4559, 0x4559,
    0x4559, 0x4559, 0x4559, 0x4559,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Profile {
    pub(super) name: &'static str,
    code_hash: &'static str,
    prefix: [u8; 15],
    pub(super) selector_table: u16,
    pub(super) isolated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Recognition {
    pub(super) bank: u16,
    pub(super) profile: &'static Profile,
}

const OLD: Profile = Profile {
    name: "carillon-cgb-v1",
    code_hash: OLD_CODE_HASH,
    prefix: [
        0xc3, 0x56, 0x40, 0xc3, 0x88, 0x40, 0xc3, 0xe0, 0x40, 0xc3, 0, 0x41, 0xc3, 0xe8, 0x40,
    ],
    selector_table: 0x40f2,
    isolated: false,
};

const ISOLATED: Profile = Profile {
    name: "carillon-cgb-v1-isolated",
    code_hash: ISOLATED_CODE_HASH,
    prefix: [
        0xc3, 0x56, 0x40, 0xc3, 0x85, 0x40, 0xc3, 0xdf, 0x40, 0xc3, 0, 0x41, 0xc3, 0xe7, 0x40,
    ],
    selector_table: 0x40f1,
    isolated: true,
};

const PROFILES: [&Profile; 2] = [&OLD, &ISOLATED];

pub(super) fn named(name: &str) -> Option<&'static Profile> {
    PROFILES
        .iter()
        .copied()
        .find(|profile| profile.name == name)
}

fn isolated_source_cartridge(bytes: &[u8]) -> bool {
    bytes.len() >= 0x8000
        && bytes.len() <= 0x80_0000
        && bytes.len().is_multiple_of(0x4000)
        && matches!(bytes[0x143], 0x80 | 0xc0)
        && matches!(bytes[0x147], 0x97 | 0x99)
}

fn matches_profile(data: &[u8], profile: &'static Profile) -> bool {
    if data[..15] != profile.prefix {
        return false;
    }
    let code_matches = zeff_firmware::sha256_hex(&data[..0x600]) == profile.code_hash;
    #[cfg(any(test, feature = "test-support"))]
    let code_matches = code_matches || super::tests::recognized(data, profile.name);
    code_matches
        && COMMANDS
            .iter()
            .enumerate()
            .all(|(i, target)| data[0x6e0 + i * 2..0x6e2 + i * 2] == target.to_le_bytes())
}

pub(super) fn recognized(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Vec<Recognition>, ScanStop> {
    let mut result = Vec::new();
    let ordinary_cartridge = super::supports_cartridge(bytes);
    if !ordinary_cartridge && !isolated_source_cartridge(bytes) {
        return Ok(result);
    }
    for (bank, data) in bytes.as_chunks::<0x4000>().0.iter().enumerate().skip(1) {
        budget.charge()?;
        budget.charge()?;
        for &profile in &PROFILES {
            if (!profile.isolated && !ordinary_cartridge)
                || (profile.isolated && !ordinary_cartridge && !isolated_source_cartridge(bytes))
            {
                continue;
            }
            if matches_profile(data, profile) {
                result.push(Recognition {
                    bank: bank as u16,
                    profile,
                });
            }
        }
    }
    Ok(result)
}
