mod cpu_io;
mod dma;
mod ppu_bus;
mod rendering;
mod timing;

use crate::cheats::NesCheatState;
use crate::hardware::apu::Apu;
use crate::hardware::cartridge::{Cartridge, NesMapper};
use crate::hardware::constants::*;
use crate::hardware::controller::{Controller, ExpansionDevice};
use crate::hardware::ppu::{
    NES_PALETTE, NES_RGB_2C03_PALETTE, NesBasePalette, NesPalette, NesPaletteMode, Ppu,
    apply_nes_emphasis, apply_nes_palette_mode, apply_rgb_ppu_emphasis,
};
use crate::hardware::timing::{CpuPpuClock, NesTiming};
use dma::DmaController;
use std::fmt;
pub use timing::PeripheralTickEvents;
pub use zeff_emu_common::debug::BusAccessEvent as DebugTraceEvent;
use zeff_emu_common::time::MasterTicks;

const VS_RGB_ZAPPER_SET_E_INES_CRC32: u32 = 0x9C41_0648;
const VS_RGB_ZAPPER_PRG_CRC32: u32 = 0xED58_8F00;
const VS_RGB_ZAPPER_DEFAULT_DIP_SWITCH: u8 = 0x28;
const ZAPPER_SENSOR_DECAY_SCANLINES: u16 = 24;
const ZAPPER_SENSOR_SAMPLE_RADIUS: i32 = 4;

fn power_on_internal_ram() -> [u8; RAM_SIZE] {
    [0xFF; RAM_SIZE]
}

pub struct Bus {
    pub ram: [u8; RAM_SIZE],
    pub(crate) ppu: Ppu,
    pub apu: Apu,
    pub cartridge: Cartridge,
    pub(crate) qualified_ppu_a12: bool,
    pub controller1: Controller,
    pub controller2: Controller,
    pub expansion_device: ExpansionDevice,

    pub(crate) ppu_cycles: u64,
    pub(crate) timing: NesTiming,
    pub(crate) ppu_clock: CpuPpuClock,
    pub(crate) sprite_fetch_a12: [bool; 8],

    pub(crate) dma_stall_cycles: u64,
    dma: DmaController,

    pub(crate) cpu_odd_cycle: bool,
    pub(crate) cpu_open_bus: u8,
    pub(crate) cpu_nmi_line_sampled: bool,
    pub(crate) cpu_step_elapsed_cycles: u64,
    pub(crate) cpu_step_events: PeripheralTickEvents,
    pub(crate) cpu_step_start_tick: Option<MasterTicks>,
    pub(crate) cpu_access_elapsed_cycles: u64,
    pub game_genie: NesCheatState,
    pub palette_mode: NesPaletteMode,
    pub custom_palette: Option<NesPalette>,
    base_palette: NesBasePalette,
    rgb_ppu_emphasis: bool,

    pub(crate) palette_luts: [[[u8; 4]; 64]; 8],

    pub(crate) debug_trace_enabled: bool,
    pub(crate) debug_trace_reads: bool,
    pub(crate) debug_trace_events: Vec<DebugTraceEvent>,
    pub(crate) vs_credit_pressed: bool,
    pub(crate) vs_coin_pulse_frames: u8,
    pub(crate) zapper_screen_pos: Option<(u16, u16)>,
    pub(crate) zapper_fallback_hit: bool,
}

impl Bus {
    pub fn new(cartridge: Cartridge, sample_rate: f64) -> Self {
        Self::new_with_timing(cartridge, sample_rate, NesTiming::Ntsc)
    }

    pub(crate) fn new_with_timing(
        cartridge: Cartridge,
        sample_rate: f64,
        timing: NesTiming,
    ) -> Self {
        let qualified_ppu_a12 = cartridge.uses_qualified_ppu_a12();
        let palette_mode = NesPaletteMode::default();
        let rgb_ppu_emphasis = Self::uses_rgb_2c03_palette(&cartridge);
        let base_palette = if rgb_ppu_emphasis {
            NES_RGB_2C03_PALETTE
        } else {
            NES_PALETTE
        };
        let mut ppu = Ppu::new_with_timing(timing);
        ppu.set_odd_frame_dot_skip_enabled(timing.odd_frame_dot_skip() && !rgb_ppu_emphasis);
        let mut apu = Apu::new_with_timing(sample_rate, timing);
        if Self::mapper_is_vs_system(cartridge.header().mapper_kind()) {
            apu.set_tonal_noise_supported(false);
        }

        Self {
            ram: power_on_internal_ram(),
            ppu,
            apu,
            cartridge,
            qualified_ppu_a12,
            controller1: Controller::new(),
            controller2: Controller::new(),
            expansion_device: ExpansionDevice::None,
            ppu_cycles: 0,
            timing,
            ppu_clock: CpuPpuClock::new(timing),
            sprite_fetch_a12: [false; 8],
            dma_stall_cycles: 0,
            dma: DmaController::default(),
            cpu_odd_cycle: false,
            cpu_open_bus: 0,
            cpu_nmi_line_sampled: false,
            cpu_step_elapsed_cycles: 0,
            cpu_step_events: PeripheralTickEvents::default(),
            cpu_step_start_tick: None,
            cpu_access_elapsed_cycles: 0,
            game_genie: NesCheatState::new(),
            palette_mode,
            custom_palette: None,
            base_palette,
            rgb_ppu_emphasis,
            palette_luts: Self::build_palette_luts(
                palette_mode,
                None,
                &base_palette,
                rgb_ppu_emphasis,
            ),
            debug_trace_enabled: false,
            debug_trace_reads: true,
            debug_trace_events: Vec::new(),
            vs_credit_pressed: false,
            vs_coin_pulse_frames: 0,
            zapper_screen_pos: None,
            zapper_fallback_hit: false,
        }
    }

