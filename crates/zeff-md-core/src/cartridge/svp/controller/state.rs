use super::{Channel, Controller, Dram, Programming};
use std::sync::Arc;
use zeff_emu_common::save_state::{StateReader, StateWriter};

impl Controller {
    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_u8(match self.programming {
            Programming::Address => 0,
            Programming::Mode(_) => 1,
            Programming::Ready { .. } => 2,
        });
        match self.programming {
            Programming::Address => {}
            Programming::Mode(address) => w.write_u16(address),
            Programming::Ready { address, mode } => {
                w.write_u16(address);
                w.write_u16(mode);
            }
        }
        for direction in self.channels {
            for channel in direction {
                w.write_bool(channel.is_some());
                if let Some(channel) = channel {
                    w.write_u32(channel.address);
                    w.write_u16(channel.mode);
                }
            }
        }
        w.write_u16(self.xst);
        w.write_u16(self.status);
        w.write_bool(self.cold_blind);
        w.write_u16(self.inputs);
        for &word in self.iram.iter() {
            w.write_u16(word);
        }
        self.dram.write_state(w);
        w.write_bytes(b"PMCS");
        w.write_bool(self.pmc_shadow.is_some());
        let shadow = self.pmc_shadow.unwrap_or(Channel {
            address: 0,
            mode: 0,
        });
        w.write_u16(shadow.address as u16);
        w.write_u16((shadow.address >> 16) as u16 | shadow.mode);
    }

    pub fn read_state(r: &mut StateReader<'_>, rom: Arc<[u16]>) -> anyhow::Result<Self> {
        let mut value = Self::with_rom(rom);
        value.programming = match r.read_u8()? {
            0 => Programming::Address,
            1 => Programming::Mode(r.read_u16()?),
            2 => {
                let address = r.read_u16()?;
                let mode = r.read_u16()?;
                Channel::new(address, mode, false)?;
                Programming::Ready { address, mode }
            }
            _ => anyhow::bail!("invalid saved PMC phase"),
        };
        for (writing, direction) in value.channels.iter_mut().enumerate() {
            for channel in direction {
                *channel = if r.read_bool()? {
                    let address = r.read_u32()?;
                    anyhow::ensure!(address <= 0x1fffff, "saved channel address");
                    let mode = r.read_u16()?;
                    anyhow::ensure!(Channel::controls_valid(mode), "saved channel mode");
                    anyhow::ensure!(writing != 0 || mode & 0x4400 == 0, "saved DRAM read mode");
                    Some(Channel { address, mode })
                } else {
                    None
                };
            }
        }
        value.xst = r.read_u16()?;
        value.status = r.read_u16()?;
        value.cold_blind = r.read_bool()?;
        value.inputs = r.read_u16()?;
        anyhow::ensure!(value.inputs & !0x0c00 == 0, "invalid synthetic input pins");
        for word in Arc::make_mut(&mut value.iram) {
            *word = r.read_u16()?;
        }
        value.dram = Dram::read_state(r)?;
        let mut tag = [0; 4];
        r.read_exact(&mut tag)?;
        anyhow::ensure!(&tag == b"PMCS", "missing PMC shadow state");
        let known = r.read_bool()?;
        let address = r.read_u16()?;
        let mode = r.read_u16()?;
        anyhow::ensure!(
            Channel::controls_valid(mode & !0x1f) && (known || address == 0 && mode == 0),
            "invalid PMC shadow state"
        );
        value.pmc_shadow = known.then_some(Channel {
            address: u32::from(address) | (u32::from(mode & 0x1f) << 16),
            mode: mode & 0xfc00,
        });
        anyhow::ensure!(
            known || value.channels.iter().flatten().all(Option::is_none),
            "programmed channel without PMC shadow"
        );
        match value.programming {
            Programming::Mode(address) => anyhow::ensure!(
                value
                    .pmc_shadow
                    .is_none_or(|shadow| shadow.address as u16 == address),
                "PMC address phase disagrees with shadow"
            ),
            Programming::Ready { address, mode } => anyhow::ensure!(
                value.pmc_shadow == Some(Channel::new(address, mode, false)?),
                "PMC programming disagrees with shadow"
            ),
            Programming::Address => {}
        }
        if value.cold_blind {
            let mut expected = Self::with_rom(value.rom.clone());
            expected.inputs = value.inputs;
            anyhow::ensure!(value == expected, "noncanonical cold controller");
        }
        Ok(value)
    }
}
