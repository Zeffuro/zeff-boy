use anyhow::{Context, Result};
use zeff_pce_core::hardware::{PceArcadeCardMode, PceControllerMode, PceMemoryBaseMode};

use super::*;
use crate::emu_backend::loader::tas::direct_pce_cd::{
    direct_pce_cd_archive_ppf_arcade_tas_sync_config_sha256,
    direct_pce_cd_archive_ppf_memory_base_tas_sync_config_sha256,
    direct_pce_cd_rar_ppf_arcade_tas_sync_config_sha256,
    direct_pce_cd_rar_ppf_memory_base_tas_sync_config_sha256,
    direct_pce_cd_selected_archive_ppf_arcade_tas_sync_config_sha256,
    direct_pce_cd_selected_archive_ppf_memory_base_tas_sync_config_sha256,
    direct_pce_cd_selected_rar_ppf_arcade_tas_sync_config_sha256,
    direct_pce_cd_selected_rar_ppf_memory_base_tas_sync_config_sha256,
    direct_pce_cd_selected_zip_ppf_arcade_tas_sync_config_sha256,
    direct_pce_cd_selected_zip_ppf_memory_base_tas_sync_config_sha256,
    direct_pce_cd_zip_ppf_arcade_tas_sync_config_sha256,
    direct_pce_cd_zip_ppf_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_archive_ppf_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_archive_ppf_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_archive_ppf_tas_sync_config_sha256,
    direct_pce_multitap_cd_rar_ppf_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_rar_ppf_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_rar_ppf_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_archive_ppf_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_archive_ppf_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_archive_ppf_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_rar_ppf_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_rar_ppf_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_rar_ppf_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_zip_ppf_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_zip_ppf_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_zip_ppf_tas_sync_config_sha256,
    direct_pce_multitap_cd_zip_ppf_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_zip_ppf_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_zip_ppf_tas_sync_config_sha256,
};

#[derive(Clone, Copy, Debug)]
enum Profile {
    TwoButton(CardKind),
    Multitap(CardKind),
}

impl Profile {
    fn card(self) -> CardKind {
        match self {
            Self::TwoButton(card) | Self::Multitap(card) => card,
        }
    }

    fn multitap(self) -> bool {
        matches!(self, Self::Multitap(_))
    }

