use super::*;

#[test]
fn pce_backend_exposes_bounded_frontend_and_debugger_capabilities() {
    let backend = build_pce_backend();
    let features = backend.capabilities();

    assert_eq!(backend.system(), ActiveSystem::Pce);
    assert_eq!(
        backend.core_family(),
        zeff_emu_common::system::CoreFamily::PcEngine
    );
    assert_eq!(
        backend.framebuffer().len(),
        ActiveSystem::Pce.framebuffer_len()
    );
    assert_eq!(
        features.system_ram_len,
        zeff_pce_core::hardware::WORK_RAM_LEN
    );
    assert_eq!(
        features.video_ram_len,
        zeff_pce_core::hardware::VDC_VRAM_BYTES
    );
    assert_eq!(
        features.memory_regions,
        vec![
            MemoryRegionDescriptor::cpu_address_space(16),
            MemoryRegionDescriptor::system_ram(zeff_pce_core::hardware::WORK_RAM_LEN),
            MemoryRegionDescriptor::video_ram(zeff_pce_core::hardware::VDC_VRAM_BYTES),
            MemoryRegionDescriptor::palette_ram(zeff_pce_core::hardware::VCE_PALETTE_COLORS * 2,),
            MemoryRegionDescriptor::oam(zeff_pce_core::hardware::VDC_SATB_WORDS * 2),
            MemoryRegionDescriptor::framebuffer(ActiveSystem::Pce.framebuffer_len()),
        ]
    );
    assert_eq!(
        features.input_features,
        crate::emu_backend::InputCapabilities::for_system(ActiveSystem::Pce)
    );
    assert_eq!(features.input_features.max_players, 5);
    assert!(features.supports_save_states);
    assert!(features.supports_state_capture);
    assert!(features.supports_rewind);
    assert!(features.supports_replay);
    assert!(features.supports_audio);
    assert!(features.supports_cheats);
    assert!(features.supports_guest_calls);
    assert!(features.supports_debugger);
    assert!(features.supports_execution_controls);
    assert!(features.supports_opcode_history);
    assert!(features.cheat_features.supports_user_cheats);
    assert!(features.cheat_features.supports_ram_writes);
    assert!(!features.cheat_features.supports_rom_patches);
    assert!(matches!(backend, EmuBackend::Pce(_)));
}

#[test]
fn pce_loader_preserves_direct_and_archive_paths() {
    let rom = build_pce_test_rom();
    let direct = load_backend_from_rom_source(
        ActiveSystem::Pce,
        &PathBuf::from("direct.pce"),
        &PathBuf::from("direct.pce"),
        Some(rom.clone()),
        BackendLoadConfig::default(),
    )
    .unwrap();
    assert_eq!(direct.backend.rom_path(), PathBuf::from("direct.pce"));
    assert_eq!(direct.backend.source_path(), PathBuf::from("direct.pce"));

    let archive = PathBuf::from("collection.zip");
    let virtual_path = archive.join("folder/game.pce");
    let archived = load_backend_from_rom_source(
        ActiveSystem::Pce,
        &archive,
        &virtual_path,
        Some(rom),
        BackendLoadConfig::default(),
    )
    .unwrap();
    assert_eq!(archived.backend.rom_path(), virtual_path);
    assert_eq!(archived.backend.source_path(), archive);
}

#[test]
fn pce_loader_classifies_structural_sf2_identically_for_direct_and_archive_sources() {
    let mut rom = build_pce_test_rom();
    rom.resize(zeff_pce_core::hardware::SF2_CE_HUCARD_IMAGE_LEN, 0xEA);
    for (source_path, rom_path) in [
        (PathBuf::from("sf2.pce"), PathBuf::from("sf2.pce")),
        (
            PathBuf::from("sf2.zip"),
            PathBuf::from("sf2.zip").join("Street Fighter II.pce"),
        ),
    ] {
        let loaded = load_backend_from_rom_source(
            ActiveSystem::Pce,
            &source_path,
            &rom_path,
            Some(rom.clone()),
            BackendLoadConfig::default(),
        )
        .unwrap();
        let EmuBackend::Pce(backend) = loaded.backend else {
            panic!("PCE loader returned a different backend");
        };
        assert_eq!(
            backend.hucard_board(),
            zeff_pce_core::hardware::PceHuCardBoard::Sf2Ce
        );
        assert_eq!(backend.hucard_rom().len(), 0x28_0000);
    }
}

