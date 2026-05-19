use super::apu::Apu;
use super::cartridge::Cartridge;
use super::constants::{EWRAM_SIZE, IO_SIZE, IWRAM_SIZE, OAM_SIZE, PALETTE_RAM_SIZE, VRAM_SIZE};
use super::dma::{DmaChannel, DmaController};
use super::keypad::{KEYINPUT, Keypad};
use super::ppu::{Ppu, PpuDebugSnapshot};
use super::timer::Timers;

#[derive(Clone, Debug)]
pub struct Bus {
    pub cartridge: Cartridge,
    pub ppu: Ppu,
    pub apu: Apu,
    pub keypad: Keypad,
    pub timers: Timers,
    pub dma: DmaController,
    pub ewram: Vec<u8>,
    pub iwram: Vec<u8>,
    pub io: Vec<u8>,
    pub palette_ram: Vec<u8>,
    pub vram: Vec<u8>,
    pub oam: Vec<u8>,
}

impl Bus {
    pub fn new(cartridge: Cartridge, sample_rate: u32) -> Self {
        Self {
            cartridge,
            ppu: Ppu::new(),
            apu: Apu::new(sample_rate),
            keypad: Keypad::new(),
            timers: Timers::default(),
            dma: DmaController::default(),
            ewram: vec![0; EWRAM_SIZE],
            iwram: vec![0; IWRAM_SIZE],
            io: vec![0; IO_SIZE],
            palette_ram: vec![0; PALETTE_RAM_SIZE],
            vram: vec![0; VRAM_SIZE],
            oam: vec![0; OAM_SIZE],
        }
    }

    pub fn read8(&self, addr: u32) -> u8 {
        match addr {
            0x0200_0000..=0x02FF_FFFF => self.ewram[(addr as usize) & (EWRAM_SIZE - 1)],
            0x0300_0000..=0x03FF_FFFF => self.iwram[(addr as usize) & (IWRAM_SIZE - 1)],
            0x0400_0000..=0x0400_03FF => self.io_read8(addr),
            0x0500_0000..=0x05FF_FFFF => self.palette_ram[(addr as usize) & (PALETTE_RAM_SIZE - 1)],
            0x0600_0000..=0x06FF_FFFF => self.vram[vram_index(addr)],
            0x0700_0000..=0x07FF_FFFF => self.oam[(addr as usize) & (OAM_SIZE - 1)],
            0x0800_0000..=0x0DFF_FFFF => self.cartridge.rom_read8(addr),
            0x0E00_0000..=0x0E00_FFFF => self.cartridge.backup_read8(addr),
            _ => 0xFF,
        }
    }

    pub fn read16(&self, addr: u32) -> u16 {
        let aligned = addr & !1;
        u16::from_le_bytes([self.read8(aligned), self.read8(aligned + 1)])
    }

    pub fn read32(&self, addr: u32) -> u32 {
        let aligned = addr & !3;
        u32::from_le_bytes([
            self.read8(aligned),
            self.read8(aligned + 1),
            self.read8(aligned + 2),
            self.read8(aligned + 3),
        ])
    }

    pub fn write8(&mut self, addr: u32, value: u8) {
        match addr {
            0x0200_0000..=0x02FF_FFFF => self.ewram[(addr as usize) & (EWRAM_SIZE - 1)] = value,
            0x0300_0000..=0x03FF_FFFF => self.iwram[(addr as usize) & (IWRAM_SIZE - 1)] = value,
            0x0400_0000..=0x0400_03FF => self.io_write8(addr, value),
            0x0500_0000..=0x05FF_FFFF => {
                self.palette_ram[(addr as usize) & (PALETTE_RAM_SIZE - 1)] = value;
            }
            0x0600_0000..=0x06FF_FFFF => {
                let index = vram_index(addr);
                self.vram[index] = value;
            }
            0x0700_0000..=0x07FF_FFFF => self.oam[(addr as usize) & (OAM_SIZE - 1)] = value,
            0x0E00_0000..=0x0E00_FFFF => self.cartridge.backup_write8(addr, value),
            _ => {}
        }
    }

    pub fn write16(&mut self, addr: u32, value: u16) {
        let aligned = addr & !1;
        if matches!(aligned, 0x0400_0000..=0x0400_03FF) {
            self.io_write16(aligned, value);
            return;
        }
        let bytes = value.to_le_bytes();
        self.write8(aligned, bytes[0]);
        self.write8(aligned + 1, bytes[1]);
    }

