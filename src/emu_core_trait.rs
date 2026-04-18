use std::path::Path;

use crate::audio_recorder::MidiApuSnapshot;

pub(crate) trait DebuggableEmulator {
    fn add_breakpoint(&mut self, addr: u16);
    fn add_watchpoint(&mut self, addr: u16, wt: zeff_emu_common::debug::WatchType);
    fn remove_breakpoint(&mut self, addr: u16);
    fn toggle_breakpoint(&mut self, addr: u16);
    fn debug_write(&mut self, addr: u16, val: u8);
}

pub(crate) trait EmulatorCore {
    fn step_frame(&mut self);
    fn framebuffer(&self) -> &[u8];
    fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>);
    #[allow(dead_code)]
    fn drain_audio_samples(&mut self) -> Vec<f32> {
        let mut buf = Vec::new();
        self.drain_audio_samples_into(&mut buf);
        buf
    }
    fn set_sample_rate(&mut self, rate: u32);
    fn set_apu_sample_generation_enabled(&mut self, enabled: bool);
    fn set_apu_channel_mutes(&mut self, mutes: &[bool]);
    fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8);
    fn is_suspended(&self) -> bool;
    fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>>;
    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>>;
    fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()>;
    fn rom_path(&self) -> &Path;
    fn rom_hash(&self) -> [u8; 32];

    #[inline]
    fn set_input_p2(&mut self, _buttons_pressed: u8, _dpad_pressed: u8) {}
    #[inline]
    fn apu_channel_snapshot(&self) -> Option<MidiApuSnapshot> {
        None
    }
    #[inline]
    fn rumble_active(&self) -> bool {
        false
    }
    #[inline]
    fn is_mbc7(&self) -> bool {
        false
    }
    #[inline]
    fn is_pocket_camera(&self) -> bool {
        false
    }
}
