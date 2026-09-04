use super::*;

fn fixture_with_catalog(
    name: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    crate::emu_backend::pce_profiles::TestControllerCatalogGuard,
)> {
    let (directory, base) = chd_fixture(name)?;
    let disc_sha256 = base
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture disc");
    let catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        disc_sha256,
        PceControllerMode::Multitap,
    );
    let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
        directory.path().join("disc.chd"),
        base.system_card_override.expect("fixture system card"),
        TEST_SYSTEM_CARD_SHA256,
    );
    Ok((directory, loader, catalog))
}

fn input() -> TasInputFrame {
    let mut input = TasInputFrame::default();
    for (index, player) in input.players.iter_mut().enumerate() {
        player.buttons = 1 << index.min(3);
        player.dpad = 1 << (3 - index.min(3));
    }
    input
}

#[test]
fn direct_chd_multitap_binds_source_reopens_and_rejects_near_misses() -> Result<()> {
    let (directory, loader, catalog) = fixture_with_catalog("pce-cd-tas-chd-multitap")?;
    let project_path = directory.path().join("movie.ztas");
    let project = loader.create_project_file(&project_path)?;
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::super::direct_pce_cd::direct_pce_multitap_cd_chd_tas_sync_config_sha256()
    );
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    let source_bytes = fs::read(directory.path().join("disc.chd"))?;
    assert_eq!(
        project.identity().source_media_sha256,
        super::super::super::direct_pce_cd::direct_pce_cd_chd_source_identity(
            TasDigest::from_bytes(&source_bytes).0,
            source_bytes.len(),
        )
    );
    assert_eq!(
        super::super::super::classify_direct_tas_execution_profile(&project)?,
        TasExecutionProfile::DirectPceMultitapCd
    );
    assert_eq!(project.identity().devices.len(), 5);

    let mut reopened = DirectPceCdTasExecutionLoader::new_for_project(
        directory.path().join("disc.chd"),
        Vec::new(),
        &project,
    )?;
    reopened.system_card_override = loader.system_card_override;
    reopened.system_card_sha256_override = loader.system_card_sha256_override;
    reopened.load_editor_engine(&TasProject::load(&project_path)?)?;

    let standard = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        directory.path().join("disc.chd"),
        loader.system_card_override.expect("fixture system card"),
        TEST_SYSTEM_CARD_SHA256,
    );
    assert!(standard.load_editor_engine(&project).is_err());
    let standard_state = standard.load_fresh_backend()?.encode_state_bytes()?;
    let mut wrong_identity = project.identity().clone();
    wrong_identity.start_state_sha256 = TasDigest::from_bytes(&standard_state);
    let wrong_state = TasProject::new(
        "wrong-topology",
        wrong_identity,
        standard_state,
        Default::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 1,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )?;
    assert!(loader.load_editor_engine(&wrong_state).is_err());

    let disc_sha256 = project.identity().effective_media_sha256.0;
    let _arcade =
        crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256);
    assert!(loader.load_fresh_backend().is_err());
    drop(_arcade);
    drop(catalog);
    let memory =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(disc_sha256);
    assert!(loader.load_fresh_backend().is_err());
    drop(memory);
    let _catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        disc_sha256,
        PceControllerMode::Multitap,
    );

    let path = directory.path().join("disc.chd");
    let mut bytes = fs::read(&path)?;
    bytes.push(0);
    fs::write(path, bytes)?;
    assert!(reopened.load_editor_engine(&project).is_err());
    Ok(())
}

#[test]
fn direct_chd_multitap_seek_and_replay_roundtrip_preserve_five_players() -> Result<()> {
    let (directory, loader, _catalog) = fixture_with_catalog("pce-cd-tas-chd-multitap-replay")?;
    let mut project = loader.create_project()?;
    let input = input();
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
    let plan = super::super::super::PrivateTasExecutionLoader::DirectPceCd(loader.clone());
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;
    let replay = zeff_emu_common::replay::ReplayPlayer::load(&replay_path)?;
    let frames = replay.peek_joypad_frames(0, 1);
    let frame = &frames[0];
    assert_eq!(
        [
            (frame.buttons, frame.dpad),
            (frame.buttons_p2, frame.dpad_p2),
            (frame.buttons_p3, frame.dpad_p3),
            (frame.buttons_p4, frame.dpad_p4),
            (frame.buttons_p5, frame.dpad_p5),
        ],
        input.players.map(|player| (player.buttons, player.dpad))
    );

    let _system_card = super::super::register_test_pce_cd_system_card(
        TEST_SYSTEM_CARD_SHA256,
        loader.system_card_override.expect("fixture system card"),
    );
    let start_state = TasProject::read_zrpl_start_state(&replay_path)?;
    let selected = super::super::super::select_private_tas_execution_loader_for_replay(
        directory.path().join("disc.chd"),
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
        super::super::super::classify_direct_tas_execution_profile(&imported)?,
        TasExecutionProfile::DirectPceMultitapCd
    );
    Ok(())
}
