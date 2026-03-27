use super::Emulator;
use crate::debug::WatchType;
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use crate::hardware::types::{CpuState, ImeState};

impl Emulator {
    pub fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }

    pub fn header(&self) -> &RomHeader {
        &self.header
    }

    pub fn cartridge_rom_bytes(&self) -> &[u8] {
        self.bus.cartridge.rom_bytes()
    }

    pub fn rumble_active(&self) -> bool {
        self.bus.cartridge.rumble_active()
    }

    pub fn hardware_mode(&self) -> HardwareMode {
        self.hardware_mode
    }

    pub fn hardware_mode_preference(&self) -> HardwareModePreference {
        self.hardware_mode_preference
    }

    pub fn is_cgb_mode(&self) -> bool {
        matches!(
            self.hardware_mode,
            HardwareMode::CGBNormal | HardwareMode::CGBDouble
        )
    }

    pub fn set_opcode_log_enabled(&mut self, enabled: bool) {
        self.opcode_log.enabled = enabled;
    }

    pub fn drain_audio_samples(&mut self) -> Vec<f32> {
        self.bus.apu_drain_samples()
    }

    pub fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.bus.apu_drain_samples_into(buf);
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.bus.set_apu_sample_rate(rate);
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.bus.set_apu_sample_generation_enabled(enabled);
    }

    pub fn set_apu_debug_capture_enabled(&mut self, enabled: bool) {
        self.bus.set_apu_debug_capture_enabled(enabled);
    }

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; 4]) {
        self.bus.set_apu_channel_mutes(mutes);
    }

    pub fn apu_channel_snapshot(&self) -> crate::hardware::apu::ApuChannelSnapshot {
        self.bus.apu_channel_snapshot()
    }

    pub fn apu_regs_snapshot(&self) -> [u8; 0x17] {
        self.bus.apu_regs_snapshot()
    }

    pub fn apu_wave_ram_snapshot(&self) -> [u8; 0x10] {
        self.bus.apu_wave_ram_snapshot()
    }

    pub fn apu_nr52_raw(&self) -> u8 {
        self.bus.apu_nr52_raw()
    }

    pub fn apu_channel_debug_samples_ordered(&self, ch: usize) -> [f32; 512] {
        self.bus.apu_channel_debug_samples_ordered(ch)
    }

    pub fn apu_master_debug_samples_ordered(&self) -> [f32; 512] {
        self.bus.apu_master_debug_samples_ordered()
    }

    pub fn apu_channel_mutes(&self) -> [bool; 4] {
        self.bus.apu_channel_mutes()
    }

    pub fn ppu_bg_palette_ram_snapshot(&self) -> [u8; 0x40] {
        self.bus.ppu_bg_palette_ram_snapshot()
    }

    pub fn ppu_obj_palette_ram_snapshot(&self) -> [u8; 0x40] {
        self.bus.ppu_obj_palette_ram_snapshot()
    }

    pub fn set_ppu_debug_flags(&mut self, bg: bool, window: bool, sprites: bool) {
        self.bus.set_ppu_debug_flags(bg, window, sprites);
    }

    pub fn set_input(&mut self, buttons: u8, dpad: u8) {
        if self.bus.apply_joypad_pressed_masks(buttons, dpad) {
            self.bus.if_reg |= 0x10;
        }
    }

    pub fn peek_byte(&self, addr: u16) -> u8 {
        self.bus.read_byte(addr)
    }

    pub fn peek_byte_raw(&self, addr: u16) -> u8 {
        self.bus.read_byte_raw(addr)
    }

    pub fn write_byte(&mut self, addr: u16, value: u8) {
        self.bus.write_byte(addr, value);
    }

    pub fn clear_rom_patches(&mut self) {
        self.bus.game_genie_patches.clear();
    }

    pub fn add_rom_patch(&mut self, patch: crate::cheats::CheatPatch) {
        self.bus.game_genie_patches.push(patch);
    }

    pub fn rom_patches(&self) -> &[crate::cheats::CheatPatch] {
        &self.bus.game_genie_patches
    }

    pub fn cpu_pc(&self) -> u16 {
        self.cpu.pc
    }

    pub fn cpu_sp(&self) -> u16 {
        self.cpu.sp
    }

    pub fn cpu_cycles(&self) -> u64 {
        self.cpu.cycles
    }

    pub fn cpu_a(&self) -> u8 {
        self.cpu.regs.a
    }

    pub fn cpu_f(&self) -> u8 {
        self.cpu.regs.f
    }

    pub fn cpu_ime(&self) -> ImeState {
        self.cpu.ime
    }

    pub fn cpu_running(&self) -> CpuState {
        self.cpu.running
    }

    pub fn is_cpu_suspended(&self) -> bool {
        self.cpu.running == CpuState::Suspended
    }

    pub fn debug_continue(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.cpu.running = CpuState::Running;
    }

    pub fn debug_step(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = true;
        self.cpu.running = CpuState::Running;
    }

    pub fn add_breakpoint(&mut self, addr: u16) {
        self.debug.add_breakpoint(addr);
    }

    pub fn remove_breakpoint(&mut self, addr: u16) {
        self.debug.remove_breakpoint(addr);
    }

    pub fn toggle_breakpoint(&mut self, addr: u16) {
        self.debug.toggle_breakpoint(addr);
    }

    pub fn add_watchpoint(&mut self, addr: u16, watch_type: WatchType) {
        self.debug.add_watchpoint(addr, watch_type);
    }

    pub fn iter_breakpoints(&self) -> impl Iterator<Item = u16> + '_ {
        self.debug.iter_breakpoints()
    }

    pub fn if_reg(&self) -> u8 {
        self.bus.if_reg
    }

    pub fn ie_reg(&self) -> u8 {
        self.bus.ie
    }

    pub fn timer_div(&self) -> u8 {
        self.bus.timer_div()
    }

    pub fn timer_tima(&self) -> u8 {
        self.bus.timer_tima()
    }

    pub fn timer_tac(&self) -> u8 {
        self.bus.timer_tac()
    }

    pub fn serial_output_bytes(&self) -> &[u8] {
        self.bus.serial_output_bytes()
    }

    pub fn set_apu_enabled(&mut self, enabled: bool) {
        self.bus.set_apu_enabled(enabled);
    }
}
