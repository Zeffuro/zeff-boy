use crate::{Budget, ScanStop};

use super::word;

mod data;
use data::PROFILES;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HeaderKind {
    Executable,
    BankTagged {
        prefix: [u8; 3],
        banks: &'static [u8],
    },
}

pub(super) struct Profile {
    pub name: &'static str,
    pub header: HeaderKind,
    pub len: usize,
    pub hash: &'static str,
    pub selector: u16,
    pub tick: u16,
    pub stride: u16,
    pub pattern_bytes: u32,
    pub operands: [usize; 5],
    pub aliases: &'static [&'static [usize]],
    pub empty_operand: Option<usize>,
    pub default_empty: u16,
    pub release_operand: usize,
}

impl Profile {
    fn header_matches(&self, code: &[u8], bank: u16) -> bool {
        match self.header {
            HeaderKind::Executable => code
                .first()
                .is_some_and(|&byte| matches!(byte, 0x21 | 0xc3)),
            HeaderKind::BankTagged { prefix, banks } => {
                (1..=u16::from(u8::MAX)).contains(&bank)
                    && banks.contains(&(bank as u8))
                    && code.first() == Some(&(bank as u8))
                    && code.get(1..3) == Some(self.selector.to_le_bytes().as_slice())
                    && code.get(3..6) == Some(prefix.as_slice())
            }
        }
    }

    fn code_offset(&self) -> usize {
        match self.header {
            HeaderKind::Executable => 0,
            HeaderKind::BankTagged { .. } => 6,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Driver {
    pub profile: &'static Profile,
    pub bank: u16,
    pub wram: u16,
    pub table: u16,
    pub sequences: u16,
    pub instruments: u16,
    pub noise: u16,
    pub frequency: u16,
    pub empty: u16,
    pub release: u16,
}

impl Driver {
    pub fn offset(self, pointer: u32) -> usize {
        usize::from(self.bank) * 0x4000 + pointer as usize - 0x4000
    }
}

pub(super) fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Vec<Driver>, ScanStop> {
    budget.charge()?;
    let mut drivers = Vec::new();
    let isolated_bank = if super::supports_cartridge(bytes) {
        None
    } else if let Some(bank) = super::isolated::source_bank(bytes, budget)? {
        Some(bank)
    } else {
        return Ok(drivers);
    };
    for bank in 1..bytes.len() / 0x4000 {
        budget.charge()?;
        if isolated_bank.is_some_and(|qualified| usize::from(qualified) != bank) {
            continue;
        }
        let base = bank * 0x4000;
        for profile in PROFILES {
            if isolated_bank.is_some() && profile.name != "gb-quickthunder-14-05" {
                continue;
            }
            if !profile.header_matches(&bytes[base..], bank as u16) {
                continue;
            }
            if let Some(driver) = recognize(bytes, bank as u16, profile, budget)? {
                drivers.push(driver);
                break;
            }
        }
    }
    if let Some(driver) = super::sampled::recognize(bytes, budget)? {
        drivers.push(driver);
    }
    #[cfg(any(test, feature = "test-support"))]
    {
        super::tests::recognize(bytes, &mut drivers);
        super::sampled::tests::recognize(bytes, &mut drivers);
    }
    Ok(drivers)
}

pub(super) fn recognize(
    bytes: &[u8],
    bank: u16,
    profile: &'static Profile,
    budget: &mut Budget<'_>,
) -> Result<Option<Driver>, ScanStop> {
    let base = usize::from(bank) * 0x4000;
    let code = &bytes[base..base + profile.len];
    if !profile.header_matches(code, bank) {
        return Ok(None);
    }
    let tick_offset = usize::from(profile.tick - 0x4000);
    if code.get(tick_offset) != Some(&0x21)
        || code.get(tick_offset + 3..tick_offset + 6) != Some(&[0x35, 0x28, 3])
    {
        return Ok(None);
    }
    let wram = word(code, tick_offset + 1);
    if !(0xc000..=0xdf80).contains(&wram) {
        return Ok(None);
    }
    let mut normalized = code.to_vec();
    if matches!(profile.header, HeaderKind::BankTagged { .. }) {
        normalized[0] = 0;
    }
    let mut at = profile.code_offset();
    while at < code.len() {
        budget.charge()?;
        let op = code[at];
        let size = instruction_length(op);
        if at + size > code.len() {
            return Ok(None);
        }
        if size == 3 {
            let value = word(code, at + 1);
            if matches!(op, 0x01 | 0x11 | 0x21) && (0x4000..0x8000).contains(&value) {
                normalized[at + 1..at + 3].fill(0);
            } else if matches!(op, 0x01 | 0x11 | 0x21 | 0xea | 0xfa)
                && (wram..wram + 128).contains(&value)
            {
                normalized[at + 1..at + 3].copy_from_slice(&(value - wram).to_le_bytes());
            }
        }
        at += size;
    }
    if zeff_firmware::sha256_hex(&normalized) != profile.hash {
        return Ok(None);
    }
    for operands in profile.aliases {
        if operands
            .iter()
            .any(|&at| word(code, at) != word(code, operands[0]))
        {
            return Ok(None);
        }
    }
    let [table, sequences, instruments, noise, frequency] =
        profile.operands.map(|at| word(code, at));
    if frequency < 0x4007 {
        return Ok(None);
    }
    let driver = Driver {
        profile,
        bank,
        wram,
        table,
        sequences,
        instruments,
        noise,
        frequency,
        empty: profile
            .empty_operand
            .map_or(profile.default_empty, |at| word(code, at)),
        release: word(code, profile.release_operand),
    };
    if [
        driver.table,
        driver.sequences,
        driver.instruments,
        driver.noise,
        driver.frequency,
        driver.empty,
        driver.release,
    ]
    .into_iter()
    .any(|pointer| !(0x4000..0x8000).contains(&pointer))
    {
        return Ok(None);
    }
    Ok(Some(driver))
}

fn instruction_length(op: u8) -> usize {
    match op {
        0x01 | 0x08 | 0x11 | 0x21 | 0x31 | 0xc2 | 0xc3 | 0xc4 | 0xca | 0xcc | 0xcd | 0xd2
        | 0xd4 | 0xda | 0xdc | 0xea | 0xfa => 3,
        0x06 | 0x0e | 0x10 | 0x16 | 0x18 | 0x1e | 0x20 | 0x26 | 0x28 | 0x2e | 0x30 | 0x36
        | 0x3e | 0xc6 | 0xcb | 0xce | 0xd6 | 0xde | 0xe0 | 0xe6 | 0xe8 | 0xee | 0xf0 | 0xf6
        | 0xf8 | 0xfe => 2,
        _ => 1,
    }
}
