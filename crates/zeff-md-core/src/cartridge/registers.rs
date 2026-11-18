use super::CartridgeHeader;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RegisterLayout {
    #[default]
    Latches,
    OrBank32KiB,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CartridgeRegisters {
    layout: RegisterLayout,
    values: [u8; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LockOn {
    Empty,
    HalfMiB,
    MiB,
    TwoMiB,
    Patch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LockOnSave {
    pub enabled: bool,
    bytes: Box<[u8; 512]>,
}

impl LockOnSave {
    pub fn new() -> Self {
        Self {
            enabled: false,
            bytes: Box::new([0xff; 512]),
        }
    }

    pub fn contains(&self, address: u32) -> bool {
        (0x200000..=0x2003ff).contains(&address) || (0x300000..=0x3fffff).contains(&address)
    }

    fn index(address: u32) -> Option<usize> {
        (0x200001..=0x2003ff)
            .contains(&address)
            .then_some(((address & 0x3ff) >> 1) as usize)
            .filter(|_| address & 1 != 0)
    }

    pub fn read(&self, address: u32) -> Option<u8> {
        Self::index(address).map(|index| self.bytes[index])
    }

    pub fn write(&mut self, address: u32, value: u8) -> bool {
        let Some(index) = Self::index(address) else {
            return false;
        };
        self.bytes[index] = value;
        true
    }

    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    pub fn load(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() == self.bytes.len(),
            "MD inserted save length mismatch"
        );
        self.bytes.copy_from_slice(bytes);
        Ok(())
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(1);
        writer.write_bool(self.enabled);
        writer.write_vec(self.bytes());
    }

    pub fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        ensure!(reader.read_u8()? == 1, "MD inserted save profile mismatch");
        let enabled = reader.read_bool()?;
        self.load(reader.read_slice(512)?)?;
        self.enabled = enabled;
        Ok(())
    }
}

impl LockOn {
    const IDENTITIES: [(usize, [u8; 32], Self); 4] = [
        (
            0x200000,
            [
                0x6e, 0x12, 0xe6, 0xb3, 0x3c, 0x26, 0xeb, 0xfc, 0xd0, 0xbe, 0x43, 0x32, 0x51, 0xd2,
                0x1c, 0xf6, 0x28, 0x4e, 0xaf, 0xe9, 0xf7, 0x1b, 0x02, 0x7b, 0xda, 0x37, 0x67, 0xae,
                0x59, 0xaf, 0xfe, 0xc1,
            ],
            Self::Empty,
        ),
        (
            0x280000,
            [
                0x78, 0x7c, 0xf4, 0xc9, 0x30, 0x67, 0x0f, 0x8b, 0x3e, 0xed, 0x36, 0xc6, 0xa5, 0xa6,
                0x41, 0x02, 0x80, 0x24, 0x6f, 0x05, 0xe7, 0x8a, 0x15, 0xef, 0x54, 0xed, 0x3e, 0xda,
                0x86, 0xac, 0x9c, 0xa1,
            ],
            Self::HalfMiB,
        ),
        (
            0x340000,
            [
                0x3d, 0xa8, 0x27, 0xd3, 0x19, 0xa6, 0x0b, 0x61, 0x80, 0xe3, 0x27, 0xc3, 0x7e, 0x1f,
                0xe5, 0xd4, 0x48, 0x74, 0x6c, 0x16, 0x44, 0xcc, 0xe0, 0x1a, 0xda, 0x1f, 0x27, 0x88,
                0xd1, 0xe1, 0x82, 0xf4,
            ],
            Self::MiB,
        ),
        (
            0x400000,
            [
                0xfb, 0xa0, 0x67, 0x7f, 0xde, 0x9f, 0x76, 0xdf, 0x93, 0xf3, 0xe9, 0x8d, 0x63, 0x10,
                0xd8, 0xaf, 0x68, 0xb9, 0x84, 0x7b, 0xde, 0x16, 0xe2, 0x53, 0xd7, 0x3c, 0xd4, 0xdd,
                0x81, 0x34, 0xed, 0x23,
            ],
            Self::TwoMiB,
        ),
    ];

    pub fn detect(bytes: &[u8]) -> Option<Self> {
        use sha2::{Digest, Sha256};
        if !Self::IDENTITIES
            .iter()
            .any(|(size, _, _)| *size == bytes.len())
            || bytes.get(0x180..0x18e) != Some(b"GM MK-1563 -00")
        {
            return None;
        }
        Self::from_identity(bytes.len(), Sha256::digest(bytes).into())
    }

    fn from_identity(size: usize, digest: [u8; 32]) -> Option<Self> {
        Self::IDENTITIES
            .iter()
            .find(|(bytes, hash, _)| *bytes == size && *hash == digest)
            .map(|(_, _, mapping)| *mapping)
    }

    pub fn supports_rom_overlay(self) -> bool {
        matches!(self, Self::Empty | Self::HalfMiB | Self::MiB | Self::Patch)
    }

    pub fn cold_prefetch_valid(self, pc: u32, source_bytes: usize) -> bool {
        let mapping = if self == Self::Patch { Self::MiB } else { self };
        let address = pc & zeff_m68k::bus::BUS_ADDRESS_MASK;
        pc & 1 == 0
            && (0..4).all(|offset| {
                let address = address + offset;
                let source = if address < 0x200000 {
                    Some(address as usize)
                } else if address <= 0x3fffff {
                    mapping.source_offset(address)
                } else {
                    None
                };
                source.is_some_and(|source| source < source_bytes)
            })
    }

    pub fn read(self, address: u32, read_source: impl FnOnce(usize) -> u8) -> u8 {
        self.source_offset(address)
            .map_or(crate::constants::UNPOPULATED_ROM_BYTE, read_source)
    }

    pub fn source_offset(self, address: u32) -> Option<usize> {
        if self.patch_enabled() && address >= 0x300000 {
            return Some(0x300000 + (address as usize & 0x3ffff));
        }
        let mask = match self {
            Self::Empty => return None,
            Self::HalfMiB => 0x7ffff,
            Self::MiB | Self::Patch => 0xfffff,
            Self::TwoMiB => 0x1fffff,
        };
        // Normal mode hides any appended patch ROM behind the inserted ROM.
        Some(0x200000 + (address as usize & mask))
    }

    pub fn patch_enabled(self) -> bool {
        self == Self::Patch
    }

    pub fn mode_sensitive(self, address: u32) -> bool {
        matches!(self, Self::MiB | Self::Patch) && (0x300000..=0x3fffff).contains(&address)
    }

    pub fn write_control(&mut self, value: u8) -> bool {
        match value {
            0 => {
                if self.patch_enabled() {
                    *self = Self::MiB;
                }
            }
            1 if matches!(self, Self::MiB | Self::Patch) => *self = Self::Patch,
            _ => return false,
        }
        true
    }

    pub fn write_patch_state(self, writer: &mut StateWriter) {
        writer.write_u8(1);
        writer.write_bool(self.patch_enabled());
    }

    pub fn read_patch_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        ensure!(
            *self == Self::MiB && reader.read_u8()? == 1 && reader.read_bool()?,
            "MD lock-on patch state mismatch"
        );
        *self = Self::Patch;
        Ok(())
    }
}

impl CartridgeRegisters {
    pub fn detect_rom(
        rom: &[u8],
        header: Option<&CartridgeHeader>,
        digest: impl FnOnce() -> [u8; 32],
    ) -> Option<Self> {
        Self::detect(rom.len(), header).or_else(|| {
            let header = header?;
            if rom.len() != 0x100000
                || header.checksum != 0
                || header.calculated_checksum != 0x6ff8
                || rom.get(0x180..0x18e) != Some(&[0; 14])
                || digest()
                    != [
                        0x49, 0x78, 0x5e, 0xaa, 0x4f, 0x8d, 0x31, 0x83, 0xed, 0x56, 0xb1, 0xaa,
                        0xa3, 0xbc, 0x4b, 0xa8, 0xd7, 0xd7, 0x3c, 0xfb, 0x44, 0xa4, 0xf1, 0xfd,
                        0x79, 0xc0, 0x29, 0x84, 0xf4, 0x90, 0x56, 0x66,
                    ]
            {
                return None;
            }
            Some(Self {
                layout: RegisterLayout::OrBank32KiB,
                values: [0; 3],
            })
        })
    }

    pub fn is_banked(&self) -> bool {
        self.layout == RegisterLayout::OrBank32KiB
    }

    pub fn source_offset(&self, address: u32) -> Option<usize> {
        (self.is_banked() && address < 0x100000)
            .then(|| (address | (u32::from(self.values[2] & 0x3f) << 15)) as usize)
    }

    pub fn accepts_bank_cycle(&self, address: u32, lanes: zeff_m68k::bus::ByteLanes) -> bool {
        self.is_banked()
            && (0x700000..0x800000).contains(&address)
            && lanes == zeff_m68k::bus::ByteLanes::Lower
    }

    pub fn write_bank(&mut self, value: u8) {
        assert!(self.is_banked());
        self.values[2] = value;
    }

    pub fn reset_bank(&mut self) {
        if self.is_banked() {
            self.values = [0; 3];
        }
    }

    pub fn accepts_journal(&self, index: u8) -> bool {
        !self.is_banked() && index < 2
    }

    pub fn validate_profile(&self, expected: &Self) -> Result<()> {
        ensure!(
            self.layout == expected.layout,
            "MD cartridge register layout mismatch"
        );
        ensure!(
            if self.is_banked() {
                self.values[..2] == [0, 0]
            } else {
                self.values[2] == 0
            },
            "invalid MD cartridge register storage"
        );
        Ok(())
    }

    pub fn detect(size: usize, header: Option<&CartridgeHeader>) -> Option<Self> {
        let header = header?;
        matches!(
            (size, header.checksum, header.calculated_checksum),
            (0x200000, 0x30b9, 0x1c2a)
                | (0x200000, 0xffff, 0x5d98)
                | (0x200000, 0x6001, 0x0211)
                | (0x100000, 0x0000, 0x8ec8)
        )
        .then(Self::default)
    }

    pub fn contains(&self, address: u32) -> bool {
        if self.is_banked() {
            (0x600000..0x800000).contains(&address)
        } else {
            (0x400000..=0x40ffff).contains(&address)
        }
    }

    pub fn index(address: u32) -> Option<u8> {
        match address & 0xfffffd {
            0x400000 => Some(0),
            0x400004 => Some(1),
            _ => None,
        }
    }

    pub fn read(&self, address: u32) -> Option<u8> {
        (!self.is_banked())
            .then(|| Self::index(address))
            .flatten()
            .map(|index| self.values[usize::from(index)])
    }

    pub fn write(&mut self, index: u8, value: u8) {
        assert!(self.accepts_journal(index));
        self.values[usize::from(index)] = value;
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(if self.is_banked() { 2 } else { 1 });
        writer.write_bytes(&self.values[..if self.is_banked() { 3 } else { 2 }]);
    }

    pub fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        ensure!(
            reader.read_u8()? == if self.is_banked() { 2 } else { 1 },
            "MD cartridge register profile mismatch"
        );
        let mut values = [reader.read_u8()?, reader.read_u8()?, 0];
        if self.is_banked() {
            values[2] = reader.read_u8()?;
            ensure!(values[..2] == [0, 0], "invalid bank-only register state");
        }
        self.values = values;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
pub(crate) mod bank_tests;
