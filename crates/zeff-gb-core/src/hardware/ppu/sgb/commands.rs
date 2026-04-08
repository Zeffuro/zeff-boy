use super::super::{
    Lcdc, PPU, SCREEN_W, SGB_ATTR_BLOCKS_H, SGB_ATTR_BLOCKS_W, SGB_BORDER_COLORS_PER_PALETTE,
    SGB_BORDER_PALETTES, SGB_BORDER_TILEMAP_SIZE, SGB_TRN_TRANSFER_SIZE, tile_data_address,
};

impl PPU {
    pub fn sgb_apply_attr_blk(&mut self, data: &[u8]) {
        if data.len() < 2 {
            return;
        }
        let count = data[1] as usize;
        let mut offset = 2;

        for _ in 0..count {
            if offset + 6 > data.len() {
                break;
            }

            let control = data[offset];
            let palettes = data[offset + 1];
            let x1 = (data[offset + 2] as usize).min(SGB_ATTR_BLOCKS_W - 1);
            let y1 = (data[offset + 3] as usize).min(SGB_ATTR_BLOCKS_H - 1);
            let x2 = (data[offset + 4] as usize).min(SGB_ATTR_BLOCKS_W - 1);
            let y2 = (data[offset + 5] as usize).min(SGB_ATTR_BLOCKS_H - 1);
            offset += 6;

            let pal_inside = palettes & 3;
            let pal_border = (palettes >> 2) & 3;
            let pal_outside = (palettes >> 4) & 3;

            let change_inside = control & 1 != 0;
            let change_border = control & 2 != 0;
            let change_outside = control & 4 != 0;

            if !change_inside && !change_border && !change_outside {
                continue;
            }

            for ty in 0..SGB_ATTR_BLOCKS_H {
                for tx in 0..SGB_ATTR_BLOCKS_W {
                    let in_rect = tx >= x1 && tx <= x2 && ty >= y1 && ty <= y2;
                    let on_border_x = tx == x1 || tx == x2;
                    let on_border_y = ty == y1 || ty == y2;
                    let on_border = in_rect && (on_border_x || on_border_y);
                    let strictly_inside = in_rect && !on_border;

                    let idx = ty * SGB_ATTR_BLOCKS_W + tx;

                    if on_border {
                        if change_border {
                            self.sgb_attr_map[idx] = pal_border;
                        } else if change_inside {
                            self.sgb_attr_map[idx] = pal_inside;
                        } else if change_outside {
                            self.sgb_attr_map[idx] = pal_outside;
                        }
                    } else if strictly_inside && change_inside {
                        self.sgb_attr_map[idx] = pal_inside;
                    } else if !in_rect && change_outside {
                        self.sgb_attr_map[idx] = pal_outside;
                    }
                }
            }
        }
        log::info!("SGB ATTR_BLK applied: {} data set(s)", count);
    }

    pub fn sgb_apply_attr_lin(&mut self, data: &[u8]) {
        if data.len() < 2 {
            return;
        }
        let count = data[1] as usize;
        let mut offset = 2;

        for _ in 0..count {
            if offset >= data.len() {
                break;
            }
            let byte = data[offset];
            offset += 1;

            let line = (byte & 0x1F) as usize;
            let palette = (byte >> 5) & 3;
            let is_vertical = byte & 0x80 != 0;

            if is_vertical {
                if line < SGB_ATTR_BLOCKS_W {
                    for ty in 0..SGB_ATTR_BLOCKS_H {
                        self.sgb_attr_map[ty * SGB_ATTR_BLOCKS_W + line] = palette;
                    }
                }
            } else if line < SGB_ATTR_BLOCKS_H {
                for tx in 0..SGB_ATTR_BLOCKS_W {
                    self.sgb_attr_map[line * SGB_ATTR_BLOCKS_W + tx] = palette;
                }
            }
        }
        log::info!("SGB ATTR_LIN applied: {} data set(s)", count);
    }

