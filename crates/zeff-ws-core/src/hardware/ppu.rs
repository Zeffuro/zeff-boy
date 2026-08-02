use super::constants::{
    CYCLES_PER_SCANLINE, FRAMEBUFFER_LEN, SCANLINES_PER_FRAME, SCREEN_HEIGHT, SCREEN_WIDTH,
};

const VISIBLE_SCANLINES: u16 = SCREEN_HEIGHT as u16;

#[derive(Clone, Debug)]
pub struct Ppu {
    framebuffer: Vec<u8>,
    pub frame_ready: bool,
    vcount: u16,
    line_cycles: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PpuDebugSnapshot {
    pub vcount: u16,
    pub line_cycles: u32,
    pub in_vblank: bool,
    pub frame_ready: bool,
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
            px.copy_from_slice(&[0xC8, 0xD0, 0xB0, 0xFF]);
        }
        Self {
            framebuffer,
            frame_ready: false,
            vcount: 0,
            line_cycles: 0,
        }
    }

    pub fn reset(&mut self) {
        self.frame_ready = false;
        self.vcount = 0;
        self.line_cycles = 0;
        self.render_power_on_frame();
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    pub fn framebuffer_mut(&mut self) -> &mut [u8] {
        &mut self.framebuffer
    }

    pub fn dimensions(&self) -> (usize, usize) {
        (SCREEN_WIDTH, SCREEN_HEIGHT)
    }

    pub fn vcount(&self) -> u16 {
        self.vcount
    }

    pub fn line_cycles(&self) -> u32 {
        self.line_cycles
    }

    pub fn set_timing_state(&mut self, vcount: u16, line_cycles: u32, frame_ready: bool) {
        self.vcount = vcount % SCANLINES_PER_FRAME;
        self.line_cycles = line_cycles % CYCLES_PER_SCANLINE;
        self.frame_ready = frame_ready;
    }

    pub fn in_vblank(&self) -> bool {
        self.vcount >= VISIBLE_SCANLINES
    }

    pub fn step_cycles(&mut self, cycles: u32, ram: &[u8], io: &[u8]) {
        let mut remaining = cycles;
        while remaining > 0 {
            let until_next_line = CYCLES_PER_SCANLINE - self.line_cycles;
            let step = remaining.min(until_next_line);
            self.line_cycles += step;
            remaining -= step;
            if self.line_cycles == CYCLES_PER_SCANLINE {
                self.line_cycles = 0;
                self.vcount += 1;
                if self.vcount == SCANLINES_PER_FRAME {
                    self.vcount = 0;
                    self.render_frame(ram, io);
                    self.frame_ready = true;
                }
            }
        }
    }

    pub fn render_frame(&mut self, ram: &[u8], io: &[u8]) {
        // This is a deliberately small renderer until the tile/sprite engine is implemented.
        // It gives deterministic visual output from VRAM instead of hiding all execution behind
        // a black placeholder frame.
        let shade_base = io.get(0x01).copied().unwrap_or(0) & 0x03;
        for y in 0..SCREEN_HEIGHT {
            for x in 0..SCREEN_WIDTH {
                let ram_index = 0x2000 + ((y * SCREEN_WIDTH + x) >> 2);
                let packed = ram.get(ram_index % ram.len()).copied().unwrap_or(0);
                let shift = (x & 0x03) * 2;
                let shade = (packed >> shift) & 0x03;
                let lum = mono_luma(shade.wrapping_add(shade_base) & 0x03);
                let pixel = (y * SCREEN_WIDTH + x) * 4;
                self.framebuffer[pixel..pixel + 4].copy_from_slice(&[lum, lum, lum, 0xFF]);
            }
        }
    }

    pub fn debug_snapshot(&self) -> PpuDebugSnapshot {
        PpuDebugSnapshot {
            vcount: self.vcount,
            line_cycles: self.line_cycles,
            in_vblank: self.in_vblank(),
            frame_ready: self.frame_ready,
        }
    }

    fn render_power_on_frame(&mut self) {
        for px in self.framebuffer.chunks_exact_mut(4) {
            px.copy_from_slice(&[0xC8, 0xD0, 0xB0, 0xFF]);
        }
    }
}

fn mono_luma(shade: u8) -> u8 {
    match shade & 0x03 {
        0 => 0xF0,
        1 => 0xA8,
        2 => 0x60,
        _ => 0x18,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::constants::CYCLES_PER_FRAME;

    #[test]
    fn frame_becomes_ready_after_one_frame_of_cycles() {
        let mut ppu = Ppu::new();
        let ram = vec![0; 0x10000];
        let io = vec![0; 0x10000];
        ppu.step_cycles(CYCLES_PER_FRAME - 1, &ram, &io);
        assert!(!ppu.frame_ready);
        ppu.step_cycles(1, &ram, &io);
        assert!(ppu.frame_ready);
        assert_eq!(ppu.vcount(), 0);
    }

    #[test]
    fn framebuffer_has_ws_dimensions() {
        let ppu = Ppu::new();
        assert_eq!(ppu.dimensions(), (224, 144));
        assert_eq!(ppu.framebuffer().len(), 224 * 144 * 4);
    }
}
