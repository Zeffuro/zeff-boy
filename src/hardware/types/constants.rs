pub(crate) use super::apu_constants::*;

pub(crate) const ROM_BANK_0_START: u16 = 0x0000;
pub(crate) const ROM_BANK_0_END: u16 = 0x3FFF;
pub(crate) const ROM_BANK_N_START: u16 = 0x4000;
pub(crate) const ROM_BANK_N_END: u16 = 0x7FFF;
pub(crate) const VRAM_START: u16 = 0x8000;
pub(crate) const VRAM_END: u16 = 0x9FFF;
pub(crate) const ERAM_START: u16 = 0xA000;
pub(crate) const ERAM_END: u16 = 0xBFFF;
pub(crate) const WRAM_0_START: u16 = 0xC000;
pub(crate) const WRAM_0_END: u16 = 0xCFFF;
pub(crate) const WRAM_N_START: u16 = 0xD000;
pub(crate) const WRAM_N_END: u16 = 0xDFFF;
pub(crate) const ECHO_RAM_START: u16 = 0xE000;
pub(crate) const ECHO_RAM_END: u16 = 0xFDFF;
pub(crate) const OAM_START: u16 = 0xFE00;
pub(crate) const OAM_END: u16 = 0xFE9F;
pub(crate) const NOT_USABLE_START: u16 = 0xFEA0;
pub(crate) const NOT_USABLE_END: u16 = 0xFEFF;
pub(crate) const IO_START: u16 = 0xFF00;
pub(crate) const IO_END: u16 = 0xFF7F;
pub(crate) const HRAM_START: u16 = 0xFF80;
pub(crate) const HRAM_END: u16 = 0xFFFE;
pub(crate) const IE_ADDR: u16 = 0xFFFF;

pub(crate) const ECHO_RAM_OFFSET: u16 = 0x2000;

pub(crate) const ROM_BANK_SIZE: usize = 0x4000;
pub(crate) const VRAM_SIZE: usize = 0x2000;
pub(crate) const ERAM_SIZE: usize = 0x2000;
pub(crate) const WRAM_SIZE: usize = 0x1000;
pub(crate) const OAM_SIZE: usize = 0xA0;
pub(crate) const IO_SIZE: usize = 0x80;
pub(crate) const HRAM_SIZE: usize = 0x7F;


pub(crate) const SERIAL_SB: u16 = 0xFF01;
pub(crate) const SERIAL_SC: u16 = 0xFF02;
pub(crate) const TIMER_DIV: u16 = 0xFF04;
pub(crate) const TIMER_TIMA: u16 = 0xFF05;
pub(crate) const TIMER_TMA: u16 = 0xFF06;
pub(crate) const TIMER_TAC: u16 = 0xFF07;

pub(crate) const INTERRUPT_IF: u16 = 0xFF0F;
pub(crate) const JOYP_P1: u16 = 0xFF00;

pub(crate) const PPU_LCDC: u16 = 0xFF40;
pub(crate) const PPU_STAT: u16 = 0xFF41;
pub(crate) const PPU_SCY: u16 = 0xFF42;
pub(crate) const PPU_SCX: u16 = 0xFF43;
pub(crate) const PPU_LY: u16 = 0xFF44;
pub(crate) const PPU_LYC: u16 = 0xFF45;
pub(crate) const PPU_DMA: u16 = 0xFF46;
pub(crate) const PPU_BGP: u16 = 0xFF47;
pub(crate) const PPU_OBP0: u16 = 0xFF48;
pub(crate) const PPU_OBP1: u16 = 0xFF49;
pub(crate) const PPU_WY: u16 = 0xFF4A;
pub(crate) const PPU_WX: u16 = 0xFF4B;
pub(crate) const CGB_KEY1: u16 = 0xFF4D;
pub(crate) const PPU_VBK: u16 = 0xFF4F;
pub(crate) const CGB_HDMA1: u16 = 0xFF51;
pub(crate) const CGB_HDMA2: u16 = 0xFF52;
pub(crate) const CGB_HDMA3: u16 = 0xFF53;
pub(crate) const CGB_HDMA4: u16 = 0xFF54;
pub(crate) const CGB_HDMA5: u16 = 0xFF55;
pub(crate) const CGB_BCPS: u16 = 0xFF68;
pub(crate) const CGB_BCPD: u16 = 0xFF69;
pub(crate) const CGB_OCPS: u16 = 0xFF6A;
pub(crate) const CGB_OCPD: u16 = 0xFF6B;

pub(crate) const CGB_SVBK: u16 = 0xFF70;

pub(crate) const INTERRUPT_IE: u16 = 0xFFFF;

pub(crate) const INT_VBLANK: u16 = 0x0040;
pub(crate) const INT_STAT: u16 = 0x0048;
pub(crate) const INT_TIMER: u16 = 0x0050;
pub(crate) const INT_SERIAL: u16 = 0x0058;
pub(crate) const INT_JOYPAD: u16 = 0x0060;

