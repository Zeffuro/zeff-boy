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
            ActiveCore::Pce(_) => (
                zeff_pce_core::hardware::PCE_HOST_FRAME_WIDTH as u32,
                zeff_pce_core::hardware::PCE_HOST_FRAME_HEIGHT as u32,
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
                zeff_pce_core::hardware::PCE_HOST_FRAME_WIDTH as u32,
                zeff_pce_core::hardware::PCE_HOST_FRAME_HEIGHT as u32,
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

    pub fn framebuffer_as_xrgb8888(&mut self) -> &[u8] {
        let fb = match &self.core {
            ActiveCore::Gb(emu) => emu.framebuffer(),
            ActiveCore::Gba(emu) => emu.framebuffer(),
            ActiveCore::Nes(emu) => emu.framebuffer(),
            ActiveCore::Pce(host) => host.framebuffer(),
            ActiveCore::Sega8(emu) => emu.framebuffer(),
            ActiveCore::Ws(emu) => emu.framebuffer(),
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
        let fb = match &self.core {
            ActiveCore::Gb(emu) => emu.framebuffer(),
            ActiveCore::Gba(emu) => emu.framebuffer(),
            ActiveCore::Nes(emu) => emu.framebuffer(),
            ActiveCore::Pce(host) => host.framebuffer(),
            ActiveCore::Sega8(emu) => emu.framebuffer(),
            ActiveCore::Ws(emu) => emu.framebuffer(),
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
