use std::collections::BTreeMap;

use crate::drivers::{
    CodeConditionalHeadCommandEdge, CodePointerDisposition, CodeSelectorConsumer, EvidenceKind,
};

use super::{Budget, FileSpan, Node, Nrom, ScanStop, records};

mod source;
use source::*;

#[cfg(test)]
mod conflict_tests;

const MAX_EDGES: usize = 48;

pub(super) fn find(
    nrom: &Nrom<'_>,
    nodes: &BTreeMap<u16, Node>,
    owned: &[Option<usize>],
    consumers: &mut [CodeSelectorConsumer],
    budget: &mut Budget<'_>,
) -> Result<(), ScanStop> {
    let protected = protected(consumers, budget)?;
    let mut staged = Vec::new();
    for (consumer_index, consumer) in consumers.iter().enumerate() {
        if !consumer
            .records
            .iter()
            .flat_map(|record| &record.streams)
            .any(|stream| stream.fetch_binding.is_some())
        {
            continue;
        }
        let Some(pairs) = records::triple_state_pairs(nrom, nodes, owned, consumer, budget)? else {
            continue;
        };
        for (record_index, record) in consumer.records.iter().enumerate() {
            if record.header & 0x80 == 0 || record.streams.len() != 3 {
                continue;
            }
            for (stream_index, stream) in record.streams.iter().enumerate() {
                budget.charge()?;
                if stream.disposition != CodePointerDisposition::Unparsed {
                    continue;
                }
                let Some(binding) = &stream.fetch_binding else {
                    continue;
                };
                let Some(head_span) = span(nrom, stream.target_cpu_address, 1) else {
                    continue;
                };
                if overlaps_any(head_span, &protected.heads, budget)?
                    || !unowned(nrom, owned, head_span, budget)?
                {
                    continue;
                }
                let head = nrom.bytes[head_span.offset as usize];
                if head & 0x80 == 0 {
                    continue;
                }
                let Some(dispatch) =
                    dispatch(nrom, nodes, owned, binding.fetch_cpu_address, budget)?
                else {
                    continue;
                };
                let request = BridgeRequest {
                    scheduler: binding.scheduler_cpu_address,
                    entry: binding.consumer_entry_cpu_address,
                    fetch: binding.fetch_cpu_address,
                    dispatch: &dispatch,
                    pairs: &pairs,
                    stream: stream_index,
                };
                let Some(bridge) = bridge(nrom, nodes, owned, request, budget)? else {
                    continue;
                };
                let Some(row_address) = dispatch.table.checked_add(u16::from(head.wrapping_mul(2)))
                else {
                    continue;
                };
                let Some(row_span) = span(nrom, row_address, 2) else {
                    continue;
                };
                if overlaps_any(row_span, &protected.all, budget)?
                    || !unowned(nrom, owned, row_span, budget)?
                {
                    continue;
                }
                let offset = row_span.offset as usize - super::PRG_START;
                let handler_address = u16::from_le_bytes(
                    nrom.prg()[offset..offset + 2]
                        .try_into()
                        .expect("two-byte row"),
                );
                let Some(edge) = handler(
                    nrom,
                    owned,
                    HandlerInput {
                        address: handler_address,
                        dispatch,
                        bridge,
                        head,
                        head_span,
                        row_span,
                    },
                    &protected.all,
                    budget,
                )?
                else {
                    continue;
                };
                if staged.len() == MAX_EDGES {
                    return Err(ScanStop::InventoryLimit);
                }
                staged.push((consumer_index, record_index, stream_index, edge));
            }
        }
    }
    let mut held = vec![false; staged.len()];
    for (index, (_, _, _, edge)) in staged.iter().enumerate() {
        for (other_index, (_, _, _, other)) in staged[..index].iter().enumerate() {
            budget.charge()?;
            if conflict(edge, other) {
                held[index] = true;
                held[other_index] = true;
            }
        }
    }
    for ((consumer, record, stream, edge), held) in staged.into_iter().zip(held) {
        if held {
            continue;
        }
        consumers[consumer].records[record].streams[stream].conditional_head_command_edge =
            Some(edge)
    }
    Ok(())
}

