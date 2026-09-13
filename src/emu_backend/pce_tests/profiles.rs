use super::*;

#[test]
fn explicit_console_wiring_overrides_auto_detection() {
    let backend = PceBackend::new_with_console_wiring(
        rom_with_program(&[0xEA]),
        PathBuf::from("override.pce"),
        PceConsoleWiring::TurboGrafx16,
    )
    .unwrap();

    assert_eq!(
        backend.machine.devices().console_wiring(),
        PceConsoleWiring::TurboGrafx16
    );
}

#[test]
fn curated_wiring_hash_propagates_for_direct_and_archive_paths() {
    let alternate_wiring_sha256 = [
        0xD2, 0xFE, 0x59, 0xCF, 0x24, 0x05, 0x3B, 0xBB, 0xB1, 0xB5, 0xDA, 0x25, 0x21, 0xA9, 0x58,
        0xE3, 0x8A, 0x98, 0x1C, 0xCE, 0x9B, 0xAB, 0xA0, 0xAF, 0x82, 0x5E, 0xA5, 0x18, 0x9D, 0x84,
        0x08, 0xDC,
    ];
    let direct_wiring_sha256 = [
        0x60, 0xC6, 0x9E, 0xE6, 0x80, 0x6A, 0xA6, 0x14, 0x45, 0x95, 0x49, 0x63, 0x3B, 0xDC, 0x72,
        0x8E, 0x10, 0x5F, 0x85, 0x92, 0xE5, 0x35, 0xFC, 0xC7, 0x96, 0xC2, 0x4C, 0xD6, 0xD7, 0x6A,
        0x6B, 0xD1,
    ];
    let archived_wiring_sha256 = [
        0xC5, 0xA3, 0x9C, 0x9D, 0x9B, 0x2D, 0x75, 0x32, 0x44, 0x81, 0x6E, 0xAF, 0xD6, 0x8F, 0x50,
        0x4A, 0x85, 0x59, 0x08, 0xEE, 0xBA, 0xB1, 0xB1, 0xC8, 0xFE, 0xA2, 0xBB, 0xF7, 0xA4, 0xA8,
        0x13, 0xC7,
    ];
    let direct = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::new(PathBuf::from("wiring-profile.pce")),
        PceHuCardOverrides::default(),
        archived_wiring_sha256,
        true,
    )
    .unwrap();
    let archive = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::with_source_path(
            PathBuf::from("cards.zip").join("wiring-profile.pce"),
            PathBuf::from("cards.zip"),
        ),
        PceHuCardOverrides::default(),
        archived_wiring_sha256,
        true,
    )
    .unwrap();
    let explicit_pce = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::new(PathBuf::from("wiring-profile.pce")),
        PceHuCardOverrides {
            console_wiring: Some(PceConsoleWiring::PcEngine),
            ..Default::default()
        },
        archived_wiring_sha256,
        true,
    )
    .unwrap();
    let direct_wiring_profile = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::new(PathBuf::from("direct-wiring.pce")),
        PceHuCardOverrides::default(),
        direct_wiring_sha256,
        true,
    )
    .unwrap();
    let alternate_archive = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::with_source_path(
            PathBuf::from("cards.7z").join("alternate-wiring.pce"),
            PathBuf::from("cards.7z"),
        ),
        PceHuCardOverrides::default(),
        alternate_wiring_sha256,
        true,
    )
    .unwrap();

    assert_eq!(
        direct.machine.devices().console_wiring(),
        PceConsoleWiring::TurboGrafx16
    );
    assert_eq!(
        direct_wiring_profile.machine.devices().console_wiring(),
        PceConsoleWiring::TurboGrafx16
    );
    assert_eq!(
        alternate_archive.machine.devices().console_wiring(),
        PceConsoleWiring::TurboGrafx16
    );
    assert_eq!(
        archive.machine.devices().console_wiring(),
        PceConsoleWiring::TurboGrafx16
    );
    assert_eq!(archive.source_path(), Path::new("cards.zip"));
    assert_eq!(
        explicit_pce.machine.devices().console_wiring(),
        PceConsoleWiring::PcEngine
    );
}

#[test]
fn supergrafx_direct_and_archive_profiles_expose_dynamic_work_ram() {
    let supergrafx_sha256 = [
        0x9B, 0x57, 0xCD, 0xF0, 0xD0, 0xB1, 0x10, 0xF4, 0x12, 0x8B, 0x86, 0x34, 0x19, 0xD5, 0xBE,
        0x99, 0xA3, 0x70, 0x8B, 0xFB, 0x11, 0xCF, 0xBE, 0x16, 0x96, 0xF2, 0x54, 0x49, 0xB9, 0x91,
        0x02, 0x6D,
    ];
    let direct = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::new(PathBuf::from("supergrafx-profile.pce")),
        PceHuCardOverrides::default(),
        supergrafx_sha256,
        true,
    )
    .unwrap();
    let mut archive = PceBackend::with_validated_paths_and_hash(
        rom_with_program(&[0xEA]),
        BackendPaths::with_source_path(
            PathBuf::from("supergrafx-profile.pce"),
            PathBuf::from("cards.zip"),
        ),
        PceHuCardOverrides::default(),
        supergrafx_sha256,
        true,
    )
    .unwrap();

    for backend in [&direct, &archive] {
        assert_eq!(
            backend.machine.hardware_topology(),
            zeff_pce_core::hardware::PceHardwareTopology::SuperGrafx
        );
        assert_eq!(
            backend.machine.devices().psg().revision(),
            zeff_pce_core::hardware::PsgRevision::HuC6280A
        );
        assert_eq!(
            backend.system_ram_len(),
            zeff_pce_core::hardware::SUPERGRAFX_WORK_RAM_LEN
        );
        assert_eq!(
            backend
                .memory_regions()
                .iter()
                .find(|region| region.kind == MemoryRegionKind::SystemRam)
                .and_then(|region| region.size),
            Some(zeff_pce_core::hardware::SUPERGRAFX_WORK_RAM_LEN)
        );
    }
    assert_eq!(archive.source_path(), Path::new("cards.zip"));
    let mut ram = Vec::new();
    archive.copy_memory_region("system_ram", &mut ram).unwrap();
    assert_eq!(ram.len(), zeff_pce_core::hardware::SUPERGRAFX_WORK_RAM_LEN);
}
