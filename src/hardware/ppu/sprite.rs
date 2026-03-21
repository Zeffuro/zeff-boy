pub(crate) struct SpriteEntry {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) tile: u8,
    pub(crate) flags: u8,
    pub(crate) oam_index: usize,
}

impl SpriteEntry {
    pub(crate) fn from_oam(oam: &[u8], index: usize) -> Self {
        let base = index * 4;
        Self {
            y: oam.get(base).copied().unwrap_or(0) as i32 - 16,
            x: oam.get(base + 1).copied().unwrap_or(0) as i32 - 8,
            tile: oam.get(base + 2).copied().unwrap_or(0),
            flags: oam.get(base + 3).copied().unwrap_or(0),
            oam_index: index,
        }
    }

    pub(crate) fn flip_x(&self) -> bool {
        self.flags & 0x20 != 0
    }

    pub(crate) fn flip_y(&self) -> bool {
        self.flags & 0x40 != 0
    }

    pub(crate) fn bg_priority(&self) -> bool {
        self.flags & 0x80 != 0
    }

    pub(crate) fn palette_number(&self) -> u8 {
        (self.flags >> 4) & 1
    }

    pub(crate) fn cgb_obj_palette_index(&self) -> u8 {
        self.flags & 0x07
    }

    pub(crate) fn cgb_vram_bank(&self) -> usize {
        ((self.flags >> 3) & 0x01) as usize
    }
}

