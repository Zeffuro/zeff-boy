mod capabilities;
mod download;
use super::*;
use crate::save_state::{MAX_STATE_BYTES, STATE_MAGIC, STATE_VERSION, read_section, write_section};
use crate::{ControllerKind, ControllerPort};
use anyhow::{Result, ensure};
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
        let version = STATE_VERSION;
        writer.write_u32(version);
        writer.write_bytes(&self.bus.cartridge.rom_digest());
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
        write_section(&mut writer, |writer| {
            writer.write_bool(self.bus.cartridge.registers.is_some());
            if let Some(registers) = &self.bus.cartridge.registers {
                registers.write_state(writer);
            }
            Ok(())
        })?;
        write_section(&mut writer, |writer| {
            writer.write_bool(self.bus.cartridge.sf001.is_some());
            if let Some(board) = &self.bus.cartridge.sf001 {
                board.write_state(writer);
            }
            Ok(())
        })?;
        self.write_capabilities(&mut writer)?;
        write_section(&mut writer, |writer| {
            writer.write_bool(self.bus.cartridge.parallel_adapter.is_some());
            if let Some(board) = &self.bus.cartridge.parallel_adapter {
                board.write_state(writer);
            }
            Ok(())
        })?;
        write_section(&mut writer, |writer| {
            writer.write_bool(self.bus.cartridge.sf004.is_some());
            if let Some(board) = &self.bus.cartridge.sf004 {
                board.write_state(writer);
            }
            Ok(())
        })?;
        write_section(&mut writer, |writer| {
            writer.write_bool(self.bus.cartridge.extended_ssf.is_some());
            if let Some(board) = &self.bus.cartridge.extended_ssf {
                board.write_state(writer);
            }
            Ok(())
        })?;
        write_section(&mut writer, |writer| {
            writer.write_bool(self.bus.cartridge.realtec.is_some());
            if let Some(board) = &self.bus.cartridge.realtec {
                board.write_state(writer);
            }
            Ok(())
        })?;
        self.write_download_state(&mut writer)?;
        self.write_tmss_state(&mut writer)?;
        self.write_svp_state(&mut writer)?;
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
            &magic == STATE_MAGIC && version == STATE_VERSION,
            "unsupported MD save-state format"
        );
        let mut rom_hash = [0; 32];
        reader.read_exact(&mut rom_hash)?;
        let expected = self.bus.cartridge.rom_digest();
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
        if let Some(board) = &mut candidate.bus.cartridge.lock_on {
            board.write_control(0);
        }

        read_section(&mut reader, |reader| {
            candidate.cpu.read_boundary_state(reader)
        })?;
        read_section(&mut reader, |reader| candidate.bus.read_state(reader))?;
        read_section(&mut reader, |reader| {
            candidate
                .sound_schedule
                .read_state(&mut candidate.sound_cpu, reader)
        })?;
        candidate.main_stopped = reader.read_bool()?;
        {
            read_section(&mut reader, |reader| {
                ensure!(
                    reader.read_bool()? == candidate.bus.cartridge.save.is_some(),
                    "MD state save RAM board mismatch"
                );
                let fixed_latch = candidate.bus.cartridge.save_latch.is_some();
                if let Some(save) = &mut candidate.bus.cartridge.save {
                    if fixed_latch {
                        save.read_state_with_fixed_latch(reader)?;
                    } else {
                        save.read_state(reader)?;
                    }
                }
                Ok(())
            })?;
        }
        {
            read_section(&mut reader, |reader| {
                ensure!(
                    reader.read_bool()? == candidate.bus.cartridge.registers.is_some(),
                    "MD cartridge register board mismatch"
                );
                if let Some(registers) = &mut candidate.bus.cartridge.registers {
                    registers.read_state(reader)?;
                }
                Ok(())
            })?;
        }
        {
            read_section(&mut reader, |reader| {
                ensure!(
                    reader.read_bool()? == candidate.bus.cartridge.sf001.is_some(),
                    "MD SF-001 state board mismatch"
                );
                if let Some(board) = &mut candidate.bus.cartridge.sf001 {
                    board.read_state(reader)?;
                }
                Ok(())
            })?;
        }
        {
            candidate.read_capabilities(&mut reader)?;
        }
        {
            read_section(&mut reader, |reader| {
                ensure!(
                    reader.read_bool()? == candidate.bus.cartridge.parallel_adapter.is_some(),
                    "MD parallel adapter owner mismatch"
                );
                if let Some(board) = &mut candidate.bus.cartridge.parallel_adapter {
                    board.read_state(reader)?;
                }
                Ok(())
            })?;
        }
        {
            read_section(&mut reader, |reader| {
                ensure!(
                    reader.read_bool()? == candidate.bus.cartridge.sf004.is_some(),
                    "MD SF-004 owner mismatch"
                );
                if let Some(board) = &mut candidate.bus.cartridge.sf004 {
                    board.read_state(reader)?;
                }
                Ok(())
            })?;
        }
        {
            read_section(&mut reader, |reader| {
                ensure!(
                    reader.read_bool()? == candidate.bus.cartridge.extended_ssf.is_some(),
                    "MD extended SSF owner mismatch"
                );
                if let Some(board) = &mut candidate.bus.cartridge.extended_ssf {
                    board.read_state(reader)?;
                }
                Ok(())
            })?;
        }
        {
            read_section(&mut reader, |reader| {
                ensure!(
                    reader.read_bool()? == candidate.bus.cartridge.realtec.is_some(),
                    "MD Realtec owner mismatch"
                );
                if let Some(board) = &mut candidate.bus.cartridge.realtec {
                    board.read_state(reader)?;
                }
                Ok(())
            })?;
        }
        candidate.read_download_state(&mut reader)?;
        candidate.read_tmss_state(&mut reader)?;
        candidate.read_svp_state(&mut reader)?;
        ensure!(reader.is_exhausted(), "trailing MD save-state data");
        candidate.validate_state()?;
        candidate.reset_call_stack();
        candidate.invalidate_audio_trace(
            zeff_emu_common::audio_trace::AudioTraceInvalidation::StateRestore,
        );
        *self = candidate;
        Ok(())
    }

    fn validate_state(&self) -> Result<()> {
        self.validate_svp_state()?;
        if let Some(tmss) = &self.bus.tmss {
            ensure!(self.bus.owned.is_none(), "TMSS requires Standard timing");
            ensure!(
                !tmss.boot_mapped() || self.bus.sound.reset_asserted,
                "unqualified TMSS SOUND boot mapping"
            );
        }
        self.bus.validate_state()?;
        let cartridge = &self.bus.cartridge;
        cartridge.validate_unprotected_rom()?;
        ensure!(
            cartridge.realtec.is_some()
                == crate::cartridge::realtec::Realtec::matches_identity(
                    cartridge.bytes.len(),
                    cartridge.rom_digest()
                ),
            "MD Realtec identity mismatch"
        );
        if let Some(board) = &cartridge.realtec {
            board.validate()?;
            ensure!(
                cartridge.save.is_none()
                    && cartridge.registers.is_none()
                    && cartridge.protection.is_none()
                    && cartridge.sf001.is_none()
                    && cartridge.sf002.is_none()
                    && cartridge.sf004.is_none()
                    && cartridge.extended_ssf.is_none()
                    && cartridge.ssf2.is_none()
                    && cartridge.lock_on.is_none()
                    && cartridge.lock_on_save.is_none()
                    && cartridge.linear_rom.is_none()
                    && cartridge.parallel_adapter.is_none(),
                "MD Realtec owner conflict"
            );
        }
        ensure!(
            cartridge.extended_ssf.is_some()
                == crate::cartridge::extended_ssf::ExtendedSsf::detect(&cartridge.bytes),
            "MD extended SSF identity mismatch"
        );
        if let Some(board) = &cartridge.extended_ssf {
            ensure!(
                board.matches_rom(&cartridge.bytes),
                "MD extended SSF geometry or page mismatch"
            );
            ensure!(
                cartridge.save.is_none()
                    && cartridge.registers.is_none()
                    && cartridge.protection.is_none()
                    && cartridge.sf001.is_none()
                    && cartridge.sf002.is_none()
                    && cartridge.sf004.is_none()
                    && cartridge.ssf2.is_none()
                    && cartridge.lock_on.is_none()
                    && cartridge.lock_on_save.is_none()
                    && cartridge.linear_rom.is_none()
                    && cartridge.parallel_adapter.is_none(),
                "MD extended SSF owner conflict"
            );
        }
        ensure!(
            cartridge.sf004.is_some() == crate::cartridge::sf004::Sf004::detect(&cartridge.bytes),
            "MD SF-004 identity mismatch"
        );
        if let Some(board) = &cartridge.sf004 {
            board.validate()?;
            ensure!(
                cartridge.save.is_none()
                    && cartridge.registers.is_none()
                    && cartridge.protection.is_none()
                    && cartridge.sf001.is_none()
                    && cartridge.sf002.is_none()
                    && cartridge.ssf2.is_none()
                    && cartridge.lock_on.is_none()
                    && cartridge.lock_on_save.is_none()
                    && cartridge.linear_rom.is_none()
                    && cartridge.parallel_adapter.is_none(),
                "MD SF-004 owner conflict"
            );
        }
        cartridge.validate_parallel_adapter()?;
        if let Some(owner) = cartridge.linear_rom {
            ensure!(
                owner.matches_digest(cartridge.rom_digest()),
                "MD bounded ROM identity mismatch"
            );
            ensure!(
                cartridge.save.is_none()
                    && cartridge.registers.is_none()
                    && cartridge.protection.is_none()
                    && cartridge.sf001.is_none()
                    && cartridge.sf002.is_none()
                    && cartridge.sf004.is_none()
                    && cartridge.ssf2.is_none()
                    && cartridge.lock_on.is_none()
                    && cartridge.lock_on_save.is_none(),
                "MD bounded ROM owner conflict"
            );
        }
        if cartridge.sf002.is_some() {
            ensure!(
                crate::cartridge::sf002::Sf002::detect(&cartridge.bytes),
                "MD SF-002 identity mismatch"
            );
            ensure!(
                cartridge
                    .save
                    .as_ref()
                    .is_some_and(crate::cartridge::SaveMemory::sf002_compatible)
                    && cartridge.sf001.is_none()
                    && cartridge.sf004.is_none()
                    && cartridge.ssf2.is_none()
                    && cartridge.registers.is_none()
                    && cartridge.protection.is_none()
                    && cartridge.lock_on.is_none()
                    && cartridge.lock_on_save.is_none(),
                "MD SF-002 owner or save conflict"
            );
        } else {
            ensure!(
                !crate::cartridge::sf002::Sf002::detect(&cartridge.bytes),
                "missing MD SF-002 owner"
            );
        }
        if let Some(board) = &cartridge.ssf2 {
            ensure!(
                board.matches_rom(&cartridge.bytes),
                "MD banked ROM identity mismatch"
            );
            ensure!(
                cartridge.save.is_none()
                    && cartridge.registers.is_none()
                    && cartridge.protection.is_none()
                    && cartridge.sf001.is_none()
                    && cartridge.sf002.is_none()
                    && cartridge.sf004.is_none()
                    && cartridge.lock_on.is_none()
                    && cartridge.lock_on_save.is_none(),
                "MD banked ROM owner conflict"
            );
        } else {
            ensure!(
                !crate::cartridge::ssf2::Ssf2::detect(&cartridge.bytes),
                "missing MD banked ROM owner"
            );
        }
        ensure!(
            self.bus.cartridge.lock_on_save.is_none()
                || (self.bus.cartridge.lock_on
                    == Some(crate::cartridge::registers::LockOn::TwoMiB)
                    && self.bus.cartridge.sf004.is_none()),
            "MD inserted save owner mismatch"
        );
        ensure!(
            self.bus.cartridge.lock_on.is_none()
                || (self.bus.cartridge.save.is_none()
                    && self.bus.cartridge.registers.is_none()
                    && self.bus.cartridge.protection.is_none()
                    && self.bus.cartridge.sf004.is_none()
                    && self.bus.cartridge.sf001.is_none()),
            "MD lock-on owner conflict"
        );
        ensure!(
            self.bus
                .cartridge
                .sf001
                .as_ref()
                .map(crate::cartridge::sf001::Sf001::revision)
                == crate::cartridge::sf001::Sf001::detect_revision(
                    &self.bus.cartridge.bytes,
                    self.bus.cartridge.header.as_deref()
                ),
            "MD SF-001 board mismatch"
        );
        ensure!(
            self.bus.cartridge.sf001.is_none()
                || (self.bus.cartridge.save.is_none() && self.bus.cartridge.sf004.is_none()),
            "MD SF-001 cannot use another save board"
        );
        ensure!(
            self.bus.cartridge.protection
                == crate::cartridge::protection::FixedProtection::detect(
                    self.bus.cartridge.bytes.len(),
                    self.bus.cartridge.header.as_deref()
                ),
            "MD fixed protection board mismatch"
        );
        let expected_registers = crate::cartridge::registers::CartridgeRegisters::detect_rom(
            &self.bus.cartridge.bytes,
            self.bus.cartridge.header.as_deref(),
            || self.bus.cartridge.rom_digest(),
        );
        ensure!(
            expected_registers.is_some() == self.bus.cartridge.registers.is_some(),
            "MD cartridge register board mismatch"
        );
        if let Some(board) = &self.bus.cartridge.registers {
            board.validate_profile(expected_registers.as_ref().unwrap())?;
        }
        self.sound_schedule
            .validate_cartridge(&self.bus.cartridge)?;
        if let Some(pending) = &self.sound_schedule.pending {
            pending.validate_download_source(
                &self.sound_cpu,
                &self.bus.sound,
                &self.bus.cartridge,
            )?;
        }
        if let Some(save) = &self.bus.cartridge.save {
            save.validate_time(self.bus.now.get(), self.region().master_hz())?;
        }
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
        if self.bus.owned.is_some() {
            ensure!(
                self.concurrent_sound
                    && self.sound_schedule == SoundSchedule::default()
                    && self.sound_cpu == zeff_z80::Cpu::new(),
                "physical machine has a second SOUND scheduler"
            );
        } else {
            self.sound_schedule
                .validate_state(&self.sound_cpu, self.bus.now.get())?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod tests;
#[cfg(test)]
mod video_tests;
