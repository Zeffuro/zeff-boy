use super::*;

#[test]
fn archive_multitap_six_routes_link_record_and_restore_with_representative_keep() {
    for (index, archive) in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip]
        .into_iter()
        .enumerate()
    {
        run_archive_multitap(archive, false, 0xA1 + index as u8, false);
        run_archive_multitap(
            archive,
            true,
            0xB1 + index as u8,
            matches!(archive, ArchiveKind::Zip),
        );
    }
}

fn run_archive_multitap(archive: ArchiveKind, selected: bool, fill: u8, keep: bool) {
    let name = format!("tas-pce-cd-{archive:?}-multitap-{selected}").to_ascii_lowercase();
    let root = crate::test_support::test_directory(&name).unwrap();
    let archive_path = root.path().join(match archive {
        ArchiveKind::SevenZip => "disc.7z",
        ArchiveKind::Rar => "disc.rar",
        ArchiveKind::Zip => "disc.zip",
    });
    let rom_path = if selected {
        write_multicue_archive(&archive_path, archive, fill);
        Some(archive_path.join("second").join("disc.cue"))
    } else {
        let mut disc = vec![fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
        disc[0..4].copy_from_slice(&[0x41, 0x4D, fill, fill.rotate_left(1)]);
        write_archive_bytes(&archive_path, archive, disc);
        Some(archive_path.join("set").join("disc.cue"))
    };
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let system_card_sha256 = zeff_firmware::sha256_bytes(system_card);
    let base = if selected {
        DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
            archive_path.clone(),
            rom_path.clone().expect("archive member"),
            system_card,
            system_card_sha256,
        )
        .unwrap()
    } else {
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            archive_path.clone(),
            system_card,
            system_card_sha256,
        )
    };
    let disc_sha256 = base
        .load_fresh_backend()
        .unwrap()
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture disc");
    let _catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        disc_sha256,
        zeff_pce_core::hardware::PceControllerMode::Multitap,
    );
    let loader = if selected {
        DirectPceCdTasExecutionLoader::new_multitap_with_rom_path_and_system_card_override(
            archive_path.clone(),
            rom_path.clone().expect("archive member"),
            system_card,
            system_card_sha256,
        )
        .unwrap()
    } else {
        DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
            archive_path.clone(),
            system_card,
            system_card_sha256,
        )
    };
    let project = loader.create_project().unwrap();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(backend, false);
    let loaded_path = rom_path.clone().unwrap_or_else(|| archive_path.clone());
    let mut app = app_with_worker(worker, 121, ActiveSystem::Pce, loaded_path.clone());
    app.rom_info.source_path = Some(archive_path);
    app.rom_info.rom_path = Some(loaded_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let snapshot =
        TasEditorControlSnapshot::capture(app.debug_windows.tas_editor.active_session().unwrap())
            .unwrap();
    app.tas_control
        .queue_acquire(121, snapshot, TasControlStartMode::Preview)
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

#[test]
fn selected_zip_archive_multitap_repair_reloads_exact_member_and_restores() {
    let root =
        crate::test_support::test_directory("tas-pce-cd-zip-selected-multitap-repair").unwrap();
    let archive_path = root.path().join("disc.zip");
    write_multicue_archive(&archive_path, ArchiveKind::Zip, 0xC7);
    let rom_path = archive_path.join("second").join("disc.cue");
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let firmware_sha256 = zeff_firmware::sha256_bytes(system_card);
    let base = DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
        archive_path.clone(),
        rom_path.clone(),
        system_card,
        firmware_sha256,
    )
    .unwrap();
    let disc_sha256 = base
        .load_fresh_backend()
        .unwrap()
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture disc");
    let _catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        disc_sha256,
        zeff_pce_core::hardware::PceControllerMode::Multitap,
    );
    let _firmware =
        crate::emu_backend::loader::register_test_pce_cd_system_card(firmware_sha256, system_card);
    let loader =
        DirectPceCdTasExecutionLoader::new_multitap_with_rom_path_and_system_card_override(
            archive_path.clone(),
            rom_path.clone(),
            system_card,
            firmware_sha256,
        )
        .unwrap();
    let project = loader.create_project().unwrap();
    let mut original = loader.load_editor_engine(&project).unwrap().into_backend();
    original.set_input_p5(1, 2);
    original.step_frame();
    let manual_path = root.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default()).unwrap();
    let seek_cache = TasSeekStateCache::open(root.path().join("seek-cache")).unwrap();
    let session = TasEditorSession::new(project, manual_path, autosaves, seek_cache).unwrap();
    let worker = EmuThread::spawn(original, false);
    let mut app = app_with_worker(worker, 131, ActiveSystem::Pce, rom_path.clone());
    app.rom_info.source_path = Some(archive_path);
    app.rom_info.rom_path = Some(rom_path);
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);

    let reply = live_ok(&mut app, LiveCommand::TasReloadGame);
    assert_eq!(reply["repair_activated"], true);
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
    assert_eq!(app.emu_worker_generation, 133);
}
