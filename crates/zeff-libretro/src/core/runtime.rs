use super::{ActiveCore, CoreState};

impl CoreState {
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
                emu.drain_audio_samples_into(&mut self.audio_buf);
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
            ActiveCore::Nes(emu) => emu.set_input(buttons, dpad),
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
            ActiveCore::Gb(emu) => emu.encode_state(),
            ActiveCore::Gba(emu) => emu.encode_state(),
            ActiveCore::Nes(emu) => emu.encode_state(),
            ActiveCore::Sega8(emu) => emu.encode_state(),
            ActiveCore::Ws(emu) => emu.encode_state(),
        }
    }

    pub fn load_state(&mut self, data: &[u8]) -> anyhow::Result<()> {
        match &mut self.core {
            ActiveCore::Gb(emu) => emu.load_state(data),
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
}

fn map_host_to_nes_byte(buttons: u8, dpad: u8) -> u8 {
    (buttons & 0x0F)
        | ((dpad & 0x04) << 2)
        | ((dpad & 0x08) << 2)
        | ((dpad & 0x02) << 5)
        | ((dpad & 0x01) << 7)
}
