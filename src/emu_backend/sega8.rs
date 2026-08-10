use std::path::{Path, PathBuf};

use zeff_emu_common::address::Address;
use zeff_sega8_core::emulator::Emulator as Sega8Emulator;
use zeff_sega8_core::hardware::cartridge::{Sega8System, SystemHint};

use crate::emu_backend::ActiveSystem;
use crate::emu_core_trait::EmulatorCore;

impl crate::emu_core_trait::DebuggableEmulator for Sega8Emulator {
    fn add_breakpoint(&mut self, addr: Address) {
        self.add_breakpoint(addr);
    }

    fn add_watchpoint(&mut self, addr: Address, wt: zeff_emu_common::debug::WatchType) {
        self.add_watchpoint(addr, wt);
    }

    fn remove_breakpoint(&mut self, addr: Address) {
        self.remove_breakpoint(addr);
    }

    fn toggle_breakpoint(&mut self, addr: Address) {
        self.toggle_breakpoint(addr);
    }

    fn debug_write(&mut self, addr: Address, val: u8) {
        self.debug_write(addr, val);
    }
}

pub(crate) struct Sega8Backend {
    pub(crate) emu: Sega8Emulator,
    rom_path: PathBuf,
    source_path: PathBuf,
}

impl Sega8Backend {
    pub(crate) fn new(emu: Sega8Emulator, rom_path: PathBuf) -> Self {
        Self::with_source_path(emu, rom_path.clone(), rom_path)
    }

    pub(crate) fn with_source_path(
        emu: Sega8Emulator,
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

    pub(crate) fn system(&self) -> ActiveSystem {
        active_system_for_sega8(self.emu.system())
    }
}

impl EmulatorCore for Sega8Backend {
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
        let arr: [bool; zeff_sega8_core::hardware::apu::PSG_CHANNEL_COUNT] =
            std::array::from_fn(|i| mutes.get(i).copied().unwrap_or(false));
        self.emu.set_apu_channel_mutes(arr);
    }

    #[inline]
    fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu.set_input(buttons_pressed, dpad_pressed);
    }

    #[inline]
    fn is_suspended(&self) -> bool {
        self.emu.is_suspended()
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

pub(crate) fn hint_for_active_system(system: ActiveSystem) -> Option<SystemHint> {
    match system {
        ActiveSystem::MasterSystem => Some(SystemHint::MasterSystem),
        ActiveSystem::GameGear => Some(SystemHint::GameGear),
        ActiveSystem::Sg1000 => Some(SystemHint::Sg1000),
        _ => None,
    }
}

fn active_system_for_sega8(system: Sega8System) -> ActiveSystem {
    match system {
        Sega8System::MasterSystem => ActiveSystem::MasterSystem,
        Sega8System::GameGear => ActiveSystem::GameGear,
        Sega8System::Sg1000 => ActiveSystem::Sg1000,
    }
}

pub(crate) fn try_load_battery_sram(
    emu: &mut Sega8Emulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    crate::save_paths::try_load_battery_sram(rom_path, "Sega 8-bit", emu.has_battery(), |bytes| {
        emu.load_battery_sram(bytes)
    })
}
