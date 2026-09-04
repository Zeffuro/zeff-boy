use super::*;

fn multitap_fixture(
    name: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    crate::emu_backend::pce_profiles::TestControllerCatalogGuard,
)> {
    let (directory, base_loader) = fixture(name)?;
    let disc_sha256 = base_loader
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture must mount a disc");
    let catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        disc_sha256,
        PceControllerMode::Multitap,
    );
    let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
        directory.path().join("disc.cue"),
        base_loader
            .system_card_override
            .expect("fixture system card"),
        TEST_SYSTEM_CARD_SHA256,
    );
    Ok((directory, loader, catalog))
}

fn five_player_input() -> TasInputFrame {
    let mut input = TasInputFrame::default();
    for (index, player) in input.players.iter_mut().enumerate() {
        player.buttons = 1 << index.min(3);
        player.dpad = 1 << (3 - index.min(3));
    }
    input
}

fn ppf_multitap_fixture(
    name: &str,
    mut patches: Vec<(String, Vec<u8>)>,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    crate::emu_backend::pce_profiles::TestControllerCatalogGuard,
    [u8; 32],
)> {
    let (directory, base_loader) = fixture(name)?;
    let cue_path = directory.path().join("disc.cue");
    if patches.is_empty() {
        let bytes = fs::read(directory.path().join("disc.bin"))?;
        patches = vec![
            ("first.ppf".to_owned(), ppf1(0, &bytes[0..1])),
            ("second.ppf".to_owned(), ppf1(1, &bytes[1..2])),
        ];
    }
    let source_disc_sha256 = base_loader
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture must mount a disc");
    let catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        source_disc_sha256,
        PceControllerMode::Multitap,
    );
    let mut loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
        cue_path.clone(),
        base_loader
            .system_card_override
            .expect("fixture system card"),
        TEST_SYSTEM_CARD_SHA256,
    );
    loader.ppf_stack_override = Some(crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path, patches,
    )?);
    Ok((directory, loader, catalog, source_disc_sha256))
}

fn project_with_identity(
    project: &TasProject,
    identity: crate::tas_project::TasProjectIdentity,
) -> Result<TasProject> {
    TasProject::new(
        "mutated",
        identity,
        project.start_state().to_vec(),
        Default::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 1,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )
}

#[test]
fn direct_ppf_multitap_binds_unpatched_witness_and_exact_ordered_stack() -> Result<()> {
    let (directory, loader, _catalog, source_disc_sha256) =
        ppf_multitap_fixture("pce-cd-tas-ppf-multitap-identity", Vec::new())?;
    let base = fs::read(directory.path().join("disc.bin"))?;
    let project_path = directory.path().join("movie.ztas");
    let project = loader.create_project_file(&project_path)?;
    assert_eq!(TasProject::load(&project_path)?, project);
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::super::direct_pce_cd::direct_pce_multitap_cd_ppf_tas_sync_config_sha256()
    );
    assert_eq!(
        project.identity().patches,
        [crate::tas_project::TasPatchIdentity {
            format: super::super::super::direct_pce_cd::PCE_CD_UNPATCHED_DISC_PATCH_FORMAT
                .to_owned(),
            sha256: TasDigest(source_disc_sha256),
        }]
    );
    assert_eq!(
        project.identity().effective_media_sha256,
        TasDigest(source_disc_sha256)
    );
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    loader.load_editor_engine(&project)?;

    for patches in [
        vec![
            ("second.ppf".to_owned(), ppf1(1, &base[1..2])),
            ("first.ppf".to_owned(), ppf1(0, &base[0..1])),
        ],
        vec![
            ("renamed.ppf".to_owned(), ppf1(0, &base[0..1])),
            ("second.ppf".to_owned(), ppf1(1, &base[1..2])),
        ],
        vec![("first.ppf".to_owned(), ppf1(0, &base[0..1]))],
    ] {
        let mut changed = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
            directory.path().join("disc.cue"),
            loader.system_card_override.expect("fixture system card"),
            TEST_SYSTEM_CARD_SHA256,
        );
        changed.ppf_stack_override = Some(crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
            &directory.path().join("disc.cue"),
            patches,
        )?);
        assert!(changed.load_editor_engine(&project).is_err());
    }

    let mut identity = project.identity().clone();
    identity.patches[0].sha256 = TasDigest([0xA5; 32]);
    assert!(
        loader
            .load_editor_engine(&project_with_identity(&project, identity)?)
            .is_err()
    );
    let mut identity = project.identity().clone();
    identity.effective_media_sha256 = TasDigest([0x5A; 32]);
    assert!(
        loader
            .load_editor_engine(&project_with_identity(&project, identity)?)
            .is_err()
    );
    let mut identity = project.identity().clone();
    identity.sync_config_sha256 =
        super::super::super::direct_pce_cd::direct_pce_cd_ppf_tas_sync_config_sha256();
    assert!(
        loader
            .load_editor_engine(&project_with_identity(&project, identity)?)
            .is_err()
    );
    Ok(())
}

