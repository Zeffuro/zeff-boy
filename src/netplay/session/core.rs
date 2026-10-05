use super::*;
use crate::emu_backend::pce::{PceBackendRollbackSession, PceBackendRollbackSnapshot};
use crate::emu_backend::ws::{WsBackendRollbackSession, WsBackendRollbackSnapshot};
use zeff_sega8_core::emulator::rollback::{Sega8RollbackSession, Sega8RollbackSnapshot};
use zeff_ws_core::emulator::link_pair::Endpoint;

pub(super) enum Lease {
    Nes(NesRollbackSession),
    Sega8(Sega8RollbackSession),
    Pce(PceBackendRollbackSession),
    Ws(Box<WsBackendRollbackSession>),
}
pub(super) enum Snapshot {
    Nes(Box<NesRollbackSnapshot>),
    Sega8(Box<Sega8RollbackSnapshot>),
    Pce(Box<PceBackendRollbackSnapshot>),
    Ws(Box<WsBackendRollbackSnapshot>),
}

impl Snapshot {
    pub(super) fn shared_media_bytes(&self) -> usize {
        match self {
            Self::Ws(state) => state.shared_media_bytes(),
            _ => 0,
        }
    }
    pub(super) fn ws(&self) -> Option<&WsBackendRollbackSnapshot> {
        match self {
            Self::Ws(state) => Some(state),
            _ => None,
        }
    }
    pub(super) fn pce(&self) -> Option<&PceBackendRollbackSnapshot> {
        match self {
            Self::Pce(state) => Some(state),
            _ => None,
        }
    }
    pub(super) fn frame(&self) -> u64 {
        match self {
            Self::Nes(state) => state.frame(),
            Self::Sega8(state) => state.frame(),
            Self::Pce(state) => state.frame(),
            Self::Ws(state) => state.frame(),
        }
    }
    pub(super) fn retained_bytes(&self) -> usize {
        match self {
            Self::Nes(state) => state.retained_bytes(),
            Self::Sega8(state) => state.retained_bytes(),
            Self::Pce(state) => state.retained_bytes(),
            Self::Ws(state) => state.retained_bytes(),
        }
    }
}