    fn build_palette_luts(
        mode: NesPaletteMode,
        custom_palette: Option<&NesPalette>,
        base_palette: &NesBasePalette,
        rgb_ppu_emphasis: bool,
    ) -> [[[u8; 4]; 64]; 8] {
        let mut luts = [[[0u8; 4]; 64]; 8];

        if mode == NesPaletteMode::Custom
            && let Some(NesPalette::WithEmphasis(palettes)) = custom_palette
        {
            for (group, palette) in palettes.iter().enumerate() {
                Self::fill_palette_lut(&mut luts[group], palette, None);
            }
            return luts;
        }

        let source_palette = match (mode, custom_palette) {
            (NesPaletteMode::Custom, Some(palette)) => palette.base(),
            _ => *base_palette,
        };

        for (group, lut) in luts.iter_mut().enumerate() {
            let mask = (group as u8) << 5;
            Self::fill_palette_lut(lut, &source_palette, Some((mode, mask, rgb_ppu_emphasis)));
        }

        luts
    }

    fn fill_palette_lut(
        lut: &mut [[u8; 4]; 64],
        palette: &NesBasePalette,
        correction: Option<(NesPaletteMode, u8, bool)>,
    ) {
        for (i, entry) in lut.iter_mut().enumerate() {
            let (r, g, b) = match correction {
                Some((mode, mask, rgb_ppu_emphasis)) => {
                    let rgb = apply_nes_palette_mode(mode, palette[i]);
                    if rgb_ppu_emphasis {
                        apply_rgb_ppu_emphasis(mask, rgb)
                    } else {
                        apply_nes_emphasis(mode, mask, rgb)
                    }
                }
                None => palette[i],
            };
            *entry = [r, g, b, 0xFF];
        }
    }

    pub fn set_palette_mode(&mut self, mode: NesPaletteMode) {
        self.palette_mode = mode;
        self.palette_luts = Self::build_palette_luts(
            mode,
            self.custom_palette.as_ref(),
            &self.base_palette,
            self.rgb_ppu_emphasis,
        );
    }

    pub fn set_custom_palette(&mut self, palette: Option<NesPalette>) {
        self.custom_palette = palette;
        self.palette_luts = Self::build_palette_luts(
            self.palette_mode,
            self.custom_palette.as_ref(),
            &self.base_palette,
            self.rgb_ppu_emphasis,
        );
    }

    pub fn reset(&mut self) {
        self.apu.reset();
        self.dma_stall_cycles = 0;
        self.dma = DmaController::default();
        self.cpu_nmi_line_sampled = self.ppu.nmi_output;
        self.cpu_step_elapsed_cycles = 0;
        self.cpu_step_events = PeripheralTickEvents::default();
        self.cpu_step_start_tick = None;
        self.cpu_access_elapsed_cycles = 0;
        self.vs_credit_pressed = false;
        self.vs_coin_pulse_frames = 0;
        self.zapper_screen_pos = None;
        self.zapper_fallback_hit = false;
    }

    pub(crate) fn set_zapper_light_sensor(
        &mut self,
        screen_pos: Option<(u16, u16)>,
        fallback_hit: bool,
    ) {
        self.zapper_screen_pos = screen_pos
            .filter(|&(x, y)| usize::from(x) < SCREEN_WIDTH && usize::from(y) < SCREEN_HEIGHT);
        self.zapper_fallback_hit = fallback_hit;
    }

