use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use zeff_pce_core::hardware::{PceArcadeCardMode, PceControllerMode, PceMemoryBaseMode};

use super::*;
use crate::emu_backend::loader::tas::direct_pce_cd::{
    PCE_CD_UNPATCHED_DISC_PATCH_FORMAT, direct_pce_cd_chd_source_identity,
    direct_pce_cd_iso_source_identity, direct_pce_multitap_cd_chd_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_chd_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_iso_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_iso_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_ppf_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_ppf_memory_base_tas_sync_config_sha256,
};

#[derive(Clone, Copy, Debug)]
enum Profile {
    Chd(CardKind),
    Iso(CardKind),
    Ppf(CardKind),
}

impl Profile {
    fn name(self) -> String {
        format!("pce-cd-zrpl-direct-card-multitap-{self:?}").to_ascii_lowercase()
    }

    fn sync_config(self) -> TasDigest {
        match self {
            Self::Chd(CardKind::Arcade) => {
                direct_pce_multitap_cd_chd_arcade_tas_sync_config_sha256()
            }
            Self::Chd(CardKind::MemoryBase) => {
                direct_pce_multitap_cd_chd_memory_base_tas_sync_config_sha256()
            }
            Self::Iso(CardKind::Arcade) => {
                direct_pce_multitap_cd_iso_arcade_tas_sync_config_sha256()
            }
            Self::Iso(CardKind::MemoryBase) => {
                direct_pce_multitap_cd_iso_memory_base_tas_sync_config_sha256()
            }
            Self::Ppf(CardKind::Arcade) => {
                direct_pce_multitap_cd_ppf_arcade_tas_sync_config_sha256()
            }
            Self::Ppf(CardKind::MemoryBase) => {
                direct_pce_multitap_cd_ppf_memory_base_tas_sync_config_sha256()
            }
            _ => unreachable!("a card-plus-Multitap profile requires a card"),
        }
    }

    fn card(self) -> CardKind {
        match self {
            Self::Chd(card) | Self::Iso(card) | Self::Ppf(card) => card,
        }
    }
}

struct Fixture {
    directory: crate::test_support::TestDirectory,
    loader: DirectPceCdTasExecutionLoader,
    system_card: &'static [u8],
    source_path: PathBuf,
    source_disc_sha256: [u8; 32],
    ppf_stack: Option<crate::emu_backend::pce_cd::PceCdTasPpfStack>,
}

fn fixture_for(profile: Profile, tag: u8) -> Result<Fixture> {
    let directory = crate::test_support::test_directory(&profile.name())?;
    let system_card = system_card();
    let (source_path, ppf_stack) = match profile {
        Profile::Chd(_) => {
            let path = directory.path().join("disc.chd");
            crate::emu_backend::pce_cd_chd::write_synthetic_uncompressed_v5_chd(&path)?;
            let mut bytes = fs::read(&path)?;
            let offset = 4 * 2_448;
            bytes[offset..offset + 8].copy_from_slice(&[
                0xDA,
                0x7A,
                b'Z',
                b'R',
                b'P',
                b'L',
                tag,
                tag.rotate_left(1),
            ]);
            fs::write(&path, bytes)?;
            (path, None)
        }
        Profile::Iso(_) => {
            let path = directory.path().join("disc.iso");
            write_tagged_disc(&path, tag)?;
            write_cue(&directory.path().join("disc.cue"), "disc.iso")?;
            (path, None)
        }
        Profile::Ppf(_) => {
            let path = directory.path().join("disc.cue");
            write_tagged_disc(&directory.path().join("disc.bin"), tag)?;
            write_cue(&path, "disc.bin")?;
            let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
                &path,
                vec![
                    ("first.ppf".to_owned(), ppf1(0, &[tag ^ 0xA5])),
                    ("second.ppf".to_owned(), ppf1(1, &[tag ^ 0x5A])),
                ],
            )?;
            (path, Some(stack))
        }
    };
    let base = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path.clone(),
        system_card,
        SYSTEM_CARD_SHA256,
    );
    let source_disc_sha256 = base
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("direct fixture disc");
    let loader = if let Some(stack) = ppf_stack.clone() {
        DirectPceCdTasExecutionLoader::new_multitap_with_system_card_and_ppf_stack(
            source_path.clone(),
            system_card,
            SYSTEM_CARD_SHA256,
            stack,
        )
    } else {
        DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
            source_path.clone(),
            system_card,
            SYSTEM_CARD_SHA256,
        )
    };
    Ok(Fixture {
        directory,
        loader,
        system_card,
        source_path,
        source_disc_sha256,
        ppf_stack,
    })
}

