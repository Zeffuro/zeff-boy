use super::Bus;
use crate::hardware::cartridge::{ChrFetchKind, Mirroring};
use crate::hardware::constants::{CTRL_NMI_ENABLE, STATUS_VBLANK};
use crate::hardware::ppu::{PRE_RENDER_SCANLINE, VBLANK_SCANLINE};

impl Bus {
    pub(super) fn ppu_read_register(&mut self, addr: u16) -> u8 {
        let ppu_cycles = self.ppu_cycles;
        let latch = self.ppu.io_latch_value_at(ppu_cycles);
        let (result, refresh_mask) = match addr {
            0x2002 => {
                let status = (self.ppu.regs.status & 0xE0) | (latch & 0x1F);
                if self.ppu.scanline == VBLANK_SCANLINE && self.ppu.dot == 1 {
                    self.ppu.suppress_vblank_edge = true;
                }
                self.ppu.regs.clear_vblank();
                self.ppu.nmi_output = false;
                self.ppu.w = false;
                (status, 0xE0)
            }
            0x2004 => {
                let mut data = self.ppu.oam[self.ppu.oam_addr as usize];
                if self.ppu.oam_addr & 0x03 == 0x02 {
                    data &= !0x1C;
                }
                (data, 0xFF)
            }
            0x2007 => {
                let addr = self.ppu.v & 0x3FFF;
                let old_v = self.ppu.v;
                let mut data = self.ppu.read_buffer;
                let mut refresh_mask = 0xFF;

                if addr >= 0x3F00 {
                    data = (self.ppu_bus_read(addr) & 0x3F) | (latch & 0xC0);
                    refresh_mask = 0x3F;
                    self.ppu.read_buffer =
                        self.ppu_bus_read_with_kind(addr - 0x1000, ChrFetchKind::CpuData);
                } else {
                    self.ppu.read_buffer = self.ppu_bus_read_with_kind(addr, ChrFetchKind::CpuData);
                }

                self.increment_v_after_ppudata_access();
                if old_v & 0x1000 != self.ppu.v & 0x1000 {
                    self.notify_ppu_address(self.ppu.v);
                }
                (data, refresh_mask)
            }
            _ => (latch, 0x00),
        };

        if refresh_mask == 0 {
            self.ppu.decay_io_latch_at(ppu_cycles);
        } else {
            self.ppu
                .refresh_io_latch_bits(result, refresh_mask, ppu_cycles);
        }
        result
    }

    pub(super) fn ppu_write_register(&mut self, addr: u16, val: u8) {
        self.ppu.refresh_io_latch_bits(val, 0xFF, self.ppu_cycles);
        match addr {
            0x2000 => {
                self.ppu.regs.ctrl = val;
                self.ppu.t = (self.ppu.t & 0xF3FF) | ((val as u16 & 0x03) << 10);
                self.ppu.nmi_output =
                    val & CTRL_NMI_ENABLE != 0 && self.ppu.regs.status & STATUS_VBLANK != 0;
            }
            0x2001 => {
                self.ppu.write_mask(val);
            }
            0x2003 => {
                self.ppu.oam_addr = val;
            }
            0x2004 => {
                self.write_oam_data(val);
            }
            0x2005 => {
                if !self.ppu.w {
                    self.ppu.t = (self.ppu.t & 0xFFE0) | ((val as u16) >> 3);
                    self.ppu.fine_x = val & 0x07;
                } else {
                    self.ppu.t = (self.ppu.t & 0x8C1F)
                        | ((val as u16 & 0x07) << 12)
                        | ((val as u16 & 0xF8) << 2);
                }
                self.ppu.w = !self.ppu.w;
            }
            0x2006 => {
                if !self.ppu.w {
                    self.ppu.t = (self.ppu.t & 0x00FF) | ((val as u16 & 0x3F) << 8);
                } else {
                    let old_v = self.ppu.v;
                    self.ppu.t = (self.ppu.t & 0xFF00) | val as u16;
                    self.ppu.v = self.ppu.t;
                    if old_v & 0x1000 != self.ppu.v & 0x1000 {
                        self.notify_ppu_address(self.ppu.v);
                    }
                }
                self.ppu.w = !self.ppu.w;
            }
            0x2007 => {
                let addr = self.ppu.v & 0x3FFF;
                let old_v = self.ppu.v;
                self.ppu_bus_write(addr, val);
                self.increment_v_after_ppudata_access();
                if old_v & 0x1000 != self.ppu.v & 0x1000 {
                    self.notify_ppu_address(self.ppu.v);
                }
            }
            _ => {}
        }
    }

