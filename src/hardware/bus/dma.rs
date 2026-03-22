use super::Bus;
use crate::hardware::types::constants::{VRAM_END, VRAM_START};
use crate::hardware::types::hardware_mode::HardwareMode;

impl Bus {
    pub(crate) fn step_oam_dma(&mut self, t_cycles: u64) {
        if !self.oam_dma_active {
            return;
        }

        self.oam_dma_t_cycle_accum = self.oam_dma_t_cycle_accum.wrapping_add(t_cycles);
        while self.oam_dma_index < 160 {
            let needed_cycles = if self.oam_dma_index == 0 { 8 } else { 4 };
            if self.oam_dma_t_cycle_accum < needed_cycles {
                break;
            }
            self.oam_dma_t_cycle_accum -= needed_cycles;

            let source_addr = self.oam_dma_source_base.wrapping_add(self.oam_dma_index);
            let value = self.read_byte(source_addr);
            self.oam[self.oam_dma_index as usize] = value;
            self.oam_dma_index += 1;
        }

        if self.oam_dma_index >= 160 {
            self.oam_dma_active = false;
            self.oam_dma_t_cycle_accum = 0;
        }
    }

    fn write_vram_dma(&mut self, addr: u16, value: u8) {
        if (VRAM_START..=VRAM_END).contains(&addr) {
            let local = (addr - VRAM_START) as usize;
            let index = self.active_vram_offset() + local;
            self.vram[index] = value;
        }
    }

    fn hdma_source_addr(&self) -> u16 {
        (u16::from(self.hdma1) << 8) | u16::from(self.hdma2 & 0xF0)
    }

    fn hdma_dest_addr(&self) -> u16 {
        0x8000 | ((u16::from(self.hdma3 & 0x1F) << 8) | u16::from(self.hdma4 & 0xF0))
    }

    fn transfer_one_hdma_block(&mut self) {
        if !self.hdma_active || self.hdma_blocks_left == 0 {
            return;
        }

        let source = self.hdma_source_addr();
        let dest = self.hdma_dest_addr();

        for i in 0..0x10u16 {
            let src = source.wrapping_add(i);
            let dst = dest.wrapping_add(i);
            let value = self.read_byte(src);
            self.write_vram_dma(dst, value);
        }

        let source_end = source.wrapping_add(0x10);
        let dest_end = dest.wrapping_add(0x10);
        self.hdma1 = (source_end >> 8) as u8;
        self.hdma2 = (source_end as u8) & 0xF0;
        self.hdma3 = ((dest_end >> 8) as u8) & 0x1F;
        self.hdma4 = (dest_end as u8) & 0xF0;
        self.hdma_blocks_left = self.hdma_blocks_left.saturating_sub(1);

        if self.hdma_blocks_left == 0 {
            self.hdma_active = false;
            self.hdma_hblank = false;
            self.hdma5 = 0xFF;
        } else {
            self.hdma5 = self.hdma_blocks_left.wrapping_sub(1) & 0x7F;
        }
    }

    pub(crate) fn execute_hdma_transfer(&mut self, control: u8) -> u64 {
        if self.hdma_active && self.hdma_hblank && (control & 0x80) == 0 {
            self.hdma_active = false;
            self.hdma_hblank = false;
            self.hdma5 = 0x80 | (self.hdma_blocks_left.saturating_sub(1) & 0x7F);
            return 0;
        }

        self.hdma_blocks_left = (control & 0x7F).wrapping_add(1);
        self.hdma_active = true;
        self.hdma_hblank = (control & 0x80) != 0;

        if self.hdma_hblank {
            self.hdma5 = self.hdma_blocks_left.wrapping_sub(1) & 0x7F;
            return 0;
        }

        let blocks = self.hdma_blocks_left as u64;
        let per_block_t_cycles = match self.hardware_mode {
            HardwareMode::CGBDouble => 64,
            _ => 32,
        };

        while self.hdma_active {
            self.transfer_one_hdma_block();
        }

        blocks * per_block_t_cycles
    }

    pub(crate) fn maybe_step_hblank_hdma(&mut self, previous_ppu_mode: u8, current_ppu_mode: u8) {
        if !self.is_cgb_mode() || !self.hdma_active || !self.hdma_hblank {
            return;
        }

        if self.io.ppu.lcdc & 0x80 == 0 || self.io.ppu.ly >= 144 {
            return;
        }

        if previous_ppu_mode != 0 && current_ppu_mode == 0 {
            self.transfer_one_hdma_block();
        }
    }

    pub(crate) fn start_oam_dma(&mut self, value: u8) {
        self.oam_dma_source_base = (value as u16) << 8;
        self.oam_dma_index = 0;
        self.oam_dma_t_cycle_accum = 0;
        self.oam_dma_active = true;
    }
}
