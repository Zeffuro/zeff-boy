use std::collections::BTreeMap;

use super::*;
use crate::tas_project::TasInitialBranch;

#[derive(Clone, Copy, Debug)]
enum Route {
    Chd,
    Iso,
    Ppf,
}

#[derive(Clone, Copy, Debug)]
enum Card {
    Arcade,
    MemoryBase,
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

impl Route {
    fn sync(self, card: Card) -> TasDigest {
        match (self, card) {
            (Self::Chd, Card::Arcade) => super::super::super::direct_pce_cd::direct_pce_multitap_cd_chd_arcade_tas_sync_config_sha256(),
            (Self::Chd, Card::MemoryBase) => super::super::super::direct_pce_cd::direct_pce_multitap_cd_chd_memory_base_tas_sync_config_sha256(),
            (Self::Iso, Card::Arcade) => super::super::super::direct_pce_cd::direct_pce_multitap_cd_iso_arcade_tas_sync_config_sha256(),
            (Self::Iso, Card::MemoryBase) => super::super::super::direct_pce_cd::direct_pce_multitap_cd_iso_memory_base_tas_sync_config_sha256(),
            (Self::Ppf, Card::Arcade) => super::super::super::direct_pce_cd::direct_pce_multitap_cd_ppf_arcade_tas_sync_config_sha256(),
            (Self::Ppf, Card::MemoryBase) => super::super::super::direct_pce_cd::direct_pce_multitap_cd_ppf_memory_base_tas_sync_config_sha256(),
        }
    }
}

impl Card {
    fn register_catalog(self, disc_sha256: [u8; 32]) -> CardCatalogGuard {
        match self {
            Self::Arcade => CardCatalogGuard::Arcade(
                crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(
                    disc_sha256,
                ),
            ),
            Self::MemoryBase => CardCatalogGuard::MemoryBase(
                crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
                    disc_sha256,
                ),
            ),
        }
    }

    fn native_flags(self) -> (bool, bool) {
        match self {
            Self::Arcade => (true, false),
            Self::MemoryBase => (false, true),
        }
    }
}

struct Fixture {
    directory: crate::test_support::TestDirectory,
    source_path: std::path::PathBuf,
    loader: DirectPceCdTasExecutionLoader,
    system_card: &'static [u8],
    source_disc_sha256: [u8; 32],
    ppf_stack: Option<crate::emu_backend::pce_cd::PceCdTasPpfStack>,
    ppf_patches: Option<Vec<(String, Vec<u8>)>>,
}

struct PpfRejectionContext<'a> {
    card: Card,
    fixture: &'a Fixture,
    project: &'a TasProject,
    card_catalog: CardCatalogGuard,
    controller_catalog: crate::emu_backend::pce_profiles::TestControllerCatalogGuard,
}

#[test]
fn direct_card_multitap_six_profiles_bind_identity_native_state_and_catalogs() -> Result<()> {
    for (index, (route, card)) in [
        (Route::Chd, Card::Arcade),
        (Route::Chd, Card::MemoryBase),
        (Route::Iso, Card::Arcade),
        (Route::Iso, Card::MemoryBase),
        (Route::Ppf, Card::Arcade),
        (Route::Ppf, Card::MemoryBase),
    ]
    .into_iter()
    .enumerate()
    {
        exercise_route(route, card, 0xF0 + index as u8)?;
    }
    Ok(())
}