#[test]
fn direct_ppf_multitap_reauthenticates_base_patch_bytes_and_source_catalog() -> Result<()> {
    let (directory, loader, catalog, _) = ppf_multitap_fixture(
        "pce-cd-tas-ppf-multitap-mutation",
        vec![("disc.ppf".to_owned(), ppf1(0, &[0xA5]))],
    )?;
    let project = loader.create_project()?;
    let cue_path = directory.path().join("disc.cue");
    let mut changed = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
        cue_path.clone(),
        loader.system_card_override.expect("fixture system card"),
        TEST_SYSTEM_CARD_SHA256,
    );
    changed.ppf_stack_override = Some(crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![("disc.ppf".to_owned(), ppf1(0, &[0xA4]))],
    )?);
    assert!(changed.load_editor_engine(&project).is_err());

    let disc_path = directory.path().join("disc.bin");
    let mut bytes = fs::read(&disc_path)?;
    bytes[7] ^= 1;
    fs::write(&disc_path, bytes)?;
    assert!(loader.load_editor_engine(&project).is_err());
    drop(catalog);

    let (directory, mut loader) = fixture("pce-cd-tas-ppf-multitap-effective-only")?;
    let cue_path = directory.path().join("disc.cue");
    let source_hash = loader
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture disc");
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![("disc.ppf".to_owned(), ppf1(0, &[0xA5]))],
    )?;
    let mut two_button = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        cue_path.clone(),
        loader.system_card_override.expect("fixture system card"),
        TEST_SYSTEM_CARD_SHA256,
    );
    two_button.ppf_stack_override = Some(stack.clone());
    let effective_hash = two_button
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("patched disc");
    loader.controller_mode = PceControllerMode::Multitap;
    loader.ppf_stack_override = Some(stack);
    assert_ne!(effective_hash, source_hash);
    let _effective_only = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        effective_hash,
        PceControllerMode::Multitap,
    );
    assert!(loader.load_fresh_backend().is_err());
    Ok(())
}

