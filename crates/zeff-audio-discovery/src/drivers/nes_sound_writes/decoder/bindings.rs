use std::collections::BTreeMap;

use crate::drivers::{
    CandidateEvidence, CodeCommandDispatch, CodePointerDisposition, CodeSelectorConsumer,
    CodeStreamBinding, EvidenceKind,
};

use super::{Budget, FileSpan, Node, Nrom, ScanStop, records};

const MAX_BINDINGS: usize = 48;

pub(super) fn bind(
    nrom: &Nrom<'_>,
    nodes: &BTreeMap<u16, Node>,
    owned: &[Option<usize>],
    consumers: &mut [CodeSelectorConsumer],
    dispatches: &[CodeCommandDispatch],
    budget: &mut Budget<'_>,
) -> Result<(), ScanStop> {
    if dispatches.is_empty() {
        return Ok(());
    }
    let bridges = bridges(nrom, nodes, owned, dispatches, budget)?;
    let mut staged = Vec::new();
    for (consumer_index, consumer) in consumers.iter().enumerate() {
        if !consumer
            .records
            .iter()
            .any(|record| record.streams.len() == 3 && record.header & 0x80 != 0)
        {
            continue;
        }
        let Some(record_pairs) = records::triple_state_pairs(nrom, nodes, owned, consumer, budget)?
        else {
            continue;
        };
        for (record_index, record) in consumer.records.iter().enumerate() {
            if record.streams.len() != 3 || record.header & 0x80 == 0 {
                continue;
            }
            for (stream_index, stream) in record.streams.iter().enumerate() {
                budget.charge()?;
                if stream.disposition != CodePointerDisposition::Unparsed {
                    continue;
                }
                let mut matched = None;
                let mut ambiguous = false;
                for bridge in &bridges {
                    budget.charge()?;
                    if bridge.state_pairs[stream_index] == record_pairs.pairs[stream_index]
                        && state_ram(record_pairs.selector_state_address)
                        && !bridge
                            .state_addresses
                            .contains(&record_pairs.selector_state_address)
                        && matched
                            .replace(binding(nrom, record_pairs.copy_span, bridge, stream_index))
                            .is_some()
                    {
                        ambiguous = true;
                        break;
                    }
                }
                if !ambiguous && let Some(binding) = matched {
                    if staged.len() == MAX_BINDINGS {
                        return Err(ScanStop::InventoryLimit);
                    }
                    staged.push((consumer_index, record_index, stream_index, binding));
                }
            }
        }
    }
    for (consumer, record, stream, binding) in staged {
        consumers[consumer].records[record].streams[stream].fetch_binding = Some(binding);
    }
    Ok(())
}

fn binding(
    nrom: &Nrom<'_>,
    copy_span: FileSpan,
    bridge: &Bridge,
    stream_index: usize,
) -> CodeStreamBinding {
    CodeStreamBinding {
        state_pointer_address: bridge.state_pairs[stream_index],
        scheduler_cpu_address: bridge.scheduler_cpu_address,
        scheduler_span: bridge.scheduler_span,
        consumer_entry_cpu_address: bridge.entry_cpu_address,
        consumer_entry_span: bridge.entry_span,
        fetch_cpu_address: bridge.fetch_cpu_address,
        fetch_span: bridge.fetch_span,
        evidence: vec![
            evidence_at(nrom, "decoded-record-fetch-copy", copy_span),
            evidence_at(
                nrom,
                "decoded-record-fetch-scheduler",
                bridge.scheduler_span,
            ),
            evidence_at(nrom, "decoded-record-fetch-entry", bridge.entry_span),
            evidence_at(nrom, "decoded-record-fetch-setup", bridge.setup_span),
            evidence_at(nrom, "decoded-record-fetch", bridge.fetch_span),
        ],
    }
}

struct Scheduler {
    cpu_address: u16,
    span: FileSpan,
    entry: u16,
    slots: [u8; 3],
}

struct Bridge {
    scheduler_cpu_address: u16,
    scheduler_span: FileSpan,
    entry_cpu_address: u16,
    entry_span: FileSpan,
    setup_span: FileSpan,
    fetch_cpu_address: u16,
    fetch_span: FileSpan,
    state_pairs: [u16; 3],
    state_addresses: [u16; 18],
}

