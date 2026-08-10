use zeff_gb_core::hardware::ppu::DmgPalettePreset;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_nes_core::hardware::ppu::NesPaletteMode;
use zeff_sega8_core::hardware::cartridge::{Sega8System, SystemHint};

pub(crate) enum ActiveCore {
    Gb(Box<zeff_gb_core::emulator::Emulator>),
    Gba(Box<zeff_gba_core::emulator::Emulator>),
    Nes(Box<zeff_nes_core::emulator::Emulator>),
    Sega8(Box<zeff_sega8_core::emulator::Emulator>),
    Ws(Box<zeff_ws_core::emulator::Emulator>),
}

pub(crate) struct CoreState {
    pub core: ActiveCore,
    pub rom_data: Vec<u8>,
    pub audio_buf: Vec<f32>,
    pub sample_rate: u32,
    pub xrgb_buf: Vec<u8>,
    pub rgb565_buf: Vec<u8>,
    pub system_ram_buf: Vec<u8>,
    pub video_ram_buf: Vec<u8>,
    pub port_device: [u32; 2],
}

impl CoreState {
    pub fn from_rom(data: &[u8], path: &str) -> anyhow::Result<Self> {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let sample_rate = 48000u32;

        let core = match ext.as_str() {
            "gba" => {
                let emu = zeff_gba_core::emulator::Emulator::new(data, sample_rate)?;
                ActiveCore::Gba(Box::new(emu))
            }
            "nes" => {
                let emu = zeff_nes_core::emulator::Emulator::new(data, sample_rate as f64)?;
                ActiveCore::Nes(Box::new(emu))
            }
            "ws" | "wsc" => {
                let emu = zeff_ws_core::emulator::Emulator::new(data, sample_rate)?;
                ActiveCore::Ws(Box::new(emu))
            }
            ext if SystemHint::from_extension(ext).is_some() => {
                let hint = SystemHint::from_extension(ext).unwrap_or(SystemHint::Auto);
                let emu =
                    zeff_sega8_core::emulator::Emulator::new_with_hint(data, sample_rate, hint)?;
                ActiveCore::Sega8(Box::new(emu))
            }
            _ => {
                let pref = HardwareModePreference::Auto;
                let mut emu = zeff_gb_core::emulator::Emulator::from_rom_data(data, pref)?;
                emu.set_sample_rate(sample_rate);
                emu.set_sgb_border_enabled(false);
                ActiveCore::Gb(Box::new(emu))
            }
        };

        Ok(Self {
            core,
            rom_data: data.to_vec(),
            audio_buf: Vec::with_capacity(4096),
            sample_rate,
            xrgb_buf: Vec::new(),
            rgb565_buf: Vec::new(),
            system_ram_buf: Vec::new(),
            video_ram_buf: Vec::new(),
            port_device: [crate::api::RETRO_DEVICE_JOYPAD; 2],
        })
    }

    pub fn reset(&mut self) {
        match &mut self.core {
            ActiveCore::Gb(_) => {
                let pref = HardwareModePreference::Auto;
                if let Ok(mut emu) =
                    zeff_gb_core::emulator::Emulator::from_rom_data(&self.rom_data, pref)
                {
                    emu.set_sample_rate(self.sample_rate);
                    emu.set_sgb_border_enabled(false);
                    self.core = ActiveCore::Gb(Box::new(emu));
                }
            }
            ActiveCore::Gba(_) => {
                if let Ok(emu) =
                    zeff_gba_core::emulator::Emulator::new(&self.rom_data, self.sample_rate)
                {
                    self.core = ActiveCore::Gba(Box::new(emu));
                }
            }
            ActiveCore::Nes(emu) => emu.reset(),
            ActiveCore::Sega8(emu) => emu.reset(),
            ActiveCore::Ws(emu) => emu.reset(),
        }
    }

    pub fn step_frame(&mut self) {
        match &mut self.core {
            ActiveCore::Gb(emu) => emu.step_frame(),
            ActiveCore::Gba(emu) => emu.step_frame(),
            ActiveCore::Nes(emu) => emu.step_frame(),
            ActiveCore::Sega8(emu) => emu.step_frame(),
            ActiveCore::Ws(emu) => emu.step_frame(),
        }
    }

