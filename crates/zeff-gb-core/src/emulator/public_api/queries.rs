use super::super::Emulator;
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use crate::hardware::types::{CpuState, ImeState};
use zeff_emu_common::time::{
    ClockRate, FrameLifecycle, MachineTiming, MasterTicks, Reset, TimingSnapshot,
};

const MASTER_CLOCK_RATE: ClockRate = ClockRate::from_hz(4_194_304);

impl Emulator {
    pub fn has_boot_rom(&self) -> bool {
        self.bus.boot_rom_bytes().is_some()
    }

    pub fn boot_rom_enabled(&self) -> bool {
        self.bus.boot_rom_enabled()
    }

    pub fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn dmg_palette_preset(&self) -> crate::hardware::ppu::DmgPalettePreset {
        self.bus.ppu_dmg_palette_preset()
    }

    pub fn header(&self) -> &RomHeader {
        &self.header
    }

    pub fn cartridge_rom_bytes(&self) -> &[u8] {
        self.bus.cartridge.rom_bytes()
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

    pub fn cpu_pc(&self) -> u16 {
        self.cpu.pc
    }

    pub fn cpu_sp(&self) -> u16 {
        self.cpu.sp
    }

    pub fn cpu_cycles(&self) -> u64 {
        self.cpu.cycles
    }

    pub fn timing_snapshot(&self) -> TimingSnapshot {
        <Self as MachineTiming>::timing_snapshot(self)
    }

    pub fn cpu_a(&self) -> u8 {
        self.cpu.regs.a
    }

    pub fn cpu_f(&self) -> u8 {
        self.cpu.regs.f
    }

    pub fn cpu_b(&self) -> u8 {
        self.cpu.regs.b
    }

    pub fn cpu_c(&self) -> u8 {
        self.cpu.regs.c
    }

    pub fn cpu_d(&self) -> u8 {
        self.cpu.regs.d
    }

    pub fn cpu_e(&self) -> u8 {
        self.cpu.regs.e
    }

    pub fn cpu_h(&self) -> u8 {
        self.cpu.regs.h
    }

    pub fn cpu_l(&self) -> u8 {
        self.cpu.regs.l
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

    pub fn ppu_cycles(&self) -> u64 {
        self.bus.ppu_cycles()
    }

    pub fn ppu_lcdc(&self) -> u8 {
        self.bus.ppu_lcdc()
    }

    pub fn ppu_stat(&self) -> u8 {
        self.bus.ppu_stat()
    }

    pub fn ppu_ly(&self) -> u8 {
        self.bus.ppu_ly()
    }

    pub fn ppu_lyc(&self) -> u8 {
        self.bus.ppu_lyc()
    }

    pub fn serial_output_bytes(&self) -> &[u8] {
        self.bus.serial_output_bytes()
    }

    pub fn peek_byte(&self, addr: u16) -> u8 {
        self.bus.read_byte(addr)
    }

    pub fn peek_byte_raw(&self, addr: u16) -> u8 {
        self.bus.read_byte_raw(addr)
    }

    pub fn printer_latest_image(&self) -> Option<&[u8]> {
        self.bus.printer_latest_image()
    }

    pub fn printer_image_count(&self) -> usize {
        self.bus.printer_image_count()
    }

    pub fn take_printer_images(&mut self) -> Vec<Vec<u8>> {
        self.bus.take_printer_images()
    }

    pub fn printer_image_dimensions() -> (usize, usize) {
        crate::hardware::printer::GameboyPrinter::image_dimensions()
    }

    pub fn clear_printer_images(&mut self) {
        self.bus.clear_printer_images();
    }

    pub fn rumble_active(&self) -> bool {
        self.bus.cartridge.rumble_active()
    }
}

impl MachineTiming for Emulator {
    fn timing_snapshot(&self) -> TimingSnapshot {
        TimingSnapshot::new(MasterTicks::new(self.cycle_count), MASTER_CLOCK_RATE)
    }
}

impl Reset for Emulator {
    #[inline]
    fn reset(&mut self) {
        Emulator::reset(self);
    }
}

impl FrameLifecycle for Emulator {
    #[inline]
    fn step_frame(&mut self) {
        Emulator::step_frame(self);
    }

    #[inline]
    fn frame_count(&self) -> u64 {
        Emulator::frame_count(self)
    }
}
