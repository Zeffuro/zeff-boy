use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};

use super::GbSong;

#[cfg(any(test, feature = "test-support"))]
mod fixture;
#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub use fixture::{fixture_rom, fixture_song};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GbBankedTiming {
    Cgb,
    CgbDouble,
    Dmg,
}

pub struct PreparedGbBanked {
    pub bytes: Vec<u8>,
    pub timing: GbBankedTiming,
    pub ready_address: u16,
    pub ready_value: u8,
    pub ack_address: u16,
    pub ack_value: u8,
    pub wait_start: u16,
    pub wait_end: u16,
}

#[derive(Clone, Copy)]
struct Contract {
    startup_hook: u16,
    bank_shadow: u8,
    selector: u16,
}

const BOOTSTRAP: usize = 0xa0;
const BOOTSTRAP_END: usize = 0x100;
const REV0_CONTRACT: Contract = Contract {
    startup_hook: 0x022b,
    bank_shadow: 0x9d,
    selector: 0x3b97,
};
const REV2_CONTRACT: Contract = Contract {
    selector: 0x3b24,
    ..REV0_CONTRACT
};
const REV3_CONTRACT: Contract = Contract {
    startup_hook: 0x0677,
    bank_shadow: 0x9f,
    selector: 0x3d98,
};

fn contract(song: &GbSong) -> Option<Contract> {
    let (contract, count) = match song.profile {
        super::PROFILE | super::REV1_PROFILE => (REV0_CONTRACT, 103),
        super::REV2_PROFILE => (REV2_CONTRACT, 103),
        super::REV3_PROFILE | super::REV4_PROFILE | super::REV5_PROFILE => (REV3_CONTRACT, 93),
        #[cfg(any(test, feature = "test-support"))]
        fixture::PROFILE_NAME => (REV0_CONTRACT, 2),
        _ => return None,
    };
    (song.index > 0 && song.index < count).then_some(contract)
}

pub fn supports_native(song: &GbSong) -> bool {
    contract(song).is_some()
}

pub fn prepare_rom(bytes: &[u8], song: &GbSong, cancel: &AtomicBool) -> Result<PreparedGbBanked> {
    let contract = contract(song)
        .ok_or_else(|| anyhow::anyhow!("GB banked selection has no qualified native playback"))?;
    super::validate_song(bytes, song, cancel)?;
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "GB banked native preparation cancelled"
    );
    build(bytes, song.index, contract)
}

fn build(bytes: &[u8], index: u16, contract: Contract) -> Result<PreparedGbBanked> {
    ensure!(
        bytes.len() == 0x20_0000
            && matches!(bytes[0x143], 0x80 | 0xc0)
            && bytes[0x147..0x149] == [0x10, 6]
            && matches!(bytes[0x149], 3 | 5),
        "GB banked native cartridge differs from its CGB/MBC3 contract"
    );
    ensure!(
        bytes[BOOTSTRAP..BOOTSTRAP_END]
            .iter()
            .all(|&byte| byte == 0),
        "GB banked native bootstrap window is not unused"
    );
    let mut code = vec![
        0xf3,
        0xaf,
        0xe0,
        0x0f,
        0xe0,
        0xff,
        0x3e,
        0x3a,
        0xe0,
        contract.bank_shadow,
        0xea,
        0,
        0x20,
        0xcd,
        0,
        0x40,
        0xaf,
        0xe0,
        0xfb,
        0x3e,
        0xa5,
        0xe0,
        0xfc,
    ];
    let wait_start = (BOOTSTRAP + code.len()) as u16;
    code.extend_from_slice(&[0xf0, 0xfb, 0xfe, 0x5a, 0x20, 0xfa, 0x11]);
    code.extend_from_slice(&index.to_le_bytes());
    code.push(0xcd);
    code.extend_from_slice(&contract.selector.to_le_bytes());
    code.extend_from_slice(&[
        0xaf, 0xe0, 0x0f, 0x3e, 1, 0xe0, 0xff, 0xfb, 0x76, 0x18, 0xfd,
    ]);
    let irq = (BOOTSTRAP + code.len()) as u16;
    code.extend_from_slice(&[
        0xf5, 0xc5, 0xd5, 0xe5, 0xcd, 0x5c, 0x40, 0xe1, 0xd1, 0xc1, 0xf1, 0xd9,
    ]);
    ensure!(
        BOOTSTRAP + code.len() <= BOOTSTRAP_END,
        "GB banked native bootstrap exceeds its qualified window"
    );
    let mut result = bytes.to_vec();
    result[BOOTSTRAP..BOOTSTRAP + code.len()].copy_from_slice(&code);
    let hook = usize::from(contract.startup_hook);
    result[hook..hook + 3].copy_from_slice(&[0xc3, BOOTSTRAP as u8, 0]);
    result[0x40..0x43].copy_from_slice(&[0xc3, irq as u8, (irq >> 8) as u8]);
    Ok(PreparedGbBanked {
        bytes: result,
        timing: GbBankedTiming::Cgb,
        ready_address: 0xfffc,
        ready_value: 0xa5,
        ack_address: 0xfffb,
        ack_value: 0x5a,
        wait_start,
        wait_end: wait_start + 6,
    })
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn fixture_profile(sha256: &str) -> Option<&'static super::Profile> {
    (sha256 == fixture::PROFILE.sha256).then_some(&fixture::PROFILE)
}
