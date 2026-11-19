const NORMAL: [u8; 8] = [0, 2, 4, 7, 9, 12, 14, 16];
const SHADOW: [u8; 8] = [0, 1, 2, 3, 4, 6, 7, 8];
const HIGHLIGHT: [u8; 8] = [8, 9, 10, 12, 13, 14, 15, 16];
const MODE4: [u8; 8] = [0, 0, 5, 5, 11, 11, 16, 16];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct DacDrive {
    channels: [u32; 3],
    mode5: bool,
}

impl DacDrive {
    pub fn decode(channels: [u8; 3], mode5: bool, shadow: bool, highlight: bool) -> Self {
        Self {
            channels: channels.map(|channel| channel_drive(channel, mode5, shadow, highlight)),
            mode5,
        }
    }

    pub fn channel_masks(self) -> [u32; 3] {
        self.channels
    }

    pub fn mode5(self) -> bool {
        self.mode5
    }
}

pub(super) fn route_channels(color: u16, visible: bool, high_bits: bool, mode5: bool) -> [u8; 3] {
    if !visible {
        return [0; 3];
    }
    let mask = if high_bits { 7 } else { 1 };
    std::array::from_fn(|channel| {
        if mode5 {
            ((color >> (channel * 3)) as u8) & mask
        } else {
            (((color >> (channel * 2)) as u8 & 3) << 1) & mask
        }
    })
}

fn channel_drive(channel: u8, mode5: bool, shadow: bool, highlight: bool) -> u32 {
    let index = usize::from(channel & 7);
    let mut drive = 0;
    if mode5 && !(shadow || highlight) {
        drive |= 1 << NORMAL[index];
    }
    if !mode5 {
        drive |= 1 << MODE4[index];
    }
    if shadow && !highlight {
        drive |= 1 << SHADOW[index];
    }
    if highlight {
        drive |= 1 << HIGHLIGHT[index];
    }
    drive
}
