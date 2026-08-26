use super::{Bus, OamCorruptionType};
use crate::hardware::ppu::{Lcdc, SCREEN_H};
use crate::hardware::types::constants::{
    ECHO_RAM_END, ECHO_RAM_OFFSET, ECHO_RAM_START, ERAM_END, ERAM_START, HRAM_END, HRAM_START,
    IE_ADDR, IO_END, IO_START, NOT_USABLE_END, NOT_USABLE_START, OAM_END, OAM_SIZE, OAM_START,
    PPU_DMA, ROM_BANK_0_START, ROM_BANK_N_END, VRAM_END, VRAM_START, WRAM_0_END, WRAM_0_START,
    WRAM_N_END, WRAM_N_START,
};
use crate::hardware::types::hardware_mode::HardwareMode;

const OAM_DMA_T_CYCLES_PER_BYTE: u64 = 4;
const HDMA_BLOCK_BYTES: u16 = 0x10;
const GDMA_BLOCK_T_CYCLES: u64 = 32;

impl Bus {
    pub fn step_oam_dma(&mut self, t_cycles: u64) {
        if !self.oam_dma_active && self.oam_dma_pending_source_base.is_none() {
            return;
        }

        self.oam_dma_t_cycle_accum = self.oam_dma_t_cycle_accum.wrapping_add(t_cycles);
        while self.oam_dma_t_cycle_accum >= OAM_DMA_T_CYCLES_PER_BYTE {
            self.oam_dma_t_cycle_accum -= OAM_DMA_T_CYCLES_PER_BYTE;

            if self.oam_dma_active && self.oam_dma_index < OAM_SIZE as u16 {
                let source_addr = self.oam_dma_source_base.wrapping_add(self.oam_dma_index);
                let value = self.read_oam_dma_source_byte(source_addr);
                self.oam[self.oam_dma_index as usize] = value;
                self.oam_dma_index += 1;

                if self.oam_dma_index >= OAM_SIZE as u16 {
                    self.oam_dma_active = false;
                }
            }

            if let Some(source_base) = self.oam_dma_pending_source_base.take() {
                self.oam_dma_source_base = source_base;
                self.oam_dma_index = 0;
                self.oam_dma_active = true;
            }

            if !self.oam_dma_active && self.oam_dma_pending_source_base.is_none() {
                self.oam_dma_t_cycle_accum = 0;
                break;
            }
        }
    }

    pub fn oam_dma_blocks_cpu_access(&self, addr: u16) -> bool {
        if !self.oam_dma_active {
            return false;
        }

        if matches!(addr, HRAM_START..=HRAM_END | PPU_DMA) {
            return false;
        }

        if matches!(
            addr,
            OAM_START..=OAM_END
                | NOT_USABLE_START..=NOT_USABLE_END
                | IO_START..=IO_END
                | IE_ADDR
        ) {
            return true;
        }

        cpu_oam_dma_bus(addr)
            .zip(cpu_oam_dma_bus(self.oam_dma_source_base))
            .is_some_and(|(cpu_bus, dma_bus)| cpu_bus == dma_bus)
    }

    pub fn cpu_read_byte_after_oam_dma_check(
        &mut self,
        addr: u16,
        blocked: bool,
        master_tick_offset: u64,
    ) -> u8 {
        if blocked {
            if self.trace_cpu_accesses {
                self.trace_cpu_read(master_tick_offset, addr, 0xFF);
            }
            return 0xFF;
        }

        self.cpu_read_byte_unblocked(addr, master_tick_offset)
    }

    pub fn cpu_write_byte_after_oam_dma_check(
        &mut self,
        addr: u16,
        value: u8,
        blocked: bool,
    ) -> u64 {
        if blocked {
            return 0;
        }

        self.cpu_write_byte_unblocked(addr, value, 0)
    }

    pub fn cpu_write_byte_after_oam_dma_and_oam_access_check(
        &mut self,
        addr: u16,
        value: u8,
        blocked_by_oam_dma: bool,
        oam_accessible_at_access: Option<bool>,
        master_tick_offset: u64,
    ) -> u64 {
        if blocked_by_oam_dma {
            return 0;
        }

        if let Some(oam_accessible) = oam_accessible_at_access {
            if !oam_accessible {
                self.maybe_trigger_oam_corruption(addr, OamCorruptionType::Write);
                return 0;
            }

            let old_value = self.oam[(addr - OAM_START) as usize];
            self.oam[(addr - OAM_START) as usize] = value;
            if self.traces_cpu_writes() {
                self.trace_cpu_write(master_tick_offset, addr, old_value, value, value);
            }
            return 0;
        }

        self.cpu_write_byte_unblocked(addr, value, master_tick_offset)
    }

