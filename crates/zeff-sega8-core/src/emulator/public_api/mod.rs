use crate::emulator::Emulator;
use crate::hardware::apu::PSG_CHANNEL_COUNT;
use crate::hardware::input::ControllerPort;

mod debug;
mod queries;

impl Emulator {
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = if sample_rate == 0 {
            super::DEFAULT_SAMPLE_RATE
        } else {
            sample_rate
        };
        self.bus.set_apu_sample_rate(self.sample_rate);
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.bus.drain_audio_samples_into(buf);
    }

    pub fn drain_audio_samples(&mut self) -> Vec<f32> {
        let mut buf = Vec::new();
        self.drain_audio_samples_into(&mut buf);
        buf
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.bus.set_apu_sample_generation_enabled(enabled);
    }

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; PSG_CHANNEL_COUNT]) {
        self.bus.set_apu_channel_mutes(mutes);
    }

    pub fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        let mut raw = 0xFF;
        if dpad_pressed & super::HOST_DPAD_UP != 0 {
            raw &= !super::SMS_PAD_UP;
        }
        if dpad_pressed & super::HOST_DPAD_DOWN != 0 {
            raw &= !super::SMS_PAD_DOWN;
        }
        if dpad_pressed & super::HOST_DPAD_LEFT != 0 {
            raw &= !super::SMS_PAD_LEFT;
        }
        if dpad_pressed & super::HOST_DPAD_RIGHT != 0 {
            raw &= !super::SMS_PAD_RIGHT;
        }
        if buttons_pressed & super::HOST_BUTTON_1 != 0 {
            raw &= !super::SMS_PAD_BUTTON_1;
        }
        if buttons_pressed & super::HOST_BUTTON_2 != 0 {
            raw &= !super::SMS_PAD_BUTTON_2;
        }
        self.bus
            .input_mut()
            .set_controller_raw(ControllerPort::One, raw);
    }
}
