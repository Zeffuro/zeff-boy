use crate::emu_backend::EmuBackend;

use super::EmuThread;

impl EmuThread {
    pub(crate) fn install_rom_patches(
        backend: &mut EmuBackend,
        cheats: &[crate::cheats::CheatPatch],
    ) {
        if let Some(gb) = backend.gb_mut() {
            use crate::cheats::CheatPatch;
            gb.emu.clear_rom_patches();
            for patch in cheats {
                match *patch {
                    CheatPatch::RomWrite { .. } | CheatPatch::RomWriteIfEquals { .. } => {
                        gb.emu.add_rom_patch(*patch);
                    }
                    _ => {}
                }
            }
        }
        if let Some(nes) = backend.nes_mut() {
            use crate::cheats::CheatPatch;
            nes.emu.clear_game_genie();
            for patch in cheats {
                match *patch {
                    CheatPatch::RomWrite { address, value } => {
                        let v = match value {
                            crate::cheats::CheatValue::Constant(v) => v,
                            _ => continue,
                        };
                        nes.emu
                            .add_game_genie_patch(zeff_nes_core::cheats::NesGameGeniePatch {
                                address,
                                value: v,
                                compare: None,
                            });
                    }
                    CheatPatch::RomWriteIfEquals {
                        address,
                        value,
                        compare,
                    } => {
                        let v = match value {
                            crate::cheats::CheatValue::Constant(v) => v,
                            _ => continue,
                        };
                        let c = match compare {
                            crate::cheats::CheatValue::Constant(c) => c,
                            _ => continue,
                        };
                        nes.emu
                            .add_game_genie_patch(zeff_nes_core::cheats::NesGameGeniePatch {
                                address,
                                value: v,
                                compare: Some(c),
                            });
                    }
                    _ => {}
                }
            }
        }
        if let Some(sega8) = backend.sega8_mut() {
            use crate::cheats::CheatPatch;
            sega8.emu.clear_rom_patches();
            for patch in cheats {
                match *patch {
                    CheatPatch::RomWrite { .. } | CheatPatch::RomWriteIfEquals { .. } => {
                        sega8.emu.add_rom_patch(*patch);
                    }
                    _ => {}
                }
            }
        }
    }

