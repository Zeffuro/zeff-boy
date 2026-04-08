use super::Bus;
use crate::hardware::cartridge::{ChrFetchKind, Mirroring};

impl Bus {
    pub(super) fn ppu_read_register(&mut self, addr: u16) -> u8 {
        let result = match addr {
            0x2002 => {
                let status = (self.ppu.regs.status & 0xE0) | (self.ppu.io_latch & 0x1F);
                self.ppu.regs.clear_vblank();
                self.ppu.w = false;
                status
            }
            0x2004 => self.ppu.oam[self.ppu.oam_addr as usize],
            0x2007 => {
                let addr = self.ppu.v & 0x3FFF;
                let mut data = self.ppu.read_buffer;

                if addr >= 0x3F00 {
                    data = self.ppu_bus_read(addr);
                    self.ppu.read_buffer = self.ppu_bus_read(addr - 0x1000);
                } else {
                    self.ppu.read_buffer = self.ppu_bus_read(addr);
                }

                self.ppu.v = self.ppu.v.wrapping_add(self.ppu.regs.vram_increment());
                data
            }
            _ => self.ppu.io_latch,
        };
        self.ppu.io_latch = result;
        result
    }

    pub(super) fn ppu_write_register(&mut self, addr: u16, val: u8) {
        self.ppu.io_latch = val;
        match addr {
            0x2000 => {
                self.ppu.regs.ctrl = val;
                self.ppu.t = (self.ppu.t & 0xF3FF) | ((val as u16 & 0x03) << 10);
            }
            0x2001 => {
                self.ppu.regs.mask = val;
            }
            0x2003 => {
                self.ppu.oam_addr = val;
            }
            0x2004 => {
                self.ppu.oam[self.ppu.oam_addr as usize] = val;
                self.ppu.oam_addr = self.ppu.oam_addr.wrapping_add(1);
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
                    self.ppu.t = (self.ppu.t & 0xFF00) | val as u16;
                    self.ppu.v = self.ppu.t;
                }
                self.ppu.w = !self.ppu.w;
            }
            0x2007 => {
                let addr = self.ppu.v & 0x3FFF;
                self.ppu_bus_write(addr, val);
                self.ppu.v = self.ppu.v.wrapping_add(self.ppu.regs.vram_increment());
            }
            _ => {}
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
            0x0000..=0x1FFF => self.cartridge.chr_read_with_kind(addr, kind),
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
            0x0000..=0x1FFF => self.cartridge.chr_write(addr, val),
            0x2000..=0x3EFF => {
                if !self
                    .cartridge
                    .ppu_nametable_write(addr, val, &mut self.ppu.nametable_ram)
                {
                    let mirrored = self.mirror_nametable_addr(addr);
                    self.ppu.nametable_ram[mirrored] = val;
                }
            }
            0x3F00..=0x3FFF => {
                let idx = Self::palette_index(addr);
                self.ppu.palette_ram[idx] = val;
            }
            _ => {}
        }
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
