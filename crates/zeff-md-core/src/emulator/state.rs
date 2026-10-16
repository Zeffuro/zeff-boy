use super::*;
use crate::save_state::{
    MAX_STATE_BYTES, MIN_STATE_VERSION, STATE_MAGIC, STATE_VERSION, read_section, write_section,
};
use crate::{ControllerKind, ControllerPort};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn region_tag(region: Region) -> u8 {
    match region {
        Region::NtscUs => 0,
        Region::Pal => 1,
        Region::NtscJapan => 2,
    }
}

fn kind_tag(kind: ControllerKind) -> u8 {
    match kind {
        ControllerKind::ThreeButton => 0,
        ControllerKind::SixButton => 1,
    }
}

impl Emulator {
    pub fn encode_state(&self) -> Result<Vec<u8>> {
        self.validate_state()?;
        let mut writer = StateWriter::new();
        writer.write_bytes(STATE_MAGIC);
        writer.write_u32(STATE_VERSION);
        writer.write_bytes(&Sha256::digest(self.bus.cartridge.bytes.as_ref()));
        writer.write_u32(self.bus.cartridge.bytes.len() as u32);
        writer.write_u8(region_tag(self.region()));
        writer.write_bool(self.concurrent_sound);
        for port in [ControllerPort::One, ControllerPort::Two] {
            writer.write_u8(kind_tag(self.bus.input.controller_kind(port)));
        }
        write_section(&mut writer, |writer| self.cpu.write_boundary_state(writer))?;
        write_section(&mut writer, |writer| self.bus.write_state(writer))?;
        write_section(&mut writer, |writer| {
            self.sound_schedule.write_state(&self.sound_cpu, writer)
        })?;
        writer.write_bool(self.main_stopped);
        write_section(&mut writer, |writer| {
            writer.write_bool(self.bus.cartridge.save.is_some());
            if let Some(save) = &self.bus.cartridge.save {
                save.write_state(writer);
            }
            Ok(())
        })?;
        let bytes = writer.into_bytes();
        ensure!(
            bytes.len() <= MAX_STATE_BYTES,
            "MD state exceeds size bound"
        );
        Ok(bytes)
    }

    pub fn decode_state(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() <= MAX_STATE_BYTES,
            "MD state exceeds size bound"
        );
        let mut reader = StateReader::new(bytes);
        let mut magic = [0; STATE_MAGIC.len()];
        reader.read_exact(&mut magic)?;
        let version = reader.read_u32()?;
        ensure!(
            &magic == STATE_MAGIC && (MIN_STATE_VERSION..=STATE_VERSION).contains(&version),
            "unsupported MD save-state format"
        );
        let mut rom_hash = [0; 32];
        reader.read_exact(&mut rom_hash)?;
        let expected: [u8; 32] = Sha256::digest(self.bus.cartridge.bytes.as_ref()).into();
        ensure!(
            rom_hash == expected && reader.read_u32()? as usize == self.bus.cartridge.bytes.len(),
            "MD save-state ROM mismatch"
        );
        ensure!(
            reader.read_u8()? == region_tag(self.region()),
            "MD save-state region mismatch"
        );
        ensure!(
            reader.read_bool()? == self.concurrent_sound,
            "MD save-state scheduling mismatch"
        );
        for port in [ControllerPort::One, ControllerPort::Two] {
            ensure!(
                reader.read_u8()? == kind_tag(self.bus.input.controller_kind(port)),
                "MD save-state controller mismatch"
            );
        }
        let mut candidate = self.clone();
        ensure!(
            version >= crate::save_state::STATE_VERSION_WITH_SAVE_RAM
                || candidate.bus.cartridge.save.is_none(),
            "legacy MD state has no cartridge save RAM"
        );
        read_section(&mut reader, |reader| {
            candidate.cpu.read_boundary_state(reader)
        })?;
        read_section(&mut reader, |reader| {
            candidate.bus.read_state(reader, version)
        })?;
        read_section(&mut reader, |reader| {
            candidate
                .sound_schedule
                .read_state(&mut candidate.sound_cpu, reader)
        })?;
        candidate.main_stopped = reader.read_bool()?;
        if version >= crate::save_state::STATE_VERSION_WITH_SAVE_RAM {
            read_section(&mut reader, |reader| {
                ensure!(
                    reader.read_bool()? == candidate.bus.cartridge.save.is_some(),
                    "MD state save RAM board mismatch"
                );
                if let Some(save) = &mut candidate.bus.cartridge.save {
                    save.read_state(reader)?;
                }
                Ok(())
            })?;
        }
        ensure!(reader.is_exhausted(), "trailing MD save-state data");
        candidate.validate_state()?;
        candidate.invalidate_audio_trace(
            zeff_emu_common::audio_trace::AudioTraceInvalidation::StateRestore,
        );
        *self = candidate;
        Ok(())
    }

    fn validate_state(&self) -> Result<()> {
        self.bus.validate_state()?;
        ensure!(
            self.cpu.clock() == self.bus.cpu_clock,
            "MD CPU and bus clock mismatch"
        );
        ensure!(
            !self.main_stopped || self.cpu.stopped(),
            "invalid MD stopped state"
        );
        ensure!(
            self.cpu.stopped() || self.cpu.queued_words().is_some(),
            "MD main CPU has no prefetch"
        );
        self.sound_schedule
            .validate_state(&self.sound_cpu, self.bus.now.get())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod video_tests;
