use std::path::{Path, PathBuf};

use zeff_emu_common::address::{Address, narrow_u16};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_nes_core::emulator::Emulator as NesEmulator;

use crate::audio_recorder::MidiApuSnapshot;
use crate::emu_core_trait::EmulatorCore;

impl crate::emu_core_trait::DebuggableEmulator for NesEmulator {
    fn add_breakpoint(&mut self, addr: Address) {
        self.add_breakpoint(narrow_u16(addr))
    }
    fn add_watchpoint(&mut self, addr: Address, wt: zeff_emu_common::debug::WatchType) {
        self.add_watchpoint(narrow_u16(addr), wt)
    }
    fn remove_breakpoint(&mut self, addr: Address) {
        self.remove_breakpoint(narrow_u16(addr))
    }
    fn toggle_breakpoint(&mut self, addr: Address) {
        self.toggle_breakpoint(narrow_u16(addr))
    }
    fn debug_write(&mut self, addr: Address, val: u8) {
        self.cpu_write8(narrow_u16(addr), val)
    }
    fn is_cpu_suspended(&self) -> bool {
        self.is_cpu_suspended()
    }
    fn debug_continue(&mut self) {
        self.debug_continue()
    }
    fn debug_step(&mut self) {
        self.debug_step()
    }
    fn set_opcode_log_enabled(&mut self, enabled: bool) {
        self.set_opcode_log_enabled(enabled)
    }
}

pub(crate) struct NesBackend {
    pub(crate) emu: NesEmulator,
    rom_path: PathBuf,
    source_path: PathBuf,
}

impl NesBackend {
    pub(crate) fn new(emu: NesEmulator, rom_path: PathBuf) -> Self {
        Self::with_source_path(emu, rom_path.clone(), rom_path)
    }

    pub(crate) fn with_source_path(
        emu: NesEmulator,
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
}

fn map_host_to_nes_byte(buttons_pressed: u8, dpad_pressed: u8) -> u8 {
    (buttons_pressed & 0x0F)
        | ((dpad_pressed & 0x04) << 2) // Up
        | ((dpad_pressed & 0x08) << 2) // Down
        | ((dpad_pressed & 0x02) << 5) // Left
        | ((dpad_pressed & 0x01) << 7) // Right
}

impl EmulatorCore for NesBackend {
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
        let arr: [bool; 5] = std::array::from_fn(|i| mutes.get(i).copied().unwrap_or(false));
        self.emu.set_apu_channel_mutes(arr);
    }

    #[inline]
    fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu.set_input(buttons_pressed, dpad_pressed);
    }

    #[inline]
    fn set_zapper_state(
        &mut self,
        enabled: bool,
        trigger: bool,
        hit: bool,
        screen_pos: Option<(u16, u16)>,
    ) {
        self.emu.set_zapper_state(enabled, trigger, hit, screen_pos);
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

    fn save_ram_kind(&self) -> SaveRamKind {
        self.emu.save_ram_kind()
    }

    fn system_ram_len(&self) -> usize {
        self.emu.system_ram().len()
    }

    fn video_ram_len(&self) -> usize {
        0x2000
    }

    fn supports_debugger(&self) -> bool {
        true
    }

    #[inline]
    fn set_input_p2(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.emu
            .set_input_p2(map_host_to_nes_byte(buttons_pressed, dpad_pressed));
    }

    fn apu_channel_snapshot(&self) -> Option<MidiApuSnapshot> {
        Some(MidiApuSnapshot::Nes(self.emu.apu_channel_snapshot()))
    }
}

pub(crate) fn try_load_battery_sram(
    emu: &mut NesEmulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    crate::save_paths::try_load_battery_sram(rom_path, "NES", emu.has_battery(), |bytes| {
        emu.load_battery_sram(bytes)
    })
}

#[cfg(test)]
mod tests;
