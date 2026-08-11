use super::{ActiveCore, CoreState};
use zeff_gb_core::hardware::ppu::DmgPalettePreset;
use zeff_nes_core::hardware::ppu::NesPaletteMode;
use zeff_sega8_core::hardware::cartridge::Sega8System;

impl CoreState {
    pub fn native_width(&self) -> u32 {
        match &self.core {
            ActiveCore::Gb(emu) => {
                let (w, _) = emu.framebuffer_dimensions();
                w as u32
            }
            ActiveCore::Gba(_) => 240,
            ActiveCore::Nes(_) => 256,
            ActiveCore::Sega8(emu) => {
                let (w, _) = emu.framebuffer_dimensions();
                w as u32
            }
            ActiveCore::Ws(emu) => {
                let (w, _) = emu.framebuffer_dimensions();
                w as u32
            }
        }
    }

    pub fn native_height(&self) -> u32 {
        match &self.core {
            ActiveCore::Gb(emu) => {
                let (_, h) = emu.framebuffer_dimensions();
                h as u32
            }
            ActiveCore::Gba(_) => 160,
            ActiveCore::Nes(_) => 240,
            ActiveCore::Sega8(emu) => {
                let (_, h) = emu.framebuffer_dimensions();
                h as u32
            }
            ActiveCore::Ws(emu) => {
                let (_, h) = emu.framebuffer_dimensions();
                h as u32
            }
        }
    }

    pub fn framebuffer_as_xrgb8888(&mut self) -> &[u8] {
        let fb = match &self.core {
            ActiveCore::Gb(emu) => emu.framebuffer(),
            ActiveCore::Gba(emu) => emu.framebuffer(),
            ActiveCore::Nes(emu) => emu.framebuffer(),
            ActiveCore::Sega8(emu) => emu.framebuffer(),
            ActiveCore::Ws(emu) => emu.framebuffer(),
        };
        self.xrgb_buf.resize(fb.len(), 0);
        for (i, chunk) in fb.chunks_exact(4).enumerate() {
            let r = chunk[0];
            let g = chunk[1];
            let b = chunk[2];
            let offset = i * 4;
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
            ActiveCore::Sega8(emu) => emu.framebuffer(),
            ActiveCore::Ws(emu) => emu.framebuffer(),
        };
        let pixel_count = fb.len() / 4;
        self.rgb565_buf.resize(pixel_count * 2, 0);
        for (i, chunk) in fb.chunks_exact(4).enumerate() {
            let r = chunk[0] as u16;
            let g = chunk[1] as u16;
            let b = chunk[2] as u16;
            let rgb565: u16 = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
            let offset = i * 2;
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
            ActiveCore::Sega8(emu) => match emu.system() {
                Sega8System::MasterSystem => "SMS",
                Sega8System::GameGear => "Game Gear",
                Sega8System::Sg1000 => "SG-1000/SC-3000",
            },
            ActiveCore::Ws(_) => "WonderSwan",
        }
    }
}
