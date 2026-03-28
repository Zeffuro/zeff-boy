use super::PocketCamera;

pub const CAMERA_WIDTH: usize = 128;
pub const CAMERA_HEIGHT: usize = 112;
pub const CAMERA_PIXELS: usize = CAMERA_WIDTH * CAMERA_HEIGHT;
#[cfg_attr(not(test), allow(dead_code))]
pub const CAMERA_IMAGE_BYTES: usize = (CAMERA_WIDTH / 8) * (CAMERA_HEIGHT / 8) * 16;

pub const SENSOR_REG_COUNT: usize = 0x36;

const CYCLES_PER_EXPOSURE_UNIT: u64 = 128;
const CAPTURE_OVERHEAD_CYCLES: u64 = 512;
const MAX_EXPOSURE_UNITS: u16 = 0x0800;

pub const IMAGE_RAM_OFFSET: usize = 0x100;

impl PocketCamera {
    pub(super) fn start_capture(&mut self) {
        let raw_exposure = self.exposure_time();
        let exposure = raw_exposure.min(MAX_EXPOSURE_UNITS);
        self.capture_cycles_remaining =
            CAPTURE_OVERHEAD_CYCLES + (exposure as u64) * CYCLES_PER_EXPOSURE_UNIT;
        self.capture_active = true;
        if raw_exposure != exposure {
            log::debug!(
                "Camera exposure clamped: raw={} capped={}",
                raw_exposure,
                exposure
            );
        }
        log::debug!(
            "Camera capture started: exposure N={}, cycles={}",
            exposure,
            self.capture_cycles_remaining
        );
    }

    pub(super) fn tick_capture(&mut self, t_cycles: u64) {
        if !self.capture_active {
            return;
        }

        if self.capture_cycles_remaining <= t_cycles {
            self.capture_cycles_remaining = 0;
            self.capture_active = false;
            self.sensor_regs[0] &= !0x01;
            self.process_captured_image();
            log::debug!("Camera capture complete");
        } else {
            self.capture_cycles_remaining -= t_cycles;
        }
    }

    pub(super) fn read_sensor_reg(&self, offset: usize) -> u8 {
        if offset >= SENSOR_REG_COUNT {
            return 0x00;
        }

        if offset == 0 {
            let busy = u8::from(self.capture_active);
            return (self.sensor_regs[0] & !0x01) | busy;
        }

        self.sensor_regs[offset]
    }

    pub(super) fn write_sensor_reg(&mut self, offset: usize, value: u8) {
        if offset >= SENSOR_REG_COUNT {
            return;
        }

        if offset == 0 {
            self.sensor_regs[0] = value;
            if value & 0x01 != 0 {
                self.start_capture();
            }
        } else {
            self.sensor_regs[offset] = value;
        }
    }

    fn exposure_time(&self) -> u16 {
        ((self.sensor_regs[2] as u16) << 8) | (self.sensor_regs[3] as u16)
    }

    fn process_captured_image(&mut self) {
        let edge_mode = (self.sensor_regs[0] >> 1) & 0x03;
        let dither_matrix = &self.sensor_regs[6..SENSOR_REG_COUNT];

        // Build the processed grayscale buffer from the host frame
        let processed = self.apply_sensor_processing(edge_mode, dither_matrix);

        // Encode into 2bpp Game Boy tile format
        let tiles = encode_2bpp_tiles(&processed, CAMERA_WIDTH, CAMERA_HEIGHT);

        // Write into RAM bank 0 at IMAGE_RAM_OFFSET
        let ram_end = IMAGE_RAM_OFFSET + tiles.len();
        if ram_end <= self.ram.len() {
            self.ram[IMAGE_RAM_OFFSET..ram_end].copy_from_slice(&tiles);
        }
    }

