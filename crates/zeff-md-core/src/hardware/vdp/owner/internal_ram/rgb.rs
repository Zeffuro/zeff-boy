use super::dac::{DacDrive, route_channels};
use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Rgb {
    color: [u16; 2],
    gate: [bool; 2],
    channels: [[u8; 3]; 2],
    channel_input: [u8; 3],
    highlight: [u8; 2],
    shadow: [u8; 2],
    visible: bool,
    mode5: bool,
    color_tag: [Option<PixelTag>; 2],
    channel_tag: [Option<PixelTag>; 2],
    input_tag: Option<PixelTag>,
}

impl Rgb {
    pub fn software_origin() -> Self {
        Self {
            color: [0; 2],
            gate: [false; 2],
            channels: [[0; 3]; 2],
            channel_input: [0; 3],
            highlight: [0; 2],
            shadow: [0; 2],
            visible: false,
            mode5: true,
            color_tag: [None; 2],
            channel_tag: [None; 2],
            input_tag: None,
        }
    }

    pub fn clock(
        &mut self,
        input: VideoInputs,
        color: u16,
        intensity: (bool, bool),
        tag: Option<PixelTag>,
    ) {
        if input.phases.horizontal_master {
            self.color[0] = color;
            self.color_tag[0] = tag;
            self.gate[0] = self.visible;
            self.channels[0] = self.channel_input;
            self.channel_tag[0] = self.input_tag;
            self.highlight[0] = ((self.highlight[1] << 1) | u8::from(intensity.0)) & 15;
            self.shadow[0] = ((self.shadow[1] << 1) | u8::from(intensity.1)) & 15;
        }
        if input.phases.horizontal_slave {
            self.color[1] = self.color[0];
            self.color_tag[1] = self.color_tag[0];
            self.gate[1] = self.gate[0];
            self.channels[1] = self.channels[0];
            self.channel_tag[1] = self.channel_tag[0];
            self.highlight[1] = self.highlight[0];
            self.shadow[1] = self.shadow[0];
        }
        self.mode5 = input.profile.memory.registers[1] & 4 != 0;
        self.channel_input = route_channels(
            self.color[1],
            self.gate[1],
            input.profile.memory.registers[0] & 4 != 0,
            self.mode5,
        );
        self.visible = input.visible_gate;
        self.input_tag = self.color_tag[1];
    }

    pub fn output(&self) -> DacDrive {
        DacDrive::decode(
            self.channels[1],
            self.mode5,
            self.shadow[1] & 8 != 0,
            self.highlight[1] & 8 != 0,
        )
    }

    pub fn visible(&self) -> bool {
        self.visible
    }
    pub fn validate_frontier(&self, field: u64) -> Result<()> {
        ensure!(
            self.color_tag
                .into_iter()
                .chain(self.channel_tag)
                .chain([self.input_tag])
                .all(|tag| tag.is_none_or(|tag| tag.field <= field)),
            "RGB tag exceeds raster frontier"
        );
        Ok(())
    }
    pub fn pixel(&self) -> Option<VideoPixel> {
        self.channel_tag[1].map(|tag| VideoPixel {
            tag,
            rgb: self.output(),
        })
    }

    pub fn validate_input(&self, register0: u8, mode5: bool) -> Result<()> {
        ensure!(self.mode5 == mode5, "stale VDP DAC mode provenance");
        let expected = route_channels(self.color[1], self.gate[1], register0 & 4 != 0, mode5);
        ensure!(
            self.channel_input == expected,
            "stale VDP RGB channel input"
        );
        Ok(())
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        for color in self.color {
            w.write_u16(color);
        }
        for gate in self.gate {
            w.write_bool(gate);
        }
        for channels in self.channels {
            w.write_bytes(&channels);
        }
        w.write_bytes(&self.channel_input);
        w.write_bytes(&self.highlight);
        w.write_bytes(&self.shadow);
        w.write_bool(self.visible);
        w.write_bool(self.mode5);
        for tag in self
            .color_tag
            .into_iter()
            .chain(self.channel_tag)
            .chain([self.input_tag])
        {
            raster::write_tag(w, tag);
        }
    }

    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let color = [r.read_u16()?, r.read_u16()?];
        let gate = [r.read_bool()?, r.read_bool()?];
        let mut channels = [[0; 3]; 2];
        for value in &mut channels {
            r.read_exact(value)?;
        }
        let mut channel_input = [0; 3];
        r.read_exact(&mut channel_input)?;
        let state = Self {
            color,
            gate,
            channels,
            channel_input,
            highlight: [r.read_u8()?, r.read_u8()?],
            shadow: [r.read_u8()?, r.read_u8()?],
            visible: r.read_bool()?,
            mode5: r.read_bool()?,
            color_tag: [raster::read_tag(r)?, raster::read_tag(r)?],
            channel_tag: [raster::read_tag(r)?, raster::read_tag(r)?],
            input_tag: raster::read_tag(r)?,
        };
        state.validate()?;
        Ok(state)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.input_tag == self.color_tag[1],
            "stale RGB pixel provenance"
        );
        ensure!(
            self.color.into_iter().all(|color| color < 512)
                && self
                    .channels
                    .into_iter()
                    .flatten()
                    .chain(self.channel_input)
                    .all(|value| value < 8)
                && self
                    .highlight
                    .into_iter()
                    .chain(self.shadow)
                    .all(|value| value < 16),
            "invalid VDP RGB pipeline"
        );
        Ok(())
    }
}
