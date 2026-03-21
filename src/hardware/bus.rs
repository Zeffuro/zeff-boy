use log::warn;
use anyhow::Result;
use crate::hardware::io::IO;
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::memory_constants::*;
use crate::hardware::types::hardware_constants::*;
use crate::hardware::cartridge::Cartridge;
pub(crate) struct Bus {
    pub(crate) cartridge: Cartridge,
    pub(crate) vram:[u8; VRAM_SIZE],              // 0x8000..0x9FFF
    pub(crate) wram_0: [u8; WRAM_SIZE],           // 0xC000..0xCFFF
    pub(crate) wram_n: [u8; WRAM_SIZE],           // 0xD000..0xDFFF
    pub(crate) oam: [u8; OAM_SIZE],               // 0xFE00..0xFE9F
    pub(crate) io_bank: [u8; IO_SIZE],            // 0xFF00..0xFF7F
    pub(crate) hram: [u8; HRAM_SIZE],             // 0xFF80..0xFFFE
    pub(crate) ie: u8,                            // 0xFFFF
    pub(crate) if_reg: u8,                        // 0xFF0F
    pub(crate) io: IO,
}

impl Bus {
    pub(crate) fn new(rom: Vec<u8>, header: &RomHeader) -> Result<Box<Self>> {
        let cartridge = Cartridge::new(rom, header);

        Ok(Box::new(Self {
            cartridge,
            vram: [0; VRAM_SIZE],
            wram_0: [0; WRAM_SIZE],
            wram_n: [0; WRAM_SIZE],
            oam: [0; OAM_SIZE],
            io_bank:[0; IO_SIZE],
            hram: [0; HRAM_SIZE],
            ie: 0,
            if_reg: 0xE1,
            io: IO::new(),
        }))
    }


    #[allow(unreachable_patterns)]
    pub(crate) fn read_byte(&self, addr: u16) -> u8 {
        match addr {
            ROM_BANK_0_START..=ROM_BANK_N_END => self.cartridge.read_rom(addr),
            VRAM_START..=VRAM_END             => self.vram[(addr - VRAM_START) as usize],
            ERAM_START..=ERAM_END             => self.cartridge.read_ram(addr),
            WRAM_0_START..=WRAM_0_END         => self.wram_0[(addr - WRAM_0_START) as usize],
            WRAM_N_START..=WRAM_N_END         => self.wram_n[(addr - WRAM_N_START) as usize],
            ECHO_RAM_START..=ECHO_RAM_END     => {
                let mirror_addr = addr - ECHO_RAM_OFFSET;
                self.read_byte(mirror_addr)
            }
            OAM_START..=OAM_END               => self.oam[(addr - OAM_START) as usize],
            NOT_USABLE_START..=NOT_USABLE_END => 0xFF,
            SERIAL_SB                         => self.io.serial.sb,
            SERIAL_SC                         => self.io.serial.sc,
            INTERRUPT_IF                      => self.if_reg,
            IO_START..=IO_END                 => self.read_io(addr),
            HRAM_START..=HRAM_END             => self.hram[(addr - HRAM_START) as usize],
            IE_ADDR                           => self.ie,
            _                                 => 0xFF,
        }
    }

    #[allow(unreachable_patterns)]
    pub(crate) fn write_byte(&mut self, addr: u16, value: u8) {
        match addr {
            ROM_BANK_0_START..=ROM_BANK_N_END => self.cartridge.write_rom(addr, value),
            VRAM_START..=VRAM_END             => self.vram[(addr - VRAM_START) as usize] = value,
            ERAM_START..=ERAM_END             => self.cartridge.write_ram(addr, value),
            WRAM_0_START..=WRAM_0_END         => self.wram_0[(addr - WRAM_0_START) as usize] = value,
            WRAM_N_START..=WRAM_N_END         => self.wram_n[(addr - WRAM_N_START) as usize] = value,
            ECHO_RAM_START..=ECHO_RAM_END     => {
                let mirror_addr = addr - ECHO_RAM_OFFSET;
                self.write_byte(mirror_addr, value);
            },
            OAM_START..=OAM_END               => self.oam[(addr - OAM_START) as usize] = value,
            NOT_USABLE_START..=NOT_USABLE_END => warn!("Attempted illegal write to ROM at address {:04X}", addr),
            IO_START..=IO_END                 => self.write_io(addr, value),
            HRAM_START..=HRAM_END             => self.hram[(addr - HRAM_START) as usize] = value,
            IE_ADDR                           => self.ie = value,
            _                                 => warn!("Attempted illegal write to UNKNOWN at address {:04X}", addr),
        }
    }

    fn read_io(&self, addr: u16) -> u8 {
        match addr {
            SERIAL_SB => self.io.serial.sb,
            SERIAL_SC => self.io.serial.sc,

            TIMER_DIV  => self.io.timer.div,
            TIMER_TIMA => self.io.timer.tima,
            TIMER_TMA  => self.io.timer.tma,
            TIMER_TAC  => self.io.timer.tac,

            INTERRUPT_IF => self.if_reg | 0xE0,

            PPU_LCDC => self.io.ppu.lcdc,
            PPU_STAT => self.io.ppu.stat | 0x80,
            PPU_SCY  => self.io.ppu.scy,
            PPU_SCX  => self.io.ppu.scx,
            PPU_LY   => self.io.ppu.ly,
            PPU_LYC  => self.io.ppu.lyc,
            PPU_WY   => self.io.ppu.wy,
            PPU_WX   => self.io.ppu.wx,
            PPU_BGP  => self.io.ppu.bgp,
            PPU_OBP0 => self.io.ppu.obp0,
            PPU_OBP1 => self.io.ppu.obp1,

            _ => self.io_bank[(addr - IO_START) as usize],
        }
    }

    fn write_io(&mut self, addr: u16, value: u8) {
        self.io_bank[(addr - IO_START) as usize] = value;

        match addr {
            SERIAL_SB => self.io.serial.sb = value,
            SERIAL_SC => self.io.serial.sc = value,

            TIMER_DIV => self.io.timer.reset_div(),
            TIMER_TIMA => self.io.timer.write_tima(value),
            TIMER_TMA  => self.io.timer.tma = value,
            TIMER_TAC  => self.io.timer.write_tac(value),

            INTERRUPT_IF => self.if_reg = value & 0x1F,

            PPU_LCDC => self.io.ppu.lcdc = value,
            PPU_STAT => self.io.ppu.stat = (self.io.ppu.stat & 0x07) | (value & 0xF8),
            PPU_SCY  => self.io.ppu.scy = value,
            PPU_SCX  => self.io.ppu.scx = value,
            PPU_LY   => self.io.ppu.ly = 0,
            PPU_LYC  => self.io.ppu.lyc = value,
            PPU_WY   => self.io.ppu.wy = value,
            PPU_WX   => self.io.ppu.wx = value,
            PPU_BGP  => self.io.ppu.bgp = value,
            PPU_OBP0 => self.io.ppu.obp0 = value,
            PPU_OBP1 => self.io.ppu.obp1 = value,

            _ => {}
        }
    }
}