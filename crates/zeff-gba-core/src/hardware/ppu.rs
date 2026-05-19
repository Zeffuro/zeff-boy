use super::constants::{FRAMEBUFFER_LEN, SCREEN_HEIGHT, SCREEN_WIDTH};

mod render;

const CYCLES_PER_SCANLINE: u32 = 1232;
const SCANLINES_PER_FRAME: u16 = 228;
const VISIBLE_SCANLINES: u16 = 160;

#[derive(Clone, Debug)]
pub struct Ppu {
    framebuffer: Vec<u8>,
    pub frame_ready: bool,
    vcount: u16,
    line_cycles: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PpuDebugSnapshot {
    pub dispcnt: u16,
    pub bgcnt: [u16; 4],
    pub vcount: u16,
    pub in_vblank: bool,
    pub display_mode: u16,
    pub bg_enabled: [bool; 4],
    pub obj_enabled: bool,
    pub obj_mapping_1d: bool,
    pub non_black_pixels: usize,
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}

impl Ppu {
    pub fn new() -> Self {
        let mut framebuffer = vec![0; FRAMEBUFFER_LEN];
        for px in framebuffer.chunks_exact_mut(4) {
            px.copy_from_slice(&[0, 0, 0, 0xFF]);
        }
        Self {
            framebuffer,
            frame_ready: false,
            vcount: 0,
            line_cycles: 0,
        }
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    pub fn dimensions(&self) -> (usize, usize) {
        (SCREEN_WIDTH, SCREEN_HEIGHT)
    }

    pub fn vcount(&self) -> u16 {
        self.vcount
    }

    pub fn in_vblank(&self) -> bool {
        self.vcount >= VISIBLE_SCANLINES
    }

    pub fn debug_snapshot(&self, io: &[u8]) -> PpuDebugSnapshot {
        let dispcnt = read_le16(io, 0);
        PpuDebugSnapshot {
            dispcnt,
            bgcnt: std::array::from_fn(|i| read_le16(io, 0x08 + i * 2)),
            vcount: self.vcount,
            in_vblank: self.in_vblank(),
            display_mode: dispcnt & 0x7,
            bg_enabled: std::array::from_fn(|i| dispcnt & (1 << (8 + i)) != 0),
            obj_enabled: dispcnt & (1 << 12) != 0,
            obj_mapping_1d: dispcnt & (1 << 6) != 0,
            non_black_pixels: self
                .framebuffer
                .chunks_exact(4)
                .filter(|px| px[0] != 0 || px[1] != 0 || px[2] != 0)
                .count(),
        }
    }

    pub fn step_cycles(&mut self, cycles: u32) {
        self.line_cycles = self.line_cycles.saturating_add(cycles);
        while self.line_cycles >= CYCLES_PER_SCANLINE {
            self.line_cycles -= CYCLES_PER_SCANLINE;
            self.vcount = self.vcount.wrapping_add(1);
            if self.vcount >= SCANLINES_PER_FRAME {
                self.vcount = 0;
            }
        }
    }

    pub fn step_frame(&mut self, io: &[u8], palette_ram: &[u8], vram: &[u8], oam: &[u8]) {
        self.render(io, palette_ram, vram, oam);
        self.frame_ready = true;
    }
}

fn read_le16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([
        data.get(offset).copied().unwrap_or(0),
        data.get(offset + 1).copied().unwrap_or(0),
    ])
}
