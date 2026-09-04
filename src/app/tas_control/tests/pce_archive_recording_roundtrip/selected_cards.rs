use super::*;

#[test]
fn selected_second_cue_card_profiles_link_record_restore_and_zip_keep() {
    let mut fill = 0xC1;
    for archive in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip] {
        for card in [CardKind::Arcade, CardKind::MemoryBase] {
            run_selected_second_cue_live_roundtrip(archive, card, fill, false);
            fill += 1;
        }
    }
    for (card, fill) in [(CardKind::Arcade, 0xD7), (CardKind::MemoryBase, 0xD8)] {
        run_selected_second_cue_live_roundtrip(ArchiveKind::Zip, card, fill, true);
    }
}

#[test]
fn selected_zip_card_profiles_repair_reload_exact_member_and_restore() {
    for (card, fill, generation) in [
        (CardKind::Arcade, 0xE7, 141),
        (CardKind::MemoryBase, 0xE8, 151),
    ] {
        run_selected_zip_card_repair(card, fill, generation);
    }
}

fn run_selected_zip_card_repair(card: CardKind, fill: u8, generation: u64) {
    let root =
        crate::test_support::test_directory(&format!("tas-pce-cd-zip-selected-{card:?}-repair"))
            .unwrap();
    let archive_path = root.path().join("disc.zip");
    write_multicue_archive(&archive_path, ArchiveKind::Zip, fill);
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
