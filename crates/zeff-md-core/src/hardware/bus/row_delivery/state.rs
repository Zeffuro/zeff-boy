use super::*;
use crate::Region;
use crate::save_state::STATE_VERSION_WITH_INTERLACE;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[cfg(test)]
#[path = "state/tests.rs"]
mod tests;

impl RowDelivery {
    pub(in super::super) fn validate_calendar(
        &self,
        position: crate::hardware::vdp::FieldPosition,
    ) -> Result<()> {
        if let Some(field) = self.field {
            ensure!(
                field.id <= position.id,
                "MD video field is ahead of its calendar"
            );
            if field.id == position.id && self.rows != 0 {
                ensure!(
                    field.interlaced == position.active_interlaced
                        && (!field.interlaced || field.odd == position.active_odd),
                    "MD video field differs from active raster history"
                );
            }
        }
        Ok(())
    }

    pub(in super::super) fn write_state(&self, writer: &mut StateWriter) -> Result<()> {
        ensure!(
            self.pending.is_none() && self.completed.is_none() && self.raster_error.is_none(),
            "MD state cannot contain a diagnostic capture or raster error"
        );
        writer.write_u16(self.width as u16);
        writer.write_u16(self.height as u16);
        writer.write_u16(self.rows as u16);
        writer.write_u64(self.frame_count);
        writer.write_bool(self.frame_ready);
        writer.write_bytes(&self.framebuffer);
        writer.write_bool(self.field.is_some());
        if let Some(field) = self.field {
            writer.write_u64(field.id);
            writer.write_u16(field.height as u16);
            writer.write_bool(field.interlaced);
            writer.write_bool(field.odd);
        }
        writer.write_u8(self.valid_fields);
        Ok(())
    }

    #[cfg(test)]
    pub(in super::super) fn read_state(
        &mut self,
        reader: &mut StateReader<'_>,
        region: Region,
    ) -> Result<()> {
        self.read_state_version(reader, region, crate::save_state::STATE_VERSION)
    }

    pub(in super::super) fn read_state_version(
        &mut self,
        reader: &mut StateReader<'_>,
        region: Region,
        version: u32,
    ) -> Result<()> {
        let width = usize::from(reader.read_u16()?);
        let height = usize::from(reader.read_u16()?);
        let rows = usize::from(reader.read_u16()?);
        let frame_count = reader.read_u64()?;
        let frame_ready = reader.read_bool()?;
        ensure!(
            matches!(width, VDP_H32_FRAME_WIDTH | VDP_FRAME_WIDTH),
            "invalid MD framebuffer width"
        );
        let interlaced = height > VDP_MAX_FRAME_HEIGHT;
        let physical_height = if interlaced { height / 2 } else { height };
        ensure!(
            !interlaced || version >= STATE_VERSION_WITH_INTERLACE,
            "legacy MD interlace framebuffer"
        );
        ensure!(
            height == physical_height * if interlaced { 2 } else { 1 }
                && (physical_height == VDP_FRAME_HEIGHT
                    || (region == Region::Pal && physical_height == VDP_MAX_FRAME_HEIGHT)),
            "invalid MD framebuffer height"
        );
        ensure!(rows <= physical_height, "invalid MD framebuffer row cursor");
        let mut candidate = Self::default();
        let length = if version < STATE_VERSION_WITH_INTERLACE {
            LEGACY_FRAMEBUFFER_LEN
        } else {
            FRAMEBUFFER_LEN
        };
        reader.read_exact(&mut candidate.framebuffer[..length])?;
        if version >= STATE_VERSION_WITH_INTERLACE {
            candidate.field = if reader.read_bool()? {
                Some(RasterField {
                    id: reader.read_u64()?,
                    height: usize::from(reader.read_u16()?),
                    interlaced: reader.read_bool()?,
                    odd: reader.read_bool()?,
                })
            } else {
                None
            };
            candidate.valid_fields = reader.read_u8()?;
            ensure!(
                candidate.valid_fields & !3 == 0,
                "invalid MD weave validity"
            );
            if let Some(field) = candidate.field {
                ensure!(
                    field.height == physical_height && field.interlaced == interlaced,
                    "inconsistent MD raster field geometry"
                );
                ensure!(
                    field.interlaced || (!field.odd && candidate.valid_fields == 0),
                    "invalid progressive MD weave"
                );
                ensure!(
                    !interlaced
                        || rows < physical_height
                        || candidate.valid_fields & (1 << usize::from(field.odd)) != 0,
                    "invalid completed MD field"
                );
            } else {
                ensure!(
                    !interlaced && candidate.valid_fields == 0,
                    "missing MD weave identity"
                );
            }
        }
        candidate.width = width;
        candidate.height = height;
        candidate.rows = rows;
        candidate.frame_count = frame_count;
        candidate.frame_ready = frame_ready;
        *self = candidate;
        Ok(())
    }
}