    pub fn sgb_apply_attr_div(&mut self, packet: &[u8; 16]) {
        let byte1 = packet[1];
        let is_vertical = byte1 & 0x40 != 0;
        let pal_above_left = byte1 & 3;
        let pal_on_line = (byte1 >> 2) & 3;
        let pal_below_right = (byte1 >> 4) & 3;
        let line = packet[2] as usize;

        if is_vertical {
            let line = line.min(SGB_ATTR_BLOCKS_W - 1);
            for ty in 0..SGB_ATTR_BLOCKS_H {
                for tx in 0..SGB_ATTR_BLOCKS_W {
                    let idx = ty * SGB_ATTR_BLOCKS_W + tx;
                    self.sgb_attr_map[idx] = if tx < line {
                        pal_above_left
                    } else if tx == line {
                        pal_on_line
                    } else {
                        pal_below_right
                    };
                }
            }
        } else {
            let line = line.min(SGB_ATTR_BLOCKS_H - 1);
            for ty in 0..SGB_ATTR_BLOCKS_H {
                for tx in 0..SGB_ATTR_BLOCKS_W {
                    let idx = ty * SGB_ATTR_BLOCKS_W + tx;
                    self.sgb_attr_map[idx] = if ty < line {
                        pal_above_left
                    } else if ty == line {
                        pal_on_line
                    } else {
                        pal_below_right
                    };
                }
            }
        }
        log::info!(
            "SGB ATTR_DIV applied: vertical={}, line={}, pals=[{},{},{}]",
            is_vertical,
            line,
            pal_above_left,
            pal_on_line,
            pal_below_right
        );
    }

    pub fn sgb_apply_attr_chr(&mut self, data: &[u8]) {
        if data.len() < 6 {
            return;
        }
        let start_x = data[1] as usize;
        let start_y = data[2] as usize;
        let count = u16::from_le_bytes([data[3], data[4]]) as usize;
        let is_horizontal = data[5] == 0;

        let mut x = start_x;
        let mut y = start_y;
        let mut data_offset = 6;
        let mut bit_offset: u8 = 0;

        for _ in 0..count {
            if data_offset >= data.len() || x >= SGB_ATTR_BLOCKS_W || y >= SGB_ATTR_BLOCKS_H {
                break;
            }

            let byte = data[data_offset];
            let shift = 6 - bit_offset * 2;
            let palette = (byte >> shift) & 3;

            let idx = y * SGB_ATTR_BLOCKS_W + x;
            if idx < self.sgb_attr_map.len() {
                self.sgb_attr_map[idx] = palette;
            }

            bit_offset += 1;
            if bit_offset == 4 {
                bit_offset = 0;
                data_offset += 1;
            }

            if is_horizontal {
                x += 1;
                if x >= SGB_ATTR_BLOCKS_W {
                    x = 0;
                    y += 1;
                }
            } else {
                y += 1;
                if y >= SGB_ATTR_BLOCKS_H {
                    y = 0;
                    x += 1;
                }
            }
        }
        log::info!(
            "SGB ATTR_CHR applied: {} tiles from ({},{})",
            count,
            start_x,
            start_y
        );
    }

    pub fn sgb_attr_set(&mut self, file_index: u8, cancel_mask: bool) {
        let file_idx = file_index as usize;
        if file_idx >= 45 {
            log::warn!("SGB ATTR_SET: invalid file index {}", file_idx);
            return;
        }

        let file_base = file_idx * 90;
        if file_base + 90 > self.sgb_attr_trn_data.len() {
            log::warn!(
                "SGB ATTR_SET: ATTR_TRN data too short for file {}",
                file_idx
            );
            return;
        }

        let mut map_idx = 0;
        for byte_idx in 0..90 {
            let byte = self.sgb_attr_trn_data[file_base + byte_idx];
            for shift in (0..4).rev() {
                if map_idx >= self.sgb_attr_map.len() {
                    break;
                }
                self.sgb_attr_map[map_idx] = (byte >> (shift * 2)) & 3;
                map_idx += 1;
            }
        }

        if cancel_mask {
            self.sgb_mask_mode = 0;
        }
        log::info!(
            "SGB ATTR_SET applied: file={}, cancel_mask={}",
            file_idx,
            cancel_mask
        );
    }

    pub fn sgb_attr_trn(&mut self, vram: &[u8], _vram_bank: u8) {
        let trn = self.capture_sgb_trn_buffer(vram);
        log_transfer_stats("ATTR_TRN", &trn);
        self.sgb_attr_trn_data.copy_from_slice(&trn);
        log::info!("SGB ATTR_TRN stored: {} bytes", SGB_TRN_TRANSFER_SIZE);
    }

    pub fn sgb_chr_trn(&mut self, vram: &[u8], _vram_bank: u8, transfer_bank: u8) {
        let trn = self.capture_sgb_trn_buffer(vram);
        log_transfer_stats("CHR_TRN", &trn);
        let len = SGB_TRN_TRANSFER_SIZE.min(trn.len());
        let dst_addr = usize::from(transfer_bank & 0x01) * SGB_TRN_TRANSFER_SIZE;
        self.sgb_border_tile_data[dst_addr..dst_addr + len].copy_from_slice(&trn[..len]);
        let tile_non_zero = self
            .sgb_border_tile_data
            .iter()
            .filter(|&&b| b != 0)
            .count();
        log::info!(
            "SGB CHR_TRN applied: bank={}, non_zero_tile_bytes={}/{}",
            transfer_bank & 0x01,
            tile_non_zero,
            self.sgb_border_tile_data.len()
        );
    }

