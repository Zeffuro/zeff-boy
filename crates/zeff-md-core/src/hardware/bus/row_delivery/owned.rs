use super::*;
use crate::hardware::vdp::owner::OwnedPixel;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

mod surface;
pub(super) use surface::Surface;

const TAG: &[u8; 8] = b"ZRGBOWN1";

impl RowDelivery {
    pub(in crate::hardware::bus) fn begin_owned(&mut self) {
        *self = Self::default();
        self.framebuffer_known = false;
        self.owned = Some(Box::new(Surface::new()));
    }

    pub(in crate::hardware::bus) fn push_owned(&mut self, pixel: Option<OwnedPixel>) {
        let surface = self.owned.as_mut().expect("owned video profile");
        let before = surface.count();
        surface.push(pixel);
        if surface.count() != before {
            let (width, height) = surface
                .copy_last(&mut self.framebuffer)
                .expect("published physical raster");
            self.width = width;
            self.height = height;
            self.frame_count = surface.count();
            self.framebuffer_known = true;
            self.frame_ready = true;
        }
    }

    pub(in crate::hardware::bus) fn validate_owned(
        &self,
        field: u64,
        pixel: Option<OwnedPixel>,
    ) -> Result<()> {
        let surface = self
            .owned
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing owned raster state"))?;
        ensure!(
            self.pending.is_none() && self.completed.is_none() && self.raster_error.is_none(),
            "physical raster has a legacy capture"
        );
        ensure!(
            self.field.is_none()
                && self.rows == 0
                && self.valid_fields == 0
                && self.unknown_field.is_none(),
            "physical raster has a second calendar"
        );
        surface.validate_state(field)?;
        surface.validate_pixel(pixel)?;
        ensure!(
            self.frame_count == surface.count(),
            "physical presentation count differs"
        );
        ensure!(
            self.framebuffer_known == (self.frame_count != 0),
            "physical framebuffer provenance differs"
        );
        if self.framebuffer_known {
            let mut expected = vec![0; FRAMEBUFFER_LEN];
            let size = surface
                .copy_last(&mut expected)
                .ok_or_else(|| anyhow::anyhow!("missing published raster"))?;
            ensure!(
                size == (self.width, self.height),
                "physical presentation dimensions differ"
            );
            let length = size.0 * size.1 * RGBA_CHANNELS;
            ensure!(
                self.framebuffer[..length] == expected[..length],
                "physical framebuffer differs"
            );
        }
        Ok(())
    }

    pub(super) fn write_owned_state(&self, writer: &mut StateWriter) -> Result<()> {
        if let Some(surface) = &self.owned {
            writer.write_bytes(TAG);
            let mut payload = StateWriter::new();
            surface.write_state(&mut payload)?;
            writer.write_section(|writer| writer.write_bytes(&payload.into_bytes()));
        }
        Ok(())
    }

    pub(super) fn read_owned_state(
        &mut self,
        reader: &mut StateReader<'_>,
        owned: bool,
    ) -> Result<()> {
        if reader.is_exhausted() {
            ensure!(!owned, "missing physical raster state");
            self.owned = None;
            return Ok(());
        }
        ensure!(owned, "physical raster without VDP owner");
        let mut tag = [0; 8];
        reader.read_exact(&mut tag)?;
        ensure!(&tag == TAG, "unknown physical raster extension");
        let mut payload = StateReader::new(reader.read_slice(1024 * 1024)?);
        let surface = Surface::read_state(&mut payload)?;
        ensure!(
            payload.is_exhausted() && reader.is_exhausted(),
            "trailing physical raster state"
        );
        self.owned = Some(Box::new(surface));
        Ok(())
    }
}
