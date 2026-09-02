use std::fs;

use anyhow::Result;

use super::*;
use crate::emu_thread::TasExecutionProfile;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasDigest, TasEditorSession, TasExternalIdentity,
    TasInputFrame, TasProject, TasSeekStateCache,
};

const TEST_SYSTEM_CARD_SHA256: [u8; 32] = [
    0x8A, 0x39, 0xD2, 0xAB, 0xD3, 0x99, 0x9A, 0xB7, 0x3C, 0x34, 0xDB, 0x24, 0x76, 0x84, 0x9C, 0xDD,
    0xF3, 0x03, 0xCE, 0x38, 0x9B, 0x35, 0x82, 0x68, 0x50, 0xF9, 0xA7, 0x00, 0x58, 0x9B, 0x4A, 0x90,
];

fn fixture(
    name: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
)> {
    let directory = crate::test_support::test_directory(name)?;
    let cue_path = directory.path().join("disc.cue");
    let disc_path = directory.path().join("disc.bin");
    let mut disc = vec![0; zeff_pce_core::hardware::CD_USER_SECTOR_BYTES * 4];
    for (index, byte) in disc.iter_mut().enumerate() {
        *byte = index as u8;
    }
    fs::write(&disc_path, disc)?;
    fs::write(
        &cue_path,
        b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    assert_eq!(
        zeff_firmware::sha256_bytes(system_card),
        TEST_SYSTEM_CARD_SHA256
    );
    Ok((
        directory,
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            cue_path,
            system_card,
            TEST_SYSTEM_CARD_SHA256,
        ),
    ))
}

fn chd_fixture(
    name: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
)> {
    let directory = crate::test_support::test_directory(name)?;
    let chd_path = directory.path().join("disc.chd");
    crate::emu_backend::pce_cd_chd::write_synthetic_uncompressed_v5_chd(&chd_path)?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    Ok((
        directory,
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            chd_path,
            system_card,
            TEST_SYSTEM_CARD_SHA256,
        ),
    ))
}

fn iso_fixture(
    name: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
)> {
    let directory = crate::test_support::test_directory(name)?;
    let iso_path = directory.path().join("disc.iso");
    let cue_path = directory.path().join("disc.cue");
    let mut disc = vec![0; zeff_pce_core::hardware::CD_USER_SECTOR_BYTES * 4];
    for (index, byte) in disc.iter_mut().enumerate() {
        *byte = index as u8;
    }
    fs::write(&iso_path, disc)?;
    fs::write(
        cue_path,
        b"FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    Ok((
        directory,
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            iso_path,
            system_card,
            TEST_SYSTEM_CARD_SHA256,
        ),
    ))
}

fn ppf1(offset: u32, bytes: &[u8]) -> Vec<u8> {
    let mut patch = b"PPF10\0".to_vec();
    patch.resize(56, 0);
    patch.extend_from_slice(&offset.to_le_bytes());
    patch.push(bytes.len() as u8);
    patch.extend_from_slice(bytes);
    patch
}

#[test]
fn create_reopen_execute_and_continue_direct_pce_cd_without_host_persistence() -> Result<()> {
    let (directory, loader) = fixture("pce-cd-tas-create")?;
    let project_path = directory.path().join("movie.ztas");
    let mut project = loader.create_project_file(&project_path)?;
    assert_eq!(TasProject::load(&project_path)?, project);
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&project)?,
        TasExecutionProfile::DirectPceCd
    );
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::Absent
    );
    assert_eq!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    assert_eq!(project.identity().firmware.len(), 1);

    let mut input = TasInputFrame::default();
    input.players[0].buttons = 0x03;
    input.players[0].dpad = 0x04;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let mut engine = loader.load_editor_engine(&project)?;
    let autosaves =
        TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, &project_path, autosaves, cache)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());

    let mut expected = loader.load_fresh_backend()?;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.step_frame();
    assert_eq!(
        engine.backend().encode_state_bytes()?,
        expected.encode_state_bytes()?
    );
    let mut actual = engine.into_backend();
    actual.step_frame();
    expected.step_frame();
    assert_eq!(actual.encode_state_bytes()?, expected.encode_state_bytes()?);
    assert_eq!(actual.flush_battery_sram()?, None);
    let replay_path = directory.path().join("verified.zrpl");
    let plan = super::super::PrivateTasExecutionLoader::DirectPceCd(loader.clone());
    assert_eq!(
        plan.verify_and_export_editor_session(&mut editor, &replay_path)?,
        replay_path
    );
    assert!(replay_path.exists());
    Ok(())
}

