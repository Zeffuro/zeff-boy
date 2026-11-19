use anyhow::{Result, ensure};
use zeff_md_core::{
    ControllerKind,
    emulator::rollback::{MdRollbackConfig, MdRollbackInputs},
};
use zeff_netplay::lockstep::Player;

pub(crate) const INPUT_DOMAIN: &[u8] =
    b"MD-host-buttons8-directions4/local-P1-three-004B-six-00FF/strict-wire-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MdNetplayInputProfile {
    player: Player,
    kinds: [ControllerKind; 2],
}
impl MdNetplayInputProfile {
    pub(in crate::netplay) fn from_config(player: Player, config: &MdRollbackConfig) -> Self {
        Self {
            player,
            kinds: config.controller_kinds(),
        }
    }
    pub(crate) fn player(self) -> Player {
        self.player
    }
    pub(crate) fn kinds(self) -> [ControllerKind; 2] {
        self.kinds
    }
    pub(crate) fn project_local(self, buttons: u8, dpad: u8) -> Result<u16> {
        project_local_host(self.kinds[index(self.player)], buttons, dpad)
    }
}
pub(crate) fn project_local_host(kind: ControllerKind, buttons: u8, dpad: u8) -> Result<u16> {
    ensure!(dpad & !0x0f == 0, "invalid MD host directions");
    let buttons = buttons & MdRollbackInputs::word_mask(kind) as u8;
    Ok(u16::from(buttons) | (u16::from(dpad) << 8))
}
#[cfg(test)]
mod tests;

pub(super) fn index(player: Player) -> usize {
    match player {
        Player::One => 0,
        Player::Two => 1,
    }
}
pub(super) fn validate(config: &MdRollbackConfig, player: Player, word: u16) -> Result<()> {
    let mut pair = [0; 2];
    pair[index(player)] = word;
    MdRollbackInputs::new(pair, config)?;
    Ok(())
}
