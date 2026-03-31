use crate::hardware::constants::*;

pub struct PpuRegisters {
    pub ctrl: u8,
    pub mask: u8,
    pub status: u8,
}

impl Default for PpuRegisters {
    fn default() -> Self {
        Self::new()
    }
}

impl PpuRegisters {
    pub fn new() -> Self {
        Self {
            ctrl: 0,
            mask: 0,
            status: 0,
        }
    }

    #[inline]
    pub fn nametable_select(&self) -> u16 {
        0x2000 + ((self.ctrl as u16) & 0x03) * 0x0400
    }

    #[inline]
    pub fn vram_increment(&self) -> u16 {
        if self.ctrl & CTRL_VRAM_INCREMENT != 0 {
            32
        } else {
            1
        }
    }

    #[inline]
    pub fn sprite_pattern_addr(&self) -> u16 {
        if self.ctrl & CTRL_SPRITE_PATTERN != 0 {
            0x1000
        } else {
            0x0000
        }
    }

    #[inline]
    pub fn bg_pattern_addr(&self) -> u16 {
        if self.ctrl & CTRL_BG_PATTERN != 0 {
            0x1000
        } else {
            0x0000
        }
    }

    #[inline]
    pub fn tall_sprites(&self) -> bool {
        self.ctrl & CTRL_TALL_SPRITES != 0
    }

    #[inline]
    pub fn nmi_enabled(&self) -> bool {
        self.ctrl & CTRL_NMI_ENABLE != 0
    }

    #[inline]
    pub fn show_bg(&self) -> bool {
        self.mask & MASK_SHOW_BG != 0
    }

    #[inline]
    pub fn show_sprites(&self) -> bool {
        self.mask & MASK_SHOW_SPRITES != 0
    }

    #[inline]
    pub fn rendering_enabled(&self) -> bool {
        self.show_bg() || self.show_sprites()
    }

    #[inline]
    pub fn show_bg_left8(&self) -> bool {
        self.mask & MASK_SHOW_BG_LEFT8 != 0
    }

    #[inline]
    pub fn show_sprites_left8(&self) -> bool {
        self.mask & MASK_SHOW_SPRITES_LEFT8 != 0
    }

    #[inline]
    pub fn greyscale(&self) -> bool {
        self.mask & MASK_GREYSCALE != 0
    }

    #[inline]
    pub fn emphasize_red(&self) -> bool {
        self.mask & MASK_EMPHASIZE_RED != 0
    }

    #[inline]
    pub fn emphasize_green(&self) -> bool {
        self.mask & MASK_EMPHASIZE_GREEN != 0
    }

    #[inline]
    pub fn emphasize_blue(&self) -> bool {
        self.mask & MASK_EMPHASIZE_BLUE != 0
    }

    pub fn set_vblank(&mut self) {
        self.status |= STATUS_VBLANK;
    }

    pub fn clear_vblank(&mut self) {
        self.status &= !STATUS_VBLANK;
    }

    pub fn set_sprite_zero_hit(&mut self) {
        self.status |= STATUS_SPRITE_ZERO_HIT;
    }

    pub fn clear_sprite_zero_hit(&mut self) {
        self.status &= !STATUS_SPRITE_ZERO_HIT;
    }

    pub fn set_sprite_overflow(&mut self) {
        self.status |= STATUS_SPRITE_OVERFLOW;
    }

    pub fn clear_sprite_overflow(&mut self) {
        self.status &= !STATUS_SPRITE_OVERFLOW;
    }
}