fn exercise_route(route: Route, card: Card, tag: u8) -> Result<()> {
    let fixture = fixture(route, card, tag)?;
    let card_catalog = card.register_catalog(fixture.source_disc_sha256);
    let controller_catalog =
        crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            fixture.source_disc_sha256,
            PceControllerMode::Multitap,
        );
    let mut project = fixture.loader.create_project()?;
    assert_eq!(project.identity().sync_config_sha256, route.sync(card));
    assert_eq!(project.identity().devices.len(), 5);
    assert_eq!(
        super::super::super::classify_direct_tas_execution_profile(&project)?,
        TasExecutionProfile::DirectPceMultitapCd
    );
    assert_eq!(project.identity().firmware.len(), 1);
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::Absent
    );

    let mut backend = fixture.loader.load_fresh_backend()?;
    let inspection =
        super::super::super::direct_pce_cd::validate_direct_pce_multitap_cd_tas_runtime(
            &backend, false,
        )?;
    assert_eq!(
        (
            inspection.arcade_card_enabled,
            inspection.memory_base_enabled
        ),
        card.native_flags()
    );
    assert!(inspection.controller_multitap.is_some());
    assert_eq!(backend.flush_battery_sram()?, None);

    assert_mismatched_project_identities(&fixture.loader, &project)?;
    assert_project_reopens(
        &fixture.source_path,
        &project,
        fixture.system_card,
        fixture.ppf_stack.clone(),
    )?;

    if matches!(card, Card::MemoryBase) {
        let mut neutral = fixture.loader.load_fresh_backend()?;
        assert_eq!(neutral.encode_state_bytes()?, project.start_state());
        let crate::emu_backend::EmuBackend::Pce(pce) = &mut neutral else {
            unreachable!("direct Memory Base fixture must load a PC Engine backend");
        };
        pce.load_memory_base128(&vec![tag; zeff_pce_core::hardware::MEMORY_BASE128_RAM_LEN])?;
        let seeded = neutral.encode_state_bytes()?;
        assert_ne!(seeded, project.start_state());
        let mut fresh = fixture.loader.load_fresh_backend()?;
        fresh.load_state_from_bytes(seeded.clone())?;
        assert_eq!(fresh.encode_state_bytes()?, seeded);
        assert_eq!(fresh.flush_battery_sram()?, None);
        let session = fixture.loader.load_session(&seeded)?;
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
        let seeded_engine = fixture.loader.load_editor_engine(&seeded_project)?;
        assert_eq!(seeded_engine.backend().encode_state_bytes()?, seeded);
    }

    let input = five_player_input();
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

    if fixture.ppf_patches.is_some() {
        assert_ppf_rejections(PpfRejectionContext {
            card,
            fixture: &fixture,
            project: &project,
            card_catalog,
            controller_catalog,
        })?;
    } else {
        card_catalog.release();
        assert!(fixture.loader.load_editor_engine(&project).is_err());
        let _card_catalog = card.register_catalog(fixture.source_disc_sha256);
        drop(controller_catalog);
        assert!(fixture.loader.load_editor_engine(&project).is_err());
    }
    Ok(())
}

fn fixture(route: Route, card: Card, tag: u8) -> Result<Fixture> {
    let directory = crate::test_support::test_directory(&format!(
        "pce-cd-tas-direct-{route:?}-{card:?}-multitap-{tag:02X}"
    ))?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let (source_path, ppf_stack, ppf_patches) = match route {
        Route::Chd => {
            let path = directory.path().join("disc.chd");
            crate::emu_backend::pce_cd_chd::write_synthetic_uncompressed_v5_chd(&path)?;
            let mut bytes = fs::read(&path)?;
            let offset = 4 * 2_448;
            bytes[offset] ^= tag;
            bytes[offset + 1] ^= tag.rotate_left(1);
            fs::write(&path, bytes)?;
            (path, None, None)
        }
        Route::Iso => {
            let path = directory.path().join("disc.iso");
            write_tagged_disc(&path, tag)?;
            fs::write(
                directory.path().join("disc.cue"),
                b"FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
            )?;
            (path, None, None)
        }
        Route::Ppf => {
            let path = directory.path().join("disc.cue");
            write_tagged_disc(&directory.path().join("disc.bin"), tag)?;
            fs::write(
                &path,
                b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
            )?;
            let patches = vec![
                ("first.ppf".to_owned(), ppf1(0, &[tag ^ 0x3C])),
                ("second.ppf".to_owned(), ppf1(1, &[tag ^ 0xC3])),
            ];
            let stack =
                crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(&path, patches.clone())?;
            (path, Some(stack), Some(patches))
        }
    };
    let base = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path.clone(),
        system_card,
        TEST_SYSTEM_CARD_SHA256,
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
            TEST_SYSTEM_CARD_SHA256,
            stack,
        )
    } else {
        DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
            source_path.clone(),
            system_card,
            TEST_SYSTEM_CARD_SHA256,
        )
    };
    Ok(Fixture {
        directory,
        source_path,
        loader,
        system_card,
        source_disc_sha256,
        ppf_stack,
        ppf_patches,
    })
}

