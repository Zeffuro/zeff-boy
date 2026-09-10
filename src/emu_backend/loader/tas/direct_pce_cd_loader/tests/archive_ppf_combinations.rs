use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;

use anyhow::Result;
use rars::rar50::{ArchiveEntry as RarArchiveEntry, Rar50Writer, WriterOptions};
use rars::{ArchiveVersion, EntrySource, FeatureSet};
use sevenz_rust2::{ArchiveEntry, ArchiveWriter, EncoderConfiguration, EncoderMethod};

use super::multicue::ArchiveKind;
use super::*;
use crate::tas_project::TasInitialBranch;

#[derive(Clone, Copy, Debug)]
enum Card {
    None,
    Arcade,
    MemoryBase,
}

#[derive(Clone, Copy, Debug)]
enum Controller {
    TwoButton,
    Multitap,
}

enum CardCatalogGuard {
    Arcade(crate::emu_backend::pce_profiles::TestArcadeCardCatalogGuard),
    MemoryBase(crate::emu_backend::pce_profiles::TestMemoryBaseCatalogGuard),
}

impl CardCatalogGuard {
    fn release(self) {
        match self {
            Self::Arcade(guard) => drop(guard),
            Self::MemoryBase(guard) => drop(guard),
        }
    }
}

impl Card {
    fn register(self, source_disc_sha256: [u8; 32]) -> Option<CardCatalogGuard> {
        match self {
            Self::None => None,
            Self::Arcade => Some(CardCatalogGuard::Arcade(
                crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(
                    source_disc_sha256,
                ),
            )),
            Self::MemoryBase => Some(CardCatalogGuard::MemoryBase(
                crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
                    source_disc_sha256,
                ),
            )),
        }
    }

    fn flags(self) -> (bool, bool) {
        match self {
            Self::None => (false, false),
            Self::Arcade => (true, false),
            Self::MemoryBase => (false, true),
        }
    }
}

impl Controller {
    fn mode(self) -> PceControllerMode {
        match self {
            Self::TwoButton => PceControllerMode::TwoButton,
            Self::Multitap => PceControllerMode::Multitap,
        }
    }

    fn register(
        self,
        source_disc_sha256: [u8; 32],
    ) -> Option<crate::emu_backend::pce_profiles::TestControllerCatalogGuard> {
        matches!(self, Self::Multitap).then(|| {
            crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
                source_disc_sha256,
                PceControllerMode::Multitap,
            )
        })
    }
}

struct Fixture {
    directory: crate::test_support::TestDirectory,
    archive: std::path::PathBuf,
    kind: ArchiveKind,
    loader: DirectPceCdTasExecutionLoader,
    system_card: &'static [u8],
    source_disc_sha256: [u8; 32],
    card: Card,
    controller: Controller,
    selected: bool,
    tag: u8,
    plain_project: Option<TasProject>,
    card_catalog: Option<CardCatalogGuard>,
    controller_catalog: Option<crate::emu_backend::pce_profiles::TestControllerCatalogGuard>,
}

#[test]
fn archive_ppf_card_and_controller_profiles_reauthenticate_every_identity_domain() -> Result<()> {
    let mut tag = 0xA0;
    for kind in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip] {
        for selected in [false, true] {
            for (card, controller) in [
                (Card::None, Controller::Multitap),
                (Card::Arcade, Controller::TwoButton),
                (Card::Arcade, Controller::Multitap),
                (Card::MemoryBase, Controller::TwoButton),
                (Card::MemoryBase, Controller::Multitap),
            ] {
                exercise_profile(kind, selected, card, controller, tag)?;
                tag = tag.wrapping_add(1);
            }
        }
    }
    Ok(())
}

