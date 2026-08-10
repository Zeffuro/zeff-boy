use crate::emulator::Emulator;
use crate::hardware::cartridge::{BackupKind, RomHeader};
use crate::hardware::cpu::{CpuMode, CpuState, FetchedInstruction};

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
