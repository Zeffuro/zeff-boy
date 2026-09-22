use std::collections::BTreeMap;

use crate::{Budget, ScanStop};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Pointer,
    IndexedPointer,
    IndexedAbsolute,
    AccumulatorPointer,
    YPointerHelper,
    VariableSlotLimit,
    #[cfg(any(test, feature = "test-support"))]
    Synthetic,
}

pub(super) struct Engine {
    pub cpu_base: usize,
    pub kind: Kind,
    pub tick: usize,
    pub end: usize,
    pub code_ranges: Vec<(usize, usize)>,
    pub writers: Vec<usize>,
    pub routing: Option<(usize, usize)>,
}

pub(super) struct Layout {
    pub engine: Engine,
    pub init: (usize, usize),
    pub selector: (usize, usize),
    pub table: usize,
    pub selectors: u16,
    pub selector_data: Option<(usize, usize)>,
}

impl Layout {
    pub fn is_code(&self, address: usize) -> bool {
        self.engine
            .code_ranges
            .iter()
            .any(|range| (range.0..range.1).contains(&address))
            || (self.init.0..self.init.1).contains(&address)
            || (self.selector.0..self.selector.1).contains(&address)
            || self
                .selector_data
                .is_some_and(|range| (range.0..range.1).contains(&address))
            || self
                .engine
                .routing
                .is_some_and(|range| (range.0..range.1).contains(&address))
    }
}

pub(super) fn engine(
    bank: &[u8],
    tick: usize,
    cpu_base: usize,
    budget: &mut Budget<'_>,
) -> Result<Option<Engine>, ScanStop> {
    let mut pending = vec![tick];
    let mut nodes = BTreeMap::new();
    let mut owned = vec![false; bank.len()];
    while let Some(pc) = pending.pop() {
        budget.charge()?;
        if nodes.contains_key(&pc) {
            continue;
        }
        if nodes.len() >= 512 || pc < tick || pc >= bank.len() || owned[pc] {
            return Ok(None);
        }
        let Some(len) = instruction_length(bank[pc]) else {
            return Ok(None);
        };
        let Some(bytes) = bank.get(pc..pc + len) else {
            return Ok(None);
        };
        if owned[pc..pc + len].iter().any(|value| *value) {
            return Ok(None);
        }
        owned[pc..pc + len].fill(true);
        nodes.insert(pc, bytes);
        let op = bytes[0];
        if op == 0x60 {
            continue;
        }
        if matches!(op, 0x20 | 0x4c) {
            let Some(target) = word(bytes, 1).checked_sub(cpu_base) else {
                return Ok(None);
            };
            pending.push(target);
            if op == 0x4c {
                continue;
            }
        } else if op & 0x1f == 0x10 {
            let Some(target) = (pc + 2).checked_add_signed(isize::from(bytes[1] as i8)) else {
                return Ok(None);
            };
            pending.push(target);
        }
        pending.push(pc + len);
    }
    let mut normalized = Vec::new();
    let mut writers = Vec::new();
    let mut end = tick;
    let mut code_ranges: Vec<(usize, usize)> = Vec::new();
    for (&pc, bytes) in &nodes {
        budget.charge()?;
        normalized.extend_from_slice(&((pc - tick) as u16).to_le_bytes());
        normalized.push(bytes.len() as u8);
        let mut instruction = bytes.to_vec();
        if bytes.len() == 3 {
            let address = word(bytes, 1);
            if address >= 0x8000 {
                if !rom_operand_is_mapped(bytes[0], address, cpu_base, bank.len()) {
                    return Ok(None);
                }
                instruction[1..].copy_from_slice(
                    &(address as u16)
                        .wrapping_sub((cpu_base + tick) as u16)
                        .to_le_bytes(),
                );
            }
            if matches!(bytes[0], 0x8d | 0x99)
                && matches!(address, 0x4000..=0x4013 | 0x4015 | 0x4017)
            {
                writers.push(pc);
            }
        }
        normalized.extend(instruction);
        end = end.max(pc + bytes.len());
        if let Some(range) = code_ranges.last_mut()
            && range.1 == pc
        {
            range.1 = pc + bytes.len();
        } else {
            code_ranges.push((pc, pc + bytes.len()));
        }
    }
    let hash = zeff_firmware::sha256_hex(&normalized);
    let kind = match hash.as_str() {
        "5bd19fe58d49aee2b424940ce8f093131f8502429f620dd5ca7caebba348f57b" => Kind::Pointer,
        "8d254da347d93661ccbfcc81797d2c8cc814c367743be4c31c949996c02b1ec2" => Kind::IndexedPointer,
        "6ed0a31e9b454a5710c9f03fc8841e30ed1a9b2f9263b8eabf23cedc78e9d967" => Kind::IndexedAbsolute,
        "261f6eb1fbbbd314a9eccbc36f050b56dd756a45ed227b262ce1726c23c7ce7a" => {
            Kind::AccumulatorPointer
        }
        "062462a07f691448ad3db2d05138ac85433dd8eafada0b3b6d9b851fd1c2c326" => Kind::YPointerHelper,
        "5781bd1cbb450663d749a8cbd84d4959f1fab00715d5cf004ca2b86a8fb04725" => {
            Kind::VariableSlotLimit
        }
        #[cfg(any(test, feature = "test-support"))]
        value if value == super::tests::engine_hash() => Kind::Synthetic,
        _ => return Ok(None),
    };
    let routing = match kind {
        Kind::Pointer => Some(tick + 0x40f),
        Kind::IndexedPointer => Some(tick + 0x414),
        Kind::IndexedAbsolute => Some(tick + 0x412),
        Kind::AccumulatorPointer => Some(tick + 0x40f),
        Kind::YPointerHelper => Some(tick + 0x420),
        Kind::VariableSlotLimit => Some(tick + 0x410),
        #[cfg(any(test, feature = "test-support"))]
        Kind::Synthetic => Some(tick + 0x80),
    };
    let masks = if kind == Kind::YPointerHelper {
        [30, 29, 27, 23]
    } else {
        [14, 13, 11, 7]
    };
    if let Some(at) = routing
        && bank.get(at..at + 12)
            != Some(&[
                1, 2, 4, 8, masks[0], masks[1], masks[2], masks[3], 0, 1, 0x82, 0x43,
            ])
    {
        return Ok(None);
    }
    Ok(Some(Engine {
        cpu_base,
        kind,
        tick,
        end,
        code_ranges,
        writers,
        routing: routing.map(|at| (at, at + 12)),
    }))
}

