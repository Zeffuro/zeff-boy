use anyhow::{Result, bail};
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub const VRAM_LEN: usize = 0x4000;
pub const FRAME_WIDTH: usize = 256;
pub const FRAME_HEIGHT: usize = 192;
pub const FRAMEBUFFER_LEN: usize = FRAME_WIDTH * FRAME_HEIGHT * 4;
pub const NTSC_CPU_CYCLES_PER_LINE: u16 = 228;
pub const NTSC_LINES_PER_FRAME: u16 = 262;
pub const VBLANK_START_LINE: u16 = 192;

const VRAM_MASK: u16 = 0x3FFF;
const REGISTER_MASKS: [u8; 8] = [0x03, 0xFB, 0x0F, 0xFF, 0x07, 0x7F, 0x07, 0xFF];
const STATUS_FIFTH_SPRITE: u8 = 0x40;
const STATUS_SPRITE_COLLISION: u8 = 0x20;
const STATUS_VBLANK: u8 = 0x80;
const SPRITE_COLLISION_OCCUPIED: u8 = 0x01;
const SPRITE_VISIBLE_OCCUPIED: u8 = 0x02;
const SPRITE_OCCUPANCY_X_OFFSET: i16 = 32;
const SPRITE_OCCUPANCY_LEN: usize = FRAME_WIDTH + 64;

const PALETTE: [[u8; 4]; 16] = [
    [0x00, 0x00, 0x00, 0xFF],
    [0x00, 0x00, 0x00, 0xFF],
    [0x21, 0xC8, 0x42, 0xFF],
    [0x5E, 0xDC, 0x78, 0xFF],
    [0x54, 0x55, 0xED, 0xFF],
    [0x7D, 0x76, 0xFC, 0xFF],
    [0xD4, 0x52, 0x4D, 0xFF],
    [0x42, 0xEB, 0xF5, 0xFF],
    [0xFC, 0x55, 0x54, 0xFF],
    [0xFF, 0x79, 0x78, 0xFF],
    [0xD4, 0xC1, 0x54, 0xFF],
    [0xE6, 0xCE, 0x80, 0xFF],
    [0x21, 0xB0, 0x3B, 0xFF],
    [0xC9, 0x5B, 0xBA, 0xFF],
    [0xCC, 0xCC, 0xCC, 0xFF],
    [0xFF, 0xFF, 0xFF, 0xFF],
];

