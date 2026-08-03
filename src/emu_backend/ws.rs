use std::path::{Path, PathBuf};

use zeff_emu_common::address::Address;
use zeff_ws_core::emulator::Emulator as WsEmulator;

use crate::emu_core_trait::EmulatorCore;

impl crate::emu_core_trait::DebuggableEmulator for WsEmulator {
    fn add_breakpoint(&mut self, _addr: Address) {}

    fn add_watchpoint(&mut self, _addr: Address, _wt: zeff_emu_common::debug::WatchType) {}

    fn remove_breakpoint(&mut self, _addr: Address) {}

    fn toggle_breakpoint(&mut self, _addr: Address) {}

    fn debug_write(&mut self, addr: Address, val: u8) {
        self.cpu_write8(addr, val);
    }
}

pub(crate) struct WsBackend {
    pub(crate) emu: WsEmulator,
    rom_path: PathBuf,
    source_path: PathBuf,
}

impl WsBackend {
    pub(crate) fn new(emu: WsEmulator, rom_path: PathBuf) -> Self {
        Self::with_source_path(emu, rom_path.clone(), rom_path)
    }

    pub(crate) fn with_source_path(
        emu: WsEmulator,
        rom_path: PathBuf,
        source_path: PathBuf,
    ) -> Self {
        Self {
            emu,
            rom_path,
            source_path,
        }
    }

    pub(crate) fn source_path(&self) -> &Path {
        &self.source_path
    }

    pub(crate) fn preferred_orientation(
        &self,
    ) -> zeff_ws_core::hardware::cartridge::RomOrientation {
        self.emu.preferred_orientation()
    }
}

impl EmulatorCore for WsBackend {
    #[inline]
    fn step_frame(&mut self) {
        self.emu.step_frame();
    }

    #[inline]
    fn framebuffer(&self) -> &[u8] {
        self.emu.framebuffer()
    }

    #[inline]
    fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.emu.drain_audio_samples_into(buf);
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.emu.set_sample_rate(rate);
    }

    fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.emu.set_apu_sample_generation_enabled(enabled);
    }

    fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        let arr: [bool; 4] = std::array::from_fn(|i| mutes.get(i).copied().unwrap_or(false));
        self.emu.set_apu_channel_mutes(arr);
    }

    #[inline]
    fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu.set_input(buttons_pressed, dpad_pressed);
    }

    #[inline]
    fn is_suspended(&self) -> bool {
        self.emu.is_cpu_suspended()
    }

    fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        crate::save_paths::flush_battery_sram(&self.rom_path, self.emu.dump_battery_sram())
    }

    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        self.emu.encode_state()
    }

    fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.emu.load_state_from_bytes(bytes)
    }

    fn rom_path(&self) -> &Path {
        &self.rom_path
    }

    fn rom_hash(&self) -> [u8; 32] {
        self.emu.rom_hash()
    }
}

pub(crate) fn try_load_battery_sram(
    emu: &mut WsEmulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    crate::save_paths::try_load_battery_sram(rom_path, "WS", emu.has_battery(), |bytes| {
        emu.load_battery_sram(bytes)
    })
}