fn exercise_profile(
    kind: ArchiveKind,
    selected: bool,
    card: Card,
    controller: Controller,
    tag: u8,
) -> Result<()> {
    let mut fixture = fixture(kind, selected, card, controller, tag)?;
    let mut project = fixture.loader.create_project()?;
    assert_eq!(
        project.identity().sync_config_sha256,
        expected_sync(kind, selected, card, controller)
    );
    assert_eq!(
        super::super::super::direct_pce_cd::PceCdTasProfile::from_sync(
            project.identity().sync_config_sha256
        )
        .expect("archive PPF profile")
        .controller(),
        controller.mode()
    );
    assert_eq!(
        project.identity().devices.len(),
        if matches!(controller, Controller::Multitap) {
            5
        } else {
            1
        }
    );
    assert_eq!(project.identity().firmware.len(), 1);
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::Absent
    );
    assert_eq!(project.identity().patches.len(), 1);
    assert_eq!(
        project.identity().patches[0].format,
        crate::emu_backend::loader::tas::direct_pce_cd::PCE_CD_UNPATCHED_DISC_PATCH_FORMAT
    );
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );

    let mut backend = fixture.loader.load_fresh_backend()?;
    let inspection = match controller {
        Controller::TwoButton => {
            super::super::super::direct_pce_cd::validate_direct_pce_cd_tas_runtime(&backend, false)?
        }
        Controller::Multitap => {
            super::super::super::direct_pce_cd::validate_direct_pce_multitap_cd_tas_runtime(
                &backend, false,
            )?
        }
    };
    assert_eq!(
        (
            inspection.arcade_card_enabled,
            inspection.memory_base_enabled
        ),
        card.flags()
    );
    assert_eq!(
        inspection.controller_multitap.is_some(),
        matches!(controller, Controller::Multitap)
    );
    assert_eq!(backend.flush_battery_sram()?, None);
    assert_eq!(
        project.identity().patches[0].sha256.0,
        fixture.source_disc_sha256
    );

    assert_forged_identities_reject(&fixture.loader, &project)?;
    assert_reopens_and_rejects_wrong_member(&fixture, &project)?;
    if let Some(plain) = fixture.plain_project.as_ref() {
        assert!(fixture.loader.load_editor_engine(plain).is_err());
    }

    if matches!(card, Card::MemoryBase) {
        assert_memory_base_state_owned(&fixture.loader, &project, tag)?;
    }

    let input = input_for(controller);
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let manual = fixture.directory.path().join("movie.ztas");
    let autosaves = TasAutosaveStore::beside_manual_save(&manual, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(fixture.directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project.clone(), manual, autosaves, cache)?;
    let mut engine = fixture.loader.load_editor_engine(&project)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    let reached = engine.backend().encode_state_bytes()?;
    assert!(engine.seek(&mut editor, 0)?.reached_target());
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    assert_eq!(engine.backend().encode_state_bytes()?, reached);

    assert_catalogs_reauthenticate(&mut fixture, &project)?;
    assert_ordered_patch_variants_reject(&fixture, &project)?;
    Ok(())
}

fn fixture(
    kind: ArchiveKind,
    selected: bool,
    card: Card,
    controller: Controller,
    tag: u8,
) -> Result<Fixture> {
    let directory = crate::test_support::test_directory(&format!(
        "pce-cd-tas-archive-ppf-{kind:?}-{selected}-{card:?}-{controller:?}-{tag:02X}"
    ))?;
    let archive = directory.path().join(format!("disc.{}", kind.extension()));
    let first = ppf1(0, &[tag ^ 0x91]);
    let second = ppf1(1, &[tag ^ 0x4E]);
    write_ppf_archive(&archive, kind, selected, tag, [&first, &second])?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let source_loader = configured_loader(selected, Controller::TwoButton, &archive, system_card)?;
    let source_disc_sha256 = source_disc_sha256(&source_loader)?;
    let controller_catalog = controller.register(source_disc_sha256);
    let plain_project = (!matches!(card, Card::None))
        .then(|| configured_loader(selected, controller, &archive, system_card)?.create_project())
        .transpose()?;
    let card_catalog = card.register(source_disc_sha256);
    let loader = configured_loader(selected, controller, &archive, system_card)?;
    Ok(Fixture {
        directory,
        archive,
        kind,
        loader,
        system_card,
        source_disc_sha256,
        card,
        controller,
        selected,
        tag,
        plain_project,
        card_catalog,
        controller_catalog,
    })
}

