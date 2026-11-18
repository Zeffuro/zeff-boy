use super::*;
use crate::emu_backend::pce::{PceBackendRollbackSession, PceBackendRollbackSnapshot};
use crate::emu_backend::ws::{WsBackendRollbackSession, WsBackendRollbackSnapshot};
use zeff_sega8_core::emulator::rollback::{Sega8RollbackSession, Sega8RollbackSnapshot};
use zeff_ws_core::emulator::link_pair::Endpoint;

pub(super) enum Lease {
    Md(super::md::MdLease),
    Nes(NesRollbackSession),
    Sega8(Sega8RollbackSession),
    Pce(PceBackendRollbackSession),
    Ws(Box<WsBackendRollbackSession>),
}
pub(super) enum Snapshot {
    Md {
        state: Box<crate::emu_backend::md::MdBackendRollbackSnapshot>,
        frame: u64,
    },
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
            Self::Md { frame, .. } => *frame,
            Self::Nes(state) => state.frame(),
            Self::Sega8(state) => state.frame(),
            Self::Pce(state) => state.frame(),
            Self::Ws(state) => state.frame(),
        }
    }
    pub(super) fn retained_bytes(&self) -> Result<usize> {
        Ok(match self {
            Self::Md { state, .. } => {
                let mut into = zeff_md_core::emulator::rollback::MdRetainedAllocations::new();
                state.visit_retained_allocations(&mut into)?;
                usize::try_from(into.finish()?.requested_payload_bytes())?
            }
            Self::Nes(state) => state.retained_bytes(),
            Self::Sega8(state) => state.retained_bytes(),
            Self::Pce(state) => state.retained_bytes(),
            Self::Ws(state) => state.retained_bytes(),
        })
    }
}

impl Lease {
    pub(super) fn validate_frame(&self, backend: &EmuBackend, frame: u64) -> Result<()> {
        if let Self::Md(lease) = self {
            let EmuBackend::Md(md) = backend else {
                anyhow::bail!("netplay lost MD backend");
            };
            return lease.validate_frame(md, frame);
        }
        ensure!(
            backend.frame_count() == frame,
            "rollback core frame differs"
        );
        Ok(())
    }
    pub(super) fn input_mask(&self) -> u16 {
        match self {
            Self::Nes(_) | Self::Sega8(_) | Self::Pce(_) => 0xff,
            Self::Ws(_) => 0x7ff,
            Self::Md(_) => zeff_md_core::emulator::rollback::MdRollbackInputs::word_mask(
                zeff_md_core::ControllerKind::SixButton,
            ),
        }
    }
    pub(super) fn validate_input(&self, player: Player, buttons: u16) -> Result<()> {
        if let Self::Md(lease) = self {
            return lease.validate(player, buttons);
        }
        ensure!(
            buttons & !self.input_mask() == 0,
            "unsupported netplay input bits"
        );
        Ok(())
    }
    pub(super) fn begin(backend: &mut EmuBackend, player: Player) -> Result<Self> {
        match backend {
            EmuBackend::Md(md) => Ok(Self::Md(super::md::MdLease::begin(md)?)),
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
            (Self::Md(lease), EmuBackend::Md(md)) => {
                let (state, frame) = lease.capture(md)?;
                Ok(Snapshot::Md {
                    state: Box::new(state),
                    frame,
                })
            }
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
        if let Self::Md(lease) = self {
            let EmuBackend::Md(md) = backend else {
                anyhow::bail!("netplay lost MD backend");
            };
            return lease.advance(md, ports);
        }
        self.validate_input(Player::One, ports[0])?;
        self.validate_input(Player::Two, ports[1])?;
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
    pub(super) fn begin_md_frame(&self, backend: &mut EmuBackend, ports: [u16; 2]) -> Result<()> {
        let (Self::Md(lease), EmuBackend::Md(md)) = (self, backend) else {
            anyhow::bail!("netplay lost MD backend");
        };
        lease.begin_frame(md, ports)
    }
    pub(super) fn resume_md_frame(
        &self,
        backend: &mut EmuBackend,
        budget: std::num::NonZeroUsize,
    ) -> Result<Option<Vec<f32>>> {
        let (Self::Md(lease), EmuBackend::Md(md)) = (self, backend) else {
            anyhow::bail!("netplay lost MD backend");
        };
        lease.resume_frame(md, budget)
    }
    pub(super) fn restore(&mut self, backend: &mut EmuBackend, state: &Snapshot) -> Result<()> {
        match (self, backend, state) {
            (Self::Md(lease), EmuBackend::Md(md), Snapshot::Md { state, frame }) => {
                lease.restore(md, state, *frame)
            }
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
    pub(super) fn is_md(&self) -> bool {
        matches!(self, Self::Md(_))
    }
    pub(super) fn md_input(
        &self,
        player: Player,
    ) -> Option<crate::netplay::md_input::MdNetplayInputProfile> {
        if let Self::Md(lease) = self {
            Some(lease.profile(player))
        } else {
            None
        }
    }
    pub(super) fn abort_md_start(&self, backend: &mut EmuBackend, raw: &[u8]) -> Result<()> {
        if let Self::Md(lease) = self {
            let EmuBackend::Md(md) = backend else {
                anyhow::bail!("netplay lost MD backend");
            };
            lease.abort(md, raw)?;
        }
        Ok(())
    }
    pub(super) fn finish_md(
        &self,
        backend: &mut EmuBackend,
        state: &Snapshot,
        raw: &[u8],
        persistent: [u8; 32],
        worker: Result<()>,
    ) -> Result<()> {
        match (self, backend, state) {
            (Self::Md(lease), EmuBackend::Md(md), Snapshot::Md { state, frame }) => {
                lease.finish(md, (state, *frame), raw, persistent, worker)
            }
            _ => anyhow::bail!("netplay lost MD baseline"),
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
        EmuBackend::Md(md) => {
            ensure!(
                enabled.is_none(),
                "MD publication belongs to verified lease release"
            );
            Ok(md.host_persistence_enabled())
        }
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
