use super::{ActiveCore, CoreState, LIBRETRO_RGB565_BYTES_PER_PIXEL};
use crate::api::retro_game_geometry;
use zeff_emu_common::system::{
    GAME_BOY_SCREEN_SIZE, GBA_SCREEN_SIZE, NES_SCREEN_SIZE, RGBA_BYTES_PER_PIXEL,
};
use zeff_gb_core::hardware::ppu::DmgPalettePreset;
use zeff_nes_core::hardware::ppu::NesPaletteMode;
use zeff_sega8_core::hardware::cartridge::Sega8System;

impl CoreState {
    pub(crate) fn video_geometry(&self) -> retro_game_geometry {
        let (base_width, base_height) = match &self.core {
            ActiveCore::Gb(emu) => {
                let (width, height) = emu.framebuffer_dimensions();
                (width as u32, height as u32)
            }
            ActiveCore::Gba(_) => GBA_SCREEN_SIZE,
            ActiveCore::Nes(_) => NES_SCREEN_SIZE,
            ActiveCore::Pce(_) => self.pce_native_output_dimensions.map_or(
                (
                    zeff_pce_core::hardware::PCE_HOST_FRAME_WIDTH as u32,
                    zeff_pce_core::hardware::PCE_HOST_FRAME_HEIGHT as u32,
                ),
                |(width, height)| (width as u32, height as u32),
            ),
            ActiveCore::Sega8(emu) => {
                let (width, height) = emu.framebuffer_dimensions();
                (width as u32, height as u32)
            }
            ActiveCore::Ws(emu) => {
                let (width, height) = emu.framebuffer_dimensions();
                (width as u32, height as u32)
            }
        };

        let (max_width, max_height) = if matches!(self.core, ActiveCore::Pce(_)) {
            (
                base_width.max(zeff_pce_core::hardware::PCE_HOST_FRAME_WIDTH as u32),
                base_height.max(zeff_pce_core::hardware::PCE_HOST_FRAME_HEIGHT as u32),
            )
        } else {
            NES_SCREEN_SIZE
        };
        let aspect_ratio = if matches!(self.core, ActiveCore::Pce(_)) {
            4.0 / 3.0
        } else {
            0.0
        };
        Self::video_geometry_for_size(base_width, base_height, max_width, max_height, aspect_ratio)
    }

    pub(crate) fn default_video_geometry() -> retro_game_geometry {
        Self::video_geometry_for_size(
            GAME_BOY_SCREEN_SIZE.0,
            GAME_BOY_SCREEN_SIZE.1,
            NES_SCREEN_SIZE.0,
            NES_SCREEN_SIZE.1,
            0.0,
        )
    }

    fn video_geometry_for_size(
        base_width: u32,
        base_height: u32,
        max_width: u32,
        max_height: u32,
        aspect_ratio: f32,
    ) -> retro_game_geometry {
        retro_game_geometry {
            base_width,
            base_height,
            max_width,
            max_height,
            aspect_ratio,
        }
    }

    pub(crate) fn pce_output_geometry(dimensions: Option<(usize, usize)>) -> retro_game_geometry {
        let (width, height) = dimensions.map_or(
            (
                zeff_pce_core::hardware::PCE_HOST_FRAME_WIDTH as u32,
                zeff_pce_core::hardware::PCE_HOST_FRAME_HEIGHT as u32,
            ),
            |(width, height)| (width as u32, height as u32),
        );
        Self::video_geometry_for_size(
            width,
            height,
            width.max(zeff_pce_core::hardware::PCE_HOST_FRAME_WIDTH as u32),
            height.max(zeff_pce_core::hardware::PCE_HOST_FRAME_HEIGHT as u32),
            4.0 / 3.0,
        )
    }

    pub fn framebuffer_as_xrgb8888(&mut self) -> &[u8] {
        if let ActiveCore::Pce(host) = &self.core {
            if let Some((width, height)) = self.pce_native_output_dimensions
                && let Some(descriptor) = host.native_frame_descriptor()
                && (descriptor.width(), descriptor.height()) == (width, height)
            {
                self.xrgb_buf.resize(width * height * 4, 0);
                host.project_native_frame_xrgb8888(descriptor, &mut self.xrgb_buf);
                return &self.xrgb_buf;
            }
            self.xrgb_buf
                .resize(zeff_pce_core::hardware::PCE_HOST_FRAME_XRGB8888_BYTES, 0);
            host.project_frame_xrgb8888(&mut self.xrgb_buf);
            return &self.xrgb_buf;
        }
        let fb = match &self.core {
            ActiveCore::Gb(emu) => emu.framebuffer(),
            ActiveCore::Gba(emu) => emu.framebuffer(),
            ActiveCore::Nes(emu) => emu.framebuffer(),
            ActiveCore::Sega8(emu) => emu.framebuffer(),
            ActiveCore::Ws(emu) => emu.framebuffer(),
            ActiveCore::Pce(_) => unreachable!(),
        };
        self.xrgb_buf.resize(fb.len(), 0);
        for (i, chunk) in fb.as_chunks::<RGBA_BYTES_PER_PIXEL>().0.iter().enumerate() {
            let r = chunk[0];
            let g = chunk[1];
            let b = chunk[2];
            let offset = i * RGBA_BYTES_PER_PIXEL;
            self.xrgb_buf[offset] = b;
            self.xrgb_buf[offset + 1] = g;
            self.xrgb_buf[offset + 2] = r;
            self.xrgb_buf[offset + 3] = 0x00;
        }
        &self.xrgb_buf
    }