fn bridges(
    nrom: &Nrom<'_>,
    nodes: &BTreeMap<u16, Node>,
    owned: &[Option<usize>],
    dispatches: &[CodeCommandDispatch],
    budget: &mut Budget<'_>,
) -> Result<Vec<Bridge>, ScanStop> {
    let mut result = Vec::new();
    for &address in nodes.keys() {
        budget.charge()?;
        let Some(scheduler) = Scheduler::at(nrom, nodes, owned, address, budget)? else {
            continue;
        };
        for dispatch in dispatches {
            for fetch in &dispatch.fetches {
                budget.charge()?;
                if let Some(bridge) =
                    Bridge::at(nrom, nodes, owned, &scheduler, (dispatch, fetch), budget)?
                {
                    result.push(bridge);
                }
            }
        }
    }
    Ok(result)
}

impl Scheduler {
    fn at(
        nrom: &Nrom<'_>,
        nodes: &BTreeMap<u16, Node>,
        owned: &[Option<usize>],
        address: u16,
        budget: &mut Budget<'_>,
    ) -> Result<Option<Self>, ScanStop> {
        let Some(block) = block(
            nrom,
            nodes,
            owned,
            address,
            &[0xa0, 0xa2, 0x20, 0xa2, 0xa0, 0x20, 0xa2, 0xa0],
            budget,
        )?
        else {
            return Ok(None);
        };
        let entry = block.word(2);
        let slots = [block.byte(1), block.byte(3), block.byte(6)];
        if block.byte(0) != 0
            || block.word(5) != entry
            || block.byte(4) != 1
            || block.byte(7) != 2
            || !distinct(&slots)
            || block.end != entry
        {
            return Ok(None);
        }
        Ok(Some(Self {
            cpu_address: address,
            span: block.span,
            entry,
            slots,
        }))
    }
}

