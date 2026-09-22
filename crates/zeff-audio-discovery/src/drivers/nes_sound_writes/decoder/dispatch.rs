use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::drivers::{
    CandidateEvidence, CodeCall, CodeCommandDispatch, CodeCommandFetch, EvidenceKind,
};

use super::{Budget, Call, FileSpan, Node, Nrom, ScanStop};

const MAX_DISPATCHES: usize = 16;
const MAX_FETCHES: usize = 64;

pub(super) fn find(
    nrom: &Nrom<'_>,
    nodes: &BTreeMap<u16, Node>,
    owned: &[Option<usize>],
    calls: &[Call],
    budget: &mut Budget<'_>,
) -> Result<Vec<CodeCommandDispatch>, ScanStop> {
    let mut dispatches = BTreeMap::<u16, Dispatch>::new();
    for &address in nodes.keys() {
        budget.charge()?;
        if let Some(dispatch) = Dispatch::at(nrom, nodes, owned, address, budget)? {
            if dispatches.len() == MAX_DISPATCHES {
                return Err(ScanStop::InventoryLimit);
            }
            dispatches.insert(address, dispatch);
        }
    }

    let mut grouped = BTreeMap::<u16, Vec<CodeCommandFetch>>::new();
    let mut fetch_count = 0;
    for &address in nodes.keys() {
        budget.charge()?;
        let Some(fetch) = Fetch::at(nrom, nodes, owned, address, budget)? else {
            continue;
        };
        let Some(dispatch) = dispatches.get(&fetch.dispatch_cpu_address) else {
            continue;
        };
        if fetch.saved_index_address != dispatch.saved_index_address
            || !distinct_roles(&fetch, dispatch)
        {
            continue;
        }
        let Some(audio_call) = first_audio_call(nodes, calls, address, budget)? else {
            continue;
        };
        if fetch_count == MAX_FETCHES {
            return Err(ScanStop::InventoryLimit);
        }
        let fetches = grouped.entry(dispatch.entry_cpu_address).or_default();
        fetches.push(CodeCommandFetch {
            cpu_address: address,
            span: fetch.span,
            source_pointer_address: fetch.source_pointer_address,
            call_cpu_address: fetch.call_cpu_address,
            call_span: fetch.call_span,
            event_cpu_address: fetch.event_cpu_address,
            audio_call,
        });
        fetch_count += 1;
    }

    let mut result = Vec::new();
    for (address, mut fetches) in grouped {
        let dispatch = dispatches
            .get(&address)
            .expect("fetch grouped by known dispatch");
        fetches.sort_by_key(|fetch| fetch.cpu_address);
        let mut evidence = vec![evidence_at(
            nrom,
            "decoded-command-dispatch",
            EvidenceKind::InstructionBytes,
            dispatch.span,
        )];
        for fetch in &fetches {
            evidence.extend([
                evidence_at(
                    nrom,
                    "decoded-command-fetch",
                    EvidenceKind::InstructionBytes,
                    fetch.span,
                ),
                evidence_at(
                    nrom,
                    "decoded-command-audio-anchor",
                    EvidenceKind::DirectCall,
                    fetch.audio_call.span,
                ),
            ]);
        }
        result.push(CodeCommandDispatch {
            entry_cpu_address: dispatch.entry_cpu_address,
            entry_span: dispatch.span,
            table_cpu_address: dispatch.table_cpu_address,
            target_pointer_address: dispatch.target_pointer_address,
            saved_index_address: dispatch.saved_index_address,
            fetches,
            evidence,
        });
    }
    Ok(result)
}

struct Dispatch {
    entry_cpu_address: u16,
    span: FileSpan,
    table_cpu_address: u16,
    target_pointer_address: u8,
    saved_index_address: u8,
}

