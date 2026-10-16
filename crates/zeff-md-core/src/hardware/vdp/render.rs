use super::constants::*;
use super::mode::{HorizontalMode, RasterGeometry};
use super::{VdpMemory, VdpRegisterPipeline, VdpSpriteLine};

mod color;
pub(super) mod geometry;
use geometry::{tile_height, vertical_mask};
mod raster;

struct BackgroundPixel {
    selected: VdpPixel,
    high_priority: bool,
    shadow_highlight: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpPlane {
    A,
    B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpRenderError {
    InvalidRow(u8),
    UnknownRegister(u8),
    UnsupportedPlaneSize(u8),
    UnsupportedScrollMode(u8),
    UnsupportedScrollCombination,
    UnsupportedRenderMode(u8),
    UnsupportedWindowScroll,
    InvalidScrollColumn(u8),
    InvalidSpriteIndex(u8),
    UnknownSpriteCache(u8),
    PendingSpriteWrites,
    SpriteLinkCycle(u8),
    UnsupportedSpriteMask,
    UnsupportedSpriteRow,
    CoordinateOutOfBounds { x: u16, y: u16 },
    PendingLineInputs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpTileRow {
    pub colors: [u8; TILE_WIDTH],
    pub palette: u8,
    pub priority: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpPixel {
    pub color: u8,
    pub palette: u8,
    pub priority: bool,
}

impl VdpPixel {
    pub fn is_transparent(self) -> bool {
        self.color == 0
    }
}

impl VdpMemory {
    /// Decodes an 8×8 mode-5 row without display fetch timing.
    pub fn tile_row(&self, name: u16, row: u8) -> Result<VdpTileRow, VdpRenderError> {
        self.tile_row_in_geometry(name, row, false)
    }

    /// Unscrolled coordinates in caller-supplied 64-KiB mode-5 noninterlace state.
    pub fn sample_plane(
        &self,
        registers: &VdpRegisterPipeline,
        plane: VdpPlane,
        x: u16,
        y: u16,
    ) -> Result<VdpPixel, VdpRenderError> {
        self.sample_plane_in_geometry(registers, plane, x, y, false)
    }

    fn sample_plane_in_geometry(
        &self,
        registers: &VdpRegisterPipeline,
        plane: VdpPlane,
        x: u16,
        y: u16,
        interlaced: bool,
    ) -> Result<VdpPixel, VdpRenderError> {
        let (base, index) = match plane {
            VdpPlane::A => (registers.plane_a_base(), PLANE_A_TABLE_REGISTER),
            VdpPlane::B => (registers.plane_b_base(), PLANE_B_TABLE_REGISTER),
        };
        let base = base.ok_or(VdpRenderError::UnknownRegister(index))?;
        let size = registers.registers()[usize::from(PLANE_SIZE_REGISTER)]
            .ok_or(VdpRenderError::UnknownRegister(PLANE_SIZE_REGISTER))?;
        let (width, height) = plane_dimensions(size)?;
        let (x, y) = (usize::from(x), usize::from(y));
        if x >= width * TILE_WIDTH || y >= height * tile_height(interlaced) {
            return Err(VdpRenderError::CoordinateOutOfBounds {
                x: x as u16,
                y: y as u16,
            });
        }
        self.sample_name(base, width, x, y, interlaced)
    }

    /// Some replaces plane A even when the window pixel is transparent.
    pub fn sample_window(
        &self,
        registers: &VdpRegisterPipeline,
        x: u16,
        y: u16,
    ) -> Result<Option<VdpPixel>, VdpRenderError> {
        self.sample_window_with_geometry(registers, x, RasterGeometry::progressive(y))
    }

    fn sample_window_with_geometry(
        &self,
        registers: &VdpRegisterPipeline,
        x: u16,
        geometry: RasterGeometry,
    ) -> Result<Option<VdpPixel>, VdpRenderError> {
        let y = geometry.source_y;
        let mode = horizontal_mode(registers)?;
        screen_bounds_with_geometry(x, geometry, mode)?;
        let horizontal = published_register(registers, WINDOW_X_REGISTER)?;
        let vertical = published_register(registers, WINDOW_Y_REGISTER)?;
        let split_x = usize::from(horizontal & WINDOW_POSITION_MASK) * WINDOW_X_STEP;
        let split_y =
            usize::from(vertical & WINDOW_POSITION_MASK) * tile_height(geometry.interlaced);
        let selected_x = (usize::from(x) >= split_x) == (horizontal & WINDOW_DIRECTION != 0);
        let selected_y = (usize::from(y) >= split_y) == (vertical & WINDOW_DIRECTION != 0);
        if !selected_x && !selected_y {
            return Ok(None);
        }
        let base = registers
            .window_base()
            .ok_or(VdpRenderError::UnknownRegister(WINDOW_TABLE_REGISTER))?;
        self.sample_name(
            base,
            mode.window_columns(),
            usize::from(x),
            usize::from(y),
            geometry.interlaced,
        )
        .map(Some)
    }

    /// Scroll offsets for an explicit fetch pair, without pixel-to-column mapping.
    pub fn scroll_offsets(
        &self,
        registers: &VdpRegisterPipeline,
        plane: VdpPlane,
        line: u8,
        column: u8,
    ) -> Result<(u16, u16), VdpRenderError> {
        self.scroll_offsets_with_geometry(
            registers,
            plane,
            RasterGeometry::progressive(u16::from(line)),
            column,
        )
    }

    fn scroll_offsets_with_geometry(
        &self,
        registers: &VdpRegisterPipeline,
        plane: VdpPlane,
        geometry: RasterGeometry,
        column: u8,
    ) -> Result<(u16, u16), VdpRenderError> {
        let line = geometry.physical_y;
        if column >= horizontal_mode(registers)?.scroll_columns() {
            return Err(VdpRenderError::InvalidScrollColumn(column));
        }
        let mode = published_register(registers, SCROLL_MODE_REGISTER)?;
        let line = match mode & HSCROLL_MODE_MASK {
            HSCROLL_FULL => 0,
            HSCROLL_CELL => usize::from(line) / usize::from(TILE_HEIGHT) * usize::from(TILE_HEIGHT),
            HSCROLL_LINE => usize::from(line),
            _ => return Err(VdpRenderError::UnsupportedScrollMode(mode)),
        };
        let base = registers
            .hscroll_table_base()
            .ok_or(VdpRenderError::UnknownRegister(HSCROLL_TABLE_REGISTER))?;
        let plane = match plane {
            VdpPlane::A => 0,
            VdpPlane::B => 1,
        };
        let address = usize::from(base) + line * SCROLL_PAIR_BYTES + plane * NAME_ENTRY_BYTES;
        let vram = self.vram();
        let horizontal =
            u16::from_be_bytes([vram[address], vram[address + 1]]) & SCROLL_OFFSET_MASK;
        let column = if mode & VSCROLL_COLUMN_MODE == 0 {
            0
        } else {
            usize::from(column)
        };
        let vertical =
            self.vsram()[column * SCROLL_PAIR_WORDS + plane] & vertical_mask(geometry.interlaced);
        Ok((horizontal, vertical))
    }

    /// Rejects combined fine horizontal and two-cell vertical scroll.
    pub fn sample_scrolled_plane(
        &self,
        registers: &VdpRegisterPipeline,
        plane: VdpPlane,
        x: u16,
        y: u16,
    ) -> Result<VdpPixel, VdpRenderError> {
        self.scrolled_plane(registers, plane, x, RasterGeometry::progressive(y), false)
    }

    fn scrolled_plane(
        &self,
        registers: &VdpRegisterPipeline,
        plane: VdpPlane,
        x: u16,
        geometry: RasterGeometry,
        raster: bool,
    ) -> Result<VdpPixel, VdpRenderError> {
        screen_bounds_with_geometry(x, geometry, horizontal_mode(registers)?)?;
        let mode = published_register(registers, SCROLL_MODE_REGISTER)?;
        let (horizontal, vertical) = self.scroll_offsets_with_geometry(
            registers,
            plane,
            geometry,
            (usize::from(x) / WINDOW_X_STEP) as u8,
        )?;
        if !raster
            && mode & VSCROLL_COLUMN_MODE != 0
            && usize::from(horizontal) % WINDOW_X_STEP != 0
        {
            return Err(VdpRenderError::UnsupportedScrollCombination);
        }
        let size = published_register(registers, PLANE_SIZE_REGISTER)?;
        let (width, height) = plane_dimensions(size)?;
        let width = width * TILE_WIDTH;
        let height = height * tile_height(geometry.interlaced);
        let source_x = x.wrapping_sub(horizontal) & (width as u16 - 1);
        let source_y = (usize::from(geometry.source_y) + usize::from(vertical)) & (height - 1);
        self.sample_plane_in_geometry(
            registers,
            plane,
            source_x,
            source_y as u16,
            geometry.interlaced,
        )
    }

    /// Packed RGB levels (0..14 per nibble); normal intensity matches CRAM encoding.
    pub fn sample_background(
        &self,
        registers: &VdpRegisterPipeline,
        x: u16,
        y: u16,
    ) -> Result<u16, VdpRenderError> {
        let background = self.background_pixel(registers, x, y)?;
        self.composite_color(registers, background, None)
    }

    /// Composes a coherent SAT snapshot without live sprite evaluation.
    pub fn sample_composite(
        &self,
        registers: &VdpRegisterPipeline,
        x: u16,
        y: u16,
    ) -> Result<u16, VdpRenderError> {
        let background = self.background_pixel(registers, x, y)?;
        self.composite_color(
            registers,
            background,
            self.sample_sprite_layer(registers, x, y)?,
        )
    }

    /// Combines completed sprite pixels with current background/CRAM, without output timing.
    pub fn sample_sprite_line(
        &self,
        registers: &VdpRegisterPipeline,
        line: &VdpSpriteLine,
        x: u16,
        y: u16,
    ) -> Result<u16, VdpRenderError> {
        self.sprite_line_color(registers, line, x, RasterGeometry::progressive(y), false)
    }

    pub(super) fn sample_sprite_line_with_geometry(
        &self,
        registers: &VdpRegisterPipeline,
        line: &VdpSpriteLine,
        x: u16,
        geometry: RasterGeometry,
    ) -> Result<u16, VdpRenderError> {
        self.sprite_line_color(registers, line, x, geometry, false)
    }

    #[cfg(test)]
    pub(super) fn raster_sprite_line_color(
        &self,
        registers: &VdpRegisterPipeline,
        line: &VdpSpriteLine,
        x: u16,
        y: u16,
    ) -> Result<u16, VdpRenderError> {
        self.raster_sprite_line_color_with_geometry(
            registers,
            line,
            x,
            RasterGeometry::progressive(y),
        )
    }

    pub(super) fn raster_sprite_line_color_with_geometry(
        &self,
        registers: &VdpRegisterPipeline,
        line: &VdpSpriteLine,
        x: u16,
        geometry: RasterGeometry,
    ) -> Result<u16, VdpRenderError> {
        self.sprite_line_color(registers, line, x, geometry, true)
    }

    fn sprite_line_color(
        &self,
        registers: &VdpRegisterPipeline,
        line: &VdpSpriteLine,
        x: u16,
        geometry: RasterGeometry,
        raster: bool,
    ) -> Result<u16, VdpRenderError> {
        let background = self.background_pixel_in_mode(registers, x, geometry, raster)?;
        let pixel =
            *line
                .pixels()
                .get(usize::from(x))
                .ok_or(VdpRenderError::CoordinateOutOfBounds {
                    x,
                    y: geometry.source_y,
                })?;
        self.composite_color(
            registers,
            background,
            (!pixel.is_transparent()).then_some(pixel),
        )
    }

    fn background_pixel(
        &self,
        registers: &VdpRegisterPipeline,
        x: u16,
        y: u16,
    ) -> Result<BackgroundPixel, VdpRenderError> {
        self.background_pixel_in_mode(registers, x, RasterGeometry::progressive(y), false)
    }

    fn background_pixel_in_mode(
        &self,
        registers: &VdpRegisterPipeline,
        x: u16,
        geometry: RasterGeometry,
        raster: bool,
    ) -> Result<BackgroundPixel, VdpRenderError> {
        let horizontal = horizontal_mode(registers)?;
        screen_bounds_with_geometry(x, geometry, horizontal)?;
        let mode = published_register(registers, DISPLAY_SIZE_REGISTER)?;
        let foreground = match self.sample_window_with_geometry(registers, x, geometry)? {
            Some(pixel) => pixel,
            None => {
                let window = published_register(registers, WINDOW_X_REGISTER)?;
                let split = usize::from(window & WINDOW_POSITION_MASK) * WINDOW_X_STEP;
                if !raster
                    && window & WINDOW_DIRECTION == 0
                    && split > 0
                    && split < horizontal.width()
                {
                    let (horizontal, _) =
                        self.scroll_offsets_with_geometry(registers, VdpPlane::A, geometry, 0)?;
                    if usize::from(horizontal) % WINDOW_X_STEP != 0 {
                        return Err(VdpRenderError::UnsupportedWindowScroll);
                    }
                }
                self.scrolled_plane(registers, VdpPlane::A, x, geometry, raster)?
            }
        };
        let background = self.scrolled_plane(registers, VdpPlane::B, x, geometry, raster)?;
        Ok(BackgroundPixel {
            selected: if foreground.is_transparent()
                || (!foreground.priority && background.priority && !background.is_transparent())
            {
                background
            } else {
                foreground
            },
            high_priority: foreground.priority || background.priority,
            shadow_highlight: mode & SHADOW_HIGHLIGHT_ENABLE != 0,
        })
    }
}

fn published_register(registers: &VdpRegisterPipeline, index: u8) -> Result<u8, VdpRenderError> {
    registers.registers()[usize::from(index)].ok_or(VdpRenderError::UnknownRegister(index))
}

pub(super) fn horizontal_mode(
    registers: &VdpRegisterPipeline,
) -> Result<HorizontalMode, VdpRenderError> {
    let Some(value) = registers.registers()[usize::from(DISPLAY_SIZE_REGISTER)] else {
        return Ok(HorizontalMode::H32);
    };
    HorizontalMode::from_register(value)
        .filter(|_| matches!(value & INTERLACE_MODE_MASK, 0 | 6))
        .ok_or(VdpRenderError::UnsupportedRenderMode(value))
}

pub(super) fn screen_bounds_in_mode(
    x: u16,
    y: u16,
    mode: HorizontalMode,
) -> Result<(), VdpRenderError> {
    if usize::from(x) >= mode.width() || usize::from(y) >= WINDOW_HEIGHT {
        Err(VdpRenderError::CoordinateOutOfBounds { x, y })
    } else {
        Ok(())
    }
}

fn plane_dimensions(value: u8) -> Result<(usize, usize), VdpRenderError> {
    let axis = |code| match code {
        0 => Some(PLANE_AXIS_SMALL),
        1 => Some(PLANE_AXIS_MEDIUM),
        3 => Some(PLANE_AXIS_LARGE),
        _ => None,
    };
    let width = axis(value & PLANE_SIZE_AXIS_MASK);
    let height = axis((value >> PLANE_HEIGHT_SHIFT) & PLANE_SIZE_AXIS_MASK);
    match (width, height) {
        (Some(width), Some(height)) if width * height <= PLANE_MAX_CELLS => Ok((width, height)),
        _ => Err(VdpRenderError::UnsupportedPlaneSize(value)),
    }
}

pub(super) fn screen_bounds_with_geometry(
    x: u16,
    geometry: RasterGeometry,
    mode: HorizontalMode,
) -> Result<(), VdpRenderError> {
    let height = WINDOW_HEIGHT * if geometry.interlaced { 2 } else { 1 };
    if usize::from(x) >= mode.width() || usize::from(geometry.source_y) >= height {
        Err(VdpRenderError::CoordinateOutOfBounds {
            x,
            y: geometry.source_y,
        })
    } else {
        Ok(())
    }
}
