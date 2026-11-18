use super::*;
use crate::hardware::tmss::Tmss;
use crate::save_state::{read_section, write_section};
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

impl Emulator {
    pub fn new_with_tmss(rom: Vec<u8>, config: LoadConfig, boot_rom: Vec<u8>) -> Result<Self> {
        let cartridge = Cartridge::new(rom).map_err(|error| anyhow::anyhow!("{error:?}"))?;
        ensure!(
            cartridge.parallel_adapter.is_none()
                && cartridge.sf001.is_none()
                && cartridge.sf002.is_none()
                && cartridge.sf004.is_none()
                && cartridge.download.is_none()
                && cartridge.realtec.is_none()
                && cartridge.ssf2.is_none()
                && cartridge.extended_ssf.is_none()
                && cartridge.registers.is_none()
                && cartridge.protection.is_none()
                && cartridge.save.is_none()
                && cartridge.save_latch.is_none()
                && cartridge.lock_on.is_none()
                && cartridge.lock_on_save.is_none()
                && cartridge.linear_rom.is_none(),
            "TMSS cartridge hardware is not qualified"
        );
        let mut bus = Bus::new(cartridge, config.region);
        bus.tmss = Some(Tmss::new(boot_rom)?);
        let mut machine = Self {
            cpu: PrefetchCpu::default(),
            bus,
            sound_cpu: zeff_z80::Cpu::new(),
            main_stopped: false,
            concurrent_sound: config.concurrent_sound,
            sound_schedule: SoundSchedule::default(),
            call_stack: call_stack::Tracker::default(),
        };
        machine
            .restart_cpu()
            .map_err(|error| anyhow::anyhow!("TMSS startup: {error:?}"))?;
        Ok(machine)
    }

    pub(super) fn write_tmss_state(&self, writer: &mut StateWriter) -> Result<()> {
        if let Some(tmss) = &self.bus.tmss {
            writer.write_bytes(b"ZMDTMSS1");
            write_section(writer, |writer| {
                tmss.write_state(writer);
                Ok(())
            })?;
        }
        Ok(())
    }

    pub(super) fn read_tmss_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        if let Some(tmss) = &mut self.bus.tmss {
            let mut tag = [0; 8];
            reader.read_exact(&mut tag)?;
            ensure!(&tag == b"ZMDTMSS1", "missing TMSS save-state owner");
            read_section(reader, |reader| tmss.read_state(reader))?;
        }
        Ok(())
    }
}
