use super::*;

mod spans;

#[cfg(test)]
mod tests;

struct RasterContext {
    bases: [u16; 2],
    horizontal: [u16; 2],
    layout: PlaneLayout,
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

    #[cfg(test)]
    fn prepare_with_geometry(
        memory: &VdpMemory,
        registers: &VdpRegisterPipeline,
        sprites: &VdpSpriteLine,
        colors: &[u16],
        geometry: RasterGeometry,
    ) -> Option<Self> {
        Self::prepare_with_horizontal(memory, registers, sprites, colors, geometry, None)
    }

    fn prepare_with_horizontal(
        memory: &VdpMemory,
        registers: &VdpRegisterPipeline,
        sprites: &VdpSpriteLine,
        colors: &[u16],
        geometry: RasterGeometry,
        fetched: Option<[u16; 2]>,
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
        let layout = PlaneLayout::new(size, geometry.interlaced);
        let scroll = published_register(registers, SCROLL_MODE_REGISTER).ok()?;
        validate_scroll_mode(registers, scroll, geometry.interlaced).ok()?;
        published_register(registers, BACKDROP_REGISTER).ok()?;
        let horizontal = if let Some(pair) = fetched {
            pair
        } else {
            [
                memory
                    .scroll_offsets_with_geometry(registers, VdpPlane::A, geometry, 0)
                    .ok()?
                    .0,
                memory
                    .scroll_offsets_with_geometry(registers, VdpPlane::B, geometry, 0)
                    .ok()?
                    .0,
            ]
        };
        Some(Self {
            bases,
            horizontal,
            layout,
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

    #[cfg(test)]
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
        let source_x = (x as u16).wrapping_sub(self.horizontal[plane]) & self.layout.x_mask;
        let source_y = (usize::from(y) + usize::from(vertical)) & self.layout.y_mask;
        memory.sample_name(
            self.bases[plane],
            self.layout.columns,
            usize::from(source_x),
            source_y,
            self.geometry.interlaced,
        )
    }

    #[cfg(test)]
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

    #[cfg(test)]
    pub(in crate::hardware::vdp) fn render_raster_sprite_line_with_geometry(
        &self,
        registers: &VdpRegisterPipeline,
        sprites: &VdpSpriteLine,
        colors: &mut [u16],
        geometry: RasterGeometry,
    ) -> Result<(), VdpRenderError> {
        self.render_raster_with_horizontal(registers, sprites, colors, geometry, None)
    }

    pub(in crate::hardware::vdp) fn render_raster_with_horizontal(
        &self,
        registers: &VdpRegisterPipeline,
        sprites: &VdpSpriteLine,
        colors: &mut [u16],
        geometry: RasterGeometry,
        fetched: Option<[u16; 2]>,
    ) -> Result<(), VdpRenderError> {
        // Incomplete inputs retain the scalar path's first-error order.
        let Some(context) = RasterContext::prepare_with_horizontal(
            self, registers, sprites, colors, geometry, fetched,
        ) else {
            for (x, color) in colors.iter_mut().enumerate() {
                if fetched.is_none() {
                    *color = self.raster_sprite_line_color_with_geometry(
                        registers, sprites, x as u16, geometry,
                    )?;
                    continue;
                }
                let background = self.background_pixel_with_horizontal(
                    registers, x as u16, geometry, true, fetched,
                )?;
                let pixel =
                    *sprites
                        .pixels()
                        .get(x)
                        .ok_or(VdpRenderError::CoordinateOutOfBounds {
                            x: x as u16,
                            y: geometry.source_y,
                        })?;
                *color = self.composite_color_checked::<true>(
                    registers,
                    background,
                    (!pixel.is_transparent()).then_some(pixel),
                )?;
            }
            return Ok(());
        };
        if self.cram_all_known() {
            context.render_spans::<false>(self, registers, sprites, colors, 0)
        } else {
            context.render_spans::<true>(self, registers, sprites, colors, 0)
        }
    }
}
