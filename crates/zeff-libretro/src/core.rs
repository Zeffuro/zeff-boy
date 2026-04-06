use zeff_gb_core::hardware::ppu::DmgPalettePreset;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_nes_core::hardware::ppu::NesPaletteMode;

pub(crate) enum ActiveCore {
    Gb(Box<zeff_gb_core::emulator::Emulator>),
    Nes(Box<zeff_nes_core::emulator::Emulator>),
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
            "nes" => {
                let emu = zeff_nes_core::emulator::Emulator::new(data, sample_rate as f64)?;
                ActiveCore::Nes(Box::new(emu))
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
            ActiveCore::Nes(emu) => emu.reset(),
        }
    }

    pub fn step_frame(&mut self) {
        match &mut self.core {
            ActiveCore::Gb(emu) => emu.step_frame(),
            ActiveCore::Nes(emu) => emu.step_frame(),
        }
    }

    pub fn framebuffer(&self) -> &[u8] {
        match &self.core {
            ActiveCore::Gb(emu) => emu.framebuffer(),
            ActiveCore::Nes(emu) => emu.framebuffer(),
        }
    }

    pub fn drain_audio(&mut self) {
        self.audio_buf.clear();
        match &mut self.core {
            ActiveCore::Gb(emu) => {
                emu.drain_audio_samples_into(&mut self.audio_buf);
            }
            ActiveCore::Nes(emu) => {
                emu.drain_audio_into_stereo(&mut self.audio_buf);
            }
        }
    }

    pub fn set_input(&mut self, buttons: u8, dpad: u8) {
        match &mut self.core {
            ActiveCore::Gb(emu) => emu.set_input(buttons, dpad),
            ActiveCore::Nes(emu) => {
                emu.set_input_p1(map_host_to_nes_byte(buttons, dpad));
            }
        }
    }

    pub fn set_input_p2(&mut self, buttons: u8, dpad: u8) {
        if let ActiveCore::Nes(emu) = &mut self.core {
            emu.set_input_p2(map_host_to_nes_byte(buttons, dpad));
        }
    }

    pub fn set_zapper_state(&mut self, trigger: bool, hit: bool) {
        if let ActiveCore::Nes(emu) = &mut self.core {
            emu.set_zapper_state(trigger, hit);
        }
    }

    pub fn native_width(&self) -> u32 {
        match &self.core {
            ActiveCore::Gb(emu) => {
                let (w, _) = emu.framebuffer_dimensions();
                w as u32
            }
            ActiveCore::Nes(_) => 256,
        }
    }

    pub fn native_height(&self) -> u32 {
        match &self.core {
            ActiveCore::Gb(emu) => {
                let (_, h) = emu.framebuffer_dimensions();
                h as u32
            }
            ActiveCore::Nes(_) => 240,
        }
    }

    pub fn fps(&self) -> f64 {
        match &self.core {
            ActiveCore::Gb(_) => 59.7275,
            ActiveCore::Nes(_) => 60.0988,
        }
    }

    pub fn encode_state(&self) -> anyhow::Result<Vec<u8>> {
        match &self.core {
            ActiveCore::Gb(emu) => emu.encode_state_bytes(),
            ActiveCore::Nes(emu) => emu.encode_state(),
        }
    }

    pub fn load_state(&mut self, data: &[u8]) -> anyhow::Result<()> {
        match &mut self.core {
            ActiveCore::Gb(emu) => emu.load_state_from_bytes(data.to_vec()),
            ActiveCore::Nes(emu) => emu.load_state(data),
        }
    }

    pub fn serialize_size(&self) -> usize {
        self.encode_state().map_or(0, |v| v.len())
    }

    pub fn battery_sram(&self) -> Option<Vec<u8>> {
        match &self.core {
            ActiveCore::Gb(emu) => emu.dump_battery_sram(),
            ActiveCore::Nes(emu) => emu.dump_battery_sram(),
        }
    }

    pub fn load_battery_sram(&mut self, data: &[u8]) {
        match &mut self.core {
            ActiveCore::Gb(emu) => {
                let _ = emu.load_battery_sram(data);
            }
            ActiveCore::Nes(emu) => {
                let _ = emu.load_battery_sram(data);
            }
        }
    }

    pub fn framebuffer_as_xrgb8888(&mut self) -> &[u8] {
        let fb = self.framebuffer().to_vec();
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
        let fb = self.framebuffer().to_vec();
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

    pub fn is_gb(&self) -> bool {
        matches!(self.core, ActiveCore::Gb(_))
    }

    pub fn is_nes(&self) -> bool {
        matches!(self.core, ActiveCore::Nes(_))
    }

    pub fn cheat_reset(&mut self) {
        match &mut self.core {
            ActiveCore::Gb(emu) => emu.clear_rom_patches(),
            ActiveCore::Nes(emu) => emu.clear_game_genie(),
        }
    }

    pub fn cheat_set(&mut self, code: &str) {
        match &mut self.core {
            ActiveCore::Gb(_) => {
                if let Ok((patches, _)) = zeff_gb_core::cheats::parse_cheat(code) {
                    if let ActiveCore::Gb(emu) = &mut self.core {
                        for p in patches {
                            emu.add_rom_patch(p);
                        }
                    }
                }
            }
            ActiveCore::Nes(_) => {
                if let Some(patch) = zeff_nes_core::cheats::decode_nes_game_genie(code) {
                    if let ActiveCore::Nes(emu) = &mut self.core {
                        emu.add_game_genie_patch(patch);
                    }
                }
            }
        }
    }

    pub fn refresh_system_ram(&mut self) {
        match &self.core {
            ActiveCore::Gb(emu) => {
                let wram = emu.wram_snapshot();
                self.system_ram_buf.resize(wram.len(), 0);
                self.system_ram_buf.copy_from_slice(wram);
            }
            ActiveCore::Nes(emu) => {
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
            ActiveCore::Nes(emu) => {
                let vram = emu.chr_ram_snapshot();
                self.video_ram_buf.resize(vram.len(), 0);
                self.video_ram_buf.copy_from_slice(&vram);
            }
        }
    }

    pub fn system_ram_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gb(emu) => emu.wram_snapshot().len(),
            ActiveCore::Nes(_) => 0x800, // 2 KiB
        }
    }

    pub fn video_ram_size(&self) -> usize {
        match &self.core {
            ActiveCore::Gb(emu) => emu.vram_snapshot().len(),
            ActiveCore::Nes(_) => 0x2000,
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
