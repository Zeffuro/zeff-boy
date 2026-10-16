use super::constants::*;
use super::mode::{HorizontalMode, RasterGeometry};
use super::render::geometry::{sprite_y, tile_height};
use super::render::{horizontal_mode, screen_bounds_with_geometry};
use super::{
    VdpMemory, VdpRegisterPipeline, VdpRenderError, VdpSprite, VdpSpritePattern, VdpWrite,
    VdpWriteError,
};
mod state;

/// Unknown until writes populate it; invalidate after raw memory/FIFO operations bypass it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpSpriteCache {
    entries: [[Option<u16>; SPRITE_CACHED_WORDS]; H40_SPRITE_COUNT as usize],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpSpriteRow {
    indices: [u8; H40_SPRITES_PER_LINE as usize],
    len: usize,
    y: u16,
    interlaced: bool,
}

impl VdpSpriteRow {
    pub fn indices(&self) -> &[u8] {
        &self.indices[..self.len]
    }

    /// Requires a stable cache; excludes changed-cache out-of-row behavior and fetch timing.
    pub fn prepare_sprite(
        &self,
        cache: &VdpSpriteCache,
        memory: &VdpMemory,
        registers: &VdpRegisterPipeline,
        slot: u8,
    ) -> Result<Option<VdpSpritePattern>, VdpRenderError> {
        let Some(&index) = self.indices().get(usize::from(slot)) else {
            return Ok(None);
        };
        let geometry = RasterGeometry {
            physical_y: 0,
            source_y: self.y,
            interlaced: self.interlaced,
        };
        let sprite = cache.sprite_entry_with_geometry(memory, registers, index, geometry)?;
        sprite
            .pattern_row_with_geometry(geometry)
            .map(Some)
            .ok_or(VdpRenderError::UnsupportedSpriteRow)
    }
}

impl Default for VdpSpriteCache {
    fn default() -> Self {
        Self {
            entries: [[None; SPRITE_CACHED_WORDS]; H40_SPRITE_COUNT as usize],
        }
    }
}

impl VdpSpriteCache {
    pub(super) fn observe_dma_byte(&mut self, r5: Option<u8>, address: u32, mode: HorizontalMode) {
        let Some(r5) = r5 else {
            self.invalidate();
            return;
        };
        if let Some(offset) = address.checked_sub(u32::from(r5) << SPRITE_TABLE_SHIFT)
            && offset < u32::from(mode.sprite_count()) * SPRITE_ENTRY_BYTES as u32
        {
            let field = offset as usize % SPRITE_ENTRY_BYTES;
            if field < SPRITE_CACHED_WORDS * NAME_ENTRY_BYTES {
                self.entries[offset as usize / SPRITE_ENTRY_BYTES][field / NAME_ENTRY_BYTES] = None;
            }
        }
    }

    /// Validates a stable, acyclic H32 list for y in 0..256, without scan timing.
    pub fn select_row(&self, y: u16) -> Result<VdpSpriteRow, VdpRenderError> {
        self.select_row_in_mode(RasterGeometry::progressive(y), false, HorizontalMode::H32)
    }

    pub fn select_row_with_registers(
        &self,
        registers: &VdpRegisterPipeline,
        y: u16,
    ) -> Result<VdpSpriteRow, VdpRenderError> {
        self.select_row_in_mode(
            RasterGeometry::progressive(y),
            false,
            horizontal_mode(registers)?,
        )
    }

    pub(super) fn select_complete_row_with_geometry(
        &self,
        geometry: RasterGeometry,
        mode: HorizontalMode,
    ) -> Result<VdpSpriteRow, VdpRenderError> {
        self.select_row_in_mode(geometry, true, mode)
    }

    fn select_row_in_mode(
        &self,
        geometry: RasterGeometry,
        reject_overflow: bool,
        mode: HorizontalMode,
    ) -> Result<VdpSpriteRow, VdpRenderError> {
        screen_bounds_with_geometry(0, geometry, mode)?;
        let y = geometry.source_y;
        let mut row = VdpSpriteRow {
            indices: [0; H40_SPRITES_PER_LINE as usize],
            len: 0,
            y,
            interlaced: geometry.interlaced,
        };
        let mut index = 0;
        let mut visited = 0u128;
        loop {
            let [position, layout] = self.known_entry_words(index, mode)?;
            let bit = 1u128 << index;
            if visited & bit != 0 {
                return Err(VdpRenderError::SpriteLinkCycle(index));
            }
            visited |= bit;
            let top = i32::from(sprite_y(position, geometry.interlaced));
            let height = (((layout >> SPRITE_HEIGHT_SHIFT) & SPRITE_SIZE_AXIS_MASK) + 1)
                * tile_height(geometry.interlaced) as u16;
            let offset = i32::from(y) - top;
            if offset >= 0 && offset < i32::from(height) {
                if row.len < usize::from(mode.sprites_per_line()) {
                    row.indices[row.len] = index;
                    row.len += 1;
                } else if reject_overflow {
                    return Err(VdpRenderError::UnsupportedSpriteRow);
                }
            }
            let link = (layout & SPRITE_LINK_MASK) as u8;
            if link == 0 {
                return Ok(row);
            }
            index = link;
        }
    }

    pub fn invalidate(&mut self) {
        self.entries = [[None; SPRITE_CACHED_WORDS]; H40_SPRITE_COUNT as usize];
    }

    pub fn entry_words(&self, index: u8) -> Result<[Option<u16>; 2], VdpRenderError> {
        self.entry_words_in_mode(index, HorizontalMode::H32)
    }

