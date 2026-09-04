use super::{ActiveCore, CoreState};
use crate::input::InputDescriptorConfig;

impl CoreState {
    pub fn step_frame(&mut self) {
        match &mut self.core {
            ActiveCore::Gb(emu) => emu.step_frame(),
            ActiveCore::Gba(emu) => emu.step_frame(),
            ActiveCore::Nes(emu) => emu.step_frame(),
            ActiveCore::Pce(host) => host.step_frame(),
            ActiveCore::Sega8(emu) => emu.step_frame(),
            ActiveCore::Ws(emu) => emu.step_frame(),
        }
        self.apply_ram_cheats();
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
            ActiveCore::Pce(host) => {
                host.drain_audio_samples_into(&mut self.audio_buf);
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
            ActiveCore::Pce(host) => host.set_input(buttons, dpad),
            ActiveCore::Sega8(emu) => emu.set_input(buttons, dpad),
            ActiveCore::Ws(emu) => emu.set_input(buttons, dpad),
        }
    }

    pub fn set_input_p2(&mut self, buttons: u8, dpad: u8) {
        match &mut self.core {
            ActiveCore::Nes(emu) => emu.set_input_p2(buttons, dpad),
            ActiveCore::Sega8(emu) => emu.set_input_p2(buttons, dpad),
            ActiveCore::Gb(_) | ActiveCore::Gba(_) | ActiveCore::Pce(_) | ActiveCore::Ws(_) => {}
        }
    }

    pub fn set_zapper_state(&mut self, trigger: bool, hit: bool) {
        if let ActiveCore::Nes(emu) = &mut self.core {
            emu.set_zapper_state(true, trigger, hit, None);
        }
    }

    pub fn supports_p2_joypad(&self) -> bool {
        match &self.core {
            ActiveCore::Nes(_) => true,
            ActiveCore::Sega8(emu) => !matches!(
                emu.system(),
                zeff_sega8_core::hardware::cartridge::Sega8System::GameGear
            ),
            ActiveCore::Gb(_) | ActiveCore::Gba(_) | ActiveCore::Pce(_) | ActiveCore::Ws(_) => {
                false
            }
        }
    }

    pub fn supports_p2_lightgun(&self) -> bool {
        matches!(&self.core, ActiveCore::Nes(_))
    }

    pub fn supports_shoulders(&self) -> bool {
        matches!(&self.core, ActiveCore::Gba(_))
    }

    pub fn input_descriptor_config(&self) -> InputDescriptorConfig {
        InputDescriptorConfig {
            supports_shoulders: self.supports_shoulders(),
            supports_p2_joypad: self.supports_p2_joypad(),
            supports_p2_lightgun: self.supports_p2_lightgun(),
        }
    }

    pub fn fps(&self) -> f64 {
        match &self.core {
            ActiveCore::Gb(_) => zeff_gb_core::hardware::types::constants::FRAME_RATE_HZ,
            ActiveCore::Gba(_) => zeff_gba_core::hardware::constants::FPS,
            ActiveCore::Nes(_) => zeff_nes_core::hardware::constants::NTSC_FRAME_RATE_HZ,
            ActiveCore::Pce(_) => zeff_emu_common::system::System::Pce.target_fps(),
            ActiveCore::Sega8(emu) => emu.video_standard().frame_rate_approx() as f64,
            ActiveCore::Ws(_) => zeff_ws_core::hardware::constants::FPS,
        }
    }

    pub fn is_pal_region(&self) -> bool {
        matches!(
            &self.core,
            ActiveCore::Sega8(emu)
                if emu.video_standard()
                    == zeff_sega8_core::hardware::timing::Sega8VideoStandard::Pal
        )
    }

    pub fn take_runtime_fault(&mut self) -> Option<String> {
        match &mut self.core {
            ActiveCore::Pce(host) => host.take_runtime_fault(),
            ActiveCore::Gb(_)
            | ActiveCore::Gba(_)
            | ActiveCore::Nes(_)
            | ActiveCore::Sega8(_)
            | ActiveCore::Ws(_) => None,
        }
    }

    pub fn encode_state(&self) -> anyhow::Result<Vec<u8>> {
        match &self.core {
            ActiveCore::Gb(emu) => emu.encode_state(),
            ActiveCore::Gba(emu) => emu.encode_state(),
            ActiveCore::Nes(emu) => emu.encode_state(),
            ActiveCore::Pce(host) => host.encode_state(),
            ActiveCore::Sega8(emu) => emu.encode_state(),
            ActiveCore::Ws(emu) => emu.encode_state(),
        }
    }

    pub fn load_state(&mut self, data: &[u8]) -> anyhow::Result<()> {
        match &mut self.core {
            ActiveCore::Gb(emu) => emu.load_state(data).map(|_| ()),
            ActiveCore::Gba(emu) => emu.load_state(data),
            ActiveCore::Nes(emu) => emu.load_state(data),
            ActiveCore::Pce(host) => host.load_state(data),
            ActiveCore::Sega8(emu) => emu.load_state(data),
            ActiveCore::Ws(emu) => emu.load_state(data),
        }
    }

    #[allow(dead_code)]
    pub fn serialize_size(&self) -> usize {
        self.encode_state().map_or(0, |v| v.len())
    }

    pub fn fixed_serialize_size(&self) -> Option<usize> {
        match &self.core {
            ActiveCore::Pce(host) => Some(host.max_encoded_state_bytes()),
            ActiveCore::Gb(_)
            | ActiveCore::Gba(_)
            | ActiveCore::Nes(_)
            | ActiveCore::Sega8(_)
            | ActiveCore::Ws(_) => None,
        }
    }
}
