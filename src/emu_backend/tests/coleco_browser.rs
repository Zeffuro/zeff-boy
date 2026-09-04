use super::*;
use std::sync::Arc;
use wasm_bindgen_test::wasm_bindgen_test;

const RETAIL_COLECO_BIOS_MD5: &str = "2c66f5911e5b42b8ebe113403548eee7";

fn cartridge() -> Vec<u8> {
    let mut rom = vec![0; 8 * 1024];
    rom[..2].copy_from_slice(&[0xAA, 0x55]);
    rom
}

fn bios_with_video_and_audio_program() -> Vec<u8> {
    let mut bios = vec![0; zeff_coleco_core::constants::BIOS_SIZE];
    bios[..21].copy_from_slice(&[
        0x3E, 0x04, 0xD3, 0xBF, // Set VDP register 7 backdrop to blue.
        0x3E, 0x87, 0xD3, 0xBF, 0x3E, 0x80, 0xD3, 0xE0, // Tone 0 low period.
        0x3E, 0x01, 0xD3, 0xE0, // Tone 0 high period.
        0x3E, 0x90, 0xD3, 0xE0, // Tone 0 full volume.
        0x76, // HALT while VDP and PSG continue clocking.
    ]);
    bios
}

fn inventory_entry(bios: Vec<u8>, md5: Option<&str>) -> zeff_firmware::FirmwareInventoryEntry {
    zeff_firmware::FirmwareInventoryEntry::from_bytes_with_legacy_digests(
        bios,
        Some("BIOS.col".to_owned()),
        md5.map(str::to_owned),
        None,
        zeff_firmware::catalog_specs(),
    )
}

fn load_with_inventory(
    inventory: zeff_firmware::FirmwareInventory,
) -> anyhow::Result<crate::emu_backend::loader::LoadedBackend> {
    let path = PathBuf::from("browser-upload.col");
    load_backend_from_rom_source(
        ActiveSystem::Coleco,
        &path,
        &path,
        Some(cartridge()),
        BackendLoadConfig {
            sample_rate: Some(44_100),
            firmware_inventory: Some(Arc::new(inventory)),
            ..BackendLoadConfig::default()
        },
    )
}

#[wasm_bindgen_test]
fn wasm_coleco_loader_rejects_missing_or_unrecognized_inventory() {
    let missing = match load_with_inventory(zeff_firmware::FirmwareInventory::new()) {
        Ok(_) => panic!("missing browser Coleco BIOS unexpectedly loaded"),
        Err(error) => error.to_string(),
    };
    assert!(missing.contains("coleco.vision.bios"));
    assert!(missing.contains("Settings > Firmware"));

    let mut inventory = zeff_firmware::FirmwareInventory::new();
    inventory.add(inventory_entry(
        vec![0; zeff_coleco_core::constants::BIOS_SIZE],
        None,
    ));
    let unrecognized = match load_with_inventory(inventory) {
        Ok(_) => panic!("unrecognized browser Coleco BIOS unexpectedly loaded"),
        Err(error) => error.to_string(),
    };
    assert!(unrecognized.contains("unrecognized hash"));
    assert!(unrecognized.contains("BIOS.col"));
}

#[wasm_bindgen_test]
fn wasm_coleco_inventory_backend_lifecycle_covers_video_audio_input_and_state() {
    let bios = bios_with_video_and_audio_program();
    let bios_sha256 = zeff_firmware::sha256_bytes(&bios);
    let mut inventory = zeff_firmware::FirmwareInventory::new();
    inventory.add(inventory_entry(bios, Some(RETAIL_COLECO_BIOS_MD5)));

    let mut loaded = load_with_inventory(inventory).expect("synthetic catalog BIOS should resolve");
    assert!(matches!(
        loaded.backend.replay_metadata().firmware.as_slice(),
        [zeff_emu_common::replay::ReplayFirmwareManifest::External {
            firmware_id,
            variant: Some(variant),
            sha256,
        }] if firmware_id == "coleco.vision.bios"
            && variant == "coleco.vision.bios.retail"
            && *sha256 == bios_sha256
    ));

    loaded.backend.set_input(0x01, 0x61);
    loaded.backend.set_input_p2(0x02, 0x0A);
    let EmuBackend::Coleco(coleco) = &loaded.backend else {
        panic!("browser Coleco load selected the wrong backend");
    };
    assert_eq!(
        coleco.emu.controller_ports().player(0),
        Some(zeff_coleco_core::StandardController {
            right: true,
            left_button: true,
            keypad: Some(zeff_coleco_core::KeypadKey::Five),
            ..zeff_coleco_core::StandardController::default()
        })
    );
    assert_eq!(
        coleco.emu.controller_ports().player(1),
        Some(zeff_coleco_core::StandardController {
            left: true,
            down: true,
            right_button: true,
            ..zeff_coleco_core::StandardController::default()
        })
    );

    FrameLifecycle::step_frame(&mut loaded.backend);
    assert_eq!(FrameLifecycle::frame_count(&loaded.backend), 1);
    let checkpoint_framebuffer = loaded.backend.framebuffer().to_vec();
    assert_eq!(
        checkpoint_framebuffer.len(),
        ActiveSystem::Coleco.framebuffer_len()
    );
    assert!(
        checkpoint_framebuffer
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [0x54, 0x55, 0xED, 0xFF])
    );

    let mut audio = Vec::new();
    loaded.backend.drain_audio_samples_into(&mut audio);
    assert!(!audio.is_empty());
    assert!(audio.iter().all(|sample| sample.is_finite()));
    assert!(audio.iter().any(|sample| *sample != 0.0));

    let checkpoint_state = loaded.backend.encode_state_bytes().unwrap();
    let EmuBackend::Coleco(coleco) = &mut loaded.backend else {
        unreachable!();
    };
    coleco.emu.bus_mut().vdp_mut().write_register(7, 0x02);
    FrameLifecycle::step_frame(&mut loaded.backend);
    assert_ne!(loaded.backend.framebuffer(), checkpoint_framebuffer);

    loaded
        .backend
        .load_state_from_bytes(checkpoint_state.clone())
        .unwrap();
    assert_eq!(loaded.backend.framebuffer(), checkpoint_framebuffer);
    assert_eq!(
        loaded.backend.encode_state_bytes().unwrap(),
        checkpoint_state
    );
}