fn configured_loader(
    selected: bool,
    controller: Controller,
    archive: &Path,
    system_card: &'static [u8],
) -> Result<DirectPceCdTasExecutionLoader> {
    configured_loader_with_member(
        selected.then_some("second/disc.cue"),
        controller,
        archive,
        system_card,
    )
}

fn configured_loader_with_member(
    member: Option<&str>,
    controller: Controller,
    archive: &Path,
    system_card: &'static [u8],
) -> Result<DirectPceCdTasExecutionLoader> {
    let selected_path = member.map(|member| archive.join(member));
    let mut loader = match controller {
        Controller::TwoButton => DirectPceCdTasExecutionLoader::new_with_rom_path(
            archive.to_owned(),
            selected_path,
            Vec::new(),
        )?,
        Controller::Multitap => DirectPceCdTasExecutionLoader::new_multitap_with_rom_path(
            archive.to_owned(),
            selected_path,
            Vec::new(),
        )?,
    };
    loader.system_card_override = Some(system_card);
    loader.system_card_sha256_override = Some(TEST_SYSTEM_CARD_SHA256);
    Ok(loader)
}

fn source_disc_sha256(loader: &DirectPceCdTasExecutionLoader) -> Result<[u8; 32]> {
    loader
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
        .and_then(|provenance| provenance.load.source_disc_sha256)
        .ok_or_else(|| anyhow::anyhow!("archive PPF source disc witness is unavailable"))
}

fn expected_sync(
    kind: ArchiveKind,
    selected: bool,
    card: Card,
    controller: Controller,
) -> TasDigest {
    let media = match kind {
        ArchiveKind::SevenZip => (false, false, false, true, false, false),
        ArchiveKind::Rar => (false, false, false, false, true, false),
        ArchiveKind::Zip => (false, false, false, false, false, true),
    };
    let archive_selection = match kind {
        ArchiveKind::SevenZip => (selected, false, false),
        ArchiveKind::Rar => (false, selected, false),
        ArchiveKind::Zip => (false, false, selected),
    };
    super::super::super::direct_pce_cd::PceCdTasProfile::from_runtime_flags(
        media,
        true,
        archive_selection,
        card.flags(),
        controller.mode(),
    )
    .expect("archive PPF card/controller profile")
    .sync_config()
}

fn assert_reopens_and_rejects_wrong_member(fixture: &Fixture, project: &TasProject) -> Result<()> {
    let _firmware = super::super::register_test_pce_cd_system_card(
        TEST_SYSTEM_CARD_SHA256,
        fixture.system_card,
    );
    let reopened = DirectPceCdTasExecutionLoader::new_for_project(
        fixture.archive.clone(),
        Vec::new(),
        project,
    )?;
    assert_eq!(
        reopened.archive_cue_member.as_deref(),
        fixture.selected.then_some("second/disc.cue")
    );
    reopened.load_editor_engine(project)?;
    if fixture.selected {
        assert!(
            DirectPceCdTasExecutionLoader::new_with_rom_path(
                fixture.archive.clone(),
                None,
                Vec::new(),
            )
            .is_err()
        );
        let first_source = source_disc_sha256(&configured_loader_with_member(
            Some("first/disc.cue"),
            Controller::TwoButton,
            &fixture.archive,
            fixture.system_card,
        )?)?;
        let _first_card_catalog = fixture.card.register(first_source);
        let _first_controller_catalog = fixture.controller.register(first_source);
        let wrong = configured_loader_with_member(
            Some("first/disc.cue"),
            fixture.controller,
            &fixture.archive,
            fixture.system_card,
        )?;
        let wrong_project = wrong.create_project()?;
        assert_eq!(
            wrong_project.identity().sync_config_sha256,
            expected_sync(fixture.kind, true, fixture.card, fixture.controller)
        );
        assert_ne!(
            wrong_project.identity().source_media_sha256,
            project.identity().source_media_sha256
        );
        assert!(wrong.load_editor_engine(project).is_err());
    }
    Ok(())
}