fn write_tagged_disc(path: &std::path::Path, tag: u8) -> Result<()> {
    let mut bytes = deterministic_disc_bytes(tag);
    bytes[..8].copy_from_slice(&[0xDA, 0x7A, b'Z', b'R', b'P', b'L', tag, tag.rotate_left(1)]);
    fs::write(path, bytes)?;
    Ok(())
}

fn five_player_input(tag: u8) -> TasInputFrame {
    let mut input = TasInputFrame::default();
    for (index, player) in input.players.iter_mut().enumerate() {
        player.buttons = match (tag.wrapping_add(index as u8)) & 3 {
            0 => 0x01,
            1 => 0x02,
            _ => 0x03,
        };
        player.dpad = 1 << ((tag as usize + index) & 3);
    }
    input
}

fn with_catalogs<T>(
    profile: Profile,
    fixture: &Fixture,
    action: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let _controller_catalog =
        crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            fixture.source_disc_sha256,
            PceControllerMode::Multitap,
        );
    match profile.card() {
        CardKind::Arcade => {
            let _card_catalog =
                crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(
                    fixture.source_disc_sha256,
                );
            action()
        }
        CardKind::MemoryBase => {
            let _card_catalog =
                crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
                    fixture.source_disc_sha256,
                );
            action()
        }
        CardKind::None => unreachable!("a card-plus-Multitap profile requires a card"),
    }
}

#[test]
fn direct_card_multitap_verified_replays_auto_select_reopen_and_seek_all_six_profiles() -> Result<()>
{
    let profiles = [
        Profile::Chd(CardKind::Arcade),
        Profile::Chd(CardKind::MemoryBase),
        Profile::Iso(CardKind::Arcade),
        Profile::Iso(CardKind::MemoryBase),
        Profile::Ppf(CardKind::Arcade),
        Profile::Ppf(CardKind::MemoryBase),
    ];
    for (index, profile) in profiles.into_iter().enumerate() {
        let fixture = fixture_for(profile, 0xE0 + index as u8)?;
        with_catalogs(profile, &fixture, || {
            exercise_profile(profile, &fixture, index)
        })?;
    }
    Ok(())
}

