use std::io::{Cursor, Write};
use std::time::{Duration, Instant};

use rars::rar50::{ArchiveEntry as RarArchiveEntry, Rar50Writer, WriterOptions};
use rars::{ArchiveVersion, EntrySource, FeatureSet};
use sevenz_rust2::{ArchiveEntry, ArchiveWriter, EncoderConfiguration, EncoderMethod};

use super::harness::{app_with_worker, live_ok, wait_for_linked, wait_for_recorded_frame};
use super::*;
use crate::emu_backend::ActiveSystem;
use crate::emu_backend::loader::{DirectPceCdTasExecutionLoader, DirectPceTasExecutionLoader};
use crate::emu_thread::EmuThread;
use crate::input::HostButton;
use crate::live_control::{LiveCommand, TasRecordMode};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasEditorSession, TasSeekStateCache,
};
use zeff_pce_core::hardware::{PceArcadeCardMode, PceMemoryBaseMode};

mod archive_ppf;
mod multitap;
mod selected_cards;

#[derive(Clone, Copy, Debug)]
enum CardKind {
    None,
    Arcade,
    MemoryBase,
}

#[derive(Clone, Copy, Debug)]
enum ArchiveKind {
    SevenZip,
    Rar,
    Zip,
}

#[test]
fn unique_cue_archive_card_profiles_link_record_and_disconnect() {
    let mut fill = 0x61;
    for archive in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip] {
        for card in [CardKind::None, CardKind::Arcade, CardKind::MemoryBase] {
            run_live_roundtrip(archive, card, fill);
            fill += 1;
        }
    }
}

#[test]
fn selected_second_cue_archive_projects_link_record_and_restore() {
    for (archive, fill) in [
        (ArchiveKind::SevenZip, 0x71),
        (ArchiveKind::Rar, 0x72),
        (ArchiveKind::Zip, 0x73),
    ] {
        run_selected_second_cue_live_roundtrip(archive, CardKind::None, fill, false);
    }
}

