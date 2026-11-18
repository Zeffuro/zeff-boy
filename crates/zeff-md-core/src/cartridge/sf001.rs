use super::CartridgeHeader;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

const SAVE_BYTES: usize = 0x10000;
const PRODUCT: &[u8; 14] = b"SF-001\0\0\0\0\0\0\0\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Revision {
    HiddenSave,
    UpperSave,
}

impl Revision {
    fn tag(self) -> u8 {
        match self {
            Self::HiddenSave => 1,
            Self::UpperSave => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mapping {
    Linear,
    Switched,
    Disconnected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Location {
    Rom(u32),
    Sram(u16),
    Disconnected,
    Unclaimed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sf001 {
    revision: Revision,
    mapping: Mapping,
    locked: bool,
    ram: Box<[u8]>,
}

impl Sf001 {
    pub fn detect_revision(bytes: &[u8], header: Option<&CartridgeHeader>) -> Option<Revision> {
        let header = header?;
        if bytes.len() != 0x400000 || bytes.get(0x180..0x18e) != Some(PRODUCT.as_slice()) {
            return None;
        }
        match (header.checksum, header.calculated_checksum) {
            (0x3e08, 0x3e08) => Some(Revision::HiddenSave),
            (0x0836, 0x0836) => Some(Revision::UpperSave),
            _ => None,
        }
    }

    pub fn new(revision: Revision) -> Self {
        Self {
            revision,
            mapping: Mapping::Linear,
            locked: false,
            ram: vec![0xff; SAVE_BYTES].into_boxed_slice(),
        }
    }

    pub fn revision(&self) -> Revision {
        self.revision
    }

    pub fn mapping(&self) -> Mapping {
        self.mapping
    }

    pub fn locked(&self) -> bool {
        self.locked
    }

    pub fn decode(&self, address: u32) -> Location {
        self.decode_at(address, self.mapping)
    }

    fn decode_at(&self, address: u32, mapping: Mapping) -> Location {
        if self.revision == Revision::UpperSave && (0x400000..=0x40ffff).contains(&address) {
            return Location::Sram(address as u16);
        }
        if address > 0x3fffff {
            return Location::Unclaimed;
        }
        match mapping {
            Mapping::Disconnected => Location::Disconnected,
            Mapping::Switched if address < 0x40000 => Location::Rom(0x380000 + address),
            Mapping::Switched if self.revision == Revision::HiddenSave && address >= 0x3c0000 => {
                Location::Sram(address as u16)
            }
            _ => Location::Rom(address),
        }
    }

    pub(crate) fn unclaimed_expansion(&self, address: u32) -> bool {
        self.revision == Revision::HiddenSave
            && self.mapping == Mapping::Linear
            && !self.locked
            && self.decode(address) == Location::Unclaimed
    }

    pub fn control(&mut self, address: u32, value: u8) -> bool {
        Self::control_state(&mut self.mapping, &mut self.locked, address, value)
    }

    fn control_state(mapping: &mut Mapping, locked: &mut bool, address: u32, value: u8) -> bool {
        if address > 0xffff || address & 0xf00 != 0xe00 {
            return false;
        }
        if !*locked {
            *mapping = if value & 0x40 != 0 {
                Mapping::Disconnected
            } else if value & 0x80 != 0 {
                Mapping::Switched
            } else {
                Mapping::Linear
            };
            *locked = value & 0x20 != 0;
        }
        true
    }

    pub fn write(&mut self, address: u32, value: u8) -> bool {
        if self.control(address, value) || (0xa130f0..=0xa130ff).contains(&address) {
            return true;
        }
        match self.decode(address) {
            Location::Sram(cell) => self.ram[usize::from(cell)] = value,
            Location::Rom(_) | Location::Disconnected => (),
            Location::Unclaimed => return false,
        }
        true
    }

    pub fn read(&self, address: u32, source: impl FnOnce(usize) -> Option<u8>) -> Option<u8> {
        match self.decode(address) {
            Location::Rom(offset) => source(offset as usize),
            Location::Sram(cell) => Some(self.ram[usize::from(cell)]),
            Location::Disconnected | Location::Unclaimed => None,
        }
    }

    pub fn read_after_writes(
        &self,
        address: u32,
        source: impl FnOnce(usize) -> Option<u8>,
        writes: &[(u32, u8)],
    ) -> Option<u8> {
        let (mut mapping, mut locked) = (self.mapping, self.locked);
        for &(target, value) in writes {
            Self::control_state(&mut mapping, &mut locked, target, value);
        }
        match self.decode_at(address, mapping) {
            Location::Rom(offset) => source(offset as usize),
            Location::Sram(cell) => {
                let mut result = self.ram[usize::from(cell)];
                (mapping, locked) = (self.mapping, self.locked);
                for &(target, value) in writes {
                    if !Self::control_state(&mut mapping, &mut locked, target, value)
                        && self.decode_at(target, mapping) == Location::Sram(cell)
                    {
                        result = value;
                    }
                }
                Some(result)
            }
            Location::Disconnected | Location::Unclaimed => None,
        }
    }

    pub fn power_on(&mut self) {
        self.mapping = Mapping::Linear;
        self.locked = false;
    }

    pub fn save_bytes(&self) -> &[u8] {
        &self.ram
    }

    pub fn load_save(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() == SAVE_BYTES,
            "MD SF-001 save must contain 65536 bytes"
        );
        self.ram.copy_from_slice(bytes);
        Ok(())
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.revision.tag());
        writer.write_u8(match self.mapping() {
            Mapping::Linear => 0,
            Mapping::Switched => 1,
            Mapping::Disconnected => 2,
        });
        writer.write_bool(self.locked());
        writer.write_u32(SAVE_BYTES as u32);
        writer.write_bytes(&self.ram);
    }

    pub fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        ensure!(
            reader.read_u8()? == self.revision.tag(),
            "MD SF-001 revision mismatch"
        );
        let mapping = match reader.read_u8()? {
            0 => Mapping::Linear,
            1 => Mapping::Switched,
            2 => Mapping::Disconnected,
            _ => anyhow::bail!("invalid MD SF-001 mapping"),
        };
        let locked = reader.read_bool()?;
        ensure!(
            reader.read_u32()? == SAVE_BYTES as u32,
            "invalid MD SF-001 SRAM length"
        );
        let mut ram = vec![0; SAVE_BYTES].into_boxed_slice();
        reader.read_exact(&mut ram)?;
        self.mapping = mapping;
        self.locked = locked;
        self.ram = ram;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests;
