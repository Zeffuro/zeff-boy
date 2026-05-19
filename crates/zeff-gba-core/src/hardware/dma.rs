#[derive(Clone, Copy, Debug, Default)]
pub struct DmaChannel {
    pub source: u32,
    pub destination: u32,
    pub count: u16,
    pub control: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DmaController {
    channels: [DmaChannel; 4],
}

impl DmaController {
    pub fn channels(&self) -> [DmaChannel; 4] {
        self.channels
    }

    pub fn set_channels(&mut self, channels: [DmaChannel; 4]) {
        self.channels = channels;
    }

    pub fn channel(&self, channel: usize) -> DmaChannel {
        self.channels.get(channel).copied().unwrap_or_default()
    }

    pub fn set_channel(&mut self, channel: usize, value: DmaChannel) {
        if let Some(ch) = self.channels.get_mut(channel) {
            *ch = value;
        }
    }

    pub fn read16(&self, channel: usize, reg: usize) -> u16 {
        let ch = self.channels.get(channel).copied().unwrap_or_default();
        match reg {
            0 => ch.source as u16,
            1 => (ch.source >> 16) as u16,
            2 => ch.destination as u16,
            3 => (ch.destination >> 16) as u16,
            4 => ch.count,
            5 => ch.control,
            _ => 0,
        }
    }

    pub fn write16(&mut self, channel: usize, reg: usize, value: u16) {
        if let Some(ch) = self.channels.get_mut(channel) {
            match reg {
                0 => ch.source = (ch.source & 0xFFFF_0000) | u32::from(value),
                1 => ch.source = (ch.source & 0x0000_FFFF) | (u32::from(value) << 16),
                2 => ch.destination = (ch.destination & 0xFFFF_0000) | u32::from(value),
                3 => ch.destination = (ch.destination & 0x0000_FFFF) | (u32::from(value) << 16),
                4 => ch.count = value,
                5 => ch.control = value,
                _ => {}
            }
        }
    }
}