fn assert_forged_identities_reject(
    loader: &DirectPceCdTasExecutionLoader,
    project: &TasProject,
) -> Result<()> {
    let mut identity = project.identity().clone();
    identity.source_media_sha256 = TasDigest([0xA1; 32]);
    assert!(
        loader
            .load_editor_engine(&project_with_identity(project, identity)?)
            .is_err()
    );

    let mut identity = project.identity().clone();
    identity.effective_media_sha256 = TasDigest([0xA2; 32]);
    assert!(
        loader
            .load_editor_engine(&project_with_identity(project, identity)?)
            .is_err()
    );

    let mut identity = project.identity().clone();
    identity.sync_config_sha256 = TasDigest([0xA3; 32]);
    assert!(
        loader
            .load_editor_engine(&project_with_identity(project, identity)?)
            .is_err()
    );

    let mut identity = project.identity().clone();
    identity.devices[0].configuration_sha256 = TasDigest([0xA4; 32]);
    assert!(
        loader
            .load_editor_engine(&project_with_identity(project, identity)?)
            .is_err()
    );

    let mut identity = project.identity().clone();
    identity.firmware.clear();
    assert!(
        loader
            .load_editor_engine(&project_with_identity(project, identity)?)
            .is_err()
    );

    let mut identity = project.identity().clone();
    identity.patches.clear();
    assert!(
        loader
            .load_editor_engine(&project_with_identity(project, identity)?)
            .is_err()
    );

    let mut identity = project.identity().clone();
    identity.patches[0].format = "unexpected-patch-format".to_owned();
    assert!(
        loader
            .load_editor_engine(&project_with_identity(project, identity)?)
            .is_err()
    );

    let mut identity = project.identity().clone();
    identity.patches[0].sha256 = TasDigest([0xA4; 32]);
    assert!(
        loader
            .load_editor_engine(&project_with_identity(project, identity)?)
            .is_err()
    );

    let mut identity = project.identity().clone();
    identity.patches.push(identity.patches[0].clone());
    assert!(
        loader
            .load_editor_engine(&project_with_identity(project, identity)?)
            .is_err()
    );
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

fn assert_memory_base_state_owned(
    loader: &DirectPceCdTasExecutionLoader,
    project: &TasProject,
    tag: u8,
) -> Result<()> {
    let mut backend = loader.load_fresh_backend()?;
    assert_eq!(backend.encode_state_bytes()?, project.start_state());
    let crate::emu_backend::EmuBackend::Pce(pce) = &mut backend else {
        unreachable!("archive PPF Memory Base fixture must load a PC Engine backend");
    };
    pce.load_memory_base128(&vec![tag; zeff_pce_core::hardware::MEMORY_BASE128_RAM_LEN])?;
    let seeded = backend.encode_state_bytes()?;
    assert_ne!(seeded, project.start_state());
    let mut restored = loader.load_fresh_backend()?;
    restored.load_state_from_bytes(seeded.clone())?;
    assert_eq!(restored.encode_state_bytes()?, seeded);
    assert_eq!(restored.flush_battery_sram()?, None);
    let session = loader.load_session(&seeded)?;
    let seeded_project = TasProject::new(
        "native-memory-base",
        session.identity().clone(),
        seeded.clone(),
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
    let seeded_engine = loader.load_editor_engine(&seeded_project)?;
    assert_eq!(seeded_engine.backend().encode_state_bytes()?, seeded);
    Ok(())
}

fn input_for(controller: Controller) -> TasInputFrame {
    let mut input = TasInputFrame::default();
    let players = if matches!(controller, Controller::Multitap) {
        5
    } else {
        1
    };
    for (index, player) in input.players.iter_mut().take(players).enumerate() {
        player.buttons = 1 << index.min(3);
        player.dpad = 1 << (3 - index.min(3));
    }
    input
}

fn assert_catalogs_reauthenticate(fixture: &mut Fixture, project: &TasProject) -> Result<()> {
    if let Some(card_catalog) = fixture.card_catalog.take() {
        card_catalog.release();
        assert!(fixture.loader.load_session(project.start_state()).is_err());
        let effective_card = fixture
            .card
            .register(project.identity().effective_media_sha256.0);
        assert!(fixture.loader.load_session(project.start_state()).is_err());
        if let Some(effective_card) = effective_card {
            effective_card.release();
        }
        fixture.card_catalog = fixture.card.register(fixture.source_disc_sha256);
    }
    if let Some(controller_catalog) = fixture.controller_catalog.take() {
        drop(controller_catalog);
        assert!(fixture.loader.load_session(project.start_state()).is_err());
        let effective_controller = Controller::Multitap
            .register(project.identity().effective_media_sha256.0)
            .expect("Multitap controller catalog");
        assert!(fixture.loader.load_session(project.start_state()).is_err());
        drop(effective_controller);
        fixture.controller_catalog = fixture.controller.register(fixture.source_disc_sha256);
    }
    Ok(())
}

fn assert_ordered_patch_variants_reject(fixture: &Fixture, project: &TasProject) -> Result<()> {
    let first = ppf1(0, &[0x41]);
    let second = ppf1(1, &[0x50]);
    write_ppf_archive(
        &fixture.archive,
        fixture.kind,
        fixture.selected,
        fixture.tag,
        [&first, &second],
    )?;
    let no_op = configured_loader(
        fixture.selected,
        fixture.controller,
        &fixture.archive,
        fixture.system_card,
    )?;
    let no_op_effective = no_op
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("no-op archive PPF effective disc");
    assert_eq!(no_op_effective, fixture.source_disc_sha256);
    assert!(no_op.load_editor_engine(project).is_err());

    let original_first = ppf1(0, &[fixture.tag ^ 0x91]);
    let original_second = ppf1(1, &[fixture.tag ^ 0x4E]);
    write_ppf_archive(
        &fixture.archive,
        fixture.kind,
        fixture.selected,
        fixture.tag,
        [&original_second, &original_first],
    )?;
    let commuting = configured_loader(
        fixture.selected,
        fixture.controller,
        &fixture.archive,
        fixture.system_card,
    )?;
    let effective = commuting
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("commuting archive PPF effective disc");
    assert_eq!(effective, project.identity().effective_media_sha256.0);
    assert!(commuting.load_editor_engine(project).is_err());
    Ok(())
}

fn write_ppf_archive(
    archive: &Path,
    kind: ArchiveKind,
    selected: bool,
    tag: u8,
    patches: [&[u8]; 2],
) -> Result<()> {
    let target = if selected { "second" } else { "set" };
    let mut entries = Vec::new();
    if selected {
        entries.extend(cue_entries("first", tag.wrapping_add(0x31)));
        entries.push(("first/disc.ppf/0002.ppf".to_owned(), patches[1].to_vec()));
        entries.push(("first/disc.ppf/0001.ppf".to_owned(), patches[0].to_vec()));
    }
    entries.extend(cue_entries(target, tag));
    entries.push((format!("{target}/disc.ppf/0002.ppf"), patches[1].to_vec()));
    entries.push((format!("{target}/disc.ppf/0001.ppf"), patches[0].to_vec()));
    write_entries(archive, kind, entries)
}

fn cue_entries(directory: &str, tag: u8) -> Vec<(String, Vec<u8>)> {
    let mut disc = vec![tag; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
    disc[..4].copy_from_slice(&[0x41, 0x50, tag, tag.rotate_left(1)]);
    vec![
        (
            format!("{directory}/disc.cue"),
            b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n".to_vec(),
        ),
        (format!("{directory}/disc.bin"), disc),
    ]
}

fn write_entries(archive: &Path, kind: ArchiveKind, entries: Vec<(String, Vec<u8>)>) -> Result<()> {
    match kind {
        ArchiveKind::SevenZip => {
            let mut writer = ArchiveWriter::create(archive)?;
            writer.set_content_methods(vec![EncoderConfiguration::new(EncoderMethod::COPY)]);
            for (name, bytes) in entries {
                writer
                    .push_archive_entry(ArchiveEntry::new_file(&name), Some(Cursor::new(bytes)))?;
            }
            writer.finish()?;
        }
        ArchiveKind::Rar => {
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
            fs::write(archive, bytes)?;
        }
        ArchiveKind::Zip => {
            let file = fs::File::create(archive)?;
            let mut writer = zip::ZipWriter::new(file);
            for (name, bytes) in entries {
                writer.start_file(name, zip::write::SimpleFileOptions::default())?;
                writer.write_all(&bytes)?;
            }
            writer.finish()?;
        }
    }
    Ok(())
}