    fn apply_sensor_processing(&self, edge_mode: u8, dither_matrix: &[u8]) -> Vec<u8> {
        let src = &self.host_frame;
        let w = CAMERA_WIDTH;
        let h = CAMERA_HEIGHT;

        let mut buf = vec![0u8; w * h];

        let copy_len = buf.len().min(src.len());
        buf[..copy_len].copy_from_slice(&src[..copy_len]);

        if edge_mode != 0 {
            let mut enhanced = buf.clone();
            for y in 0..h {
                for x in 1..(w - 1) {
                    let idx = y * w + x;
                    let left = buf[idx - 1] as i16;
                    let center = buf[idx] as i16;
                    let right = buf[idx + 1] as i16;
                    let edge = (2 * center - left - right).clamp(-128, 127);
                    let strength = match edge_mode {
                        1 => 1,
                        2 => 2,
                        3 => 3,
                        _ => 0,
                    };
                    let val = (center + (edge * strength) / 4).clamp(0, 255) as u8;
                    enhanced[idx] = val;
                }
            }
            buf = enhanced;
        }

        let mut result = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let idx = y * w + x;
                let pixel = buf[idx];
                let dx = x & 3;
                let dy = y & 3;
                let mat_idx = dy * 4 + dx;

                let (t0, t1, t2) = threshold_triplet(dither_matrix, mat_idx);
                let p = pixel;

                result[idx] = if p < t0 {
                    3
                } else if p < t1 {
                    2
                } else if p < t2 {
                    1
                } else {
                    0
                };
            }
        }

        result
    }
}

fn encode_2bpp_tiles(pixels: &[u8], width: usize, height: usize) -> Vec<u8> {
    let tiles_x = width / 8;
    let tiles_y = height / 8;
    let mut out = vec![0u8; tiles_x * tiles_y * 16];

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let tile_idx = ty * tiles_x + tx;
            let tile_base = tile_idx * 16;

            for row in 0..8 {
                let py = ty * 8 + row;
                let mut lo = 0u8;
                let mut hi = 0u8;

                for col in 0..8 {
                    let px = tx * 8 + col;
                    let val = if py < height && px < width {
                        pixels[py * width + px] & 0x03
                    } else {
                        0
                    };

                    let bit = 7 - col;
                    lo |= (val & 0x01) << bit;
                    hi |= ((val >> 1) & 0x01) << bit;
                }

                out[tile_base + row * 2] = lo;
                out[tile_base + row * 2 + 1] = hi;
            }
        }
    }

    out
}

fn threshold_triplet(dither_matrix: &[u8], idx: usize) -> (u8, u8, u8) {
    const BASE: (u8, u8, u8) = (64, 128, 192);
    if dither_matrix.len() < 48 || idx >= 16 {
        return BASE;
    }

    // Pan Docs: 48 threshold bytes arranged as three 4x4 planes.
    let mut t = [
        dither_matrix[idx],
        dither_matrix[16 + idx],
        dither_matrix[32 + idx],
    ];
    t.sort_unstable();

    // If the game did not initialize these registers yet, keep legacy behavior.
    if t[0] == t[1] && t[1] == t[2] {
        return BASE;
    }

    (t[0], t[1], t[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_2bpp_single_tile_all_white() {
        // 2-bit value 0 = white in GB
        let pixels = vec![0u8; 64];
        let tiles = encode_2bpp_tiles(&pixels, 8, 8);
        assert_eq!(tiles.len(), 16);
        assert!(tiles.iter().all(|&b| b == 0));
    }

    #[test]
    fn encode_2bpp_single_tile_all_black() {
        // 2-bit value 3 = darkest
        let pixels = vec![3u8; 64];
        let tiles = encode_2bpp_tiles(&pixels, 8, 8);
        assert_eq!(tiles.len(), 16);
        // Each row: lo=0xFF, hi=0xFF
        for row in 0..8 {
            assert_eq!(tiles[row * 2], 0xFF);
            assert_eq!(tiles[row * 2 + 1], 0xFF);
        }
    }

    #[test]
    fn encode_2bpp_correct_tile_count() {
        let pixels = vec![0u8; CAMERA_WIDTH * CAMERA_HEIGHT];
        let tiles = encode_2bpp_tiles(&pixels, CAMERA_WIDTH, CAMERA_HEIGHT);
        assert_eq!(tiles.len(), CAMERA_IMAGE_BYTES);
    }

    #[test]
    fn camera_image_bytes_matches_spec() {
        assert_eq!(CAMERA_IMAGE_BYTES, 16 * 14 * 16);
    }

    #[test]
    fn threshold_triplet_uses_dither_planes() {
        let mut regs = [0u8; 48];
        regs[0] = 40;
        regs[16] = 120;
        regs[32] = 200;
        assert_eq!(threshold_triplet(&regs, 0), (40, 120, 200));
    }

    #[test]
    fn threshold_triplet_falls_back_when_uninitialized() {
        let regs = [0u8; 48];
        assert_eq!(threshold_triplet(&regs, 0), (64, 128, 192));
    }
}
