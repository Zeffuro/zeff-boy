use super::Bus;
use crate::hardware::cartridge::Cartridge;
use crate::hardware::io::IO;
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::constants::{HRAM_SIZE, IO_SIZE, OAM_SIZE, VRAM_SIZE, WRAM_SIZE};
use crate::hardware::types::hardware_mode::HardwareMode;
use anyhow::Result;

impl Bus {
    pub(crate) fn new(
        rom: Vec<u8>,
        header: &RomHeader,
        hardware_mode: HardwareMode,
    ) -> Result<Self> {
        let cartridge = Cartridge::new(rom, header);

        let mut bus = Self {
            cartridge,
            hardware_mode,
            vram: [0; VRAM_SIZE * 2],
            wram: [0; WRAM_SIZE * 8],
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
            if_reg: 0xE1,
            io: IO::new(),
            trace_cpu_accesses: false,
            cpu_read_trace: Vec::with_capacity(8),
            cpu_write_trace: Vec::with_capacity(4),
            game_genie_patches: Vec::new(),
        };

        bus.sync_timer_serial_mode();
        bus.io.ppu.set_sgb_mode(matches!(
            bus.hardware_mode,
            HardwareMode::SGB1 | HardwareMode::SGB2
        ));
        bus.key1 = match bus.hardware_mode {
            HardwareMode::CGBDouble => 0xFE,
            _ => 0x7E,
        };

        Ok(bus)
    }

    pub(super) fn is_cgb_mode(&self) -> bool {
        matches!(
            self.hardware_mode,
            HardwareMode::CGBNormal | HardwareMode::CGBDouble
        )
    }

    pub(super) fn active_vram_offset(&self) -> usize {
        if self.is_cgb_mode() {
            (self.vram_bank as usize & 0x01) * VRAM_SIZE
        } else {
            0
        }
    }

    pub(super) fn active_wram_bank(&self) -> usize {
        if self.is_cgb_mode() {
            let bank = (self.wram_bank & 0x07) as usize;
            if bank == 0 { 1 } else { bank }
        } else {
            1
        }
    }

    pub(crate) fn maybe_switch_cgb_speed(&mut self) -> bool {
        if !self.is_cgb_mode() || (self.key1 & 0x01) == 0 {
            return false;
        }

        self.hardware_mode = match self.hardware_mode {
            HardwareMode::CGBNormal => HardwareMode::CGBDouble,
            HardwareMode::CGBDouble => HardwareMode::CGBNormal,
            mode => mode,
        };

        self.sync_timer_serial_mode();
        self.key1 = match self.hardware_mode {
            HardwareMode::CGBDouble => 0xFE,
            _ => 0x7E,
        };
        true
    }
}
