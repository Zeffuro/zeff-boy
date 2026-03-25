pub use super::apu_constants::*;

pub const ROM_BANK_0_START: u16 = 0x0000;
pub const ROM_BANK_N_END: u16 = 0x7FFF;
pub const VRAM_START: u16 = 0x8000;
pub const VRAM_END: u16 = 0x9FFF;
pub const ERAM_START: u16 = 0xA000;
pub const ERAM_END: u16 = 0xBFFF;
pub const WRAM_0_START: u16 = 0xC000;
pub const WRAM_0_END: u16 = 0xCFFF;
pub const WRAM_N_START: u16 = 0xD000;
pub const WRAM_N_END: u16 = 0xDFFF;
pub const ECHO_RAM_START: u16 = 0xE000;
pub const ECHO_RAM_END: u16 = 0xFDFF;
pub const OAM_START: u16 = 0xFE00;
pub const OAM_END: u16 = 0xFE9F;
pub const NOT_USABLE_START: u16 = 0xFEA0;
pub const NOT_USABLE_END: u16 = 0xFEFF;
pub const IO_START: u16 = 0xFF00;
pub const IO_END: u16 = 0xFF7F;
pub const HRAM_START: u16 = 0xFF80;
pub const HRAM_END: u16 = 0xFFFE;
pub const IE_ADDR: u16 = 0xFFFF;

pub const ECHO_RAM_OFFSET: u16 = 0x2000;

pub const VRAM_SIZE: usize = 0x2000;
pub const WRAM_SIZE: usize = 0x1000;
pub const OAM_SIZE: usize = 0xA0;
pub const IO_SIZE: usize = 0x80;
pub const HRAM_SIZE: usize = 0x7F;

pub const SERIAL_SB: u16 = 0xFF01;
pub const SERIAL_SC: u16 = 0xFF02;
pub const TIMER_DIV: u16 = 0xFF04;
pub const TIMER_TIMA: u16 = 0xFF05;
pub const TIMER_TMA: u16 = 0xFF06;
pub const TIMER_TAC: u16 = 0xFF07;

pub const INTERRUPT_IF: u16 = 0xFF0F;
pub const JOYP_P1: u16 = 0xFF00;

pub const PPU_LCDC: u16 = 0xFF40;
pub const PPU_STAT: u16 = 0xFF41;
pub const PPU_SCY: u16 = 0xFF42;
pub const PPU_SCX: u16 = 0xFF43;
pub const PPU_LY: u16 = 0xFF44;
pub const PPU_LYC: u16 = 0xFF45;
pub const PPU_DMA: u16 = 0xFF46;
pub const PPU_BGP: u16 = 0xFF47;
pub const PPU_OBP0: u16 = 0xFF48;
pub const PPU_OBP1: u16 = 0xFF49;
pub const PPU_WY: u16 = 0xFF4A;
pub const PPU_WX: u16 = 0xFF4B;
pub const CGB_KEY1: u16 = 0xFF4D;
pub const PPU_VBK: u16 = 0xFF4F;
pub const CGB_HDMA1: u16 = 0xFF51;
pub const CGB_HDMA2: u16 = 0xFF52;
pub const CGB_HDMA3: u16 = 0xFF53;
pub const CGB_HDMA4: u16 = 0xFF54;
pub const CGB_HDMA5: u16 = 0xFF55;
pub const CGB_BCPS: u16 = 0xFF68;
pub const CGB_BCPD: u16 = 0xFF69;
pub const CGB_OCPS: u16 = 0xFF6A;
pub const CGB_OCPD: u16 = 0xFF6B;

pub const CGB_SVBK: u16 = 0xFF70;


pub const INT_VBLANK: u16 = 0x0040;
pub const INT_STAT: u16 = 0x0048;
pub const INT_TIMER: u16 = 0x0050;
pub const INT_SERIAL: u16 = 0x0058;
pub const INT_JOYPAD: u16 = 0x0060;
