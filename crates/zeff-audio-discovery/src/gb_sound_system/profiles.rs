use crate::{Budget, ScanStop};

use super::{GbSoundSystemHardware, word};

#[path = "data.rs"]
mod data;

pub(super) struct Profile {
    pub name: &'static str,
    pub hash: &'static str,
    pub end: usize,
    pub wram: u16,
    pub hardware: GbSoundSystemHardware,
    pub table_low: usize,
    pub table_high: usize,
    pub dispatch_hash: &'static str,
    pub operands: &'static [(usize, usize, i16)],
}

pub(super) struct Driver {
    pub profile: &'static Profile,
    pub bank: u16,
    pub table: u16,
    pub dispatch: u16,
    pub count: u16,
    pub hardware: GbSoundSystemHardware,
    pub normal_host: bool,
}

pub(super) fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Vec<Driver>, ScanStop> {
    budget.charge()?;
    let mut drivers = Vec::new();
    if !super::supports_cartridge(bytes) {
        return Ok(drivers);
    }
    let normal_host = zeff_firmware::sha256_hex(&bytes[0x150..0x184])
        == "90cdd0111b8d79ce4471deb576e479ec34a9182b79bc6c6d4f6481ae45c8ac23";
    for bank in 1..bytes.len() / 0x4000 {
        budget.charge()?;
        let code = &bytes[bank * 0x4000..(bank + 1) * 0x4000];
        if [0, 3, 6].iter().any(|&at| code[at] != 0xc3) {
            continue;
        }
        #[cfg(not(any(test, feature = "test-support")))]
        let profiles = data::PROFILES.iter();
        #[cfg(any(test, feature = "test-support"))]
        let profiles = data::PROFILES
            .iter()
            .chain([&super::tests::PROFILE, &super::tests::NORMAL_PROFILE]);
        for profile in profiles {
            budget.charge()?;
            let mut normalized = code[..profile.end].to_vec();
            for &(low, high, _) in profile.operands {
                normalized[low] = 0;
                normalized[high] = 0;
            }
            if zeff_firmware::sha256_hex(&normalized) != profile.hash {
                continue;
            }
            let (lo, hi, _) = profile.operands[0];
            let dispatch = u16::from_le_bytes([code[lo], code[hi]]);
            let table = u16::from_le_bytes([code[profile.table_low], code[profile.table_high]]);
            let minimum = 0x4000 + profile.end;
            if usize::from(dispatch) < minimum
                || dispatch > 0x7eb8
                || dispatch & 0xff != 0xb8
                || usize::from(table) < minimum
                || table > 0x7ffc
                || profile.operands.iter().any(|&(lo, hi, relative)| {
                    let pointer = u16::from_le_bytes([code[lo], code[hi]]);
                    if relative >= 0 {
                        pointer != dispatch + relative as u16
                    } else {
                        usize::from(pointer) < minimum || pointer >= 0x8000
                    }
                })
            {
                continue;
            }
            let tables = usize::from(dispatch - 0x4000);
            let mut dispatch_bytes = code[tables..tables + 0xb8].to_vec();
            // Music command 19 is rejected before dispatch, so its unused slot is irrelevant.
            dispatch_bytes[0x6e..0x70].fill(0);
            if zeff_firmware::sha256_hex(&dispatch_bytes) != profile.dispatch_hash {
                continue;
            }
            let count = (0..64)
                .take_while(|index| {
                    let at = usize::from(table - 0x4000) + index * 4;
                    at + 4 <= code.len()
                        && [word(code, at), word(code, at + 2)]
                            .iter()
                            .all(|&pointer| usize::from(pointer) >= minimum && pointer < 0x8000)
                })
                .count() as u16;
            if count != 0 {
                drivers.push(Driver {
                    profile,
                    bank: bank as u16,
                    table,
                    dispatch,
                    count,
                    hardware: if normal_host {
                        GbSoundSystemHardware::CgbNormal
                    } else {
                        profile.hardware
                    },
                    normal_host,
                });
            }
            break;
        }
    }
    Ok(drivers)
}
