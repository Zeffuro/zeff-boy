use super::LoadError;
use super::eeprom::{ChipProfile, Eeprom};
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

mod bounded_bytes;
mod bounded_odd;
mod bounded_switched;
mod expansion_bytes;
mod headerless_odd8;
mod latched;
mod mapping;
mod reserved_switched;
mod state;
#[cfg(test)]
pub(crate) use expansion_bytes::fixture_rom as expansion_save_fixture;
#[cfg(test)]
pub(crate) use headerless_odd8::fixture as headerless_odd8_fixture;

pub(crate) const SAVE_START: u32 = 0x20_0000;
pub(crate) const SAVE_CONTROL: u32 = 0xa1_30f0;
pub(crate) const SAVE_CONTROL_END: u32 = SAVE_CONTROL + 0x0e;
const DECLARATION_PREFIX: [u8; 4] = [b'R', b'A', 0xf8, 0x20];
const HEADER_START: usize = 0x1b0;
const SWITCHED_ROM_BYTES: usize = 0x30_0000;
const SWITCHED_SAVE_END: u32 = 0x40_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OddSaveLayout {
    ExpansionBytesKiB64,
    BoundedSwitchedBytes2048,
    BoundedOdd2400,
    BoundedSwitchedOddKiB8,
    ReservedSwitchedRom,
    Serial128,
    Serial256,
    SplitSerial256,
    SplitSerial2048,
    Bytes512,
    KiB8,
    FixedOddKiB8,
    KiB32,
    BoundedSplitKiB32,
    SplitSerial8192,
    Parallel2048,
    BoundedParallel2048,
    SplitSerial512,
    HeaderlessOddKiB8,
}

impl OddSaveLayout {
    fn bytes(self) -> usize {
        match self {
            Self::ExpansionBytesKiB64 => 65536,
            Self::ReservedSwitchedRom => 0,
            Self::BoundedOdd2400 => 2400,
            Self::Serial128 => 128,
            Self::Serial256 | Self::SplitSerial256 => 256,
            Self::SplitSerial2048
            | Self::Parallel2048
            | Self::BoundedParallel2048
            | Self::BoundedSwitchedBytes2048 => 2048,
            Self::Bytes512 | Self::SplitSerial512 => 512,
            Self::KiB8
            | Self::FixedOddKiB8
            | Self::HeaderlessOddKiB8
            | Self::SplitSerial8192
            | Self::BoundedSwitchedOddKiB8 => 8192,
            Self::KiB32 | Self::BoundedSplitKiB32 => 32768,
        }
    }

    fn split_serial(self) -> bool {
        matches!(
            self,
            Self::SplitSerial256
                | Self::SplitSerial512
                | Self::SplitSerial2048
                | Self::SplitSerial8192
        )
    }