    pub fn sgb_pal_trn(&mut self, vram: &[u8], _vram_bank: u8) {
        let trn = self.capture_sgb_trn_buffer(vram);
        log_transfer_stats("PAL_TRN", &trn);
        self.sgb_pal_trn_data.copy_from_slice(&trn);
        let non_zero_palettes = trn.chunks(8).filter(|c| c.iter().any(|&b| b != 0)).count();
        log::info!(
            "SGB PAL_TRN stored: {}/{} system palettes with non-zero colors",
            non_zero_palettes,
            SGB_TRN_TRANSFER_SIZE / 8
        );
    }

    pub fn sgb_pct_trn(&mut self, vram: &[u8], _vram_bank: u8) {
        let trn = self.capture_sgb_trn_buffer(vram);
        log_transfer_stats("PCT_TRN", &trn);

        for i in 0..SGB_BORDER_TILEMAP_SIZE {
            let offset = i * 2;
            self.sgb_border_tilemap[i] = u16::from_le_bytes([trn[offset], trn[offset + 1]]);
        }

        let palette_offset = SGB_BORDER_TILEMAP_SIZE * 2;
        self.sgb_border_palettes = [[0; SGB_BORDER_COLORS_PER_PALETTE]; SGB_BORDER_PALETTES];
        for palette in 0..4usize {
            for color in 0..16usize {
                let offset = palette_offset + ((palette * 16 + color) * 2);
                if offset + 1 >= trn.len() {
                    break;
                }
                let c = u16::from_le_bytes([trn[offset], trn[offset + 1]]);
                self.sgb_border_palettes[palette][color] = c;
                self.sgb_border_palettes[palette + 4][color] = c;
            }
        }

        let tilemap_non_zero = self.sgb_border_tilemap.iter().filter(|&&e| e != 0).count();
        let palette_non_zero = self
            .sgb_border_palettes
            .iter()
            .flat_map(|p| p.iter())
            .filter(|&&c| c != 0)
            .count();
        log::info!(
            "SGB PCT_TRN applied: non_zero_tilemap_entries={}/{}, non_zero_palette_colors={}/{}",
            tilemap_non_zero,
            self.sgb_border_tilemap.len(),
            palette_non_zero,
            self.sgb_border_palettes.len() * 16
        );
    }

    fn capture_sgb_trn_buffer(&self, vram: &[u8]) -> [u8; SGB_TRN_TRANSFER_SIZE] {
        let mut trn = [0u8; SGB_TRN_TRANSFER_SIZE];

        let tile_map_base: usize = if self.lcdc.contains(Lcdc::BG_TILEMAP) {
            0x1C00
        } else {
            0x1800
        };
        let tile_data_unsigned = self.lcdc.contains(Lcdc::TILE_DATA);
        let tiles_per_row = SCREEN_W / 8;
        let total_tiles = SGB_TRN_TRANSFER_SIZE / 16;

        for idx in 0..total_tiles {
            let tile_y = idx / tiles_per_row;
            let tile_x = idx % tiles_per_row;
            let out_base = idx * 16;

            let map_addr = tile_map_base + tile_y * 32 + tile_x;
            let tile_index = vram.get(map_addr).copied().unwrap_or(0);
            let tile_addr = tile_data_address(tile_index, tile_data_unsigned);

            for i in 0..16usize {
                trn[out_base + i] = vram.get(tile_addr + i).copied().unwrap_or(0);
            }
        }

        trn
    }
}

fn log_transfer_stats(label: &str, trn: &[u8; SGB_TRN_TRANSFER_SIZE]) {
    let non_zero = trn.iter().filter(|&&b| b != 0).count();
    let checksum = trn.iter().fold(0u32, |acc, &b| {
        acc.wrapping_mul(16777619).wrapping_add(u32::from(b))
    });
    let first_non_zero = trn.iter().position(|&b| b != 0).unwrap_or(usize::MAX);
    log::info!(
        "SGB {} capture: non_zero_bytes={}/{}, first_non_zero={}, checksum=0x{:08X}",
        label,
        non_zero,
        trn.len(),
        first_non_zero,
        checksum
    );
}
