use super::*;
use crate::emu_backend::pce::PceBackendRollbackSession;
use zeff_nes_core::emulator::rollback::NesRollbackSession;
use zeff_sega8_core::emulator::rollback::Sega8RollbackSession;

pub(super) enum ReferenceLease {
    Nes(NesRollbackSession),
    Sega8(Sega8RollbackSession),
    Pce(PceBackendRollbackSession),
}

impl ReferenceLease {
    pub(super) fn begin(backend: &mut EmuBackend) -> Self {
        match backend {
            EmuBackend::Nes(nes) => Self::Nes(nes.emu.begin_rollback_session().unwrap()),
            EmuBackend::Sega8(sega) => Self::Sega8(sega.emu.begin_rollback_session().unwrap()),
            EmuBackend::Pce(pce) => Self::Pce(pce.begin_netplay_rollback().unwrap()),
            _ => unreachable!(),
        }
    }

    pub(super) fn advance(&self, backend: &mut EmuBackend, ports: [u16; 2]) -> Vec<f32> {
        let ports = ports.map(|buttons| u8::try_from(buttons).unwrap());
        match (self, backend) {
            (Self::Nes(lease), EmuBackend::Nes(nes)) => {
                lease.advance_frame(&mut nes.emu, ports).unwrap()
            }
            (Self::Sega8(lease), EmuBackend::Sega8(sega)) => {
                lease.advance_frame(&mut sega.emu, ports).unwrap()
            }
            (Self::Pce(lease), EmuBackend::Pce(pce)) => lease.advance_frame(pce, ports).unwrap(),
            _ => unreachable!(),
        }
    }
}

pub(super) fn runtime(backend: &EmuBackend) -> Option<Vec<u8>> {
    match backend {
        EmuBackend::Sega8(sega) => Some(sega.emu.encode_rollback_runtime_state()),
        EmuBackend::Pce(pce) => Some(pce.netplay_runtime_state_bytes()),
        _ => None,
    }
}

pub(super) fn persistent(backend: &EmuBackend) -> Vec<u8> {
    match backend {
        EmuBackend::Nes(nes) => nes.emu.dump_persistent_data().unwrap_or_default(),
        EmuBackend::Sega8(sega) => sega.emu.bus().cartridge_ram_visible().to_vec(),
        EmuBackend::Pce(pce) => pce.netplay_persistent_state_bytes(),
        _ => unreachable!(),
    }
}

pub(super) fn persistence_enabled(backend: &EmuBackend) -> bool {
    match backend {
        EmuBackend::Nes(nes) => nes.host_persistence_enabled(),
        EmuBackend::Sega8(sega) => sega.host_persistence_enabled(),
        EmuBackend::Pce(pce) => pce.host_persistence_enabled(),
        _ => unreachable!(),
    }
}
