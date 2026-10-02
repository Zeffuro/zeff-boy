use std::collections::BTreeMap;

use crate::{Budget, ScanStop, drivers::CodeWrite, tracker::FileSpan};

use super::{MAX_CALL_SITES, MAX_NODES, MAX_WRITERS};

pub(super) const ROOTS: [u16; 6] = [0x100, 0x40, 0x48, 0x50, 0x58, 0x60];

pub(super) struct Rom<'a> {
    bytes: &'a [u8],
    window_len: usize,
    pub mbc0: bool,
}

pub(super) struct Decoded {
    pub nodes: BTreeMap<u16, Node>,
    pub writes: Vec<CodeWrite>,
}

pub(super) struct Node {
    length: u8,
    pub successors: [Option<u16>; 2],
    pub call_target: Option<u16>,
}

impl Node {
    pub fn span(&self, address: u16) -> FileSpan {
        FileSpan {
            offset: u32::from(address),
            byte_len: u32::from(self.length),
        }
    }
}

impl<'a> Rom<'a> {
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        let header = bytes.get(..0x150)?;
        if !matches!(header[0x143], 0 | 0x80 | 0xc0) || header[0x149] > 5 {
            return None;
        }
        let checksum = header[0x134..0x14d]
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_sub(*byte).wrapping_sub(1));
        if checksum != header[0x14d] {
            return None;
        }
        let banks = match header[0x148] {
            value @ 0..=8 => 2usize << value,
            0x52 => 72,
            0x53 => 80,
            0x54 => 96,
            _ => return None,
        };
        let ram_banks = match header[0x149] {
            0 => 0,
            1 | 2 => 1,
            3 => 4,
            4 => 16,
            5 => 8,
            _ => return None,
        };
        let (mbc0, max_banks, max_ram_banks) = match header[0x147] {
            0x00 => (true, 2, 0),
            0x08 | 0x09 => (true, 2, 1),
            0x01 | 0x0f | 0x11 => (false, 128, 0),
            0x02 | 0x03 | 0x10 | 0x12 | 0x13 => (false, 128, 4),
            0x05 | 0x06 => (false, 16, 0),
            0x19 | 0x1c => (false, 512, 0),
            0x1a | 0x1b => (false, 512, 16),
            0x1d | 0x1e => (false, 512, 8),
            _ => return None,
        };
        if banks > max_banks || ram_banks > max_ram_banks || bytes.len() != banks * 0x4000 {
            return None;
        }
        Some(Self {
            bytes,
            window_len: if mbc0 { 0x8000 } else { 0x4000 },
            mbc0,
        })
    }

    fn instruction(&self, address: u16, length: usize) -> Option<&[u8]> {
        let start = usize::from(address);
        let end = start.checked_add(length)?;
        if end > self.window_len || (start < 0x150 && end > 0x104) {
            return None;
        }
        self.bytes.get(start..end)
    }
}

pub(super) fn decode(rom: &Rom<'_>, budget: &mut Budget<'_>) -> Result<Option<Decoded>, ScanStop> {
    let mut nodes = BTreeMap::new();
    let mut pending = ROOTS.into_iter().rev().collect::<Vec<_>>();
    let mut owned = vec![None; rom.window_len];
    let mut writes = Vec::new();
    let mut call_sites = 0;
    while let Some(address) = pending.pop() {
        budget.charge()?;
        if nodes.contains_key(&address) || usize::from(address) >= rom.window_len {
            continue;
        }
        if nodes.len() == MAX_NODES || pending.len() > MAX_NODES * 2 {
            return Err(ScanStop::ValidationLimit);
        }
        let Some(opcode) = rom.instruction(address, 1).map(|bytes| bytes[0]) else {
            return Ok(None);
        };
        let Some(length) = opcode_length(opcode) else {
            return Ok(None);
        };
        let Some(instruction) = rom.instruction(address, usize::from(length)) else {
            return Ok(None);
        };
        if opcode == 0x10 && instruction[1] != 0 {
            return Ok(None);
        }
        let start = usize::from(address);
        let end = start + usize::from(length);
        if owned[start..end]
            .iter()
            .any(|owner| owner.is_some_and(|owner| owner != address))
        {
            return Ok(None);
        }
        owned[start..end].fill(Some(address));
        let register = match opcode {
            0xe0 => Some(0xff00 | u16::from(instruction[1])),
            0xea => Some(word(instruction)),
            _ => None,
        };
        if let Some(register @ (0xff10..=0xff26 | 0xff30..=0xff3f)) = register {
            if writes.len() == MAX_WRITERS {
                return Err(ScanStop::InventoryLimit);
            }
            writes.push(CodeWrite {
                cpu_address: address,
                register,
                span: FileSpan {
                    offset: u32::from(address),
                    byte_len: u32::from(length),
                },
            });
        }
        let next = address.wrapping_add(u16::from(length));
        let relative = || next.wrapping_add_signed(i16::from(instruction[1] as i8));
        let call_target = match opcode {
            0xc4 | 0xcc | 0xcd | 0xd4 | 0xdc => Some(word(instruction)),
            0xc7 | 0xcf | 0xd7 | 0xdf | 0xe7 | 0xef | 0xf7 | 0xff => Some(u16::from(opcode & 0x38)),
            _ => None,
        };
        if call_target.is_some() {
            call_sites += 1;
            if call_sites > MAX_CALL_SITES {
                return Err(ScanStop::InventoryLimit);
            }
        }
        let successors = if let Some(target) = call_target {
            // Fallthrough assumes a possible returning call, including conditional calls.
            [Some(target), Some(next)]
        } else {
            match opcode {
                0xc3 => [Some(word(instruction)), None],
                0xc2 | 0xca | 0xd2 | 0xda => [Some(word(instruction)), Some(next)],
                0x18 => [Some(relative()), None],
                0x20 | 0x28 | 0x30 | 0x38 => [Some(relative()), Some(next)],
                0xc9 | 0xd9 | 0xe9 => [None, None],
                _ => [Some(next), None],
            }
        };
        pending.extend(successors.iter().flatten().copied());
        nodes.insert(
            address,
            Node {
                length,
                successors,
                call_target,
            },
        );
    }
    writes.sort_by_key(|write| write.cpu_address);
    Ok(Some(Decoded { nodes, writes }))
}

fn word(instruction: &[u8]) -> u16 {
    u16::from_le_bytes([instruction[1], instruction[2]])
}

pub(super) fn opcode_length(opcode: u8) -> Option<u8> {
    // Operand widths follow the SM83 core's dispatch and load/flow handlers.
    match opcode {
        0xd3 | 0xdb | 0xdd | 0xe3 | 0xe4 | 0xeb | 0xec | 0xed | 0xf4 | 0xfc | 0xfd => None,
        0x01 | 0x08 | 0x11 | 0x21 | 0x31 | 0xc2 | 0xc3 | 0xc4 | 0xca | 0xcc | 0xcd | 0xd2
        | 0xd4 | 0xda | 0xdc | 0xea | 0xfa => Some(3),
        0x06 | 0x0e | 0x10 | 0x16 | 0x18 | 0x1e | 0x20 | 0x26 | 0x28 | 0x2e | 0x30 | 0x36
        | 0x38 | 0x3e | 0xc6 | 0xcb | 0xce | 0xd6 | 0xde | 0xe0 | 0xe6 | 0xe8 | 0xee | 0xf0
        | 0xf6 | 0xf8 | 0xfe => Some(2),
        _ => Some(1),
    }
}
