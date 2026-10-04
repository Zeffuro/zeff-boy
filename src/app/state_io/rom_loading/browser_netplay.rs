use anyhow::{Context, Result, ensure};
use zeff_netplay::rollback::InputDelay;

use super::{ActiveSystem, App, EmuBackend};

impl App {
    pub(in crate::app) fn browser_netplay_candidate(
        &self,
        delay: InputDelay,
    ) -> Result<EmuBackend> {
        let (path, bytes) = self
            .browser_netplay_media
            .as_ref()
            .context("Load a direct .nes file first")?;
        ensure!(
            self.rom_info.source_path.as_ref() == Some(path)
                && self.rom_info.rom_path.as_ref() == Some(path),
            "Cartridge source changed"
        );
        let build = crate::netplay::connect::executable_build()?;
        let mut config = self.backend_load_config(ActiveSystem::Nes);
        config.initial_input = None;
        config.sample_rate = Some(48_000);
        let (backend, _) =
            self.init_backend(ActiveSystem::Nes, path, path, Some(bytes.to_vec()), config)?;
        crate::netplay::identity::identity_with_delay(&backend, build, delay)?;
        Ok(backend)
    }

    pub(in crate::app) fn prepare_netplay_game(
        &mut self,
        delay: InputDelay,
    ) -> Result<zeff_netplay_connect::protocol::SessionIdentity> {
        ensure!(
            self.emu_thread.is_none() && self.wasm_retired_threads.is_empty(),
            "Browser storage is busy"
        );
        let backend = self.browser_netplay_candidate(delay)?;
        let identity = crate::netplay::identity::identity_with_delay(
            &backend,
            crate::netplay::connect::executable_build()?,
            delay,
        )?;
        let lobby_identity = zeff_netplay_connect::protocol::SessionIdentity {
            core: "nes".into(),
            content_hash: const_hex::encode(identity.effective),
            compatibility_hash: const_hex::encode(identity.config),
            mode: zeff_netplay_connect::protocol::SessionMode::SharedConsole,
        };
        self.finalize_rom_load(
            &backend,
            ActiveSystem::Nes,
            backend.rom_path().to_path_buf(),
            backend.source_path().to_path_buf(),
        );
        self.spawn_emu_thread(backend);
        self.pending_debug_actions = crate::debug::DebugUiActions::none();
        self.cached_ui_data = None;
        self.undo_load_state = None;
        self.undo_save_state_path = None;
        Ok(lobby_identity)
    }
}
