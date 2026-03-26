pub const NMI_VECTOR_LO: u16 = 0xFFFA;
pub const NMI_VECTOR_HI: u16 = 0xFFFB;
pub const RESET_VECTOR_LO: u16 = 0xFFFC;
pub const RESET_VECTOR_HI: u16 = 0xFFFD;
pub const IRQ_VECTOR_LO: u16 = 0xFFFE;
pub const IRQ_VECTOR_HI: u16 = 0xFFFF;

pub const STACK_BASE: u16 = 0x0100;

pub const RAM_SIZE: usize = 0x800;
pub const RAM_MIRROR_MASK: u16 = 0x07FF;
pub const PPU_REG_MIRROR_MASK: u16 = 0x2007;

pub const PPU_REG_CTRL: u16 = 0x2000;
pub const PPU_REG_MASK: u16 = 0x2001;
pub const PPU_REG_STATUS: u16 = 0x2002;
pub const PPU_REG_OAM_ADDR: u16 = 0x2003;
pub const PPU_REG_OAM_DATA: u16 = 0x2004;
pub const PPU_REG_SCROLL: u16 = 0x2005;
pub const PPU_REG_ADDR: u16 = 0x2006;
pub const PPU_REG_DATA: u16 = 0x2007;

pub const OAM_DMA: u16 = 0x4014;
pub const APU_STATUS: u16 = 0x4015;
pub const CONTROLLER1: u16 = 0x4016;
pub const CONTROLLER2: u16 = 0x4017;

pub const NAMETABLE_BASE: u16 = 0x2000;
pub const ATTRIBUTE_TABLE_BASE: u16 = 0x23C0;
pub const PALETTE_START: u16 = 0x3F00;


pub const SCROLL_HORIZONTAL_MASK: u16 = 0x041F;
pub const SCROLL_VERTICAL_MASK: u16 = 0x7BE0;
pub const COARSE_X_MASK: u16 = 0x001F;
pub const COARSE_Y_MASK: u16 = 0x03E0;
pub const FINE_Y_MASK: u16 = 0x7000;
pub const NAMETABLE_X_BIT: u16 = 0x0400;
pub const NAMETABLE_Y_BIT: u16 = 0x0800;
pub const NAMETABLE_SELECT_MASK: u16 = 0x0C00;

pub const CTRL_VRAM_INCREMENT: u8 = 0x04;
pub const CTRL_SPRITE_PATTERN: u8 = 0x08;
pub const CTRL_BG_PATTERN: u8 = 0x10;
pub const CTRL_TALL_SPRITES: u8 = 0x20;
pub const CTRL_NMI_ENABLE: u8 = 0x80;

pub const MASK_GREYSCALE: u8 = 0x01;
pub const MASK_SHOW_BG_LEFT8: u8 = 0x02;
pub const MASK_SHOW_SPRITES_LEFT8: u8 = 0x04;
pub const MASK_SHOW_BG: u8 = 0x08;
pub const MASK_SHOW_SPRITES: u8 = 0x10;
pub const MASK_EMPHASIZE_RED: u8 = 0x20;
pub const MASK_EMPHASIZE_GREEN: u8 = 0x40;
pub const MASK_EMPHASIZE_BLUE: u8 = 0x80;

pub const STATUS_SPRITE_OVERFLOW: u8 = 0x20;
pub const STATUS_SPRITE_ZERO_HIT: u8 = 0x40;
pub const STATUS_VBLANK: u8 = 0x80;

pub const APU_CPU_CLOCK_NTSC: f64 = 1_789_773.0;
pub const FRAME_QUARTER_1: u64 = 3729;
pub const FRAME_QUARTER_2: u64 = 7457;
pub const FRAME_QUARTER_3: u64 = 11186;
pub const FRAME_4STEP_END: u64 = 14915;
pub const FRAME_5STEP_END: u64 = 18641;

pub const MIX_PULSE: f32 = 0.00752;
pub const MIX_TND_TRI: f32 = 0.00851;
pub const MIX_TND_NOISE: f32 = 0.00494;
pub const MIX_TND_DMC: f32 = 0.00335;

pub const CPU_CYCLES_PER_FRAME: u64 = 29781;
pub const NES_FRAME_DURATION_NS: u64 = 16_639_267;

