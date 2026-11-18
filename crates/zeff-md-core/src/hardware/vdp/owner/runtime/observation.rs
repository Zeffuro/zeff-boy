use super::*;
use crate::hardware::vdp::owner::domain::RenderGeometry;
use crate::hardware::vdp::owner::internal_ram::VideoPixel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OwnedGeometry {
    pub mode5: bool,
    pub height: u16,
    pub horizontal: u16,
    pub vertical: u16,
    pub horizontal_active: bool,
    pub vertical_active: bool,
    pub display_enabled: bool,
    pub wide: bool,
}

impl From<RenderGeometry> for OwnedGeometry {
    fn from(value: RenderGeometry) -> Self {
        Self {
            mode5: value.mode5,
            height: value.height,
            horizontal: value.horizontal,
            vertical: value.vertical,
            horizontal_active: value.horizontal_active,
            vertical_active: value.vertical_active,
            display_enabled: value.display_enabled,
            wide: value.wide,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OwnedPixel {
    pub field: u64,
    pub x: u16,
    pub y: u16,
    pub wide: bool,
    pub height: u16,
    pub mode5: bool,
    pub output_mode5: bool,
    pub horizontal_active: bool,
    pub vertical_active: bool,
    pub rgb: u32,
}

impl OwnedPixel {
    pub fn rectangular(self) -> bool {
        self.y < self.height && self.x < if self.wide { 320 } else { 256 }
    }
    pub fn visible(self) -> bool {
        self.horizontal_active && self.vertical_active && self.rectangular()
    }
    pub(in crate::hardware::vdp::owner) fn resolve(pixel: VideoPixel) -> Result<Self> {
        let mut rgb = 0;
        if pixel.tag.visible() {
            for (channel, mask) in pixel.rgb.channel_masks().into_iter().enumerate() {
                ensure!(
                    mask.count_ones() == 1,
                    "unresolved visible VDP DAC conflict"
                );
                let drive = mask.trailing_zeros();
                let value = if pixel.rgb.mode5() {
                    let level = match drive {
                        0..=4 => drive,
                        6..=10 => drive - 1,
                        12..=16 => drive - 2,
                        _ => anyhow::bail!("unowned M5 DAC drive"),
                    };
                    level * 255 / 14
                } else {
                    match drive {
                        0 => 0,
                        5 => {
                            if channel == 2 {
                                102
                            } else {
                                85
                            }
                        }
                        11 => 170,
                        16 => 255,
                        _ => anyhow::bail!("unowned M4 DAC drive"),
                    }
                };
                rgb |= value << (channel * 8);
            }
        }
        let tag = pixel.tag;
        Ok(Self {
            field: tag.field,
            x: tag.x,
            y: tag.y,
            wide: tag.wide,
            height: tag.height,
            mode5: tag.mode5,
            output_mode5: pixel.rgb.mode5(),
            horizontal_active: tag.horizontal_active,
            vertical_active: tag.vertical_active,
            rgb,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OwnedObservation {
    pub half: u64,
    pub board_clocks: [bool; 2],
    pub psg_write: bool,
    pub internal_sequence: u64,
    pub registers: [u8; 24],
    pub status: u16,
    pub hv: u16,
    pub interrupt: u8,
    pub sound_interrupt: bool,
    pub geometry: OwnedGeometry,
    pub raster_field: u64,
    pub pixel: Option<OwnedPixel>,
}