fn conflict(a: &CodeConditionalHeadCommandEdge, b: &CodeConditionalHeadCommandEdge) -> bool {
    overlaps(a.dispatch_row_span, b.handler_span)
        || overlaps(a.handler_span, b.dispatch_row_span)
        || (a.dispatch_row_span != b.dispatch_row_span
            && overlaps(a.dispatch_row_span, b.dispatch_row_span))
        || (a.handler_span != b.handler_span && overlaps(a.handler_span, b.handler_span))
}

struct Protected {
    heads: Vec<FileSpan>,
    all: Vec<FileSpan>,
}
fn protected(
    consumers: &[CodeSelectorConsumer],
    budget: &mut Budget<'_>,
) -> Result<Protected, ScanStop> {
    let mut heads = Vec::new();
    let mut all = Vec::new();
    for consumer in consumers {
        budget.charge()?;
        heads.push(consumer.pointer_aperture);
        all.push(consumer.pointer_aperture);
        for record in &consumer.records {
            budget.charge()?;
            heads.push(record.prefix_span);
            all.push(record.prefix_span);
            for stream in &record.streams {
                budget.charge()?;
                all.push(stream.entry_span);
                if let Some(span) = stream.target_span {
                    all.push(span)
                }
            }
        }
    }
    Ok(Protected { heads, all })
}

struct Dispatch {
    table: u16,
    source: u8,
    target: u8,
    saved: u8,
    span: FileSpan,
}
fn dispatch(
    nrom: &Nrom<'_>,
    nodes: &BTreeMap<u16, Node>,
    owned: &[Option<usize>],
    fetch_address: u16,
    budget: &mut Budget<'_>,
) -> Result<Option<Dispatch>, ScanStop> {
    let Some(fetch) = graph_instruction(nrom, nodes, owned, fetch_address, budget)? else {
        return Ok(None);
    };
    let Some(guard_address) = fetch_address.checked_add(2) else {
        return Ok(None);
    };
    let Some(guard) = graph_instruction(nrom, nodes, owned, guard_address, budget)? else {
        return Ok(None);
    };
    if fetch.opcode != 0xb1
        || fetch.byte() == 0xff
        || guard.opcode != 0x10
        || guard.branch().is_none()
    {
        return Ok(None);
    }
    let mut at = guard_address.checked_add(2);
    let (index, call) = loop {
        let Some(address) = at else { return Ok(None) };
        let Some(node) = graph_instruction(nrom, nodes, owned, address, budget)? else {
            return Ok(None);
        };
        match node.opcode {
            0x8d if (0x0100..=0x07ff).contains(&node.word()) => at = address.checked_add(3),
            0xa6 => {
                let Some(call_address) = address.checked_add(2) else {
                    return Ok(None);
                };
                let Some(call) = graph_instruction(nrom, nodes, owned, call_address, budget)?
                else {
                    return Ok(None);
                };
                break (node, call);
            }
            _ => return Ok(None),
        }
    };
    if call.opcode != 0x20 {
        return Ok(None);
    }
    let Some(block) = graph_block(
        nrom,
        nodes,
        owned,
        call.word(),
        &[0x0a, 0xa8, 0xb9, 0x85, 0xb9, 0x85, 0xa0, 0xa6, 0x6c],
        budget,
    )?
    else {
        return Ok(None);
    };
    let table = block.word(2);
    let target = block.byte(3);
    let Some(source_high) = fetch.byte().checked_add(1) else {
        return Ok(None);
    };
    let Some(table_high) = table.checked_add(1) else {
        return Ok(None);
    };
    let Some(target_high) = target.checked_add(1) else {
        return Ok(None);
    };
    if block.word(4) != table_high
        || block.byte(5) != target_high
        || block.byte(6) != 0
        || block.word(8) != u16::from(target)
        || index.byte() != block.byte(7)
        || !distinct(&[
            fetch.byte(),
            source_high,
            target,
            target_high,
            block.byte(7),
        ])
    {
        return Ok(None);
    }
    Ok(Some(Dispatch {
        table,
        source: fetch.byte(),
        target,
        saved: block.byte(7),
        span: block.span,
    }))
}

