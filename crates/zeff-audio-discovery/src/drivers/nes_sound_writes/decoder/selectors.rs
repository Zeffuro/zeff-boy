use std::collections::{BTreeMap, BTreeSet};

use crate::drivers::{
    CodeCall, CodePointerDisposition, CodeSelectorConsumer, CodeSelectorControl,
    CodeSelectorPointer,
};

use super::{Budget, Call, FileSpan, Node, Nrom, ScanStop};

const MAX_CONSUMERS: usize = 16;
const ENTRY_LEN: usize = 56;

pub(super) fn find(
    nrom: &Nrom<'_>,
    nodes: &BTreeMap<u16, Node>,
    owned: &[Option<usize>],
    calls: &[Call],
    budget: &mut Budget<'_>,
) -> Result<Vec<CodeSelectorConsumer>, ScanStop> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for audio_call in calls {
        budget.charge()?;
        let Some(call_cpu_address) = audio_call.cpu_address.checked_add(3) else {
            continue;
        };
        let Some(node) = nodes.get(&call_cpu_address) else {
            continue;
        };
        if node.opcode != 0x20 {
            continue;
        }
        let Some(entry) = node.target else { continue };
        if seen.contains(&entry) {
            continue;
        }
        let Some(consumer) = inspect(nrom, nodes, owned, entry, audio_call, budget)? else {
            continue;
        };
        if result.len() == MAX_CONSUMERS {
            return Err(ScanStop::InventoryLimit);
        }
        seen.insert(entry);
        result.push(consumer);
    }
    Ok(result)
}

fn inspect(
    nrom: &Nrom<'_>,
    nodes: &BTreeMap<u16, Node>,
    owned: &[Option<usize>],
    entry: u16,
    audio_call: &Call,
    budget: &mut Budget<'_>,
) -> Result<Option<CodeSelectorConsumer>, ScanStop> {
    let Some(entry_span) = span(nrom, entry, ENTRY_LEN) else {
        return Ok(None);
    };
    let start = entry_span.offset as usize - super::PRG_START;
    let b = &nrom.prg()[start..start + ENTRY_LEN];
    let selector = b[1];
    let c1 = b[6];
    let c2 = b[14];
    let v1 = b[10];
    let v2 = b[18];
    let bound = b[32];
    let pointer = b[44];
    let header = b[55];
    let action = u16::from_le_bytes([b[20], b[21]]);
    let table = u16::from_le_bytes([b[41], b[42]]);
    let Some(table_high) = table.checked_add(1) else {
        return Ok(None);
    };
    let Some(pointer_high) = pointer.checked_add(1) else {
        return Ok(None);
    };
    if !(4..=128).contains(&bound)
        || c1 == 0
        || c2 == 0
        || c1 == c2
        || c1 >= bound
        || c2 >= bound
        || v1 & 0x80 == 0
        || action >= 0x800
    {
        return Ok(None);
    }
    let expected = [
        0xa6,
        selector,
        0xd0,
        1,
        0x60,
        0xe0,
        c1,
        0xd0,
        4,
        0xa9,
        v1,
        0x30,
        6,
        0xe0,
        c2,
        0xd0,
        10,
        0xa9,
        v2,
        0x8d,
        b[20],
        b[21],
        0xa9,
        0,
        0x85,
        selector,
        0x60,
        0xa6,
        selector,
        0x30,
        5,
        0xe0,
        bound,
        0x90,
        1,
        0x60,
        0xca,
        0x8a,
        0x0a,
        0xa8,
        0xb9,
        b[41],
        b[42],
        0x85,
        pointer,
        0xb9,
        table_high as u8,
        (table_high >> 8) as u8,
        0x85,
        pointer_high,
        0xa0,
        0,
        0xb1,
        pointer,
        0x85,
        header,
    ];
    for (&actual, expected) in b.iter().zip(expected) {
        budget.charge()?;
        if actual != expected {
            return Ok(None);
        }
    }
    let mut at = 0;
    while at < ENTRY_LEN {
        budget.charge()?;
        let Some(node) = nodes.get(&(entry + at as u16)) else {
            return Ok(None);
        };
        at += usize::from(node.length);
    }
    if at != ENTRY_LEN {
        return Ok(None);
    }
    let aperture_len = usize::from(bound - 1) * 2;
    let Some(pointer_aperture) = span(nrom, table, aperture_len) else {
        return Ok(None);
    };
    let table_offset = pointer_aperture.offset as usize - super::PRG_START;
    if owned[table_offset..table_offset + aperture_len]
        .iter()
        .any(Option::is_some)
    {
        return Ok(None);
    }
    let mut pointers = Vec::new();
    for raw_selector in 1..bound {
        budget.charge()?;
        if raw_selector == c1 || raw_selector == c2 {
            continue;
        }
        let offset = table_offset + usize::from(raw_selector - 1) * 2;
        let target = u16::from_le_bytes(nrom.prg()[offset..offset + 2].try_into().unwrap());
        let target_span = span(nrom, target, 1);
        let disposition = match nrom.offset(target) {
            None => CodePointerDisposition::Unmapped,
            Some(offset) if owned[offset].is_some() => CodePointerDisposition::DecodedCode,
            Some(offset) if (table_offset..table_offset + aperture_len).contains(&offset) => {
                CodePointerDisposition::PointerTable
            }
            Some(_) => CodePointerDisposition::Unparsed,
        };
        pointers.push(CodeSelectorPointer {
            raw_selector,
            entry_span: nrom.span(offset, 2),
            target_cpu_address: target,
            target_span,
            disposition,
        });
    }
    let call_cpu_address = audio_call
        .cpu_address
        .checked_add(3)
        .expect("adjacent decoded call");
    Ok(Some(CodeSelectorConsumer {
        records: Vec::new(),
        audio_call: CodeCall {
            cpu_address: audio_call.cpu_address,
            target_cpu_address: audio_call.target_cpu_address,
            span: audio_call.span,
            writer_cpu_address: audio_call.writer_cpu_address,
            writer_span: audio_call.writer_span,
        },
        call_cpu_address,
        call_span: span(nrom, call_cpu_address, 3).expect("decoded caller"),
        entry_cpu_address: entry,
        entry_span,
        selector_address: selector,
        upper_bound_exclusive: bound,
        pointer_address: pointer,
        header_address: header,
        table_cpu_address: table,
        pointer_aperture,
        control_address: action,
        controls: vec![
            CodeSelectorControl {
                raw_selector: c1,
                value: v1,
            },
            CodeSelectorControl {
                raw_selector: c2,
                value: v2,
            },
        ],
        pointers,
    }))
}

fn span(nrom: &Nrom<'_>, address: u16, len: usize) -> Option<FileSpan> {
    let start = nrom.offset(address)?;
    let last = address.checked_add(u16::try_from(len.checked_sub(1)?).ok()?)?;
    (start.checked_add(len)? <= nrom.prg_len && nrom.offset(last)? == start + len - 1)
        .then(|| nrom.span(start, len))
}
