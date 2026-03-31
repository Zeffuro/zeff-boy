use super::Bus;
use super::io_bus;
use crate::hardware::cartridge::Cartridge;
use crate::hardware::io::IO;
use crate::hardware::types::constants::{
    HRAM_SIZE, IO_SIZE, IO_START, OAM_SIZE, VRAM_SIZE, WRAM_SIZE,
};
use crate::save_state::{StateReader, StateReaderGbExt, StateWriter, StateWriterGbExt};
use anyhow::Result;

impl Bus {
    /// Build a 128-byte snapshot of all IO registers (FF00–FF7F) for BESS export.
    pub fn collect_io_register_snapshot(&self) -> [u8; 128] {
        let mut regs = [0u8; 128];
        for i in 0..128u16 {
            regs[i as usize] = io_bus::read_io(self, IO_START + i);
        }
        regs
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_hardware_mode(self.hardware_mode);
        self.cartridge.write_state(writer);
        writer.write_bytes(&self.vram);
        writer.write_bytes(&self.wram);
        writer.write_u8(self.vram_bank);
        writer.write_u8(self.wram_bank);
        writer.write_u8(self.key1);
        writer.write_u8(self.hdma1);
        writer.write_u8(self.hdma2);
        writer.write_u8(self.hdma3);
        writer.write_u8(self.hdma4);
        writer.write_u8(self.hdma5);
        writer.write_bool(self.hdma_active);
        writer.write_bool(self.hdma_hblank);
        writer.write_u8(self.hdma_blocks_left);
        writer.write_bool(self.oam_dma_active);
        writer.write_u16(self.oam_dma_source_base);
        writer.write_u16(self.oam_dma_index);
        writer.write_u64(self.oam_dma_t_cycle_accum);
        writer.write_bytes(&self.oam);
        writer.write_bytes(&self.io_bank);
        writer.write_bytes(&self.hram);
        writer.write_u8(self.ie);
        writer.write_u8(self.if_reg);
        self.io.write_state(writer);
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let hardware_mode = reader.read_hardware_mode()?;
        let cartridge = Cartridge::read_state(reader)?;

        let mut bus = Self {
            cartridge,
            hardware_mode,
            vram: vec![0u8; VRAM_SIZE * 2].into_boxed_slice(),
            wram: vec![0u8; WRAM_SIZE * 8].into_boxed_slice(),
            vram_bank: 0,
            wram_bank: 1,
            key1: 0x7E,
            hdma1: 0xFF,
            hdma2: 0xFF,
            hdma3: 0xFF,
            hdma4: 0xFF,
            hdma5: 0xFF,
            hdma_active: false,
            hdma_hblank: false,
            hdma_blocks_left: 0,
            oam_dma_active: false,
            oam_dma_source_base: 0,
            oam_dma_index: 0,
            oam_dma_t_cycle_accum: 0,
            oam: [0; OAM_SIZE],
            io_bank: [0; IO_SIZE],
            hram: [0; HRAM_SIZE],
            ie: 0,
            if_reg: 0,
            io: IO::new(),
            trace_cpu_accesses: false,
            cpu_read_trace: Vec::with_capacity(8),
            cpu_write_trace: Vec::with_capacity(4),
            game_genie_patches: Vec::new(),
        };

        reader.read_exact(&mut bus.vram)?;
        reader.read_exact(&mut bus.wram)?;
        bus.vram_bank = reader.read_u8()?;
        bus.wram_bank = reader.read_u8()?;
        bus.key1 = reader.read_u8()?;
        bus.hdma1 = reader.read_u8()?;
        bus.hdma2 = reader.read_u8()?;
        bus.hdma3 = reader.read_u8()?;
        bus.hdma4 = reader.read_u8()?;
        bus.hdma5 = reader.read_u8()?;
        bus.hdma_active = reader.read_bool()?;
        bus.hdma_hblank = reader.read_bool()?;
        bus.hdma_blocks_left = reader.read_u8()?;
        bus.oam_dma_active = reader.read_bool()?;
        bus.oam_dma_source_base = reader.read_u16()?;
        bus.oam_dma_index = reader.read_u16()?;
        bus.oam_dma_t_cycle_accum = reader.read_u64()?;
        reader.read_exact(&mut bus.oam)?;
        reader.read_exact(&mut bus.io_bank)?;
        reader.read_exact(&mut bus.hram)?;
        bus.ie = reader.read_u8()?;
        bus.if_reg = reader.read_u8()?;
        bus.io = IO::read_state(reader)?;

        bus.trace_cpu_accesses = false;
        bus.cpu_read_trace.clear();
        bus.cpu_write_trace.clear();
        Ok(bus)
    }
}