impl Dispatch {
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
            &[0x0a, 0xa8, 0xb9, 0x85, 0xb9, 0x85, 0xa0, 0xa6, 0x6c],
            budget,
        )?
        else {
            return Ok(None);
        };
        let table = block.word(2);
        let Some(table_high) = table.checked_add(1) else {
            return Ok(None);
        };
        let target = block.byte(3);
        let Some(target_high) = target.checked_add(1) else {
            return Ok(None);
        };
        let Some(table_span) = span(nrom, table, 2) else {
            return Ok(None);
        };
        let table_offset = table_span.offset as usize - super::PRG_START;
        if block.word(4) != table_high
            || block.byte(5) != target_high
            || block.byte(6) != 0
            || block.word(8) != u16::from(target)
            || owned[table_offset..table_offset + 2]
                .iter()
                .any(Option::is_some)
        {
            return Ok(None);
        }
        Ok(Some(Self {
            entry_cpu_address: address,
            span: block.span,
            table_cpu_address: table,
            target_pointer_address: target,
            saved_index_address: block.byte(7),
        }))
    }
}

struct Fetch {
    span: FileSpan,
    source_pointer_address: u8,
    saved_index_address: u8,
    call_cpu_address: u16,
    call_span: FileSpan,
    event_cpu_address: u16,
    dispatch_cpu_address: u16,
}

impl Fetch {
    fn at(
        nrom: &Nrom<'_>,
        nodes: &BTreeMap<u16, Node>,
        owned: &[Option<usize>],
        address: u16,
        budget: &mut Budget<'_>,
    ) -> Result<Option<Self>, ScanStop> {
        let Some(fetch) = instruction(nrom, nodes, owned, address, budget)? else {
            return Ok(None);
        };
        if fetch.opcode != 0xb1 || fetch.len != 2 || fetch.byte(1) == 0xff {
            return Ok(None);
        }
        let Some(guard_address) = address.checked_add(u16::from(fetch.len)) else {
            return Ok(None);
        };
        let Some(guard) = instruction(nrom, nodes, owned, guard_address, budget)? else {
            return Ok(None);
        };
        if guard.opcode != 0x10 || guard.len != 2 {
            return Ok(None);
        }
        let Some(event_cpu_address) = branch_target(guard_address, &guard) else {
            return Ok(None);
        };
        if !nodes.contains_key(&event_cpu_address) {
            return Ok(None);
        }

        let mut at = guard_address.checked_add(u16::from(guard.len));
        while let Some(current) = at {
            let Some(node) = instruction(nrom, nodes, owned, current, budget)? else {
                return Ok(None);
            };
            match node.opcode {
                0x8d if node.len == 3 && (0x0100..=0x07ff).contains(&node.word()) => {
                    at = current.checked_add(u16::from(node.len));
                }
                0xa6 if node.len == 2 => {
                    let Some(call_address) = current.checked_add(u16::from(node.len)) else {
                        return Ok(None);
                    };
                    let Some(call) = instruction(nrom, nodes, owned, call_address, budget)? else {
                        return Ok(None);
                    };
                    let Some(end) = call_address.checked_add(u16::from(call.len)) else {
                        return Ok(None);
                    };
                    if call.opcode != 0x20 || call.len != 3 || event_cpu_address < end {
                        return Ok(None);
                    }
                    let Some(fetch_span) = span(nrom, address, usize::from(end - address)) else {
                        return Ok(None);
                    };
                    return Ok(Some(Self {
                        span: fetch_span,
                        source_pointer_address: fetch.byte(1),
                        saved_index_address: node.byte(1),
                        call_cpu_address: call_address,
                        call_span: call.span,
                        event_cpu_address,
                        dispatch_cpu_address: call.word(),
                    }));
                }
                _ => return Ok(None),
            }
        }
        Ok(None)
    }
}

fn distinct_roles(fetch: &Fetch, dispatch: &Dispatch) -> bool {
    let Some(source_high) = fetch.source_pointer_address.checked_add(1) else {
        return false;
    };
    let Some(target_high) = dispatch.target_pointer_address.checked_add(1) else {
        return false;
    };
    let roles = [
        fetch.source_pointer_address,
        source_high,
        dispatch.target_pointer_address,
        target_high,
        dispatch.saved_index_address,
    ];
    roles
        .iter()
        .enumerate()
        .all(|(index, role)| !roles[..index].contains(role))
}

