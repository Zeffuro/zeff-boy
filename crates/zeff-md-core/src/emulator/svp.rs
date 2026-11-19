use super::*;
use crate::svp::{Error, FunctionalProfile, InstructionModel, Progress, ServiceProgress};
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

impl Emulator {
    // Functional word 0/A levels at DSP instruction boundaries.
    pub fn new_synthetic_svp_with_control(
        rom: Vec<u8>,
        config: LoadConfig,
        irom: Vec<u16>,
    ) -> Result<Self> {
        let mut machine = Self::new_synthetic_svp(rom, config, irom)?;
        machine
            .bus
            .cartridge
            .svp
            .as_mut()
            .unwrap()
            .enable_boundary_control()?;
        Ok(machine)
    }

    // Explicit caller grants; this attachment does not select physical DSP clocks.
    pub fn new_synthetic_svp(rom: Vec<u8>, config: LoadConfig, irom: Vec<u16>) -> Result<Self> {
        ensure!(
            rom.len().is_multiple_of(2) && rom.len() <= 0x200000,
            "SVP ROM byte count"
        );
        let words = rom
            .as_chunks::<2>()
            .0
            .iter()
            .map(|word| u16::from_be_bytes(*word))
            .collect();
        let model = InstructionModel::new_synthetic(words, irom)?;
        let mut machine = Self::new(rom, config).map_err(|error| anyhow::anyhow!("{error:?}"))?;
        machine.validate_svp_cartridge()?;
        machine.bus.cartridge.svp = Some(Box::new(model));
        Ok(machine)
    }

    pub fn new_synthetic_svp_serviced(
        rom: Vec<u8>,
        config: LoadConfig,
        irom: Vec<u16>,
    ) -> Result<Self> {
        let mut machine = Self::new_synthetic_svp(rom, config, irom)?;
        machine
            .bus
            .cartridge
            .svp
            .as_mut()
            .unwrap()
            .enable_synthetic_services()?;
        Ok(machine)
    }

    pub fn new_functional_svp(
        rom: Vec<u8>,
        config: LoadConfig,
        irom: Vec<u16>,
        profile: FunctionalProfile,
    ) -> Result<Self> {
        let mut machine = Self::new_synthetic_svp_with_control(rom, config, irom)?;
        let model = machine.bus.cartridge.svp.as_mut().unwrap();
        model.enable_synthetic_services()?;
        model.enable_functional_clock(profile, machine.bus.now.get())?;
        Ok(machine)
    }

    pub fn advance_synthetic_svp_service(
        &mut self,
        granted: bool,
    ) -> Result<ServiceProgress, Error> {
        let cart = &mut self.bus.cartridge;
        let rom = crate::svp::CurrentRom::new(&cart.bytes, &cart.rom_overlay);
        let model = cart.svp.as_mut().ok_or(Error::Image("no SVP attachment"))?;
        if model.functional_timing().is_some() {
            return Err(Error::Image("functional clock owns service grants"));
        }
        model.advance_service_current(granted, Some(rom))
    }

    pub fn advance_synthetic_svp(&mut self, granted: bool) -> Result<Progress, Error> {
        self.bus
            .cartridge
            .svp
            .as_mut()
            .ok_or(Error::Image("no SVP attachment"))?
            .advance(granted)
    }

    pub fn synthetic_svp(&self) -> Option<&InstructionModel> {
        self.bus.cartridge.svp.as_deref()
    }

    fn validate_svp_cartridge(&self) -> Result<()> {
        let cart = &self.bus.cartridge;
        ensure!(
            self.bus.owned.is_none() && self.bus.tmss.is_none(),
            "unqualified SVP timing or boot owner"
        );
        ensure!(
            cart.save.is_none()
                && cart.save_latch.is_none()
                && cart.download.is_none()
                && cart.linear_rom.is_none()
                && cart.unprotected_rom.is_none()
                && cart.parallel_adapter.is_none()
                && cart.registers.is_none()
                && cart.protection.is_none()
                && cart.sf001.is_none()
                && cart.sf002.is_none()
                && cart.sf004.is_none()
                && cart.extended_ssf.is_none()
                && cart.realtec.is_none()
                && cart.ssf2.is_none()
                && cart.lock_on.is_none()
                && cart.lock_on_save.is_none(),
            "unqualified SVP cartridge composition"
        );
        Ok(())
    }

    pub(super) fn validate_svp_state(&self) -> Result<()> {
        if self.bus.cartridge.svp.is_some() {
            self.validate_svp_cartridge()?;
            self.sound_schedule.validate_svp_history(
                &self.sound_cpu,
                &self.bus.cartridge,
                self.bus.now.get(),
            )?;
            if let Some(timing) = self.synthetic_svp().unwrap().functional_timing() {
                ensure!(
                    timing.master_tick == self.bus.now.get(),
                    "functional SVP/master time mismatch"
                );
            }
        }
        Ok(())
    }

    pub(super) fn write_svp_state(&self, writer: &mut StateWriter) -> Result<()> {
        if let Some(svp) = &self.bus.cartridge.svp {
            let historical = svp.has_rom_history() || self.sound_schedule.has_svp_history();
            writer.write_bytes(if historical { b"ZMDSVPA2" } else { b"ZMDSVPS1" });
            writer.write_vec(&svp.encode_state()?);
            if historical {
                writer.write_vec(&self.sound_schedule.write_svp_history(&self.sound_cpu)?);
            }
        }
        Ok(())
    }

    pub(super) fn read_svp_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        if let Some(svp) = &mut self.bus.cartridge.svp {
            let mut tag = [0; 8];
            reader.read_exact(&mut tag)?;
            match &tag {
                b"ZMDSVPS1" => {
                    let bytes = reader.read_slice(512 * 1024)?;
                    ensure!(
                        bytes.starts_with(b"ZSINST01"),
                        "legacy SVP component schema"
                    );
                    svp.decode_state(bytes)?;
                }
                b"ZMDSVPA2" => {
                    svp.decode_state(reader.read_slice(512 * 1024)?)?;
                    self.sound_schedule
                        .read_svp_history(&self.sound_cpu, reader.read_slice(32 * 1024)?)?;
                    ensure!(
                        svp.has_rom_history() || self.sound_schedule.has_svp_history(),
                        "redundant historical SVP attachment"
                    );
                }
                _ => anyhow::bail!("missing SVP save-state owner"),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