fn exercise_profile(profile: Profile, fixture: &Fixture, index: usize) -> Result<()> {
    let backend = fixture.loader.load_fresh_backend()?;
    let pce = backend.pce().expect("direct card fixture backend");
    assert_eq!(
        pce.arcade_card_mode(),
        if matches!(profile.card(), CardKind::Arcade) {
            PceArcadeCardMode::Enabled
        } else {
            PceArcadeCardMode::Disabled
        }
    );
    assert_eq!(
        pce.memory_base_mode(),
        if matches!(profile.card(), CardKind::MemoryBase) {
            PceMemoryBaseMode::Enabled
        } else {
            PceMemoryBaseMode::Disabled
        }
    );

    let frame = 60 + index as u64;
    let input = five_player_input(0xC1 + index as u8);
    let project_path = fixture.directory.path().join("source.ztas");
    let replay_path = fixture.directory.path().join("verified.zrpl");
    let imported_path = fixture.directory.path().join("imported.ztas");
    let mut project = fixture.loader.create_project()?;
    assert_eq!(project.identity().sync_config_sha256, profile.sync_config());
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
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::Absent
    );
    assert_media_domains(profile, fixture, &project)?;
    let initial_frame_count = project.branch("main").expect("main branch").frame_count();
    project.edit_transaction(|edit| {
        edit.insert_frames("main", initial_frame_count, frame + 1 - initial_frame_count)?;
        edit.set_input_range("main", frame, 1, input)
    })?;

    let autosaves =
        TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(fixture.directory.path().join("source-seek-cache"))?;
    let mut editor = TasEditorSession::new(project.clone(), &project_path, autosaves, cache)?;
    PrivateTasExecutionLoader::DirectPceCd(fixture.loader.clone())
        .verify_and_export_editor_session(&mut editor, &replay_path)?;

    let replay = zeff_emu_common::replay::ReplayPlayer::load(&replay_path)?;
    let replay_frame = &replay.peek_joypad_frames(frame as usize, 1)[0];
    assert_eq!(
        [
            (replay_frame.buttons, replay_frame.dpad),
            (replay_frame.buttons_p2, replay_frame.dpad_p2),
            (replay_frame.buttons_p3, replay_frame.dpad_p3),
            (replay_frame.buttons_p4, replay_frame.dpad_p4),
            (replay_frame.buttons_p5, replay_frame.dpad_p5),
        ],
        input.players.map(|player| (player.buttons, player.dpad))
    );

    let _system_card =
        super::super::register_test_pce_cd_system_card(SYSTEM_CARD_SHA256, fixture.system_card);
    let _ppf_stack = fixture.ppf_stack.clone().map(|stack| {
        super::super::register_test_pce_cd_ppf_stack(fixture.source_path.clone(), stack)
    });
    let start_state = TasProject::read_zrpl_start_state(&replay_path)?;
    let selected = super::super::select_private_tas_execution_loader_for_replay(
        fixture.source_path.clone(),
        None,
        crate::emu_backend::ActiveSystem::Pce,
        Vec::new(),
        &start_state,
    )?;
    let imported = selected.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(imported.identity(), project.identity());
    assert_eq!(
        imported
            .branch("main")
            .expect("main branch")
            .input_at(frame),
        input
    );
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&imported)?,
        crate::emu_thread::TasExecutionProfile::DirectPceMultitapCd
    );

    let reopened = super::super::select_private_tas_execution_loader_for_project(
        fixture.source_path.clone(),
        crate::emu_backend::ActiveSystem::Pce,
        Vec::new(),
        &imported,
    )?;
    let autosaves =
        TasAutosaveStore::beside_manual_save(&imported_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(fixture.directory.path().join("imported-seek-cache"))?;
    let mut imported_editor = TasEditorSession::open(&imported_path, autosaves, cache)?;
    let mut engine = reopened.load_editor_engine(imported_editor.project())?;
    assert!(
        engine
            .seek(&mut imported_editor, frame + 1)?
            .reached_target()
    );
    Ok(())
}

fn assert_media_domains(profile: Profile, fixture: &Fixture, project: &TasProject) -> Result<()> {
    let identity = project.identity();
    match profile {
        Profile::Chd(_) => {
            let bytes = fs::read(&fixture.source_path)?;
            assert_eq!(
                identity.source_media_sha256,
                direct_pce_cd_chd_source_identity(TasDigest::from_bytes(&bytes).0, bytes.len())
            );
            assert_ne!(
                identity.source_media_sha256,
                identity.effective_media_sha256
            );
            assert!(identity.patches.is_empty());
        }
        Profile::Iso(_) => {
            let bytes = fs::read(&fixture.source_path)?;
            assert_eq!(
                identity.source_media_sha256,
                direct_pce_cd_iso_source_identity(TasDigest::from_bytes(&bytes).0, bytes.len())
            );
            assert_ne!(
                identity.source_media_sha256,
                identity.effective_media_sha256
            );
            assert!(identity.patches.is_empty());
        }
        Profile::Ppf(_) => {
            assert_eq!(identity.patches.len(), 1);
            assert_eq!(
                identity.patches[0].format,
                PCE_CD_UNPATCHED_DISC_PATCH_FORMAT
            );
            assert_eq!(
                identity.patches[0].sha256,
                TasDigest(fixture.source_disc_sha256)
            );
            assert_ne!(identity.source_media_sha256, identity.patches[0].sha256);
            assert_ne!(
                identity.source_media_sha256,
                identity.effective_media_sha256
            );
            assert_ne!(identity.patches[0].sha256, identity.effective_media_sha256);
        }
    }
    Ok(())
}

