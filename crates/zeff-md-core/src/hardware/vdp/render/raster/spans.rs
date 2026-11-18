use super::*;

struct Run {
    row: VdpTileRow,
    offset: usize,
    end: usize,
}

impl Run {
    fn pixel(&self, color: u8) -> VdpPixel {
        VdpPixel {
            color,
            palette: self.row.palette,
            priority: self.row.priority,
        }
    }
}

impl RasterContext {
    fn run(&self, memory: &VdpMemory, plane: usize, x: usize, limit: usize) -> Run {
        let window =
            plane == 0 && (self.window_row || (x >= self.window_split) == self.window_right);
        let mut end = limit;
        let (base, columns, source_x, source_y) = if window {
            (
                self.window_base,
                self.window_columns,
                x,
                usize::from(self.geometry.source_y),
            )
        } else {
            let column = if self.column_scroll {
                end = end.min(x + WINDOW_X_STEP - x % WINDOW_X_STEP);
                x / WINDOW_X_STEP
            } else {
                0
            };
            let vertical = memory.vsram()[column * SCROLL_PAIR_WORDS + plane]
                & vertical_mask(self.geometry.interlaced);
            (
                self.bases[plane],
                self.layout.columns,
                usize::from((x as u16).wrapping_sub(self.horizontal[plane]) & self.layout.x_mask),
                (usize::from(self.geometry.source_y) + usize::from(vertical)) & self.layout.y_mask,
            )
        };
        if plane == 0 && !self.window_row && x < self.window_split {
            end = end.min(self.window_split);
        }
        let offset = source_x % TILE_WIDTH;
        end = end.min(x + TILE_WIDTH - offset);
        let height = tile_height(self.geometry.interlaced);
        let cell = source_y / height * columns + source_x / TILE_WIDTH;
        let address = (usize::from(base) + cell * NAME_ENTRY_BYTES) % VRAM_SIZE;
        let vram = memory.vram();
        let name = u16::from_be_bytes([vram[address], vram[address + 1]]);
        let row = memory
            .tile_row_in_geometry(name, (source_y % height) as u8, self.geometry.interlaced)
            .expect("row reduced by active tile height");
        Run { row, offset, end }
    }

    pub(super) fn render_spans<const CHECK: bool>(
        &self,
        memory: &VdpMemory,
        registers: &VdpRegisterPipeline,
        sprites: &VdpSpriteLine,
        colors: &mut [u16],
        start: usize,
    ) -> Result<(), VdpRenderError> {
        let limit = start + colors.len();
        if start == limit {
            return Ok(());
        }
        let mut x = start;
        let mut foreground = self.run(memory, 0, x, limit);
        let mut background = self.run(memory, 1, x, limit);
        loop {
            let end = foreground.end.min(background.end);
            let count = end - x;
            let foreground_colors =
                &foreground.row.colors[foreground.offset..foreground.offset + count];
            let background_colors =
                &background.row.colors[background.offset..background.offset + count];
            for (((color, sprite), a), b) in colors[x - start..end - start]
                .iter_mut()
                .zip(&sprites.pixels()[x..end])
                .zip(foreground_colors)
                .zip(background_colors)
            {
                let a = foreground.pixel(*a);
                let b = background.pixel(*b);
                let selected =
                    if a.is_transparent() || (!a.priority && b.priority && !b.is_transparent()) {
                        b
                    } else {
                        a
                    };
                *color = memory.composite_color_checked::<CHECK>(
                    registers,
                    BackgroundPixel {
                        selected,
                        high_priority: a.priority || b.priority,
                        shadow_highlight: self.shadow_highlight,
                    },
                    (!sprite.is_transparent()).then_some(*sprite),
                )?;
            }
            foreground.offset += count;
            background.offset += count;
            x = end;
            if x == limit {
                return Ok(());
            }
            if x == foreground.end {
                foreground = self.run(memory, 0, x, limit);
            }
            if x == background.end {
                background = self.run(memory, 1, x, limit);
            }
        }
    }
}
