use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_m68k::bus::{BusAccess, ByteLanes};

const PRODUCT: &[u8; 14] = b"SF-002        ";
const SAVE_DECLARATION: &[u8; 12] = b"RA\xf8\x20\x00\x3c\x00\x01\x00\x3c\x3f\xff";
const STATE_TAG: &[u8; 4] = b"SF02";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mapping {
    #[default]
    Unknown,
    Linear,
    Mirrored,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Location {
    Rom(usize),
    Unclaimed,
    Unsupported(ReadError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadError {
    UnknownUpper,
    UnpopulatedRom { offset: usize },
    UnsupportedAddress { address: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreviewError {
    UnqualifiedWrite { index: usize },
    Read(ReadError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResetError {
    UnqualifiedWarmReset,
    UnqualifiedGuestResetOutput,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Sf002 {
    mapping: Mapping,
}

impl Sf002 {
    pub fn detect(rom: &[u8]) -> bool {
        Self::identity(
            rom.len(),
            rom.get(0x180..0x18e),
            rom.get(0x1b0..0x1bc),
            || Sha256::digest(rom).into(),
        )
    }

    fn identity(
        size: usize,
        product: Option<&[u8]>,
        declaration: Option<&[u8]>,
        digest: impl FnOnce() -> [u8; 32],
    ) -> bool {
        size == 0x22_0000
            && product == Some(PRODUCT.as_slice())
            && declaration == Some(SAVE_DECLARATION.as_slice())
            && digest()
                == [
                    0x17, 0x3a, 0x45, 0xad, 0xaa, 0xec, 0xcc, 0xd4, 0x42, 0xb7, 0xa7, 0xaf, 0xc5,
                    0xe5, 0x8e, 0x8e, 0x2b, 0x30, 0x11, 0xf3, 0xac, 0xc9, 0x44, 0x51, 0xe5, 0x9c,
                    0x17, 0x0b, 0x9f, 0xc0, 0x7b, 0x44,
                ]
    }

    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub fn mapping(&self) -> Mapping {
        self.mapping
    }

    pub fn decode(&self, address: u32) -> Location {
        Self::decode_at(self.mapping, address)
    }

    fn decode_at(mapping: Mapping, address: u32) -> Location {
        match address {
            0..=0x1fffff => Location::Rom(address as usize),
            0x200000..=0x3bffff => match mapping {
                Mapping::Unknown => Location::Unsupported(ReadError::UnknownUpper),
                Mapping::Linear if address < 0x220000 => Location::Rom(address as usize),
                Mapping::Linear => Location::Unsupported(ReadError::UnpopulatedRom {
                    offset: address as usize,
                }),
                Mapping::Mirrored => Location::Rom((address - 0x200000) as usize),
            },
            0x3c0000..=0x3c3fff => Location::Unclaimed,
            _ => Location::Unsupported(ReadError::UnsupportedAddress { address }),
        }
    }

    fn offset(location: Location) -> std::result::Result<Option<usize>, ReadError> {
        match location {
            Location::Rom(offset) => Ok(Some(offset)),
            Location::Unclaimed => Ok(None),
            Location::Unsupported(error) => Err(error),
        }
    }

    pub fn source_offset(&self, address: u32) -> std::result::Result<Option<usize>, ReadError> {
        Self::offset(self.decode(address))
    }

    fn control_mapping(mapping: Mapping, access: BusAccess, value: u16) -> Option<Mapping> {
        if access.logical_address != 0xe01
            || access.bus_address != 0xe00
            || access.lanes != ByteLanes::Lower
        {
            return None;
        }
        match value as u8 {
            0x00 => Some(Mapping::Linear),
            0x81 if mapping != Mapping::Unknown => Some(Mapping::Mirrored),
            _ => None,
        }
    }

    pub fn accepts_control(&self, access: BusAccess, value: u16) -> bool {
        Self::control_mapping(self.mapping, access, value).is_some()
    }

    pub fn control(&mut self, access: BusAccess, value: u16) -> bool {
        let Some(mapping) = Self::control_mapping(self.mapping, access, value) else {
            return false;
        };
        self.mapping = mapping;
        true
    }

    pub fn rejects_time_write(bus_address: u32) -> bool {
        (0xa13000..=0xa130ff).contains(&bus_address)
    }

    pub fn validate_warm_reset(&self) -> std::result::Result<(), ResetError> {
        Err(ResetError::UnqualifiedWarmReset)
    }

    pub fn validate_guest_reset_output(&self) -> std::result::Result<(), ResetError> {
        Err(ResetError::UnqualifiedGuestResetOutput)
    }

    pub fn source_offset_after_writes(
        &self,
        address: u32,
        writes: &[(BusAccess, u16)],
    ) -> std::result::Result<Option<usize>, PreviewError> {
        let mut mapping = self.mapping;
        for (index, &(access, value)) in writes.iter().enumerate() {
            mapping = Self::control_mapping(mapping, access, value)
                .ok_or(PreviewError::UnqualifiedWrite { index })?;
        }
        Self::offset(Self::decode_at(mapping, address)).map_err(PreviewError::Read)
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(STATE_TAG);
        writer.write_u8(match self.mapping {
            Mapping::Unknown => 0,
            Mapping::Linear => 1,
            Mapping::Mirrored => 2,
        });
    }

    pub fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        let mut tag = [0; 4];
        reader.read_exact(&mut tag)?;
        ensure!(&tag == STATE_TAG, "MD SF-002 state tag mismatch");
        let mapping = match reader.read_u8()? {
            0 => Mapping::Unknown,
            1 => Mapping::Linear,
            2 => Mapping::Mirrored,
            _ => anyhow::bail!("invalid MD SF-002 mapping"),
        };
        self.mapping = mapping;
        Ok(())
    }
}