#[test]
fn selected_zip_hucard_multitap_links_and_records_five_players() {
    let root = crate::test_support::test_directory("tas-pce-multitap-zip-live").unwrap();
    let archive_path = root.path().join("games.zip");
    let first = pce_hucard();
    let mut selected = pce_hucard();
    *selected.last_mut().unwrap() ^= 1;
    crate::test_support::write_zip(
        &archive_path,
        &[("first.pce", &first), ("folder/selected.pce", &selected)],
    )
    .unwrap();
    let selected_path = archive_path.join("folder/selected.pce");
    let loader = DirectPceTasExecutionLoader::new_zip_multitap(
        archive_path.clone(),
        Some(selected_path.clone()),
    );
    let project = loader.create_project().unwrap();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 96, ActiveSystem::Pce, selected_path.clone());
    app.rom_info.source_path = Some(archive_path);
    app.rom_info.rom_path = Some(selected_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(96, snapshot, TasControlStartMode::Preview)
        .unwrap();
    wait_for_linked(&mut app);
    for command in [
        LiveCommand::Button {
            player: 1,
            key: HostButton::Left,
            pressed: true,
        },
        LiveCommand::Button {
            player: 5,
            key: HostButton::A,
            pressed: true,
        },
    ] {
        live_ok(&mut app, command);
    }
    live_ok(
        &mut app,
        LiveCommand::TasRecordFrame {
            mode: TasRecordMode::Replace,
        },
    );
    wait_for_recorded_frame(&mut app);
    let input = app
        .debug_windows
        .tas_editor
        .active_session()
        .unwrap()
        .selected_branch()
        .input_at(0);
    assert_eq!(input.players[0].dpad, 0x02);
    assert_eq!(input.players[4].buttons, 0x01);

    let reply = live_ok(&mut app, LiveCommand::TasDisconnect { keep: false });
    assert_eq!(reply["live"]["state"], "returning");
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.tas_control.state != TasControlState::Detached && Instant::now() < deadline {
        app.drain_emu_responses();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(app.tas_control.state, TasControlState::Detached);
}

fn run_selected_second_cue_live_roundtrip(
    archive: ArchiveKind,
    card: CardKind,
    fill: u8,
    keep: bool,
) {
    let name =
        format!("tas-pce-cd-{archive:?}-selected-second-{card:?}-{keep}").to_ascii_lowercase();
    let root = crate::test_support::test_directory(&name).unwrap();
    let archive_path = root.path().join(match archive {
        ArchiveKind::SevenZip => "disc.7z",
        ArchiveKind::Rar => "disc.rar",
        ArchiveKind::Zip => "disc.zip",
    });
    write_multicue_archive(&archive_path, archive, fill);
    let first_cue = archive_path.join("first").join("disc.cue");
    let second_cue = archive_path.join("second").join("disc.cue");
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let system_card_sha256 = zeff_firmware::sha256_bytes(system_card);
    let loader = DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
        archive_path.clone(),
        second_cue.clone(),
        system_card,
        system_card_sha256,
    )
    .unwrap();
    let disc_sha256 = loader
        .load_fresh_backend()
        .unwrap()
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("selected fixture disc");
    let _arcade_catalog = matches!(card, CardKind::Arcade).then(|| {
        crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256)
    });
    let _memory_base_catalog = matches!(card, CardKind::MemoryBase).then(|| {
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(disc_sha256)
    });
    let first_loader = DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
        archive_path.clone(),
        first_cue,
        system_card,
        system_card_sha256,
    )
    .unwrap();
    let project = loader.create_project().unwrap();
    assert_ne!(
        project.identity().source_media_sha256,
        first_loader
            .create_project()
            .unwrap()
            .identity()
            .source_media_sha256
    );
    let engine = loader.load_editor_engine(&project).unwrap();
    let pce = engine.backend().pce().expect("selected fixture PCE-CD");
    assert_eq!(
        pce.arcade_card_mode(),
        if matches!(card, CardKind::Arcade) {
            PceArcadeCardMode::Enabled
        } else {
            PceArcadeCardMode::Disabled
        }
    );
    assert_eq!(
        pce.memory_base_mode(),
        if matches!(card, CardKind::MemoryBase) {
            PceMemoryBaseMode::Enabled
        } else {
            PceMemoryBaseMode::Disabled
        }
    );
    let backend = engine.into_backend();

    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 95, ActiveSystem::Pce, second_cue.clone());
    app.rom_info.source_path = Some(archive_path);
    app.rom_info.rom_path = Some(second_cue);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(95, snapshot, TasControlStartMode::Preview)
        .unwrap();
    wait_for_linked(&mut app);
    live_ok(
        &mut app,
        LiveCommand::Button {
            player: 1,
            key: HostButton::A,
            pressed: true,
        },
    );
    live_ok(
        &mut app,
        LiveCommand::TasRecordFrame {
            mode: TasRecordMode::Replace,
        },
    );
    wait_for_recorded_frame(&mut app);
    assert_eq!(
        app.debug_windows
            .tas_editor
            .active_session()
            .unwrap()
            .selected_branch()
            .input_at(0)
            .players[0]
            .buttons,
        0x01
    );

    let reply = live_ok(&mut app, LiveCommand::TasDisconnect { keep });
    assert_eq!(
        reply["live"]["state"],
        if keep { "keeping" } else { "returning" }
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.tas_control.state != TasControlState::Detached && Instant::now() < deadline {
        app.drain_emu_responses();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(app.tas_control.state, TasControlState::Detached);
}

fn run_live_roundtrip(archive: ArchiveKind, card: CardKind, fill: u8) {
    let name = format!("tas-pce-cd-{archive:?}-live-record-{card:?}").to_ascii_lowercase();
    let root = crate::test_support::test_directory(&name).unwrap();
    let archive_path = root.path().join(match archive {
        ArchiveKind::SevenZip => "disc.7z",
        ArchiveKind::Rar => "disc.rar",
        ArchiveKind::Zip => "disc.zip",
    });
    write_archive(&archive_path, archive, fill);
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        archive_path.clone(),
        system_card,
        zeff_firmware::sha256_bytes(system_card),
    );
    let disc_sha256 = loader
        .load_fresh_backend()
        .unwrap()
        .pce()
        .unwrap()
        .normalized_disc_hash()
        .unwrap();
    let _arcade_catalog = matches!(card, CardKind::Arcade).then(|| {
        crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256)
    });
    let _memory_base_catalog = matches!(card, CardKind::MemoryBase).then(|| {
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(disc_sha256)
    });
    let project = loader.create_project().unwrap();
    let engine = loader.load_editor_engine(&project).unwrap();
    let pce = engine.backend().pce().unwrap();
    assert_eq!(
        pce.arcade_card_mode(),
        if matches!(card, CardKind::Arcade) {
            PceArcadeCardMode::Enabled
        } else {
            PceArcadeCardMode::Disabled
        }
    );
    assert_eq!(
        pce.memory_base_mode(),
        if matches!(card, CardKind::MemoryBase) {
            PceMemoryBaseMode::Enabled
        } else {
            PceMemoryBaseMode::Disabled
        }
    );
    let backend = engine.into_backend();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let loaded_path = if matches!(archive, ArchiveKind::Zip) {
        archive_path.join("set").join("disc.cue")
    } else {
        archive_path.clone()
    };
    let mut app = app_with_worker(worker, 94, ActiveSystem::Pce, loaded_path.clone());
    app.rom_info.source_path = Some(archive_path);
    app.rom_info.rom_path = Some(loaded_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    assert!(matches!(
        app.detached_tas_editor_live_status(),
        crate::debug::TasEditorLiveStatus::Unavailable(reason)
            if reason == "Checking TAS readiness…"
    ));
    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(94, snapshot, TasControlStartMode::Preview)
        .unwrap();
    wait_for_linked(&mut app);

    for command in [
        LiveCommand::Button {
            player: 1,
            key: HostButton::Left,
            pressed: true,
        },
        LiveCommand::Button {
            player: 1,
            key: HostButton::A,
            pressed: true,
        },
    ] {
        live_ok(&mut app, command);
    }
    live_ok(
        &mut app,
        LiveCommand::TasRecordFrame {
            mode: TasRecordMode::Replace,
        },
    );
    wait_for_recorded_frame(&mut app);

    let input = app
        .debug_windows
        .tas_editor
        .active_session()
        .unwrap()
        .selected_branch()
        .input_at(0);
    assert_eq!(input.players[0].buttons, 0x01);
    assert_eq!(input.players[0].dpad, 0x02);

    let reply = live_ok(&mut app, LiveCommand::TasDisconnect { keep: false });
    assert_eq!(reply["live"]["state"], "returning");
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.tas_control.state != TasControlState::Detached && Instant::now() < deadline {
        app.drain_emu_responses();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(app.tas_control.state, TasControlState::Detached);
}

fn write_archive(path: &std::path::Path, archive: ArchiveKind, fill: u8) {
    write_archive_bytes(
        path,
        archive,
        vec![fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
    );
}

fn write_archive_bytes(path: &std::path::Path, archive: ArchiveKind, disc: Vec<u8>) {
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    match archive {
        ArchiveKind::SevenZip => write_7z(path, cue, disc),
        ArchiveKind::Rar => write_rar(path, cue, disc),
        ArchiveKind::Zip => write_zip(path, cue, disc),
    }
}

fn pce_hucard() -> Vec<u8> {
    let mut rom = vec![0; zeff_pce_core::hardware::PCEAS_HEADER_LEN];
    rom[0] = 1;
    rom.extend(vec![0xEA; 0x2000]);
    rom
}

fn write_multicue_archive(path: &std::path::Path, archive: ArchiveKind, second_fill: u8) {
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let first = vec![second_fill ^ 0xFF; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
    let mut second = vec![second_fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
    second[0..4].copy_from_slice(&[0x41, 0x50, second_fill, second_fill.rotate_left(1)]);
    match archive {
        ArchiveKind::SevenZip => {
            let mut writer = ArchiveWriter::create(path).unwrap();
            writer.set_content_methods(vec![EncoderConfiguration::new(EncoderMethod::COPY)]);
            for (name, bytes) in [
                ("first/disc.cue", cue.as_slice()),
                ("first/disc.bin", first.as_slice()),
                ("second/disc.cue", cue.as_slice()),
                ("second/disc.bin", second.as_slice()),
            ] {
                writer
                    .push_archive_entry(ArchiveEntry::new_file(name), Some(Cursor::new(bytes)))
                    .unwrap();
            }
            writer.finish().unwrap();
        }
        ArchiveKind::Rar => {
            let entries = [
                ("first/disc.cue", cue.as_slice()),
                ("first/disc.bin", first.as_slice()),
                ("second/disc.cue", cue.as_slice()),
                ("second/disc.bin", second.as_slice()),
            ]
            .into_iter()
            .map(|(name, bytes)| {
                RarArchiveEntry::new(
                    name.as_bytes().to_vec(),
                    EntrySource::from_bytes(std::sync::Arc::<[u8]>::from(bytes.to_vec())),
                )
            })
            .collect::<Vec<_>>();
            let bytes = Rar50Writer::new(
                WriterOptions::new(ArchiveVersion::Rar50, FeatureSet::store_only())
                    .with_compression_level(0),
            )
            .entries(entries)
            .finish()
            .unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        ArchiveKind::Zip => {
            let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
            for (name, bytes) in [
                ("first/disc.cue", cue.as_slice()),
                ("first/disc.bin", first.as_slice()),
                ("second/disc.cue", cue.as_slice()),
                ("second/disc.bin", second.as_slice()),
            ] {
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(bytes).unwrap();
            }
            writer.finish().unwrap();
        }
    }
}

fn write_7z(path: &std::path::Path, cue: &[u8], disc: Vec<u8>) {
    let mut writer = ArchiveWriter::create(path).unwrap();
    writer.set_content_methods(vec![EncoderConfiguration::new(EncoderMethod::COPY)]);
    writer
        .push_archive_entry(
            ArchiveEntry::new_file("set/disc.cue"),
            Some(Cursor::new(cue.to_vec())),
        )
        .unwrap();
    writer
        .push_archive_entry(
            ArchiveEntry::new_file("set/disc.bin"),
            Some(Cursor::new(disc)),
        )
        .unwrap();
    writer.finish().unwrap();
}

fn write_rar(path: &std::path::Path, cue: &[u8], disc: Vec<u8>) {
    let entries = [
        ("set/disc.cue".as_bytes().to_vec(), cue.to_vec()),
        ("set/disc.bin".as_bytes().to_vec(), disc),
    ]
    .into_iter()
    .map(|(name, data)| {
        RarArchiveEntry::new(
            name,
            EntrySource::from_bytes(std::sync::Arc::<[u8]>::from(data)),
        )
    })
    .collect::<Vec<_>>();
    let bytes = Rar50Writer::new(
        WriterOptions::new(ArchiveVersion::Rar50, FeatureSet::store_only())
            .with_compression_level(0),
    )
    .entries(entries)
    .finish()
    .unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn write_zip(path: &std::path::Path, cue: &[u8], disc: Vec<u8>) {
    let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("set/disc.cue", options).unwrap();
    writer.write_all(cue).unwrap();
    writer.start_file("set/disc.bin", options).unwrap();
    writer.write_all(&disc).unwrap();
    writer.finish().unwrap();
}