#[test]
fn direct_card_multitap_replay_selection_requires_independent_catalogs() -> Result<()> {
    let fixture = fixture_for(Profile::Iso(CardKind::MemoryBase), 0xF8)?;
    let controller_catalog =
        crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            fixture.source_disc_sha256,
            PceControllerMode::Multitap,
        );
    let memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
            fixture.source_disc_sha256,
        );
    let project_path = fixture.directory.path().join("source.ztas");
    let replay_path = fixture.directory.path().join("verified.zrpl");
    let project = fixture.loader.create_project()?;
    let autosaves =
        TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(fixture.directory.path().join("source-seek-cache"))?;
    let mut editor = TasEditorSession::new(project, &project_path, autosaves, cache)?;
    PrivateTasExecutionLoader::DirectPceCd(fixture.loader.clone())
        .verify_and_export_editor_session(&mut editor, &replay_path)?;
    let _system_card =
        super::super::register_test_pce_cd_system_card(SYSTEM_CARD_SHA256, fixture.system_card);
    let start_state = TasProject::read_zrpl_start_state(&replay_path)?;

    drop(controller_catalog);
    assert!(
        super::super::select_private_tas_execution_loader_for_replay(
            fixture.source_path.clone(),
            None,
            crate::emu_backend::ActiveSystem::Pce,
            Vec::new(),
            &start_state,
        )
        .and_then(|plan| plan.load_session(&start_state))
        .is_err()
    );
    let _controller_catalog =
        crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            fixture.source_disc_sha256,
            PceControllerMode::Multitap,
        );
    drop(memory_base_catalog);
    assert!(
        super::super::select_private_tas_execution_loader_for_replay(
            fixture.source_path.clone(),
            None,
            crate::emu_backend::ActiveSystem::Pce,
            Vec::new(),
            &start_state,
        )
        .and_then(|plan| plan.load_session(&start_state))
        .is_err()
    );
    Ok(())
}

