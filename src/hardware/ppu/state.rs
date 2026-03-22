use super::{PPU, default_framebuffer};
use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;

impl PPU {
    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.lcdc);
        writer.write_u8(self.stat);
        writer.write_u8(self.scy);
        writer.write_u8(self.scx);
        writer.write_u8(self.ly);
        writer.write_u8(self.lyc);
        writer.write_u8(self.wy);
        writer.write_u8(self.wx);
        writer.write_u8(self.bgp);
        writer.write_u8(self.obp0);
        writer.write_u8(self.obp1);
        writer.write_bytes(&self.bg_palette_ram);
        writer.write_bytes(&self.obj_palette_ram);
        writer.write_u8(self.bcps);
        writer.write_u8(self.ocps);
        writer.write_u64(self.cycles);
        writer.write_bool(self.sgb_enabled);
        writer.write_u8(self.sgb_mask_mode);
        writer.write_u8(self.sgb_active_palette);
        for palette in &self.sgb_palettes {
            for color in palette {
                writer.write_u16(*color);
            }
        }
        writer.write_u8(self.window_line_counter);
        writer.write_bool(self.window_was_active_this_frame);
        writer.write_bool(self.window_y_triggered);
        writer.write_bool(self.cgb_mode);
        writer.write_bool(self.rendered_current_line);
        writer.write_bool(self.prev_stat_line);
    }

    pub(crate) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let mut ppu = Self::new();
        ppu.lcdc = reader.read_u8()?;
        ppu.stat = reader.read_u8()?;
        ppu.scy = reader.read_u8()?;
        ppu.scx = reader.read_u8()?;
        ppu.ly = reader.read_u8()?;
        ppu.lyc = reader.read_u8()?;
        ppu.wy = reader.read_u8()?;
        ppu.wx = reader.read_u8()?;
        ppu.bgp = reader.read_u8()?;
        ppu.obp0 = reader.read_u8()?;
        ppu.obp1 = reader.read_u8()?;
        reader.read_exact(&mut ppu.bg_palette_ram)?;
        reader.read_exact(&mut ppu.obj_palette_ram)?;
        ppu.bcps = reader.read_u8()?;
        ppu.ocps = reader.read_u8()?;
        ppu.cycles = reader.read_u64()?;
        ppu.sgb_enabled = reader.read_bool()?;
        ppu.sgb_mask_mode = reader.read_u8()?;
        ppu.sgb_active_palette = reader.read_u8()?;
        for palette in &mut ppu.sgb_palettes {
            for color in palette {
                *color = reader.read_u16()?;
            }
        }
        ppu.window_line_counter = reader.read_u8()?;
        ppu.window_was_active_this_frame = reader.read_bool()?;
        ppu.window_y_triggered = reader.read_bool()?;
        ppu.cgb_mode = reader.read_bool()?;
        ppu.rendered_current_line = reader.read_bool()?;
        ppu.prev_stat_line = reader.read_bool()?;
        ppu.framebuffer = default_framebuffer();
        Ok(ppu)
    }
}
