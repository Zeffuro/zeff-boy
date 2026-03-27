use crate::hardware::bus::Bus;
use crate::hardware::cartridge::Cartridge;
use crate::hardware::cpu::Cpu;
use sha2::{Sha256, Digest};
use std::fmt;
use std::path::PathBuf;

mod runtime;
mod state_io;

pub use crate::hardware::constants::CPU_CYCLES_PER_FRAME;

pub struct Emulator {
    pub cpu: Cpu,
    pub bus: Bus,
    pub(crate) rom_path: PathBuf,
    pub rom_hash: [u8; 32],
    pub opcode_log: crate::debug::OpcodeLog,
    pub debug: crate::debug::DebugController,
}

impl Emulator {
    pub fn new(rom_data: &[u8], rom_path: PathBuf, sample_rate: f64) -> anyhow::Result<Self> {
        let cartridge = Cartridge::load(rom_data)?;
        let bus = Bus::new(cartridge, sample_rate);

        let rom_hash: [u8; 32] = Sha256::digest(rom_data).into();

        let mut emu = Self {
            cpu: Cpu::new(),
            bus,
            rom_path,
            rom_hash,
            opcode_log: crate::debug::OpcodeLog::new(),
            debug: crate::debug::DebugController::new(),
        };
        emu.reset();
        if let Some(path) = emu.try_load_battery_sram()? {
            log::info!("Loaded battery save from {}", path);
        }
        Ok(emu)
    }

    pub fn reset(&mut self) {
        self.cpu = Cpu::new();
        self.cpu.reset(&mut self.bus);
        self.opcode_log.clear();
        self.debug.clear_hits();
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.bus.ppu.framebuffer[..]
    }

    pub fn rom_path(&self) -> &std::path::Path {
        &self.rom_path
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

    pub fn encode_state(&self) -> anyhow::Result<Vec<u8>> {
        crate::save_state::encode_state(self)
    }

    pub fn load_state(&mut self, data: &[u8]) -> anyhow::Result<()> {
        crate::save_state::decode_state(self, data)?;
        self.opcode_log.clear();
        Ok(())
    }

    pub fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.load_state(&bytes)
    }

    pub fn save_state_slot(&self, slot: u8) -> anyhow::Result<String> {
        let path = crate::save_state::slot_path(self.rom_hash, slot)?;
        let bytes = self.encode_state()?;
        crate::save_state::write_state_bytes_to_file(&path, &bytes)?;
        Ok(path.display().to_string())
    }

    pub fn load_state_slot(&mut self, slot: u8) -> anyhow::Result<String> {
        let path = crate::save_state::slot_path(self.rom_hash, slot)?;
        self.load_state_from_path(&path)?;
        Ok(path.display().to_string())
    }

    pub fn load_state_from_path(&mut self, path: &std::path::Path) -> anyhow::Result<()> {
        let bytes = std::fs::read(path)
            .map_err(|e| anyhow::anyhow!("failed to read NES save state: {}: {e}", path.display()))?;
        self.load_state(&bytes)
    }
}

impl fmt::Debug for Emulator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NES Emulator")
            .field("cpu", &self.cpu)
            .field("bus", &self.bus)
            .field("opcode_log", &self.opcode_log)
            .field("debug", &self.debug)
            .finish_non_exhaustive()
    }
}
