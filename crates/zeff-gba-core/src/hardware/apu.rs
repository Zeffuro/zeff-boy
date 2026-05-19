#[derive(Clone, Debug)]
pub struct Apu {
    sample_rate: u32,
    sample_generation_enabled: bool,
    channel_mutes: [bool; 6],
}

impl Default for Apu {
    fn default() -> Self {
        Self::new(48_000)
    }
}

impl Apu {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            sample_generation_enabled: true,
            channel_mutes: [false; 6],
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.max(1);
    }

    pub fn set_sample_generation_enabled(&mut self, enabled: bool) {
        self.sample_generation_enabled = enabled;
    }

    pub fn set_channel_mutes(&mut self, mutes: [bool; 6]) {
        self.channel_mutes = mutes;
    }

    pub fn drain_samples_into(&mut self, _buf: &mut Vec<f32>) {}

    pub(crate) fn sample_generation_enabled(&self) -> bool {
        self.sample_generation_enabled
    }

    pub(crate) fn channel_mutes(&self) -> [bool; 6] {
        self.channel_mutes
    }
}