pub(super) fn rom_operand_is_mapped(op: u8, address: usize, base: usize, width: usize) -> bool {
    let indexed = matches!(
        op,
        0x19 | 0x1d | 0x1e | 0x7d | 0x7e | 0x99 | 0x9d | 0xb9 | 0xbd | 0xdd | 0xde | 0xfd | 0xfe
    );
    (base..base + width).contains(&address) && (!indexed || address + 255 < base + width)
}

pub(super) fn resolve(
    bank: &[u8],
    engine: Engine,
    budget: &mut Budget<'_>,
) -> Result<Option<Layout>, ScanStop> {
    if matches!(
        engine.kind,
        Kind::AccumulatorPointer | Kind::YPointerHelper | Kind::VariableSlotLimit
    ) {
        return super::mapped::resolve(bank, engine, budget);
    }
    #[cfg(any(test, feature = "test-support"))]
    if engine.kind == Kind::Synthetic {
        return super::tests::resolve(bank, engine, budget);
    }
    let mut init = None;
    let mut selector = None;
    for at in 0..bank.len() {
        if at % 64 == 0 {
            budget.charge()?;
        }
        if bank[at] == 0xa0
            && init_matches(bank, at, engine.kind)
            && init.replace((at, at + 26)).is_some()
        {
            return Ok(None);
        }
        let seed = match engine.kind {
            Kind::Pointer => bank[at] == 0xa5,
            Kind::IndexedPointer => bank[at] == 0x85,
            Kind::IndexedAbsolute => bank[at] == 0x0a,
            Kind::AccumulatorPointer | Kind::YPointerHelper | Kind::VariableSlotLimit => {
                unreachable!()
            }
            #[cfg(any(test, feature = "test-support"))]
            Kind::Synthetic => unreachable!(),
        };
        if seed {
            budget.charge()?;
            if let Some(found) = selector_at(bank, at, &engine)
                && selector.replace(found).is_some()
            {
                return Ok(None);
            }
        }
    }
    let (Some(mut init), Some((start, end, table))) = (init, selector) else {
        return Ok(None);
    };
    if engine.kind == Kind::IndexedPointer {
        if init.0 < 2
            || bank.get(init.0 - 2..init.0) != Some(&[0x98, 0x48])
            || bank.get(init.1 - 1..init.1 + 2) != Some(&[0x68, 0xa8, 0x60])
        {
            return Ok(None);
        }
        init = (init.0 - 2, init.1 + 2);
    }
    let selectors = if engine.kind == Kind::Pointer {
        256
    } else {
        128
    };
    if table < engine.end || table + usize::from(selectors) * 4 > bank.len() {
        return Ok(None);
    }
    Ok(Some(Layout {
        engine,
        init,
        selector: (start, end),
        table,
        selectors,
        selector_data: None,
    }))
}

fn init_matches(bank: &[u8], at: usize, kind: Kind) -> bool {
    let Some(code) = bank.get(at..at + 26) else {
        return false;
    };
    let (counter, mask) = if kind == Kind::Pointer {
        (0xb0, 0xac)
    } else {
        (0xaf, 0xab)
    };
    let last = if kind == Kind::IndexedPointer {
        0x68
    } else {
        0x60
    };
    code == [
        0xa0, 0, 0x8c, counter, 7, 0x8c, 0x15, 0x40, 0x8c, mask, 7, 0xa9, 0xff, 0x99, 0, 7, 0x98,
        0x18, 0x69, 0x15, 0xa8, 0xc9, 0xa8, 0xd0, 0xf2, last,
    ]
}

