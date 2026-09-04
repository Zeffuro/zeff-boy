use super::multicue::{ArchiveKind, write_multicue_archive_with_second_fill};
use super::*;

impl ArchiveKind {
    fn multitap_sync(self, selected: bool) -> TasDigest {
        match (self, selected) {
            (Self::SevenZip, false) => super::super::super::direct_pce_cd::direct_pce_multitap_cd_archive_tas_sync_config_sha256(),
            (Self::SevenZip, true) => super::super::super::direct_pce_cd::direct_pce_multitap_cd_selected_archive_tas_sync_config_sha256(),
            (Self::Rar, false) => super::super::super::direct_pce_cd::direct_pce_multitap_cd_rar_tas_sync_config_sha256(),
            (Self::Rar, true) => super::super::super::direct_pce_cd::direct_pce_multitap_cd_selected_rar_tas_sync_config_sha256(),
            (Self::Zip, false) => super::super::super::direct_pce_cd::direct_pce_multitap_cd_zip_tas_sync_config_sha256(),
            (Self::Zip, true) => super::super::super::direct_pce_cd::direct_pce_multitap_cd_selected_zip_tas_sync_config_sha256(),
        }
    }

    fn next(self) -> Self {
        match self {
            Self::SevenZip => Self::Rar,
            Self::Rar => Self::Zip,
            Self::Zip => Self::SevenZip,
        }
    }
}

#[test]
fn archive_multitap_six_routes_create_reopen_seek_and_bind_format_and_member() -> Result<()> {
    for (index, kind) in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip]
        .into_iter()
        .enumerate()
    {
        exercise_route(kind, false, 0x81 + index as u8)?;
        exercise_route(kind, true, 0x91 + index as u8)?;
    }
    Ok(())
}