    pub fn framebuffer_as_rgb565(&mut self) -> &[u8] {
        if let ActiveCore::Pce(host) = &self.core {
            if let Some((width, height)) = self.pce_native_output_dimensions
                && let Some(descriptor) = host.native_frame_descriptor()
                && (descriptor.width(), descriptor.height()) == (width, height)
            {
                self.rgb565_buf
                    .resize(width * height * LIBRETRO_RGB565_BYTES_PER_PIXEL, 0);
                host.project_native_frame_rgb565(descriptor, &mut self.rgb565_buf);
                return &self.rgb565_buf;
            }
            self.rgb565_buf
                .resize(zeff_pce_core::hardware::PCE_HOST_FRAME_RGB565_BYTES, 0);
            host.project_frame_rgb565(&mut self.rgb565_buf);
            return &self.rgb565_buf;
        }
        let fb = match &self.core {
            ActiveCore::Gb(emu) => emu.framebuffer(),
            ActiveCore::Gba(emu) => emu.framebuffer(),
            ActiveCore::Nes(emu) => emu.framebuffer(),
            ActiveCore::Sega8(emu) => emu.framebuffer(),
            ActiveCore::Ws(emu) => emu.framebuffer(),
            ActiveCore::Pce(_) => unreachable!(),
        };
        let pixel_count = fb.len() / RGBA_BYTES_PER_PIXEL;
        self.rgb565_buf
            .resize(pixel_count * LIBRETRO_RGB565_BYTES_PER_PIXEL, 0);
        for (i, chunk) in fb.as_chunks::<RGBA_BYTES_PER_PIXEL>().0.iter().enumerate() {
            let r = chunk[0] as u16;
            let g = chunk[1] as u16;
            let b = chunk[2] as u16;
            let rgb565: u16 = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
            let offset = i * LIBRETRO_RGB565_BYTES_PER_PIXEL;
            self.rgb565_buf[offset] = (rgb565 & 0xFF) as u8;
            self.rgb565_buf[offset + 1] = (rgb565 >> 8) as u8;
        }
        &self.rgb565_buf
    }

    pub(crate) fn pending_pce_native_output_dimensions(&self) -> Option<Option<(usize, usize)>> {
        if !matches!(self.core, ActiveCore::Pce(_)) || self.pce_native_output_rejected {
            return None;
        }
        let ActiveCore::Pce(host) = &self.core else {
            unreachable!();
        };
        let desired = host
            .native_frame_descriptor()
            .map(|descriptor| (descriptor.width(), descriptor.height()));
        (desired != self.pce_native_output_dimensions).then_some(desired)
    }

    pub(crate) fn apply_pce_native_output_dimensions(
        &mut self,
        dimensions: Option<(usize, usize)>,
        accepted: bool,
    ) {
        if accepted {
            self.pce_native_output_dimensions = dimensions;
        } else {
            self.pce_native_output_dimensions = None;
            self.pce_native_output_rejected = true;
        }
    }

    pub fn set_dmg_palette(&mut self, preset: DmgPalettePreset) {
        if let ActiveCore::Gb(emu) = &mut self.core {
            emu.set_dmg_palette_preset(preset);
        }
    }

    pub fn set_nes_palette_mode(&mut self, mode: NesPaletteMode) {
        if let ActiveCore::Nes(emu) = &mut self.core {
            emu.set_palette_mode(mode);
        }
    }

    pub fn set_sgb_border_enabled(&mut self, enabled: bool) {
        if let ActiveCore::Gb(emu) = &mut self.core {
            emu.set_sgb_border_enabled(enabled);
        }
    }

    pub fn sgb_border_active(&self) -> bool {
        if let ActiveCore::Gb(emu) = &self.core {
            emu.sgb_border_active()
        } else {
            false
        }
    }

    #[allow(dead_code)]
    pub fn is_gb(&self) -> bool {
        matches!(self.core, ActiveCore::Gb(_))
    }

    pub fn system_label(&self) -> &'static str {
        match &self.core {
            ActiveCore::Gb(_) => "GB/GBC",
            ActiveCore::Gba(_) => "GBA",
            ActiveCore::Nes(_) => "NES",
            ActiveCore::Pce(_) => "PC Engine",
            ActiveCore::Sega8(emu) => match emu.system() {
                Sega8System::MasterSystem => "SMS",
                Sega8System::GameGear => "Game Gear",
                Sega8System::Sg1000 => "SG-1000/SC-3000",
            },
            ActiveCore::Ws(_) => "WonderSwan",
        }
    }
}