fn selector_at(bank: &[u8], at: usize, engine: &Engine) -> Option<(usize, usize, usize)> {
    let (start, len, table, zero, expected) = match engine.kind {
        Kind::Pointer => {
            let code = bank.get(at..at + 73)?;
            if code[..6] != [0xa5, 0xb2, 0xa0, 0, 0x84, 0xb2] {
                return None;
            }
            (
                at,
                73,
                usize::from(code[14]) | usize::from(code[18]) << 8,
                Some([14, 18]),
                "05b41dc9c02744846d120e066e55bab509730ac45161e80c2c7daa54efabe2f0",
            )
        }
        Kind::IndexedPointer => {
            let code = bank.get(at..at + 83)?;
            if code[..5] != [0x85, 0xb9, 0x98, 0x48, 0xa9] {
                return None;
            }
            (
                at,
                83,
                usize::from(code[5]) | usize::from(code[9]) << 8,
                Some([5, 9]),
                "c9e93942bd8aca53c41b70c0be32b0ed90cc77eaf26d8e992833475507cb3947",
            )
        }
        Kind::IndexedAbsolute => {
            let code = bank.get(at..at + 62)?;
            if code[..7] != [0x0a, 0x0a, 0xb0, 0xc2, 0xa8, 0x8a, 0x48] {
                return None;
            }
            (
                at.checked_sub(58)?,
                120,
                word(code, 8),
                None,
                "6b1ea403c55947652eab27b53447c90723f8f8470532962810d763f0eb563b66",
            )
        }
        Kind::AccumulatorPointer | Kind::YPointerHelper | Kind::VariableSlotLimit => return None,
        #[cfg(any(test, feature = "test-support"))]
        Kind::Synthetic => return None,
    };
    if !(engine.cpu_base..engine.cpu_base + bank.len()).contains(&table) {
        return None;
    }
    let mut code = bank.get(start..start + len)?.to_vec();
    if let Some(positions) = zero {
        for position in positions {
            code[position] = 0;
        }
    }
    let mut pc = 0;
    while pc < code.len() {
        let len = instruction_length(code[pc])?;
        if pc + len > code.len() {
            return None;
        }
        if len == 3 {
            let address = word(&code, pc + 1);
            let normalized = if engine.kind == Kind::IndexedAbsolute
                && (table..=table + 259).contains(&address)
            {
                Some((address - table) as u16)
            } else if address >= 0x8000 {
                if !rom_operand_is_mapped(code[pc], address, engine.cpu_base, bank.len()) {
                    return None;
                }
                Some((address as u16).wrapping_sub((engine.cpu_base + engine.tick) as u16))
            } else {
                None
            };
            if let Some(value) = normalized {
                code[pc + 1..pc + 3].copy_from_slice(&value.to_le_bytes());
            }
        }
        pc += len;
    }
    (zeff_firmware::sha256_hex(&code) == expected).then_some((
        start,
        start + len,
        table - engine.cpu_base,
    ))
}

pub(super) fn word(bytes: &[u8], at: usize) -> usize {
    usize::from(bytes[at]) | usize::from(bytes[at + 1]) << 8
}

pub(super) fn instruction_length(op: u8) -> Option<usize> {
    Some(match op {
        0x0a | 0x18 | 0x38 | 0x48 | 0x4a | 0x60 | 0x68 | 0x6a | 0x88 | 0x8a | 0x98 | 0xa8
        | 0xaa => 1,
        0x09 | 0x10 | 0x11 | 0x26 | 0x29 | 0x30 | 0x49 | 0x65 | 0x69 | 0x70 | 0x84 | 0x85
        | 0x90 | 0xa0 | 0xa4 | 0xa5 | 0xa9 | 0xb0 | 0xb1 | 0xc0 | 0xc9 | 0xd0 | 0xe6 | 0xf0 => 2,
        0x0d | 0x19 | 0x1d | 0x1e | 0x20 | 0x2c | 0x2d | 0x2e | 0x4c | 0x4e | 0x6d | 0x6e
        | 0x7d | 0x7e | 0x8c | 0x8d | 0x99 | 0x9d | 0xac | 0xad | 0xb9 | 0xbd | 0xdd | 0xde
        | 0xcd | 0xee | 0xfd | 0xfe => 3,
        0xc8 | 0x2a => 1,
        0x06 | 0x46 | 0x66 | 0x86 | 0xa6 | 0xe5 | 0xe9 => 2,
        _ => return None,
    })
}

#[cfg(test)]
mod mapped_operand_tests {
    use super::rom_operand_is_mapped;

    #[test]
    fn indexed_operands_keep_their_entire_index_range_in_the_source_page() {
        for op in [0x19, 0x1d, 0x7d, 0xb9, 0xbd, 0xdd, 0xfd] {
            assert!(rom_operand_is_mapped(op, 0x9f00, 0x8000, 0x2000));
            assert!(!rom_operand_is_mapped(op, 0x9f01, 0x8000, 0x2000));
            assert!(!rom_operand_is_mapped(op, 0xa000, 0x8000, 0x2000));
        }
        assert!(rom_operand_is_mapped(0xad, 0x9fff, 0x8000, 0x2000));
        assert!(!rom_operand_is_mapped(0xad, 0xa000, 0x8000, 0x2000));
    }
}
