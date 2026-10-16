use super::*;
use crate::save_state::{read_section, write_section};
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

impl Bus {
    pub(crate) fn write_state(&self, writer: &mut StateWriter) -> Result<()> {
        self.validate_state()?;
        writer.write_u64(self.now.get());
        writer.write_u64(self.cpu_clock.0);
        writer.write_u8(self.irq_level);
        writer.write_u64(self.reset_pulses);
        writer.write_bool(self.prefetch_upper.is_some());
        if let Some(value) = self.prefetch_upper {
            writer.write_u8(value);
        }
        writer.write_bytes(&self.ram);
        write_section(writer, |writer| {
            self.input.write_state(writer);
            Ok(())
        })?;
        write_section(writer, |writer| self.sound.write_state(writer))?;
        write_section(writer, |writer| {
            self.vdp.write_state_in_region(writer, self.region)
        })?;
        write_section(writer, |writer| self.video.write_state(writer))?;
        write_section(writer, |writer| self.audio.write_state(writer))?;
        Ok(())
    }

    pub(crate) fn read_state(&mut self, reader: &mut StateReader<'_>, version: u32) -> Result<()> {
        let mut candidate = self.clone();
        candidate.now = MasterTicks::new(reader.read_u64()?);
        candidate.cpu_clock = CpuClocks(reader.read_u64()?);
        candidate.irq_level = reader.read_u8()?;
        candidate.reset_pulses = reader.read_u64()?;
        candidate.prefetch_upper = if reader.read_bool()? {
            Some(reader.read_u8()?)
        } else {
            None
        };
        candidate.reset_output = false;
        candidate.clock_fault = false;
        reader.read_exact(&mut candidate.ram)?;
        read_section(reader, |reader| {
            candidate
                .input
                .read_state(reader, candidate.now.get(), version)
        })?;
        read_section(reader, |reader| {
            candidate.sound = SoundMemory::read_state(reader)?;
            Ok(())
        })?;
        read_section(reader, |reader| {
            candidate
                .vdp
                .read_state_version(reader, candidate.region, version)
        })?;
        read_section(reader, |reader| {
            candidate
                .video
                .read_state_version(reader, candidate.region, version)
        })?;
        read_section(reader, |reader| {
            candidate
                .audio
                .read_state_version(reader, candidate.region, version)
        })?;
        candidate.validate_state()?;
        *self = candidate;
        Ok(())
    }

    pub(crate) fn validate_state(&self) -> Result<()> {
        if let Some(save) = &self.cartridge.save {
            save.validate_state()?;
        }
        ensure!(
            !self.clock_fault && !self.reset_output,
            "MD state cannot contain an interrupted bus operation"
        );
        ensure!(
            self.irq_level <= 7 && self.ram.len() == WORK_RAM_SIZE,
            "invalid MD bus state"
        );
        ensure!(self.vdp.now() == self.now, "MD VDP and bus clock mismatch");
        ensure!(
            self.audio.state_now() == self.now.get(),
            "MD audio and bus clock mismatch"
        );
        self.vdp.validate_state(self.region)?;
        self.video.validate_calendar(
            self.vdp
                .field_position(self.region)
                .map_err(|error| anyhow::anyhow!("invalid MD field calendar: {error:?}"))?,
        )?;
        Ok(())
    }
}
