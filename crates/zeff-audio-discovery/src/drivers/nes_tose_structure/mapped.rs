use crate::{Budget, ScanStop};

use super::layout::{Engine, Kind, Layout, instruction_length, rom_operand_is_mapped, word};

#[cfg(test)]
mod tests;

struct Contract {
    init: Vec<u8>,
    selector_len: usize,
    table_bytes: [usize; 2],
    mask_operand: usize,
    selector_hash: &'static str,
    masks: [u8; 4],
}

fn contract(kind: Kind) -> Contract {
    let (ram, counter, mask) = if kind == Kind::AccumulatorPointer {
        (0x20, 0xd0, 0xcc)
    } else {
        (0, 0xb0, 0xac)
    };
    let init = if kind == Kind::YPointerHelper {
        vec![
            0xa0, 0xff, 0x8c, 0xb1, 4, 0xc8, 0x8c, 0x15, 0x40, 0x8c, 0xac, 4, 0x8c, 0xb0, 4, 0xa9,
            0xff, 0x99, 0, 4, 0x98, 0x18, 0x69, 0x15, 0xa8, 0xc9, 0xa8, 0xd0, 0xf2, 0x60,
        ]
    } else {
        vec![
            0xa0, 0, 0x8c, counter, 7, 0x8c, 0x15, 0x40, 0x8c, mask, 7, 0xa9, 0xff, 0x99, ram, 7,
            0x98, 0x18, 0x69, 0x15, 0xa8, 0xc9, 0xa8, 0xd0, 0xf2, 0x60,
        ]
    };
    let (selector_len, table_bytes, mask_operand, selector_hash, masks) = match kind {
        Kind::AccumulatorPointer => (
            75,
            [12, 16],
            40,
            "72d2ea64be23885f595d11abaaf8fd63958e985e7c6a7b6078af6207a171bfdd",
            [14, 13, 11, 7],
        ),
        Kind::YPointerHelper => (
            85,
            [18, 25],
            47,
            "0914859638b493613874139ff4cbc6b02cd535fadd0eca951036140e73d37b37",
            [30, 29, 27, 23],
        ),
        Kind::VariableSlotLimit => (
            75,
            [12, 16],
            40,
            "631aea0460d59563c865bb0a2a6b1b548a7e8ec11b371fcab8c959b5c2cb3906",
            [14, 13, 11, 7],
        ),
        _ => unreachable!(),
    };
    Contract {
        init,
        selector_len,
        table_bytes,
        mask_operand,
        selector_hash,
        masks,
    }
}

pub(super) fn resolve(
    bank: &[u8],
    engine: Engine,
    budget: &mut Budget<'_>,
) -> Result<Option<Layout>, ScanStop> {
    let contract = contract(engine.kind);
    let mut init = None;
    let mut selector = None;
    for at in 0..bank.len() {
        if at.is_multiple_of(64) {
            budget.charge()?;
        }
        if bank.get(at..at + contract.init.len()) == Some(contract.init.as_slice())
            && init.replace((at, at + contract.init.len())).is_some()
        {
            return Ok(None);
        }
        let seed = if engine.kind == Kind::YPointerHelper {
            0x84
        } else {
            0xa0
        };
        if bank[at] == seed {
            budget.charge()?;
            if let Some(found) = selector_at(bank, at, &engine, &contract)
                && selector.replace(found).is_some()
            {
                return Ok(None);
            }
        }
    }
    let (Some(init), Some((start, table, masks))) = (init, selector) else {
        return Ok(None);
    };
    if table < engine.end || table + 1024 > bank.len() {
        return Ok(None);
    }
    Ok(Some(Layout {
        init,
        selector: (start, start + contract.selector_len),
        selector_data: Some((masks, masks + 4)),
        table,
        selectors: 256,
        engine,
    }))
}

fn selector_at(
    bank: &[u8],
    at: usize,
    engine: &Engine,
    contract: &Contract,
) -> Option<(usize, usize, usize)> {
    let mut code = bank.get(at..at + contract.selector_len)?.to_vec();
    let table = usize::from(code[contract.table_bytes[0]])
        | usize::from(code[contract.table_bytes[1]]) << 8;
    let masks = word(&code, contract.mask_operand).checked_sub(engine.cpu_base)?;
    if bank.get(masks..masks + 4) != Some(&contract.masks) {
        return None;
    }
    for position in contract.table_bytes {
        code[position] = 0;
    }
    let mut pc = 0;
    while pc < code.len() {
        let len = instruction_length(code[pc])?;
        if pc + len > code.len() {
            return None;
        }
        if len == 3 {
            let address = word(&code, pc + 1);
            if address >= 0x8000 {
                if !rom_operand_is_mapped(code[pc], address, engine.cpu_base, bank.len()) {
                    return None;
                }
                code[pc + 1..pc + 3].copy_from_slice(
                    &(address as u16)
                        .wrapping_sub((engine.cpu_base + engine.tick) as u16)
                        .to_le_bytes(),
                );
            }
        }
        pc += len;
    }
    (zeff_firmware::sha256_hex(&code) == contract.selector_hash).then_some((
        at,
        table.checked_sub(engine.cpu_base)?,
        masks,
    ))
}
