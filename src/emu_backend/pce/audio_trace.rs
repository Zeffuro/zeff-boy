use super::*;
use zeff_emu_common::audio_trace::{Huc6280AudioTrace, MAX_AUDIO_TRACE_EVENTS};

impl PceBackend {
    pub(crate) fn hucard_without_host_persistence(
        hucard_rom: Vec<u8>,
        rom_path: PathBuf,
        source_path: PathBuf,
        console_wiring: Option<PceConsoleWiring>,
        hucard_board: Option<PceHuCardBoard>,
        cartridge_hardware: Option<zeff_pce_core::hardware::PceCartridgeHardware>,
    ) -> anyhow::Result<Self> {
        Self::with_paths_and_persistence(
            hucard_rom,
            BackendPaths::with_source_path(rom_path, source_path),
            console_wiring,
            hucard_board,
            cartridge_hardware,
            false,
        )
    }

    pub(crate) fn reset_and_begin_audio_trace(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.host_persistence_enabled,
            "audio capture requires host persistence disabled"
        );
        self.machine
            .reset_and_begin_audio_trace(MAX_AUDIO_TRACE_EVENTS)?;
        self.frame_count = 0;
        self.pending_runtime_fault = None;
        self.invalidate_frame_output();
        Ok(())
    }

    pub(crate) fn finish_audio_trace(&mut self) -> Option<Huc6280AudioTrace> {
        self.machine.finish_audio_trace()
    }
}
