use crate::cheats::NesCheatState;
use crate::hardware::apu::Apu;
use crate::hardware::cartridge::{Cartridge, ChrFetchKind, Mirroring};
use crate::hardware::constants::*;
use crate::hardware::controller::Controller;
use crate::hardware::ppu::{
    NES_PALETTE, NesPaletteMode, PRE_RENDER_SCANLINE, Ppu, apply_nes_palette_mode,
};
use std::fmt;

pub enum DebugTraceEvent {
    Read {
        addr: u16,
        value: u8,
    },
    Write {
        addr: u16,
        old_value: u8,
        new_value: u8,
    },
}

pub struct Bus {
    pub ram: [u8; RAM_SIZE],
    pub(crate) ppu: Ppu,
    pub apu: Apu,
    pub cartridge: Cartridge,
    pub controller1: Controller,
    pub controller2: Controller,

    pub(crate) ppu_cycles: u64,

    pub(crate) dma_stall_cycles: u64,

    pub(crate) cpu_odd_cycle: bool,
    pub(crate) cpu_open_bus: u8,
    pub game_genie: NesCheatState,
    pub palette_mode: NesPaletteMode,

    pub(crate) debug_trace_enabled: bool,
    pub(crate) debug_trace_events: Vec<DebugTraceEvent>,
}

impl Bus {
    pub fn new(cartridge: Cartridge, sample_rate: f64) -> Self {
        Self {
            ram: [0; RAM_SIZE],
            ppu: Ppu::new(),
            apu: Apu::new(sample_rate),
            cartridge,
            controller1: Controller::new(),
            controller2: Controller::new(),
            ppu_cycles: 0,
            dma_stall_cycles: 0,
            cpu_odd_cycle: false,
            cpu_open_bus: 0,
            game_genie: NesCheatState::new(),
            palette_mode: NesPaletteMode::default(),
            debug_trace_enabled: false,
            debug_trace_events: Vec::new(),
        }
    }

    pub fn set_palette_mode(&mut self, mode: NesPaletteMode) {
        self.palette_mode = mode;
    }

    pub fn palette_mode(&self) -> NesPaletteMode {
        self.palette_mode
    }

    pub fn palette_color_rgba(&self, pal_idx: u8) -> [u8; 4] {
        let (r, g, b) = NES_PALETTE[(pal_idx & 0x3F) as usize];
        let (r, g, b) = apply_nes_palette_mode(self.palette_mode, (r, g, b));
        [r, g, b, 255]
    }

    #[inline]
    pub fn cpu_read(&mut self, addr: u16) -> u8 {
        let val = match addr {
            0x0000..=0x1FFF => self.ram[(addr & RAM_MIRROR_MASK) as usize],
            0x2000..=0x3FFF => self.ppu_read_register(addr & PPU_REG_MIRROR_MASK),
            0x4000..=0x4013 => self.cpu_open_bus,
            OAM_DMA => self.cpu_open_bus,
            APU_STATUS => self.apu.read_status(),
            CONTROLLER1 => self.controller1.read(),
            CONTROLLER2 => self.controller2.read(),
            0x4018..=0x401F => self.cpu_open_bus,
            0x4020..=0x7FFF => self.cartridge.cpu_read(addr),
            0x8000..=0xFFFF => {
                let rom_val = self.cartridge.cpu_read(addr);
                self.game_genie.intercept(addr, rom_val).unwrap_or(rom_val)
            }
        };
        self.cpu_open_bus = val;
        if self.debug_trace_enabled {
            self.debug_trace_events
                .push(DebugTraceEvent::Read { addr, value: val });
        }
        val
    }