fn first_audio_call(
    nodes: &BTreeMap<u16, Node>,
    calls: &[Call],
    fetch: u16,
    budget: &mut Budget<'_>,
) -> Result<Option<CodeCall>, ScanStop> {
    for call in calls {
        if reachable(nodes, call.cpu_address, fetch, budget)? {
            return Ok(Some(CodeCall {
                cpu_address: call.cpu_address,
                target_cpu_address: call.target_cpu_address,
                span: call.span,
                writer_cpu_address: call.writer_cpu_address,
                writer_span: call.writer_span,
            }));
        }
    }
    Ok(None)
}

fn reachable(
    nodes: &BTreeMap<u16, Node>,
    start: u16,
    goal: u16,
    budget: &mut Budget<'_>,
) -> Result<bool, ScanStop> {
    let mut pending = VecDeque::from([start]);
    let mut visited = BTreeSet::new();
    while let Some(address) = pending.pop_front() {
        if !visited.insert(address) {
            continue;
        }
        budget.charge()?;
        if visited.len() > super::MAX_GRAPH_NODES {
            return Ok(false);
        }
        if address == goal {
            return Ok(true);
        }
        let Some(node) = nodes.get(&address) else {
            continue;
        };
        let Some(next) = address.checked_add(u16::from(node.length)) else {
            continue;
        };
        match node.opcode {
            0x00 | 0x40 | 0x60 | 0x6c => {}
            0x4c => pending.extend(node.target),
            0x20 => {
                pending.push_back(next);
                pending.extend(node.target);
            }
            opcode if is_branch(opcode) => {
                pending.push_back(next);
                pending.extend(
                    node.branch
                        .and_then(|branch| next.checked_add_signed(i16::from(branch))),
                );
            }
            _ => pending.push_back(next),
        }
        if pending.len() > super::MAX_GRAPH_NODES * 2 {
            return Ok(false);
        }
    }
    Ok(false)
}

struct Instruction {
    opcode: u8,
    len: u8,
    operand: [u8; 2],
    span: FileSpan,
    branch: Option<i8>,
}

impl Instruction {
    fn byte(&self, index: usize) -> u8 {
        self.operand[index - 1]
    }

    fn word(&self) -> u16 {
        u16::from_le_bytes(self.operand)
    }
}

struct Block {
    instructions: Vec<Instruction>,
    span: FileSpan,
}

impl Block {
    fn byte(&self, index: usize) -> u8 {
        self.instructions[index].byte(1)
    }

    fn word(&self, index: usize) -> u16 {
        self.instructions[index].word()
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
        let Some(node) = instruction(nrom, nodes, owned, at, budget)? else {
            return Ok(None);
        };
        if node.opcode != opcode {
            return Ok(None);
        }
        let Some(next) = at.checked_add(u16::from(node.len)) else {
            return Ok(None);
        };
        at = next;
        instructions.push(node);
    }
    let Some(span) = span(nrom, address, usize::from(at - address)) else {
        return Ok(None);
    };
    Ok(Some(Block { instructions, span }))
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
        opcode: node.opcode,
        len: node.length,
        operand: [
            bytes.get(1).copied().unwrap_or(0),
            bytes.get(2).copied().unwrap_or(0),
        ],
        span,
        branch: node.branch,
    }))
}

fn branch_target(address: u16, instruction: &Instruction) -> Option<u16> {
    address
        .checked_add(u16::from(instruction.len))?
        .checked_add_signed(i16::from(instruction.branch?))
}

fn is_branch(opcode: u8) -> bool {
    matches!(
        opcode,
        0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xb0 | 0xd0 | 0xf0
    )
}

fn span(nrom: &Nrom<'_>, address: u16, len: usize) -> Option<FileSpan> {
    let start = nrom.offset(address)?;
    let last = address.checked_add(u16::try_from(len.checked_sub(1)?).ok()?)?;
    (start.checked_add(len)? <= nrom.prg_len && nrom.offset(last)? == start + len - 1)
        .then(|| nrom.span(start, len))
}

fn evidence_at(
    nrom: &Nrom<'_>,
    signature: &'static str,
    kind: EvidenceKind,
    span: FileSpan,
) -> CandidateEvidence {
    super::super::evidence_at(nrom.bytes, signature, kind, span)
}
