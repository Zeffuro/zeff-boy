use std::collections::BTreeMap;

use anyhow::Result;
use zeff_emu_common::replay::{ReplayEvent, ReplayStartMetadata};

use super::*;
use crate::emu_backend::gb::{GbPersistentLoadOutcome, GbTasLoadProvenanceSeed, GbTasLoadSetup};
use crate::emu_backend::{ActiveSystem, EmuBackend, GbBackend, load_backend_from_rom_source};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasEditorSession, TasInitialBranch,
    TasInputFrame, TasSeekStateCache,
};
use crate::test_support::{build_gb_test_rom, test_directory};

fn write_direct_rom(label: &str) -> Result<(crate::test_support::TestDirectory, PathBuf, Vec<u8>)> {
    let directory = test_directory(label)?;
    let path = directory.path().join("game.gb");
    let bytes = build_gb_test_rom();
    std::fs::write(&path, &bytes)?;
    Ok((directory, path, bytes))
}

#[test]
fn creates_reopens_and_seeks_a_direct_rom_only_project() -> Result<()> {
    let (directory, source_path, source_bytes) = write_direct_rom("tas-direct-gb-flow")?;
    let loader = DirectGbTasExecutionLoader::new(source_path, Vec::new());
    let project_path = directory.path().join("movie.ztas");

    let in_memory = loader.create_project()?;
    let decoded = TasProject::decode(&in_memory.encode()?)?;
    assert_eq!(decoded.project_id(), in_memory.project_id());
    assert_eq!(decoded.identity(), in_memory.identity());
    assert_eq!(decoded.start_state(), in_memory.start_state());
    assert_eq!(decoded.replay_start(), in_memory.replay_start());
    assert_eq!(decoded.branches(), in_memory.branches());
    assert_eq!(decoded.edit_generation(), in_memory.edit_generation());
    assert_eq!(decoded.rerecord_count(), in_memory.rerecord_count());
    assert_eq!(decoded.active_branch_id(), in_memory.active_branch_id());
    let created = loader.create_project_file(&project_path)?;
    let reopened = TasProject::load(&project_path)?;
    assert_eq!(created, reopened);
    assert_eq!(
        reopened.project_id(),
        format!("gb-{}", TasDigest::from_bytes(&source_bytes).to_hex())
    );
    assert_eq!(reopened.identity().devices, direct_gb_tas_devices());
    assert_eq!(
        reopened.identity().sync_config_sha256,
        direct_gb_tas_sync_config_sha256()
    );
    assert_eq!(reopened.branches().len(), 1);
    assert_eq!(reopened.branches()[0].frame_count(), 1);

    let mut engine = loader.load_editor_engine(&reopened)?;
    let manual_path = directory.path().join("manual.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(reopened, manual_path, autosaves, seek_cache)?;
    let outcome = engine.seek(&mut editor, 1)?;
    assert!(outcome.reached_target());
    assert_eq!(outcome.cursor, 1);
    assert_eq!(outcome.framebuffer.width(), 160);
    assert_eq!(outcome.framebuffer.height(), 144);
    Ok(())
}

#[test]
fn rejects_ineligible_media_before_creating_a_project() -> Result<()> {
    let (directory, source_path, _) = write_direct_rom("tas-direct-gb-media")?;
    for label in ["wrong-size", "mapper", "declared-size", "ram", "cgb"] {
        let mut bytes = build_gb_test_rom();
        match label {
            "wrong-size" => {
                bytes.pop();
            }
            "mapper" => bytes[0x147] = 0x01,
            "declared-size" => bytes[0x148] = 0x01,
            "ram" => bytes[0x149] = 0x02,
            "cgb" => bytes[0x143] = 0xC0,
            _ => unreachable!(),
        }
        std::fs::write(&source_path, bytes)?;
        let error = DirectGbTasExecutionLoader::new(source_path.clone(), Vec::new())
            .create_project()
            .unwrap_err();
        assert!(!error.to_string().is_empty(), "{label}");
    }
    drop(directory);
    Ok(())
}

#[test]
fn strict_state_and_fresh_baseline_are_required() -> Result<()> {
    let (_directory, source_path, _) = write_direct_rom("tas-direct-gb-state")?;
    let loader = DirectGbTasExecutionLoader::new(source_path, Vec::new());
    let project = loader.create_project()?;
    let mut legacy = project.start_state().to_vec();
    legacy[8..12].copy_from_slice(&12u32.to_le_bytes());
    assert!(loader.load_session(&legacy).is_err());

    let mut changed = project.start_state().to_vec();
    let last = changed.len() - 1;
    changed[last] ^= 1;
    assert!(loader.load_session(&changed).is_err());
    Ok(())
}

#[test]
fn identity_rejects_runtime_facts_outside_the_profile() -> Result<()> {
    let (_directory, source_path, source_bytes) = write_direct_rom("tas-direct-gb-runtime")?;
    let backend = load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        &source_path,
        &source_path,
        None,
        BackendLoadConfig {
            gb_hardware_mode_preference: HardwareModePreference::Auto,
            gb_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )?
    .backend;
    let start_state = backend.encode_state_bytes()?;
    assert!(direct_gb_tas_identity(&backend, &source_bytes, &start_state).is_err());
    Ok(())
}

fn profile_backend(
    setup: GbTasLoadSetup,
    raw_source_media_len: usize,
    persistent_load: GbPersistentLoadOutcome,
    external_boot_rom: bool,
) -> EmuBackend {
    let source = build_gb_test_rom();
    let path = PathBuf::from("profile.gb");
    let emu = if external_boot_rom {
        zeff_gb_core::emulator::Emulator::from_rom_data_with_boot_rom(
            &source,
            setup.requested_hardware_mode,
            &[0; 0x100],
        )
        .unwrap()
    } else {
        zeff_gb_core::emulator::Emulator::from_rom_data(&source, setup.requested_hardware_mode)
            .unwrap()
    };
    let provenance = GbTasLoadProvenanceSeed::new(
        zeff_firmware::sha256_bytes(&source),
        raw_source_media_len,
        &path,
        &path,
        setup,
    )
    .finish(
        persistent_load,
        emu.hardware_mode(),
        emu.sample_rate(),
        emu.has_boot_rom(),
    );
    let mut backend = EmuBackend::Gb(Box::new(GbBackend::with_load_provenance(
        emu,
        path.clone(),
        path,
        provenance,
    )));
    backend.set_firmware_manifests(
        crate::emu_backend::firmware::default_firmware_manifests_for_active_system(
            ActiveSystem::GameBoy,
        ),
    );
    backend
}

#[test]
fn runtime_rejects_every_direct_gb_profile_deviation() {
    let setup = GbTasLoadSetup {
        loaded_from_source_path: true,
        requested_hardware_mode: HardwareModePreference::ForceDmg,
        ..GbTasLoadSetup::default()
    };
    assert!(
        validate_direct_gb_tas_runtime(
            &profile_backend(setup, 32 * 1024, GbPersistentLoadOutcome::Absent, false,),
            false,
        )
        .is_ok()
    );

    let deviations = [
        (
            "route",
            GbTasLoadSetup {
                loaded_from_source_path: false,
                ..setup
            },
            32 * 1024,
            GbPersistentLoadOutcome::Absent,
            false,
        ),
        (
            "length",
            setup,
            32 * 1024 + 1,
            GbPersistentLoadOutcome::Absent,
            false,
        ),
        (
            "mods",
            GbTasLoadSetup {
                any_mod_enabled: true,
                ..setup
            },
            32 * 1024,
            GbPersistentLoadOutcome::Absent,
            false,
        ),
        (
            "persistence",
            setup,
            32 * 1024,
            GbPersistentLoadOutcome::Loaded,
            false,
        ),
        (
            "boot",
            setup,
            32 * 1024,
            GbPersistentLoadOutcome::Absent,
            true,
        ),
        (
            "hardware",
            GbTasLoadSetup {
                requested_hardware_mode: HardwareModePreference::Auto,
                ..setup
            },
            32 * 1024,
            GbPersistentLoadOutcome::Absent,
            false,
        ),
    ];
    for (label, setup, len, persistent_load, external_boot_rom) in deviations {
        assert!(
            validate_direct_gb_tas_runtime(
                &profile_backend(setup, len, persistent_load, external_boot_rom),
                false,
            )
            .is_err(),
            "{label}"
        );
    }

    let mut sample_rate = profile_backend(setup, 32 * 1024, GbPersistentLoadOutcome::Absent, false);
    sample_rate.set_sample_rate(44_100);
    assert!(validate_direct_gb_tas_runtime(&sample_rate, false).is_err());

    let mut palette = profile_backend(setup, 32 * 1024, GbPersistentLoadOutcome::Absent, false);
    let EmuBackend::Gb(gb) = &mut palette else {
        unreachable!();
    };
    gb.emu
        .set_dmg_palette_preset(zeff_gb_core::hardware::ppu::DmgPalettePreset::Mint);
    assert!(validate_direct_gb_tas_runtime(&palette, false).is_err());

    let mut serial = profile_backend(setup, 32 * 1024, GbPersistentLoadOutcome::Absent, false);
    assert!(
        serial.set_game_boy_serial_device(zeff_gb_core::hardware::GameBoySerialDevice::Printer)
    );
    assert!(validate_direct_gb_tas_runtime(&serial, false).is_err());

    let mut firmware = profile_backend(setup, 32 * 1024, GbPersistentLoadOutcome::Absent, false);
    firmware.set_firmware_manifests(Vec::new());
    assert!(validate_direct_gb_tas_runtime(&firmware, false).is_err());

    let cheats = profile_backend(setup, 32 * 1024, GbPersistentLoadOutcome::Absent, false);
    assert!(validate_direct_gb_tas_runtime(&cheats, true).is_err());
}

#[test]
fn branch_scope_rejects_non_p1_input_events_and_nondefault_start_metadata() -> Result<()> {
    let (_directory, source_path, _) = write_direct_rom("tas-direct-gb-branch")?;
    let loader = DirectGbTasExecutionLoader::new(source_path, Vec::new());
    let project = loader.create_project()?;
    let mut p2 = project.clone();
    p2.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
            TasInputFrame {
                players: [
                    TasControllerInput::default(),
                    TasControllerInput {
                        buttons: 1,
                        dpad: 0,
                    },
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                ],
                ..TasInputFrame::default()
            },
        )
    })?;
    assert!(DirectGbTasExecutionLoader::validate_project_branch_scope(&p2, "main").is_err());

    let mut high_bits = project.clone();
    high_bits.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
            TasInputFrame {
                players: [
                    TasControllerInput {
                        buttons: 0x10,
                        dpad: 0,
                    },
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                    TasControllerInput::default(),
                ],
                ..TasInputFrame::default()
            },
        )
    })?;
    assert!(DirectGbTasExecutionLoader::validate_project_branch_scope(&high_bits, "main").is_err());

    let mut special_input = project.clone();
    special_input.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
            TasInputFrame {
                zapper: crate::tas_project::TasZapperInput {
                    enabled: true,
                    ..crate::tas_project::TasZapperInput::default()
                },
                ..TasInputFrame::default()
            },
        )
    })?;
    assert!(
        DirectGbTasExecutionLoader::validate_project_branch_scope(&special_input, "main").is_err()
    );

    let mut event = project.clone();
    event.edit_transaction(|edit| {
        edit.replace_branch_events("main", vec![ReplayEvent::FdsDiskSide { frame: 0, side: 0 }])
    })?;
    assert!(DirectGbTasExecutionLoader::validate_project_branch_scope(&event, "main").is_err());

    let identity = project.identity().clone();
    let start_state = project.start_state().to_vec();
    let replay_start = ReplayStartMetadata {
        game_boy_link_tick: Some(0),
        ..ReplayStartMetadata::default()
    };
    let linked = TasProject::new(
        "linked",
        identity,
        start_state,
        replay_start,
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 1,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )?;
    assert!(DirectGbTasExecutionLoader::validate_project_branch_scope(&linked, "main").is_err());
    Ok(())
}