    fn name(self) -> &'static str {
        match self {
            Self::TwoButton(CardKind::Arcade) => "arcade-two-button",
            Self::TwoButton(CardKind::MemoryBase) => "memory-base-two-button",
            Self::Multitap(CardKind::None) => "multitap",
            Self::Multitap(CardKind::Arcade) => "arcade-multitap",
            Self::Multitap(CardKind::MemoryBase) => "memory-base-multitap",
            _ => unreachable!("archive PPF combinations exclude plain two-button"),
        }
    }

    fn sync_config(self, kind: ArchivePpfKind, selected: bool) -> TasDigest {
        match (self, kind, selected) {
            (Self::TwoButton(CardKind::Arcade), ArchivePpfKind::SevenZip, false) => {
                direct_pce_cd_archive_ppf_arcade_tas_sync_config_sha256()
            }
            (Self::TwoButton(CardKind::Arcade), ArchivePpfKind::SevenZip, true) => {
                direct_pce_cd_selected_archive_ppf_arcade_tas_sync_config_sha256()
            }
            (Self::TwoButton(CardKind::Arcade), ArchivePpfKind::Rar, false) => {
                direct_pce_cd_rar_ppf_arcade_tas_sync_config_sha256()
            }
            (Self::TwoButton(CardKind::Arcade), ArchivePpfKind::Rar, true) => {
                direct_pce_cd_selected_rar_ppf_arcade_tas_sync_config_sha256()
            }
            (Self::TwoButton(CardKind::Arcade), ArchivePpfKind::Zip, false) => {
                direct_pce_cd_zip_ppf_arcade_tas_sync_config_sha256()
            }
            (Self::TwoButton(CardKind::Arcade), ArchivePpfKind::Zip, true) => {
                direct_pce_cd_selected_zip_ppf_arcade_tas_sync_config_sha256()
            }
            (Self::TwoButton(CardKind::MemoryBase), ArchivePpfKind::SevenZip, false) => {
                direct_pce_cd_archive_ppf_memory_base_tas_sync_config_sha256()
            }
            (Self::TwoButton(CardKind::MemoryBase), ArchivePpfKind::SevenZip, true) => {
                direct_pce_cd_selected_archive_ppf_memory_base_tas_sync_config_sha256()
            }
            (Self::TwoButton(CardKind::MemoryBase), ArchivePpfKind::Rar, false) => {
                direct_pce_cd_rar_ppf_memory_base_tas_sync_config_sha256()
            }
            (Self::TwoButton(CardKind::MemoryBase), ArchivePpfKind::Rar, true) => {
                direct_pce_cd_selected_rar_ppf_memory_base_tas_sync_config_sha256()
            }
            (Self::TwoButton(CardKind::MemoryBase), ArchivePpfKind::Zip, false) => {
                direct_pce_cd_zip_ppf_memory_base_tas_sync_config_sha256()
            }
            (Self::TwoButton(CardKind::MemoryBase), ArchivePpfKind::Zip, true) => {
                direct_pce_cd_selected_zip_ppf_memory_base_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::None), ArchivePpfKind::SevenZip, false) => {
                direct_pce_multitap_cd_archive_ppf_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::None), ArchivePpfKind::SevenZip, true) => {
                direct_pce_multitap_cd_selected_archive_ppf_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::None), ArchivePpfKind::Rar, false) => {
                direct_pce_multitap_cd_rar_ppf_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::None), ArchivePpfKind::Rar, true) => {
                direct_pce_multitap_cd_selected_rar_ppf_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::None), ArchivePpfKind::Zip, false) => {
                direct_pce_multitap_cd_zip_ppf_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::None), ArchivePpfKind::Zip, true) => {
                direct_pce_multitap_cd_selected_zip_ppf_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::Arcade), ArchivePpfKind::SevenZip, false) => {
                direct_pce_multitap_cd_archive_ppf_arcade_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::Arcade), ArchivePpfKind::SevenZip, true) => {
                direct_pce_multitap_cd_selected_archive_ppf_arcade_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::Arcade), ArchivePpfKind::Rar, false) => {
                direct_pce_multitap_cd_rar_ppf_arcade_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::Arcade), ArchivePpfKind::Rar, true) => {
                direct_pce_multitap_cd_selected_rar_ppf_arcade_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::Arcade), ArchivePpfKind::Zip, false) => {
                direct_pce_multitap_cd_zip_ppf_arcade_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::Arcade), ArchivePpfKind::Zip, true) => {
                direct_pce_multitap_cd_selected_zip_ppf_arcade_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::MemoryBase), ArchivePpfKind::SevenZip, false) => {
                direct_pce_multitap_cd_archive_ppf_memory_base_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::MemoryBase), ArchivePpfKind::SevenZip, true) => {
                direct_pce_multitap_cd_selected_archive_ppf_memory_base_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::MemoryBase), ArchivePpfKind::Rar, false) => {
                direct_pce_multitap_cd_rar_ppf_memory_base_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::MemoryBase), ArchivePpfKind::Rar, true) => {
                direct_pce_multitap_cd_selected_rar_ppf_memory_base_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::MemoryBase), ArchivePpfKind::Zip, false) => {
                direct_pce_multitap_cd_zip_ppf_memory_base_tas_sync_config_sha256()
            }
            (Self::Multitap(CardKind::MemoryBase), ArchivePpfKind::Zip, true) => {
                direct_pce_multitap_cd_selected_zip_ppf_memory_base_tas_sync_config_sha256()
            }
            _ => unreachable!("archive PPF combination"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ArchivePpfKind {
    SevenZip,
    Rar,
    Zip,
}

impl ArchivePpfKind {
    fn extension(self) -> &'static str {
        match self {
            Self::SevenZip => "7z",
            Self::Rar => "rar",
            Self::Zip => "zip",
        }
    }
}

struct Fixture {
    directory: crate::test_support::TestDirectory,
    archive: PathBuf,
    rom_path: Option<PathBuf>,
    loader: DirectPceCdTasExecutionLoader,
    system_card: &'static [u8],
    source_disc_sha256: [u8; 32],
}