    pub fn entry_words_with_registers(
        &self,
        registers: &VdpRegisterPipeline,
        index: u8,
    ) -> Result<[Option<u16>; 2], VdpRenderError> {
        self.entry_words_in_mode(index, horizontal_mode(registers)?)
    }

    fn entry_words_in_mode(
        &self,
        index: u8,
        mode: HorizontalMode,
    ) -> Result<[Option<u16>; 2], VdpRenderError> {
        if index >= mode.sprite_count() {
            return Err(VdpRenderError::InvalidSpriteIndex(index));
        }
        self.entries
            .get(usize::from(index))
            .copied()
            .ok_or(VdpRenderError::InvalidSpriteIndex(index))
    }

    /// Completed even VRAM word with stable R5 and horizontal mode; no byte-latch timing.
    pub fn complete_write(
        &mut self,
        memory: &mut VdpMemory,
        registers: &VdpRegisterPipeline,
        write: VdpWrite,
    ) -> Result<(), VdpWriteError> {
        let command = write.command;
        if command.code & !COMMAND_CD4 != VRAM_WRITE_CODE
            || command.address > COMMAND_ADDRESS_MASK
            || command.address & u32::from(WORD_ODD_ADDRESS_BIT) != 0
        {
            return Err(VdpWriteError::UnsupportedCommand(command));
        }
        let r5 = registers.registers()[usize::from(SPRITE_TABLE_REGISTER)]
            .ok_or(VdpWriteError::UnknownSpriteTable)?;
        memory.write_word(command, write.data)?;
        self.record_write(
            r5,
            write,
            registers.horizontal_mode().unwrap_or(HorizontalMode::H32),
        );
        Ok(())
    }

    pub(super) fn observe_completed_write(
        &mut self,
        r5: Option<u8>,
        write: VdpWrite,
        mode: HorizontalMode,
    ) {
        let Some(r5) = r5 else {
            self.invalidate();
            return;
        };
        if write.command.address & u32::from(WORD_ODD_ADDRESS_BIT) == 0 {
            self.record_write(r5, write, mode);
        } else if write
            .command
            .address
            .checked_sub(u32::from(r5) << SPRITE_TABLE_SHIFT)
            .is_some_and(|offset| {
                offset < u32::from(mode.sprite_count()) * SPRITE_ENTRY_BYTES as u32
            })
        {
            self.invalidate();
        }
    }

    fn record_write(&mut self, r5: u8, write: VdpWrite, mode: HorizontalMode) {
        let command = write.command;
        let base = u32::from(r5) << SPRITE_TABLE_SHIFT;
        if let Some(offset) = command.address.checked_sub(base)
            && offset < u32::from(mode.sprite_count()) * SPRITE_ENTRY_BYTES as u32
        {
            let field = offset as usize % SPRITE_ENTRY_BYTES;
            if field < SPRITE_CACHED_WORDS * NAME_ENTRY_BYTES {
                let mask = if field == SPRITE_Y_OFFSET {
                    SPRITE_CACHED_POSITION_MASK
                } else {
                    SPRITE_LAYOUT_MASK
                };
                self.entries[offset as usize / SPRITE_ENTRY_BYTES][field / NAME_ENTRY_BYTES] =
                    Some(write.data & mask);
            }
        }
    }

    /// Cached Y/size/link with current VRAM X/name, without physical fetch timing.
    pub fn sprite_entry(
        &self,
        memory: &VdpMemory,
        registers: &VdpRegisterPipeline,
        index: u8,
    ) -> Result<VdpSprite, VdpRenderError> {
        self.sprite_entry_with_geometry(memory, registers, index, RasterGeometry::progressive(0))
    }

    fn sprite_entry_with_geometry(
        &self,
        memory: &VdpMemory,
        registers: &VdpRegisterPipeline,
        index: u8,
        geometry: RasterGeometry,
    ) -> Result<VdpSprite, VdpRenderError> {
        let [y, layout] = self.known_entry_words(index, horizontal_mode(registers)?)?;
        memory.sprite_entry_with_layout_and_geometry(registers, index, Some([y, layout]), geometry)
    }

    // Unknown cache fields use published VRAM during functional raster startup.
    pub(super) fn raster_sprite_entry_with_geometry(
        &self,
        memory: &VdpMemory,
        registers: &VdpRegisterPipeline,
        index: u8,
        geometry: RasterGeometry,
    ) -> Result<VdpSprite, VdpRenderError> {
        let [y, layout] = self.entry_words_with_registers(registers, index)?;
        let mut sprite =
            memory.sprite_entry_with_layout_and_geometry(registers, index, None, geometry)?;
        if let Some(y) = y {
            sprite.y = sprite_y(y, geometry.interlaced);
        }
        if let Some(layout) = layout {
            sprite.width = (((layout >> SPRITE_WIDTH_SHIFT) & SPRITE_SIZE_AXIS_MASK) + 1) as u8
                * TILE_WIDTH as u8;
            sprite.height = (((layout >> SPRITE_HEIGHT_SHIFT) & SPRITE_SIZE_AXIS_MASK) + 1) as u8
                * tile_height(geometry.interlaced) as u8;
            sprite.link = (layout & SPRITE_LINK_MASK) as u8;
        }
        Ok(sprite)
    }

    fn known_entry_words(
        &self,
        index: u8,
        mode: HorizontalMode,
    ) -> Result<[u16; 2], VdpRenderError> {
        let [Some(y), Some(layout)] = self.entry_words_in_mode(index, mode)? else {
            return Err(VdpRenderError::UnknownSpriteCache(index));
        };
        Ok([y, layout])
    }
}