    pub fn drain_audio(&mut self) {
        self.audio_buf.clear();
        match &mut self.core {
            ActiveCore::Gb(emu) => {
                emu.drain_audio_samples_into(&mut self.audio_buf);
            }
            ActiveCore::Gba(emu) => {
                emu.drain_audio_samples_into(&mut self.audio_buf);
            }
            ActiveCore::Nes(emu) => {
                emu.drain_audio_into_stereo(&mut self.audio_buf);
            }
            ActiveCore::Sega8(emu) => {
                emu.drain_audio_samples_into(&mut self.audio_buf);
            }
            ActiveCore::Ws(emu) => {
                emu.drain_audio_samples_into(&mut self.audio_buf);
            }
        }
    }

    pub fn set_input(&mut self, buttons: u8, dpad: u8) {
        match &mut self.core {
            ActiveCore::Gb(emu) => emu.set_input(buttons, dpad),
            ActiveCore::Gba(emu) => emu.set_input(buttons, dpad),
            ActiveCore::Nes(emu) => {
                emu.set_input_p1(map_host_to_nes_byte(buttons, dpad));
            }
            ActiveCore::Sega8(emu) => emu.set_input(buttons, dpad),
            ActiveCore::Ws(emu) => emu.set_input(buttons, dpad),
        }
    }

    pub fn set_input_p2(&mut self, buttons: u8, dpad: u8) {
        if let ActiveCore::Nes(emu) = &mut self.core {
            emu.set_input_p2(map_host_to_nes_byte(buttons, dpad));
        }
    }

    pub fn set_zapper_state(&mut self, trigger: bool, hit: bool) {
        if let ActiveCore::Nes(emu) = &mut self.core {
            emu.set_zapper_state(true, trigger, hit, None);
        }
    }

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

    pub fn fps(&self) -> f64 {
        match &self.core {
            ActiveCore::Gb(_) => 59.7275,
            ActiveCore::Gba(_) => zeff_gba_core::hardware::constants::FPS,
            ActiveCore::Nes(_) => 60.0988,
            ActiveCore::Sega8(_) => {
                zeff_sega8_core::hardware::constants::SEGA8_NTSC_FRAME_RATE_APPROX as f64
            }
            ActiveCore::Ws(_) => zeff_ws_core::hardware::constants::FPS,
        }
    }

    pub fn encode_state(&self) -> anyhow::Result<Vec<u8>> {
        match &self.core {
            ActiveCore::Gb(emu) => emu.encode_state_bytes(),
            ActiveCore::Gba(emu) => emu.encode_state(),
            ActiveCore::Nes(emu) => emu.encode_state(),
            ActiveCore::Sega8(emu) => emu.encode_state(),
            ActiveCore::Ws(emu) => emu.encode_state(),
        }
    }

    pub fn load_state(&mut self, data: &[u8]) -> anyhow::Result<()> {
        match &mut self.core {
            ActiveCore::Gb(emu) => emu.load_state_from_bytes(data.to_vec()),
            ActiveCore::Gba(emu) => emu.load_state(data),
            ActiveCore::Nes(emu) => emu.load_state(data),
            ActiveCore::Sega8(emu) => emu.load_state(data),
            ActiveCore::Ws(emu) => emu.load_state(data),
        }
    }

    #[allow(dead_code)]
    pub fn serialize_size(&self) -> usize {
        self.encode_state().map_or(0, |v| v.len())
    }

    pub fn battery_sram(&self) -> Option<Vec<u8>> {
        match &self.core {
            ActiveCore::Gb(emu) => emu.dump_battery_sram(),
            ActiveCore::Gba(emu) => emu.dump_battery_sram(),
            ActiveCore::Nes(emu) => emu.dump_battery_sram(),
            ActiveCore::Sega8(emu) => emu.dump_battery_sram(),
            ActiveCore::Ws(emu) => emu.dump_battery_sram(),
        }
    }