    #[inline]
    fn rendering_active_scanline(&self) -> bool {
        self.ppu.rendering_enabled()
            && (self.ppu.scanline < 240 || self.ppu.scanline == PRE_RENDER_SCANLINE)
    }

    #[inline]
    pub(super) fn write_oam_data(&mut self, val: u8) {
        if self.rendering_active_scanline() {
            return;
        }

        self.ppu.oam[self.ppu.oam_addr as usize] = val;
        self.ppu.oam_addr = self.ppu.oam_addr.wrapping_add(1);
    }

    #[inline]
    fn increment_v_after_ppudata_access(&mut self) {
        if self.rendering_active_scanline() {
            self.ppu.increment_scroll_x();
            self.ppu.increment_scroll_y();
        } else {
            self.ppu.v = self.ppu.v.wrapping_add(self.ppu.regs.vram_increment());
        }
    }

    #[inline]
    pub fn ppu_bus_read(&mut self, addr: u16) -> u8 {
        self.ppu_bus_read_with_kind(addr, ChrFetchKind::Background)
    }

    #[inline]
    pub(super) fn ppu_bus_read_with_kind(&mut self, addr: u16, kind: ChrFetchKind) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if matches!(kind, ChrFetchKind::CpuData) {
                    self.notify_ppu_address(addr);
                }
                self.cartridge.chr_read_with_kind(addr, kind)
            }
            0x2000..=0x3EFF => {
                if let Some(val) = self
                    .cartridge
                    .ppu_nametable_read(addr, &self.ppu.nametable_ram)
                {
                    val
                } else {
                    let mirrored = self.mirror_nametable_addr(addr);
                    self.ppu.nametable_ram[mirrored]
                }
            }
            0x3F00..=0x3FFF => {
                let idx = Self::palette_index(addr);
                self.ppu.palette_ram[idx]
            }
            _ => 0,
        }
    }

    pub fn ppu_bus_write(&mut self, addr: u16, val: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                self.notify_ppu_address(addr);
                self.cartridge.chr_write(addr, val);
            }
            0x2000..=0x3EFF
                if !self
                    .cartridge
                    .ppu_nametable_write(addr, val, &mut self.ppu.nametable_ram) =>
            {
                let mirrored = self.mirror_nametable_addr(addr);
                self.ppu.nametable_ram[mirrored] = val;
            }
            0x3F00..=0x3FFF => {
                let idx = Self::palette_index(addr);
                self.ppu.palette_ram[idx] = val;
            }
            _ => {}
        }
    }

    #[inline]
    pub(super) fn notify_ppu_address(&mut self, addr: u16) {
        self.cartridge
            .notify_ppu_a12(addr & 0x1000 != 0, self.ppu_cycles);
    }

    fn mirror_nametable_addr(&self, addr: u16) -> usize {
        let addr = (addr - 0x2000) & 0x0FFF;
        match self.cartridge.mirroring() {
            Mirroring::Horizontal => {
                let table = (addr / 0x0400) & 0x03;
                let offset = addr & 0x03FF;
                let physical = match table {
                    0 | 1 => offset,
                    2 | 3 => 0x0400 + offset,
                    _ => unreachable!(),
                };
                physical as usize
            }
            Mirroring::Vertical => (addr & 0x07FF) as usize,
            Mirroring::SingleScreenLower => (addr & 0x03FF) as usize,
            Mirroring::SingleScreenUpper => (0x0400 + (addr & 0x03FF)) as usize,
            Mirroring::FourScreen => addr as usize,
        }
    }

    fn palette_index(addr: u16) -> usize {
        let mut idx = (addr & 0x1F) as usize;
        if idx >= 16 && idx.is_multiple_of(4) {
            idx -= 16;
        }
        idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cartridge::Cartridge;
    use crate::hardware::constants::{CTRL_NMI_ENABLE, OAM_DMA, STATUS_VBLANK};
    use crate::hardware::ppu::PPU_IO_LATCH_DECAY_PPU_CYCLES;

    fn test_bus() -> Bus {
        let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;

        let cart = Cartridge::load(&rom).expect("test ROM should load");
        Bus::new(cart, 44_100.0)
    }

    #[test]
    fn enabling_nmi_during_vblank_raises_edge_once() {
        let mut bus = test_bus();
        bus.ppu.regs.set_vblank();
        bus.begin_cpu_step_timing(zeff_emu_common::time::MasterTicks::ZERO);

        bus.cpu_write_timed(0x2000, CTRL_NMI_ENABLE);

        assert!(bus.ppu.nmi_output);
        let events = bus.finish_cpu_step_timing(1);

        assert!(events.nmi_raised);
        assert_eq!(events.first_nmi_cpu_cycle, Some(1));

        let events = bus.tick_peripherals(1);
        assert!(!events.nmi_raised);
    }

    #[test]
    fn disabling_nmi_between_vblank_edge_and_cpu_sample_cancels_it() {
        let mut bus = test_bus();
        bus.ppu.scanline = VBLANK_SCANLINE;
        bus.ppu.dot = 0;
        bus.ppu.regs.ctrl = CTRL_NMI_ENABLE;
        bus.begin_cpu_step_timing(zeff_emu_common::time::MasterTicks::ZERO);

        bus.cpu_write_timed(0x2000, 0);
        let events = bus.finish_cpu_step_timing(1);

        assert_eq!(bus.ppu.dot, 3);
        assert!(!bus.ppu.nmi_output);
        assert!(!events.nmi_raised);
    }

    #[test]
    fn reading_status_between_vblank_edge_and_cpu_sample_cancels_it() {
        let mut bus = test_bus();
        bus.ppu.scanline = VBLANK_SCANLINE;
        bus.ppu.dot = 0;
        bus.ppu.regs.ctrl = CTRL_NMI_ENABLE;
        bus.begin_cpu_step_timing(zeff_emu_common::time::MasterTicks::ZERO);

        let status = bus.cpu_read_timed(0x2002);
        let events = bus.finish_cpu_step_timing(1);

        assert_ne!(status & STATUS_VBLANK, 0);
        assert!(!bus.ppu.nmi_output);
        assert!(!events.nmi_raised);
    }

    #[test]
    fn timed_ppumask_access_occurs_after_two_ppu_dots() {
        let mut enable = test_bus();
        enable.ppu.scanline = PRE_RENDER_SCANLINE;
        enable.ppu.dot = 337;
        enable.ppu.odd_frame = true;
        enable.begin_cpu_step_timing(zeff_emu_common::time::MasterTicks::ZERO);

        enable.cpu_write_timed(0x2001, 0x18);

        assert_eq!(enable.ppu.scanline, PRE_RENDER_SCANLINE);
        assert_eq!(enable.ppu.dot, 340);
        assert!(enable.ppu.rendering_enabled());

        let mut disable = test_bus();
        disable.ppu.regs.mask = 0x18;
        disable.ppu.scanline = PRE_RENDER_SCANLINE;
        disable.ppu.dot = 337;
        disable.ppu.odd_frame = true;
        disable.begin_cpu_step_timing(zeff_emu_common::time::MasterTicks::ZERO);

        disable.cpu_write_timed(0x2001, 0);

        assert_eq!(disable.ppu.scanline, 0);
        assert_eq!(disable.ppu.dot, 0);
        assert!(!disable.ppu.rendering_enabled());
    }

    #[test]
    fn reading_ppustatus_clears_vblank_and_nmi_output() {
        let mut bus = test_bus();
        bus.ppu.regs.set_vblank();
        bus.ppu_write_register(0x2000, CTRL_NMI_ENABLE);

        let status = bus.ppu_read_register(0x2002);

        assert_ne!(status & STATUS_VBLANK, 0);
        assert_eq!(bus.ppu.regs.status & STATUS_VBLANK, 0);
        assert!(!bus.ppu.nmi_output);

        bus.ppu_write_register(0x2000, CTRL_NMI_ENABLE);
        assert!(!bus.ppu.nmi_output);
    }

    #[test]
    fn reading_ppustatus_on_vblank_edge_suppresses_flag_and_nmi() {
        let mut bus = test_bus();
        bus.ppu.scanline = VBLANK_SCANLINE;
        bus.ppu.dot = 1;
        bus.ppu_write_register(0x2000, CTRL_NMI_ENABLE);

        let status = bus.ppu_read_register(0x2002);
        assert_eq!(status & STATUS_VBLANK, 0);
        assert!(bus.ppu.suppress_vblank_edge);

        let events = bus.tick_peripherals(1);
        assert!(!events.nmi_raised);
        assert_eq!(bus.ppu.regs.status & STATUS_VBLANK, 0);
        assert!(!bus.ppu.nmi_output);
        assert!(!bus.ppu.suppress_vblank_edge);
    }

    #[test]
    fn reading_ppustatus_right_after_vblank_edge_clears_the_nmi_line() {
        let mut bus = test_bus();
        bus.ppu.scanline = VBLANK_SCANLINE;
        bus.ppu.dot = 2;
        bus.ppu.regs.set_vblank();
        bus.ppu.nmi_output = true;

        let status = bus.ppu_read_register(0x2002);

        assert_ne!(status & STATUS_VBLANK, 0);
        assert_eq!(bus.ppu.regs.status & STATUS_VBLANK, 0);
        assert!(!bus.ppu.suppress_vblank_edge);
        assert!(!bus.ppu.nmi_output);
    }

    #[test]
    fn ppu_io_latch_decays_to_zero() {
        let mut bus = test_bus();

        bus.ppu_write_register(0x2000, 0xFF);
        bus.ppu_cycles = PPU_IO_LATCH_DECAY_PPU_CYCLES;

        assert_eq!(bus.ppu_read_register(0x2000), 0x00);
    }

    #[test]
    fn reading_write_only_ppu_register_does_not_refresh_io_latch() {
        let mut bus = test_bus();

        bus.ppu_write_register(0x2000, 0xFF);
        bus.ppu_cycles = PPU_IO_LATCH_DECAY_PPU_CYCLES - 1;
        assert_eq!(bus.ppu_read_register(0x2000), 0xFF);

        bus.ppu_cycles = PPU_IO_LATCH_DECAY_PPU_CYCLES;
        assert_eq!(bus.ppu_read_register(0x2000), 0x00);
    }

    #[test]
    fn reading_ppustatus_refreshes_only_high_status_bits() {
        let mut bus = test_bus();

        bus.ppu_write_register(0x2000, 0x1F);
        bus.ppu_cycles = PPU_IO_LATCH_DECAY_PPU_CYCLES - 1;
        bus.ppu.regs.status = 0xE0;
        assert_eq!(bus.ppu_read_register(0x2002), 0xFF);

        bus.ppu_cycles = PPU_IO_LATCH_DECAY_PPU_CYCLES;

        assert_eq!(bus.ppu_read_register(0x2000), 0xE0);
    }

    #[test]
    fn palette_data_read_uses_io_latch_for_high_bits() {
        let mut bus = test_bus();

        bus.ppu_write_register(0x2000, 0xC0);
        bus.ppu.v = 0x3F00;
        bus.ppu.palette_ram[0] = 0x15;

        assert_eq!(bus.ppu_read_register(0x2007), 0xD5);

        bus.ppu_cycles = PPU_IO_LATCH_DECAY_PPU_CYCLES;
        bus.ppu.v = 0x3F00;

        assert_eq!(bus.ppu_read_register(0x2007), 0x15);
    }

    #[test]
    fn oam_attribute_read_clears_unused_bits_and_refreshes_io_latch() {
        let mut bus = test_bus();

        bus.ppu.oam_addr = 0x02;
        bus.ppu.oam[0x02] = 0xFF;

        assert_eq!(bus.ppu_read_register(0x2004), 0xE3);
        assert_eq!(bus.ppu_read_register(0x2000), 0xE3);
    }

    #[test]
    fn ppudata_access_during_rendering_increments_x_and_y_scroll() {
        let mut bus = test_bus();
        bus.ppu.regs.mask = 0x18;
        bus.ppu.regs.ctrl = 0x04;
        bus.ppu.scanline = 12;
        bus.ppu.dot = 120;
        bus.ppu.v = 0x0001;

        let _ = bus.ppu_read_register(0x2007);

        assert_eq!(bus.ppu.v, 0x1002);
    }

    #[test]
    fn ppudata_access_during_rendering_uses_scroll_wrapping_rules() {
        let mut bus = test_bus();
        bus.ppu.regs.mask = 0x18;
        bus.ppu.scanline = 12;
        bus.ppu.dot = 120;
        bus.ppu.v = 0x7000 | (29 << 5) | 31;

        bus.ppu_write_register(0x2007, 0x55);

        assert_eq!(bus.ppu.v, 0x0C00);
    }

    #[test]
    fn ppudata_access_outside_rendering_uses_ppuctrl_increment() {
        let mut bus = test_bus();
        bus.ppu.regs.ctrl = 0x04;
        bus.ppu.scanline = VBLANK_SCANLINE;
        bus.ppu.dot = 10;
        bus.ppu.v = 0x2000;

        bus.ppu_write_register(0x2007, 0x55);

        assert_eq!(bus.ppu.v, 0x2020);
    }

    #[test]
    fn oamdata_write_during_rendering_is_ignored() {
        let mut bus = test_bus();
        bus.ppu.regs.mask = 0x18;
        bus.ppu.scanline = 20;
        bus.ppu.dot = 100;
        bus.ppu.oam_addr = 0x02;
        bus.ppu.oam[0x02] = 0x11;

        bus.ppu_write_register(0x2004, 0xFF);

        assert_eq!(bus.ppu.oam[0x02], 0x11);
        assert_eq!(bus.ppu.oam_addr, 0x02);
    }

    #[test]
    fn oamdata_write_outside_rendering_still_writes_and_increments() {
        let mut bus = test_bus();
        bus.ppu.regs.mask = 0x18;
        bus.ppu.scanline = VBLANK_SCANLINE;
        bus.ppu.dot = 10;
        bus.ppu.oam_addr = 0x02;

        bus.ppu_write_register(0x2004, 0x77);

        assert_eq!(bus.ppu.oam[0x02], 0x77);
        assert_eq!(bus.ppu.oam_addr, 0x03);
    }

    #[test]
    fn oam_dma_during_rendering_does_not_replace_oam() {
        let mut bus = test_bus();
        bus.ppu.regs.mask = 0x18;
        bus.ppu.scanline = 20;
        bus.ppu.dot = 100;
        bus.ppu.oam_addr = 0x00;
        bus.ppu.oam = [0x11; 256];
        bus.ram.fill(0x77);

        bus.cpu_write(OAM_DMA, 0x00);

        assert_eq!(bus.ppu.oam, [0x11; 256]);
        assert_eq!(bus.ppu.oam_addr, 0x00);
    }
}
