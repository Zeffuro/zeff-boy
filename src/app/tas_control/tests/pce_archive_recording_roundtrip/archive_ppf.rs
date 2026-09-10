use super::*;

#[test]
fn archive_ppf_all_six_routes_link_record_and_restore_with_selected_zip_keep() {
    let mut fill = 0x81;
    for archive in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip] {
        for selected in [false, true] {
            run_live_roundtrip(archive, selected, fill, false);
            fill = fill.wrapping_add(1);
        }
    }
    run_live_roundtrip(ArchiveKind::Zip, true, 0xA7, true);
}

#[test]
fn selected_archive_ppf_card_multitap_records_fifth_player_and_keeps_or_restores() {
    for memory_base in [false, true] {
        for keep in [false, true] {
            run_live_profile_roundtrip(ArchiveKind::Zip, true, 0x69, keep, Some(memory_base));
        }
    }
}

#[test]
fn selected_zip_archive_ppf_repair_reloads_exact_member_and_restores() {
    let root = crate::test_support::test_directory("tas-pce-cd-zip-selected-ppf-repair").unwrap();
    let archive_path = root.path().join("disc.zip");
    write_archive_ppf(&archive_path, ArchiveKind::Zip, true, 0xB7, None);
    let rom_path = archive_path.join("second").join("disc.cue");
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let firmware_sha256 = zeff_firmware::sha256_bytes(system_card);
    let loader = DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
        archive_path.clone(),
        rom_path.clone(),
        system_card,
        firmware_sha256,
    )
    .unwrap();
    let _firmware =
        crate::emu_backend::loader::register_test_pce_cd_system_card(firmware_sha256, system_card);
    let project = loader.create_project().unwrap();
    let mut original = loader.load_editor_engine(&project).unwrap().into_backend();
    original.set_input(1, 2);
    original.step_frame();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(original, false);
    let generation = 171;
    let mut app = app_with_worker(worker, generation, ActiveSystem::Pce, rom_path.clone());
    app.rom_info.source_path = Some(archive_path);
    app.rom_info.rom_path = Some(rom_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    assert_eq!(
        live_ok(&mut app, LiveCommand::TasReloadGame)["repair_activated"],
        true
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision { .. }
    ) && Instant::now() < deadline
    {
        app.drain_emu_responses();
        app.begin_queued_tas_control_acquire();
        let _ = live_ok(&mut app, LiveCommand::TasStatus);
        std::thread::yield_now();
    }
    assert!(matches!(
        app.tas_control.state,
        TasControlState::AwaitingDecision { .. }
    ));
    live_ok(&mut app, LiveCommand::TasDisconnect { keep: false });
    let deadline = Instant::now() + Duration::from_secs(5);
    while (app.tas_control.state != TasControlState::Detached
        || app.tas_repair_state() != crate::app::tas_control::repair::TasRepairState::Detached)
        && Instant::now() < deadline
    {
        app.drain_emu_responses();
        app.pump_tas_repair_resolution();
        std::thread::yield_now();
    }
    assert_eq!(app.tas_control.state, TasControlState::Detached);
    assert_eq!(
        app.tas_repair_state(),
        crate::app::tas_control::repair::TasRepairState::Detached
    );
    assert_eq!(app.emu_worker_generation, generation + 2);
}

fn run_live_roundtrip(archive: ArchiveKind, selected: bool, fill: u8, keep: bool) {
    run_live_profile_roundtrip(archive, selected, fill, keep, None);
}