#[test]
fn pce_loader_applies_board_override_identically_for_direct_and_archive_sources() {
    let mut rom = build_pce_test_rom();
    rom.resize(zeff_pce_core::hardware::POPULOUS_HUCARD_IMAGE_LEN, 0xEA);
    for (source_path, rom_path) in [
        (
            PathBuf::from("synthetic.pce"),
            PathBuf::from("synthetic.pce"),
        ),
        (
            PathBuf::from("synthetic.zip"),
            PathBuf::from("synthetic.zip").join("game.pce"),
        ),
    ] {
        let plain = load_backend_from_rom_source(
            ActiveSystem::Pce,
            &source_path,
            &rom_path,
            Some(rom.clone()),
            BackendLoadConfig::default(),
        )
        .unwrap();
        assert_eq!(plain.backend.save_ram_kind(), SaveRamKind::none());
        let EmuBackend::Pce(plain) = plain.backend else {
            panic!("PCE loader returned a different backend");
        };
        assert_eq!(
            plain.hucard_board(),
            zeff_pce_core::hardware::PceHuCardBoard::Plain
        );

        let mut populous = load_backend_from_rom_source(
            ActiveSystem::Pce,
            &source_path,
            &rom_path,
            Some(rom.clone()),
            BackendLoadConfig {
                pce_hucard_board: Some(zeff_pce_core::hardware::PceHuCardBoard::Populous),
                ..BackendLoadConfig::default()
            },
        )
        .unwrap();
        assert_eq!(
            populous.backend.save_ram_kind(),
            SaveRamKind::mapper_ram_unknown(zeff_pce_core::hardware::POPULOUS_HUCARD_RAM_LEN)
        );
        assert!(!populous.backend.save_ram_kind().is_battery_backed());
        let mut ram = Vec::new();
        assert_eq!(
            populous
                .backend
                .copy_memory_region("save_ram", &mut ram)
                .unwrap(),
            MemoryRegionDescriptor::save_ram(zeff_pce_core::hardware::POPULOUS_HUCARD_RAM_LEN)
        );
        assert_eq!(
            ram,
            vec![0; zeff_pce_core::hardware::POPULOUS_HUCARD_RAM_LEN]
        );
        let EmuBackend::Pce(populous) = populous.backend else {
            panic!("PCE loader returned a different backend");
        };
        assert_eq!(
            populous.hucard_board(),
            zeff_pce_core::hardware::PceHuCardBoard::Populous
        );
    }
}

fn build_pce_263_line_test_rom() -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    rom[..13].copy_from_slice(&[
        0xA9, 0xFF, 0x53, 0x01, 0xA9, 0x04, 0x8D, 0x00, 0x04, 0xD4, 0xEA, 0x80, 0xFD,
    ]);
    rom[0x1FFE..0x2000].copy_from_slice(&0xE000_u16.to_le_bytes());
    rom
}

#[test]
fn pce_loader_applies_requested_audio_rate_for_direct_and_archive_sources() {
    let rom = build_pce_263_line_test_rom();
    for (source_path, rom_path) in [
        (PathBuf::from("rate.pce"), PathBuf::from("rate.pce")),
        (
            PathBuf::from("rate.zip"),
            PathBuf::from("rate.zip").join("game.pce"),
        ),
    ] {
        let mut loaded = load_backend_from_rom_source(
            ActiveSystem::Pce,
            &source_path,
            &rom_path,
            Some(rom.clone()),
            BackendLoadConfig {
                sample_rate: Some(48_000),
                ..BackendLoadConfig::default()
            },
        )
        .unwrap();
        let mut frame_counts = Vec::with_capacity(120);
        let mut total_frames = 0;
        for _ in 0..120 {
            loaded.backend.step_frame();
            let mut samples = Vec::new();
            loaded.backend.drain_audio_samples_into(&mut samples);
            let frames = samples.len() / 2;
            frame_counts.push(frames);
            total_frames += frames;
        }

        assert_eq!(total_frames, 96_279);
        assert!(frame_counts.iter().all(|count| matches!(count, 802 | 803)));
    }
}

#[test]
fn pce_loader_rejects_invalid_plain_hucard_lengths() {
    for rom in [
        Vec::new(),
        vec![0; zeff_pce_core::hardware::HUCARD_ROM_REGION_LEN + 1],
        vec![0; zeff_pce_core::hardware::HUCARD_ROM_REGION_LEN + 0x2000],
    ] {
        assert!(
            load_backend_from_rom_source(
                ActiveSystem::Pce,
                &PathBuf::from("invalid.pce"),
                &PathBuf::from("invalid.pce"),
                Some(rom),
                BackendLoadConfig::default(),
            )
            .is_err()
        );
    }

    let header_shaped = vec![0; 0x2000 + 512];
    for (source_path, rom_path) in [
        (PathBuf::from("headered.pce"), PathBuf::from("headered.pce")),
        (
            PathBuf::from("headered.zip"),
            PathBuf::from("headered.zip").join("game.pce"),
        ),
    ] {
        let result = load_backend_from_rom_source(
            ActiveSystem::Pce,
            &source_path,
            &rom_path,
            Some(header_shaped.clone()),
            BackendLoadConfig::default(),
        );
        let Err(error) = result else {
            panic!("header-shaped HuCard image must be rejected");
        };
        assert!(error.to_string().contains("multiple of 8192 bytes"));
    }
}
