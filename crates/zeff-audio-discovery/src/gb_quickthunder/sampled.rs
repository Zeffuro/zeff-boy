use crate::{Budget, ScanStop};

use super::profiles::{self, Driver, HeaderKind, Profile};

#[cfg(any(test, feature = "test-support"))]
pub(super) mod tests;

static PROFILE: Profile = Profile {
    name: "gb-quickthunder-timer-13-01",
    header: HeaderKind::Executable,
    len: 1939,
    hash: "528994f4d1551acd048033d965ec0a603e7916b8923c70dbba6fe496fc309b73",
    selector: 0x45dc,
    tick: 0x4006,
    stride: 13,
    pattern_bytes: 2,
    operands: [1507, 70, 156, 1186, 366],
    aliases: &[&[70, 524, 925, 1097, 1400], &[156, 621, 1482], &[366, 831]],
    empty_operand: Some(1619),
    default_empty: 0x4798,
    release_operand: 1664,
};

pub(super) fn is_sampled(driver: Driver) -> bool {
    driver.profile.name == PROFILE.name
}

pub(super) fn recognize(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<Driver>, ScanStop> {
    if bytes.len() != 0x100000
        || bytes[0x143] != 0xc0
        || bytes[0x147..0x14a] != [0x19, 5, 0]
        || bytes[0xf8000..0xf8006] != [0xc3, 0xdc, 0x45, 0xc3, 0xa9, 0x46]
    {
        return Ok(None);
    }
    for _ in bytes.chunks(4096) {
        budget.charge()?;
    }
    if zeff_firmware::sha256_hex(bytes)
        != "bfe2bc3682e92d3d6583e4e06539f61add816e3566c292f890865f18d885aeb2"
    {
        return Ok(None);
    }
    profiles::recognize(bytes, 62, &PROFILE, budget)
}