#[derive(Clone)]
pub struct Vdp {
    vram: [u8; VRAM_LEN],
    registers: [u8; 8],
    framebuffer: Vec<u8>,
    sprite_occupancy: [u8; SPRITE_OCCUPANCY_LEN],
    address: u16,
    control_latch: Option<u8>,
    read_ahead: u8,
    write_buffer: u8,
    status: u8,
    cycles_into_line: u16,
    scanline: u16,
    frame_count: u64,
    frame_ready: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpMode {
    Graphics1,
    Graphics2,
    Text,
    Multicolor,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpDebugSnapshot {
    pub status: u8,
    pub address: u16,
    pub scanline: u16,
    pub cycles_into_line: u16,
    pub frame_count: u64,
    pub control_latch: Option<u8>,
    pub read_ahead: u8,
    pub write_buffer: u8,
    pub nmi_line: bool,
    pub display_enabled: bool,
    pub mode: VdpMode,
    pub name_table_base: usize,
    pub pattern_table_base: usize,
    pub color_table_base: usize,
    pub sprite_attribute_table_base: usize,
    pub sprite_pattern_table_base: usize,
    pub backdrop_color: u8,
    pub text_foreground_color: u8,
    pub text_background_color: u8,
    pub sprite_size: usize,
    pub sprite_magnified: bool,
}

impl Default for Vdp {
    fn default() -> Self {
        Self::new()
    }
}

impl Vdp {
    pub fn new() -> Self {
        let mut vdp = Self {
            vram: [0; VRAM_LEN],
            registers: [0; 8],
            framebuffer: vec![0; FRAMEBUFFER_LEN],
            sprite_occupancy: [0; SPRITE_OCCUPANCY_LEN],
            address: 0,
            control_latch: None,
            read_ahead: 0,
            write_buffer: 0,
            status: 0,
            cycles_into_line: 0,
            scanline: 0,
            frame_count: 0,
            frame_ready: false,
        };
        vdp.clear_to_backdrop();
        vdp
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    pub fn vram(&self) -> &[u8; VRAM_LEN] {
        &self.vram
    }

    pub fn registers(&self) -> &[u8; 8] {
        &self.registers
    }

    pub fn scanline(&self) -> u16 {
        self.scanline
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn debug_snapshot(&self) -> VdpDebugSnapshot {
        let mode = self.display_mode();
        let pattern_table_base = match mode {
            VdpMode::Graphics2 => usize::from(self.registers[4] & 0x04) << 11,
            _ => usize::from(self.registers[4] & 0x07) << 11,
        };
        let color_table_base = match mode {
            VdpMode::Graphics2 => usize::from(self.registers[3] & 0x80) << 6,
            _ => usize::from(self.registers[3]) << 6,
        };
        let base_sprite_size = if self.registers[1] & 0x02 != 0 { 16 } else { 8 };
        let sprite_magnified = self.registers[1] & 0x01 != 0;
        VdpDebugSnapshot {
            status: self.status,
            address: self.address,
            scanline: self.scanline,
            cycles_into_line: self.cycles_into_line,
            frame_count: self.frame_count,
            control_latch: self.control_latch,
            read_ahead: self.read_ahead,
            write_buffer: self.write_buffer,
            nmi_line: self.nmi_line(),
            display_enabled: self.registers[1] & 0x40 != 0,
            mode,
            name_table_base: usize::from(self.registers[2] & 0x0F) << 10,
            pattern_table_base,
            color_table_base,
            sprite_attribute_table_base: usize::from(self.registers[5] & 0x7F) << 7,
            sprite_pattern_table_base: usize::from(self.registers[6] & 0x07) << 11,
            backdrop_color: self.registers[7] & 0x0F,
            text_foreground_color: self.registers[7] >> 4,
            text_background_color: self.registers[7] & 0x0F,
            sprite_size: base_sprite_size * if sprite_magnified { 2 } else { 1 },
            sprite_magnified,
        }
    }

    pub fn nmi_line(&self) -> bool {
        self.status & STATUS_VBLANK != 0 && self.registers[1] & 0x20 != 0
    }

    pub fn take_frame_ready(&mut self) -> bool {
        std::mem::take(&mut self.frame_ready)
    }

    pub fn read_data_port(&mut self) -> u8 {
        self.control_latch = None;
        let value = self.read_ahead;
        self.read_ahead = self.vram[self.address as usize];
        self.increment_address();
        value
    }

    pub fn read_data(&mut self) -> u8 {
        self.read_data_port()
    }

    pub fn write_data_port(&mut self, value: u8) {
        self.control_latch = None;
        self.write_buffer = value;
        self.read_ahead = value;
        self.vram[self.address as usize] = value;
        self.increment_address();
    }

    pub fn write_data(&mut self, value: u8) {
        self.write_data_port(value);
    }

    pub fn write_control_port(&mut self, value: u8) {
        let Some(first) = self.control_latch.take() else {
            self.address = (self.address & 0x3F00) | u16::from(value);
            self.control_latch = Some(value);
            return;
        };

        self.address = ((u16::from(value & 0x3F) << 8) | u16::from(first)) & VRAM_MASK;
        if value & 0x80 != 0 {
            self.write_register((value & 0x07) as usize, first);
            return;
        }

        if value & 0x40 == 0 {
            self.read_ahead = self.vram[self.address as usize];
            self.increment_address();
        }
    }

    pub fn write_control(&mut self, value: u8) {
        self.write_control_port(value);
    }

    pub fn read_status_port(&mut self) -> u8 {
        let status = self.status;
        self.status = 0x1F;
        self.control_latch = None;
        status
    }

    pub fn read_status(&mut self) -> u8 {
        self.read_status_port()
    }

    pub fn write_register(&mut self, index: usize, value: u8) {
        if let Some(register) = self.registers.get_mut(index) {
            *register = value & REGISTER_MASKS[index];
        }
    }

    pub fn read_register(&self, index: usize) -> Option<u8> {
        self.registers.get(index).copied()
    }

    pub fn write_vram(&mut self, address: u16, value: u8) {
        self.vram[(address & VRAM_MASK) as usize] = value;
    }

    pub fn read_vram(&self, address: u16) -> u8 {
        self.vram[(address & VRAM_MASK) as usize]
    }

    pub fn step_cycles(&mut self, mut cycles: u32) {
        while cycles != 0 {
            if self.cycles_into_line == 0 && self.scanline < FRAME_HEIGHT as u16 {
                self.render_scanline(usize::from(self.scanline));
            }

            let remaining = u32::from(NTSC_CPU_CYCLES_PER_LINE - self.cycles_into_line);
            let elapsed = cycles.min(remaining);
            self.cycles_into_line += elapsed as u16;
            cycles -= elapsed;

            if self.cycles_into_line == NTSC_CPU_CYCLES_PER_LINE {
                self.cycles_into_line = 0;
                self.advance_scanline();
            }
        }
    }

    pub fn render_frame(&mut self) {
        for y in 0..FRAME_HEIGHT {
            self.render_scanline(y);
        }
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.vram);
        writer.write_bytes(&self.registers);
        writer.write_bytes(&self.framebuffer);
        writer.write_u16(self.address);
        writer.write_bool(self.control_latch.is_some());
        writer.write_u8(self.control_latch.unwrap_or(0));
        writer.write_u8(self.read_ahead);
        writer.write_u8(self.write_buffer);
        writer.write_u8(self.status);
        writer.write_u16(self.cycles_into_line);
        writer.write_u16(self.scanline);
        writer.write_u64(self.frame_count);
        writer.write_bool(self.frame_ready);
    }

    pub fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        let mut vram = [0; VRAM_LEN];
        let mut registers = [0; 8];
        let mut framebuffer = vec![0; FRAMEBUFFER_LEN];
        reader.read_exact(&mut vram)?;
        reader.read_exact(&mut registers)?;
        reader.read_exact(&mut framebuffer)?;
        let address = reader.read_u16()?;
        let has_control_latch = reader.read_bool()?;
        let control_latch_value = reader.read_u8()?;
        let control_latch = has_control_latch.then_some(control_latch_value);
        let read_ahead = reader.read_u8()?;
        let write_buffer = reader.read_u8()?;
        let status = reader.read_u8()?;
        let cycles_into_line = reader.read_u16()?;
        let scanline = reader.read_u16()?;
        let frame_count = reader.read_u64()?;
        let frame_ready = reader.read_bool()?;

        if address > VRAM_MASK {
            bail!("Coleco VDP address exceeds 16 KiB VRAM: {address:#06X}");
        }
        if cycles_into_line >= NTSC_CPU_CYCLES_PER_LINE {
            bail!("Coleco VDP cycle position exceeds a scanline: {cycles_into_line}");
        }
        if scanline >= NTSC_LINES_PER_FRAME {
            bail!("Coleco VDP scanline exceeds an NTSC frame: {scanline}");
        }
        for (index, value) in registers.iter().copied().enumerate() {
            if value & !REGISTER_MASKS[index] != 0 {
                bail!("Coleco VDP register {index} contains unmasked bits: {value:#04X}");
            }
        }

        self.vram = vram;
        self.registers = registers;
        self.framebuffer = framebuffer;
        self.address = address;
        self.control_latch = control_latch;
        self.read_ahead = read_ahead;
        self.write_buffer = write_buffer;
        self.status = status;
        self.cycles_into_line = cycles_into_line;
        self.scanline = scanline;
        self.frame_count = frame_count;
        self.frame_ready = frame_ready;
        Ok(())
    }

    fn increment_address(&mut self) {
        self.address = self.address.wrapping_add(1) & VRAM_MASK;
    }

    fn advance_scanline(&mut self) {
        self.scanline += 1;
        if self.scanline == VBLANK_START_LINE {
            self.status |= STATUS_VBLANK;
            self.frame_ready = true;
        }
        if self.scanline == NTSC_LINES_PER_FRAME {
            self.scanline = 0;
            self.frame_count = self.frame_count.wrapping_add(1);
        }
    }

    fn display_mode(&self) -> VdpMode {
        match (
            self.registers[1] & 0x10 != 0,
            self.registers[1] & 0x08 != 0,
            self.registers[0] & 0x02 != 0,
        ) {
            (false, false, false) => VdpMode::Graphics1,
            (false, false, true) => VdpMode::Graphics2,
            (true, false, false) => VdpMode::Text,
            (false, true, false) => VdpMode::Multicolor,
            _ => VdpMode::Unsupported,
        }
    }

    fn clear_to_backdrop(&mut self) {
        let color = PALETTE[(self.registers[7] & 0x0F) as usize];
        for pixel in self.framebuffer.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&color);
        }
    }