    pub fn write32(&mut self, addr: u32, value: u32) {
        let aligned = addr & !3;
        if matches!(aligned, 0x0400_0000..=0x0400_03FF) {
            self.io_write16(aligned, value as u16);
            self.io_write16(aligned + 2, (value >> 16) as u16);
            return;
        }
        let bytes = value.to_le_bytes();
        for (i, byte) in bytes.into_iter().enumerate() {
            self.write8(aligned + i as u32, byte);
        }
    }

    pub fn step_cycles(&mut self, cycles: u32) {
        self.ppu.step_cycles(cycles);
    }

    pub fn render_frame(&mut self) {
        self.ppu
            .step_frame(&self.io, &self.palette_ram, &self.vram, &self.oam);
    }

    pub fn ppu_debug_snapshot(&self) -> PpuDebugSnapshot {
        self.ppu.debug_snapshot(&self.io)
    }

    pub fn system_ram(&self) -> (&[u8], &[u8]) {
        (&self.ewram, &self.iwram)
    }

    fn io_read8(&self, addr: u32) -> u8 {
        let offset = addr & 0x3FF;
        let value = match offset {
            0x100..=0x10F => {
                let timer = ((offset - 0x100) / 4) as usize;
                let control = (offset & 0x2) != 0;
                self.timers.read16(timer, control)
            }
            0x0B0..=0x0DF => {
                let rel = offset - 0x0B0;
                self.dma
                    .read16((rel / 12) as usize, ((rel % 12) / 2) as usize)
            }
            0x004 => {
                let mut dispstat = read_io16(&self.io, 0x004);
                if self.ppu.in_vblank() {
                    dispstat |= 1;
                } else {
                    dispstat &= !1;
                }
                dispstat
            }
            0x006 => self.ppu.vcount(),
            0x130 => self.keypad.read_keyinput(),
            0x132 => self.keypad.read_keycnt(),
            _ => {
                let index = offset as usize;
                u16::from_le_bytes([
                    self.io.get(index & !1).copied().unwrap_or(0),
                    self.io.get((index & !1) + 1).copied().unwrap_or(0),
                ])
            }
        };
        let shift = ((addr & 1) * 8) as u16;
        (value >> shift) as u8
    }

    fn io_write8(&mut self, addr: u32, value: u8) {
        let offset = (addr & 0x3FF) as usize;
        if offset < self.io.len() {
            self.io[offset] = value;
        }
        let aligned = (addr & !1) & 0x3FF;
        let existing = self.io_read8(aligned) as u16 | ((self.io_read8(aligned + 1) as u16) << 8);
        let value16 = if addr & 1 == 0 {
            (existing & 0xFF00) | u16::from(value)
        } else {
            (existing & 0x00FF) | (u16::from(value) << 8)
        };
        self.io_write16(0x0400_0000 | aligned, value16);
    }

    fn io_write16(&mut self, addr: u32, value: u16) {
        let offset = (addr & 0x3FF) as usize;
        if offset < self.io.len() {
            let bytes = value.to_le_bytes();
            self.io[offset] = bytes[0];
            if offset + 1 < self.io.len() {
                self.io[offset + 1] = bytes[1];
            }
        }

        match addr {
            0x0400_0100..=0x0400_010F => {
                let offset = addr & 0xF;
                let timer = (offset / 4) as usize;
                let control = (offset & 0x2) != 0;
                self.timers.write16(timer, control, value);
            }
            KEYINPUT => {}
            0x0400_0132 => self.keypad.write_keycnt(value),
            0x0400_00B0..=0x0400_00DF => {
                let rel = addr - 0x0400_00B0;
                let channel = (rel / 12) as usize;
                let reg = ((rel % 12) / 2) as usize;
                let old_control = self.dma.channel(channel).control;
                self.dma.write16(channel, reg, value);
                if reg == 5 && old_control & 0x8000 == 0 && value & 0x8000 != 0 {
                    self.try_run_immediate_dma(channel);
                }
            }
            _ => {}
        }
    }

    fn try_run_immediate_dma(&mut self, channel: usize) {
        let mut ch = self.dma.channel(channel);
        if (ch.control >> 12) & 0x3 != 0 {
            return;
        }
        self.run_dma(channel, &mut ch);
        if ch.control & (1 << 9) == 0 {
            ch.control &= !0x8000;
        }
        self.dma.set_channel(channel, ch);
    }

