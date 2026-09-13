use std::collections::{BTreeMap, BTreeSet};

use crate::{Budget, ScanStop};

use super::{ReadError, word};

mod data;

pub(super) struct Profile {
    pub name: &'static str,
    pub hash: &'static str,
    pub table_operand: usize,
    pub direct_patterns: bool,
    pub wave_table: bool,
    pub aliases: &'static [&'static [usize]],
}

pub(super) struct Driver {
    pub profile: &'static Profile,
    pub bank: u16,
    pub wram: u16,
    pub table: u16,
    pub code: Vec<(usize, usize)>,
    pub constants: Vec<u16>,
}

impl Driver {
    pub fn offset(&self, pointer: usize) -> usize {
        usize::from(self.bank) * 0x4000 + pointer - 0x4000
    }
}

pub(super) fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Vec<Driver>, ScanStop> {
    budget.charge()?;
    let mut drivers = Vec::new();
    if !super::supports_cartridge(bytes) {
        return Ok(drivers);
    }
    for bank in 1..bytes.len() / 0x4000 {
        budget.charge()?;
        let code = &bytes[bank * 0x4000..(bank + 1) * 0x4000];
        if [0, 3, 6].iter().any(|&at| code[at] != 0xc3) {
            continue;
        }
        match recognize(code, bank as u16, budget) {
            Ok(Some(driver)) => drivers.push(driver),
            Ok(None) | Err(ReadError::Invalid) => (),
            Err(ReadError::Stop(stop)) => return Err(stop),
        }
    }
    Ok(drivers)
}

fn recognize(code: &[u8], bank: u16, budget: &mut Budget<'_>) -> Result<Option<Driver>, ReadError> {
    let instructions = closure(code, budget)?;
    let ram_operands: Vec<_> = instructions
        .iter()
        .filter(|&&(at, len)| {
            len == 3
                && matches!(code[at], 0x01 | 0x11 | 0x21 | 0xea | 0xfa)
                && (0xc000..0xe000).contains(&word(code, at + 1))
        })
        .map(|&(at, _)| at + 1)
        .collect();
    let Some(wram) = ram_operands.iter().map(|&at| word(code, at)).min() else {
        return Ok(None);
    };
    if wram > 0xde00
        || ram_operands
            .iter()
            .any(|&at| word(code, at) >= wram + 512 || word(code, at) >= 0xdf80)
    {
        return Ok(None);
    }
    let mut normalized = Vec::new();
    let mut pointers: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for &(at, len) in &instructions {
        budget.charge()?;
        normalized.extend((at as u16).to_le_bytes());
        let mut instruction = code[at..at + len].to_vec();
        if len == 3 {
            let value = word(code, at + 1);
            if ram_operands.binary_search(&(at + 1)).is_ok() {
                instruction[1..].copy_from_slice(&(value - wram).to_le_bytes());
            } else if matches!(code[at], 0x01 | 0x11 | 0x21) && (0x4000..0x8000).contains(&value) {
                pointers.entry(value).or_default().push(at + 1);
                instruction[1..].fill(0);
            }
        }
        normalized.extend(instruction);
    }
    let hash = zeff_firmware::sha256_hex(&normalized);
    #[cfg(not(any(test, feature = "test-support")))]
    let mut profiles = data::PROFILES.iter();
    #[cfg(any(test, feature = "test-support"))]
    let mut profiles = data::PROFILES
        .iter()
        .chain(std::iter::once(&super::tests::PROFILE));
    let Some(profile) = profiles.find(|profile| profile.hash == hash) else {
        return Ok(None);
    };
    if profile.aliases.iter().any(|operands| {
        operands
            .iter()
            .any(|&at| word(code, at) != word(code, operands[0]))
    }) {
        return Ok(None);
    }
    let table = word(code, profile.table_operand);
    if !(0x4000..0x8000).contains(&table) || pointers.keys().any(|&pointer| pointer > 0x7e00) {
        return Ok(None);
    }
    Ok(Some(Driver {
        profile,
        bank,
        wram,
        table,
        code: instructions,
        constants: pointers.keys().copied().filter(|&p| p != table).collect(),
    }))
}

fn closure(code: &[u8], budget: &mut Budget<'_>) -> Result<Vec<(usize, usize)>, ReadError> {
    let mut pending = vec![0, 3, 6];
    let mut seen = BTreeMap::new();
    let mut occupied = BTreeSet::new();
    while let Some(at) = pending.pop() {
        budget.charge()?;
        if seen.contains_key(&at) {
            continue;
        }
        let op = *code.get(at).ok_or(ReadError::Invalid)?;
        let len = instruction_length(op);
        if at + len > code.len()
            || matches!(
                op,
                0xc7 | 0xcf
                    | 0xd7
                    | 0xdf
                    | 0xe7
                    | 0xef
                    | 0xf7
                    | 0xff
                    | 0xe9
                    | 0xd3
                    | 0xdb
                    | 0xdd
                    | 0xe3
                    | 0xe4
                    | 0xeb
                    | 0xec
                    | 0xed
                    | 0xf4
                    | 0xfc
                    | 0xfd
            )
            || (at..at + len).any(|byte| !occupied.insert(byte))
        {
            return Err(ReadError::Invalid);
        }
        seen.insert(at, len);
        if matches!(
            op,
            0xc3 | 0xc2 | 0xca | 0xd2 | 0xda | 0xcd | 0xc4 | 0xcc | 0xd4 | 0xdc
        ) {
            let pointer = word(code, at + 1);
            if !(0x4000..0x8000).contains(&pointer) {
                return Err(ReadError::Invalid);
            }
            pending.push(usize::from(pointer - 0x4000));
        }
        if matches!(op, 0x18 | 0x20 | 0x28 | 0x30 | 0x38) {
            let target = (at + len) as i32 + i32::from(code[at + 1] as i8);
            if target < 0 {
                return Err(ReadError::Invalid);
            }
            pending.push(target as usize);
        }
        if !matches!(op, 0xc3 | 0x18 | 0xc9 | 0xd9) {
            pending.push(at + len);
        }
    }
    Ok(seen.into_iter().collect())
}

fn instruction_length(op: u8) -> usize {
    match op {
        0x01 | 0x08 | 0x11 | 0x21 | 0x31 | 0xc2 | 0xc3 | 0xc4 | 0xca | 0xcc | 0xcd | 0xd2
        | 0xd4 | 0xda | 0xdc | 0xea | 0xfa => 3,
        0x06 | 0x0e | 0x10 | 0x16 | 0x18 | 0x1e | 0x20 | 0x26 | 0x28 | 0x2e | 0x30 | 0x36
        | 0x38 | 0x3e | 0xc6 | 0xcb | 0xce | 0xd6 | 0xde | 0xe0 | 0xe6 | 0xe8 | 0xee | 0xf0
        | 0xf6 | 0xf8 | 0xfe => 2,
        _ => 1,
    }
}

#[test]
fn conditional_relative_and_prefixed_instructions_preserve_boundaries() {
    let bytes = super::tests::synthetic_rom();
    let mut code = bytes[0x8000..0xc000].to_vec();
    code[0x80..0x85].copy_from_slice(&[0x38, 2, 0xcb, 0x7f, 0xc9]);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 10000,
    };
    let instructions = closure(&code, &mut budget).unwrap();
    assert!(instructions.contains(&(0x80, 2)));
    assert!(instructions.contains(&(0x82, 2)));
    code[0x81] = 1;
    assert!(closure(&code, &mut budget).is_err());
}
