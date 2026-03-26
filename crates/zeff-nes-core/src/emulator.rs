use crate::hardware::bus::Bus;
use crate::hardware::cartridge::Cartridge;
use crate::hardware::cpu::Cpu;
use std::fmt;
use std::path::PathBuf;

mod runtime;

pub use crate::hardware::constants::CPU_CYCLES_PER_FRAME;

pub struct Emulator {
    pub cpu: Cpu,
    pub bus: Bus,
    pub rom_path: PathBuf,
}

impl Emulator {
    pub fn new(rom_data: &[u8], rom_path: PathBuf, sample_rate: f64) -> anyhow::Result<Self> {
        let cartridge = Cartridge::load(rom_data)?;
        let bus = Bus::new(cartridge, sample_rate);
        let mut emu = Self {
            cpu: Cpu::new(),
            bus,
            rom_path,
        };
        emu.reset();
        Ok(emu)
    }

    pub fn reset(&mut self) {
        self.cpu = Cpu::new();
        self.cpu.reset(&mut self.bus);
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.bus.ppu.framebuffer
    }

    pub fn frame_ready(&self) -> bool {
        self.bus.ppu.frame_ready
    }

    pub fn clear_frame_ready(&mut self) {
        self.bus.ppu.frame_ready = false;
    }

    pub fn drain_audio_samples(&mut self) -> Vec<f32> {
        self.bus.apu.drain_samples()
    }
}

impl fmt::Debug for Emulator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NES Emulator")
            .field("cpu", &self.cpu)
            .field("bus", &self.bus)
            .finish_non_exhaustive()
    }
}


