use crate::{Budget, ScanStop};

mod build_1;
mod build_2;
mod build_3;

pub(super) struct Selection {
    pub index: u16,
    pub entry: u32,
    pub module: u32,
    pub spans: &'static [(u32, u32)],
}

pub(super) struct Profile {
    pub name: &'static str,
    pub hash: &'static str,
    pub init: u16,
    pub stop: u16,
    pub select: u16,
    pub tick: u16,
    pub shadow: u8,
    pub compressed: bool,
    pub selections: &'static [Selection],
}

const PROFILES: [&Profile; 3] = [&build_1::PROFILE, &build_2::PROFILE, &build_3::PROFILE];

pub(super) fn named(name: &str) -> Option<&'static Profile> {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(profile) = super::tests::PROFILES.iter().find(|p| p.name == name) {
        return Some(profile);
    }
    PROFILES
        .iter()
        .copied()
        .find(|profile| profile.name == name)
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
    for (profile, fixture) in super::tests::PROFILES.iter().zip([
        super::tests::synthetic_rom(),
        super::tests::synthetic_mbc2_rom(),
    ]) {
        if hash == zeff_firmware::sha256_hex(&fixture) {
            return Ok(Some(profile));
        }
    }
    Ok(PROFILES
        .iter()
        .copied()
        .find(|profile| profile.hash == hash))
}