    pub fn cpu_oam_write_accessible(&self) -> bool {
        self.io.ppu.cpu_oam_write_accessible()
    }

    pub(super) fn cpu_read_byte_unblocked(&mut self, addr: u16, master_tick_offset: u64) -> u8 {
        if (OAM_START..=NOT_USABLE_END).contains(&addr) && !self.io.ppu.cpu_oam_read_accessible() {
            self.maybe_trigger_oam_corruption(addr, OamCorruptionType::Read);
        }

        let value = self.read_byte(addr);
        if self.trace_cpu_accesses {
            self.trace_cpu_read(master_tick_offset, addr, value);
        }
        value
    }

    pub(super) fn cpu_write_byte_unblocked(
        &mut self,
        addr: u16,
        value: u8,
        master_tick_offset: u64,
    ) -> u64 {
        let old_value = if self.traces_cpu_writes() {
            self.read_byte(addr)
        } else {
            0
        };
        if (OAM_START..=NOT_USABLE_END).contains(&addr) && !self.io.ppu.cpu_oam_write_accessible() {
            self.maybe_trigger_oam_corruption(addr, OamCorruptionType::Write);
        }

        let extra_t_cycles = self.write_byte(addr, value);
        if self.traces_cpu_writes() {
            let new_value = self.read_byte(addr);
            self.trace_cpu_write(master_tick_offset, addr, old_value, value, new_value);
        }
        extra_t_cycles
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

        for i in 0..HDMA_BLOCK_BYTES {
            let src = source.wrapping_add(i);
            let dst = dest.wrapping_add(i);
            let value = self.read_byte(src);
            self.write_vram_dma(dst, value);
        }

        let source_end = source.wrapping_add(HDMA_BLOCK_BYTES);
        let dest_end = dest.wrapping_add(HDMA_BLOCK_BYTES);
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

    pub fn execute_hdma_transfer(&mut self, control: u8) -> u64 {
        if self.hdma_active && self.hdma_hblank && (control & 0x80) == 0 {
            self.hdma_active = false;
            self.hdma_hblank = false;
            self.hdma5 = 0x80 | (control & 0x7F);
            return 0;
        }

        self.hdma_blocks_left = (control & 0x7F).wrapping_add(1);
        self.hdma_active = true;
        self.hdma_hblank = (control & 0x80) != 0;

        if self.hdma_hblank {
            self.hdma5 = self.hdma_blocks_left.wrapping_sub(1) & 0x7F;
            if !self.io.ppu.lcdc.contains(Lcdc::LCD_ENABLE)
                || (self.io.ppu.ly < SCREEN_H as u8 && self.io.ppu.mode() == 0)
            {
                self.transfer_one_hdma_block();
            }
            return 0;
        }

        let blocks = self.hdma_blocks_left as u64;
        let per_block_t_cycles = match self.hardware_mode {
            HardwareMode::CGBDouble => GDMA_BLOCK_T_CYCLES * 2,
            _ => GDMA_BLOCK_T_CYCLES,
        };

        while self.hdma_active {
            self.transfer_one_hdma_block();
        }

        blocks * per_block_t_cycles
    }

    pub fn maybe_step_hblank_hdma(&mut self, previous_ppu_mode: u8, current_ppu_mode: u8) {
        if !self.is_cgb_mode() || !self.hdma_active || !self.hdma_hblank {
            return;
        }

        if !self.io.ppu.lcdc.contains(Lcdc::LCD_ENABLE) || self.io.ppu.ly >= SCREEN_H as u8 {
            return;
        }

        if previous_ppu_mode != 0 && current_ppu_mode == 0 {
            self.transfer_one_hdma_block();
        }
    }

    pub fn start_oam_dma(&mut self, value: u8) {
        self.oam_dma_pending_source_base = Some((value as u16) << 8);
        self.oam_dma_t_cycle_accum = 0;
    }

    fn read_oam_dma_source_byte(&self, addr: u16) -> u8 {
        if !self.is_cgb_mode() && (ECHO_RAM_START..=IE_ADDR).contains(&addr) {
            return self.read_byte(addr - ECHO_RAM_OFFSET);
        }

        self.read_byte(addr)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OamDmaBus {
    External,
    Video,
}

fn cpu_oam_dma_bus(addr: u16) -> Option<OamDmaBus> {
    match addr {
        ROM_BANK_0_START..=ROM_BANK_N_END
        | ERAM_START..=ERAM_END
        | WRAM_0_START..=WRAM_0_END
        | WRAM_N_START..=WRAM_N_END
        | ECHO_RAM_START..=ECHO_RAM_END => Some(OamDmaBus::External),
        VRAM_START..=VRAM_END => Some(OamDmaBus::Video),
        _ => None,
    }
}
