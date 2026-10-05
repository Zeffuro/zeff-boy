use super::*;

pub(super) fn media() -> Vec<u8> {
    crate::emu_backend::pce::netplay_fixture_hucard()
}

pub(super) fn backend(topology: u8, bytes: &[u8]) -> EmuBackend {
    load(topology, bytes, false)
}

pub(super) fn zip_backend(topology: u8, bytes: &[u8]) -> EmuBackend {
    load(topology, bytes, true)
}

fn load(topology: u8, bytes: &[u8], zipped: bool) -> EmuBackend {
    let (path, rom, bytes) = source(bytes, "browser-netplay-proof.pce", zipped);
    load_backend_from_rom_source(
        ActiveSystem::Pce,
        &path,
        &rom,
        Some(bytes.clone()),
        BackendLoadConfig {
            sample_rate: Some(48_000),
            pce_netplay: true,
            netplay_browser_media: crate::emu_backend::loader::NetplayRomMedia::browser(&bytes),
            pce_load_battery_bram: false,
            pce_cartridge_hardware: Some(if topology == 0 {
                zeff_pce_core::hardware::PceCartridgeHardware::Base
            } else {
                zeff_pce_core::hardware::PceCartridgeHardware::SuperGrafx
            }),
            ..Default::default()
        },
    )
    .unwrap()
    .backend
}

#[wasm_bindgen_test(async)]
async fn browser_netplay_pce_base_worker_matches_reference_and_restores() {
    cases(0).await;
}

#[wasm_bindgen_test(async)]
async fn browser_netplay_pce_supergrafx_worker_matches_reference_and_restores() {
    cases(1).await;
}

async fn cases(topology: u8) {
    crate::platform::init_storage().await;
    for delay in [0, 2, 3] {
        for host_first in [true, false] {
            case(ActiveSystem::Pce, topology, delay, host_first).await;
        }
    }
}