fn fixture(profile: Profile, kind: ArchivePpfKind, selected: bool, tag: u8) -> Result<Fixture> {
    let directory = crate::test_support::test_directory(&format!(
        "pce-cd-zrpl-archive-ppf-combinations-{}-{}-{selected}-{tag:02x}",
        kind.extension(),
        profile.name()
    ))?;
    let archive = directory.path().join(format!("disc.{}", kind.extension()));
    write_archive_ppf(&archive, kind, selected, tag, false, false)?;
    let rom_path = selected.then(|| archive.join("second").join("disc.cue"));
    let plain_archive = directory.path().join(format!("plain.{}", kind.extension()));
    let mut plain_entries = Vec::new();
    if selected {
        plain_entries.extend(cue_entries("first", tag.wrapping_add(1)));
    }
    plain_entries.extend(cue_entries(if selected { "second" } else { "set" }, tag));
    write_archive_entries(&plain_archive, kind, plain_entries)?;
    let system_card = system_card();
    let plain_rom_path = selected.then(|| plain_archive.join("second").join("disc.cue"));
    let base = match plain_rom_path {
        Some(rom_path) => {
            DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
                plain_archive,
                rom_path,
                system_card,
                SYSTEM_CARD_SHA256,
            )?
        }
        None => DirectPceCdTasExecutionLoader::new_with_system_card_override(
            plain_archive,
            system_card,
            SYSTEM_CARD_SHA256,
        ),
    };
    let source_disc_sha256 = base
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("archive PPF fixture base disc");
    let loader = new_loader(profile, archive.clone(), rom_path.clone(), system_card)?;
    Ok(Fixture {
        directory,
        archive,
        rom_path,
        loader,
        system_card,
        source_disc_sha256,
    })
}

fn new_loader(
    profile: Profile,
    archive: PathBuf,
    rom_path: Option<PathBuf>,
    system_card: &'static [u8],
) -> Result<DirectPceCdTasExecutionLoader> {
    match (profile.multitap(), rom_path) {
        (false, Some(rom_path)) => {
            DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
                archive,
                rom_path,
                system_card,
                SYSTEM_CARD_SHA256,
            )
        }
        (true, Some(rom_path)) => {
            DirectPceCdTasExecutionLoader::new_multitap_with_rom_path_and_system_card_override(
                archive,
                rom_path,
                system_card,
                SYSTEM_CARD_SHA256,
            )
        }
        (false, None) => Ok(
            DirectPceCdTasExecutionLoader::new_with_system_card_override(
                archive,
                system_card,
                SYSTEM_CARD_SHA256,
            ),
        ),
        (true, None) => Ok(
            DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
                archive,
                system_card,
                SYSTEM_CARD_SHA256,
            ),
        ),
    }
}

fn with_catalogs<T>(
    profile: Profile,
    fixture: &Fixture,
    action: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let _controller = profile.multitap().then(|| {
        crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            fixture.source_disc_sha256,
            PceControllerMode::Multitap,
        )
    });
    match profile.card() {
        CardKind::None => action(),
        CardKind::Arcade => {
            let _card = crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(
                fixture.source_disc_sha256,
            );
            action()
        }
        CardKind::MemoryBase => {
            let _card = crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
                fixture.source_disc_sha256,
            );
            action()
        }
    }
}

fn tagged_input(tag: u8, multitap: bool) -> TasInputFrame {
    let mut input = TasInputFrame::default();
    for (index, player) in input.players.iter_mut().enumerate() {
        player.buttons = 1 << ((tag as usize + index) & 1);
        player.dpad = 1 << ((tag as usize + index) & 3);
    }
    if !multitap {
        input.players[1..].fill(TasControllerInput::default());
    }
    input
}

#[test]
fn archive_ppf_combinations_verify_export_import_reopen_and_seek() -> Result<()> {
    let profiles = [
        Profile::Multitap(CardKind::None),
        Profile::TwoButton(CardKind::Arcade),
        Profile::TwoButton(CardKind::MemoryBase),
        Profile::Multitap(CardKind::Arcade),
        Profile::Multitap(CardKind::MemoryBase),
    ];
    for (route, (kind, selected)) in [
        (ArchivePpfKind::SevenZip, false),
        (ArchivePpfKind::SevenZip, true),
        (ArchivePpfKind::Rar, false),
        (ArchivePpfKind::Rar, true),
        (ArchivePpfKind::Zip, false),
        (ArchivePpfKind::Zip, true),
    ]
    .into_iter()
    .enumerate()
    {
        for (variant, profile) in profiles.into_iter().enumerate() {
            let fixture = fixture(profile, kind, selected, 0x81 + (route * 5 + variant) as u8)?;
            with_catalogs(profile, &fixture, || {
                exercise(profile, kind, selected, &fixture, route * 5 + variant)
            })
            .with_context(|| format!("{kind:?} selected={selected} {}", profile.name()))?;
        }
    }
    Ok(())
}

