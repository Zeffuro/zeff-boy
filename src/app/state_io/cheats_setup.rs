use super::App;
use crate::debug::types::CheatState;
use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::libretro_common::LibretroPlatform;
use std::path::Path;

impl App {
    pub(in crate::app) fn save_current_cheats(&self) {
        if let Some(ref title) = self.debug_windows.cheat.rom_title {
            crate::cheats::save_game_cheats(
                self.active_system,
                Some(title),
                self.debug_windows.cheat.rom_crc32,
                &self.debug_windows.cheat.user_codes,
                &self.debug_windows.cheat.libretro_codes,
            );
        }
    }

    pub(in crate::app) fn setup_cheats_for_rom(
        &mut self,
        system: ActiveSystem,
        path: &Path,
        backend: &EmuBackend,
    ) {
        self.save_current_cheats();

        self.debug_windows.cheat.active_system = system;

        if let Some(gb) = backend.gb() {
            let rom_header_title = gb.emu.header().title.clone();
            let is_gbc = gb.emu.header().is_cgb_compatible || gb.emu.header().is_cgb_exclusive;
            let rom_crc32 = crc32fast::hash(gb.emu.cartridge_rom_bytes());
            let platform = if is_gbc {
                crate::libretro_common::LibretroPlatform::Gbc
            } else {
                crate::libretro_common::LibretroPlatform::Gb
            };
            apply_cheat_rom_info(
                &mut self.debug_windows.cheat,
                system,
                rom_header_title,
                Some(rom_crc32),
                platform,
            );
        } else if system == ActiveSystem::Nes {
            let rom_title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("NES ROM")
                .to_string();
            let rom_crc32 = backend.nes().map(|nes| nes.emu.rom_crc32());
            let platform = crate::libretro_common::LibretroPlatform::Nes;
            apply_cheat_rom_info(
                &mut self.debug_windows.cheat,
                system,
                rom_title,
                rom_crc32,
                platform,
            );
        } else if let Some(gba) = backend.gba() {
            let rom_title = if gba.emu.cartridge_header().title.is_empty() {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("GBA ROM")
                    .to_string()
            } else {
                gba.emu.cartridge_header().title.clone()
            };
            let rom_crc32 = Some(crc32fast::hash(gba.emu.cartridge_rom_bytes()));
            apply_cheat_rom_info(
                &mut self.debug_windows.cheat,
                system,
                rom_title,
                rom_crc32,
                crate::libretro_common::LibretroPlatform::Gba,
            );
        } else if let Some(sega8) = backend.sega8() {
            let rom_title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Sega 8-bit ROM")
                .to_string();
            let rom_crc32 = Some(crc32fast::hash(sega8.emu.bus().cartridge.rom()));
            match sega8_libretro_platform(system) {
                Some(platform) => apply_cheat_rom_info(
                    &mut self.debug_windows.cheat,
                    system,
                    rom_title,
                    rom_crc32,
                    platform,
                ),
                None => apply_local_cheat_rom_info(
                    &mut self.debug_windows.cheat,
                    system,
                    rom_title,
                    rom_crc32,
                ),
            }
        } else if let Some(ws) = backend.ws() {
            let rom_title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("WonderSwan ROM")
                .to_string();
            apply_local_cheat_rom_info(
                &mut self.debug_windows.cheat,
                system,
                rom_title,
                Some(ws.emu.rom_crc32()),
            );
        } else {
            self.debug_windows.cheat.rom_title = None;
            self.debug_windows.cheat.rom_crc32 = None;
            self.debug_windows.cheat.rom_metadata_title = None;
            self.debug_windows.cheat.rom_metadata_rom_name = None;
            self.debug_windows.cheat.libretro_search_hints.clear();
            self.debug_windows.cheat.libretro_search.clear();
            self.debug_windows.cheat.user_codes.clear();
            self.debug_windows.cheat.libretro_codes.clear();
        }

        self.debug_windows.cheat.libretro_results.clear();
        self.debug_windows.cheat.libretro_file_list = None;
        self.debug_windows.cheat.libretro_status = None;
        self.debug_windows.cheat.cheats_dirty = true;
    }

    pub(in crate::app) fn setup_mods_for_rom(&mut self, system: ActiveSystem, original_crc: u32) {
        let dir = crate::mods::mods_dir_for_rom(system, original_crc);
        let entries = crate::mods::load_mod_config(&dir);
        let warnings = crate::mods::mod_advisories(&dir, &entries);
        self.debug_windows.mod_state.entries = entries;
        self.debug_windows.mod_state.mods_dir = Some(dir);
        self.debug_windows.mod_state.needs_reload = false;
        self.debug_windows.mod_state.status_message =
            (!warnings.is_empty()).then(|| warnings.join("\n"));
    }
}

fn sega8_libretro_platform(system: ActiveSystem) -> Option<LibretroPlatform> {
    match system {
        ActiveSystem::MasterSystem => Some(LibretroPlatform::MasterSystem),
        ActiveSystem::GameGear => Some(LibretroPlatform::GameGear),
        _ => None,
    }
}

fn apply_cheat_rom_info(
    cheat: &mut CheatState,
    system: ActiveSystem,
    rom_title: String,
    rom_crc32: Option<u32>,
    platform: LibretroPlatform,
) {
    let libretro_meta =
        rom_crc32.and_then(|crc| crate::libretro_metadata::lookup_cached(crc, platform));
    let search_hints =
        crate::libretro_metadata::build_cheat_search_hints(&rom_title, libretro_meta.as_ref());

    cheat.rom_title = Some(rom_title.clone());
    cheat.rom_crc32 = rom_crc32;
    cheat.rom_metadata_title = libretro_meta.as_ref().map(|m| m.title.clone());
    cheat.rom_metadata_rom_name = libretro_meta.as_ref().map(|m| m.rom_name.clone());
    cheat.libretro_platform = platform;
    cheat.libretro_search_hints = search_hints;
    cheat.libretro_search = cheat
        .libretro_search_hints
        .first()
        .cloned()
        .unwrap_or(rom_title);

    let (user, libretro) =
        crate::cheats::load_game_cheats(system, cheat.rom_title.as_deref(), rom_crc32);
    cheat.user_codes = user;
    cheat.libretro_codes = libretro;
}

fn apply_local_cheat_rom_info(
    cheat: &mut CheatState,
    system: ActiveSystem,
    rom_title: String,
    rom_crc32: Option<u32>,
) {
    cheat.rom_title = Some(rom_title.clone());
    cheat.rom_crc32 = rom_crc32;
    cheat.rom_metadata_title = None;
    cheat.rom_metadata_rom_name = None;
    cheat.libretro_search_hints.clear();
    cheat.libretro_search = rom_title;

    let (user, libretro) =
        crate::cheats::load_game_cheats(system, cheat.rom_title.as_deref(), rom_crc32);
    cheat.user_codes = user;
    cheat.libretro_codes = libretro;
}