fn write_tagged_disc(path: &std::path::Path, tag: u8) -> Result<()> {
    let mut disc = vec![tag; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
    disc[..4].copy_from_slice(&[0x43, 0x44, tag, tag.rotate_left(1)]);
    fs::write(path, disc)?;
    Ok(())
}

fn five_player_input() -> TasInputFrame {
    let mut input = TasInputFrame::default();
    for (index, player) in input.players.iter_mut().enumerate() {
        player.buttons = 1 << index.min(3);
        player.dpad = 1 << (3 - index.min(3));
    }
    input
}

fn assert_project_reopens(
    source_path: &std::path::Path,
    project: &TasProject,
    system_card: &'static [u8],
    ppf_stack: Option<crate::emu_backend::pce_cd::PceCdTasPpfStack>,
) -> Result<()> {
    if let Some(stack) = ppf_stack {
        let _firmware =
            super::super::register_test_pce_cd_system_card(TEST_SYSTEM_CARD_SHA256, system_card);
        let _stack = super::super::register_test_pce_cd_ppf_stack(source_path.to_owned(), stack);
        let reopened = DirectPceCdTasExecutionLoader::new_for_project(
            source_path.to_owned(),
            Vec::new(),
            project,
        )?;
        reopened.load_editor_engine(project)?;
    } else {
        let mut reopened = DirectPceCdTasExecutionLoader::new_for_project(
            source_path.to_owned(),
            Vec::new(),
            project,
        )?;
        reopened.system_card_override = Some(system_card);
        reopened.system_card_sha256_override = Some(TEST_SYSTEM_CARD_SHA256);
        reopened.load_editor_engine(project)?;
    }
    Ok(())
}

fn assert_mismatched_project_identities(
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
    identity.devices.pop();
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

fn assert_ppf_rejections(context: PpfRejectionContext<'_>) -> Result<()> {
    let PpfRejectionContext {
        card,
        fixture,
        project,
        card_catalog,
        controller_catalog,
    } = context;
    let patches = fixture
        .ppf_patches
        .as_deref()
        .expect("PPF fixture must provide its ordered stack");
    let mut tampered = patches.to_vec();
    tampered[0].1 = ppf1(0, &[0xA5]);
    let changed = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_and_ppf_stack(
        fixture.source_path.clone(),
        fixture.system_card,
        TEST_SYSTEM_CARD_SHA256,
        crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(&fixture.source_path, tampered)?,
    );
    let changed_effective = changed
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("changed PPF fixture disc");
    assert_ne!(
        changed_effective,
        project.identity().effective_media_sha256.0
    );
    assert!(changed.load_editor_engine(project).is_err());

    let mut reordered = patches.to_vec();
    reordered.reverse();
    let reordered = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_and_ppf_stack(
        fixture.source_path.clone(),
        fixture.system_card,
        TEST_SYSTEM_CARD_SHA256,
        crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(&fixture.source_path, reordered)?,
    );
    let reordered_effective = reordered
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("reordered PPF fixture disc");
    assert_eq!(
        reordered_effective,
        project.identity().effective_media_sha256.0
    );
    assert!(reordered.load_editor_engine(project).is_err());

    let mut marker = project.identity().clone();
    marker.patches.clear();
    let markerless = project_with_identity(project, marker)?;
    assert!(fixture.loader.load_editor_engine(&markerless).is_err());
    assert!(
        DirectPceCdTasExecutionLoader::new_for_project(
            fixture.source_path.clone(),
            Vec::new(),
            &markerless,
        )
        .is_err()
    );
    let mut marker = project.identity().clone();
    marker.patches[0].sha256 = TasDigest([0xA4; 32]);
    let changed_marker = project_with_identity(project, marker)?;
    assert!(fixture.loader.load_editor_engine(&changed_marker).is_err());
    assert!(
        DirectPceCdTasExecutionLoader::new_for_project(
            fixture.source_path.clone(),
            Vec::new(),
            &changed_marker,
        )
        .is_err()
    );

    let effective = project.identity().effective_media_sha256.0;
    assert_ne!(effective, fixture.source_disc_sha256);
    card_catalog.release();
    let effective_card = card.register_catalog(effective);
    assert!(fixture.loader.load_editor_engine(project).is_err());
    effective_card.release();
    let source_card = card.register_catalog(fixture.source_disc_sha256);
    drop(controller_catalog);
    let effective_controller =
        crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            effective,
            PceControllerMode::Multitap,
        );
    assert!(fixture.loader.load_editor_engine(project).is_err());
    drop(effective_controller);
    source_card.release();
    Ok(())
}
