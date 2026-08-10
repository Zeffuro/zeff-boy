pub(super) fn gba_rom_info(emu: &zeff_gba_core::emulator::Emulator) -> crate::debug::RomDebugInfo {
    crate::debug::RomDebugInfo {
        sections: vec![
            crate::debug::RomInfoSection {
                heading: "GBA Header",
                fields: vec![
                    ("Title", emu.cartridge_header().title.clone()),
                    ("Game Code", emu.cartridge_header().game_code.clone()),
                    ("Maker Code", emu.cartridge_header().maker_code.clone()),
                    ("Backup", format!("{:?}", emu.backup_kind())),
                ],
            },
            crate::debug::RomInfoSection {
                heading: "Core",
                fields: vec![(
                    "Status",
                    "Experimental ARM/Thumb interpreter; bitmap video modes enabled".into(),
                )],
            },
        ],
    }
}
