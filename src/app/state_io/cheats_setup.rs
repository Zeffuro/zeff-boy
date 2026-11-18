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
        reset_libretro_requests(&mut self.debug_windows.cheat);

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
        } else if let EmuBackend::Md(md) = backend {
            apply_md_cheat_rom_info(&mut self.debug_windows.cheat, system, path, md);
        } else if let EmuBackend::Pce(pce) = backend {
            apply_pce_cheat_rom_info(&mut self.debug_windows.cheat, system, path, pce);
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

fn reset_libretro_requests(cheat: &mut CheatState) {
    cheat.libretro_results.clear();
    cheat.libretro_file_list = None;
    cheat.libretro_status = None;
    cheat.libretro_rx = None;
    cheat.libretro_busy = false;
}

fn apply_md_cheat_rom_info(
    cheat: &mut CheatState,
    system: ActiveSystem,
    path: &Path,
    md: &crate::emu_backend::MdBackend,
) {
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Mega Drive ROM")
        .to_string();
    apply_cheat_rom_info(
        cheat,
        system,
        title,
        Some(crc32fast::hash(md.core().rom_bytes())),
        LibretroPlatform::Md,
    );
}

fn apply_pce_cheat_rom_info(
    cheat: &mut CheatState,
    system: ActiveSystem,
    path: &Path,
    pce: &crate::emu_backend::PceBackend,
) {
    let rom_title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("PC Engine game")
        .to_string();
    let rom_crc32 = Some(crc32fast::hash(&pce.controller_profile_hash()));
    apply_cheat_rom_info(
        cheat,
        system,
        rom_title,
        rom_crc32,
        pce_libretro_platform(pce.cdrom2().is_some(), pce.hardware_topology()),
    );
}

fn pce_libretro_platform(
    has_cd: bool,
    topology: zeff_pce_core::hardware::PceHardwareTopology,
) -> LibretroPlatform {
    if has_cd {
        LibretroPlatform::PceCd
    } else if topology == zeff_pce_core::hardware::PceHardwareTopology::SuperGrafx {
        LibretroPlatform::SuperGrafx
    } else {
        LibretroPlatform::Pce
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emu_core_trait::EmulatorCore;
    use zeff_emu_common::time::FrameLifecycle;
    use zeff_pce_core::hardware::PceHardwareTopology;

    #[test]
    fn pce_cheat_database_follows_loaded_hardware() {
        assert_eq!(
            pce_libretro_platform(false, PceHardwareTopology::Base),
            LibretroPlatform::Pce
        );
        assert_eq!(
            pce_libretro_platform(true, PceHardwareTopology::Base),
            LibretroPlatform::PceCd
        );
        assert_eq!(
            pce_libretro_platform(false, PceHardwareTopology::SuperGrafx),
            LibretroPlatform::SuperGrafx
        );
    }

    #[test]
    fn pce_setup_import_and_application_use_physical_work_ram() {
        let mut rom = vec![0xEA; 0x2000];
        rom[..6].copy_from_slice(&[0xA9, 0xF8, 0x53, 0x02, 0x80, 0xFE]);
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        let mut backend =
            crate::emu_backend::PceBackend::new(rom, Path::new("Synthetic PCE.pce").to_path_buf())
                .expect("PCE backend should initialize");
        backend.step_frame();

        let mut state = CheatState::new();
        state.active_system = ActiveSystem::Pce;
        apply_pce_cheat_rom_info(
            &mut state,
            ActiveSystem::Pce,
            Path::new("Synthetic PCE.pce"),
            &backend,
        );
        assert_eq!(state.rom_title.as_deref(), Some("Synthetic PCE"));
        assert_eq!(state.libretro_platform, LibretroPlatform::Pce);
        assert_eq!(
            state.rom_crc32,
            Some(crc32fast::hash(&backend.controller_profile_hash()))
        );
        assert!(
            crate::emu_backend::CheatCapabilities::for_system(ActiveSystem::Pce)
                .supports_libretro_database
        );

        state.user_codes.clear();
        state.libretro_codes = crate::cheats::parse_cht_file_for_system(
            "cheat0_desc = \"Lives\"\ncheat0_code = \"1f008d:09\"\ncheat0_enable = true\n",
            ActiveSystem::Pce,
        );
        let patches =
            crate::cheats::collect_enabled_patches(&state.user_codes, &state.libretro_codes);
        EmulatorCore::apply_ram_cheats(&mut backend, &patches);

        assert_eq!(
            backend.debug_peek8(zeff_emu_common::address::Address::from(0x208D_u16)),
            0x09
        );
    }
}

#[cfg(test)]
mod md_database_tests {
    use super::*;

    #[test]
    fn md_cheat_setup_uses_source_crc_title_and_clears_previous_rom_state() {
        let mut rom = vec![0; 0x400];
        rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
        rom[0x1f0] = b'U';
        rom[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
        let crc = crc32fast::hash(&rom);
        let path = Path::new("Synthetic MD setup.md");
        let backend = crate::emu_backend::load_backend_from_rom_source(
            ActiveSystem::Md,
            path,
            path,
            Some(rom),
            crate::emu_backend::BackendLoadConfig {
                md_load_save_ram: false,
                md_host_persistence: false,
                ..Default::default()
            },
        )
        .unwrap()
        .backend;
        let EmuBackend::Md(md) = backend else {
            unreachable!()
        };
        let expected_codes = crate::cheats::load_game_cheats(
            ActiveSystem::Md,
            Some("Synthetic MD setup"),
            Some(crc),
        );
        let expected_metadata = crate::libretro_metadata::lookup_cached(crc, LibretroPlatform::Md);
        let expected_hints = crate::libretro_metadata::build_cheat_search_hints(
            "Synthetic MD setup",
            expected_metadata.as_ref(),
        );
        let mut state = CheatState::new();
        state.active_system = ActiveSystem::Md;
        state.rom_metadata_title = Some("Previous metadata".into());
        state.rom_metadata_rom_name = Some("Previous.gbc".into());
        state.libretro_search_hints = vec!["Previous hint".into()];
        state.libretro_search = "Previous query".into();
        state.libretro_file_list = Some(vec!["Previous.cht".into()]);
        state.libretro_results = vec!["Previous.cht".into()];
        state.libretro_status = Some("Previous status".into());
        state.user_codes = crate::cheats::parse_cht_file_for_system(
            "cheat0_code = \"FF0100:11\"\n",
            ActiveSystem::Md,
        );
        state.libretro_codes = state.user_codes.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        state.libretro_rx = Some(rx);
        state.libretro_busy = true;
        tx.send(crate::debug::LibretroAsyncResult::Downloaded {
            filename: "Previous.cht".into(),
            result: Ok("cheat0_code = \"FF0100:22\"\n".into()),
        })
        .unwrap();
        reset_libretro_requests(&mut state);
        apply_md_cheat_rom_info(&mut state, ActiveSystem::Md, path, &md);
        assert_eq!(state.rom_title.as_deref(), Some("Synthetic MD setup"));
        assert_eq!(state.rom_crc32, Some(crc));
        assert_eq!(state.libretro_platform, LibretroPlatform::Md);
        assert_eq!(
            state.rom_metadata_title,
            expected_metadata.as_ref().map(|meta| meta.title.clone())
        );
        assert_eq!(
            state.rom_metadata_rom_name,
            expected_metadata.as_ref().map(|meta| meta.rom_name.clone())
        );
        assert_eq!(state.libretro_search_hints, expected_hints);
        assert_eq!(
            state.libretro_search,
            expected_hints
                .first()
                .map(String::as_str)
                .unwrap_or("Synthetic MD setup")
        );
        assert_eq!(
            crate::cheats::export_cht_file(&state.user_codes),
            crate::cheats::export_cht_file(&expected_codes.0)
        );
        assert_eq!(
            crate::cheats::export_cht_file(&state.libretro_codes),
            crate::cheats::export_cht_file(&expected_codes.1)
        );
        assert!(state.libretro_results.is_empty() && state.libretro_file_list.is_none());
        assert!(
            state.libretro_rx.is_none() && !state.libretro_busy && state.libretro_status.is_none()
        );
        assert!(
            tx.send(crate::debug::LibretroAsyncResult::FileList(Ok(vec![])))
                .is_err()
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod md_database_native_tests;
