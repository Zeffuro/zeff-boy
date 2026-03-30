use crate::hardware::cartridge::Cartridge;
use crate::hardware::io::IO;
use crate::hardware::types::constants::*;
use crate::hardware::types::hardware_mode::HardwareMode;
use std::fmt;

mod dma;
mod io_bus;
mod lifecycle;
mod mem_map;
mod oam_corruption;
mod state;
mod trace;

pub use oam_corruption::OamCorruptionType;
pub use trace::CpuAccessTraceEvent;

pub struct Bus {
    pub cartridge: Cartridge,
    pub hardware_mode: HardwareMode,
    pub vram: Box<[u8]>,
    pub wram: Box<[u8]>,
    pub vram_bank: u8,
    pub wram_bank: u8,
    pub key1: u8,
    pub hdma1: u8,
    pub hdma2: u8,
    pub hdma3: u8,
    pub hdma4: u8,
    pub hdma5: u8,
    pub hdma_active: bool,
    pub hdma_hblank: bool,
    pub hdma_blocks_left: u8,
    pub oam_dma_active: bool,
    oam_dma_source_base: u16,
    oam_dma_index: u16,
    oam_dma_t_cycle_accum: u64,
    pub oam: [u8; OAM_SIZE],
    pub io_bank: [u8; IO_SIZE],
    pub hram: [u8; HRAM_SIZE],
    pub ie: u8,
    pub if_reg: u8,
    io: IO,
    pub trace_cpu_accesses: bool,
    cpu_read_trace: Vec<(u16, u8)>,
    cpu_write_trace: Vec<(u16, u8, u8)>,
    pub game_genie_patches: Vec<crate::cheats::CheatPatch>,
}

impl fmt::Debug for Bus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bus")
            .field("hardware_mode", &self.hardware_mode)
            .field("vram_bank", &self.vram_bank)
            .field("wram_bank", &self.wram_bank)
            .field("key1", &format_args!("{:#04X}", self.key1))
            .field("ie", &format_args!("{:#04X}", self.ie))
            .field("if_reg", &format_args!("{:#04X}", self.if_reg))
            .field("oam_dma_active", &self.oam_dma_active)
            .field("hdma_active", &self.hdma_active)
            .field("hdma_hblank", &self.hdma_hblank)
            .field("game_genie_patches", &self.game_genie_patches.len())
            .field("io", &self.io)
            .finish_non_exhaustive()
    }
}

impl Bus {
    pub fn joypad_p1(&self) -> u8 {
        self.io.joypad.read()
    }

    pub fn write_joypad_p1(&mut self, value: u8) {
        self.io.joypad.write(value);
    }

    pub fn apply_joypad_pressed_masks(
        &mut self,
        buttons_pressed: u8,
        dpad_pressed: u8,
    ) -> bool {
        self.io
            .joypad
            .apply_pressed_masks(buttons_pressed, dpad_pressed)
    }

    pub fn apu_sample_rate(&self) -> u32 {
        self.io.apu.sample_rate
    }

    pub fn set_apu_sample_rate(&mut self, sample_rate: u32) {
        self.io.apu.set_sample_rate(sample_rate);
    }

    pub fn set_apu_debug_capture_enabled(&mut self, enabled: bool) {
        self.io.apu.debug_capture_enabled = enabled;
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.io.apu.sample_generation_enabled = enabled;
    }

    pub fn set_apu_enabled(&mut self, enabled: bool) {
        self.io.apu.apu_enabled = enabled;
    }

    pub fn apu_drain_samples_into(&mut self, target: &mut Vec<f32>) {
        self.io.apu.drain_samples_into(target);
    }

    pub fn apu_drain_samples(&mut self) -> Vec<f32> {
        self.io.apu.drain_samples()
    }

