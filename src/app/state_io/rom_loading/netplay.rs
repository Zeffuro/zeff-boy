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
        ensure!(
            self.rom_info.rom_path.as_ref() == Some(&path)
                && super::direct_netplay_media(self.active_system, &path),
            "load a direct cartridge file for netplay"
        );
        let system = self.active_system;
        let build = crate::netplay::connect::executable_build()?;
        let mut config = self.backend_load_config(system);
        config.initial_input = None;
        config.sample_rate = Some(48_000);
        config.pce_netplay = system == crate::emu_backend::ActiveSystem::Pce;
        let (candidate, _) = self.init_backend(system, &path, &path, None, config.clone())?;
        crate::netplay::identity::identity_with_delay(&candidate, build, delay)?;
        self.stop_emu_thread();
        let (backend, original_crc) = self.init_backend(system, &path, &path, None, config)?;
        let identity = crate::netplay::identity::identity_with_delay(&backend, build, delay)?;
        let lobby_identity = zeff_netplay_connect::protocol::SessionIdentity {
            core: system.code().into(),
            content_hash: const_hex::encode(identity.effective),
            compatibility_hash: const_hex::encode(identity.config),
            mode: zeff_netplay_connect::protocol::SessionMode::SharedConsole,
        };
        self.commit_prepared_rom(PreparedRomLoad {
            source_path: path.clone(),
            rom_path: path,
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
}