fn exercise_route(kind: ArchiveKind, selected: bool, fill: u8) -> Result<()> {
    let directory = crate::test_support::test_directory(&format!(
        "pce-cd-tas-archive-multitap-{kind:?}-{selected}"
    ))?;
    let path = directory.path().join(format!("disc.{}", kind.extension()));
    if selected {
        write_multicue_archive_with_second_fill(&path, kind, fill)?;
    } else {
        write_unique_archive(&path, kind, fill)?;
    }
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let rom_path = selected.then(|| path.join("second").join("disc.cue"));
    let base = configured_loader(
        DirectPceCdTasExecutionLoader::new_with_rom_path(
            path.clone(),
            rom_path.clone(),
            Vec::new(),
        )?,
        system_card,
    );
    let disc_sha256 = base
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture disc");
    assert!(!super::super::super::direct_pce_cd::direct_pce_cd_arcade_eligible(false, disc_sha256));
    assert!(
        !super::super::super::direct_pce_cd::direct_pce_cd_memory_base_eligible(
            false,
            false,
            false,
            disc_sha256
        )
    );
    let catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        disc_sha256,
        PceControllerMode::Multitap,
    );
    let loader = configured_loader(
        DirectPceCdTasExecutionLoader::new_multitap_with_rom_path(
            path.clone(),
            rom_path,
            Vec::new(),
        )?,
        system_card,
    );
    let mut project = loader.create_project()?;
    assert_eq!(
        project.identity().sync_config_sha256,
        kind.multitap_sync(selected)
    );
    assert_eq!(project.identity().devices.len(), 5);
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    let loaded = loader.load_fresh_backend()?;
    let provenance = loaded
        .pce()
        .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
        .expect("archive provenance");
    let member_sha256 = provenance
        .load
        .archive_cue_member_path_sha256
        .or(provenance.load.rar_cue_member_path_sha256)
        .or(provenance.load.zip_cue_member_path_sha256)
        .expect("archive member identity");
    let profile = super::super::super::direct_pce_cd::PceCdTasProfile::from_sync(
        project.identity().sync_config_sha256,
    )
    .expect("archive Multitap profile");
    assert_eq!(
        project.identity().source_media_sha256,
        profile
            .archive_source_identity(
                provenance.load.raw_source_media_sha256,
                provenance.load.raw_source_media_len,
                member_sha256,
            )
            .expect("archive source identity")
    );

    let input = five_player_input();
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let manual_path = directory.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project.clone(), manual_path, autosaves, cache)?;
    let mut engine = loader.load_editor_engine(&project)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    let reached = engine.backend().encode_state_bytes()?;
    assert!(engine.seek(&mut editor, 0)?.reached_target());
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    assert_eq!(engine.backend().encode_state_bytes()?, reached);

    let mut reopened =
        DirectPceCdTasExecutionLoader::new_for_project(path.clone(), Vec::new(), &project)?;
    reopened.system_card_override = Some(system_card);
    reopened.system_card_sha256_override = Some(TEST_SYSTEM_CARD_SHA256);
    reopened.load_editor_engine(&project)?;

    let other_path = directory
        .path()
        .join(format!("other.{}", kind.next().extension()));
    write_unique_archive(&other_path, kind.next(), if selected { 0x22 } else { fill })?;
    let other = configured_loader(
        DirectPceCdTasExecutionLoader::new_multitap(other_path, Vec::new()),
        system_card,
    );
    assert!(other.load_editor_engine(&project).is_err());

    let arcade =
        crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256);
    assert!(loader.load_fresh_backend().is_err());
    drop(arcade);

    if selected {
        let wrong = configured_loader(
            DirectPceCdTasExecutionLoader::new_multitap_with_rom_path(
                path,
                Some(
                    directory
                        .path()
                        .join(format!("disc.{}", kind.extension()))
                        .join("first")
                        .join("disc.cue"),
                ),
                Vec::new(),
            )?,
            system_card,
        );
        assert!(wrong.load_editor_engine(&project).is_err());
    }
    if selected && matches!(kind, ArchiveKind::Zip) {
        let mut identity = project.identity().clone();
        identity.devices.pop();
        assert!(
            loader
                .load_editor_engine(&project_with_identity(&project, identity)?)
                .is_err()
        );

        let mut identity = project.identity().clone();
        identity.patches.push(crate::tas_project::TasPatchIdentity {
            format: "ppf1".to_owned(),
            sha256: TasDigest([0xA5; 32]),
        });
        assert!(
            loader
                .load_editor_engine(&project_with_identity(&project, identity)?)
                .is_err()
        );

        let mut identity = project.identity().clone();
        identity.source_media_sha256 = TasDigest([0xA6; 32]);
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

        let wrong_state = base.load_fresh_backend()?.encode_state_bytes()?;
        let mut identity = project.identity().clone();
        identity.start_state_sha256 = TasDigest::from_bytes(&wrong_state);
        let wrong_topology = TasProject::new(
            "wrong-topology",
            identity,
            wrong_state,
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
        assert!(loader.load_editor_engine(&wrong_topology).is_err());

        let mut invalid_branch = project.clone();
        let mut invalid_input = TasInputFrame::default();
        invalid_input.players[4].buttons = 0x80;
        invalid_branch
            .edit_transaction(|edit| edit.set_input_range("main", 0, 1, invalid_input))?;
        assert!(loader.load_editor_engine(&invalid_branch).is_err());

        let mut archive = fs::OpenOptions::new()
            .append(true)
            .open(directory.path().join("disc.zip"))?;
        archive.write_all(&[0])?;
        assert!(loader.load_fresh_backend().is_ok());
        assert!(loader.load_editor_engine(&project).is_err());
    }
    drop(catalog);
    assert!(loader.load_fresh_backend().is_err());
    Ok(())
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

fn configured_loader(
    mut loader: DirectPceCdTasExecutionLoader,
    system_card: &'static [u8],
) -> DirectPceCdTasExecutionLoader {
    loader.system_card_override = Some(system_card);
    loader.system_card_sha256_override = Some(TEST_SYSTEM_CARD_SHA256);
    loader
}

fn five_player_input() -> TasInputFrame {
    let mut input = TasInputFrame::default();
    for (index, player) in input.players.iter_mut().enumerate() {
        player.buttons = 1 << index.min(3);
        player.dpad = 1 << (3 - index.min(3));
    }
    input
}

fn write_unique_archive(path: &Path, kind: ArchiveKind, fill: u8) -> Result<()> {
    let mut disc = vec![fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
    disc[0..4].copy_from_slice(&[0x4D, 0x54, fill, fill.rotate_left(1)]);
    match kind {
        ArchiveKind::SevenZip => write_archive_fixture_bytes(path, "set", disc, false),
        ArchiveKind::Rar => write_rar_fixture_bytes(path, "set", disc),
        ArchiveKind::Zip => write_zip_fixture_bytes(path, "set", disc),
    }
}