    pub fn apu_channel_snapshot(&self) -> crate::hardware::apu::ApuChannelSnapshot {
        self.io.apu.channel_snapshot()
    }

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; 4]) {
        self.io.apu.set_channel_mutes(mutes);
    }

    pub fn apu_regs_snapshot(&self) -> [u8; 0x17] {
        self.io.apu.regs_snapshot()
    }

    pub fn apu_wave_ram_snapshot(&self) -> [u8; 0x10] {
        self.io.apu.wave_ram_snapshot()
    }

    pub fn apu_nr52_raw(&self) -> u8 {
        self.io.apu.nr52_raw()
    }

    pub fn apu_channel_debug_samples_ordered(&self, channel: usize) -> [f32; 512] {
        self.io.apu.channel_debug_samples_ordered(channel)
    }

    pub fn apu_master_debug_samples_ordered(&self) -> [f32; 512] {
        self.io.apu.master_debug_samples_ordered()
    }

    pub fn apu_channel_mutes(&self) -> [bool; 4] {
        self.io.apu.channel_mutes()
    }

    pub fn apply_bess_apu_io(&mut self, io_regs: &[u8]) {
        self.io.apu.apply_bess_io(io_regs);
    }

    pub(in crate::hardware) fn ppu_mode(&self) -> u8 {
        self.io.ppu.mode()
    }

    pub fn ppu_framebuffer(&self) -> &[u8] {
        &self.io.ppu.framebuffer
    }

    pub fn ppu_lcdc(&self) -> u8 {
        self.io.ppu.lcdc.bits()
    }

    pub fn ppu_stat(&self) -> u8 {
        self.io.ppu.stat
    }

    pub fn ppu_scy(&self) -> u8 {
        self.io.ppu.scy
    }

    pub fn ppu_scx(&self) -> u8 {
        self.io.ppu.scx
    }

    pub fn ppu_ly(&self) -> u8 {
        self.io.ppu.ly
    }

    pub fn ppu_lyc(&self) -> u8 {
        self.io.ppu.lyc
    }

    pub fn ppu_wy(&self) -> u8 {
        self.io.ppu.wy
    }

    pub fn ppu_wx(&self) -> u8 {
        self.io.ppu.wx
    }

    pub fn ppu_bgp(&self) -> u8 {
        self.io.ppu.bgp
    }

    pub fn ppu_obp0(&self) -> u8 {
        self.io.ppu.obp0
    }

    pub fn ppu_obp1(&self) -> u8 {
        self.io.ppu.obp1
    }

    pub fn ppu_cycles(&self) -> u64 {
        self.io.ppu.cycles
    }

    pub fn ppu_cgb_mode(&self) -> bool {
        self.io.ppu.cgb_mode
    }

    pub fn ppu_bg_palette_ram_snapshot(&self) -> [u8; 0x40] {
        self.io.ppu.bg_palette_ram
    }

    pub fn ppu_obj_palette_ram_snapshot(&self) -> [u8; 0x40] {
        self.io.ppu.obj_palette_ram
    }

    pub fn ppu_bg_palette_ram(&self) -> &[u8; 0x40] {
        &self.io.ppu.bg_palette_ram
    }

    pub fn ppu_obj_palette_ram(&self) -> &[u8; 0x40] {
        &self.io.ppu.obj_palette_ram
    }

    pub fn ppu_bg_palette_ram_mut(&mut self) -> &mut [u8; 0x40] {
        &mut self.io.ppu.bg_palette_ram
    }

    pub fn ppu_obj_palette_ram_mut(&mut self) -> &mut [u8; 0x40] {
        &mut self.io.ppu.obj_palette_ram
    }

    pub fn set_ppu_debug_flags(&mut self, bg: bool, window: bool, sprites: bool) {
        self.io.ppu.debug_flags.bg = bg;
        self.io.ppu.debug_flags.window = window;
        self.io.ppu.debug_flags.sprites = sprites;
    }

    pub fn set_ppu_sgb_mode(&mut self, enabled: bool) {
        self.io.ppu.set_sgb_mode(enabled);
    }

    pub fn apply_bess_ppu_registers(&mut self, io: &[u8], is_cgb: bool) {
        self.io.ppu.lcdc = crate::hardware::ppu::Lcdc::from_bits_truncate(io[0x40]);
        self.io.ppu.stat = io[0x41];
        self.io.ppu.scy = io[0x42];
        self.io.ppu.scx = io[0x43];
        self.io.ppu.ly = io[0x44];
        self.io.ppu.lyc = io[0x45];
        self.io.ppu.bgp = io[0x47];
        self.io.ppu.obp0 = io[0x48];
        self.io.ppu.obp1 = io[0x49];
        self.io.ppu.wy = io[0x4A];
        self.io.ppu.wx = io[0x4B];

        if is_cgb {
            self.io.ppu.cgb_mode = io[0x4C] & 0x04 == 0;
            self.io.ppu.bcps = io[0x68];
            self.io.ppu.ocps = io[0x6A];
        }
    }

    #[inline]
    pub(in crate::hardware) fn step_ppu(&mut self, system_t_cycles: u64) -> u8 {
        let cgb_mode = matches!(
            self.hardware_mode,
            HardwareMode::CGBNormal | HardwareMode::CGBDouble
        );
        self.io
            .ppu
            .step(system_t_cycles, &self.vram, &self.oam, cgb_mode)
    }

    pub fn timer_div(&self) -> u8 {
        self.io.timer.div()
    }

    pub fn timer_tima(&self) -> u8 {
        self.io.timer.tima()
    }

    pub fn timer_tma(&self) -> u8 {
        self.io.timer.tma()
    }

    pub fn timer_tac(&self) -> u8 {
        self.io.timer.tac()
    }

    #[inline]
    pub(in crate::hardware) fn step_timer(&mut self, t_cycles: u64) {
        if self.io.timer.step(t_cycles) {
            self.if_reg |= 0x04;
        }
    }

    #[inline]
    pub(in crate::hardware) fn step_serial(&mut self, t_cycles: u64) {
        if self.io.serial.step(t_cycles) {
            self.if_reg |= 0x08;
        }
    }

    #[inline]
    pub(in crate::hardware) fn step_apu(&mut self, system_t_cycles: u64) {
        self.io.apu.step(system_t_cycles);
    }

    pub fn serial_output_bytes(&self) -> &[u8] {
        self.io.serial.output_bytes()
    }

    pub fn sync_timer_serial_mode(&mut self) {
        self.io.timer.set_mode(self.hardware_mode);
        self.io.serial.set_mode(self.hardware_mode);
    }

    pub fn apply_bess_timer_serial_registers(&mut self, io: &[u8], mode: HardwareMode) {
        self.io.serial.write_sb(io[0x01]);
        self.io.serial.write_sc(io[0x02] & !0x80);
        self.io.serial.set_mode(mode);
        self.io.serial.reset_cycles();

        self.io.timer.apply_bess_div(io[0x04]);
        self.io.timer.set_tima_raw(io[0x05]);
        self.io.timer.set_tma_raw(io[0x06]);
        self.io.timer.set_tac_raw(io[0x07]);
        self.io.timer.set_mode(mode);
    }
}

#[cfg(test)]
mod tests;