    pub(crate) fn current_zapper_light_detected(&self) -> bool {
        let Some((x, y)) = self.zapper_screen_pos else {
            return self.zapper_fallback_hit;
        };

        if self.ppu.scanline >= self.ppu.vblank_start_scanline() || self.ppu.scanline < y {
            return false;
        }
        if self.ppu.scanline == y && self.ppu.dot <= x.saturating_add(1) {
            return false;
        }
        if self.ppu.scanline.saturating_sub(y) > ZAPPER_SENSOR_DECAY_SCANLINES {
            return false;
        }

        self.zapper_framebuffer_region_is_bright(x, y)
    }

    fn zapper_framebuffer_region_is_bright(&self, x: u16, y: u16) -> bool {
        let center_x = i32::from(x);
        let center_y = i32::from(y);

        for sample_y in (center_y - ZAPPER_SENSOR_SAMPLE_RADIUS).max(0)
            ..=(center_y + ZAPPER_SENSOR_SAMPLE_RADIUS).min((SCREEN_HEIGHT - 1) as i32)
        {
            for sample_x in (center_x - ZAPPER_SENSOR_SAMPLE_RADIUS).max(0)
                ..=(center_x + ZAPPER_SENSOR_SAMPLE_RADIUS).min((SCREEN_WIDTH - 1) as i32)
            {
                let idx = ((sample_y as usize * SCREEN_WIDTH + sample_x as usize) * 4)
                    .min(self.ppu.framebuffer.len().saturating_sub(4));
                let r = self.ppu.framebuffer[idx];
                let g = self.ppu.framebuffer[idx + 1];
                let b = self.ppu.framebuffer[idx + 2];
                if Self::zapper_pixel_is_bright(r, g, b) {
                    return true;
                }
            }
        }

        false
    }

    fn zapper_pixel_is_bright(r: u8, g: u8, b: u8) -> bool {
        let min_component = r.min(g).min(b);
        let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
        min_component >= 160 && luma >= 190.0
    }

    pub(crate) fn is_vs_system_mapper(&self) -> bool {
        Self::mapper_is_vs_system(self.cartridge.header().mapper_kind())
    }

    fn mapper_is_vs_system(mapper: NesMapper) -> bool {
        matches!(
            mapper,
            NesMapper::VsSystem | NesMapper::Vrc1VsSystem | NesMapper::LegacyVsVrc1
        )
    }

    fn matches_vs_rgb_zapper_profile(cartridge: &Cartridge) -> bool {
        Self::mapper_is_vs_system(cartridge.header().mapper_kind())
            && (cartridge.rom_crc32() == VS_RGB_ZAPPER_SET_E_INES_CRC32
                || cartridge.prg_crc32() == VS_RGB_ZAPPER_PRG_CRC32)
    }

    fn uses_rgb_2c03_palette(cartridge: &Cartridge) -> bool {
        Self::matches_vs_rgb_zapper_profile(cartridge)
    }

    pub(crate) fn set_vs_system_credit_input(&mut self, pressed: bool) {
        if !self.is_vs_system_mapper() {
            self.vs_credit_pressed = false;
            self.vs_coin_pulse_frames = 0;
            return;
        }

        if pressed && !self.vs_credit_pressed {
            self.vs_coin_pulse_frames = 4;
        }
        self.vs_credit_pressed = pressed;
    }

    pub(crate) fn finish_vs_system_input_frame(&mut self) {
        self.vs_coin_pulse_frames = self.vs_coin_pulse_frames.saturating_sub(1);
    }

    pub(crate) fn vs_system_4016_bits(&self) -> u8 {
        if !self.is_vs_system_mapper() {
            return 0;
        }

        let coin_inserted = if self.vs_coin_pulse_frames > 0 {
            0x20
        } else {
            0
        };
        let dip_low_bits = (self.vs_system_dip_switch() & 0x03) << 3;
        coin_inserted | dip_low_bits
    }

    pub(crate) fn vs_system_4017_bits(&self) -> u8 {
        self.vs_system_dip_switch() & !0x03
    }

    fn vs_system_dip_switch(&self) -> u8 {
        if !self.is_vs_system_mapper() {
            return 0;
        }

        if Self::matches_vs_rgb_zapper_profile(&self.cartridge) {
            VS_RGB_ZAPPER_DEFAULT_DIP_SWITCH
        } else {
            0
        }
    }

    pub fn palette_mode(&self) -> NesPaletteMode {
        self.palette_mode
    }

    pub fn palette_color_rgba(&self, pal_idx: u8) -> [u8; 4] {
        self.palette_luts[0][(pal_idx & 0x3F) as usize]
    }

