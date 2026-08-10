use crate::emulator::{Emulator, dimensions_for_system};
use crate::hardware::bus::Bus;
use crate::hardware::cartridge::Sega8System;
use crate::hardware::cpu::{Cpu, CpuTrap};

impl Emulator {
    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    pub fn framebuffer_dimensions(&self) -> (usize, usize) {
        dimensions_for_system(self.system())
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn system_ram(&self) -> &[u8] {
        self.bus.work_ram()
    }

    pub fn vram_snapshot(&self) -> &[u8] {
        self.bus.vdp().vram()
    }

    pub fn video_ram_snapshot(&self) -> &[u8] {
        self.vram_snapshot()
    }

    pub fn palette_ram_snapshot(&self) -> &[u8] {
        self.bus.vdp().cram()
    }

    pub fn system(&self) -> Sega8System {
        self.bus.cartridge.system()
    }

    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    pub fn bus_mut(&mut self) -> &mut Bus {
        &mut self.bus
    }

    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    pub fn cpu_trap(&self) -> Option<CpuTrap> {
        self.cpu.trap()
    }

    pub fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }
}