#[test]
fn direct_ppf_multitap_replay_auto_selection_preserves_five_players() -> Result<()> {
    let (directory, loader, _catalog, _) = ppf_multitap_fixture(
        "pce-cd-tas-ppf-multitap-replay",
        vec![("disc.ppf".to_owned(), ppf1(0, &[0x70]))],
    )?;
    let mut project = loader.create_project()?;
    let input = five_player_input();
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let project_path = directory.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, project_path, autosaves, cache)?;
    let replay_path = directory.path().join("movie.zrpl");
    let plan = super::super::super::PrivateTasExecutionLoader::DirectPceCd(loader.clone());
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;

    let cue_path = directory.path().join("disc.cue");
    let _card = super::super::register_test_pce_cd_system_card(
        TEST_SYSTEM_CARD_SHA256,
        loader.system_card_override.expect("fixture system card"),
    );
    let _stack = super::super::register_test_pce_cd_ppf_stack(
        cue_path.clone(),
        loader.ppf_stack_override.clone().expect("PPF stack"),
    );
    let start_state = TasProject::read_zrpl_start_state(&replay_path)?;
    let selected = super::super::super::select_private_tas_execution_loader_for_replay(
        cue_path,
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

#[test]
fn legacy_two_button_ppf_identity_remains_patch_list_free() -> Result<()> {
    let (directory, base_loader) = fixture("pce-cd-tas-ppf-legacy-empty-patches")?;
    let cue_path = directory.path().join("disc.cue");
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![("disc.ppf".to_owned(), ppf1(0, &[0xA5]))],
    )?;
    let expected_source = TasDigest(stack.source_media_identity().0);
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        cue_path,
        base_loader
            .system_card_override
            .expect("fixture system card"),
        TEST_SYSTEM_CARD_SHA256,
        stack,
    );
    let project = loader.create_project()?;
    assert!(project.identity().patches.is_empty());
    assert_eq!(project.identity().source_media_sha256, expected_source);
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::super::direct_pce_cd::direct_pce_cd_ppf_tas_sync_config_sha256()
    );
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    loader.load_editor_engine(&project)?;
    Ok(())
}

#[test]
fn direct_cue_multitap_binds_exact_catalog_identity_and_reset_mux() -> Result<()> {
    let (directory, loader, _catalog) = multitap_fixture("pce-cd-tas-multitap-identity")?;
    let backend = loader.load_fresh_backend()?;
    let inspection =
        super::super::super::direct_pce_cd::validate_direct_pce_multitap_cd_tas_runtime(
            &backend, false,
        )?;
    let multitap = inspection.controller_multitap.expect("Multitap state");
    assert!(
        multitap
            .buttons
            .into_iter()
            .all(|buttons| buttons.is_empty())
    );
    assert_eq!(multitap.active_port, None);
    assert!(multitap.select_high && multitap.clear_high);

    let project_path = directory.path().join("movie.ztas");
    let project = loader.create_project_file(&project_path)?;
    assert_eq!(
        super::super::super::classify_direct_tas_execution_profile(&project)?,
        TasExecutionProfile::DirectPceMultitapCd
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::super::direct_pce_cd::direct_pce_multitap_cd_tas_sync_config_sha256()
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
    let mut reopened = DirectPceCdTasExecutionLoader::new_for_project(
        directory.path().join("disc.cue"),
        Vec::new(),
        &project,
    )?;
    reopened.system_card_override = loader.system_card_override;
    reopened.system_card_sha256_override = loader.system_card_sha256_override;
    reopened.load_editor_engine(&TasProject::load(&project_path)?)?;

    assert!(
        DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
            directory.path().join("disc.zip"),
            loader.system_card_override.expect("fixture system card"),
            TEST_SYSTEM_CARD_SHA256,
        )
        .load_fresh_backend()
        .is_err()
    );
    assert!(
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            directory.path().join("disc.cue"),
            loader.system_card_override.expect("fixture system card"),
            TEST_SYSTEM_CARD_SHA256,
        )
        .load_editor_engine(&project)
        .is_err()
    );
    Ok(())
}

#[test]
fn direct_cue_multitap_isolated_seek_and_replay_roundtrip_preserve_five_players() -> Result<()> {
    let (directory, loader, _catalog) = multitap_fixture("pce-cd-tas-multitap-roundtrip")?;
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

    let imported_path = directory.path().join("imported.ztas");
    let start_state = TasProject::read_zrpl_start_state(&replay_path)?;
    let selected = super::super::super::PrivateTasExecutionLoader::DirectPceCd(loader.clone());
    loader.load_session(&start_state)?;
    assert!(
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            directory.path().join("disc.cue"),
            loader.system_card_override.expect("fixture system card"),
            TEST_SYSTEM_CARD_SHA256,
        )
        .load_session(&start_state)
        .is_err()
    );
    let imported = selected.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(
        imported.branch("main").expect("main branch").input_at(0),
        input
    );
    assert_eq!(
        super::super::super::classify_direct_tas_execution_profile(&imported)?,
        TasExecutionProfile::DirectPceMultitapCd
    );
    Ok(())
}