impl Bridge {
    fn at(
        nrom: &Nrom<'_>,
        nodes: &BTreeMap<u16, Node>,
        owned: &[Option<usize>],
        scheduler: &Scheduler,
        command: (&CodeCommandDispatch, &crate::drivers::CodeCommandFetch),
        budget: &mut Budget<'_>,
    ) -> Result<Option<Self>, ScanStop> {
        let (dispatch, fetch) = command;
        let Some(entry) = block(
            nrom,
            nodes,
            owned,
            scheduler.entry,
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
        let target = dispatch.target_pointer_address;
        let Some(target_high) = target.checked_add(1) else {
            return Ok(None);
        };
        let saved = dispatch.saved_index_address;
        let voice = entry.byte(0);
        let stack = entry.byte(11);
        let Some(base) = entry.word(2).checked_sub(0x13) else {
            return Ok(None);
        };
        if entry.byte(1) != 0
            || entry.word(3) != base
            || Some(entry.word(5)) != base.checked_add(1)
            || entry.byte(6) != source_high
            || entry.byte(9) != 9
            || Some(entry.word(10)) != base.checked_add(2)
            || entry.byte(12) != saved
            || Some(entry.word(13)) != base.checked_add(3)
            || source != fetch.source_pointer_address
            || !distinct(&[
                source,
                source_high,
                target,
                target_high,
                saved,
                voice,
                stack,
            ])
        {
            return Ok(None);
        }
        let Some(setup_address) = entry.branch(14) else {
            return Ok(None);
        };
        let Some(setup) = block(
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
            || setup.end != fetch.cpu_address
            || fetch.source_pointer_address != source
        {
            return Ok(None);
        }
        let mut state_pairs = [0; 3];
        let mut state_addresses = [0; 18];
        for (index, slot) in scheduler.slots.into_iter().enumerate() {
            for (suffix_index, suffix) in [0, 1, 2, 3, 0x13, 0x15].into_iter().enumerate() {
                let Some(address) = state_address(base, slot, suffix) else {
                    return Ok(None);
                };
                if !state_ram(address) {
                    return Ok(None);
                }
                state_addresses[index * 6 + suffix_index] = address;
            }
            state_pairs[index] = state_addresses[index * 6];
        }
        if !distinct(&state_addresses) {
            return Ok(None);
        }
        Ok(Some(Self {
            scheduler_cpu_address: scheduler.cpu_address,
            scheduler_span: scheduler.span,
            entry_cpu_address: scheduler.entry,
            entry_span: entry.span,
            setup_span: setup.span,
            fetch_cpu_address: fetch.cpu_address,
            fetch_span: fetch.span,
            state_pairs,
            state_addresses,
        }))
    }
}

struct Instruction {
    pc: u16,
    opcode: u8,
    len: u8,
    operand: [u8; 2],
    branch: Option<i8>,
}

impl Instruction {
    fn byte(&self) -> u8 {
        self.operand[0]
    }

    fn word(&self) -> u16 {
        u16::from_le_bytes(self.operand)
    }
}

struct Block {
    instructions: Vec<Instruction>,
    span: FileSpan,
    end: u16,
}

impl Block {
    fn byte(&self, index: usize) -> u8 {
        self.instructions[index].byte()
    }

    fn word(&self, index: usize) -> u16 {
        self.instructions[index].word()
    }

    fn branch(&self, index: usize) -> Option<u16> {
        let instruction = &self.instructions[index];
        self.end_of(index)?
            .checked_add_signed(i16::from(instruction.branch?))
    }

    fn end_of(&self, index: usize) -> Option<u16> {
        self.instructions[index]
            .pc
            .checked_add(u16::from(self.instructions[index].len))
    }
}

fn block(
    nrom: &Nrom<'_>,
    nodes: &BTreeMap<u16, Node>,
    owned: &[Option<usize>],
    address: u16,
    opcodes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Option<Block>, ScanStop> {
    let mut at = address;
    let mut instructions = Vec::with_capacity(opcodes.len());
    for &opcode in opcodes {
        let Some(instruction) = instruction(nrom, nodes, owned, at, budget)? else {
            return Ok(None);
        };
        if instruction.opcode != opcode {
            return Ok(None);
        }
        let Some(next) = at.checked_add(u16::from(instruction.len)) else {
            return Ok(None);
        };
        at = next;
        instructions.push(instruction);
    }
    let Some(span) = span(nrom, address, usize::from(at - address)) else {
        return Ok(None);
    };
    Ok(Some(Block {
        instructions,
        span,
        end: at,
    }))
}

fn instruction(
    nrom: &Nrom<'_>,
    nodes: &BTreeMap<u16, Node>,
    owned: &[Option<usize>],
    address: u16,
    budget: &mut Budget<'_>,
) -> Result<Option<Instruction>, ScanStop> {
    budget.charge()?;
    let Some(node) = nodes.get(&address) else {
        return Ok(None);
    };
    let Some(span) = span(nrom, address, usize::from(node.length)) else {
        return Ok(None);
    };
    let offset = span.offset as usize - super::PRG_START;
    let bytes = &nrom.prg()[offset..offset + usize::from(node.length)];
    if bytes[0] != node.opcode
        || owned[offset..offset + bytes.len()]
            .iter()
            .any(|owner| *owner != Some(offset))
    {
        return Ok(None);
    }
    Ok(Some(Instruction {
        pc: address,
        opcode: node.opcode,
        len: node.length,
        operand: [
            bytes.get(1).copied().unwrap_or(0),
            bytes.get(2).copied().unwrap_or(0),
        ],
        branch: node.branch,
    }))
}

fn state_address(base: u16, slot: u8, suffix: u16) -> Option<u16> {
    base.checked_add(u16::from(slot))?.checked_add(suffix)
}

fn state_ram(address: u16) -> bool {
    (0x0200..=0x07ff).contains(&address)
}

fn distinct<T: Eq>(values: &[T]) -> bool {
    values
        .iter()
        .enumerate()
        .all(|(index, value)| !values[..index].contains(value))
}

fn span(nrom: &Nrom<'_>, address: u16, len: usize) -> Option<FileSpan> {
    let start = nrom.offset(address)?;
    let last = address.checked_add(u16::try_from(len.checked_sub(1)?).ok()?)?;
    (start.checked_add(len)? <= nrom.prg_len && nrom.offset(last)? == start + len - 1)
        .then(|| nrom.span(start, len))
}

fn evidence_at(nrom: &Nrom<'_>, signature: &'static str, span: FileSpan) -> CandidateEvidence {
    super::super::evidence_at(nrom.bytes, signature, EvidenceKind::InstructionBytes, span)
}