    pub(crate) fn apply_ram_cheats(
        emu: &mut zeff_gb_core::emulator::Emulator,
        cheats: &[crate::cheats::CheatPatch],
    ) {
        use crate::cheats::CheatPatch;
        for patch in cheats {
            match *patch {
                CheatPatch::RamWrite { address, value } => {
                    let current = emu.peek_byte_raw(address);
                    emu.write_byte(address, value.resolve_with_current(current));
                }
                CheatPatch::RamWriteIfEquals {
                    address,
                    value,
                    compare,
                } => {
                    let current = emu.peek_byte_raw(address);
                    if compare.matches(current) {
                        emu.write_byte(address, value.resolve_with_current(current));
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn apply_nes_ram_cheats(
        emu: &mut zeff_nes_core::emulator::Emulator,
        cheats: &[crate::cheats::CheatPatch],
    ) {
        use crate::cheats::CheatPatch;
        for patch in cheats {
            match *patch {
                CheatPatch::RamWrite { address, value } => {
                    let v = match value {
                        crate::cheats::CheatValue::Constant(v) => v,
                        _ => continue,
                    };
                    emu.cpu_write(address, v);
                }
                CheatPatch::RamWriteIfEquals {
                    address,
                    value,
                    compare,
                } => {
                    let current = emu.cpu_peek(address);
                    if compare.matches(current) {
                        let v = match value {
                            crate::cheats::CheatValue::Constant(v) => v,
                            _ => continue,
                        };
                        emu.cpu_write(address, v);
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn apply_gba_ram_cheats(
        emu: &mut zeff_gba_core::emulator::Emulator,
        cheats: &[crate::cheats::CheatPatch],
    ) {
        use crate::cheats::CheatPatch;
        for patch in cheats {
            match *patch {
                CheatPatch::WideRamWrite { address, value } => {
                    let current = emu.cpu_peek8(address);
                    emu.cpu_write8(address, value.resolve_with_current(current));
                }
                CheatPatch::WideRamWriteIfEquals {
                    address,
                    value,
                    compare,
                } => {
                    let current = emu.cpu_peek8(address);
                    if compare.matches(current) {
                        emu.cpu_write8(address, value.resolve_with_current(current));
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn apply_sega8_ram_cheats(
        emu: &mut zeff_sega8_core::emulator::Emulator,
        cheats: &[crate::cheats::CheatPatch],
    ) {
        use crate::cheats::CheatPatch;
        for patch in cheats {
            match *patch {
                CheatPatch::RamWrite { address, value } => {
                    let current = emu.cpu_peek8(address);
                    emu.cpu_write8(address, value.resolve_with_current(current));
                }
                CheatPatch::RamWriteIfEquals {
                    address,
                    value,
                    compare,
                } => {
                    let current = emu.cpu_peek8(address);
                    if compare.matches(current) {
                        emu.cpu_write8(address, value.resolve_with_current(current));
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn apply_ws_ram_cheats(
        emu: &mut zeff_ws_core::emulator::Emulator,
        cheats: &[crate::cheats::CheatPatch],
    ) {
        use crate::cheats::CheatPatch;
        for patch in cheats {
            match *patch {
                CheatPatch::WideRamWrite { address, value } => {
                    let current = emu.cpu_peek8(address);
                    emu.cpu_write8(address, value.resolve_with_current(current));
                }
                CheatPatch::WideRamWriteIfEquals {
                    address,
                    value,
                    compare,
                } => {
                    let current = emu.cpu_peek8(address);
                    if compare.matches(current) {
                        emu.cpu_write8(address, value.resolve_with_current(current));
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EmuThread;
    use crate::cheats::{CheatPatch, CheatValue};
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

    #[test]
    fn gba_wide_ram_cheats_write_cpu_address_space() {
        let mut emu =
            zeff_gba_core::emulator::Emulator::new(&gba_rom(), 44_100).expect("GBA should init");

        emu.cpu_write8(0x0200_0000, 0x01);
        EmuThread::apply_gba_ram_cheats(
            &mut emu,
            &[CheatPatch::WideRamWrite {
                address: 0x0200_0000,
                value: CheatValue::Constant(0x42),
            }],
        );

        assert_eq!(emu.cpu_peek8(0x0200_0000), 0x42);
    }

    #[test]
    fn ws_wide_ram_cheats_write_cpu_address_space() {
        let mut emu =
            zeff_ws_core::emulator::Emulator::new(&ws_rom(), 44_100).expect("WS should init");

        emu.cpu_write8(0x0000_1234, 0x01);
        EmuThread::apply_ws_ram_cheats(
            &mut emu,
            &[CheatPatch::WideRamWrite {
                address: 0x0000_1234,
                value: CheatValue::Constant(0x42),
            }],
        );

        assert_eq!(emu.cpu_peek8(0x0000_1234), 0x42);
    }

    #[test]
    fn wide_conditional_ram_cheats_check_existing_value() {
        let mut gba =
            zeff_gba_core::emulator::Emulator::new(&gba_rom(), 44_100).expect("GBA should init");
        let mut ws =
            zeff_ws_core::emulator::Emulator::new(&ws_rom(), 44_100).expect("WS should init");

        gba.cpu_write8(0x0200_0000, 0x01);
        ws.cpu_write8(0x0000_1234, 0x01);

        let missed = CheatPatch::WideRamWriteIfEquals {
            address: 0x0200_0000,
            value: CheatValue::Constant(0x42),
            compare: CheatValue::Constant(0x99),
        };
        EmuThread::apply_gba_ram_cheats(&mut gba, &[missed]);
        assert_eq!(gba.cpu_peek8(0x0200_0000), 0x01);

        let hit = CheatPatch::WideRamWriteIfEquals {
            address: 0x0000_1234,
            value: CheatValue::Constant(0x42),
            compare: CheatValue::Constant(0x01),
        };
        EmuThread::apply_ws_ram_cheats(&mut ws, &[hit]);
        assert_eq!(ws.cpu_peek8(0x0000_1234), 0x42);
    }

    #[test]
    fn sega8_ram_cheats_write_cpu_address_space() {
        let mut emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
            &[0x76],
            zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE,
            SystemHint::MasterSystem,
        )
        .expect("Sega 8-bit emulator should initialize");

        emu.cpu_write8(0xC123, 0x01);
        EmuThread::apply_sega8_ram_cheats(
            &mut emu,
            &[CheatPatch::RamWrite {
                address: 0xC123,
                value: CheatValue::Constant(0x42),
            }],
        );

        assert_eq!(emu.cpu_peek8(0xC123), 0x42);
    }

    #[test]
    fn sega8_conditional_ram_cheats_check_existing_value() {
        let mut emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
            &[0x76],
            zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE,
            SystemHint::MasterSystem,
        )
        .expect("Sega 8-bit emulator should initialize");

        emu.cpu_write8(0xC123, 0x01);
        EmuThread::apply_sega8_ram_cheats(
            &mut emu,
            &[CheatPatch::RamWriteIfEquals {
                address: 0xC123,
                value: CheatValue::Constant(0x42),
                compare: CheatValue::Constant(0x99),
            }],
        );
        assert_eq!(emu.cpu_peek8(0xC123), 0x01);

        EmuThread::apply_sega8_ram_cheats(
            &mut emu,
            &[CheatPatch::RamWriteIfEquals {
                address: 0xC123,
                value: CheatValue::Constant(0x42),
                compare: CheatValue::Constant(0x01),
            }],
        );
        assert_eq!(emu.cpu_peek8(0xC123), 0x42);
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
        let mut backend =
            crate::emu_backend::EmuBackend::from_sega8(emu, std::path::PathBuf::from("test.sms"));

        EmuThread::install_rom_patches(
            &mut backend,
            &[CheatPatch::RomWriteIfEquals {
                address: 0x1234,
                value: CheatValue::Constant(0x9A),
                compare: CheatValue::Constant(0x56),
            }],
        );

        let sega8 = backend
            .sega8()
            .expect("backend should remain Sega 8-bit after cheat install");
        assert_eq!(sega8.emu.rom_patches().len(), 1);
        assert_eq!(sega8.emu.cpu_peek8(0x1234), 0x9A);
    }
}
