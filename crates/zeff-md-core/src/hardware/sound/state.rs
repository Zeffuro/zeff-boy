use super::*;
use anyhow::{Result, bail, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

impl SoundMemory {
    pub(crate) fn write_state(&self, w: &mut Writer) -> Result<()> {
        self.validate_state()?;
        w.write_bytes(&self.ram[..]);
        w.write_u16(self.bank);
        w.write_bool(self.requested);
        w.write_bool(self.granted);
        w.write_bool(self.reset_asserted);
        w.write_bool(self.irq_asserted);
        Ok(())
    }

    pub(crate) fn read_state(r: &mut Reader<'_>) -> Result<Self> {
        let mut ram = [0; SOUND_RAM_SIZE];
        r.read_exact(&mut ram)?;
        let memory = Self {
            ram: ram.into(),
            bank: r.read_u16()?,
            requested: r.read_bool()?,
            granted: r.read_bool()?,
            reset_asserted: r.read_bool()?,
            irq_asserted: r.read_bool()?,
        };
        memory.validate_state()?;
        Ok(memory)
    }

    pub(super) fn validate_state(&self) -> Result<()> {
        ensure!(
            self.bank < 1 << (SOUND_BANK_INPUT_SHIFT + 1),
            "invalid sound bank"
        );
        ensure!(
            !self.granted || (self.requested && !self.reset_asserted),
            "invalid sound bus grant"
        );
        Ok(())
    }
}

pub(super) fn write_direction(direction: Direction, w: &mut Writer) {
    w.write_u8(match direction {
        Direction::Read => 0,
        Direction::Write => 1,
    });
}

pub(super) fn read_direction(r: &mut Reader<'_>) -> Result<Direction> {
    match r.read_u8()? {
        0 => Ok(Direction::Read),
        1 => Ok(Direction::Write),
        _ => bail!("invalid sound access direction"),
    }
}

impl BankAccess {
    pub(super) fn write_state(&self, w: &mut Writer) {
        w.write_u32(self.address);
        write_direction(self.direction, w);
        w.write_u8(self.value);
    }

    pub(super) fn read_state(r: &mut Reader<'_>) -> Result<Self> {
        let access = Self {
            address: r.read_u32()?,
            direction: read_direction(r)?,
            value: r.read_u8()?,
        };
        ensure!(
            access.address <= BUS_ADDRESS_MASK,
            "invalid sound bank address"
        );
        Ok(access)
    }
}

impl WorkWrites {
    pub(super) fn write_state(&self, w: &mut Writer) {
        for slot in self.0 {
            w.write_bool(slot.is_some());
            if let Some((address, value)) = slot {
                w.write_u32(address as u32);
                w.write_u8(value);
            }
        }
    }

    pub(super) fn read_state(r: &mut Reader<'_>) -> Result<Self> {
        let mut writes = Self::default();
        for slot in &mut writes.0 {
            if r.read_bool()? {
                let address = r.read_u32()? as usize;
                ensure!(
                    address <= WORK_RAM_MASK,
                    "invalid sound work RAM journal index"
                );
                *slot = Some((address, r.read_u8()?));
            }
        }
        Ok(writes)
    }
}

impl PsgWrites {
    pub(super) fn write_state(&self, w: &mut Writer) {
        for slot in self.0 {
            w.write_bool(slot.is_some());
            if let Some(value) = slot {
                w.write_u8(value);
            }
        }
    }

    pub(super) fn read_state(r: &mut Reader<'_>) -> Result<Self> {
        let mut writes = Self::default();
        for slot in &mut writes.0 {
            if r.read_bool()? {
                *slot = Some(r.read_u8()?);
            }
        }
        Ok(writes)
    }
}

impl Opn2Writes {
    pub(super) fn write_state(&self, w: &mut Writer) {
        for slot in self.0 {
            w.write_bool(slot.is_some());
            if let Some((port, value)) = slot {
                w.write_u8(port);
                w.write_u8(value);
            }
        }
    }

    pub(super) fn read_state(r: &mut Reader<'_>) -> Result<Self> {
        let mut writes = Self::default();
        for slot in &mut writes.0 {
            if r.read_bool()? {
                let port = r.read_u8()?;
                ensure!(port < 4, "invalid sound OPN2 journal port");
                *slot = Some((port, r.read_u8()?));
            }
        }
        Ok(writes)
    }
}