    pub fn palette_lut(&self) -> [[u8; 4]; 64] {
        self.palette_luts[0]
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
        for decay_at in self.ppu.io_latch_decay_at_ppu_cycle {
            w.write_u64(decay_at);
        }
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
        if r.is_exhausted() {
            self.ppu
                .refresh_io_latch_bits(self.ppu.io_latch, 0xFF, self.ppu_cycles);
        } else {
            for decay_at in &mut self.ppu.io_latch_decay_at_ppu_cycle {
                *decay_at = r.read_u64()?;
            }
        }
        self.cpu_nmi_line_sampled = self.ppu.nmi_output;
        self.dma_stall_cycles = 0;
        self.cpu_step_elapsed_cycles = 0;
        self.cpu_step_events = PeripheralTickEvents::default();
        self.cpu_step_start_tick = None;
        self.cpu_access_elapsed_cycles = 0;
        self.sprite_fetch_a12 = [false; 8];
        self.dma = DmaController::default();
        Ok(())
    }

    pub(crate) fn write_dma_state(&self, w: &mut crate::save_state::StateWriter) {
        self.dma.write_state(w);
    }

    pub(crate) fn read_dma_state(
        &mut self,
        r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        self.dma.read_state(r)
    }

    pub(crate) fn write_apu_runtime_state(&self, w: &mut crate::save_state::StateWriter) {
        self.apu.write_frame_counter_runtime_state(w);
    }

    pub(crate) fn read_apu_runtime_state(
        &mut self,
        r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        self.apu.read_frame_counter_runtime_state(r)
    }

    pub(crate) fn write_ppu_runtime_state(&self, w: &mut crate::save_state::StateWriter) {
        let mut sprite_a12 = 0u8;
        for (index, high) in self.sprite_fetch_a12.iter().copied().enumerate() {
            sprite_a12 |= u8::from(high) << index;
        }
        w.write_u8(sprite_a12);
        self.cartridge.write_ppu_runtime_state(w);
        w.write_u8(self.ppu.sprite_eval_oam_addr);
        w.write_u8(self.ppu.sprite_eval_secondary_addr);
        w.write_u8(self.ppu.sprite_eval_latch);
        w.write_bool(self.ppu.sprite_eval_in_range);
        w.write_bool(self.ppu.sprite_eval_done);
        w.write_bool(self.ppu.sprite_eval_sprite_zero);
        w.write_u8(self.ppu.sprite_eval_overflow_remaining);
    }

    pub(crate) fn read_ppu_runtime_state(
        &mut self,
        r: &mut crate::save_state::StateReader,
        has_sprite_evaluation_state: bool,
    ) -> anyhow::Result<()> {
        let sprite_a12 = r.read_u8()?;
        for (index, high) in self.sprite_fetch_a12.iter_mut().enumerate() {
            *high = sprite_a12 & (1 << index) != 0;
        }
        self.cartridge.read_ppu_runtime_state(r)?;

        if has_sprite_evaluation_state {
            self.ppu.sprite_eval_oam_addr = r.read_u8()?;
            self.ppu.sprite_eval_secondary_addr = r.read_u8()?;
            if self.ppu.sprite_eval_secondary_addr > 32 {
                anyhow::bail!(
                    "invalid secondary OAM evaluation address {}",
                    self.ppu.sprite_eval_secondary_addr
                );
            }
            self.ppu.sprite_eval_latch = r.read_u8()?;
            self.ppu.sprite_eval_in_range = r.read_bool()?;
            self.ppu.sprite_eval_done = r.read_bool()?;
            self.ppu.sprite_eval_sprite_zero = r.read_bool()?;
            self.ppu.sprite_eval_overflow_remaining = r.read_u8()?;
            if self.ppu.sprite_eval_overflow_remaining > 3 {
                anyhow::bail!(
                    "invalid sprite overflow evaluation counter {}",
                    self.ppu.sprite_eval_overflow_remaining
                );
            }
        } else {
            self.ppu.sprite_eval_oam_addr = 0;
            self.ppu.sprite_eval_secondary_addr = 0;
            self.ppu.sprite_eval_latch = 0xFF;
            self.ppu.sprite_eval_in_range = false;
            self.ppu.sprite_eval_done = true;
            self.ppu.sprite_eval_sprite_zero = false;
            self.ppu.sprite_eval_overflow_remaining = 0;
        }

        Ok(())
    }

    pub(crate) fn write_mutable_media_state(&self, w: &mut crate::save_state::StateWriter) {
        self.cartridge.write_mutable_media_state(w);
    }

    pub(crate) fn read_mutable_media_state(
        &mut self,
        r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        self.cartridge.read_mutable_media_state(r)
    }

    pub(crate) fn reset_mutable_media_to_source(&mut self) {
        self.cartridge.reset_mutable_media_to_source();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_on_internal_ram_is_deterministic_nonzero() {
        let ram = power_on_internal_ram();
        assert!(ram.iter().all(|&byte| byte == 0xFF));
    }
}