#[test]
fn direct_ppf_card_multitap_replay_rebinds_commuting_order_and_rejects_effective_or_base_changes()
-> Result<()> {
    let tag = 0xF0;
    let directory = crate::test_support::test_directory("pce-cd-zrpl-direct-ppf-reject")?;
    let source_path = directory.path().join("disc.cue");
    write_tagged_disc(&directory.path().join("disc.bin"), tag)?;
    write_cue(&source_path, "disc.bin")?;
    let system_card = system_card();
    let base_loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path.clone(),
        system_card,
        SYSTEM_CARD_SHA256,
    );
    let source_disc_sha256 = base_loader
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("PPF base disc");
    let first = ppf1(0, &[tag ^ 0xA5]);
    let second = ppf1(1, &[tag ^ 0x5A]);
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &source_path,
        vec![
            ("first.ppf".to_owned(), first.clone()),
            ("second.ppf".to_owned(), second.clone()),
        ],
    )?;
    let fixture = Fixture {
        directory,
        loader: DirectPceCdTasExecutionLoader::new_multitap_with_system_card_and_ppf_stack(
            source_path.clone(),
            system_card,
            SYSTEM_CARD_SHA256,
            stack.clone(),
        ),
        system_card,
        source_path,
        source_disc_sha256,
        ppf_stack: Some(stack),
    };
    let _controller_catalog =
        crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            fixture.source_disc_sha256,
            PceControllerMode::Multitap,
        );
    let _card_catalog = crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(
        fixture.source_disc_sha256,
    );
    let project_path = fixture.directory.path().join("source.ztas");
    let replay_path = fixture.directory.path().join("verified.zrpl");
    let project = fixture.loader.create_project()?;
    let original_project = project.clone();
    let autosaves =
        TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(fixture.directory.path().join("source-seek-cache"))?;
    let mut editor = TasEditorSession::new(project, &project_path, autosaves, cache)?;
    PrivateTasExecutionLoader::DirectPceCd(fixture.loader.clone())
        .verify_and_export_editor_session(&mut editor, &replay_path)?;
    let start_state = TasProject::read_zrpl_start_state(&replay_path)?;
    let _system_card =
        super::super::register_test_pce_cd_system_card(SYSTEM_CARD_SHA256, fixture.system_card);

    let wrong_stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &fixture.source_path,
        vec![
            ("second.ppf".to_owned(), second.clone()),
            ("first.ppf".to_owned(), first.clone()),
        ],
    )?;
    let reordered_loader =
        DirectPceCdTasExecutionLoader::new_multitap_with_system_card_and_ppf_stack(
            fixture.source_path.clone(),
            fixture.system_card,
            SYSTEM_CARD_SHA256,
            wrong_stack.clone(),
        );
    let original_effective = fixture
        .loader
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("original PPF disc");
    let reordered_effective = reordered_loader
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("reordered PPF disc");
    assert_eq!(reordered_effective, original_effective);
    assert!(
        reordered_loader
            .load_editor_engine(&original_project)
            .is_err()
    );
    let reordered_session_identity = reordered_loader
        .load_session(&start_state)?
        .identity()
        .clone();
    let reordered_project = reordered_loader.create_project()?;
    assert_eq!(reordered_project.identity(), &reordered_session_identity);
    let _wrong_stack =
        super::super::register_test_pce_cd_ppf_stack(fixture.source_path.clone(), wrong_stack);
    let reordered = super::super::select_private_tas_execution_loader_for_replay(
        fixture.source_path.clone(),
        None,
        crate::emu_backend::ActiveSystem::Pce,
        Vec::new(),
        &start_state,
    )?;
    let reordered_path = fixture.directory.path().join("reordered.ztas");
    let imported = reordered.import_replay_file(&replay_path, &reordered_path, false)?;
    assert_eq!(imported.identity(), &reordered_session_identity);
    assert_eq!(
        imported.source_replay_sha256(),
        Some(TasDigest::from_bytes(&fs::read(&replay_path)?))
    );
    assert!(imported.verification_is_current("main")?);
    assert_ne!(
        imported.identity().source_media_sha256,
        original_project.identity().source_media_sha256
    );
    assert_eq!(
        imported.identity().effective_media_sha256,
        original_project.identity().effective_media_sha256
    );
    assert_eq!(imported.start_state(), original_project.start_state());
    assert_eq!(
        imported.identity().devices,
        original_project.identity().devices
    );
    assert_eq!(
        imported.identity().sync_config_sha256,
        original_project.identity().sync_config_sha256
    );
    let autosaves =
        TasAutosaveStore::beside_manual_save(&reordered_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(fixture.directory.path().join("reordered-seek-cache"))?;
    let mut imported_editor = TasEditorSession::open(&reordered_path, autosaves, cache)?;
    let mut reordered_engine = reordered_loader.load_editor_engine(imported_editor.project())?;
    assert!(
        reordered_engine
            .seek(&mut imported_editor, 1)?
            .reached_target()
    );
    assert!(fixture.loader.load_editor_engine(&imported).is_err());
    drop(_wrong_stack);

    let changed_stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &fixture.source_path,
        vec![
            ("first.ppf".to_owned(), ppf1(0, &[tag ^ 0x3C])),
            ("second.ppf".to_owned(), second),
        ],
    )?;
    let changed_loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_and_ppf_stack(
        fixture.source_path.clone(),
        fixture.system_card,
        SYSTEM_CARD_SHA256,
        changed_stack.clone(),
    );
    let changed_effective = changed_loader
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("changed PPF disc");
    assert_ne!(changed_effective, original_effective);
    let changed_path = fixture.directory.path().join("changed.ztas");
    let _changed_stack =
        super::super::register_test_pce_cd_ppf_stack(fixture.source_path.clone(), changed_stack);
    assert!(
        super::super::select_private_tas_execution_loader_for_replay(
            fixture.source_path.clone(),
            None,
            crate::emu_backend::ActiveSystem::Pce,
            Vec::new(),
            &start_state,
        )
        .and_then(|plan| plan.import_replay_file(&replay_path, &changed_path, false))
        .is_err()
    );
    assert!(!changed_path.exists());
    drop(_changed_stack);

    let _good_stack = super::super::register_test_pce_cd_ppf_stack(
        fixture.source_path.clone(),
        fixture.ppf_stack.clone().expect("fixture PPF stack"),
    );
    let disc_path = fixture.directory.path().join("disc.bin");
    let mut bytes = fs::read(&disc_path)?;
    bytes[7] ^= 1;
    fs::write(disc_path, bytes)?;
    assert!(
        super::super::select_private_tas_execution_loader_for_replay(
            fixture.source_path.clone(),
            None,
            crate::emu_backend::ActiveSystem::Pce,
            Vec::new(),
            &start_state,
        )
        .and_then(|plan| plan.load_session(&start_state))
        .is_err()
    );
    Ok(())
}
