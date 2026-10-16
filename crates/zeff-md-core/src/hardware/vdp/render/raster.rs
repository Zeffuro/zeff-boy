use super::*;

#[cfg(test)]
mod tests;

struct RasterContext {
    bases: [u16; 2],
    horizontal: [u16; 2],
    columns: usize,
    x_mask: u16,
    y_mask: usize,
    geometry: RasterGeometry,
    column_scroll: bool,
    window_base: u16,
    window_columns: usize,
    window_split: usize,
    window_right: bool,
    window_row: bool,
    shadow_highlight: bool,
}

impl RasterContext {
    #[cfg(test)]
    fn prepare(
        memory: &VdpMemory,
        registers: &VdpRegisterPipeline,
        sprites: &VdpSpriteLine,
        colors: &[u16],
        y: u16,
    ) -> Option<Self> {
        Self::prepare_with_geometry(
            memory,
            registers,
            sprites,
            colors,
            RasterGeometry::progressive(y),
        )
    }

    fn prepare_with_geometry(
        memory: &VdpMemory,
        registers: &VdpRegisterPipeline,
        sprites: &VdpSpriteLine,
        colors: &[u16],
        geometry: RasterGeometry,
    ) -> Option<Self> {
        let mode = horizontal_mode(registers).ok()?;
        if screen_bounds_with_geometry(0, geometry, mode).is_err()
            || colors.len() != mode.width()
            || sprites.pixels().len() != mode.width()
        {
            return None;
        }
        let display = published_register(registers, DISPLAY_SIZE_REGISTER).ok()?;
        let window_x = published_register(registers, WINDOW_X_REGISTER).ok()?;
        let window_y = published_register(registers, WINDOW_Y_REGISTER).ok()?;
        let window_base = registers.window_base()?;
        let bases = [registers.plane_a_base()?, registers.plane_b_base()?];
        let size = published_register(registers, PLANE_SIZE_REGISTER).ok()?;
        let (columns, rows) = plane_dimensions(size).ok()?;
        let scroll = published_register(registers, SCROLL_MODE_REGISTER).ok()?;
        published_register(registers, BACKDROP_REGISTER).ok()?;
        let horizontal = [
            memory
                .scroll_offsets_with_geometry(registers, VdpPlane::A, geometry, 0)
                .ok()?
                .0,
            memory
                .scroll_offsets_with_geometry(registers, VdpPlane::B, geometry, 0)
                .ok()?
                .0,
        ];
        Some(Self {
            bases,
            horizontal,
            columns,
            x_mask: (columns * TILE_WIDTH - 1) as u16,
            y_mask: rows * tile_height(geometry.interlaced) - 1,
            geometry,
            column_scroll: scroll & VSCROLL_COLUMN_MODE != 0,
            window_base,
            window_columns: mode.window_columns(),
            window_split: usize::from(window_x & WINDOW_POSITION_MASK) * WINDOW_X_STEP,
            window_right: window_x & WINDOW_DIRECTION != 0,
            window_row: (usize::from(geometry.source_y)
                >= usize::from(window_y & WINDOW_POSITION_MASK) * tile_height(geometry.interlaced))
                == (window_y & WINDOW_DIRECTION != 0),
            shadow_highlight: display & SHADOW_HIGHLIGHT_ENABLE != 0,
        })
    }

    fn plane(
        &self,
        memory: &VdpMemory,
        plane: usize,
        x: usize,
        y: u16,
    ) -> Result<VdpPixel, VdpRenderError> {
        let column = if self.column_scroll {
            x / WINDOW_X_STEP
        } else {
            0
        };
        let vertical = memory.vsram()[column * SCROLL_PAIR_WORDS + plane]
            & vertical_mask(self.geometry.interlaced);
        let source_x = (x as u16).wrapping_sub(self.horizontal[plane]) & self.x_mask;
        let source_y = (usize::from(y) + usize::from(vertical)) & self.y_mask;
        memory.sample_name(
            self.bases[plane],
            self.columns,
            usize::from(source_x),
            source_y,
            self.geometry.interlaced,
        )
    }

    fn background(
        &self,
        memory: &VdpMemory,
        x: usize,
        y: u16,
    ) -> Result<BackgroundPixel, VdpRenderError> {
        let foreground = if self.window_row || (x >= self.window_split) == self.window_right {
            memory.sample_name(
                self.window_base,
                self.window_columns,
                x,
                usize::from(y),
                self.geometry.interlaced,
            )?
        } else {
            self.plane(memory, 0, x, y)?
        };
        let background = self.plane(memory, 1, x, y)?;
        Ok(BackgroundPixel {
            selected: if foreground.is_transparent()
                || (!foreground.priority && background.priority && !background.is_transparent())
            {
                background
            } else {
                foreground
            },
            high_priority: foreground.priority || background.priority,
            shadow_highlight: self.shadow_highlight,
        })
    }
}

impl VdpMemory {
    #[cfg(test)]
    pub(in crate::hardware::vdp) fn render_raster_sprite_line(
        &self,
        registers: &VdpRegisterPipeline,
        sprites: &VdpSpriteLine,
        colors: &mut [u16],
        y: u16,
    ) -> Result<(), VdpRenderError> {
        self.render_raster_sprite_line_with_geometry(
            registers,
            sprites,
            colors,
            RasterGeometry::progressive(y),
        )
    }

    pub(in crate::hardware::vdp) fn render_raster_sprite_line_with_geometry(
        &self,
        registers: &VdpRegisterPipeline,
        sprites: &VdpSpriteLine,
        colors: &mut [u16],
        geometry: RasterGeometry,
    ) -> Result<(), VdpRenderError> {
        // Incomplete inputs retain the scalar path's first-error order.
        let Some(context) =
            RasterContext::prepare_with_geometry(self, registers, sprites, colors, geometry)
        else {
            for (x, color) in colors.iter_mut().enumerate() {
                *color = self.raster_sprite_line_color_with_geometry(
                    registers, sprites, x as u16, geometry,
                )?;
            }
            return Ok(());
        };
        for (x, (color, pixel)) in colors.iter_mut().zip(sprites.pixels()).enumerate() {
            *color = self.composite_color(
                registers,
                context.background(self, x, geometry.source_y)?,
                (!pixel.is_transparent()).then_some(*pixel),
            )?;
        }
        Ok(())
    }
}
