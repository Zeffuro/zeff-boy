#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PceConsoleWiring {
    #[default]
    PcEngine,
    TurboGrafx16,
}

impl PceConsoleWiring {
    #[inline]
    pub const fn controller_upper_bits(self) -> u8 {
        match self {
            Self::PcEngine => 0xF0,
            Self::TurboGrafx16 => 0xB0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PceCartridgeHardware {
    #[default]
    Base,
    SuperGrafx,
}

pub const SF2_CE_HUCARD_IMAGE_LEN: usize = 0x28_0000;
pub const POPULOUS_HUCARD_IMAGE_LEN: usize = 0x08_0000;
const STANDARD_HUCARD_384K_IMAGE_LEN: usize = 0x06_0000;
const STANDARD_HUCARD_512K_IMAGE_LEN: usize = 0x08_0000;
const HUCARD_BANK_LEN: u32 = 0x2000;
pub const POPULOUS_HUCARD_RAM_LEN: usize = 0x8000;
pub const SUPER_SYSTEM_CARD_RAM_LEN: usize = 0x30_000;
pub const SYSTEM_CARD_V1_V2_IMAGE_LEN: usize = 0x40000;
pub const SF2_CE_CANONICAL_SHA256: [u8; 32] = [
    0x6D, 0x65, 0x78, 0x74, 0x23, 0x84, 0xCF, 0x0B, 0x77, 0x14, 0x14, 0xDD, 0xF0, 0x16, 0xB7, 0xDC,
    0x13, 0x3A, 0xD0, 0x35, 0x0A, 0x83, 0x8D, 0x13, 0x40, 0x36, 0x12, 0x4F, 0xED, 0x6E, 0x60, 0xE1,
];
pub const POPULOUS_CANONICAL_SHA256: [u8; 32] = [
    0x16, 0xA7, 0x96, 0x63, 0x4A, 0x2B, 0x3D, 0xA4, 0x17, 0xA7, 0x75, 0xCB, 0x64, 0xFF, 0xA3, 0x32,
    0x2A, 0xD7, 0xBA, 0xF9, 0x01, 0xC2, 0x88, 0x93, 0x4D, 0x28, 0xDF, 0x96, 0x7F, 0x79, 0x77, 0xF6,
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PceHuCardBoard {
    #[default]
    Plain,
    Sf2Ce,
    Populous,
    SystemCardV1V2,
    SystemCardV3,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PceCartridgeDescriptor {
    image_sha256: Option<[u8; 32]>,
    console_wiring_override: Option<PceConsoleWiring>,
    hardware_override: Option<PceCartridgeHardware>,
    board_override: Option<PceHuCardBoard>,
}

impl PceCartridgeDescriptor {
    pub const fn from_sha256(image_sha256: [u8; 32]) -> Self {
        Self {
            image_sha256: Some(image_sha256),
            console_wiring_override: None,
            hardware_override: None,
            board_override: None,
        }
    }

    pub const fn with_console_wiring(mut self, console_wiring: PceConsoleWiring) -> Self {
        self.console_wiring_override = Some(console_wiring);
        self
    }

    pub const fn with_required_hardware(mut self, hardware: PceCartridgeHardware) -> Self {
        self.hardware_override = Some(hardware);
        self
    }

    pub const fn with_hucard_board(mut self, board: PceHuCardBoard) -> Self {
        self.board_override = Some(board);
        self
    }

    #[inline]
    pub const fn image_sha256(self) -> Option<[u8; 32]> {
        self.image_sha256
    }

    pub fn console_wiring(self) -> PceConsoleWiring {
        if let Some(console_wiring) = self.console_wiring_override {
            return console_wiring;
        }
        if self.image_sha256.is_some_and(is_turbografx16_payload) {
            PceConsoleWiring::TurboGrafx16
        } else {
            PceConsoleWiring::PcEngine
        }
    }

    pub fn required_hardware(self) -> PceCartridgeHardware {
        if let Some(hardware) = self.hardware_override {
            return hardware;
        }
        if self
            .image_sha256
            .is_some_and(|hash| SUPERGRAFX_REQUIRED_SHA256.contains(&hash))
        {
            PceCartridgeHardware::SuperGrafx
        } else {
            PceCartridgeHardware::Base
        }
    }

    pub const fn hucard_board(self, image_len: usize) -> PceHuCardBoard {
        if let Some(board) = self.board_override {
            return board;
        }
        match self.image_sha256 {
            Some(SF2_CE_CANONICAL_SHA256) => PceHuCardBoard::Sf2Ce,
            Some(POPULOUS_CANONICAL_SHA256) => PceHuCardBoard::Populous,
            _ if image_len == SF2_CE_HUCARD_IMAGE_LEN => PceHuCardBoard::Sf2Ce,
            _ => PceHuCardBoard::Plain,
        }
    }
}

#[derive(Debug)]
pub(crate) enum PceHuCard {
    Plain(Box<[u8]>),
    Sf2Ce {
        rom: Box<[u8]>,
        bank: u8,
    },
    Populous {
        rom: Box<[u8]>,
        ram: Box<[u8; POPULOUS_HUCARD_RAM_LEN]>,
    },
    SystemCardV1V2(Box<[u8]>),
    SystemCardV3 {
        rom: Box<[u8]>,
        ram: Box<[u8; SUPER_SYSTEM_CARD_RAM_LEN]>,
    },
}

impl PceHuCard {
    pub(crate) fn new(image: Vec<u8>, board: PceHuCardBoard) -> Self {
        match board {
            PceHuCardBoard::Plain => Self::Plain(image.into_boxed_slice()),
            PceHuCardBoard::Sf2Ce => Self::Sf2Ce {
                rom: image.into_boxed_slice(),
                bank: 0,
            },
            PceHuCardBoard::Populous => Self::Populous {
                rom: image.into_boxed_slice(),
                ram: Box::new([0; POPULOUS_HUCARD_RAM_LEN]),
            },
            PceHuCardBoard::SystemCardV1V2 => Self::SystemCardV1V2(image.into_boxed_slice()),
            PceHuCardBoard::SystemCardV3 => Self::SystemCardV3 {
                rom: image.into_boxed_slice(),
                ram: Box::new([0; SUPER_SYSTEM_CARD_RAM_LEN]),
            },
        }
    }

    pub(crate) fn board(&self) -> PceHuCardBoard {
        match self {
            Self::Plain(_) => PceHuCardBoard::Plain,
            Self::Sf2Ce { .. } => PceHuCardBoard::Sf2Ce,
            Self::Populous { .. } => PceHuCardBoard::Populous,
            Self::SystemCardV1V2(_) => PceHuCardBoard::SystemCardV1V2,
            Self::SystemCardV3 { .. } => PceHuCardBoard::SystemCardV3,
        }
    }

    pub(crate) fn image(&self) -> &[u8] {
        match self {
            Self::Plain(rom)
            | Self::Sf2Ce { rom, .. }
            | Self::Populous { rom, .. }
            | Self::SystemCardV1V2(rom) => rom,
            Self::SystemCardV3 { rom, .. } => rom,
        }
    }

    pub(crate) fn rom_offset(&self, physical_offset: u32) -> Option<u32> {
        match self {
            Self::Plain(rom) => plain_rom_offset(rom.len(), physical_offset),
            Self::Sf2Ce { bank, .. } => match physical_offset {
                0..=0x07_FFFF => Some(physical_offset),
                0x08_0000..=0x0F_FFFF => {
                    Some(0x08_0000 + u32::from(*bank) * 0x08_0000 + (physical_offset - 0x08_0000))
                }
                _ => None,
            },
            Self::Populous { .. } => (physical_offset <= 0x07_FFFF).then_some(physical_offset),
            Self::SystemCardV1V2(_) | Self::SystemCardV3 { .. } => (physical_offset <= 0x07_FFFF)
                .then_some(physical_offset & (SYSTEM_CARD_V1_V2_IMAGE_LEN as u32 - 1)),
        }
    }

    pub(crate) fn mapping_token(&self) -> u8 {
        match self {
            Self::Sf2Ce { bank, .. } => *bank,
            _ => 0,
        }
    }

    pub(crate) fn read(&self, offset: u32) -> u8 {
        match self {
            Self::Plain(rom) => plain_rom_offset(rom.len(), offset)
                .map(|offset| rom[offset as usize])
                .unwrap_or(0xFF),
            Self::Sf2Ce { rom, bank } => {
                let image_offset = if offset < 0x08_0000 {
                    offset as usize
                } else {
                    0x08_0000 + usize::from(*bank) * 0x08_0000 + (offset as usize - 0x08_0000)
                };
                rom[image_offset]
            }
            Self::Populous { rom, ram } => match offset {
                0..=0x07_FFFF => rom[offset as usize],
                0x08_0000..=0x08_7FFF => ram[offset as usize - 0x08_0000],
                _ => 0xFF,
            },
            Self::SystemCardV1V2(rom) => {
                if offset <= 0x07_FFFF {
                    rom[offset as usize & (SYSTEM_CARD_V1_V2_IMAGE_LEN - 1)]
                } else {
                    0xFF
                }
            }
            Self::SystemCardV3 { rom, ram } => match offset {
                0..=0x07_FFFF => rom[offset as usize & (SYSTEM_CARD_V1_V2_IMAGE_LEN - 1)],
                0x0D_0000..=0x0F_FFFF => ram[offset as usize - 0x0D_0000],
                _ => 0xFF,
            },
        }
    }

    pub(crate) fn write(&mut self, offset: u32, value: u8) {
        match self {
            Self::Sf2Ce { bank, .. } if (0x001FF0..=0x001FF3).contains(&offset) => {
                *bank = (offset - 0x001FF0) as u8;
            }
            Self::Populous { ram, .. } if (0x08_0000..=0x08_7FFF).contains(&offset) => {
                ram[offset as usize - 0x08_0000] = value;
            }
            Self::SystemCardV3 { ram, .. } if (0x0D_0000..=0x0F_FFFF).contains(&offset) => {
                ram[offset as usize - 0x0D_0000] = value;
            }
            _ => {}
        }
    }

    pub(crate) fn reset(&mut self) {
        match self {
            Self::Sf2Ce { bank, .. } => *bank = 0,
            Self::SystemCardV3 { ram, .. } => ram.fill(0),
            _ => {}
        }
    }

    pub(crate) fn ram(&self) -> Option<&[u8; POPULOUS_HUCARD_RAM_LEN]> {
        match self {
            Self::Populous { ram, .. } => Some(ram),
            _ => None,
        }
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        match self {
            Self::Plain(_) | Self::SystemCardV1V2(_) => {}
            Self::Sf2Ce { bank, .. } => writer.write_u8(*bank),
            Self::Populous { ram, .. } => writer.write_bytes(&ram[..]),
            Self::SystemCardV3 { ram, .. } => writer.write_bytes(&ram[..]),
        }
    }

    pub(super) fn read_state(&mut self, reader: &mut StateReader<'_>) -> anyhow::Result<()> {
        match self {
            Self::Plain(_) | Self::SystemCardV1V2(_) => {}
            Self::Sf2Ce { bank, .. } => {
                let saved_bank = reader.read_u8()?;
                if saved_bank > 3 {
                    bail!("invalid Street Fighter II HuCard bank in save-state: {saved_bank}");
                }
                *bank = saved_bank;
            }
            Self::Populous { ram, .. } => reader.read_exact(&mut ram[..])?,
            Self::SystemCardV3 { ram, .. } => reader.read_exact(&mut ram[..])?,
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn system_card_ram_mut(&mut self) -> Option<&mut [u8; SUPER_SYSTEM_CARD_RAM_LEN]> {
        match self {
            Self::SystemCardV3 { ram, .. } => Some(ram),
            _ => None,
        }
    }
}

fn plain_rom_offset(image_len: usize, physical_offset: u32) -> Option<u32> {
    if physical_offset > 0x0F_FFFF {
        return None;
    }
    let bank = physical_offset / HUCARD_BANK_LEN;
    let offset_in_bank = physical_offset % HUCARD_BANK_LEN;
    let mapped_bank = match image_len {
        STANDARD_HUCARD_384K_IMAGE_LEN if bank < 0x40 => bank & 0x1F,
        STANDARD_HUCARD_384K_IMAGE_LEN => (bank & 0x0F) + 0x20,
        STANDARD_HUCARD_512K_IMAGE_LEN if bank < 0x40 => bank,
        STANDARD_HUCARD_512K_IMAGE_LEN => (bank & 0x1F) + 0x20,
        _ => bank,
    };
    let mapped_offset = mapped_bank * HUCARD_BANK_LEN + offset_in_bank;
    ((mapped_offset as usize) < image_len).then_some(mapped_offset)
}
const SUPERGRAFX_REQUIRED_SHA256: [[u8; 32]; 5] = [
    [
        0x50, 0x06, 0xF2, 0xDA, 0x9C, 0xB6, 0x45, 0x31, 0x2A, 0x0C, 0x58, 0x90, 0x44, 0xDF, 0x50,
        0xD3, 0xF9, 0x71, 0x06, 0xD2, 0xD2, 0x29, 0x1B, 0xF9, 0x88, 0x3D, 0xAC, 0xF9, 0x89, 0x60,
        0xC2, 0xFE,
    ],
    [
        0x5F, 0x3B, 0x43, 0x0E, 0x34, 0xC7, 0x92, 0x18, 0xA9, 0xF8, 0x9A, 0x40, 0x3A, 0x28, 0x60,
        0x37, 0xB2, 0xFB, 0x17, 0x2B, 0x52, 0x83, 0x73, 0xDF, 0x5B, 0xA7, 0x0A, 0xED, 0xBE, 0xCD,
        0x36, 0xD7,
    ],
    [
        0x41, 0xE0, 0x6B, 0xEE, 0xAC, 0xFD, 0x05, 0xC8, 0x37, 0xC9, 0xBB, 0x76, 0xDA, 0x73, 0xC2,
        0x8D, 0x14, 0xDC, 0x2F, 0x66, 0x25, 0x0A, 0x24, 0x5B, 0x69, 0x31, 0x71, 0x2F, 0x36, 0xC4,
        0xE4, 0x57,
    ],
    [
        0x48, 0x2F, 0xFF, 0x40, 0x1F, 0x8A, 0x0F, 0x42, 0x48, 0xAF, 0x16, 0x22, 0x4C, 0x31, 0xBC,
        0x16, 0x6A, 0x58, 0x3B, 0x49, 0x14, 0x13, 0x55, 0x9A, 0x89, 0xC4, 0x25, 0x16, 0x54, 0x20,
        0xA9, 0xDD,
    ],
    [
        0x9B, 0x57, 0xCD, 0xF0, 0xD0, 0xB1, 0x10, 0xF4, 0x12, 0x8B, 0x86, 0x34, 0x19, 0xD5, 0xBE,
        0x99, 0xA3, 0x70, 0x8B, 0xFB, 0x11, 0xCF, 0xBE, 0x16, 0x96, 0xF2, 0x54, 0x49, 0xB9, 0x91,
        0x02, 0x6D,
    ],
];
use super::cartridge_catalog::is_turbografx16_payload;
use anyhow::bail;
use zeff_emu_common::save_state::{StateReader, StateWriter};
