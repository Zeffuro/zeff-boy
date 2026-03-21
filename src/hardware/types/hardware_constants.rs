pub(crate) const SERIAL_SB: u16 = 0xFF01;
pub(crate) const SERIAL_SC: u16 = 0xFF02;
pub(crate) const TIMER_DIV: u16 = 0xFF04;
pub(crate) const TIMER_TIMA: u16 = 0xFF05;
pub(crate) const TIMER_TMA: u16 = 0xFF06;
pub(crate) const TIMER_TAC: u16 = 0xFF07;

pub(crate) const INTERRUPT_IF: u16 = 0xFF0F;

pub(crate) const PPU_LCDC: u16 = 0xFF40;
pub(crate) const PPU_STAT: u16 = 0xFF41;
pub(crate) const PPU_SCY: u16  = 0xFF42;
pub(crate) const PPU_SCX: u16  = 0xFF43;
pub(crate) const PPU_LY: u16   = 0xFF44;
pub(crate) const PPU_LYC: u16  = 0xFF45;
pub(crate) const PPU_BGP: u16  = 0xFF47;
pub(crate) const PPU_OBP0: u16 = 0xFF48;
pub(crate) const PPU_OBP1: u16 = 0xFF49;
pub(crate) const PPU_WY: u16   = 0xFF4A;
pub(crate) const PPU_WX: u16   = 0xFF4B;

pub(crate) const INTERRUPT_IE: u16 = 0xFFFF;

pub(crate) const INT_VBLANK: u16  = 0x0040;
pub(crate) const INT_STAT: u16    = 0x0048;
pub(crate) const INT_TIMER: u16   = 0x0050;
pub(crate) const INT_SERIAL: u16  = 0x0058;
pub(crate) const INT_JOYPAD: u16  = 0x0060;
