use super::EmuBackend;
use crate::cheats::CheatPatch;

impl EmuBackend {
    pub(crate) fn install_rom_patches(&mut self, cheats: &[CheatPatch]) {
        match self {
            Self::Gb(gb) => {
                gb.emu.clear_rom_patches();
                for patch in cheats.iter().copied() {
                    if patch.is_rom_patch() {
                        gb.emu.add_rom_patch(patch);
                    }
                }
            }
            Self::Gba(_) => {}
            Self::Nes(nes) => {
                nes.emu.clear_game_genie();
                for patch in cheats.iter().copied() {
                    if let Some((address, value, compare)) = patch.constant_rom_write() {
                        nes.emu
                            .add_game_genie_patch(zeff_nes_core::cheats::NesGameGeniePatch {
                                address,
                                value,
                                compare,
                            });
                    }
                }
            }
            Self::Sega8(sega8) => {
                sega8.emu.clear_rom_patches();
                for patch in cheats.iter().copied() {
                    if patch.is_rom_patch() {
                        sega8.emu.add_rom_patch(patch);
                    }
                }
            }
            Self::Ws(_) => {}
        }
    }

    pub(crate) fn apply_ram_cheats(&mut self, cheats: &[CheatPatch]) {
        match self {
            Self::Gb(gb) => {
                zeff_emu_common::cheats::apply_ram_cheats_16(&mut gb.emu, cheats);
            }
            Self::Gba(gba) => {
                zeff_emu_common::cheats::apply_wide_ram_cheats(&mut gba.emu, cheats);
            }
            Self::Nes(nes) => {
                zeff_emu_common::cheats::apply_ram_cheats_16(&mut nes.emu, cheats);
            }
            Self::Sega8(sega8) => {
                zeff_emu_common::cheats::apply_ram_cheats_16(&mut sega8.emu, cheats);
            }
            Self::Ws(ws) => {
                zeff_emu_common::cheats::apply_wide_ram_cheats(&mut ws.emu, cheats);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EmuBackend;
    use crate::cheats::{CheatCode, CheatPatch, CheatType, CheatValue, collect_enabled_patches};
    use zeff_sega8_core::hardware::cartridge::SystemHint;

    fn gba_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0xC0];
        rom[0xA0..0xA4].copy_from_slice(b"TEST");
        rom[0xAC..0xB0].copy_from_slice(b"ABCD");
        rom[0xB0..0xB2].copy_from_slice(b"01");
        rom[0xB2] = 0x96;
        rom
    }

    fn ws_rom() -> Vec<u8> {
        let mut rom = vec![0xFF; 0x10000];
        rom[0..2].copy_from_slice(&[0x90, 0xF4]);
        let reset = rom.len() - 16;
        rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
        let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
        let footer = rom.len() - 10;
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        rom
    }

    fn nes_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;

        let prg = 16;
        rom[prg] = 0xEA;
        rom[prg + 0x3FFC] = 0x00;
        rom[prg + 0x3FFD] = 0x80;
        rom
    }

    fn gb_backend() -> EmuBackend {
        let emu = zeff_gb_core::emulator::Emulator::from_rom_data(
            &[0; 0x8000],
            zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::Auto,
        )
        .expect("GB emulator should initialize");
        EmuBackend::from_gb(emu, std::path::PathBuf::from("test.gb"))
    }

    fn gb_backend_from_rom(rom: &[u8]) -> EmuBackend {
        let emu = zeff_gb_core::emulator::Emulator::from_rom_data(
            rom,
            zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::Auto,
        )
        .expect("GB emulator should initialize");
        EmuBackend::from_gb(emu, std::path::PathBuf::from("test.gb"))
    }

    fn nes_backend() -> EmuBackend {
        let emu = zeff_nes_core::emulator::Emulator::new(&nes_rom(), 44_100.0)
            .expect("NES emulator should initialize");
        EmuBackend::from_nes(emu, std::path::PathBuf::from("test.nes"))
    }

    fn gba_backend() -> EmuBackend {
        let emu =
            zeff_gba_core::emulator::Emulator::new(&gba_rom(), 44_100).expect("GBA should init");
        EmuBackend::from_gba(emu, std::path::PathBuf::from("test.gba"))
    }

    fn ws_backend() -> EmuBackend {
        let emu = zeff_ws_core::emulator::Emulator::new(&ws_rom(), 44_100).expect("WS should init");
        EmuBackend::from_ws(emu, std::path::PathBuf::from("test.ws"))
    }