#[test]
fn direct_cue_multitap_rejects_catalog_device_card_and_patch_near_misses() -> Result<()> {
    let (directory, base) = fixture("pce-cd-tas-multitap-near-misses")?;
    let system_card = base.system_card_override.expect("fixture system card");
    let cue_path = directory.path().join("disc.cue");
    let disc_sha256 = base
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture disc");
    let mut loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
        cue_path.clone(),
        system_card,
        TEST_SYSTEM_CARD_SHA256,
    );
    assert!(loader.load_fresh_backend().is_err());

    let catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        disc_sha256,
        PceControllerMode::Multitap,
    );
    let project = loader.create_project()?;
    let mut identity = project.identity().clone();
    identity.devices.pop();
    let invalid = TasProject::new(
        "invalid-device",
        identity,
        project.start_state().to_vec(),
        Default::default(),
        crate::tas_project::TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 1,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        std::collections::BTreeMap::new(),
    )?;
    assert!(loader.load_editor_engine(&invalid).is_err());

    drop(catalog);
    let _arcade =
        crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256);
    assert!(loader.load_fresh_backend().is_err());
    drop(_arcade);
    let catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        disc_sha256,
        PceControllerMode::Multitap,
    );
    loader.ppf_stack_override = Some(crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![("disc.ppf".to_owned(), ppf1(0, &[0xA5]))],
    )?);
    let _arcade =
        crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256);
    assert!(loader.load_fresh_backend().is_err());
    drop(_arcade);
    drop(catalog);
    Ok(())
}

#[test]
fn direct_cue_multitap_rejects_invalid_media_and_unsupported_branch_data() -> Result<()> {
    let (directory, loader, _catalog) = multitap_fixture("pce-cd-tas-multitap-scope")?;
    let system_card = loader.system_card_override.expect("fixture system card");
    let path = directory.path().join("disc.unsupported");
    fs::write(&path, [])?;
    assert!(
        DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
            path,
            system_card,
            TEST_SYSTEM_CARD_SHA256,
        )
        .load_fresh_backend()
        .is_err()
    );

    let project = loader.create_project()?;
    let mut high_bits = project.clone();
    let mut input = TasInputFrame::default();
    input.players[4].buttons = 0x10;
    high_bits.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    assert!(loader.load_editor_engine(&high_bits).is_err());

    let mut special = project.clone();
    let input = TasInputFrame {
        tilt_x_bits: 1,
        ..Default::default()
    };
    special.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    assert!(loader.load_editor_engine(&special).is_err());

    let mut event = project.clone();
    event.edit_transaction(|edit| {
        edit.replace_branch_events(
            "main",
            vec![zeff_emu_common::replay::ReplayEvent::FdsDiskSide { frame: 0, side: 0 }],
        )
    })?;
    assert!(loader.load_editor_engine(&event).is_err());

    let replay_start = TasProject::new(
        "replay-start",
        project.identity().clone(),
        project.start_state().to_vec(),
        zeff_emu_common::replay::ReplayStartMetadata {
            game_boy_link_tick: Some(0),
            ..Default::default()
        },
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 1,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )?;
    assert!(loader.load_editor_engine(&replay_start).is_err());

    let standard = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        directory.path().join("disc.cue"),
        system_card,
        TEST_SYSTEM_CARD_SHA256,
    );
    let state = standard.load_fresh_backend()?.encode_state_bytes()?;
    let mut identity = project.identity().clone();
    identity.start_state_sha256 = TasDigest::from_bytes(&state);
    let wrong_state = TasProject::new(
        "wrong-state",
        identity,
        state,
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
    Ok(())
}
