use super::*;

impl PceBackend {
    pub(crate) fn new_cdrom2(
        system_card_rom: Vec<u8>,
        disc: CdDisc,
        config: PceCdBackendConfig,
    ) -> anyhow::Result<Self> {
        Self::new_cdrom2_with_host_persistence(system_card_rom, disc, config, true)
    }

    pub(crate) fn new_cdrom2_without_host_persistence(
        system_card_rom: Vec<u8>,
        disc: CdDisc,
        config: PceCdBackendConfig,
    ) -> anyhow::Result<Self> {
        Self::new_cdrom2_with_host_persistence(system_card_rom, disc, config, false)
    }

    pub(super) fn new_cdrom2_with_host_persistence(
        system_card_rom: Vec<u8>,
        disc: CdDisc,
        config: PceCdBackendConfig,
        host_persistence_enabled: bool,
    ) -> anyhow::Result<Self> {
        let recovery_identity = disc.content_hash();
        let arcade_card_enabled = match config.arcade_card_mode {
            PceArcadeCardMode::Automatic => {
                automatic_arcade_card_enabled(Some(config.source_disc_hash))
            }
            PceArcadeCardMode::Enabled => true,
            PceArcadeCardMode::Disabled => false,
        };
        anyhow::ensure!(
            !arcade_card_enabled || config.system_card_board == PceHuCardBoard::SystemCardV3,
            "Arcade Card requires a System Card v3 CD environment"
        );
        let machine = PceMachine::with_cdrom2_system_card_controller_and_arcade_card(
            system_card_rom,
            config.system_card_board,
            disc,
            config.console_wiring,
            ControllerPort::two_button(),
            arcade_card_enabled,
        )?;
        let paths = BackendPaths::with_source_path(config.cue_path, config.source_path);
        let mut sram_recovery = if host_persistence_enabled {
            crate::save_paths::battery_sram_session(paths.rom_path(), "pce", recovery_identity)
        } else {
            Default::default()
        };
        if host_persistence_enabled {
            sram_recovery.begin(
                &memory_base128_path(),
                "pce",
                recovery_identity,
                "memory-base-128",
            );
        }
        let mut backend = Self {
            machine,
            paths,
            rom_hash: config.content_hash,
            source_crc32: Some(config.content_crc32),
            source_disc_hash: Some(config.source_disc_hash),
            frame_output: Default::default(),
            frame_count: 0,
            pending_runtime_fault: None,
            overscan_mode: PceOverscanMode::default(),
            palette_mode: PcePaletteMode::default(),
            pce_controller_mode: PceControllerMode::Automatic,
            pce_memory_base_mode: PceMemoryBaseMode::Automatic,
            pce_arcade_card_mode: if arcade_card_enabled {
                PceArcadeCardMode::Enabled
            } else {
                PceArcadeCardMode::Disabled
            },
            mouse_host_buttons: PadButtons::empty(),
            sram_recovery,
            memory_base_force_flush: false,
            host_persistence_enabled,
            tas_load_provenance: None,
        };
        backend.invalidate_frame_output();
        backend.update_controller_mode(PceControllerMode::Automatic);
        backend.update_memory_base_mode(PceMemoryBaseMode::Automatic);
        Ok(backend)
    }

    #[cfg(test)]
    pub(crate) fn new(hucard_rom: Vec<u8>, rom_path: PathBuf) -> anyhow::Result<Self> {
        Self::with_paths(hucard_rom, BackendPaths::new(rom_path), None, None, None)
    }

    #[cfg(test)]
    pub(crate) fn new_with_console_wiring(
        hucard_rom: Vec<u8>,
        rom_path: PathBuf,
        console_wiring: PceConsoleWiring,
    ) -> anyhow::Result<Self> {
        Self::with_paths(
            hucard_rom,
            BackendPaths::new(rom_path),
            Some(console_wiring),
            None,
            None,
        )
    }

    pub(crate) fn new_with_overrides(
        hucard_rom: Vec<u8>,
        rom_path: PathBuf,
        console_wiring: Option<PceConsoleWiring>,
        hucard_board: Option<PceHuCardBoard>,
        cartridge_hardware: Option<zeff_pce_core::hardware::PceCartridgeHardware>,
    ) -> anyhow::Result<Self> {
        Self::with_paths(
            hucard_rom,
            BackendPaths::new(rom_path),
            console_wiring,
            hucard_board,
            cartridge_hardware,
        )
    }