    fn chip_profile(self) -> Option<ChipProfile> {
        match self {
            Self::Serial128 => Some(ChipProfile::Serial128),
            Self::Serial256 => Some(ChipProfile::Serial256),
            Self::SplitSerial256 => Some(ChipProfile::Split256),
            Self::SplitSerial512 => Some(ChipProfile::Split512),
            Self::SplitSerial2048 => Some(ChipProfile::Split2048),
            Self::SplitSerial8192 => Some(ChipProfile::Split8192),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SaveMemory {
    layout: OddSaveLayout,
    start: u32,
    switched: bool,
    initially_enabled: bool,
    time_latch: bool,
    pub enabled: bool,
    pub write_protected: bool,
    pub bytes: Box<[u8]>,
    serial: Option<Eeprom>,
}

impl SaveMemory {
    pub fn from_rom(rom: &[u8]) -> Result<Option<Self>, LoadError> {
        if expansion_bytes::detect(rom) {
            return Ok(Some(Self::new_expansion_bytes()));
        }
        if bounded_bytes::detect(rom) {
            return Ok(Some(Self::new_bounded_bytes()));
        }
        if reserved_switched::detect(rom) {
            return Ok(Some(Self::new_reserved_switched()));
        }
        if bounded_switched::detect(rom) {
            return Ok(Some(Self::new_bounded_switched()));
        }
        if bounded_odd::detect(rom) {
            return Ok(Some(Self::new_bounded_odd()));
        }
        if bounded_split_profile(rom) {
            return Ok(Some(Self {
                layout: OddSaveLayout::BoundedSplitKiB32,
                start: SAVE_START,
                switched: false,
                initially_enabled: true,
                time_latch: false,
                enabled: true,
                write_protected: false,
                bytes: vec![0xff; 32768].into_boxed_slice(),
                serial: None,
            }));
        }
        if let Some(layout) = split_serial_profile(rom) {
            return Ok(Some(Self {
                layout,
                start: SAVE_START,
                switched: true,
                initially_enabled: false,
                time_latch: false,
                enabled: false,
                write_protected: false,
                bytes: vec![0xff; layout.bytes()].into_boxed_slice(),
                serial: Some(match layout {
                    OddSaveLayout::SplitSerial512 => Eeprom::new_split_512(),
                    OddSaveLayout::SplitSerial2048 => Eeprom::new_split_2048(),
                    OddSaveLayout::SplitSerial8192 => Eeprom::new_split_8192(),
                    _ => Eeprom::new_split_lines(),
                }),
            }));
        }
        if let Some(separate_address) = Eeprom::profile(rom) {
            let layout = if separate_address {
                OddSaveLayout::Serial256
            } else {
                OddSaveLayout::Serial128
            };
            return Ok(Some(Self {
                layout,
                start: SAVE_START,
                switched: false,
                initially_enabled: true,
                time_latch: false,
                enabled: true,
                write_protected: false,
                bytes: vec![0xff; layout.bytes()].into_boxed_slice(),
                serial: Some(Eeprom::new(separate_address)),
            }));
        }
        if internal_ram_declaration_profile(rom) {
            return Ok(None);
        }
        let headerless = headerless_odd32_profile(rom);
        let fixed_parallel = fixed_parallel_profile(rom);
        if !headerless
            && fixed_parallel.is_none()
            && rom.get(HEADER_START..HEADER_START + 2) != Some(b"RA")
        {
            return Ok(None);
        }
        let (layout, start) = if headerless {
            (OddSaveLayout::KiB32, SAVE_START)
        } else if let Some(layout) = fixed_parallel {
            (layout, SAVE_START)
        } else if bounded_parallel_2048_profile(rom) {
            (OddSaveLayout::BoundedParallel2048, SAVE_START)
        } else if parallel_2048_profile(rom) {
            (OddSaveLayout::Parallel2048, SAVE_START)
        } else if corrected_odd8_profile(rom) || header_odd8_profile(rom) {
            (OddSaveLayout::KiB8, SAVE_START)
        } else {
            odd_geometry(rom).ok_or(LoadError::UnsupportedSaveHardware)?
        };
        let switched = rom.len() > start as usize;
        if switched && (start != SAVE_START || !switching_profile(rom, layout)) {
            return Err(LoadError::UnsupportedSaveHardware);
        }
        let initially_enabled = !switched
            && (layout != OddSaveLayout::Bytes512
                || initially_mapped_odd512(rom)
                || fixed_parallel.is_some());
        Ok(Some(Self {
            layout,
            start,
            switched,
            initially_enabled,
            time_latch: switched && layout == OddSaveLayout::KiB8 && time_latch_profile(rom),
            enabled: initially_enabled,
            write_protected: false,
            bytes: vec![0xff; layout.bytes()].into_boxed_slice(),
            serial: None,
        }))
    }

    pub(crate) fn sf002_compatible(&self) -> bool {
        self.layout == OddSaveLayout::KiB8
            && self.start == 0x3c0000
            && !self.switched
            && self.initially_enabled
            && self.enabled
            && !self.write_protected
            && !self.time_latch
            && self.serial.is_none()
            && self.bytes.len() == 8192
    }

    pub fn contains(&self, address: u32) -> bool {
        if self.expansion_bytes() {
            return expansion_bytes::domain(address);
        }
        if self.partial_switched() {
            return bounded_switched::domain(address);
        }
        if self.bounded_odd() {
            return bounded_odd::domain(address);
        }
        let end = if self.layout.split_serial() {
            SWITCHED_ROM_BYTES as u32
        } else if self.layout == OddSaveLayout::Parallel2048
            || self.switched
            || self.serial.is_some()
        {
            SWITCHED_SAVE_END
        } else {
            self.start + self.layout.bytes() as u32 * 2
        };
        (self.start..end).contains(&address)
    }

    pub(crate) fn bounded_split(&self) -> bool {
        self.layout == OddSaveLayout::BoundedSplitKiB32
    }

    pub fn banked_readable(&self) -> bool {
        !self.expansion_bytes()
            && !self.partial_switched()
            && (self.bounded_split()
                || self.switched && (self.serial.is_none() || self.rom_visible())
                || self.start != SAVE_START && self.serial.is_none())
    }

    pub fn validate_state(&self) -> Result<()> {
        self.validate_state_inner(false)
    }

    fn validate_state_inner(&self, fixed_latch: bool) -> Result<()> {
        if self.expansion_bytes() {
            self.validate_expansion_bytes()?;
        }
        if self.layout == OddSaveLayout::HeaderlessOddKiB8 {
            self.validate_fixed_parallel(OddSaveLayout::HeaderlessOddKiB8)?;
        }
        if self.bounded_bytes() {
            self.validate_bounded_bytes()?;
        }
        if self.reserved_switched() {
            self.validate_reserved_switched()?;
        }
        if self.bounded_switched() {
            self.validate_bounded_switched()?;
        }
        if self.bounded_odd() {
            self.validate_fixed_parallel(OddSaveLayout::BoundedOdd2400)?;
        }
        if self.bounded_split() {
            self.validate_fixed_parallel(OddSaveLayout::BoundedSplitKiB32)?;
        }
        ensure!(
            !self.write_protected || self.enabled && self.protectable(),
            "invalid MD save RAM protection"
        );
        ensure!(
            self.serial.is_some()
                == matches!(
                    self.layout,
                    OddSaveLayout::Serial128
                        | OddSaveLayout::Serial256
                        | OddSaveLayout::SplitSerial256
                        | OddSaveLayout::SplitSerial512
                        | OddSaveLayout::SplitSerial2048
                        | OddSaveLayout::SplitSerial8192
                ),
            "invalid MD save hardware"
        );
        if let Some(serial) = &self.serial {
            ensure!(
                if serial.split_lines() {
                    self.switched
                } else {
                    self.enabled && !self.switched
                },
                "invalid MD EEPROM mapping"
            );
            ensure!(
                serial.split_lines() == self.layout.split_serial(),
                "invalid MD EEPROM wire profile"
            );
            ensure!(
                serial.separate_address()
                    == matches!(
                        self.layout,
                        OddSaveLayout::Serial256
                            | OddSaveLayout::SplitSerial256
                            | OddSaveLayout::SplitSerial512
                            | OddSaveLayout::SplitSerial2048
                            | OddSaveLayout::SplitSerial8192
                    ),
                "invalid MD EEPROM profile"
            );
            serial.validate()?;
            ensure!(
                Some(serial.chip_profile()) == self.layout.chip_profile(),
                "invalid MD EEPROM chip profile"
            );
        }
        ensure!(
            self.bytes.len() == self.layout.bytes(),
            "invalid MD save RAM size"
        );
        ensure!(
            self.switched
                || self.enabled
                || self.layout == OddSaveLayout::Bytes512
                || fixed_latch && self.fixed_latched_odd32(),
            "invalid MD save RAM mapping"
        );
        Ok(())
    }

    pub fn validate_time(&self, now: u64, master_hz: u64) -> Result<()> {
        if let Some(serial) = &self.serial {
            serial.validate_time(now, master_hz)?;
        }
        Ok(())
    }

    pub fn validate_rom_profile(&self, rom: &[u8]) -> Result<()> {
        ensure!(
            self.expansion_bytes() == expansion_bytes::detect(rom),
            "MD expansion save profile differs from ROM"
        );
        if self.expansion_bytes() {
            return self.validate_expansion_bytes();
        }
        ensure!(
            self.bounded_bytes() == bounded_bytes::detect(rom),
            "MD bounded byte save profile differs from ROM"
        );
        if self.bounded_bytes() {
            self.validate_bounded_bytes()?;
        }
        ensure!(
            self.reserved_switched() == reserved_switched::detect(rom),
            "MD reserved switched save profile differs from ROM"
        );
        if self.reserved_switched() {
            self.validate_reserved_switched()?;
        }
        ensure!(
            self.bounded_switched() == bounded_switched::detect(rom),
            "MD bounded switched save profile differs from ROM"
        );
        if self.bounded_switched() {
            self.validate_bounded_switched()?;
        }
        ensure!(
            self.bounded_odd() == bounded_odd::detect(rom),
            "MD bounded odd save profile differs from ROM"
        );
        if self.bounded_odd() {
            self.validate_fixed_parallel(OddSaveLayout::BoundedOdd2400)?;
        }
        let bounded_split = bounded_split_profile(rom);
        ensure!(
            self.bounded_split() == bounded_split,
            "MD split save profile differs from ROM"
        );
        if bounded_split {
            self.validate_fixed_parallel(OddSaveLayout::BoundedSplitKiB32)?;
        }
        let fixed_parallel = fixed_parallel_profile(rom);
        ensure!(
            (self.layout == OddSaveLayout::HeaderlessOddKiB8)
                == (fixed_parallel == Some(OddSaveLayout::HeaderlessOddKiB8)),
            "MD headerless fixed save profile differs from ROM"
        );
        if let Some(layout) = fixed_parallel {
            self.validate_fixed_parallel(layout)?;
        }
        let expected = split_serial_profile(rom).or_else(|| {
            Eeprom::profile(rom).map(|separate| {
                if separate {
                    OddSaveLayout::Serial256
                } else {
                    OddSaveLayout::Serial128
                }
            })
        });
        let expected_start = if bounded_split
            || expected.is_some()
            || fixed_parallel.is_some()
            || bounded_parallel_2048_profile(rom)
            || parallel_2048_profile(rom)
            || corrected_odd8_profile(rom)
            || header_odd8_profile(rom)
        {
            SAVE_START
        } else {
            odd_geometry(rom).map_or(SAVE_START, |(_, start)| start)
        };
        ensure!(
            self.start == expected_start,
            "MD save address differs from ROM"
        );
        ensure!(
            expected == self.serial.as_ref().map(|_| self.layout),
            "MD save profile differs from ROM"
        );
        Ok(())
    }

    fn validate_fixed_parallel(&self, layout: OddSaveLayout) -> Result<()> {
        ensure!(
            self.layout == layout
                && self.start == SAVE_START
                && !self.switched
                && self.initially_enabled
                && self.enabled
                && !self.write_protected
                && !self.time_latch
                && self.serial.is_none(),
            "MD fixed parallel mapping differs from ROM"
        );
        Ok(())
    }

    pub fn serial_read(&self, address: u32) -> Option<u16> {
        self.serial
            .as_ref()
            .filter(|_| !self.rom_visible() && self.contains(address))
            .map(|serial| u16::from(serial.output()))
    }

    pub fn serial_write(
        &mut self,
        value: u16,
        upper: bool,
        lower: bool,
        now: u64,
        master_hz: u64,
    ) -> bool {
        let Some(serial) = &mut self.serial else {
            return false;
        };
        if serial.split_lines() {
            if upper && lower {
                self.enabled = value & 1 == 0;
            } else if upper {
                serial.write_line((value >> 8) as u8, true, now, master_hz, &mut self.bytes);
            } else if lower {
                serial.write_line(value as u8, false, now, master_hz, &mut self.bytes);
            }
            return true;
        }
        if lower || serial.separate_address() && upper {
            let value = if lower {
                value as u8
            } else {
                (value >> 8) as u8
            };
            serial.write(value, now, master_hz, &mut self.bytes);
        }
        true
    }

    pub fn load(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            !self.reserved_switched(),
            "MD cartridge has no supported save RAM"
        );
        ensure!(
            bytes.len() == self.layout.bytes(),
            "MD save RAM must contain exactly {} bytes",
            self.layout.bytes()
        );
        self.bytes.copy_from_slice(bytes);
        Ok(())
    }
}

fn bounded_split_profile(rom: &[u8]) -> bool {
    rom.len() == 0x40_0000
        && matches!(
            rom.get(0x180..0x18e),
            Some(b"GM T-172116-00") | Some(b"GM T-172026-04")
        )
        && rom.get(HEADER_START..HEADER_START + 12)
            == Some(b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\xff\xff")
}
fn odd_geometry(rom: &[u8]) -> Option<(OddSaveLayout, u32)> {
    let declaration = rom.get(HEADER_START..HEADER_START + 12)?;
    if declaration[..4] != DECLARATION_PREFIX {
        return None;
    }
    let start = u32::from_be_bytes(declaration[4..8].try_into().ok()?);
    let end = u32::from_be_bytes(declaration[8..12].try_into().ok()?);
    if start & 1 == 0 || end & 1 == 0 || start < SAVE_START || end >= SWITCHED_SAVE_END {
        return None;
    }
    let layout = match end.checked_sub(start)? / 2 + 1 {
        512 if start == SAVE_START + 1 => OddSaveLayout::Bytes512,
        8192 => OddSaveLayout::KiB8,
        32768 => OddSaveLayout::KiB32,
        _ => return None,
    };
    Some((layout, start & !1))
}

fn initially_mapped_odd512(rom: &[u8]) -> bool {
    rom.len() == 0x10_0000
        && matches!(
            rom.get(0x180..0x18e),
            Some(b"GM 00005518-00") | Some(b"GM 00005518-03") | Some(b"GM 00005532-00")
        )
}

fn switching_profile(rom: &[u8], layout: OddSaveLayout) -> bool {
    if rom.len() != SWITCHED_ROM_BYTES {
        return false;
    }
    match layout {
        OddSaveLayout::KiB8 => matches!(
            rom.get(0x180..0x18e),
            Some(b"GM G-5524  -00")
                | Some(b"GM MK -1307   ")
                | Some(b"GM MK-1307 -00")
                | Some(b"GM MK-1354 -00")
                | Some(b"GM G-5543  -00")
        ),
        OddSaveLayout::KiB32 => matches!(
            rom.get(0x180..0x18e),
            Some(b"GM MK-1239 -00")
                | Some(b"GM MK-1242 -00")
                | Some(b"GM MK-1244 -00")
                | Some(b"GM 00001022-00")
        ),
        OddSaveLayout::ExpansionBytesKiB64
        | OddSaveLayout::BoundedSwitchedBytes2048
        | OddSaveLayout::ReservedSwitchedRom
        | OddSaveLayout::BoundedSwitchedOddKiB8
        | OddSaveLayout::BoundedOdd2400
        | OddSaveLayout::FixedOddKiB8
        | OddSaveLayout::HeaderlessOddKiB8
        | OddSaveLayout::Bytes512
        | OddSaveLayout::Serial128
        | OddSaveLayout::Serial256
        | OddSaveLayout::SplitSerial256
        | OddSaveLayout::SplitSerial512
        | OddSaveLayout::SplitSerial2048
        | OddSaveLayout::SplitSerial8192
        | OddSaveLayout::Parallel2048
        | OddSaveLayout::BoundedParallel2048
        | OddSaveLayout::BoundedSplitKiB32 => false,
    }
}

fn time_latch_profile(rom: &[u8]) -> bool {
    matches!(
        rom.get(0x180..0x18e),
        Some(b"GM MK-1354 -00") | Some(b"GM G-5543  -00")
    )
}

fn parallel_2048_profile(rom: &[u8]) -> bool {
    // Programming delay is not modeled for this parallel profile.
    rom.len() == 0x20_0000
        && rom.get(0x180..0x18e) == Some(b"GM T-119186-00")
        && rom.get(HEADER_START..HEADER_START + 12)
            == Some(b"RA\xf8\x40\x00\x20\x00\x01\x00\x20\x0f\xff")
}

fn bounded_parallel_2048_profile(rom: &[u8]) -> bool {
    rom.len() == 0x20_0000
        && rom.get(0x180..0x18e) == Some(b"GM T-119156-00")
        && rom.get(HEADER_START..HEADER_START + 12)
            == Some(b"RA\xf8\x40\x00\x20\x00\x01\x00\x20\x07\xff")
}

fn corrected_odd8_profile(rom: &[u8]) -> bool {
    rom.len() == 0xa_0000
        && rom.get(0x180..0x18e) == Some(b"GM T-26013 -00")
        && rom.get(HEADER_START..HEADER_START + 12)
            == Some(&[b'R', b'A', 0xf8, 0x20, 0x20, 0, 0, 0x20, 0, 1, 0, 0x20])
}

fn headerless_odd32_profile(rom: &[u8]) -> bool {
    rom.len() == 0x20_0000
        && rom.get(0x180..0x18e) == Some(b"GM ACLD012 -00")
        && rom.get(0x18e..0x190) == Some(&[0xd7, 0x23])
        && rom.get(HEADER_START..HEADER_START + 12) == Some(&[b' '; 12])
}

fn header_odd8_profile(rom: &[u8]) -> bool {
    matches!(
        (
            rom.len(),
            rom.get(0x180..0x18e),
            rom.get(HEADER_START..HEADER_START + 12)
        ),
        (
            0x10_0000,
            Some(b"GM    55508-00"),
            Some(b"RA\xf8\x20\x00\x20\x00\x00\x00\x20\x3f\xff")
        ) | (
            0xa_0000,
            Some(b"GM 00005502-00"),
            Some(b"RA\xf8\x20\x00\x20\x00\x00\x00\x20\x3f\xff")
        ) | (
            0x10_0000,
            Some(b"GM T-106113-00"),
            Some(b"RA\xe0\x20\x00\x20\x00\x01\x00\x20\x3f\xff")
        )
    ) || content_odd8_profile(rom)
}

fn content_odd8_profile(rom: &[u8]) -> bool {
    use sha2::{Digest, Sha256};
    content_odd8_identity(
        rom.len(),
        rom.get(0x180..0x18e),
        rom.get(HEADER_START..HEADER_START + 12),
        || Sha256::digest(rom).into(),
    )
}

fn content_odd8_identity(
    size: usize,
    product: Option<&[u8]>,
    declaration: Option<&[u8]>,
    digest: impl FnOnce() -> [u8; 32],
) -> bool {
    size == 0x20_0000
        && product == Some(b"GM T-      -00")
        && declaration == Some(b"RA\xf8\x00\x00\x20\x00\x01\x00\x20\x3f\xff")
        && digest()
            == [
                0x6e, 0x5f, 0x0f, 0x3a, 0xb2, 0xf8, 0x00, 0x77, 0x3a, 0xc2, 0xdf, 0x61, 0xa4, 0xcc,
                0x60, 0xfb, 0x44, 0x92, 0x3f, 0x33, 0xa6, 0x23, 0x5e, 0x30, 0xc3, 0xf3, 0x53, 0xe9,
                0x21, 0x49, 0x7a, 0x36,
            ]
}

fn fixed_parallel_profile(rom: &[u8]) -> Option<OddSaveLayout> {
    use sha2::{Digest, Sha256};
    if headerless_odd8::detect(rom) {
        return Some(OddSaveLayout::HeaderlessOddKiB8);
    }
    fixed_parallel_identity(
        rom.len(),
        rom.get(0x180..0x18e),
        rom.get(HEADER_START..HEADER_START + 12),
        || Sha256::digest(rom).into(),
    )
}

fn fixed_parallel_identity(
    len: usize,
    product: Option<&[u8]>,
    declaration: Option<&[u8]>,
    digest: impl FnOnce() -> [u8; 32],
) -> Option<OddSaveLayout> {
    let (layout, expected) = match (len, product, declaration) {
        (0x10_0000, Some(b"GM T-50286 -00"), Some(b"            ")) => (
            OddSaveLayout::FixedOddKiB8,
            [
                0x99, 0x7c, 0xbd, 0x68, 0x2b, 0xc7, 0xc0, 0x63, 0x63, 0x02, 0xf0, 0x72, 0x19, 0xd9,
                0x15, 0x22, 0x44, 0xc8, 0xae, 0x06, 0x02, 0x8f, 0xd7, 0xd9, 0x14, 0x63, 0xd3, 0x57,
                0x56, 0x63, 0x0c, 0xe5,
            ],
        ),
        (
            0x20_0000,
            Some(b"GM G-5519  -00"),
            Some(b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\x03\xff"),
        ) => (
            OddSaveLayout::Bytes512,
            [
                0x37, 0x96, 0xf4, 0x07, 0xae, 0x0b, 0xf3, 0xb7, 0x4b, 0xbe, 0x53, 0x33, 0x5a, 0xdf,
                0x57, 0xf2, 0x95, 0xf0, 0xbf, 0x53, 0xef, 0x7d, 0x82, 0xb6, 0x03, 0xac, 0x73, 0x35,
                0x0e, 0xac, 0x8c, 0xae,
            ],
        ),
        (
            0x20_0000,
            Some(b"GM T-119076-00"),
            Some(b"RA\xf8\x40\x00\x20\x00\x01\x00\x20\x07\xff"),
        ) => (
            OddSaveLayout::BoundedParallel2048,
            [
                0x24, 0x98, 0xde, 0x5f, 0x6c, 0x5d, 0x23, 0xa2, 0xdd, 0xe9, 0xa5, 0xb6, 0x02, 0x81,
                0xdb, 0xfd, 0xc4, 0x35, 0xff, 0x6e, 0xb7, 0xd2, 0x93, 0x7d, 0x77, 0x0c, 0x2f, 0xf4,
                0xf3, 0x7d, 0xb9, 0x26,
            ],
        ),
        (
            0x10_0000,
            Some(b"GM T-119016-00"),
            Some(b"RA\xf8\x40\x00\x20\x00\x01\x00\x20\x07\xff"),
        ) => (
            OddSaveLayout::BoundedParallel2048,
            [
                0x21, 0x27, 0x4f, 0x87, 0xc5, 0x0a, 0x7c, 0x77, 0x4b, 0x87, 0x83, 0x3b, 0xec, 0xc4,
                0xe6, 0xc3, 0x73, 0x6e, 0x12, 0x34, 0x9c, 0x6b, 0x8a, 0xbc, 0x81, 0xb9, 0x2d, 0x04,
                0x1f, 0xd1, 0x75, 0x54,
            ],
        ),
        (
            0x20_0000,
            Some(b"GM T-119096-00"),
            Some(b"RA\xf8\x40\x00\x20\x00\x01\x00\x20\x03\xff"),
        ) => (
            OddSaveLayout::Bytes512,
            [
                0x6e, 0x3a, 0xa9, 0x8a, 0x12, 0x38, 0x5d, 0x12, 0x15, 0xb5, 0x60, 0x8e, 0x2f, 0x42,
                0x67, 0xee, 0xa0, 0xef, 0xec, 0x28, 0x79, 0xe9, 0xc0, 0xa2, 0x16, 0x20, 0xb3, 0x55,
                0xeb, 0x6a, 0xa2, 0xab,
            ],
        ),
        (
            0x10_0000,
            Some(b"GM T-119046-00"),
            Some(b"RA\xf8\x40\x00\x20\x00\x01\x00\x20\x0f\xff"),
        ) => (
            OddSaveLayout::BoundedParallel2048,
            [
                0x5a, 0xdf, 0x28, 0xad, 0x77, 0x2d, 0x88, 0x55, 0x69, 0xa5, 0x0a, 0x4e, 0x48, 0x40,
                0x80, 0x04, 0xda, 0x37, 0xa2, 0xd4, 0x25, 0xa8, 0xfe, 0x90, 0xa5, 0xf2, 0xa3, 0xcb,
                0xa9, 0xa0, 0x1d, 0x94,
            ],
        ),
        _ => return None,
    };
    (digest() == expected).then_some(layout)
}

fn split_serial_profile(rom: &[u8]) -> Option<OddSaveLayout> {
    if headerless_split_512_profile(rom) {
        return Some(OddSaveLayout::SplitSerial512);
    }
    match (
        rom.len(),
        rom.get(0x180..0x18e),
        rom.get(HEADER_START..HEADER_START + 12),
    ) {
        (
            SWITCHED_ROM_BYTES,
            Some(b"GM T-081276 00"),
            Some(b"RA\xe8\x40\x00\x20\x00\x00\x00\x20\x00\x01"),
        ) => Some(OddSaveLayout::SplitSerial256),
        (
            0x40_0000,
            Some(b"GM T-081586-00"),
            Some(b"RA\xe8\x40\x00\x20\x00\x00\x00\x20\x00\x01"),
        ) => Some(OddSaveLayout::SplitSerial2048),
        (
            0x40_0000,
            Some(b"GM T-81576 -00"),
            Some(b"RA\xe8\x40\x00\x20\x00\x01\x00\x20\x00\x01"),
        )
        | (
            0x40_0000,
            Some(b"GM T-81476 -00"),
            Some(b"RA\xe8\x40\x00\x20\x00\x00\x00\x20\x00\x01"),
        ) => Some(OddSaveLayout::SplitSerial8192),
        _ => None,
    }
}

fn headerless_split_512_profile(rom: &[u8]) -> bool {
    use sha2::{Digest, Sha256};
    headerless_split_512_identity(
        rom.len(),
        rom.get(0x180..0x18e),
        rom.get(HEADER_START..HEADER_START + 12),
        || Sha256::digest(rom).into(),
    )
}

fn headerless_split_512_identity(
    size: usize,
    product: Option<&[u8]>,
    declaration: Option<&[u8]>,
    digest: impl FnOnce() -> [u8; 32],
) -> bool {
    if product != Some(b"GM T-81406 -00") || declaration != Some(b"            ") {
        return false;
    }
    let expected = match size {
        0x30_0000 => [
            0x5b, 0x73, 0x5b, 0x44, 0x31, 0x02, 0xf7, 0x71, 0xbc, 0x7d, 0xb4, 0x9d, 0xcf, 0x34,
            0xde, 0x64, 0x0c, 0x10, 0xd2, 0x12, 0xf5, 0xcb, 0x4a, 0xf1, 0x25, 0x1d, 0x95, 0x67,
            0x51, 0xf2, 0x10, 0x72,
        ],
        0x40_0000 => [
            0x8b, 0xdd, 0x2f, 0xa6, 0x8e, 0x70, 0xf7, 0x27, 0xf6, 0xc8, 0x6f, 0x82, 0x66, 0xec,
            0x70, 0x34, 0x57, 0xe0, 0x9c, 0x29, 0x40, 0xcb, 0xd8, 0xf8, 0x2c, 0x37, 0x60, 0xe2,
            0x17, 0x7b, 0x88, 0xfe,
        ],
        _ => return false,
    };
    digest() == expected
}

fn internal_ram_declaration_profile(rom: &[u8]) -> bool {
    use sha2::{Digest, Sha256};
    internal_ram_declaration_identity(
        rom.len(),
        rom.get(0x180..0x18e),
        rom.get(HEADER_START..HEADER_START + 12),
        || Sha256::digest(rom).into(),
    )
}

fn internal_ram_declaration_identity(
    size: usize,
    product: Option<&[u8]>,
    declaration: Option<&[u8]>,
    digest: impl FnOnce() -> [u8; 32],
) -> bool {
    size == 0x10_0000
        && product == Some(b"MDGM-0001-0001")
        && declaration == Some(b"RA\x20\x20\x00\xff\x00\x00\x00\xff\xff\xff")
        && matches!(
            digest(),
            [
                0xd5, 0x75, 0xdc, 0x6f, 0x55, 0xcc, 0x28, 0xb3, 0xa6, 0x71, 0x52, 0xec, 0x58, 0x79,
                0xef, 0x97, 0x1b, 0x45, 0x2c, 0x04, 0x86, 0x63, 0x69, 0x38, 0x6a, 0x7f, 0x59, 0xbc,
                0x96, 0x75, 0xc4, 0x3d,
            ] | [
                0x1b, 0xc1, 0xca, 0xd1, 0x73, 0x6c, 0xde, 0xbb, 0xe2, 0x30, 0xfc, 0xb6, 0xb4, 0x64,
                0xcf, 0x02, 0xe0, 0xc5, 0x3d, 0x8b, 0x41, 0x34, 0x08, 0x4a, 0x1a, 0x3b, 0x00, 0x76,
                0x67, 0x88, 0x5b, 0x3b,
            ]
        )
}

#[cfg(test)]
pub(crate) mod profile_tests;
