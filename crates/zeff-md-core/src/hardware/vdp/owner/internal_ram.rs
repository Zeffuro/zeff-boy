mod cram;
mod dac;
pub(in crate::hardware::vdp) use dac::DacDrive;
mod input;
mod palette;
mod raster;
mod rgb;
mod state;
mod vsram;

use super::frontend::MemoryDrives;
use crate::hardware::vdp::control_core::PhaseLevels;
use anyhow::{Result, ensure};
use cram::Cram;
use input::Store;
pub(in crate::hardware::vdp) use input::{
    BusUpdate, PlanesInputs, PlanesOutput, Profile, Target, VideoInputs, VideoOutput, VsramView,
    Write,
};
use palette::Palette;
pub(in crate::hardware::vdp) use palette::{PalettePixel, PaletteSignals};
use raster::Raster;
pub(in crate::hardware::vdp) use raster::{PixelTag, VideoPixel};
use rgb::Rgb;
use vsram::Vsram;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct InternalRam {
    vsram: Vsram,
    cram: Cram,
    sequence: u64,
}

impl InternalRam {
    pub fn validate_video_source(
        &self,
        geometry: super::domain::RenderGeometry,
        display_active: bool,
    ) -> Result<()> {
        self.cram
            .palette
            .validate_input(geometry.mode5, display_active)?;
        self.cram.palette.validate_geometry(geometry)
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn raster_field(&self) -> u64 {
        self.cram.palette.field()
    }
    pub fn pixel(&self) -> Option<VideoPixel> {
        self.cram.rgb.pixel()
    }
    pub fn visible_gate(&self) -> bool {
        self.cram.rgb.visible()
    }
    pub fn software_origin() -> Self {
        Self {
            vsram: Vsram::software_origin(),
            cram: Cram::software_origin(),
            sequence: 0,
        }
    }

    pub fn clock_planes(
        &mut self,
        input: PlanesInputs,
        read_word: impl FnMut(u8) -> Result<u16>,
    ) -> Result<PlanesOutput> {
        input.profile.validate()?;
        ensure!(
            input.frontend.address <= 0x1ffff && input.horizontal_counter < 512,
            "invalid internal VDP planes input"
        );
        let mut candidate = *self;
        let before = candidate.vsram.view();
        let evaluation = candidate.vsram.evaluate(input, read_word)?;
        let write = candidate.receipt(evaluation.store)?;
        let output = PlanesOutput {
            before,
            after: candidate.vsram.view(),
            bus: evaluation.bus,
            write,
        };
        *self = candidate;
        Ok(output)
    }

    pub fn clock_video(
        &mut self,
        input: VideoInputs,
        read_word: impl FnMut(u8) -> Result<u16>,
    ) -> Result<VideoOutput> {
        input.profile.validate()?;
        ensure!(
            input.frontend.address <= 0x1ffff,
            "invalid internal VDP video input"
        );
        let mut candidate = *self;
        let before_packed = candidate.cram.out;
        let evaluation = candidate.cram.evaluate(input, read_word)?;
        let write = candidate.receipt(evaluation.store)?;
        let output = VideoOutput {
            before_packed,
            after_packed: candidate.cram.out,
            palette_address: candidate.cram.palette.address(),
            rgb: candidate.cram.rgb.output(),
            bus: evaluation.bus,
            write,
        };
        *self = candidate;
        Ok(output)
    }

    fn receipt(&mut self, store: Option<Store>) -> Result<Option<Write>> {
        let Some(store) = store else {
            return Ok(None);
        };
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("internal VDP RAM receipt overflow"))?;
        self.sequence = sequence;
        Ok(Some(Write {
            sequence,
            target: store.target,
            index: store.index,
            mask: store.mask,
            value: store.value,
        }))
    }
}