struct BridgeRequest<'a> {
    scheduler: u16,
    entry: u16,
    fetch: u16,
    dispatch: &'a Dispatch,
    pairs: &'a records::TripleStatePairs,
    stream: usize,
}
struct Bridge {
    base: u16,
    state: [u16; 18],
    selector_state: u16,
}
fn bridge(
    nrom: &Nrom<'_>,
    nodes: &BTreeMap<u16, Node>,
    owned: &[Option<usize>],
    request: BridgeRequest<'_>,
    budget: &mut Budget<'_>,
) -> Result<Option<Bridge>, ScanStop> {
    let Some(scheduler) = graph_block(
        nrom,
        nodes,
        owned,
        request.scheduler,
        &[0xa0, 0xa2, 0x20, 0xa2, 0xa0, 0x20, 0xa2, 0xa0],
        budget,
    )?
    else {
        return Ok(None);
    };
    let slots = [scheduler.byte(1), scheduler.byte(3), scheduler.byte(6)];
    if scheduler.byte(0) != 0
        || scheduler.word(2) != request.entry
        || scheduler.byte(4) != 1
        || scheduler.word(5) != request.entry
        || scheduler.byte(7) != 2
        || !distinct(&slots)
        || u16::try_from(scheduler.span.byte_len)
            .ok()
            .and_then(|len| request.scheduler.checked_add(len))
            != Some(request.entry)
    {
        return Ok(None);
    }
    let Some(entry) = graph_block(
        nrom,
        nodes,
        owned,
        request.entry,
        &[
            0x84, 0xa9, 0x9d, 0xbd, 0x85, 0xbd, 0x85, 0x8a, 0x18, 0x69, 0x7d, 0x85, 0x86, 0xbd,
            0xf0,
        ],
        budget,
    )?
    else {
        return Ok(None);
    };
    let source = entry.byte(4);
    let Some(source_high) = source.checked_add(1) else {
        return Ok(None);
    };
    let Some(target_high) = request.dispatch.target.checked_add(1) else {
        return Ok(None);
    };
    let Some(base) = entry.word(2).checked_sub(0x13) else {
        return Ok(None);
    };
    if entry.byte(1) != 0
        || entry.word(3) != base
        || Some(entry.word(5)) != base.checked_add(1)
        || entry.byte(6) != source_high
        || entry.byte(9) != 9
        || Some(entry.word(10)) != base.checked_add(2)
        || entry.byte(12) != request.dispatch.saved
        || Some(entry.word(13)) != base.checked_add(3)
        || source != request.dispatch.source
        || !distinct(&[
            source,
            source_high,
            request.dispatch.target,
            target_high,
            request.dispatch.saved,
            entry.byte(0),
            entry.byte(11),
        ])
    {
        return Ok(None);
    }
    let Some(setup_address) = entry.branch(14) else {
        return Ok(None);
    };
    let Some(setup) = graph_block(
        nrom,
        nodes,
        owned,
        setup_address,
        &[0xa9, 0x9d, 0xa8],
        budget,
    )?
    else {
        return Ok(None);
    };
    if setup.byte(0) != 0
        || Some(setup.word(1)) != base.checked_add(0x15)
        || u16::try_from(setup.span.byte_len)
            .ok()
            .and_then(|len| setup_address.checked_add(len))
            != Some(request.fetch)
    {
        return Ok(None);
    }
    let mut state = [0; 18];
    for (index, slot) in slots.into_iter().enumerate() {
        for (suffix_index, suffix) in [0, 1, 2, 3, 0x13, 0x15].into_iter().enumerate() {
            let Some(address) = base
                .checked_add(u16::from(slot))
                .and_then(|x| x.checked_add(suffix))
            else {
                return Ok(None);
            };
            if !(0x0200..=0x07ff).contains(&address) {
                return Ok(None);
            }
            state[index * 6 + suffix_index] = address
        }
    }
    if !distinct(&state)
        || request.pairs.pairs[request.stream] != state[request.stream * 6]
        || !(0x0200..=0x07ff).contains(&request.pairs.selector_state_address)
        || state.contains(&request.pairs.selector_state_address)
    {
        return Ok(None);
    }
    Ok(Some(Bridge {
        base,
        state,
        selector_state: request.pairs.selector_state_address,
    }))
}

