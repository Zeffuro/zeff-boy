use super::*;
use crate::hardware::vdp::owner::runtime::{OwnedObservation, OwnedPixel};

impl<R: RenderBus> Domain<R> {
    pub(in crate::hardware::vdp::owner) fn pal(&self) -> bool {
        self.pal
    }
    pub(in crate::hardware::vdp::owner) fn interrupt_level(&self) -> u8 {
        self.irq.level() as u8
    }
    pub(in crate::hardware::vdp::owner) fn hv_word(&self) -> u16 {
        self.counter.drive(true, true).word
    }
    pub(in crate::hardware::vdp::owner) fn geometry(&self) -> RenderGeometry {
        self.render.geometry()
    }

    pub(in crate::hardware::vdp::owner) fn observe(&self) -> Result<OwnedObservation> {
        Ok(OwnedObservation {
            half: self.half,
            board_clocks: self.board_clocks(),
            psg_write: self.view(0)?.drives.levels.psg_write,
            internal_sequence: self.internal_sequence(),
            registers: self.registers(),
            status: self.status_word(),
            hv: self.hv_word(),
            interrupt: self.interrupt_level(),
            sound_interrupt: self.irq.sound,
            geometry: self.geometry().into(),
            raster_field: self.raster_field(),
            pixel: self.pixel().map(OwnedPixel::resolve).transpose()?,
        })
    }
}