    fn sega8_backend() -> EmuBackend {
        let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
            &[0x76],
            zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE,
            SystemHint::MasterSystem,
        )
        .expect("Sega 8-bit emulator should initialize");
        EmuBackend::from_sega8(emu, std::path::PathBuf::from("test.sms"))
    }

    #[test]
    fn gb_ram_cheats_resolve_masked_values() {
        let mut backend = gb_backend();
        let EmuBackend::Gb(gb) = &mut backend else {
            panic!("expected GB backend");
        };
        gb.emu.write_byte(0xC000, 0xA5);

        backend.apply_ram_cheats(&[CheatPatch::RamWrite {
            address: 0xC000,
            value: CheatValue::PreserveWithCurrent {
                mask: 0xF0,
                base: 0x0B,
            },
        }]);

        assert_eq!(backend.gb().unwrap().emu.peek_byte_raw(0xC000), 0xAB);
    }

    #[test]
    fn gb_parameterized_ram_cheats_use_app_selected_value() {
        let mut backend = gb_backend();
        let cheat = CheatCode {
            name: "parameterized".to_string(),
            code_text: "01??A5C6".to_string(),
            enabled: true,
            parameter_value: Some(0x3C),
            code_type: CheatType::GameShark,
            patches: vec![CheatPatch::RamWrite {
                address: 0xC6A5,
                value: CheatValue::UserParameterized {
                    mask: 0xFF,
                    base: 0x00,
                },
            }],
        };
        let patches = collect_enabled_patches(&[cheat], &[]);

        backend.apply_ram_cheats(&patches);

        assert_eq!(backend.gb().unwrap().emu.peek_byte_raw(0xC6A5), 0x3C);
    }

    #[test]
    fn gb_rom_cheats_install_compare_gated_patches() {
        let mut rom = vec![0; 0x8000];
        rom[0x1234] = 0x56;
        let mut backend = gb_backend_from_rom(&rom);

        backend.install_rom_patches(&[CheatPatch::RomWriteIfEquals {
            address: 0x1234,
            value: CheatValue::Constant(0x9A),
            compare: CheatValue::Constant(0x56),
        }]);

        let gb = backend
            .gb()
            .expect("backend should remain GB after cheat install");
        assert_eq!(gb.emu.rom_patches().len(), 1);
        assert_eq!(gb.emu.cpu_peek8(0x1234), 0x9A);

        backend.install_rom_patches(&[CheatPatch::RomWriteIfEquals {
            address: 0x1234,
            value: CheatValue::Constant(0x33),
            compare: CheatValue::Constant(0x99),
        }]);

        let gb = backend
            .gb()
            .expect("backend should remain GB after cheat install");
        assert_eq!(gb.emu.rom_patches().len(), 1);
        assert_eq!(gb.emu.cpu_peek8(0x1234), 0x56);
    }

    #[test]
    fn nes_ram_cheats_check_existing_value_and_resolve_masked_values() {
        let mut backend = nes_backend();
        let EmuBackend::Nes(nes) = &mut backend else {
            panic!("expected NES backend");
        };
        nes.emu.cpu_write(0x0000, 0x11);

        backend.apply_ram_cheats(&[CheatPatch::RamWriteIfEquals {
            address: 0x0000,
            value: CheatValue::Constant(0x42),
            compare: CheatValue::Constant(0x99),
        }]);
        assert_eq!(backend.nes().unwrap().emu.cpu_peek(0x0000), 0x11);

        backend.apply_ram_cheats(&[CheatPatch::RamWriteIfEquals {
            address: 0x0000,
            value: CheatValue::Constant(0x42),
            compare: CheatValue::Constant(0x11),
        }]);
        assert_eq!(backend.nes().unwrap().emu.cpu_peek(0x0000), 0x42);

        let EmuBackend::Nes(nes) = &mut backend else {
            panic!("expected NES backend");
        };
        nes.emu.cpu_write(0x0000, 0xA5);
        backend.apply_ram_cheats(&[CheatPatch::RamWrite {
            address: 0x0000,
            value: CheatValue::PreserveWithCurrent {
                mask: 0xF0,
                base: 0x0B,
            },
        }]);
        assert_eq!(backend.nes().unwrap().emu.cpu_peek(0x0000), 0xAB);
    }

    #[test]
    fn nes_rom_cheats_install_compare_gated_patches() {
        let mut backend = nes_backend();

        backend.install_rom_patches(&[CheatPatch::RomWriteIfEquals {
            address: 0x8000,
            value: CheatValue::Constant(0x42),
            compare: CheatValue::Constant(0xEA),
        }]);

        assert_eq!(backend.nes().unwrap().emu.cpu_peek(0x8000), 0x42);

        backend.install_rom_patches(&[CheatPatch::RomWriteIfEquals {
            address: 0x8000,
            value: CheatValue::Constant(0x99),
            compare: CheatValue::Constant(0x11),
        }]);

        assert_eq!(backend.nes().unwrap().emu.cpu_peek(0x8000), 0xEA);
    }

    #[test]
    fn gba_wide_ram_cheats_write_cpu_address_space() {
        let mut backend = gba_backend();
        let EmuBackend::Gba(gba) = &mut backend else {
            panic!("expected GBA backend");
        };
        gba.emu.cpu_write8(0x0200_0000, 0x01);

        backend.apply_ram_cheats(&[CheatPatch::WideRamWrite {
            address: 0x0200_0000,
            value: CheatValue::Constant(0x42),
        }]);

        assert_eq!(backend.gba().unwrap().emu.cpu_peek8(0x0200_0000), 0x42);
    }

    #[test]
    fn ws_wide_ram_cheats_write_cpu_address_space() {
        let mut backend = ws_backend();
        let EmuBackend::Ws(ws) = &mut backend else {
            panic!("expected WS backend");
        };
        ws.emu.cpu_write8(0x0000_1234, 0x01);

        backend.apply_ram_cheats(&[CheatPatch::WideRamWrite {
            address: 0x0000_1234,
            value: CheatValue::Constant(0x42),
        }]);

        assert_eq!(backend.ws().unwrap().emu.cpu_peek8(0x0000_1234), 0x42);
    }

    #[test]
    fn wide_conditional_ram_cheats_check_existing_value() {
        let mut gba = gba_backend();
        let mut ws = ws_backend();

        let EmuBackend::Gba(gba_backend) = &mut gba else {
            panic!("expected GBA backend");
        };
        gba_backend.emu.cpu_write8(0x0200_0000, 0x01);
        let EmuBackend::Ws(ws_backend) = &mut ws else {
            panic!("expected WS backend");
        };
        ws_backend.emu.cpu_write8(0x0000_1234, 0x01);

        let missed = CheatPatch::WideRamWriteIfEquals {
            address: 0x0200_0000,
            value: CheatValue::Constant(0x42),
            compare: CheatValue::Constant(0x99),
        };
        gba.apply_ram_cheats(&[missed]);
        assert_eq!(gba.gba().unwrap().emu.cpu_peek8(0x0200_0000), 0x01);

        let hit = CheatPatch::WideRamWriteIfEquals {
            address: 0x0000_1234,
            value: CheatValue::Constant(0x42),
            compare: CheatValue::Constant(0x01),
        };
        ws.apply_ram_cheats(&[hit]);
        assert_eq!(ws.ws().unwrap().emu.cpu_peek8(0x0000_1234), 0x42);
    }

    #[test]
    fn sega8_ram_cheats_write_cpu_address_space() {
        let mut backend = sega8_backend();
        let EmuBackend::Sega8(sega8) = &mut backend else {
            panic!("expected Sega8 backend");
        };
        sega8.emu.cpu_write8(0xC123, 0x01);

        backend.apply_ram_cheats(&[CheatPatch::RamWrite {
            address: 0xC123,
            value: CheatValue::Constant(0x42),
        }]);

        assert_eq!(backend.sega8().unwrap().emu.cpu_peek8(0xC123), 0x42);
    }

    #[test]
    fn sega8_conditional_ram_cheats_check_existing_value() {
        let mut backend = sega8_backend();
        let EmuBackend::Sega8(sega8) = &mut backend else {
            panic!("expected Sega8 backend");
        };
        sega8.emu.cpu_write8(0xC123, 0x01);

        backend.apply_ram_cheats(&[CheatPatch::RamWriteIfEquals {
            address: 0xC123,
            value: CheatValue::Constant(0x42),
            compare: CheatValue::Constant(0x99),
        }]);
        assert_eq!(backend.sega8().unwrap().emu.cpu_peek8(0xC123), 0x01);

        backend.apply_ram_cheats(&[CheatPatch::RamWriteIfEquals {
            address: 0xC123,
            value: CheatValue::Constant(0x42),
            compare: CheatValue::Constant(0x01),
        }]);
        assert_eq!(backend.sega8().unwrap().emu.cpu_peek8(0xC123), 0x42);
    }

    #[test]
    fn sega8_rom_cheats_install_as_core_rom_patches() {
        let mut rom = vec![0; zeff_sega8_core::hardware::constants::ROM_BANK_SIZE * 2];
        rom[0x1234] = 0x56;
        let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
            &rom,
            zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE,
            SystemHint::MasterSystem,
        )
        .expect("Sega 8-bit emulator should initialize");
        let mut backend = EmuBackend::from_sega8(emu, std::path::PathBuf::from("test.sms"));

        backend.install_rom_patches(&[CheatPatch::RomWriteIfEquals {
            address: 0x1234,
            value: CheatValue::Constant(0x9A),
            compare: CheatValue::Constant(0x56),
        }]);

        let sega8 = backend
            .sega8()
            .expect("backend should remain Sega 8-bit after cheat install");
        assert_eq!(sega8.emu.rom_patches().len(), 1);
        assert_eq!(sega8.emu.cpu_peek8(0x1234), 0x9A);
    }
}
