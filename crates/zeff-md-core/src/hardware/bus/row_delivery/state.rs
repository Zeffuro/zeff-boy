use super::*;
use crate::Region;

use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[cfg(test)]
#[path = "state/tests.rs"]
mod tests;

impl RowDelivery {
    #[cfg(test)]
    pub(in super::super) fn validate_calendar(
        &self,
        position: crate::hardware::vdp::FieldPosition,
    ) -> Result<()> {
        let region = if position.lines > 300 {
            Region::Pal
        } else {
            Region::NtscUs
        };
        self.validate_calendar_at(position, position.line_start, None, region, None)
    }

    pub(in super::super) fn validate_calendar_at(
        &self,
        position: crate::hardware::vdp::FieldPosition,
        now: MasterTicks,
        fetch_phase: Option<u64>,
        region: Region,
        vertical_next_field: Option<bool>,
    ) -> Result<()> {
        let height = if region == Region::Pal || vertical_next_field.is_some() {
            VDP_MAX_FRAME_HEIGHT
        } else {
            VDP_FRAME_HEIGHT
        };
        self.scroll.validate(
            position,
            height,
            now,
            fetch_phase,
            vertical_next_field.unwrap_or(u64::from(position.line) + 1 == position.lines),
        )?;
        ensure!(
            vertical_next_field.is_none()
                || matches!(self.height, VDP_FRAME_HEIGHT | VDP_MAX_FRAME_HEIGHT),
            "unqualified MD framebuffer height"
        );
        ensure!(
            self.framebuffer_known || !self.frame_ready,
            "unknown MD framebuffer marked ready"
        );
        ensure!(
            self.unknown_field
                .is_none_or(|field| field <= position.id && !self.framebuffer_known),
            "invalid unknown MD raster field"
        );
        if let Some(field) = self.field {
            ensure!(
                field.id <= position.id,
                "MD video field is ahead of its calendar"
            );
            if field.id == position.id && self.rows != 0 {
                ensure!(
                    field.mode == position.active_mode
                        && (!field.mode.interlaced() || field.odd == position.active_odd),
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
            field.mode.write_state(writer);
            writer.write_bool(field.odd);
        }
        writer.write_u8(self.valid_fields);
        self.scroll.write_state(writer);
        writer.write_bool(self.framebuffer_known);
        writer.write_bool(self.unknown_field.is_some());
        if let Some(field) = self.unknown_field {
            writer.write_u64(field);
        }
        self.write_owned_state(writer)?;
        Ok(())
    }

    #[cfg(test)]
    pub(in super::super) fn read_state(
        &mut self,
        reader: &mut StateReader<'_>,
        region: Region,
    ) -> Result<()> {
        self.read_state_profile(reader, region, false)
    }

    #[cfg(test)]
    pub(in super::super) fn read_state_profile(
        &mut self,
        reader: &mut StateReader<'_>,
        region: Region,
        vertical: bool,
    ) -> Result<()> {
        self.read_state_profile_kind(reader, region, vertical, false)
    }

    pub(in super::super) fn read_state_profile_kind(
        &mut self,
        reader: &mut StateReader<'_>,
        region: Region,
        vertical: bool,
        owned: bool,
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
            (owned && !interlaced && matches!(height, 192 | 224 | 240))
                || (!owned
                    && height == physical_height * if interlaced { 2 } else { 1 }
                    && (physical_height == VDP_FRAME_HEIGHT
                        || (physical_height == VDP_MAX_FRAME_HEIGHT
                            && (region == Region::Pal || vertical && !interlaced)))),
            "invalid MD framebuffer height"
        );
        ensure!(rows <= physical_height, "invalid MD framebuffer row cursor");
        let mut candidate = Self::default();
        let length = { FRAMEBUFFER_LEN };
        reader.read_exact(&mut candidate.framebuffer[..length])?;
        {
            candidate.field = if reader.read_bool()? {
                Some(RasterField {
                    id: reader.read_u64()?,
                    height: usize::from(reader.read_u16()?),
                    mode: InterlaceMode::read_state(reader)?,
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
                    field.height == physical_height && field.mode.interlaced() == interlaced,
                    "inconsistent MD raster field geometry"
                );
                ensure!(
                    field.mode.interlaced() || (!field.odd && candidate.valid_fields == 0),
                    "invalid progressive MD weave"
                );
                ensure!(
                    !interlaced
                        || rows < physical_height
                        || candidate.valid_fields & (1 << usize::from(field.odd)) != 0,
                    "invalid completed MD field"
                );
            } else {
                ensure!(candidate.valid_fields == 0, "missing MD weave identity");
            }
        }
        candidate.width = width;
        candidate.height = height;
        candidate.rows = rows;
        candidate.frame_count = frame_count;
        candidate.frame_ready = frame_ready;
        {
            candidate.scroll = ScrollFetch::read_state(reader)?;
        }
        {
            candidate.framebuffer_known = reader.read_bool()?;
            candidate.unknown_field = if reader.read_bool()? {
                Some(reader.read_u64()?)
            } else {
                None
            };
            ensure!(
                candidate.framebuffer_known || !frame_ready,
                "unknown MD framebuffer marked ready"
            );
            ensure!(
                candidate.unknown_field.is_none() || !candidate.framebuffer_known,
                "unknown MD field has known framebuffer"
            );
        }
        ensure!(
            candidate.field.is_some() || !interlaced || !candidate.framebuffer_known,
            "missing MD weave identity"
        );
        candidate.read_owned_state(reader, owned)?;
        *self = candidate;
        Ok(())
    }
}
