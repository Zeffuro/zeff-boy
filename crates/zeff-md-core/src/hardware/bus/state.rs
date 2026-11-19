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
        match self.prefetch {
            Prefetch::Empty => writer.write_u8(0),
            Prefetch::UpperOnly(value) => {
                writer.write_u8(1);
                writer.write_u8(value);
            }
            Prefetch::Full(value) => {
                writer.write_u8(2);
                writer.write_u16(value);
            }
        }
        writer.write_bytes(&self.ram);
        write_section(writer, |writer| {
            self.input.write_state(writer, self.input_state_tick()?)
        })?;
        write_section(writer, |writer| self.sound.write_state(writer))?;
        write_section(writer, |writer| {
            self.vdp.write_state_in_region(writer, self.region)
        })?;
        write_section(writer, |writer| self.video.write_state(writer))?;
        write_section(writer, |writer| self.audio.write_state(writer))?;
        self.write_owned_state(writer)?;
        Ok(())
    }

    pub(crate) fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        let mut candidate = self.clone();
        candidate.now = MasterTicks::new(reader.read_u64()?);
        candidate.cpu_clock = CpuClocks(reader.read_u64()?);
        candidate.irq_level = reader.read_u8()?;
        candidate.reset_pulses = reader.read_u64()?;
        candidate.prefetch = match reader.read_u8()? {
            0 => Prefetch::Empty,
            1 => Prefetch::UpperOnly(reader.read_u8()?),
            2 => Prefetch::Full(reader.read_u16()?),
            _ => anyhow::bail!("invalid MD prefetch state tag"),
        };
        candidate.reset_output = false;
        candidate.rmw = None;
        candidate.clock_fault = false;
        reader.read_exact(&mut candidate.ram)?;
        let input_bytes = reader.read_slice(crate::constants::MAX_STATE_BYTES)?;
        read_section(reader, |reader| {
            candidate.sound = SoundMemory::read_state(reader)?;
            Ok(())
        })?;
        read_section(reader, |reader| {
            candidate.vdp.read_state(reader, candidate.region)
        })?;
        read_section(reader, |reader| {
            let owned = candidate.vdp.is_owned();
            candidate.video.read_state_profile_kind(
                reader,
                candidate.region,
                !owned && candidate.vdp.vertical_next_field().is_some(),
                owned,
            )
        })?;
        read_section(reader, |reader| {
            candidate.audio.read_state(reader, candidate.region)
        })?;
        candidate.read_owned_state(reader)?;
        let mut input = StateReader::new(input_bytes);
        candidate
            .input
            .read_state(&mut input, candidate.input_state_tick()?, candidate.region)?;
        ensure!(input.is_exhausted(), "trailing MD input state");
        candidate.validate_state()?;
        *self = candidate;
        Ok(())
    }

    pub(crate) fn validate_state(&self) -> Result<()> {
        if self.owned.is_some() || self.vdp.is_owned() {
            return self.validate_owned_state();
        }
        self.cartridge.validate_expansion_save()?;
        self.input.validate_serial(self.now.get(), self.region)?;
        self.cartridge.validate_save_latch()?;
        self.cartridge.validate_download()?;
        if let Some(save) = &self.cartridge.save {
            if self.cartridge.save_latch.is_none() {
                save.validate_state()?;
            }
            save.validate_rom_profile(&self.cartridge.bytes)?;
        }
        ensure!(
            !self.clock_fault && !self.reset_output && self.rmw.is_none(),
            "MD state cannot contain an interrupted bus operation"
        );
        ensure!(
            self.irq_level <= 7 && self.ram.len() == WORK_RAM_SIZE,
            "invalid MD bus state"
        );
        ensure!(self.vdp.now() == self.now, "MD VDP and bus clock mismatch");
        ensure!(
            !self.vdp.has_timed_status()
                && !self.vdp.has_timed_control()
                && !self.vdp.has_main_data(),
            "MD state cannot contain an interrupted VDP port transaction"
        );
        ensure!(
            self.audio.state_now() == self.now.get(),
            "MD audio and bus clock mismatch"
        );
        self.vdp.validate_state(self.region)?;
        let position = self
            .vdp
            .field_position(self.region)
            .map_err(|error| anyhow::anyhow!("invalid MD field calendar: {error:?}"))?;
        let fetch_phase = self
            .vdp
            .scroll_fetch_phase(self.region)
            .map_err(|error| anyhow::anyhow!("invalid MD scroll calendar: {error:?}"))?;
        self.video.validate_calendar_at(
            position,
            self.now,
            Some(fetch_phase),
            self.region,
            self.vdp.vertical_next_field(),
        )?;
        Ok(())
    }
}