    #[allow(dead_code)]
    pub fn load_battery_sram(&mut self, data: &[u8]) {
        match &mut self.core {
            ActiveCore::Gb(emu) => {
                let _ = emu.load_battery_sram(data);
            }
            ActiveCore::Gba(emu) => {
                let _ = emu.load_battery_sram(data);
            }
            ActiveCore::Nes(emu) => {
                let _ = emu.load_battery_sram(data);
            }
            ActiveCore::Sega8(emu) => {
                let _ = emu.load_battery_sram(data);
            }
            ActiveCore::Ws(emu) => {
                let _ = emu.load_battery_sram(data);
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

    pub fn sync_sram_to_buf(&self, buf: &mut Vec<u8>) {
        if let Some(sram) = self.battery_sram() {
            buf.resize(sram.len(), 0);
            buf.copy_from_slice(&sram);
        }
    }

    #[allow(dead_code)]
    pub fn load_sram_from_buf(&mut self, buf: &[u8]) {
        if !buf.is_empty() {
            self.load_battery_sram(buf);
        }
    }

    pub fn sram_size(&self) -> usize {
        self.battery_sram().map_or(0, |s| s.len())
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

    pub fn is_nes(&self) -> bool {
        matches!(self.core, ActiveCore::Nes(_))
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

    pub fn cheat_reset(&mut self) {
        match &mut self.core {
            ActiveCore::Gb(emu) => emu.clear_rom_patches(),
            ActiveCore::Gba(_) => {}
            ActiveCore::Nes(emu) => emu.clear_game_genie(),
            ActiveCore::Sega8(_) => {}
            ActiveCore::Ws(_) => {}
        }
    }

    pub fn cheat_set(&mut self, code: &str) {
        match &mut self.core {
            ActiveCore::Gb(emu) => {
                if let Ok((patches, _)) = zeff_gb_core::cheats::parse_cheat(code) {
                    for p in patches {
                        emu.add_rom_patch(p);
                    }
                }
            }
            ActiveCore::Gba(_) => {}
            ActiveCore::Nes(emu) => {
                if let Some(patch) = zeff_nes_core::cheats::decode_nes_game_genie(code) {
                    emu.add_game_genie_patch(patch);
                }
            }
            ActiveCore::Sega8(_) => {}
            ActiveCore::Ws(_) => {}
        }
    }

    pub fn refresh_system_ram(&mut self) {
        match &self.core {
            ActiveCore::Gb(emu) => {
                let wram = emu.wram_snapshot();
                self.system_ram_buf.resize(wram.len(), 0);
                self.system_ram_buf.copy_from_slice(wram);
            }
            ActiveCore::Gba(emu) => {
                let (ewram, iwram) = emu.system_ram();
                self.system_ram_buf.clear();
                self.system_ram_buf.extend_from_slice(ewram);
                self.system_ram_buf.extend_from_slice(iwram);
            }
            ActiveCore::Nes(emu) => {
                let ram = emu.system_ram();
                self.system_ram_buf.resize(ram.len(), 0);
                self.system_ram_buf.copy_from_slice(ram);
            }
            ActiveCore::Sega8(emu) => {
                let ram = emu.bus().work_ram();
                self.system_ram_buf.resize(ram.len(), 0);
                self.system_ram_buf.copy_from_slice(ram);
            }
            ActiveCore::Ws(emu) => {
                let ram = emu.system_ram();
                self.system_ram_buf.resize(ram.len(), 0);
                self.system_ram_buf.copy_from_slice(ram);
            }
        }
    }

    pub fn refresh_video_ram(&mut self) {
        match &mut self.core {
            ActiveCore::Gb(emu) => {
                let vram = emu.vram_snapshot();
                self.video_ram_buf.resize(vram.len(), 0);
                self.video_ram_buf.copy_from_slice(vram);
            }
            ActiveCore::Gba(emu) => {
                let vram = emu.vram_snapshot();
                self.video_ram_buf.resize(vram.len(), 0);
                self.video_ram_buf.copy_from_slice(vram);
            }
            ActiveCore::Nes(emu) => {
                let vram = emu.chr_ram_snapshot();
                self.video_ram_buf.resize(vram.len(), 0);
                self.video_ram_buf.copy_from_slice(&vram);
            }
            ActiveCore::Sega8(emu) => {
                let vram = emu.bus().vdp().vram();
                self.video_ram_buf.resize(vram.len(), 0);
                self.video_ram_buf.copy_from_slice(vram);
            }
            ActiveCore::Ws(_) => {
                self.video_ram_buf.clear();
            }
        }
    }

    pub fn system_ram_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gb(emu) => emu.wram_snapshot().len(),
            ActiveCore::Gba(_) => 0x48000,
            ActiveCore::Nes(_) => 0x800, // 2 KiB
            ActiveCore::Sega8(emu) => emu.bus().work_ram().len(),
            ActiveCore::Ws(emu) => emu.system_ram().len(),
        }
    }

    pub fn video_ram_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gb(emu) => emu.vram_snapshot().len(),
            ActiveCore::Gba(_) => zeff_gba_core::hardware::constants::VRAM_SIZE,
            ActiveCore::Nes(_) => 0x2000,
            ActiveCore::Sega8(emu) => emu.bus().vdp().vram().len(),
            ActiveCore::Ws(_) => 0,
        }
    }
}

fn map_host_to_nes_byte(buttons: u8, dpad: u8) -> u8 {
    (buttons & 0x0F)
        | ((dpad & 0x04) << 2)
        | ((dpad & 0x08) << 2)
        | ((dpad & 0x02) << 5)
        | ((dpad & 0x01) << 7)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_sega8(ext: &str) -> CoreState {
        CoreState::from_rom(&[0x76], &format!("test.{ext}")).expect("Sega 8-bit ROM should load")
    }

    fn ws_rom() -> Vec<u8> {
        let mut rom = vec![0xFF; 0x10000];
        rom[0..2].copy_from_slice(&[0x90, 0xF4]);
        let reset = rom.len() - 16;
        rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
        let footer = rom.len() - 10;
        rom[footer + 4] = 0x01;
        let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        rom
    }

    #[test]
    fn sega8_extensions_select_expected_systems() {
        let sms = load_sega8("sms");
        assert!(matches!(sms.core, ActiveCore::Sega8(_)));
        assert_eq!(sms.system_label(), "SMS");
        assert_eq!(sms.native_width(), 256);
        assert_eq!(sms.native_height(), 192);
        assert_eq!(sms.sram_size(), 0);

        let gg = load_sega8("gg");
        assert!(matches!(gg.core, ActiveCore::Sega8(_)));
        assert_eq!(gg.system_label(), "Game Gear");
        assert_eq!(gg.native_width(), 160);
        assert_eq!(gg.native_height(), 144);
        assert_eq!(gg.sram_size(), 0);

        for ext in ["sg", "sc"] {
            let sg = load_sega8(ext);
            assert!(matches!(sg.core, ActiveCore::Sega8(_)));
            assert_eq!(sg.system_label(), "SG-1000/SC-3000");
            assert_eq!(sg.native_width(), 256);
            assert_eq!(sg.native_height(), 192);
            assert_eq!(sg.sram_size(), 0);
        }
    }

    #[test]
    fn libretro_valid_extensions_include_gba_and_sega8() {
        let extensions = crate::callbacks::VALID_EXTENSIONS
            .to_str()
            .expect("valid extensions should be UTF-8");

        for ext in [
            "gb", "gbc", "gba", "nes", "ws", "wsc", "sms", "gg", "sg", "sc",
        ] {
            assert!(
                extensions.split('|').any(|entry| entry == ext),
                "missing extension: {ext}"
            );
        }
    }

    #[test]
    fn wonderswan_extensions_select_ws_core() {
        let rom = ws_rom();
        for ext in ["ws", "wsc"] {
            let state = CoreState::from_rom(&rom, &format!("test.{ext}"))
                .expect("WonderSwan ROM should load");

            assert!(matches!(state.core, ActiveCore::Ws(_)));
            assert_eq!(state.system_label(), "WonderSwan");
            assert_eq!(state.native_width(), 224);
            assert_eq!(state.native_height(), 144);
            assert!(state.encode_state().is_ok());
        }
    }
}