fn run_live_profile_roundtrip(
    archive: ArchiveKind,
    selected: bool,
    fill: u8,
    keep: bool,
    memory_base: Option<bool>,
) {
    let name = format!("tas-pce-cd-{archive:?}-ppf-{selected}-{keep}-{memory_base:?}")
        .to_ascii_lowercase();
    let root = crate::test_support::test_directory(&name).unwrap();
    let archive_path = root.path().join(match archive {
        ArchiveKind::SevenZip => "disc.7z",
        ArchiveKind::Rar => "disc.rar",
        ArchiveKind::Zip => "disc.zip",
    });
    write_archive_ppf(
        &archive_path,
        archive,
        selected,
        fill,
        memory_base.map(|_| name.as_str()),
    );
    let cue_directory = if selected { "second" } else { "set" };
    let rom_path = archive_path.join(cue_directory).join("disc.cue");
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let firmware_sha256 = zeff_firmware::sha256_bytes(system_card);
    let loader = if selected {
        DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
            archive_path.clone(),
            rom_path.clone(),
            system_card,
            firmware_sha256,
        )
        .unwrap()
    } else {
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            archive_path.clone(),
            system_card,
            firmware_sha256,
        )
    };
    let source_hash = loader
        .load_fresh_backend()
        .unwrap()
        .pce()
        .unwrap()
        .tas_load_provenance()
        .unwrap()
        .load
        .source_disc_sha256
        .unwrap();
    let _arcade_catalog = (memory_base == Some(false)).then(|| {
        crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(source_hash)
    });
    let _memory_catalog = (memory_base == Some(true)).then(|| {
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(source_hash)
    });
    let _controller_catalog = memory_base.map(|_| {
        crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            source_hash,
            zeff_pce_core::hardware::PceControllerMode::Multitap,
        )
    });
    let loader = if memory_base.is_some() {
        DirectPceCdTasExecutionLoader::new_multitap_with_rom_path_and_system_card_override(
            archive_path.clone(),
            rom_path.clone(),
            system_card,
            firmware_sha256,
        )
        .unwrap()
    } else {
        loader
    };
    let project = loader.create_project().unwrap();
    assert_eq!(project.identity().patches.len(), 1);
    assert!(if memory_base.is_some() {
        crate::emu_backend::loader::is_direct_pce_multitap_cd_archive_ppf_tas_sync_config_sha256(
            project.identity().sync_config_sha256,
        )
    } else {
        crate::emu_backend::loader::is_direct_pce_cd_archive_ppf_tas_sync_config_sha256(
            project.identity().sync_config_sha256,
        )
    });
    let engine = loader.load_editor_engine(&project).unwrap();
    assert!(
        engine
            .backend()
            .pce()
            .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
            .is_some_and(|provenance| provenance.load.direct_pce_cd_archive_ppf)
    );
    let backend = engine.into_backend();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let generation = 181 + u64::from(fill);
    let mut app = app_with_worker(worker, generation, ActiveSystem::Pce, rom_path.clone());
    app.rom_info.source_path = Some(archive_path);
    app.rom_info.rom_path = Some(rom_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(generation, snapshot, TasControlStartMode::Preview)
        .unwrap();
    wait_for_linked(&mut app);
    live_ok(
        &mut app,
        LiveCommand::Button {
            player: if memory_base.is_some() { 5 } else { 1 },
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
            .players[if memory_base.is_some() { 4 } else { 0 }]
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

fn write_archive_ppf(
    path: &std::path::Path,
    archive: ArchiveKind,
    selected: bool,
    fill: u8,
    tag: Option<&str>,
) {
    let target = if selected { "second" } else { "set" };
    let mut entries = Vec::new();
    if selected {
        entries.extend(cue_entries("first", fill ^ 0xFF));
    }
    entries.extend(cue_entries(target, fill));
    if let Some(tag) = tag {
        let disc = &mut entries
            .iter_mut()
            .find(|(name, _)| name == &format!("{target}/disc.bin"))
            .unwrap()
            .1;
        disc[128..128 + tag.len()].copy_from_slice(tag.as_bytes());
    }
    entries.push((
        format!("{target}/disc.ppf/0001.ppf"),
        ppf1(0, &[fill.rotate_left(1)]),
    ));
    match archive {
        ArchiveKind::SevenZip => {
            let mut writer = ArchiveWriter::create(path).unwrap();
            writer.set_content_methods(vec![EncoderConfiguration::new(EncoderMethod::COPY)]);
            for (name, bytes) in entries {
                writer
                    .push_archive_entry(ArchiveEntry::new_file(&name), Some(Cursor::new(bytes)))
                    .unwrap();
            }
            writer.finish().unwrap();
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
            .finish()
            .unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        ArchiveKind::Zip => {
            let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
            for (name, bytes) in entries {
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(&bytes).unwrap();
            }
            writer.finish().unwrap();
        }
    }
}

fn cue_entries(directory: &str, fill: u8) -> Vec<(String, Vec<u8>)> {
    vec![
        (
            format!("{directory}/disc.cue"),
            b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n".to_vec(),
        ),
        (
            format!("{directory}/disc.bin"),
            vec![fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
        ),
    ]
}

fn ppf1(offset: u32, bytes: &[u8]) -> Vec<u8> {
    let mut patch = b"PPF10\0".to_vec();
    patch.resize(56, 0);
    patch.extend_from_slice(&offset.to_le_bytes());
    patch.push(bytes.len() as u8);
    patch.extend_from_slice(bytes);
    patch
}
