use super::{ActiveSystem, App, BackendLoadConfig, EmuBackend, Path};
use crate::emu_backend::load_backend_from_rom_source;

pub(in crate::app::state_io) fn direct_netplay_media(system: ActiveSystem, path: &Path) -> bool {
    let extension = match system {
        ActiveSystem::Nes => "nes",
        ActiveSystem::MasterSystem => "sms",
        ActiveSystem::Sg1000 => "sg",
        ActiveSystem::Pce => "pce",
        _ => return false,
    };
    path.extension()
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

impl App {
    pub(in crate::app::state_io) fn init_backend(
        &self,
        system: ActiveSystem,
        path: &Path,
        rom_path: &Path,
        preloaded_data: Option<Vec<u8>>,
        config: BackendLoadConfig,
    ) -> anyhow::Result<(EmuBackend, u32)> {
        #[cfg(target_arch = "wasm32")]
        let mut config = config;
        #[cfg(target_arch = "wasm32")]
        {
            config.netplay_browser_media = preloaded_data.as_ref().and_then(|bytes| {
                direct_netplay_media(system, rom_path)
                    .then(|| crate::emu_backend::loader::NetplayRomMedia::browser(bytes))
                    .flatten()
            });
        }
        let loaded = load_backend_from_rom_source(system, path, rom_path, preloaded_data, config)?;
        Ok((loaded.backend, loaded.original_crc32))
    }
}