fn exercise(
    profile: Profile,
    kind: ArchivePpfKind,
    selected: bool,
    fixture: &Fixture,
    ordinal: usize,
) -> Result<()> {
    let backend = fixture.loader.load_fresh_backend()?;
    let pce = backend.pce().expect("archive PPF PC Engine backend");
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

    let frame = 64 + ordinal as u64;
    let input = tagged_input(0xC0 + ordinal as u8, profile.multitap());
    let project_path = fixture.directory.path().join("source.ztas");
    let replay_path = fixture.directory.path().join("verified.zrpl");
    let imported_path = fixture.directory.path().join("imported.ztas");
    let mut project = fixture.loader.create_project()?;
    assert_eq!(
        project.identity().sync_config_sha256,
        profile.sync_config(kind, selected)
    );
    assert_eq!(project.identity().patches.len(), 1);
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::Absent
    );
    if profile.multitap() {
        assert_eq!(
            project
                .identity()
                .devices
                .iter()
                .map(|device| device.port.as_str())
                .collect::<Vec<_>>(),
            ["p1", "p2", "p3", "p4", "p5"]
        );
    }
    let length = project.branch("main").expect("main branch").frame_count();
    project.edit_transaction(|edit| {
        edit.insert_frames("main", length, frame + 1 - length)?;
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
    let start_state = TasProject::read_zrpl_start_state(&replay_path)?;
    let selected_loader = super::super::select_private_tas_execution_loader_for_replay(
        fixture.archive.clone(),
        fixture.rom_path.clone(),
        crate::emu_backend::ActiveSystem::Pce,
        Vec::new(),
        &start_state,
    )?;
    let imported = selected_loader.import_replay_file(&replay_path, &imported_path, false)?;
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
        if profile.multitap() {
            crate::emu_thread::TasExecutionProfile::DirectPceMultitapCd
        } else {
            crate::emu_thread::TasExecutionProfile::DirectPceCd
        }
    );
    let reopened = super::super::select_private_tas_execution_loader_for_project(
        fixture.archive.clone(),
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

#[test]
fn archive_ppf_portable_replay_rebinds_same_effective_order_and_rejects_changed_patch_or_base()
-> Result<()> {
    let profile = Profile::Multitap(CardKind::MemoryBase);
    let kind = ArchivePpfKind::Zip;
    let fixture = fixture(profile, kind, true, 0xF1)?;
    with_catalogs(profile, &fixture, || {
        let project_path = fixture.directory.path().join("source.ztas");
        let replay_path = fixture.directory.path().join("verified.zrpl");
        let mut project = fixture.loader.create_project()?;
        project.edit_transaction(|edit| {
            edit.set_input_range("main", 0, 1, tagged_input(0xF2, true))
        })?;
        let autosaves =
            TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
        let cache = TasSeekStateCache::open(fixture.directory.path().join("source-seek-cache"))?;
        let mut editor = TasEditorSession::new(project.clone(), &project_path, autosaves, cache)?;
        PrivateTasExecutionLoader::DirectPceCd(fixture.loader.clone())
            .verify_and_export_editor_session(&mut editor, &replay_path)?;
        let original = TasProject::load(&project_path)?;
        let start_state = TasProject::read_zrpl_start_state(&replay_path)?;
        let _system_card =
            super::super::register_test_pce_cd_system_card(SYSTEM_CARD_SHA256, fixture.system_card);

        write_archive_ppf(&fixture.archive, kind, true, 0xF1, true, false)?;
        let reordered_loader = new_loader(
            profile,
            fixture.archive.clone(),
            fixture.rom_path.clone(),
            fixture.system_card,
        )?;
        assert!(reordered_loader.load_editor_engine(&original).is_err());
        let rebound_path = fixture.directory.path().join("rebound.ztas");
        let rebound_identity = reordered_loader
            .load_session(&start_state)?
            .identity()
            .clone();
        let rebound_plan = super::super::select_private_tas_execution_loader_for_replay(
            fixture.archive.clone(),
            fixture.rom_path.clone(),
            crate::emu_backend::ActiveSystem::Pce,
            Vec::new(),
            &start_state,
        )?;
        let rebound = rebound_plan.import_replay_file(&replay_path, &rebound_path, false)?;
        assert_eq!(rebound.identity(), &rebound_identity);
        assert_eq!(
            rebound.source_replay_sha256(),
            Some(TasDigest::from_bytes(&fs::read(&replay_path)?))
        );
        assert_ne!(
            rebound.identity().source_media_sha256,
            original.identity().source_media_sha256
        );
        assert_eq!(
            rebound.identity().effective_media_sha256,
            original.identity().effective_media_sha256
        );
        assert_eq!(rebound.start_state(), original.start_state());
        assert_eq!(rebound.identity().devices, original.identity().devices);
        assert_eq!(
            rebound.identity().sync_config_sha256,
            original.identity().sync_config_sha256
        );
        assert!(rebound.verification_is_current("main")?);
        let autosaves =
            TasAutosaveStore::beside_manual_save(&rebound_path, TasAutosaveConfig::default())?;
        let cache = TasSeekStateCache::open(fixture.directory.path().join("rebound-seek-cache"))?;
        let mut rebound_editor = TasEditorSession::open(&rebound_path, autosaves, cache)?;
        let mut rebound_engine = reordered_loader.load_editor_engine(rebound_editor.project())?;
        assert!(
            rebound_engine
                .seek(&mut rebound_editor, 1)?
                .reached_target()
        );

        write_archive_ppf(&fixture.archive, kind, true, 0xF1, false, true)?;
        let changed_patch = fixture.directory.path().join("changed-patch.ztas");
        assert!(
            super::super::select_private_tas_execution_loader_for_replay(
                fixture.archive.clone(),
                fixture.rom_path.clone(),
                crate::emu_backend::ActiveSystem::Pce,
                Vec::new(),
                &start_state,
            )
            .and_then(|plan| plan.import_replay_file(&replay_path, &changed_patch, false))
            .is_err()
        );
        assert!(!changed_patch.exists());

        write_archive_ppf(&fixture.archive, kind, true, 0xF0, false, false)?;
        let changed_base = fixture.directory.path().join("changed-base.ztas");
        assert!(
            super::super::select_private_tas_execution_loader_for_replay(
                fixture.archive.clone(),
                fixture.rom_path.clone(),
                crate::emu_backend::ActiveSystem::Pce,
                Vec::new(),
                &start_state,
            )
            .and_then(|plan| plan.import_replay_file(&replay_path, &changed_base, false))
            .is_err()
        );
        assert!(!changed_base.exists());
        Ok(())
    })
}

fn write_archive_ppf(
    path: &Path,
    kind: ArchivePpfKind,
    selected: bool,
    tag: u8,
    reordered: bool,
    changed_patch: bool,
) -> Result<()> {
    let target = if selected { "second" } else { "set" };
    let mut entries = Vec::new();
    if selected {
        entries.extend(cue_entries("first", tag.wrapping_add(1)));
    }
    entries.extend(cue_entries(target, tag));
    let first = ppf1(0, &[tag ^ if changed_patch { 0x3C } else { 0xA5 }]);
    let second = ppf1(1, &[tag ^ 0x5A]);
    if reordered {
        entries.push((format!("{target}/disc.ppf/0001.ppf"), second));
        entries.push((format!("{target}/disc.ppf/0002.ppf"), first));
    } else {
        entries.push((format!("{target}/disc.ppf/0001.ppf"), first));
        entries.push((format!("{target}/disc.ppf/0002.ppf"), second));
    }
    write_archive_entries(path, kind, entries)
}

fn cue_entries(directory: &str, tag: u8) -> Vec<(String, Vec<u8>)> {
    let mut disc = deterministic_disc_bytes(tag);
    disc[..8].copy_from_slice(&[0xDA, 0x7A, b'Z', b'R', b'P', b'L', tag, tag.rotate_left(1)]);
    vec![
        (format!("{directory}/disc.cue"), cue()),
        (format!("{directory}/disc.bin"), disc),
    ]
}

fn write_archive_entries(
    path: &Path,
    kind: ArchivePpfKind,
    entries: Vec<(String, Vec<u8>)>,
) -> Result<()> {
    match kind {
        ArchivePpfKind::SevenZip => {
            let mut writer = ArchiveWriter::create(path)?;
            writer.set_content_methods(vec![EncoderConfiguration::new(EncoderMethod::COPY)]);
            for (name, bytes) in entries {
                writer
                    .push_archive_entry(ArchiveEntry::new_file(&name), Some(Cursor::new(bytes)))?;
            }
            writer.finish()?;
        }
        ArchivePpfKind::Rar => {
            let entries = entries
                .into_iter()
                .map(|(name, bytes)| {
                    RarArchiveEntry::new(
                        name.into_bytes(),
                        EntrySource::from_bytes(std::sync::Arc::<[u8]>::from(bytes)),
                    )
                })
                .collect::<Vec<_>>();
            let bytes = Rar50Writer::new(
                WriterOptions::new(ArchiveVersion::Rar50, FeatureSet::store_only())
                    .with_compression_level(0),
            )
            .entries(entries)
            .finish()?;
            fs::write(path, bytes)?;
        }
        ArchivePpfKind::Zip => {
            let mut writer = zip::ZipWriter::new(fs::File::create(path)?);
            for (name, bytes) in entries {
                writer.start_file(name, zip::write::SimpleFileOptions::default())?;
                writer.write_all(&bytes)?;
            }
            writer.finish()?;
        }
    }
    Ok(())
}

fn cue() -> Vec<u8> {
    b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n".to_vec()
}
