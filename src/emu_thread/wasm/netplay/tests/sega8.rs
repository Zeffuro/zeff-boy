use super::*;
use zeff_nes_core::emulator::rollback::NesRollbackSession;
use zeff_sega8_core::emulator::rollback::Sega8RollbackSession;

pub(super) fn media() -> Vec<u8> {
    let mut rom = vec![0; 32768];
    let program = [
        0x3e, 0x84, 0xd3, 0x7f, 0x3e, 0x12, 0xd3, 0x7f, 0x3e, 0x90, 0xd3, 0x7f, 0x3e, 0x40, 0xd3,
        0xbf, 0x3e, 0x81, 0xd3, 0xbf, 0x3e, 0x08, 0x32, 0xfc, 0xff, 0xdb, 0xdc, 0x32, 0x00, 0xc0,
        0xdb, 0xdd, 0x32, 0x01, 0xc0, 0x3a, 0x02, 0xc0, 0x3c, 0x32, 0x02, 0xc0, 0xd3, 0xbe, 0x3e,
        0x5a, 0x32, 0x00, 0x80, 0xc3, 0x19, 0x00,
    ];
    rom[..program.len()].copy_from_slice(&program);
    rom[0x66..0x6f].copy_from_slice(&[0x3a, 0x03, 0xc0, 0x3c, 0x32, 0x03, 0xc0, 0xed, 0x45]);
    rom
}

pub(super) fn backend(system: ActiveSystem, timing: u8, bytes: &[u8]) -> EmuBackend {
    let path = Path::new(if system == ActiveSystem::MasterSystem {
        "browser-netplay-proof.sms"
    } else {
        "browser-netplay-proof.sg"
    });
    load_backend_from_rom_source(
        system,
        path,
        path,
        Some(bytes.to_vec()),
        BackendLoadConfig {
            sample_rate: Some(48_000),
            initial_input: None,
            sega8_browser_source: true,
            sega8_load_battery_sram: false,
            sega8_video_standard: Some(if timing == 0 {
                zeff_sega8_core::hardware::timing::Sega8VideoStandard::Ntsc
            } else {
                zeff_sega8_core::hardware::timing::Sega8VideoStandard::Pal
            }),
            ..Default::default()
        },
    )
    .unwrap()
    .backend
}

pub(super) enum ReferenceLease {
    Nes(NesRollbackSession),
    Sega8(Sega8RollbackSession),
}

impl ReferenceLease {
    pub(super) fn begin(backend: &mut EmuBackend) -> Self {
        match backend {
            EmuBackend::Nes(nes) => Self::Nes(nes.emu.begin_rollback_session().unwrap()),
            EmuBackend::Sega8(sega) => Self::Sega8(sega.emu.begin_rollback_session().unwrap()),
            _ => unreachable!(),
        }
    }

    pub(super) fn advance(&self, backend: &mut EmuBackend, ports: [u8; 2]) -> Vec<f32> {
        match (self, backend) {
            (Self::Nes(lease), EmuBackend::Nes(nes)) => {
                lease.advance_frame(&mut nes.emu, ports).unwrap()
            }
            (Self::Sega8(lease), EmuBackend::Sega8(sega)) => {
                lease.advance_frame(&mut sega.emu, ports).unwrap()
            }
            _ => unreachable!(),
        }
    }
}

pub(super) fn runtime(backend: &EmuBackend) -> Option<Vec<u8>> {
    backend
        .sega8()
        .map(|sega| sega.emu.encode_rollback_runtime_state())
}

pub(super) fn persistent(backend: &EmuBackend) -> Vec<u8> {
    match backend {
        EmuBackend::Nes(nes) => nes.emu.dump_persistent_data().unwrap_or_default(),
        EmuBackend::Sega8(sega) => sega.emu.bus().cartridge_ram_visible().to_vec(),
        _ => unreachable!(),
    }
}

pub(super) fn persistence_enabled(backend: &EmuBackend) -> bool {
    match backend {
        EmuBackend::Nes(nes) => nes.host_persistence_enabled(),
        EmuBackend::Sega8(sega) => sega.host_persistence_enabled(),
        _ => unreachable!(),
    }
}

#[wasm_bindgen_test(async)]
async fn browser_netplay_sega_worker_matches_reference_and_restores() {
    crate::platform::init_storage().await;
    for system in [ActiveSystem::MasterSystem, ActiveSystem::Sg1000] {
        for timing in [0, 1] {
            for delay in [0, 2] {
                for host_first in [true, false] {
                    case(system, timing, delay, host_first).await;
                }
            }
        }
    }
}
