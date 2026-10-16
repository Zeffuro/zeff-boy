use super::constants::*;
use super::mode::{HorizontalMode, RasterGeometry};
use super::render::geometry::{pattern_index_mask, sprite_y, tile_height};
use super::render::{horizontal_mode, screen_bounds_in_mode};
use super::{VdpMemory, VdpPixel, VdpRegisterPipeline, VdpRenderError, VdpTileRow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpSprite {
    pub x: i16,
    pub y: i16,
    pub width: u8,
    pub height: u8,
    pub link: u8,
    pub name: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpSpritePattern {
    pub x: i16,
    columns: u8,
    first_row: u16,
    stride: u8,
    attributes: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpSpriteLine {
    pixels: [VdpPixel; H40_SCREEN_WIDTH],
    width: usize,
}

impl VdpSpriteLine {
    pub fn pixels(&self) -> &[VdpPixel] {
        &self.pixels[..self.width]
    }
}

impl VdpSpritePattern {
    pub fn columns(&self) -> u8 {
        self.columns
    }

    pub fn row_address(&self, column: u8) -> Option<u16> {
        if column >= self.columns {
            return None;
        }
        let column = if self.attributes & TILE_HFLIP != 0 {
            self.columns - 1 - column
        } else {
            column
        };
        let row =
            (self.first_row + u16::from(column) * u16::from(self.stride)) & SPRITE_ROW_ADDRESS_MASK;
        Some(row * TILE_ROW_BYTES as u16)
    }

    /// Reads current pattern VRAM using previously prepared row metadata.
    pub fn tile_row(
        &self,
        memory: &VdpMemory,
        column: u8,
    ) -> Result<Option<VdpTileRow>, VdpRenderError> {
        let Some(address) = self.row_address(column) else {
            return Ok(None);
        };
        let name = (address / TILE_BYTES as u16) | self.attributes;
        let row = (address % TILE_BYTES as u16 / TILE_ROW_BYTES as u16) as u8;
        memory.tile_row(name, row).map(Some)
    }
}

impl VdpSprite {
    pub(super) fn pattern_row(self, y: u16) -> Option<VdpSpritePattern> {
        self.pattern_row_with_geometry(RasterGeometry::progressive(y))
    }

    pub(super) fn pattern_row_with_geometry(
        self,
        geometry: RasterGeometry,
    ) -> Option<VdpSpritePattern> {
        let row = i32::from(geometry.source_y) - i32::from(self.y);
        if row < 0 || row >= i32::from(self.height) {
            return None;
        }
        let row = if self.name & TILE_VFLIP != 0 {
            i32::from(self.height) - 1 - row
        } else {
            row
        } as u16;
        Some(VdpSpritePattern {
            x: self.x,
            columns: self.width / TILE_WIDTH as u8,
            first_row: ((self.name & pattern_index_mask(geometry.interlaced))
                * tile_height(geometry.interlaced) as u16
                + row)
                & SPRITE_ROW_ADDRESS_MASK,
            stride: self.height,
            attributes: self.name & (TILE_HFLIP | TILE_PALETTE_MASK | TILE_PRIORITY),
        })
    }
}

impl VdpMemory {
    /// Composes one H32 row from ordered patterns, without masking history, overflow or timing.
    pub fn compose_sprite_line(
        &self,
        patterns: &[VdpSpritePattern],
    ) -> Result<VdpSpriteLine, VdpRenderError> {
        self.compose_sprite_line_in_mode(patterns, HorizontalMode::H32)
    }

    pub fn compose_sprite_line_with_registers(
        &self,
        registers: &VdpRegisterPipeline,
        patterns: &[VdpSpritePattern],
    ) -> Result<VdpSpriteLine, VdpRenderError> {
        self.compose_sprite_line_in_mode(patterns, horizontal_mode(registers)?)
    }

    pub(super) fn compose_sprite_line_in_mode(
        &self,
        patterns: &[VdpSpritePattern],
        mode: HorizontalMode,
    ) -> Result<VdpSpriteLine, VdpRenderError> {
        self.compose_sprite_line_bounded(patterns, mode, false)
    }

    pub(super) fn compose_raster_sprite_line(
        &self,
        patterns: &[VdpSpritePattern],
        mode: HorizontalMode,
    ) -> Result<VdpSpriteLine, VdpRenderError> {
        self.compose_sprite_line_bounded(patterns, mode, true)
    }

    fn compose_sprite_line_bounded(
        &self,
        patterns: &[VdpSpritePattern],
        mode: HorizontalMode,
        truncate: bool,
    ) -> Result<VdpSpriteLine, VdpRenderError> {
        if patterns.len() > usize::from(mode.sprites_per_line()) {
            return Err(VdpRenderError::UnsupportedSpriteRow);
        }
        let mut width = 0;
        for pattern in patterns {
            width += u16::from(pattern.columns) * TILE_WIDTH as u16;
            if !truncate && usize::from(width) > mode.width() {
                return Err(VdpRenderError::UnsupportedSpriteRow);
            }
            if pattern.x == -SPRITE_ORIGIN {
                return Err(VdpRenderError::UnsupportedSpriteMask);
            }
        }
        let mut line = VdpSpriteLine {
            pixels: [VdpPixel {
                color: 0,
                palette: 0,
                priority: false,
            }; H40_SCREEN_WIDTH],
            width: mode.width(),
        };
        let mut remaining = mode.width() / TILE_WIDTH;
        for pattern in patterns {
            for column in 0..pattern.columns {
                if remaining == 0 {
                    return Ok(line);
                }
                remaining -= 1;
                let row = pattern
                    .tile_row(self, column)?
                    .ok_or(VdpRenderError::UnsupportedSpriteRow)?;
                for (offset, color) in row.colors.into_iter().enumerate() {
                    let x = i32::from(pattern.x)
                        + i32::from(column) * TILE_WIDTH as i32
                        + offset as i32;
                    if !(0..mode.width() as i32).contains(&x) || color == 0 {
                        continue;
                    }
                    let pixel = &mut line.pixels[x as usize];
                    if pixel.is_transparent() {
                        *pixel = VdpPixel {
                            color,
                            palette: row.palette,
                            priority: row.priority,
                        };
                    }
                }
            }
        }
        Ok(line)
    }

    /// 64-KiB mode-5 entry; caller supplies SAT cache coherence.
    pub fn sprite_entry(
        &self,
        registers: &VdpRegisterPipeline,
        index: u8,
    ) -> Result<VdpSprite, VdpRenderError> {
        self.sprite_entry_with_layout(registers, index, None)
    }

    pub(super) fn sprite_entry_with_layout(
        &self,
        registers: &VdpRegisterPipeline,
        index: u8,
        cached: Option<[u16; 2]>,
    ) -> Result<VdpSprite, VdpRenderError> {
        self.sprite_entry_with_layout_and_geometry(
            registers,
            index,
            cached,
            RasterGeometry::progressive(0),
        )
    }

    pub(super) fn sprite_entry_with_layout_and_geometry(
        &self,
        registers: &VdpRegisterPipeline,
        index: u8,
        cached: Option<[u16; 2]>,
        geometry: RasterGeometry,
    ) -> Result<VdpSprite, VdpRenderError> {
        if index >= horizontal_mode(registers)?.sprite_count() {
            return Err(VdpRenderError::InvalidSpriteIndex(index));
        }
        let base = registers
            .sprite_table_base()
            .ok_or(VdpRenderError::UnknownRegister(SPRITE_TABLE_REGISTER))?;
        let address = usize::from(base) + usize::from(index) * SPRITE_ENTRY_BYTES;
        let vram = self.vram();
        let word =
            |offset| u16::from_be_bytes([vram[address + offset], vram[address + offset + 1]]);
        let [y, layout] =
            cached.unwrap_or_else(|| [word(SPRITE_Y_OFFSET), word(SPRITE_SIZE_LINK_OFFSET)]);
        Ok(VdpSprite {
            x: (word(SPRITE_X_OFFSET) & SPRITE_POSITION_MASK) as i16 - SPRITE_ORIGIN,
            y: sprite_y(y, geometry.interlaced),
            width: (((layout >> SPRITE_WIDTH_SHIFT) & SPRITE_SIZE_AXIS_MASK) + 1) as u8
                * TILE_WIDTH as u8,
            height: (((layout >> SPRITE_HEIGHT_SHIFT) & SPRITE_SIZE_AXIS_MASK) + 1) as u8
                * tile_height(geometry.interlaced) as u8,
            link: (layout & SPRITE_LINK_MASK) as u8,
            name: word(SPRITE_NAME_OFFSET),
        })
    }

    pub fn sample_sprite(
        &self,
        registers: &VdpRegisterPipeline,
        index: u8,
        x: u16,
        y: u16,
    ) -> Result<Option<VdpPixel>, VdpRenderError> {
        screen_bounds_in_mode(x, y, horizontal_mode(registers)?)?;
        self.sprite_pixel(self.sprite_entry(registers, index)?, x, y)
    }

    /// Requires a coherent row below mode limits, without masking, overflow or cyclic lists.
    pub fn sample_sprite_layer(
        &self,
        registers: &VdpRegisterPipeline,
        x: u16,
        y: u16,
    ) -> Result<Option<VdpPixel>, VdpRenderError> {
        let mode = horizontal_mode(registers)?;
        screen_bounds_in_mode(x, y, mode)?;
        let mut index = 0;
        let mut visited = 0u128;
        let mut count = 0;
        let mut pixels = 0;
        let mut selected = None;
        loop {
            let sprite = self.sprite_entry(registers, index)?;
            let bit = 1u128 << index;
            if visited & bit != 0 {
                return Err(VdpRenderError::SpriteLinkCycle(index));
            }
            visited |= bit;
            let row = i32::from(y) - i32::from(sprite.y);
            if row >= 0 && row < i32::from(sprite.height) {
                count += 1;
                pixels += u16::from(sprite.width);
                if count > mode.sprites_per_line() || usize::from(pixels) > mode.width() {
                    return Err(VdpRenderError::UnsupportedSpriteRow);
                }
                if sprite.x == -SPRITE_ORIGIN {
                    return Err(VdpRenderError::UnsupportedSpriteMask);
                }
                if selected.is_none() {
                    selected = self
                        .sprite_pixel(sprite, x, y)?
                        .filter(|pixel| !pixel.is_transparent());
                }
            }
            if sprite.link == 0 {
                return Ok(selected);
            }
            index = sprite.link;
        }
    }

    fn sprite_pixel(
        &self,
        sprite: VdpSprite,
        x: u16,
        y: u16,
    ) -> Result<Option<VdpPixel>, VdpRenderError> {
        let x = i32::from(x) - i32::from(sprite.x);
        if x < 0 || x >= i32::from(sprite.width) {
            return Ok(None);
        }
        let Some(pattern) = sprite.pattern_row(y) else {
            return Ok(None);
        };
        Ok(pattern
            .tile_row(self, (x as usize / TILE_WIDTH) as u8)?
            .map(|row| VdpPixel {
                color: row.colors[x as usize % TILE_WIDTH],
                palette: row.palette,
                priority: row.priority,
            }))
    }
}