#[test]
fn direct_chd_memory_base_binds_raw_source_and_normalized_disc_before_continuation() -> Result<()> {
    let (directory, loader) = chd_fixture("pce-cd-tas-chd-memory-base")?;
    let normalized_disc_sha256 = loader
        .load_fresh_backend()?
        .pce()
        .expect("direct PC Engine CD fixture must load a PC Engine backend")
        .normalized_disc_hash()
        .expect("direct PC Engine CD fixture must mount a normalized disc");
    let _memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
            normalized_disc_sha256,
        );
    let mut project = loader.create_project()?;
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::direct_pce_cd::direct_pce_cd_chd_memory_base_tas_sync_config_sha256()
    );
    let mut input = TasInputFrame::default();
    input.players[0].buttons = 0x03;
    input.players[0].dpad = 0x04;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let autosaves = TasAutosaveStore::beside_manual_save(
        &directory.path().join("movie.ztas"),
        TasAutosaveConfig::default(),
    )?;
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(
        project.clone(),
        directory.path().join("movie.ztas"),
        autosaves,
        cache,
    )?;
    let mut engine = loader.load_editor_engine(&project)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    let mut expected = loader.load_fresh_backend()?;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.step_frame();
    assert_eq!(
        engine.backend().encode_state_bytes()?,
        expected.encode_state_bytes()?
    );

    let path = directory.path().join("disc.chd");
    let mut bytes = fs::read(&path)?;
    bytes[4 * 2_448] ^= 1;
    fs::write(path, bytes)?;
    assert!(loader.load_editor_engine(&project).is_err());
    Ok(())
}

#[test]
fn direct_iso_memory_base_binds_raw_source_and_rejects_mutated_or_ambiguous_cue_selection()
-> Result<()> {
    let (directory, loader) = iso_fixture("pce-cd-tas-iso-memory-base")?;
    let normalized_disc_sha256 = loader
        .load_fresh_backend()?
        .pce()
        .expect("direct PC Engine CD fixture must load a PC Engine backend")
        .normalized_disc_hash()
        .expect("direct PC Engine CD fixture must mount a normalized disc");
    let _memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
            normalized_disc_sha256,
        );
    let project = loader.create_project()?;
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::direct_pce_cd::direct_pce_cd_iso_memory_base_tas_sync_config_sha256()
    );

    let iso_path = directory.path().join("disc.iso");
    let mut bytes = fs::read(&iso_path)?;
    bytes[0] ^= 1;
    fs::write(&iso_path, bytes)?;
    assert!(loader.load_editor_engine(&project).is_err());

    let (directory, loader) = iso_fixture("pce-cd-tas-iso-memory-base-ambiguous")?;
    let normalized_disc_sha256 = loader
        .load_fresh_backend()?
        .pce()
        .expect("direct PC Engine CD fixture must load a PC Engine backend")
        .normalized_disc_hash()
        .expect("direct PC Engine CD fixture must mount a normalized disc");
    let _memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
            normalized_disc_sha256,
        );
    let project = loader.create_project()?;
    fs::write(
        directory.path().join("duplicate.cue"),
        b"FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )?;
    assert!(loader.load_editor_engine(&project).is_err());
    Ok(())
}

#[test]
fn direct_ppf_binds_ordered_snapshot_and_normalized_disc_before_reopen() -> Result<()> {
    let (directory, base_loader) = fixture("pce-cd-tas-ppf")?;
    let cue_path = directory.path().join("disc.cue");
    let first = ppf1(0, &[0xA5]);
    let second = ppf1(1, &[0x5A]);
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![
            ("first.ppf".to_owned(), first.clone()),
            ("second.ppf".to_owned(), second.clone()),
        ],
    )?;
    let reversed = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![
            ("second.ppf".to_owned(), second),
            ("first.ppf".to_owned(), first),
        ],
    )?;
    assert_ne!(
        stack.source_media_identity(),
        reversed.source_media_identity()
    );
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        cue_path,
        base_loader.system_card_override.unwrap(),
        TEST_SYSTEM_CARD_SHA256,
        stack,
    );
    let project = loader.create_project()?;
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::direct_pce_cd::direct_pce_cd_ppf_tas_sync_config_sha256()
    );
    let disc_path = directory.path().join("disc.bin");
    let mut bytes = fs::read(&disc_path)?;
    bytes[2] ^= 1;
    fs::write(disc_path, bytes)?;
    assert!(loader.load_editor_engine(&project).is_err());
    Ok(())
}

