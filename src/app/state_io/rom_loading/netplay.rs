use anyhow::{Context, Result, ensure};

use super::{App, PreparedRomLoad};

impl App {
    pub(in crate::app) fn prepare_netplay_game(
        &mut self,
        delay: zeff_netplay::rollback::InputDelay,
    ) -> Result<zeff_netplay_connect::protocol::SessionIdentity> {
        let path = self
            .rom_info
            .source_path
            .clone()
            .context("no cartridge loaded")?;
        let rom_path = self
            .rom_info
            .rom_path
            .clone()
            .context("no cartridge selected")?;
        ensure!(
            super::direct_netplay_media(self.active_system, &rom_path)
                && (path == rom_path || super::is_zip_path(&path)),
            "load a supported cartridge file or ZIP for netplay"
        );
        let system = self.active_system;
        let build = crate::netplay::connect::executable_build()?;
        let mut config = self.backend_load_config(system);
        config.initial_input = None;
        config.sample_rate = Some(48_000);
        config.pce_netplay = system == crate::emu_backend::ActiveSystem::Pce;
        let (candidate, _) = self.init_netplay_backend(system, &path, &rom_path, config.clone())?;
        let candidate_identity =
            crate::netplay::identity::identity_with_delay(&candidate, build, delay)?;
        ensure!(
            Some(candidate_identity.effective) == self.rom_info.rom_hash,
            "cartridge contents changed since loading"
        );
        self.stop_emu_thread();
        let (backend, original_crc) =
            self.init_netplay_backend(system, &path, &rom_path, config)?;
        let identity = crate::netplay::identity::identity_with_delay(&backend, build, delay)?;
        ensure!(
            identity.source == candidate_identity.source
                && identity.media_len == candidate_identity.media_len
                && identity.effective == candidate_identity.effective
                && identity.config == candidate_identity.config,
            "cartridge contents or configuration changed during netplay preparation"
        );
        let lobby_identity = zeff_netplay_connect::protocol::SessionIdentity {
            core: system.code().into(),
            content_hash: const_hex::encode(identity.effective),
            compatibility_hash: const_hex::encode(identity.config),
            mode: crate::netplay::capabilities::session_mode(self.active_system),
        };
        self.commit_prepared_rom(PreparedRomLoad {
            source_path: path.clone(),
            rom_path,
            system,
            auto_load_state: false,
            backend,
            original_crc,
        });
        self.pending_debug_actions = crate::debug::DebugUiActions::none();
        self.cached_ui_data = None;
        self.undo_load_state = None;
        self.undo_save_state_path = None;
        Ok(lobby_identity)
    }

    fn init_netplay_backend(
        &self,
        system: super::ActiveSystem,
        source: &std::path::Path,
        rom: &std::path::Path,
        mut config: super::BackendLoadConfig,
    ) -> Result<(super::EmuBackend, u32)> {
        let bytes = if source == rom {
            None
        } else {
            let extension = rom
                .extension()
                .and_then(|value| value.to_str())
                .context("invalid cartridge extension")?;
            let extraction = crate::rom_archive::extract_authenticated_bounded_zip_member(
                source,
                Some(rom),
                extension,
                128 * 1024 * 1024,
                64 * 1024 * 1024,
            )?;
            config.authenticated_zip_member = Some(extraction.witness);
            Some(extraction.bytes)
        };
        self.init_backend(system, source, rom, bytes, config)
    }
}

#[cfg(test)]
mod tests;
