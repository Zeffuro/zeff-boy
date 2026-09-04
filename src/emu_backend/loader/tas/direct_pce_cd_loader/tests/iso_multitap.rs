use super::*;
use crate::tas_project::TasControllerInput;

fn fixture_with_catalog(
    name: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    crate::emu_backend::pce_profiles::TestControllerCatalogGuard,
)> {
    let (directory, base) = iso_fixture(name)?;
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
        directory.path().join("disc.iso"),
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
fn direct_iso_multitap_binds_source_reopens_and_rejects_profile_near_misses() -> Result<()> {
    let (directory, loader, catalog) = fixture_with_catalog("pce-cd-tas-iso-multitap")?;
    let project_path = directory.path().join("movie.ztas");
    let project = loader.create_project_file(&project_path)?;
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::super::direct_pce_cd::direct_pce_multitap_cd_iso_tas_sync_config_sha256()
    );
    for other in [
        super::super::super::direct_pce_cd::direct_pce_cd_iso_tas_sync_config_sha256(),
        super::super::super::direct_pce_cd::direct_pce_multitap_cd_tas_sync_config_sha256(),
        super::super::super::direct_pce_cd::direct_pce_multitap_cd_chd_tas_sync_config_sha256(),
    ] {
        assert_ne!(project.identity().sync_config_sha256, other);
    }
    let source_bytes = fs::read(directory.path().join("disc.iso"))?;
    assert_eq!(
        project.identity().source_media_sha256,
        super::super::super::direct_pce_cd::direct_pce_cd_iso_source_identity(
            TasDigest::from_bytes(&source_bytes).0,
            source_bytes.len(),
        )
    );
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    assert_eq!(
        super::super::super::classify_direct_tas_execution_profile(&project)?,
        TasExecutionProfile::DirectPceMultitapCd
    );
    assert_eq!(
        project
            .identity()
            .devices
            .iter()
            .map(|device| (device.port.as_str(), device.device.as_str()))
            .collect::<Vec<_>>(),
        [
            ("p1", "pce-two-button-controller"),
            ("p2", "pce-two-button-controller"),
            ("p3", "pce-two-button-controller"),
            ("p4", "pce-two-button-controller"),
            ("p5", "pce-two-button-controller"),
        ]
    );

    let mut reopened = DirectPceCdTasExecutionLoader::new_for_project(
        directory.path().join("disc.iso"),
        Vec::new(),
        &project,
    )?;
    reopened.system_card_override = loader.system_card_override;
    reopened.system_card_sha256_override = loader.system_card_sha256_override;
    reopened.load_editor_engine(&TasProject::load(&project_path)?)?;

    let standard = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        directory.path().join("disc.iso"),
        loader.system_card_override.expect("fixture system card"),
        TEST_SYSTEM_CARD_SHA256,
    );
    assert!(standard.load_editor_engine(&project).is_err());

    let mut wrong_sync_identity = project.identity().clone();
    wrong_sync_identity.sync_config_sha256 =
        super::super::super::direct_pce_cd::direct_pce_multitap_cd_tas_sync_config_sha256();
    let wrong_sync = TasProject::new(
        "wrong-sync",
        wrong_sync_identity,
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
    )?;
    assert!(loader.load_editor_engine(&wrong_sync).is_err());

    let mut wrong_device_identity = project.identity().clone();
    wrong_device_identity.devices.pop();
    let wrong_device = TasProject::new(
        "wrong-device",
        wrong_device_identity,
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
    )?;
    assert!(loader.load_editor_engine(&wrong_device).is_err());

    let mut wrong_branch = project.clone();
    let mut invalid_players: [TasControllerInput; 5] = [Default::default(); 5];
    invalid_players[4].buttons = 0x10;
    let invalid_input = TasInputFrame {
        players: invalid_players,
        ..Default::default()
    };
    wrong_branch.edit_transaction(|edit| edit.set_input_range("main", 0, 1, invalid_input))?;
    assert!(loader.load_editor_engine(&wrong_branch).is_err());

    let disc_sha256 = project.identity().effective_media_sha256.0;
    let arcade =
        crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256);
    assert!(loader.load_fresh_backend().is_err());
    drop(arcade);

    let cue_path = directory.path().join("disc.cue");
    let mut ppf_loader = loader.clone();
    ppf_loader.ppf_stack_override = Some(crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![("disc.ppf".to_owned(), ppf1(0, &[0xA5]))],
    )?);
    assert!(ppf_loader.load_fresh_backend().is_err());

    drop(catalog);
    let memory =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(disc_sha256);
    assert!(loader.load_fresh_backend().is_err());
    drop(memory);
    let wrong_controller = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        disc_sha256,
        PceControllerMode::TwoButton,
    );
    assert!(loader.load_fresh_backend().is_err());
    drop(wrong_controller);
    assert!(loader.load_fresh_backend().is_err());

    for extension in ["7z", "rar", "zip"] {
        let path = directory.path().join(format!("disc.{extension}"));
        fs::write(&path, &source_bytes)?;
        assert!(
            DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
                path,
                loader.system_card_override.expect("fixture system card"),
                TEST_SYSTEM_CARD_SHA256,
            )
            .load_fresh_backend()
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn direct_iso_multitap_requires_one_valid_referring_cue_and_binds_both_media_domains() -> Result<()>
{
    let (missing_directory, missing_loader, _catalog) =
        fixture_with_catalog("pce-cd-tas-iso-multitap-missing-cue")?;
    fs::remove_file(missing_directory.path().join("disc.cue"))?;
    assert!(missing_loader.load_fresh_backend().is_err());

    let (invalid_directory, invalid_loader, _catalog) =
        fixture_with_catalog("pce-cd-tas-iso-multitap-invalid-cue")?;
    fs::write(
        invalid_directory.path().join("disc.cue"),
        b"not a cue sheet",
    )?;
    assert!(invalid_loader.load_fresh_backend().is_err());

    let (ambiguous_directory, ambiguous_loader, _catalog) =
        fixture_with_catalog("pce-cd-tas-iso-multitap-ambiguous-cue")?;
    fs::write(
        ambiguous_directory.path().join("duplicate.cue"),
        b"FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )?;
    assert!(ambiguous_loader.load_fresh_backend().is_err());

    let (cue_directory, cue_loader, _catalog) =
        fixture_with_catalog("pce-cd-tas-iso-multitap-cue-mutation")?;
    let project = cue_loader.create_project()?;
    fs::write(
        cue_directory.path().join("disc.cue"),
        b"FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:01\n",
    )?;
    let changed_disc = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        cue_directory.path().join("disc.iso"),
        cue_loader
            .system_card_override
            .expect("fixture system card"),
        TEST_SYSTEM_CARD_SHA256,
    )
    .load_fresh_backend()?
    .pce()
    .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
    .expect("changed fixture disc");
    let _changed_catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        changed_disc,
        PceControllerMode::Multitap,
    );
    assert!(cue_loader.load_editor_engine(&project).is_err());

    let (raw_directory, raw_loader, _catalog) =
        fixture_with_catalog("pce-cd-tas-iso-multitap-raw-mutation")?;
    let project = raw_loader.create_project()?;
    let iso_path = raw_directory.path().join("disc.iso");
    let mut bytes = fs::read(&iso_path)?;
    bytes[0] ^= 1;
    fs::write(&iso_path, bytes)?;
    let changed_disc = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        iso_path,
        raw_loader
            .system_card_override
            .expect("fixture system card"),
        TEST_SYSTEM_CARD_SHA256,
    )
    .load_fresh_backend()?
    .pce()
    .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
    .expect("changed fixture disc");
    let _changed_catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        changed_disc,
        PceControllerMode::Multitap,
    );
    assert!(raw_loader.load_editor_engine(&project).is_err());
    Ok(())
}

#[test]
fn direct_iso_multitap_seek_and_automatic_replay_roundtrip_preserve_five_players() -> Result<()> {
    let (directory, loader, _catalog) = fixture_with_catalog("pce-cd-tas-iso-multitap-replay")?;
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
    let frame = &replay.peek_joypad_frames(0, 1)[0];
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
        directory.path().join("disc.iso"),
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