impl Lease {
    pub(super) fn input_mask(&self) -> u16 {
        match self {
            Self::Nes(_) | Self::Sega8(_) | Self::Pce(_) => 0xff,
            Self::Ws(_) => 0x7ff,
        }
    }
    pub(super) fn validate_input(&self, buttons: u16) -> Result<()> {
        ensure!(
            buttons & !self.input_mask() == 0,
            "unsupported netplay input bits"
        );
        Ok(())
    }
    pub(super) fn begin(backend: &mut EmuBackend, player: Player) -> Result<Self> {
        match backend {
            EmuBackend::Nes(nes) => Ok(Self::Nes(nes.emu.begin_rollback_session()?)),
            EmuBackend::Sega8(sega) => Ok(Self::Sega8(sega.emu.begin_rollback_session()?)),
            EmuBackend::Pce(pce) => Ok(Self::Pce(pce.begin_netplay_rollback()?)),
            EmuBackend::Ws(ws) => Ok(Self::Ws(Box::new(ws.begin_netplay_rollback(
                match player {
                    Player::One => Endpoint::Zero,
                    Player::Two => Endpoint::One,
                },
            )?))),
            _ => anyhow::bail!("unsupported netplay core"),
        }
    }
    pub(super) fn capture(&self, backend: &EmuBackend) -> Result<Snapshot> {
        match (self, backend) {
            (Self::Ws(lease), EmuBackend::Ws(ws)) => Ok(Snapshot::Ws(Box::new(lease.capture(ws)?))),
            (Self::Pce(lease), EmuBackend::Pce(pce)) => {
                Ok(Snapshot::Pce(Box::new(lease.capture(pce)?)))
            }
            (Self::Nes(lease), EmuBackend::Nes(nes)) => {
                Ok(Snapshot::Nes(Box::new(lease.capture(&nes.emu)?)))
            }
            (Self::Sega8(lease), EmuBackend::Sega8(sega)) => {
                Ok(Snapshot::Sega8(Box::new(lease.capture(&sega.emu)?)))
            }
            _ => anyhow::bail!("netplay lost core backend"),
        }
    }
    pub(super) fn advance(
        &mut self,
        backend: &mut EmuBackend,
        ports: [u16; 2],
    ) -> Result<Vec<f32>> {
        self.validate_input(ports[0])?;
        self.validate_input(ports[1])?;
        if let (Self::Ws(lease), EmuBackend::Ws(ws)) = (&mut *self, &mut *backend) {
            return lease.advance_frame(ws, ports);
        }
        let ports = [u8::try_from(ports[0])?, u8::try_from(ports[1])?];
        match (self, backend) {
            (Self::Pce(lease), EmuBackend::Pce(pce)) => lease.advance_frame(pce, ports),
            (Self::Nes(lease), EmuBackend::Nes(nes)) => lease.advance_frame(&mut nes.emu, ports),
            (Self::Sega8(lease), EmuBackend::Sega8(sega)) => {
                lease.advance_frame(&mut sega.emu, ports)
            }
            _ => anyhow::bail!("netplay lost core backend"),
        }
    }
    pub(super) fn restore(&mut self, backend: &mut EmuBackend, state: &Snapshot) -> Result<()> {
        match (self, backend, state) {
            (Self::Ws(lease), EmuBackend::Ws(ws), Snapshot::Ws(state)) => lease.restore(ws, state),
            (Self::Pce(lease), EmuBackend::Pce(pce), Snapshot::Pce(state)) => {
                lease.restore(pce, state)
            }
            (Self::Nes(lease), EmuBackend::Nes(nes), Snapshot::Nes(state)) => {
                lease.restore(&mut nes.emu, state)
            }
            (Self::Sega8(lease), EmuBackend::Sega8(sega), Snapshot::Sega8(state)) => {
                lease.restore(&mut sega.emu, state)
            }
            _ => anyhow::bail!("netplay lost core backend"),
        }
    }
    pub(super) fn restore_checkpoint(
        &mut self,
        backend: &mut EmuBackend,
        state: &Snapshot,
        bytes: Vec<u8>,
    ) -> Result<()> {
        match (self, &mut *backend, state) {
            (Self::Ws(lease), EmuBackend::Ws(ws), Snapshot::Ws(state)) => {
                lease.restore_after_session(ws, state, &bytes)
            }
            (Self::Pce(lease), EmuBackend::Pce(pce), Snapshot::Pce(state)) => {
                lease.restore_after_session(pce, state, &bytes)
            }
            (Self::Sega8(lease), EmuBackend::Sega8(sega), Snapshot::Sega8(state)) => {
                lease.restore_after_session(&mut sega.emu, state, &bytes)
            }
            (Self::Nes(_), EmuBackend::Nes(_), Snapshot::Nes(_)) => {
                ensure!(
                    backend.load_state_from_bytes(bytes)?
                        == zeff_emu_common::StateRestoreOutcome::Exact,
                    "inexact netplay restoration"
                );
                Ok(())
            }
            _ => anyhow::bail!("netplay lost core backend"),
        }
    }
}

pub(super) fn persistence(backend: &mut EmuBackend, enabled: Option<bool>) -> Result<bool> {
    match backend {
        EmuBackend::Ws(ws) => {
            let previous = ws.host_persistence_enabled();
            if let Some(enabled) = enabled {
                ws.set_host_persistence_enabled(enabled);
            }
            Ok(previous)
        }
        EmuBackend::Pce(pce) => {
            let previous = pce.host_persistence_enabled();
            if let Some(enabled) = enabled {
                pce.set_host_persistence_enabled(enabled);
            }
            Ok(previous)
        }
        EmuBackend::Nes(nes) => {
            let previous = nes.host_persistence_enabled();
            if let Some(enabled) = enabled {
                nes.set_host_persistence_enabled(enabled);
            }
            Ok(previous)
        }
        EmuBackend::Sega8(sega) => {
            let previous = sega.host_persistence_enabled();
            if let Some(enabled) = enabled {
                sega.set_host_persistence_enabled(enabled);
            }
            Ok(previous)
        }
        _ => anyhow::bail!("netplay lost core backend"),
    }
}
