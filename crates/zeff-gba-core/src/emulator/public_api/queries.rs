use crate::emulator::Emulator;
use crate::hardware::cartridge::{BackupKind, RomHeader};
use crate::hardware::cpu::{CpuMode, CpuState, FetchedInstruction};
use zeff_emu_common::time::{
    ClockRate, FrameLifecycle, MachineTiming, MasterTicks, Reset, TimingSnapshot,
};

const MASTER_CLOCK_RATE: ClockRate =
    ClockRate::from_hz(crate::hardware::constants::CPU_CLOCK_HZ as u64);

impl Emulator {
    pub fn framebuffer(&self) -> &[u8] {
        self.bus.ppu.framebuffer()
    }

    pub fn framebuffer_dimensions(&self) -> (usize, usize) {
        self.bus.ppu.dimensions()
    }

    pub fn frame_ready(&self) -> bool {
        self.bus.ppu.frame_ready
    }

    pub fn clear_frame_ready(&mut self) {
        self.bus.ppu.frame_ready = false;
    }

    pub fn apu_debug_snapshot(&self) -> crate::hardware::apu::ApuDebugSnapshot {
        self.bus.apu.debug_snapshot()
    }

    pub fn dma_channels_snapshot(&self) -> [crate::hardware::dma::DmaChannel; 4] {
        self.bus.dma.channels()
    }

    pub fn cpu_state(&self) -> CpuState {
        self.cpu.state
    }

    pub fn cpu_pc(&self) -> u32 {
        self.cpu.pc()
    }

    pub fn cpu_registers(&self) -> [u32; 16] {
        self.cpu.regs
    }

    pub fn cpu_cpsr(&self) -> u32 {
        self.cpu.cpsr
    }

    pub fn cpu_mode(&self) -> CpuMode {
        self.cpu.mode()
    }

    pub fn cpu_thumb_state(&self) -> bool {
        self.cpu.thumb_state()
    }

    pub fn cpu_visible_pc(&self) -> u32 {
        self.cpu.visible_pc()
    }

    pub fn last_fetch(&self) -> Option<FetchedInstruction> {
        self.cpu.last_fetch
    }

    pub fn cpu_cycles(&self) -> u64 {
        self.cpu.cycles
    }

    pub fn timing_snapshot(&self) -> TimingSnapshot {
        <Self as MachineTiming>::timing_snapshot(self)
    }

    pub fn cartridge_header(&self) -> &RomHeader {
        self.bus.cartridge.header()
    }

    pub fn cartridge_rom_bytes(&self) -> &[u8] {
        self.bus.cartridge.rom()
    }

    pub fn backup_kind(&self) -> BackupKind {
        self.bus.cartridge.backup_kind()
    }

    pub fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }

    pub fn rom_offset_for_cpu_address(&self, address: u32) -> Option<usize> {
        let offset = match address {
            0x0800_0000..=0x09FF_FFFF => address - 0x0800_0000,
            0x0A00_0000..=0x0BFF_FFFF => address - 0x0A00_0000,
            0x0C00_0000..=0x0DFF_FFFF => address - 0x0C00_0000,
            _ => return None,
        };
        ((offset as usize) < self.bus.cartridge.rom().len()).then_some(offset as usize)
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn system_ram(&self) -> (&[u8], &[u8]) {
        self.bus.system_ram()
    }

    pub fn vram_snapshot(&self) -> &[u8] {
        &self.bus.vram
    }

    pub fn video_ram_snapshot(&self) -> &[u8] {
        self.vram_snapshot()
    }

    pub fn palette_ram_snapshot(&self) -> &[u8] {
        &self.bus.palette_ram
    }

    pub fn io_snapshot(&self) -> &[u8] {
        &self.bus.io
    }

    pub fn oam_snapshot(&self) -> &[u8] {
        &self.bus.oam
    }

    pub fn ppu_debug_snapshot(&self) -> crate::hardware::ppu::PpuDebugSnapshot {
        self.bus.ppu_debug_snapshot()
    }
}

impl MachineTiming for Emulator {
    fn timing_snapshot(&self) -> TimingSnapshot {
        TimingSnapshot::new(MasterTicks::new(self.cpu.cycles), MASTER_CLOCK_RATE)
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