    fn clear_scanline_to_backdrop(&mut self, y: usize) {
        let color = PALETTE[(self.registers[7] & 0x0F) as usize];
        let start = y * FRAME_WIDTH * 4;
        let end = start + FRAME_WIDTH * 4;
        for pixel in self.framebuffer[start..end].as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&color);
        }
    }

    fn render_scanline(&mut self, y: usize) {
        self.clear_scanline_to_backdrop(y);
        if self.registers[1] & 0x40 == 0 {
            return;
        }

        let mode = self.display_mode();
        match mode {
            VdpMode::Graphics1 => self.render_graphics1_scanline(y),
            VdpMode::Graphics2 => self.render_graphics2_scanline(y),
            VdpMode::Text => self.render_text_scanline(y),
            VdpMode::Multicolor => self.render_multicolor_scanline(y),
            VdpMode::Unsupported => {}
        }
        if self.registers[1] & 0x10 == 0 {
            self.render_sprite_scanline(y);
        }
    }

    fn render_graphics1_scanline(&mut self, y: usize) {
        let name_base = usize::from(self.registers[2] & 0x0F) << 10;
        let color_base = usize::from(self.registers[3]) << 6;
        let pattern_base = usize::from(self.registers[4] & 0x07) << 11;
        let tile_y = y / 8;
        let row = y & 7;
        for tile_x in 0..32 {
            let tile = self.vram[(name_base + tile_y * 32 + tile_x) & usize::from(VRAM_MASK)];
            let colors = self.vram[(color_base + usize::from(tile >> 3)) & usize::from(VRAM_MASK)];
            let pattern =
                self.vram[(pattern_base + usize::from(tile) * 8 + row) & usize::from(VRAM_MASK)];
            self.draw_pattern_row(tile_x * 8, y, pattern, (colors >> 4, colors & 0x0F), 8);
        }
    }

    fn render_graphics2_scanline(&mut self, y: usize) {
        let name_base = usize::from(self.registers[2] & 0x0F) << 10;
        let color_base = usize::from(self.registers[3] & 0x80) << 6;
        let pattern_base = usize::from(self.registers[4] & 0x04) << 11;
        let color_mask = (usize::from(self.registers[3] & 0x7F) << 6) | 0x3F;
        let pattern_mask = (usize::from(self.registers[4] & 0x03) << 11)
            | (usize::from(self.registers[3] & 0x1F) << 6)
            | 0x3F;
        let tile_y = y / 8;
        let row = y & 7;
        let section = (tile_y / 8) * 0x800;
        for tile_x in 0..32 {
            let tile = self.vram[(name_base + tile_y * 32 + tile_x) & usize::from(VRAM_MASK)];
            let offset = section + usize::from(tile) * 8 + row;
            let pattern =
                self.vram[(pattern_base + (offset & pattern_mask)) & usize::from(VRAM_MASK)];
            let colors = self.vram[(color_base + (offset & color_mask)) & usize::from(VRAM_MASK)];
            self.draw_pattern_row(tile_x * 8, y, pattern, (colors >> 4, colors & 0x0F), 8);
        }
    }

    fn render_text_scanline(&mut self, y: usize) {
        let name_base = usize::from(self.registers[2] & 0x0F) << 10;
        let pattern_base = usize::from(self.registers[4] & 0x07) << 11;
        let foreground = self.registers[7] >> 4;
        let background = self.registers[7] & 0x0F;
        let row = y / 8;
        let line = y & 7;
        for column in 0..40 {
            let tile = self.vram[(name_base + row * 40 + column) & usize::from(VRAM_MASK)];
            let pattern =
                self.vram[(pattern_base + usize::from(tile) * 8 + line) & usize::from(VRAM_MASK)];
            self.draw_pattern_row(8 + column * 6, y, pattern, (foreground, background), 6);
        }
    }

    fn render_multicolor_scanline(&mut self, y: usize) {
        let name_base = usize::from(self.registers[2] & 0x0F) << 10;
        let pattern_base = usize::from(self.registers[4] & 0x07) << 11;
        let tile_y = y / 8;
        let pattern_row = (tile_y & 3) * 2 + (y & 7) / 4;
        for tile_x in 0..32 {
            let tile = self.vram[(name_base + tile_y * 32 + tile_x) & usize::from(VRAM_MASK)];
            let colors = self.vram
                [(pattern_base + usize::from(tile) * 8 + pattern_row) & usize::from(VRAM_MASK)];
            self.fill_span(tile_x * 8, y, 4, colors >> 4);
            self.fill_span(tile_x * 8 + 4, y, 4, colors & 0x0F);
        }
    }

    fn render_sprite_scanline(&mut self, y: usize) {
        let attribute_base = usize::from(self.registers[5] & 0x7F) << 7;
        let pattern_base = usize::from(self.registers[6] & 0x07) << 11;
        let base_size = if self.registers[1] & 0x02 != 0 { 16 } else { 8 };
        let scale = if self.registers[1] & 0x01 != 0 { 2 } else { 1 };
        let sprite_size = base_size * scale;

        if self.status & STATUS_FIFTH_SPRITE == 0 {
            self.status = (self.status & !0x1F) | 0x1F;
        }
        self.sprite_occupancy.fill(0);
        let screen_line = y as i16;
        let mut visible_sprites = 0usize;
        for index in 0..32usize {
            let address = (attribute_base + index * 4) & usize::from(VRAM_MASK);
            let sprite_y = self.vram[address];
            if sprite_y == 0xD0 {
                break;
            }
            let screen_y = if sprite_y > 0xE0 {
                i16::from(sprite_y) - 255
            } else {
                i16::from(sprite_y) + 1
            };
            if screen_line < screen_y || screen_line >= screen_y + sprite_size as i16 {
                continue;
            }
            visible_sprites += 1;
            if visible_sprites == 5 {
                if self.status & (STATUS_FIFTH_SPRITE | STATUS_VBLANK) == 0 {
                    self.status = (self.status & !0x1F) | STATUS_FIFTH_SPRITE | index as u8;
                }
                break;
            }

            let x_raw = self.vram[(address + 1) & usize::from(VRAM_MASK)];
            let screen_x = if self.vram[(address + 3) & usize::from(VRAM_MASK)] & 0x80 != 0 {
                i16::from(x_raw) - 32
            } else {
                i16::from(x_raw)
            };
            let mut pattern = self.vram[(address + 2) & usize::from(VRAM_MASK)];
            if base_size == 16 {
                pattern &= 0xFC;
            }
            let color = self.vram[(address + 3) & usize::from(VRAM_MASK)] & 0x0F;
            let source_y = usize::try_from(screen_line - screen_y).unwrap() / scale;
            for source_x in 0..base_size {
                let pattern_offset = if base_size == 16 {
                    match (source_x >= 8, source_y >= 8) {
                        (false, false) => 0,
                        (false, true) => 1,
                        (true, false) => 2,
                        (true, true) => 3,
                    }
                } else {
                    0
                };
                let pattern_row = source_y & 7;
                let pattern_byte = self.vram[(pattern_base
                    + usize::from(pattern.wrapping_add(pattern_offset as u8)) * 8
                    + pattern_row)
                    & usize::from(VRAM_MASK)];
                if pattern_byte & (0x80 >> (source_x & 7)) == 0 {
                    continue;
                }
                for x_scale in 0..scale {
                    let x = screen_x + (source_x * scale + x_scale) as i16;
                    let occupancy_index = usize::try_from(x + SPRITE_OCCUPANCY_X_OFFSET).unwrap();
                    if self.sprite_occupancy[occupancy_index] & SPRITE_COLLISION_OCCUPIED != 0 {
                        self.status |= STATUS_SPRITE_COLLISION;
                    } else {
                        self.sprite_occupancy[occupancy_index] |= SPRITE_COLLISION_OCCUPIED;
                    }
                    if !(0..FRAME_WIDTH as i16).contains(&x) {
                        continue;
                    }
                    if color != 0
                        && self.sprite_occupancy[occupancy_index] & SPRITE_VISIBLE_OCCUPIED == 0
                    {
                        self.sprite_occupancy[occupancy_index] |= SPRITE_VISIBLE_OCCUPIED;
                        self.set_pixel(x as usize, y, color);
                    }
                }
            }
        }
    }

    fn draw_pattern_row(
        &mut self,
        x: usize,
        y: usize,
        pattern: u8,
        colors: (u8, u8),
        width: usize,
    ) {
        for bit in 0..width {
            let color = if pattern & (0x80 >> bit) != 0 {
                colors.0
            } else {
                colors.1
            };
            self.set_pixel(x + bit, y, color);
        }
    }

    fn fill_span(&mut self, x: usize, y: usize, width: usize, color: u8) {
        for column in x..x + width {
            self.set_pixel(column, y, color);
        }
    }

    fn set_pixel(&mut self, x: usize, y: usize, color: u8) {
        if x >= FRAME_WIDTH || y >= FRAME_HEIGHT {
            return;
        }
        let offset = (y * FRAME_WIDTH + x) * 4;
        let color = if color == 0 {
            self.registers[7] & 0x0F
        } else {
            color
        };
        self.framebuffer[offset..offset + 4].copy_from_slice(&PALETTE[color as usize]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_register(vdp: &mut Vdp, index: u8, value: u8) {
        vdp.write_control_port(value);
        vdp.write_control_port(0x80 | index);
    }

    #[test]
    fn control_port_masks_registers_and_data_port_uses_read_ahead() {
        let mut vdp = Vdp::new();
        for (index, mask) in REGISTER_MASKS.iter().copied().enumerate() {
            set_register(&mut vdp, index as u8, 0xFF);
            assert_eq!(vdp.read_register(index), Some(mask));
        }

        vdp.write_control_port(0xFE);
        vdp.write_control_port(0x7F);
        vdp.write_data_port(0xA5);
        vdp.write_data_port(0x5A);
        vdp.write_control_port(0xFE);
        vdp.write_control_port(0x3F);
        assert_eq!(vdp.read_data_port(), 0xA5);
        assert_eq!(vdp.read_data_port(), 0x5A);
    }

    #[test]
    fn data_port_accesses_cancel_a_pending_control_byte() {
        let mut vdp = Vdp::new();
        vdp.write_control_port(0x34);
        vdp.write_data_port(0x11);
        vdp.write_control_port(0x12);
        vdp.write_control_port(0x40);
        vdp.write_data_port(0xA5);
        assert_eq!(vdp.read_vram(0x0012), 0xA5);

        vdp.write_control_port(0x56);
        let _ = vdp.read_data_port();
        vdp.write_control_port(0x78);
        vdp.write_control_port(0x40);
        vdp.write_data_port(0x5A);
        assert_eq!(vdp.read_vram(0x0078), 0x5A);
    }

    #[test]
    fn first_control_byte_immediately_replaces_the_low_address_byte() {
        let mut vdp = Vdp::new();
        vdp.write_control_port(0x34);
        vdp.write_control_port(0x52);
        vdp.write_control_port(0xAB);
        vdp.write_data_port(0x5A);

        assert_eq!(vdp.read_vram(0x12AB), 0x5A);
        assert_eq!(vdp.debug_snapshot().address, 0x12AC);
    }

    #[test]
    fn register_writes_replace_the_vram_address_latch() {
        let mut vdp = Vdp::new();
        vdp.write_control_port(0xA5);
        vdp.write_control_port(0x83);
        vdp.write_data_port(0x5A);

        assert_eq!(vdp.read_vram(0x03A5), 0x5A);
        assert_eq!(vdp.debug_snapshot().address, 0x03A6);
    }

    #[test]
    fn vblank_starts_on_line_192_and_status_read_drops_nmi() {
        let mut vdp = Vdp::new();
        set_register(&mut vdp, 1, 0x60);
        vdp.step_cycles(u32::from(NTSC_CPU_CYCLES_PER_LINE) * 191);
        assert_eq!(vdp.scanline(), 191);
        assert!(!vdp.nmi_line());

        vdp.step_cycles(u32::from(NTSC_CPU_CYCLES_PER_LINE));
        assert_eq!(vdp.scanline(), VBLANK_START_LINE);
        assert!(vdp.nmi_line());
        assert_ne!(vdp.read_status_port() & STATUS_VBLANK, 0);
        assert!(!vdp.nmi_line());
        assert_eq!(vdp.read_status_port(), 0x1F);
    }

    #[test]
    fn mid_frame_writes_only_affect_future_scanlines() {
        let mut vdp = Vdp::new();
        set_register(&mut vdp, 1, 0x40);
        set_register(&mut vdp, 2, 0x01);
        set_register(&mut vdp, 3, 0x00);
        set_register(&mut vdp, 4, 0x01);
        set_register(&mut vdp, 7, 0x04);
        vdp.write_vram(0x400, 1);
        vdp.write_vram(0x000, 0xE0);
        vdp.write_vram(0x808, 0x80);
        vdp.step_cycles(1);

        set_register(&mut vdp, 7, 0x03);
        vdp.write_vram(0x000, 0xA0);
        vdp.write_vram(0x809, 0x80);
        vdp.step_cycles(u32::from(NTSC_CPU_CYCLES_PER_LINE));

        assert_eq!(&vdp.framebuffer()[0..4], &PALETTE[0x0E]);
        assert_eq!(&vdp.framebuffer()[4..8], &PALETTE[0x04]);
        let second_line = FRAME_WIDTH * 4;
        assert_eq!(
            &vdp.framebuffer()[second_line..second_line + 4],
            &PALETTE[0x0A]
        );
        assert_eq!(
            &vdp.framebuffer()[second_line + 4..second_line + 8],
            &PALETTE[0x03]
        );

        let lines_remaining = VBLANK_START_LINE - vdp.scanline();
        vdp.step_cycles(u32::from(lines_remaining) * u32::from(NTSC_CPU_CYCLES_PER_LINE));
        assert_eq!(&vdp.framebuffer()[0..4], &PALETTE[0x0E]);
        assert_eq!(&vdp.framebuffer()[4..8], &PALETTE[0x04]);
    }

    #[test]
    fn graphics_modes_render_expected_pixels() {
        let mut vdp = Vdp::new();
        set_register(&mut vdp, 1, 0x40);
        set_register(&mut vdp, 2, 0x01);
        set_register(&mut vdp, 3, 0x00);
        set_register(&mut vdp, 4, 0x01);
        vdp.write_vram(0x400, 1);
        vdp.write_vram(0, 0xE4);
        vdp.write_vram(0x808, 0x80);
        vdp.render_frame();
        assert_eq!(&vdp.framebuffer()[0..4], &PALETTE[0x0E]);
        assert_eq!(&vdp.framebuffer()[4..8], &PALETTE[0x04]);

        set_register(&mut vdp, 0, 0x02);
        set_register(&mut vdp, 3, 0x80);
        set_register(&mut vdp, 4, 0x00);
        vdp.write_vram(0x008, 0x80);
        vdp.write_vram(0x2008, 0xA3);
        vdp.render_frame();
        assert_eq!(&vdp.framebuffer()[0..4], &PALETTE[0x0A]);
        assert_eq!(&vdp.framebuffer()[16..20], &PALETTE[0x03]);
    }

    #[test]
    fn invalid_mode_combinations_respect_the_m1_sprite_disable() {
        for mode in [3, 5, 6, 7] {
            let mut vdp = Vdp::new();
            let mut register1 = 0x40;
            if mode & 1 != 0 {
                register1 |= 0x10;
            }
            if mode & 4 != 0 {
                register1 |= 0x08;
            }
            set_register(&mut vdp, 0, if mode & 2 != 0 { 0x02 } else { 0 });
            set_register(&mut vdp, 1, register1);
            set_register(&mut vdp, 6, 0x01);
            vdp.write_vram(0, 0xFF);
            vdp.write_vram(1, 0);
            vdp.write_vram(2, 0);
            vdp.write_vram(3, 0x0F);
            vdp.write_vram(4, 0xD0);
            vdp.write_vram(0x0800, 0x80);

            vdp.render_frame();

            assert_eq!(vdp.display_mode(), VdpMode::Unsupported);
            let first_pixel = if mode == 6 {
                &PALETTE[0x0F]
            } else {
                &PALETTE[0]
            };
            assert_eq!(&vdp.framebuffer()[0..4], first_pixel);
            assert_eq!(&vdp.framebuffer()[4..8], &PALETTE[0]);
        }
    }

    #[test]
    fn graphics_one_uses_the_full_color_base_and_multicolor_uses_two_byte_rows() {
        let mut vdp = Vdp::new();
        set_register(&mut vdp, 1, 0x40);
        set_register(&mut vdp, 2, 0x01);
        set_register(&mut vdp, 3, 0x02);
        set_register(&mut vdp, 4, 0x01);
        vdp.write_vram(0x400, 1);
        vdp.write_vram(0x080, 0xF2);
        vdp.write_vram(0x808, 0x80);
        vdp.render_frame();
        assert_eq!(&vdp.framebuffer()[0..4], &PALETTE[0x0F]);
        assert_eq!(&vdp.framebuffer()[4..8], &PALETTE[0x02]);

        set_register(&mut vdp, 0, 0x00);
        set_register(&mut vdp, 1, 0x48);
        vdp.write_vram(0x808, 0xA3);
        vdp.write_vram(0x809, 0xE6);
        vdp.write_vram(0x420, 1);
        vdp.write_vram(0x80A, 0xC5);
        vdp.write_vram(0x80B, 0x7D);
        vdp.render_frame();
        assert_eq!(&vdp.framebuffer()[0..4], &PALETTE[0x0A]);
        assert_eq!(&vdp.framebuffer()[16..20], &PALETTE[0x03]);
        let second_block_row = FRAME_WIDTH * 4 * 4;
        assert_eq!(
            &vdp.framebuffer()[second_block_row..second_block_row + 4],
            &PALETTE[0x0E]
        );
        let next_tile_row = FRAME_WIDTH * 8 * 4;
        assert_eq!(
            &vdp.framebuffer()[next_tile_row..next_tile_row + 4],
            &PALETTE[0x0C]
        );
        let next_tile_second_block = FRAME_WIDTH * 12 * 4;
        assert_eq!(
            &vdp.framebuffer()[next_tile_second_block..next_tile_second_block + 4],
            &PALETTE[0x07]
        );
    }

    #[test]
    fn graphics_two_table_mask_bits_alias_sections() {
        let mut vdp = Vdp::new();
        set_register(&mut vdp, 0, 0x02);
        set_register(&mut vdp, 1, 0x40);
        set_register(&mut vdp, 2, 0x01);
        set_register(&mut vdp, 3, 0x80);
        set_register(&mut vdp, 4, 0x00);
        vdp.write_vram(0x500, 1);
        vdp.write_vram(0x0008, 0x80);
        vdp.write_vram(0x2008, 0xE4);
        vdp.write_vram(0x0808, 0x00);
        vdp.write_vram(0x2808, 0x00);
        vdp.render_frame();

        let middle_section_pixel = FRAME_WIDTH * 64 * 4;
        assert_eq!(
            &vdp.framebuffer()[middle_section_pixel..middle_section_pixel + 4],
            &PALETTE[0x0E]
        );
    }

    #[test]
    fn graphics_two_r3_mask_aliases_pattern_name_bits_on_tms99_family() {
        let mut vdp = Vdp::new();
        set_register(&mut vdp, 0, 0x02);
        set_register(&mut vdp, 1, 0x40);
        set_register(&mut vdp, 2, 0x01);
        set_register(&mut vdp, 3, 0x80);
        set_register(&mut vdp, 4, 0x00);
        vdp.write_vram(0x0400, 0x20);
        vdp.write_vram(0x0000, 0x80);
        vdp.write_vram(0x0100, 0x00);
        vdp.write_vram(0x2000, 0xE4);
        vdp.write_vram(0x2100, 0xE4);
        vdp.render_frame();
        assert_eq!(&vdp.framebuffer()[0..4], &PALETTE[0x0E]);

        set_register(&mut vdp, 3, 0x84);
        vdp.render_frame();
        assert_eq!(&vdp.framebuffer()[0..4], &PALETTE[0x04]);
    }

    #[test]
    fn sprites_latch_collision_and_fifth_sprite_index() {
        let mut vdp = Vdp::new();
        set_register(&mut vdp, 1, 0x40);
        set_register(&mut vdp, 5, 0x00);
        set_register(&mut vdp, 6, 0x01);
        vdp.write_vram(0, 10);
        vdp.write_vram(1, 10);
        vdp.write_vram(2, 0);
        vdp.write_vram(3, 0x01);
        vdp.write_vram(4, 10);
        vdp.write_vram(5, 10);
        vdp.write_vram(6, 0);
        vdp.write_vram(7, 0x02);
        for index in 2..5 {
            let base = index * 4;
            vdp.write_vram(base as u16, 10);
            vdp.write_vram((base + 1) as u16, (index * 16) as u8);
            vdp.write_vram((base + 2) as u16, 0);
            vdp.write_vram((base + 3) as u16, 0x03);
        }
        vdp.write_vram(20, 0xD0);
        vdp.write_vram(0x800, 0x80);
        vdp.render_frame();
        let status = vdp.read_status_port();
        assert_ne!(status & STATUS_SPRITE_COLLISION, 0);
        assert_ne!(status & STATUS_FIFTH_SPRITE, 0);
        assert_eq!(status & 0x1F, 4);
    }

    #[test]
    fn sprite_status_latches_on_the_affected_scanline() {
        let mut vdp = Vdp::new();
        set_register(&mut vdp, 1, 0x40);
        set_register(&mut vdp, 5, 0x00);
        set_register(&mut vdp, 6, 0x01);
        for index in 0..5 {
            let base = index * 4;
            vdp.write_vram(base, 10);
            vdp.write_vram(base + 1, if index < 2 { 10 } else { index as u8 * 16 });
            vdp.write_vram(base + 2, 0);
            vdp.write_vram(base + 3, index as u8 + 1);
        }
        vdp.write_vram(20, 0xD0);
        vdp.write_vram(0x800, 0x80);

        vdp.step_cycles(u32::from(NTSC_CPU_CYCLES_PER_LINE) * 11);
        assert_eq!(
            vdp.status & (STATUS_SPRITE_COLLISION | STATUS_FIFTH_SPRITE),
            0
        );
        vdp.step_cycles(1);
        assert_ne!(vdp.status & STATUS_SPRITE_COLLISION, 0);
        assert_ne!(vdp.status & STATUS_FIFTH_SPRITE, 0);
        assert_eq!(vdp.status & 0x1F, 4);
    }

    #[test]
    fn fifth_sprite_does_not_latch_while_frame_flag_is_pending() {
        let mut vdp = Vdp::new();
        set_register(&mut vdp, 1, 0x40);
        set_register(&mut vdp, 5, 0x00);
        for index in 0..5 {
            let base = index * 4;
            vdp.write_vram(base, 0);
        }
        vdp.write_vram(20, 0xD0);
        vdp.status = STATUS_VBLANK;

        vdp.render_frame();

        assert_eq!(vdp.status & STATUS_FIFTH_SPRITE, 0);
    }

    #[test]
    fn sixteen_pixel_sprites_use_column_major_pattern_quadrants() {
        let mut vdp = Vdp::new();
        set_register(&mut vdp, 1, 0x42);
        set_register(&mut vdp, 5, 0x00);
        set_register(&mut vdp, 6, 0x01);
        vdp.write_vram(0, 0);
        vdp.write_vram(1, 0);
        vdp.write_vram(2, 0);
        vdp.write_vram(3, 0x0F);
        vdp.write_vram(4, 0xD0);
        vdp.write_vram(0x808, 0x80);
        vdp.render_frame();

        let bottom_left = (9 * FRAME_WIDTH) * 4;
        let top_right = (FRAME_WIDTH + 8) * 4;
        assert_eq!(
            &vdp.framebuffer()[bottom_left..bottom_left + 4],
            &PALETTE[0x0F]
        );
        assert_eq!(&vdp.framebuffer()[top_right..top_right + 4], &PALETTE[0x00]);
    }

    #[test]
    fn sprite_y_e0_does_not_wrap_but_e1_does() {
        let mut vdp = Vdp::new();
        set_register(&mut vdp, 1, 0x43);
        set_register(&mut vdp, 5, 0x20);
        set_register(&mut vdp, 6, 0x01);
        vdp.write_vram(0x1000, 0xE0);
        vdp.write_vram(0x1001, 0x00);
        vdp.write_vram(0x1002, 0x00);
        vdp.write_vram(0x1003, 0x0F);
        vdp.write_vram(0x1004, 0xD0);
        vdp.write_vram(0x080F, 0x80);
        vdp.render_frame();
        assert_eq!(&vdp.framebuffer()[0..4], &PALETTE[0]);

        vdp.write_vram(0x1000, 0xE1);
        vdp.render_frame();
        assert_eq!(&vdp.framebuffer()[0..4], &PALETTE[0x0F]);
    }

    #[test]
    fn offscreen_transparent_sprite_pixels_still_collide() {
        let mut vdp = Vdp::new();
        set_register(&mut vdp, 1, 0x40);
        set_register(&mut vdp, 5, 0x00);
        set_register(&mut vdp, 6, 0x01);
        for index in 0..2 {
            let base = index * 4;
            vdp.write_vram(base, 0xFF);
            vdp.write_vram(base + 1, 0xFF);
            vdp.write_vram(base + 2, 0x00);
            vdp.write_vram(base + 3, 0x00);
        }
        vdp.write_vram(8, 0xD0);
        vdp.write_vram(0x0800, 0x40);
        vdp.render_frame();

        assert_ne!(vdp.read_status_port() & STATUS_SPRITE_COLLISION, 0);
    }

    #[test]
    fn state_roundtrip_preserves_midline_port_and_framebuffer_state() {
        let mut vdp = Vdp::new();
        set_register(&mut vdp, 7, 0x0F);
        vdp.write_control_port(0x34);
        vdp.step_cycles(17);
        vdp.write_data_port(0xA5);
        let mut writer = StateWriter::new();
        vdp.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let mut restored = Vdp::new();
        restored.read_state(&mut StateReader::new(&bytes)).unwrap();
        assert_eq!(restored.vram(), vdp.vram());
        assert_eq!(restored.registers(), vdp.registers());
        assert_eq!(restored.framebuffer(), vdp.framebuffer());
        assert_eq!(restored.scanline(), vdp.scanline());
        assert_eq!(restored.read_data_port(), vdp.read_data_port());
    }

    #[test]
    fn state_roundtrip_preserves_a_pending_control_address_byte() {
        let mut vdp = Vdp::new();
        vdp.write_control_port(0x34);
        let mut writer = StateWriter::new();
        vdp.write_state(&mut writer);

        let bytes = writer.into_bytes();
        let mut restored = Vdp::new();
        restored.read_state(&mut StateReader::new(&bytes)).unwrap();
        restored.write_data_port(0xA5);

        assert_eq!(restored.read_vram(0x0034), 0xA5);
        assert_eq!(restored.debug_snapshot().address, 0x0035);
    }
}
