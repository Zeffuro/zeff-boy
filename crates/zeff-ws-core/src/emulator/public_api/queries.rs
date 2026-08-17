use crate::emulator::Emulator;
use crate::hardware::cartridge::{RomFooter, RomOrientation};
use crate::hardware::cpu::{CpuState, CpuTrap, FetchedInstruction};

impl Emulator {
    pub fn framebuffer(&self) -> &[u8] {
        self.bus.ppu.framebuffer()
    }

    pub fn framebuffer_dimensions(&self) -> (usize, usize) {
        self.bus.ppu.dimensions()
    }

    pub fn system_ram(&self) -> &[u8] {
        &self.bus.ram
    }

    pub fn vram_snapshot(&self) -> &[u8] {
        &self.bus.ram
    }

    pub fn video_ram_snapshot(&self) -> &[u8] {
        self.vram_snapshot()
    }

    pub fn frame_ready(&self) -> bool {
        self.bus.ppu.frame_ready
    }

    pub fn clear_frame_ready(&mut self) {
        self.bus.ppu.frame_ready = false;
    }

    pub fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn rom_crc32(&self) -> u32 {
        self.rom_crc32
    }

    pub fn cartridge_rom_bytes(&self) -> &[u8] {
        self.bus.cartridge.rom()
    }

    pub fn footer(&self) -> &RomFooter {
        self.bus.cartridge.footer()
    }

    pub fn preferred_orientation(&self) -> RomOrientation {
        self.footer().orientation()
    }

    pub fn cpu_state(&self) -> CpuState {
        self.cpu.state
    }

    pub fn cpu_pc(&self) -> u32 {
        self.cpu.pc()
    }

    pub fn rom_offset_for_cpu_address(&self, addr: u32) -> Option<usize> {
        self.bus.cartridge.rom_offset_for_address(addr)
    }

    pub fn rom_mapping_token(&self) -> u64 {
        self.bus.cartridge.rom_mapping_token()
    }

    pub fn cpu_registers(&self) -> [u16; 8] {
        self.cpu.regs
    }

    pub fn cpu_segments(&self) -> [u16; 4] {
        self.cpu.segments
    }

    pub fn cpu_ip(&self) -> u16 {
        self.cpu.ip
    }

    pub fn cpu_flags(&self) -> u16 {
        self.cpu.flags
    }

    pub fn cpu_cycles(&self) -> u64 {
        self.cpu.cycles
    }

    pub fn cpu_last_opcode(&self) -> u8 {
        self.cpu.last_opcode
    }

    pub fn last_fetch(&self) -> Option<FetchedInstruction> {
        self.cpu.last_fetch
    }

    pub fn last_trap(&self) -> Option<CpuTrap> {
        self.cpu.last_trap
    }

    pub fn io_peek8(&self, port: u16) -> u8 {
        self.bus.io_peek8(port)
    }

    pub fn io_read8(&mut self, port: u16) -> u8 {
        self.bus.io_read8(port)
    }

    pub fn io_write8(&mut self, port: u16, value: u8) {
        self.bus.io_write8(port, value);
    }

    pub fn ppu_debug_snapshot(&self) -> crate::hardware::ppu::PpuDebugSnapshot {
        self.bus.ppu_debug_snapshot()
    }

    pub fn apu_debug_snapshot(&self) -> crate::hardware::apu::ApuDebugSnapshot {
        self.bus.apu_debug_snapshot()
    }

    pub fn uart_debug_snapshot(&self) -> crate::hardware::bus::UartDebugSnapshot {
        self.bus.uart_debug_snapshot()
    }
}
