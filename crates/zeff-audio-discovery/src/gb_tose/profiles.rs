use crate::{Budget, ScanStop};

use super::word;

pub(super) struct Profile {
    pub name: &'static str,
    pub prefix: &'static [u8],
    pub len: usize,
    pub relocations: &'static [usize],
    pub table_operand: usize,
    pub hash: &'static str,
    pub selector: u16,
    pub tick: u16,
}

#[derive(Clone, Copy)]
pub(super) struct Driver {
    pub profile: &'static Profile,
    pub start: usize,
    pub table: u16,
}

static CLASSIC_25: Profile = Profile {
    name: "gb-tose-classic-25",
    prefix: &[0x3e, 0x80, 0xe0, 0x26, 0xaf, 0xe0, 0x25, 0xea],
    len: 1982,
    relocations: &[
        36, 39, 42, 199, 202, 231, 252, 293, 374, 502, 505, 531, 657, 669, 672, 682, 709, 715, 748,
        772, 782, 792, 808, 828, 832, 851, 875, 915, 948, 963, 975, 981, 1018, 1030, 1114, 1129,
        1159, 1186, 1225, 1274, 1334, 1352, 1371, 1377, 1380,
    ],
    table_operand: 56,
    hash: "dc5fdaa717f9c2b17e9aaa81e1ba971fb664e7d6dbb58c9cfec31aec7a5455ca",
    selector: 35,
    tick: 129,
};

pub(super) fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Vec<Driver>, ScanStop> {
    budget.charge()?;
    let mut drivers = Vec::new();
    if !super::supports_cartridge(bytes) {
        return Ok(drivers);
    }
    let profiles = std::iter::once(&CLASSIC_25);
    #[cfg(any(test, feature = "test-support"))]
    let profiles = profiles.chain(std::iter::once(&super::tests::PROFILE));
    for profile in profiles {
        for start in 0x300..=0x4000 - profile.len {
            budget.charge()?;
            if bytes.get(start..start + profile.prefix.len()) != Some(profile.prefix) {
                continue;
            }
            let code = &bytes[start..start + profile.len];
            let table = word(code, profile.table_operand);
            if !(0x4000..=0x7ff0).contains(&table) {
                continue;
            }
            let mut normalized = code.to_vec();
            let mut valid = true;
            for &at in profile.relocations {
                let value = usize::from(word(code, at));
                if !(start..start + profile.len).contains(&value) {
                    valid = false;
                    break;
                }
                normalized[at..at + 2].copy_from_slice(&((value - start) as u16).to_le_bytes());
            }
            normalized[profile.table_operand..profile.table_operand + 2].fill(0);
            budget.charge()?;
            if valid && zeff_firmware::sha256_hex(&normalized) == profile.hash {
                drivers.push(Driver {
                    profile,
                    start,
                    table,
                });
            }
        }
    }
    Ok(drivers)
}