struct HandlerInput {
    address: u16,
    dispatch: Dispatch,
    bridge: Bridge,
    head: u8,
    head_span: FileSpan,
    row_span: FileSpan,
}
fn handler(
    nrom: &Nrom<'_>,
    owned: &[Option<usize>],
    input: HandlerInput,
    protected: &[FileSpan],
    budget: &mut Budget<'_>,
) -> Result<Option<CodeConditionalHeadCommandEdge>, ScanStop> {
    let Some(handler_span) = span(nrom, input.address, 15) else {
        return Ok(None);
    };
    if overlaps_any(handler_span, protected, budget)? || overlaps(handler_span, input.row_span) {
        return Ok(None);
    }
    let mut at = input.address;
    let mut instructions = Vec::new();
    for opcode in [0xc8, 0xa2, 0xb1, 0x9d, 0xc8, 0xe8, 0xe0, 0xd0, 0x60] {
        let Some(instruction) = local_instruction(nrom, owned, at, budget)? else {
            return Ok(None);
        };
        if instruction.opcode != opcode {
            return Ok(None);
        }
        let Some(next) = at.checked_add(u16::from(instruction.len)) else {
            return Ok(None);
        };
        at = next;
        instructions.push(instruction)
    }
    let Some(destination) = input.bridge.base.checked_add(0xb1) else {
        return Ok(None);
    };
    let Some(end) = destination.checked_add(7) else {
        return Ok(None);
    };
    if Some(at) != input.address.checked_add(15)
        || instructions[1].byte() != 0
        || instructions[2].byte() != input.dispatch.source
        || instructions[3].word() != destination
        || instructions[6].byte() != 8
        || instructions[7].branch() != input.address.checked_add(3)
        || !(0x0200..=0x07ff).contains(&destination)
        || !(0x0200..=0x07ff).contains(&end)
        || (destination..=end)
            .any(|x| input.bridge.state.contains(&x) || x == input.bridge.selector_state)
    {
        return Ok(None);
    }
    Ok(Some(CodeConditionalHeadCommandEdge {
        head_byte: input.head,
        head_span: input.head_span,
        dispatch_row_span: input.row_span,
        handler_cpu_address: input.address,
        handler_span,
        operand_count: 8,
        destination_start: destination,
        destination_end_inclusive: end,
        evidence: vec![
            evidence_at(
                nrom,
                "immutable-head-byte",
                EvidenceKind::DriverData,
                input.head_span,
            ),
            evidence_at(
                nrom,
                "revalidated-command-dispatch",
                EvidenceKind::InstructionBytes,
                input.dispatch.span,
            ),
            evidence_at(
                nrom,
                "revalidated-command-dispatch-row",
                EvidenceKind::DriverData,
                input.row_span,
            ),
            evidence_at(
                nrom,
                "revalidated-fixed-row-handler",
                EvidenceKind::InstructionBytes,
                handler_span,
            ),
        ],
    }))
}
