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
}