    #[inline]
    pub fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & RAM_MIRROR_MASK) as usize],
            0x2000..=0x3FFF => self.ppu.peek_register(addr & PPU_REG_MIRROR_MASK),
            0x4000..=0x4013 => 0,
            OAM_DMA => 0,
            APU_STATUS => self.apu.peek_status(),
            CONTROLLER1 => 0,
            CONTROLLER2 => 0,
            0x4018..=0x401F => 0,
            0x4020..=0xFFFF => self.cartridge.cpu_peek(addr),
        }
    }

    #[inline]
    pub fn cpu_write(&mut self, addr: u16, val: u8) {
        if self.debug_trace_enabled {
            let old = self.cpu_peek(addr);
            self.debug_trace_events.push(DebugTraceEvent::Write {
                addr,
                old_value: old,
                new_value: val,
            });
        }
        match addr {
            0x0000..=0x1FFF => {
                self.ram[(addr & RAM_MIRROR_MASK) as usize] = val;
            }

            0x2000..=0x3FFF => {
                self.ppu_write_register(addr & PPU_REG_MIRROR_MASK, val);
            }

            0x4000..=0x4013 | APU_STATUS | CONTROLLER2 => {
                self.apu.write_register(addr, val, self.cpu_odd_cycle);
            }

            OAM_DMA => {
                let base = (val as u16) << 8;
                for i in 0..256u16 {
                    let byte = self.cpu_read(base + i);
                    self.ppu.oam[self.ppu.oam_addr as usize] = byte;
                    self.ppu.oam_addr = self.ppu.oam_addr.wrapping_add(1);
                }

                self.dma_stall_cycles = if self.cpu_odd_cycle { 514 } else { 513 };
            }

            CONTROLLER1 => {
                self.controller1.write(val);
                self.controller2.write(val);
            }

            0x4018..=0x401F => { /* test mode registers:ignored */ }

            0x4020..=0xFFFF => {
                self.cartridge.cpu_write(addr, val);
            }
        }
    }

    fn ppu_read_register(&mut self, addr: u16) -> u8 {
        let result = match addr {
            0x2002 => {
                let status = (self.ppu.regs.status & 0xE0) | (self.ppu.io_latch & 0x1F);
                self.ppu.regs.clear_vblank();
                self.ppu.w = false;
                status
            }
            0x2004 => self.ppu.oam[self.ppu.oam_addr as usize],
            0x2007 => {
                let addr = self.ppu.v & 0x3FFF;
                let mut data = self.ppu.read_buffer;

                if addr >= 0x3F00 {
                    data = self.ppu_bus_read(addr);
                    self.ppu.read_buffer = self.ppu_bus_read(addr - 0x1000);
                } else {
                    self.ppu.read_buffer = self.ppu_bus_read(addr);
                }

                self.ppu.v = self.ppu.v.wrapping_add(self.ppu.regs.vram_increment());
                data
            }
            _ => self.ppu.io_latch,
        };
        self.ppu.io_latch = result;
        result
    }

    fn ppu_write_register(&mut self, addr: u16, val: u8) {
        self.ppu.io_latch = val;
        match addr {
            0x2000 => {
                self.ppu.regs.ctrl = val;
                self.ppu.t = (self.ppu.t & 0xF3FF) | ((val as u16 & 0x03) << 10);
            }
            0x2001 => {
                self.ppu.regs.mask = val;
            }
            0x2003 => {
                self.ppu.oam_addr = val;
            }
            0x2004 => {
                self.ppu.oam[self.ppu.oam_addr as usize] = val;
                self.ppu.oam_addr = self.ppu.oam_addr.wrapping_add(1);
            }
            0x2005 => {
                if !self.ppu.w {
                    self.ppu.t = (self.ppu.t & 0xFFE0) | ((val as u16) >> 3);
                    self.ppu.fine_x = val & 0x07;
                } else {
                    self.ppu.t = (self.ppu.t & 0x8C1F)
                        | ((val as u16 & 0x07) << 12)
                        | ((val as u16 & 0xF8) << 2);
                }
                self.ppu.w = !self.ppu.w;
            }
            0x2006 => {
                if !self.ppu.w {
                    self.ppu.t = (self.ppu.t & 0x00FF) | ((val as u16 & 0x3F) << 8);
                } else {
                    self.ppu.t = (self.ppu.t & 0xFF00) | val as u16;
                    self.ppu.v = self.ppu.t;
                }
                self.ppu.w = !self.ppu.w;
            }
            0x2007 => {
                let addr = self.ppu.v & 0x3FFF;
                self.ppu_bus_write(addr, val);
                self.ppu.v = self.ppu.v.wrapping_add(self.ppu.regs.vram_increment());
            }
            _ => {}
        }
    }

    #[inline]
    pub fn ppu_bus_read(&self, addr: u16) -> u8 {
        self.ppu_bus_read_with_kind(addr, ChrFetchKind::Background)
    }

    #[inline]
    fn ppu_bus_read_with_kind(&self, addr: u16, kind: ChrFetchKind) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.cartridge.chr_read_with_kind(addr, kind),
            0x2000..=0x3EFF => {
                if let Some(val) = self
                    .cartridge
                    .ppu_nametable_read(addr, &self.ppu.nametable_ram)
                {
                    val
                } else {
                    let mirrored = self.mirror_nametable_addr(addr);
                    self.ppu.nametable_ram[mirrored]
                }
            }
            0x3F00..=0x3FFF => {
                let idx = Self::palette_index(addr);
                self.ppu.palette_ram[idx]
            }
            _ => 0,
        }
    }

    pub fn ppu_bus_write(&mut self, addr: u16, val: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.cartridge.chr_write(addr, val),
            0x2000..=0x3EFF => {
                if !self
                    .cartridge
                    .ppu_nametable_write(addr, val, &mut self.ppu.nametable_ram)
                {
                    let mirrored = self.mirror_nametable_addr(addr);
                    self.ppu.nametable_ram[mirrored] = val;
                }
            }
            0x3F00..=0x3FFF => {
                let idx = Self::palette_index(addr);
                self.ppu.palette_ram[idx] = val;
            }
            _ => {}
        }
    }

    fn mirror_nametable_addr(&self, addr: u16) -> usize {
        let addr = (addr - 0x2000) & 0x0FFF;
        match self.cartridge.mirroring() {
            Mirroring::Horizontal => {
                let table = (addr / 0x0400) & 0x03;
                let offset = addr & 0x03FF;
                let physical = match table {
                    0 | 1 => offset,
                    2 | 3 => 0x0400 + offset,
                    _ => unreachable!(),
                };
                physical as usize
            }
            Mirroring::Vertical => (addr & 0x07FF) as usize,
            Mirroring::SingleScreenLower => (addr & 0x03FF) as usize,
            Mirroring::SingleScreenUpper => (0x0400 + (addr & 0x03FF)) as usize,
            Mirroring::FourScreen => addr as usize,
        }
    }

    fn palette_index(addr: u16) -> usize {
        let mut idx = (addr & 0x1F) as usize;
        if idx >= 16 && idx.is_multiple_of(4) {
            idx -= 16;
        }
        idx
    }

    fn ppu_render_dot(&mut self) {
        let scanline = self.ppu.scanline;
        let dot = self.ppu.dot;
        let rendering = self.ppu.regs.rendering_enabled();
        let visible_line = scanline < 240;
        let pre_render = scanline == PRE_RENDER_SCANLINE;
        let render_line = visible_line || pre_render;

        if rendering && render_line {
            let bg_hi = self.ppu.regs.bg_pattern_addr() != 0;
            let spr_hi = self.ppu.regs.sprite_pattern_addr() != 0;
            let notify_dot = if bg_hi && !spr_hi { 324 } else { 260 };
            if dot == notify_dot {
                self.cartridge.notify_scanline();
            }
        }

        if rendering && visible_line && dot == 0 {
            self.evaluate_sprites_for_scanline(scanline);
        }
        if rendering && pre_render && dot == 0 {
            self.evaluate_sprites_for_scanline(0);
        }

        if visible_line && (1..=256).contains(&dot) {
            if rendering {
                let pal_idx = self.ppu.compose_pixel() as usize;
                Self::write_pixel(&mut self.ppu, dot, scanline, pal_idx, self.palette_mode);
            } else {
                let pal_idx = (self.ppu.palette_ram[0] & 0x3F) as usize;
                Self::write_pixel(&mut self.ppu, dot, scanline, pal_idx, self.palette_mode);
            }
        }

        if rendering && render_line {
            let in_bg_range = (1..=256).contains(&dot) || (321..=336).contains(&dot);

            if in_bg_range {
                self.ppu.update_shifters();

                match (dot - 1) % 8 {
                    0 => {
                        self.ppu.load_bg_shifters();
                        let addr = 0x2000 | (self.ppu.v & 0x0FFF);
                        self.ppu.bg_next_tile_id = self.ppu_bus_read(addr);
                    }
                    2 => {
                        let v = self.ppu.v;
                        let addr = 0x23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07);
                        let attrib = self.ppu_bus_read(addr);
                        let shift = ((v >> 4) & 0x04) | (v & 0x02);
                        self.ppu.bg_next_tile_attrib = (attrib >> shift) & 0x03;
                    }
                    4 => {
                        let base = self.ppu.regs.bg_pattern_addr();
                        let fine_y = (self.ppu.v >> 12) & 0x07;
                        let addr = base + (self.ppu.bg_next_tile_id as u16) * 16 + fine_y;
                        self.ppu.bg_next_tile_lo = self.ppu_bus_read(addr);
                    }
                    6 => {
                        let base = self.ppu.regs.bg_pattern_addr();
                        let fine_y = (self.ppu.v >> 12) & 0x07;
                        let addr = base + (self.ppu.bg_next_tile_id as u16) * 16 + fine_y + 8;
                        self.ppu.bg_next_tile_hi = self.ppu_bus_read(addr);
                    }
                    7 => {
                        self.ppu.increment_scroll_x();
                    }
                    _ => {}
                }
            }

            if dot == 256 {
                self.ppu.increment_scroll_y();
            }

            if dot == 257 {
                self.ppu.copy_horizontal_bits();
            }

            if pre_render && (280..=304).contains(&dot) {
                self.ppu.copy_vertical_bits();
            }
        }
    }

    #[inline]
    fn write_pixel(
        ppu: &mut Ppu,
        dot: u16,
        scanline: u16,
        pal_idx: usize,
        palette_mode: NesPaletteMode,
    ) {
        let effective_idx = if ppu.regs.greyscale() {
            pal_idx & 0x30
        } else {
            pal_idx
        };
        let (r, g, b) = NES_PALETTE[effective_idx];
        let (mut r, mut g, mut b) = apply_nes_palette_mode(palette_mode, (r, g, b));

        let emph_r = ppu.regs.emphasize_red();
        let emph_g = ppu.regs.emphasize_green();
        let emph_b = ppu.regs.emphasize_blue();
        if emph_r || emph_g || emph_b {
            const ATTEN_NUM: u16 = 192;
            const ATTEN_DEN: u16 = 235;
            if !emph_r {
                r = (r as u16 * ATTEN_NUM / ATTEN_DEN) as u8;
            }
            if !emph_g {
                g = (g as u16 * ATTEN_NUM / ATTEN_DEN) as u8;
            }
            if !emph_b {
                b = (b as u16 * ATTEN_NUM / ATTEN_DEN) as u8;
            }
        }

        let x = (dot - 1) as usize;
        let y = scanline as usize;
        let offset = (y * 256 + x) * 4;
        ppu.framebuffer[offset] = r;
        ppu.framebuffer[offset + 1] = g;
        ppu.framebuffer[offset + 2] = b;
        ppu.framebuffer[offset + 3] = 0xFF;
    }

    #[inline]
    fn evaluate_sprites_for_scanline(&mut self, target: u16) {
        let sprite_height: u16 = if self.ppu.regs.tall_sprites() { 16 } else { 8 };
        let pattern_base = self.ppu.regs.sprite_pattern_addr();

        self.ppu.sprite_count = 0;
        self.ppu.sprite_zero_rendering = false;
        self.ppu.sprite_patterns_lo = [0; 8];
        self.ppu.sprite_patterns_hi = [0; 8];
        self.ppu.sprite_attribs = [0; 8];
        self.ppu.sprite_x_counters = [0xFF; 8];
        self.ppu.overflow_bug_m = 0;

        let mut count: u8 = 0;

        for i in 0..64usize {
            let base = i * 4;

            let oam_y = if count >= 8 {
                self.ppu.oam[(base + self.ppu.overflow_bug_m as usize) & 0xFF] as u16
            } else {
                self.ppu.oam[base] as u16
            };

            let effective_y = oam_y.wrapping_add(1);
            let diff = target.wrapping_sub(effective_y);
            if diff >= sprite_height {
                if count >= 8 {
                    self.ppu.overflow_bug_m = self.ppu.overflow_bug_m.wrapping_add(1) & 0x03;
                }
                continue;
            }

            if count >= 8 {
                self.ppu.regs.set_sprite_overflow();
                break;
            }

            if i == 0 {
                self.ppu.sprite_zero_rendering = true;
            }

            let tile_index = self.ppu.oam[base + 1];
            let attributes = self.ppu.oam[base + 2];
            let sprite_x = self.ppu.oam[base + 3];
            let flip_h = attributes & 0x40 != 0;
            let flip_v = attributes & 0x80 != 0;

            let mut row = diff;
            if flip_v {
                row = sprite_height - 1 - row;
            }

            let lo_addr = if sprite_height == 8 {
                pattern_base + (tile_index as u16) * 16 + row
            } else {
                let bank = (tile_index as u16 & 0x01) * 0x1000;
                let tile = tile_index as u16 & 0xFE;
                if row < 8 {
                    bank + tile * 16 + row
                } else {
                    bank + (tile + 1) * 16 + (row - 8)
                }
            };
            let hi_addr = lo_addr + 8;

            let mut lo = self.ppu_bus_read_with_kind(lo_addr, ChrFetchKind::Sprite);
            let mut hi = self.ppu_bus_read_with_kind(hi_addr, ChrFetchKind::Sprite);

            if flip_h {
                lo = lo.reverse_bits();
                hi = hi.reverse_bits();
            }

            let idx = count as usize;
            self.ppu.sprite_patterns_lo[idx] = lo;
            self.ppu.sprite_patterns_hi[idx] = hi;
            self.ppu.sprite_attribs[idx] = attributes;
            self.ppu.sprite_x_counters[idx] = sprite_x;

            count += 1;
        }

        self.ppu.sprite_count = count;
    }

    pub fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.ram);
        self.ppu.write_state(w);
        self.apu.write_state(w);
        self.cartridge.write_state(w);
        self.controller1.write_state(w);
        self.controller2.write_state(w);
        w.write_u64(self.ppu_cycles);
        w.write_u8(self.cpu_open_bus);
    }

    pub fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.ram)?;
        self.ppu.read_state(r)?;
        self.apu.read_state(r)?;
        self.cartridge.read_state(r)?;
        self.controller1.read_state(r)?;
        self.controller2.read_state(r)?;
        self.ppu_cycles = r.read_u64()?;
        self.cpu_open_bus = r.read_u8()?;
        Ok(())
    }

    fn dmc_dma_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & RAM_MIRROR_MASK) as usize],
            0x4020..=0xFFFF => self.cartridge.cpu_read(addr),
            _ => 0,
        }
    }

    pub fn tick_peripherals(&mut self, cpu_cycles: u64) -> bool {
        let ppu_dots = cpu_cycles * 3;
        let mut nmi_raised = false;
        for _ in 0..ppu_dots {
            self.ppu_render_dot();
            if self.ppu.tick() {
                nmi_raised = true;
            }
            self.ppu_cycles += 1;
        }
        for _ in 0..cpu_cycles {
            self.apu.tick();

            if self.apu.dmc.needs_dma() {
                let addr = self.apu.dmc.dma_address();
                let byte = self.dmc_dma_read(addr);
                self.apu.dmc.fill_sample_buffer(byte);
                let base = if self.cpu_odd_cycle { 4 } else { 3 };
                let conflict = if self.dma_stall_cycles > 0 { 1 } else { 0 };
                self.dma_stall_cycles += base + conflict;
            }

            self.cartridge.clock_cpu();
        }
        nmi_raised
    }
}

impl fmt::Debug for Bus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bus")
            .field("ppu", &self.ppu)
            .field("apu", &self.apu)
            .field("mirroring", &self.cartridge.mirroring())
            .finish_non_exhaustive()
    }
}