    pub(crate) fn with_source_path_and_overrides(
        hucard_rom: Vec<u8>,
        rom_path: PathBuf,
        source_path: PathBuf,
        console_wiring: Option<PceConsoleWiring>,
        hucard_board: Option<PceHuCardBoard>,
        cartridge_hardware: Option<zeff_pce_core::hardware::PceCartridgeHardware>,
    ) -> anyhow::Result<Self> {
        Self::with_paths(
            hucard_rom,
            BackendPaths::with_source_path(rom_path, source_path),
            console_wiring,
            hucard_board,
            cartridge_hardware,
        )
    }

    pub(super) fn with_paths(
        hucard_rom: Vec<u8>,
        paths: BackendPaths,
        console_wiring: Option<PceConsoleWiring>,
        hucard_board: Option<PceHuCardBoard>,
        cartridge_hardware: Option<zeff_pce_core::hardware::PceCartridgeHardware>,
    ) -> anyhow::Result<Self> {
        Self::with_paths_and_persistence(
            hucard_rom,
            paths,
            console_wiring,
            hucard_board,
            cartridge_hardware,
            true,
        )
    }

    pub(super) fn with_paths_and_persistence(
        hucard_rom: Vec<u8>,
        paths: BackendPaths,
        console_wiring: Option<PceConsoleWiring>,
        hucard_board: Option<PceHuCardBoard>,
        cartridge_hardware: Option<zeff_pce_core::hardware::PceCartridgeHardware>,
        host_persistence_enabled: bool,
    ) -> anyhow::Result<Self> {
        let hucard_rom = normalize_hucard_image(hucard_rom)?;
        anyhow::ensure!(!hucard_rom.is_empty(), "PC Engine HuCard image is empty");
        anyhow::ensure!(
            hucard_rom.len().is_multiple_of(HUCARD_BANK_LEN),
            "PC Engine HuCard image length must be a multiple of {HUCARD_BANK_LEN} bytes"
        );
        let rom_hash = zeff_firmware::sha256_bytes(&hucard_rom);
        Self::with_validated_paths_and_hash(
            hucard_rom,
            paths,
            PceHuCardOverrides {
                console_wiring,
                hucard_board,
                cartridge_hardware,
            },
            rom_hash,
            host_persistence_enabled,
        )
    }

    pub(super) fn with_validated_paths_and_hash(
        hucard_rom: Vec<u8>,
        paths: BackendPaths,
        overrides: PceHuCardOverrides,
        rom_hash: [u8; 32],
        host_persistence_enabled: bool,
    ) -> anyhow::Result<Self> {
        let mut cartridge = PceCartridgeDescriptor::from_sha256(rom_hash);
        if let Some(console_wiring) = overrides.console_wiring {
            cartridge = cartridge.with_console_wiring(console_wiring);
        }
        if let Some(hucard_board) = overrides.hucard_board {
            cartridge = cartridge.with_hucard_board(hucard_board);
        }
        if let Some(cartridge_hardware) = overrides.cartridge_hardware {
            cartridge = cartridge.with_required_hardware(cartridge_hardware);
        }
        let machine = PceMachine::with_cartridge_and_controller(
            hucard_rom,
            cartridge,
            ControllerPort::two_button(),
        )?;
        let mut sram_recovery = if host_persistence_enabled {
            crate::save_paths::battery_sram_session(paths.rom_path(), "pce", rom_hash)
        } else {
            Default::default()
        };
        if host_persistence_enabled {
            sram_recovery.begin(&memory_base128_path(), "pce", rom_hash, "memory-base-128");
        }
        let mut backend = Self {
            machine,
            paths,
            rom_hash,
            source_crc32: None,
            source_disc_hash: None,
            frame_output: Default::default(),
            frame_count: 0,
            pending_runtime_fault: None,
            overscan_mode: PceOverscanMode::default(),
            palette_mode: PcePaletteMode::default(),
            pce_controller_mode: PceControllerMode::Automatic,
            pce_memory_base_mode: PceMemoryBaseMode::Automatic,
            pce_arcade_card_mode: PceArcadeCardMode::Disabled,
            mouse_host_buttons: PadButtons::empty(),
            sram_recovery,
            memory_base_force_flush: false,
            host_persistence_enabled,
            tas_load_provenance: None,
        };
        backend.invalidate_frame_output();
        backend.update_controller_mode(PceControllerMode::Automatic);
        backend.update_memory_base_mode(PceMemoryBaseMode::Automatic);
        Ok(backend)
    }
}