    fn run_dma(&mut self, channel: usize, ch: &mut DmaChannel) {
        let word = ch.control & (1 << 10) != 0;
        let unit = if word { 4 } else { 2 };
        let mut count = u32::from(ch.count);
        if count == 0 {
            count = if channel == 3 { 0x1_0000 } else { 0x4000 };
        }

        let dest_mode = (ch.control >> 5) & 0x3;
        let src_mode = (ch.control >> 7) & 0x3;
        let mut src = ch.source;
        let mut dst = ch.destination;
        for _ in 0..count {
            if word {
                let value = self.read32(src);
                self.write32(dst, value);
            } else {
                let value = self.read16(src);
                self.write16(dst, value);
            }
            src = step_dma_addr(src, src_mode, unit);
            dst = step_dma_addr(dst, dest_mode, unit);
        }
        ch.source = src;
        ch.destination = dst;
        ch.count = 0;
    }
}

fn read_io16(io: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([
        io.get(offset).copied().unwrap_or(0),
        io.get(offset + 1).copied().unwrap_or(0),
    ])
}

fn step_dma_addr(addr: u32, mode: u16, unit: u32) -> u32 {
    match mode {
        0 | 3 => addr.wrapping_add(unit),
        1 => addr.wrapping_sub(unit),
        2 => addr,
        _ => addr,
    }
}

fn vram_index(addr: u32) -> usize {
    let mut offset = ((addr - 0x0600_0000) as usize) & 0x1FFFF;
    if offset >= VRAM_SIZE {
        offset -= 0x8000;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cartridge::RomHeader;

    fn cartridge() -> Cartridge {
        let mut rom = vec![0; 0xC0];
        rom[0xB2] = 0x96;
        rom[0xA0..0xA4].copy_from_slice(b"TEST");
        Cartridge::load(&rom).unwrap()
    }

    #[test]
    fn ewram_mirrors() {
        let mut bus = Bus::new(cartridge(), 48_000);
        bus.write8(0x0204_0000, 0x42);
        assert_eq!(bus.read8(0x0200_0000), 0x42);
    }

    #[test]
    fn rom_reads_from_cartridge() {
        let bus = Bus::new(cartridge(), 48_000);
        assert_eq!(bus.read8(0x0800_00B2), 0x96);
        let _ = RomHeader::parse(bus.cartridge.rom()).unwrap();
    }

    #[test]
    fn mode3_render_reads_vram_pixels() {
        let mut bus = Bus::new(cartridge(), 48_000);
        bus.write16(0x0400_0000, 3);
        bus.write16(0x0600_0000, 0x001F);

        bus.render_frame();

        assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn mode0_text_bg_render_reads_tiles() {
        let mut bus = Bus::new(cartridge(), 48_000);
        bus.write16(0x0400_0000, 1 << 8);
        bus.write16(0x0400_0008, 1 << 8);
        bus.write16(0x0500_0002, 0x03E0);
        bus.write8(0x0600_0000, 0x11);
        bus.write16(0x0600_0800, 0);

        bus.render_frame();

        assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0xFF, 0x00, 0xFF]);
    }

    #[test]
    fn obj_render_draws_sprite_pixels() {
        let mut bus = Bus::new(cartridge(), 48_000);
        bus.write16(0x0400_0000, (1 << 6) | (1 << 12));
        bus.write16(0x0500_0202, 0x7C00);
        bus.write8(0x0601_0000, 0x11);
        bus.write16(0x0700_0000, 0);
        bus.write16(0x0700_0002, 0);
        bus.write16(0x0700_0004, 0);

        bus.render_frame();

        assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0x00, 0xFF, 0xFF]);
    }

    #[test]
    fn mode2_affine_bg_render_reads_tiles() {
        let mut bus = Bus::new(cartridge(), 48_000);
        bus.write16(0x0400_0000, 2 | (1 << 10));
        bus.write16(0x0400_000C, 1 << 8);
        bus.write16(0x0400_0020, 0x0100);
        bus.write16(0x0400_0026, 0x0100);
        bus.write16(0x0500_0002, 0x001F);
        bus.write8(0x0600_0000, 1);
        bus.write8(0x0600_0800, 0);

        bus.render_frame();

        assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn immediate_dma_copies_words() {
        let mut bus = Bus::new(cartridge(), 48_000);
        bus.write32(0x0200_0000, 0x1122_3344);
        bus.write32(0x0400_00B0, 0x0200_0000);
        bus.write32(0x0400_00B4, 0x0300_0000);
        bus.write16(0x0400_00B8, 1);
        bus.write16(0x0400_00BA, 0x8400);

        assert_eq!(bus.read32(0x0300_0000), 0x1122_3344);
    }
}
