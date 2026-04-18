mod cpu_io;
mod ppu_bus;
mod rendering;

use crate::cheats::NesCheatState;
use crate::hardware::apu::Apu;
use crate::hardware::cartridge::Cartridge;
use crate::hardware::constants::*;
use crate::hardware::controller::Controller;
use crate::hardware::ppu::{NES_PALETTE, NesPaletteMode, Ppu, apply_nes_palette_mode};
use std::fmt;

pub enum DebugTraceEvent {
    Read {
        addr: u16,
        value: u8,
    },
    Write {
        addr: u16,
        old_value: u8,
        new_value: u8,
    },
}

pub struct Bus {
    pub ram: [u8; RAM_SIZE],
    pub(crate) ppu: Ppu,
    pub apu: Apu,
    pub cartridge: Cartridge,
    pub controller1: Controller,
    pub controller2: Controller,

    pub(crate) ppu_cycles: u64,

    pub(crate) dma_stall_cycles: u64,

    pub(crate) cpu_odd_cycle: bool,
    pub(crate) cpu_open_bus: u8,
    pub game_genie: NesCheatState,
    pub palette_mode: NesPaletteMode,

    pub(crate) palette_lut: [[u8; 4]; 64],

    pub(crate) debug_trace_enabled: bool,
    pub(crate) debug_trace_events: Vec<DebugTraceEvent>,
}

impl Bus {
    pub fn new(cartridge: Cartridge, sample_rate: f64) -> Self {
        let palette_mode = NesPaletteMode::default();
        Self {
            ram: [0; RAM_SIZE],
            ppu: Ppu::new(),
            apu: Apu::new(sample_rate),
            cartridge,
            controller1: Controller::new(),
            controller2: Controller::new(),
            ppu_cycles: 0,
            dma_stall_cycles: 0,
            cpu_odd_cycle: false,
            cpu_open_bus: 0,
            game_genie: NesCheatState::new(),
            palette_mode,
            palette_lut: Self::build_palette_lut(palette_mode),
            debug_trace_enabled: false,
            debug_trace_events: Vec::new(),
        }
    }

    fn build_palette_lut(mode: NesPaletteMode) -> [[u8; 4]; 64] {
        let mut lut = [[0u8; 4]; 64];
        for (i, entry) in lut.iter_mut().enumerate() {
            let (r, g, b) = apply_nes_palette_mode(mode, NES_PALETTE[i]);
            *entry = [r, g, b, 0xFF];
        }
        lut
    }

    pub fn set_palette_mode(&mut self, mode: NesPaletteMode) {
        self.palette_mode = mode;
        self.palette_lut = Self::build_palette_lut(mode);
    }

    pub fn palette_mode(&self) -> NesPaletteMode {
        self.palette_mode
    }

    pub fn palette_color_rgba(&self, pal_idx: u8) -> [u8; 4] {
        self.palette_lut[(pal_idx & 0x3F) as usize]
    }

    pub fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.ram);
        self.ppu.write_state(w);
        self.apu.write_state(w);
        self.cartridge.write_state(w);
        self.controller1.write_state(w);
        self.controller2.write_state(w);
        w.write_u64(self.ppu_cycles);
        w.write_u8(self.cpu_open_bus);
    }

    pub fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.ram)?;
        self.ppu.read_state(r)?;
        self.apu.read_state(r)?;
        self.cartridge.read_state(r)?;
        self.controller1.read_state(r)?;
        self.controller2.read_state(r)?;
        self.ppu_cycles = r.read_u64()?;
        self.cpu_open_bus = r.read_u8()?;
        Ok(())
    }

    fn dmc_dma_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & RAM_MIRROR_MASK) as usize],
            0x4020..=0xFFFF => self.cartridge.cpu_read(addr),
            _ => 0,
        }
    }

    pub fn tick_peripherals(&mut self, cpu_cycles: u64) -> bool {
        let ppu_dots = cpu_cycles * 3;
        let mut nmi_raised = false;
        for _ in 0..ppu_dots {
            self.ppu_render_dot();
            if self.ppu.tick() {
                nmi_raised = true;
            }
            self.ppu_cycles += 1;
        }
        for _ in 0..cpu_cycles {
            self.apu.expansion_audio = self.cartridge.audio_output();
            self.apu.tick();

            if self.apu.dmc.needs_dma() {
                let addr = self.apu.dmc.dma_address();
                let byte = self.dmc_dma_read(addr);
                self.apu.dmc.fill_sample_buffer(byte);
                let base = if self.cpu_odd_cycle { 4 } else { 3 };
                let conflict = if self.dma_stall_cycles > 0 { 1 } else { 0 };
                self.dma_stall_cycles += base + conflict;
            }

            self.cartridge.clock_cpu();
        }
        nmi_raised
    }
}

impl fmt::Debug for Bus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bus")
            .field("ppu", &self.ppu)
            .field("apu", &self.apu)
            .field("mirroring", &self.cartridge.mirroring())
            .finish_non_exhaustive()
    }
}