#[test]
fn direct_ppf_memory_base_reopens_and_rejects_patch_order_or_base_mutation() -> Result<()> {
    let (directory, base_loader) = fixture("pce-cd-tas-ppf-memory-base")?;
    let cue_path = directory.path().join("disc.cue");
    let base_disc_sha256 = base_loader
        .load_fresh_backend()?
        .pce()
        .expect("direct PC Engine CD fixture must load a PC Engine backend")
        .normalized_disc_hash()
        .expect("direct PC Engine CD fixture must mount a normalized disc");
    let _memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(base_disc_sha256);
    let first = ppf1(0, &[0xA5]);
    let second = ppf1(1, &[0x5A]);
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![
            ("first.ppf".to_owned(), first.clone()),
            ("second.ppf".to_owned(), second.clone()),
        ],
    )?;
    let mutated = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![
            ("first.ppf".to_owned(), ppf1(0, &[0xA4])),
            ("second.ppf".to_owned(), second.clone()),
        ],
    )?;
    let reversed = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![
            ("second.ppf".to_owned(), second),
            ("first.ppf".to_owned(), first),
        ],
    )?;
    let system_card = base_loader.system_card_override.unwrap();
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        cue_path.clone(),
        system_card,
        TEST_SYSTEM_CARD_SHA256,
        stack,
    );
    let project_path = directory.path().join("movie.ztas");
    let project = loader.create_project_file(&project_path)?;
    assert_eq!(TasProject::load(&project_path)?, project);
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::direct_pce_cd::direct_pce_cd_ppf_memory_base_tas_sync_config_sha256()
    );
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    assert_eq!(
        loader
            .load_editor_engine(&project)?
            .backend()
            .pce()
            .unwrap()
            .memory_base_mode(),
        PceMemoryBaseMode::Enabled
    );

    let reversed_loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        cue_path.clone(),
        system_card,
        TEST_SYSTEM_CARD_SHA256,
        reversed,
    );
    assert!(reversed_loader.load_editor_engine(&project).is_err());
    let mutated_loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        cue_path,
        system_card,
        TEST_SYSTEM_CARD_SHA256,
        mutated,
    );
    assert!(mutated_loader.load_editor_engine(&project).is_err());

    let disc_path = directory.path().join("disc.bin");
    let mut bytes = fs::read(&disc_path)?;
    bytes[3] ^= 1;
    fs::write(disc_path, bytes)?;
    assert!(loader.load_editor_engine(&project).is_err());
    Ok(())
}

#[test]
fn direct_pce_cd_rejects_mutation_extensions_and_incompatible_runtime() -> Result<()> {
    let (directory, loader) = fixture("pce-cd-tas-reject")?;
    let project = loader.create_project()?;
    fs::write(directory.path().join("disc.bin"), vec![0xA5; 4 * 2048])?;
    assert!(loader.load_editor_engine(&project).is_err());

    for extension in ["zip", "chd", "iso"] {
        let path = directory.path().join(format!("disc.{extension}"));
        fs::write(&path, [])?;
        assert!(
            DirectPceCdTasExecutionLoader::new_with_system_card_override(
                path,
                Box::leak(vec![0; 256 * 1024].into_boxed_slice()),
                TEST_SYSTEM_CARD_SHA256,
            )
            .load_fresh_backend()
            .is_err()
        );
    }

    let (_topology_directory, loader) = fixture("pce-cd-tas-topology")?;
    let mut backend = loader.load_fresh_backend()?;
    let EmuBackend::Pce(pce) = &mut backend else {
        unreachable!();
    };
    pce.update_controller_mode(PceControllerMode::SixButton);
    assert!(validate_direct_pce_cd_tas_runtime(&backend, false).is_err());
    let mut backend = loader.load_fresh_backend()?;
    let EmuBackend::Pce(pce) = &mut backend else {
        unreachable!();
    };
    pce.update_memory_base_mode(PceMemoryBaseMode::Enabled);
    assert!(validate_direct_pce_cd_tas_runtime(&backend, false).is_err());
    assert!(validate_direct_pce_cd_tas_runtime(&loader.load_fresh_backend()?, true).is_err());
    assert_ne!(TasDigest(TEST_SYSTEM_CARD_SHA256), TasDigest([0; 32]));
    Ok(())
}
