use super::*;

struct Fixture {
    directory: crate::test_support::TestDirectory,
    loader: DirectPceCdTasExecutionLoader,
    disc_sha256: [u8; 32],
    memory_base_catalog: crate::emu_backend::pce_profiles::TestMemoryBaseCatalogGuard,
    controller_catalog: crate::emu_backend::pce_profiles::TestControllerCatalogGuard,
}

fn fixture_with_catalogs(name: &str) -> Result<Fixture> {
    let (directory, base) = fixture(name)?;
    let disc_sha256 = base
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture disc");
    let memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(disc_sha256);
    let controller_catalog =
        crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            disc_sha256,
            PceControllerMode::Multitap,
        );
    let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
        directory.path().join("disc.cue"),
        base.system_card_override.expect("fixture system card"),
        TEST_SYSTEM_CARD_SHA256,
    );
    Ok(Fixture {
        directory,
        loader,
        disc_sha256,
        memory_base_catalog,
        controller_catalog,
    })
}

fn five_player_input() -> TasInputFrame {
    let mut input = TasInputFrame::default();
    for (index, player) in input.players.iter_mut().enumerate() {
        player.buttons = 1 << index.min(3);
        player.dpad = 1 << (3 - index.min(3));
    }
    input
}

#[test]
fn direct_cue_memory_base_multitap_binds_identity_and_strict_native_state() -> Result<()> {
    let Fixture {
        directory,
        loader,
        memory_base_catalog: _memory_base_catalog,
        controller_catalog: _controller_catalog,
        ..
    } = fixture_with_catalogs("pce-cd-tas-memory-base-multitap-identity")?;
    let mut backend = loader.load_fresh_backend()?;
    let inspection =
        super::super::super::direct_pce_cd::validate_direct_pce_multitap_cd_tas_runtime(
            &backend, false,
        )?;
    assert!(!inspection.arcade_card_enabled);
    assert!(inspection.memory_base_enabled);
    let multitap = inspection.controller_multitap.expect("Multitap state");
    assert!(
        multitap
            .buttons
            .into_iter()
            .all(|buttons| buttons.is_empty())
    );
    assert_eq!(multitap.active_port, None);
    assert!(multitap.select_high && multitap.clear_high);

    let path = directory.path().join("movie.ztas");
    let project = loader.create_project_file(&path)?;
    assert_eq!(TasProject::load(&path)?, project);
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::super::direct_pce_cd::direct_pce_multitap_cd_memory_base_tas_sync_config_sha256()
    );
    assert_ne!(
        project.identity().sync_config_sha256,
        super::super::super::direct_pce_cd::direct_pce_multitap_cd_tas_sync_config_sha256()
    );
    assert!(project.identity().patches.is_empty());
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::Absent
    );
    assert_eq!(
        project
            .identity()
            .devices
            .iter()
            .map(|device| device.port.as_str())
            .collect::<Vec<_>>(),
        ["p1", "p2", "p3", "p4", "p5"]
    );
    assert_eq!(
        super::super::super::classify_direct_tas_execution_profile(&project)?,
        TasExecutionProfile::DirectPceMultitapCd
    );
    assert!(
        backend
            .pce()
            .expect("PC Engine backend")
            .inspect_current_native_cd_tas_state_for_profile_and_controller(
                project.start_state(),
                false,
                false,
                PceControllerMode::Multitap,
            )
            .is_err()
    );
    assert_eq!(backend.flush_battery_sram()?, None);

    let mut reopened = DirectPceCdTasExecutionLoader::new_for_project(
        directory.path().join("disc.cue"),
        Vec::new(),
        &project,
    )?;
    reopened.system_card_override = loader.system_card_override;
    reopened.system_card_sha256_override = loader.system_card_sha256_override;
    reopened.load_editor_engine(&project)?;
    assert!(
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            directory.path().join("disc.cue"),
            loader.system_card_override.expect("fixture system card"),
            TEST_SYSTEM_CARD_SHA256,
        )
        .load_session(project.start_state())
        .is_err()
    );
    Ok(())
}

#[test]
fn direct_cue_memory_base_multitap_seeks_replays_and_reauthenticates_source() -> Result<()> {
    let Fixture {
        directory,
        loader,
        memory_base_catalog: _memory_base_catalog,
        controller_catalog: _controller_catalog,
        ..
    } = fixture_with_catalogs("pce-cd-tas-memory-base-multitap-roundtrip")?;
    let mut project = loader.create_project()?;
    let input = five_player_input();
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let manual_path = directory.path().join("source.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;
    let mut engine = loader.load_editor_engine(editor.project())?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    let reached = engine.backend().encode_state_bytes()?;
    assert!(engine.seek(&mut editor, 0)?.reached_target());
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    assert_eq!(engine.backend().encode_state_bytes()?, reached);

    let replay_path = directory.path().join("movie.zrpl");
    super::super::super::PrivateTasExecutionLoader::DirectPceCd(loader.clone())
        .verify_and_export_editor_session(&mut editor, &replay_path)?;
    let _firmware = super::super::register_test_pce_cd_system_card(
        TEST_SYSTEM_CARD_SHA256,
        loader.system_card_override.expect("fixture system card"),
    );
    let start_state = TasProject::read_zrpl_start_state(&replay_path)?;
    let selected = super::super::super::select_private_tas_execution_loader_for_replay(
        directory.path().join("disc.cue"),
        None,
        ActiveSystem::Pce,
        Vec::new(),
        &start_state,
    )?;
    let imported = selected.import_replay_file(
        &replay_path,
        &directory.path().join("imported.ztas"),
        false,
    )?;
    assert_eq!(imported.branch("main").expect("main").input_at(0), input);
    assert_eq!(
        imported.identity().sync_config_sha256,
        super::super::super::direct_pce_cd::direct_pce_multitap_cd_memory_base_tas_sync_config_sha256()
    );

    let disc_path = directory.path().join("disc.bin");
    let mut bytes = fs::read(&disc_path)?;
    bytes[19] ^= 1;
    fs::write(disc_path, bytes)?;
    assert!(loader.load_editor_engine(&imported).is_err());
    Ok(())
}

#[test]
fn direct_cue_memory_base_multitap_requires_independent_catalogs_and_exact_route() -> Result<()> {
    let Fixture {
        directory,
        loader,
        disc_sha256,
        memory_base_catalog,
        controller_catalog,
    } = fixture_with_catalogs("pce-cd-tas-memory-base-multitap-catalogs")?;
    let project = loader.create_project()?;
    drop(memory_base_catalog);
    assert!(loader.load_editor_engine(&project).is_err());
    let memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(disc_sha256);
    drop(controller_catalog);
    assert!(loader.load_editor_engine(&project).is_err());
    let _controller_catalog =
        crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            disc_sha256,
            PceControllerMode::Multitap,
        );

    let arcade_catalog =
        crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256);
    assert!(loader.load_editor_engine(&project).is_err());
    drop(arcade_catalog);

    let cue_path = directory.path().join("disc.cue");
    let mut ppf = loader.clone();
    ppf.ppf_stack_override = Some(crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![("disc.ppf".to_owned(), ppf1(0, &[0xA5]))],
    )?);
    assert!(ppf.load_fresh_backend().is_err());
    assert!(ppf.load_editor_engine(&project).is_err());

    let wrong_card = Box::leak(vec![1; 256 * 1024].into_boxed_slice());
    let wrong_loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
        cue_path,
        wrong_card,
        zeff_firmware::sha256_bytes(wrong_card),
    );
    assert!(wrong_loader.create_project().is_err());
    drop(memory_base_catalog);
    Ok(())
}
