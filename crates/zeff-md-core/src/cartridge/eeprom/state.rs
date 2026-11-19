use super::*;

impl Eeprom {
    pub(crate) fn validate_time(&self, now: u64, master_hz: u64) -> Result<()> {
        ensure!(
            self.busy_until
                <= now.saturating_add(
                    master_hz
                        .div_ceil(WRITE_CYCLES_PER_SECOND)
                        .saturating_mul(if self.cached_address() { 8 } else { 1 })
                ),
            "invalid MD EEPROM programming deadline"
        );
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.address <= self.address_mask(),
            "invalid MD EEPROM address"
        );
        ensure!(
            self.page_base <= self.address_mask() && self.page_base & self.page_mask() == 0,
            "invalid MD EEPROM page"
        );
        ensure!(
            self.page_len() == 64 || self.dirty < (1_u64 << self.page_len()),
            "invalid MD EEPROM write mask"
        );
        ensure!(
            self.page[self.page_len()..].iter().all(|byte| *byte == 0),
            "invalid MD EEPROM unused page bytes"
        );
        let separate_phase = matches!(
            self.phase,
            Phase::Device
                | Phase::DeviceAck
                | Phase::WordAddress
                | Phase::WordAddressAck
                | Phase::WordAddressHigh
                | Phase::WordAddressHighAck
        );
        ensure!(
            !separate_phase || self.separate_address(),
            "invalid MD EEPROM address phase"
        );
        ensure!(
            !self.separate_address() || !matches!(self.phase, Phase::Address | Phase::AddressAck),
            "invalid MD EEPROM chip phase"
        );
        ensure!(
            !matches!(
                self.phase,
                Phase::WordAddressHigh | Phase::WordAddressHighAck
            ) || self.cached_address(),
            "invalid MD EEPROM high address phase"
        );
        let valid = match self.phase {
            Phase::Idle | Phase::WaitStop => self.bits == 0 && self.released && self.dirty == 0,
            Phase::Address | Phase::Device => self.bits < 8 && self.released && self.dirty == 0,
            Phase::WordAddress | Phase::WordAddressHigh => {
                self.bits < 8 && self.released && !self.reading && self.dirty == 0
            }
            Phase::Write => {
                self.bits < 8 && self.released && !self.reading && self.write_cursor_valid()
            }
            Phase::AddressAck
            | Phase::WriteAck
            | Phase::DeviceAck
            | Phase::WordAddressAck
            | Phase::WordAddressHighAck => {
                (8..=9).contains(&self.bits)
                    && (self.bits != 9 || self.scl)
                    && self.released
                        == (if self.bits == 8 && self.scl {
                            true
                        } else {
                            !self.ack
                        })
                    && if self.phase == Phase::AddressAck {
                        self.dirty == 0
                            && self.address == u16::from(self.shift >> 1)
                            && self.reading == (self.shift & 1 != 0)
                    } else if self.phase == Phase::DeviceAck {
                        self.dirty == 0
                            && self.reading == (self.shift & 1 != 0)
                            && (!self.ack || self.accepts_device(self.shift))
                            && (!self.ack
                                || !self.banked_address()
                                || self.address & self.bank_mask() == self.device_bank(self.shift))
                    } else if self.phase == Phase::WordAddressHighAck {
                        self.dirty == 0
                            && !self.reading
                            && self.ack == (self.shift & 0xe0 == 0)
                            && (!self.ack || self.address >> 8 == u16::from(self.shift))
                    } else if self.phase == Phase::WordAddressAck {
                        self.dirty == 0
                            && !self.reading
                            && self.ack
                            && self.address & 0xff == u16::from(self.shift)
                            && self.page_base == self.address & !self.page_mask()
                    } else {
                        !self.reading
                            && self.ack
                            && self.write_cursor_valid()
                            && self.dirty
                                & (1_u64
                                    << if self.cached_address() {
                                        self.write_offset().wrapping_sub(1) & 63
                                    } else {
                                        self.address.wrapping_sub(1) & self.page_mask()
                                    })
                                != 0
                    }
            }
            Phase::Read => {
                self.bits <= 8
                    && self.dirty == 0
                    && self.reading
                    && (self.bits != 0 || !self.scl)
                    && (self.bits != 8 || self.scl)
            }
            Phase::ReadAck => {
                self.bits <= 1
                    && self.dirty == 0
                    && self.released
                    && self.reading
                    && (self.bits != 1 || self.scl)
            }
        };
        ensure!(valid, "invalid MD EEPROM serial phase");
        Ok(())
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.scl);
        writer.write_bool(self.sda);
        writer.write_bool(self.released);
        writer.write_u8(self.phase as u8);
        writer.write_u8(self.bits);
        writer.write_u8(self.shift);
        if self.wide_address() {
            writer.write_u16(self.address);
        } else {
            writer.write_u8(self.address as u8);
        }
        writer.write_bool(self.reading);
        writer.write_bool(self.ack);
        writer.write_bytes(&self.page[..self.page_len()]);
        if self.wide_address() {
            writer.write_u16(self.page_base);
            if self.cached_address() {
                writer.write_u64(self.dirty);
            } else {
                writer.write_u16(self.dirty as u16);
            }
        } else {
            writer.write_u8(self.page_base as u8);
            writer.write_u8(self.dirty as u8);
        }
        writer.write_u64(self.busy_until);
    }

    pub(crate) fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        let mut candidate = self.clone();
        candidate.scl = reader.read_bool()?;
        candidate.sda = reader.read_bool()?;
        candidate.released = reader.read_bool()?;
        candidate.phase = match reader.read_u8()? {
            0 => Phase::Idle,
            1 => Phase::Address,
            2 => Phase::AddressAck,
            3 => Phase::Write,
            4 => Phase::WriteAck,
            5 => Phase::Read,
            6 => Phase::ReadAck,
            7 => Phase::WaitStop,
            8 => Phase::Device,
            9 => Phase::DeviceAck,
            10 => Phase::WordAddress,
            11 => Phase::WordAddressAck,
            12 if candidate.cached_address() => Phase::WordAddressHigh,
            13 if candidate.cached_address() => Phase::WordAddressHighAck,
            _ => anyhow::bail!("invalid MD EEPROM phase tag"),
        };
        candidate.bits = reader.read_u8()?;
        candidate.shift = reader.read_u8()?;
        candidate.address = if candidate.wide_address() {
            reader.read_u16()?
        } else {
            u16::from(reader.read_u8()?)
        };
        candidate.reading = reader.read_bool()?;
        candidate.ack = reader.read_bool()?;
        let length = candidate.page_len();
        reader.read_exact(&mut candidate.page[..length])?;
        if candidate.wide_address() {
            candidate.page_base = reader.read_u16()?;
            candidate.dirty = if candidate.cached_address() {
                reader.read_u64()?
            } else {
                u64::from(reader.read_u16()?)
            };
        } else {
            candidate.page_base = u16::from(reader.read_u8()?);
            candidate.dirty = u64::from(reader.read_u8()?);
        }
        candidate.busy_until = reader.read_u64()?;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
}
