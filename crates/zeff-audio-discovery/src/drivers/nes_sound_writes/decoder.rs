use std::collections::{BTreeMap, BTreeSet};

use crate::{Budget, ScanStop, tracker::FileSpan};

mod bindings;
mod dispatch;
mod head_edges;
mod records;
mod selectors;

const PRG_START: usize = 16;
const PRG_16K: usize = 0x4000;
const MAX_GRAPH_NODES: usize = 4096;
const MAX_EVIDENCE: usize = 25;

pub(super) struct Nrom<'a> {
    bytes: &'a [u8],
    prg_len: usize,
}

pub(super) struct Decoded {
    pub vectors: Vec<Vector>,
    pub writes: Vec<Write>,
    pub calls: Vec<Call>,
    pub command_dispatches: Vec<super::super::CodeCommandDispatch>,
    pub selector_consumers: Vec<super::super::CodeSelectorConsumer>,
}

pub(super) struct Vector {
    pub signature: &'static str,
    pub span: FileSpan,
    address: u16,
}

pub(super) struct Write {
    pub cpu_address: u16,
    pub register: u16,
    pub span: FileSpan,
}

pub(super) struct Call {
    pub cpu_address: u16,
    pub target_cpu_address: u16,
    pub span: FileSpan,
    pub writer_cpu_address: u16,
    pub writer_span: FileSpan,
}

struct Node {
    opcode: u8,
    length: u8,
    target: Option<u16>,
    branch: Option<i8>,
}

impl<'a> Nrom<'a> {
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        let header = bytes.get(..PRG_START)?;
        if &header[..4] != b"NES\x1a"
            || !matches!(header[4], 1 | 2)
            || header[6] & 0xfc != 0
            || header[7] != 0
            || header[8..].iter().any(|value| *value != 0)
        {
            return None;
        }
        let prg_len = usize::from(header[4]) * PRG_16K;
        let expected = PRG_START + prg_len + usize::from(header[5]) * 0x2000;
        (bytes.len() == expected).then_some(Self { bytes, prg_len })
    }

    fn prg(&self) -> &[u8] {
        &self.bytes[PRG_START..PRG_START + self.prg_len]
    }

    fn offset(&self, address: u16) -> Option<usize> {
        let address = usize::from(address);
        (address >= 0x8000).then(|| {
            let offset = address - 0x8000;
            if self.prg_len == PRG_16K {
                offset & (PRG_16K - 1)
            } else {
                offset
            }
        })
    }

    fn span(&self, offset: usize, byte_len: usize) -> FileSpan {
        FileSpan {
            offset: (PRG_START + offset) as u32,
            byte_len: byte_len as u32,
        }
    }
}

pub(super) fn decode(
    nrom: &Nrom<'_>,
    budget: &mut Budget<'_>,
) -> Result<Option<Decoded>, ScanStop> {
    let mut vectors = Vec::with_capacity(3);
    for (signature, at) in [
        ("vector-nmi", 0xfffa),
        ("vector-reset", 0xfffc),
        ("vector-irq", 0xfffe),
    ] {
        budget.charge()?;
        let Some(offset) = nrom.offset(at) else {
            return Ok(None);
        };
        let Some(bytes) = nrom.prg().get(offset..offset + 2) else {
            return Ok(None);
        };
        let address = u16::from_le_bytes(bytes.try_into().expect("two-byte vector"));
        if nrom.offset(address).is_none() {
            return Ok(None);
        }
        vectors.push(Vector {
            signature,
            span: nrom.span(offset, 2),
            address,
        });
    }

    let mut pending = vectors
        .iter()
        .map(|vector| vector.address)
        .collect::<Vec<_>>();
    pending.reverse();
    let mut decoded = BTreeSet::new();
    let mut nodes = BTreeMap::new();
    let mut owned = vec![None; nrom.prg_len];
    let mut writes = BTreeMap::new();
    while let Some(address) = pending.pop() {
        budget.charge()?;
        let Some(offset) = nrom.offset(address) else {
            continue;
        };
        if decoded.contains(&address) {
            continue;
        }
        if decoded.len() == MAX_GRAPH_NODES || pending.len() > MAX_GRAPH_NODES * 2 {
            return Err(ScanStop::ValidationLimit);
        }
        let Some(opcode) = nrom.prg().get(offset).copied() else {
            return Ok(None);
        };
        let Some(length) = official_length(opcode) else {
            return Ok(None);
        };
        let Some(instruction) = nrom.prg().get(offset..offset + length) else {
            return Ok(None);
        };
        if owned[offset..offset + length]
            .iter()
            .any(|owner| owner.is_some_and(|owner| owner != offset))
        {
            return Ok(None);
        }
        owned[offset..offset + length].fill(Some(offset));
        decoded.insert(address);

        if matches!(opcode, 0x8c..=0x8e) {
            let target = u16::from_le_bytes([instruction[1], instruction[2]]);
            if is_apu_register(target) {
                writes.entry(target).or_insert(Write {
                    cpu_address: address,
                    register: target,
                    span: nrom.span(offset, length),
                });
            }
        }
        if writes.len() + vectors.len() > MAX_EVIDENCE {
            return Err(ScanStop::InventoryLimit);
        }
        successors(address, opcode, instruction, &mut pending);
        nodes.insert(
            address,
            Node {
                opcode,
                length: length as u8,
                target: matches!(opcode, 0x20 | 0x4c).then(|| word(instruction)),
                branch: is_branch(opcode).then(|| instruction[1] as i8),
            },
        );
    }
    let writes = writes.into_values().collect::<Vec<_>>();
    let calls = if writes.len() < 2 {
        Vec::new()
    } else {
        calls(&nodes, &writes, nrom, budget)?
    };
    let mut selector_consumers = selectors::find(nrom, &nodes, &owned, &calls, budget)?;
    for consumer in &mut selector_consumers {
        records::inspect(nrom, &nodes, &owned, consumer, budget)?;
    }
    let command_dispatches = if writes.len() < 2 {
        Vec::new()
    } else {
        dispatch::find(nrom, &nodes, &owned, &calls, budget)?
    };
    bindings::bind(
        nrom,
        &nodes,
        &owned,
        &mut selector_consumers,
        &command_dispatches,
        budget,
    )?;
    head_edges::find(nrom, &nodes, &owned, &mut selector_consumers, budget)?;
    Ok(Some(Decoded {
        vectors,
        writes,
        calls,
        command_dispatches,
        selector_consumers,
    }))
}

fn calls(
    nodes: &BTreeMap<u16, Node>,
    writes: &[Write],
    nrom: &Nrom<'_>,
    budget: &mut Budget<'_>,
) -> Result<Vec<Call>, ScanStop> {
    let retained_writes = writes
        .iter()
        .map(|write| (write.cpu_address, write))
        .collect::<BTreeMap<_, _>>();
    let mut calls = Vec::new();
    for (&cpu_address, node) in nodes {
        if node.opcode != 0x20 {
            continue;
        }
        let Some(target_cpu_address) = node.target else {
            continue;
        };
        let Some(writer) = reachable_writer(nodes, &retained_writes, target_cpu_address, budget)?
        else {
            continue;
        };
        if calls.len() == MAX_CALLS {
            return Err(ScanStop::InventoryLimit);
        }
        calls.push(Call {
            cpu_address,
            target_cpu_address,
            span: nrom.span(nrom.offset(cpu_address).expect("decoded call"), 3),
            writer_cpu_address: writer.cpu_address,
            writer_span: writer.span,
        });
    }
    Ok(calls)
}

const MAX_CALLS: usize = 64;

fn reachable_writer<'a>(
    nodes: &BTreeMap<u16, Node>,
    retained_writes: &'a BTreeMap<u16, &'a Write>,
    target: u16,
    budget: &mut Budget<'_>,
) -> Result<Option<&'a Write>, ScanStop> {
    let mut pending = vec![target];
    let mut visited = BTreeSet::new();
    let mut writer = None;
    while let Some(address) = pending.pop() {
        if !visited.insert(address) {
            continue;
        }
        budget.charge()?;
        if visited.len() > MAX_GRAPH_NODES {
            return Err(ScanStop::ValidationLimit);
        }
        let Some(node) = nodes.get(&address) else {
            continue;
        };
        if let Some(candidate) = retained_writes.get(&address)
            && writer.is_none_or(|current: &Write| candidate.cpu_address < current.cpu_address)
        {
            writer = Some(*candidate);
        }
        local_successors(address, node, &mut pending);
        if pending.len() > MAX_GRAPH_NODES * 2 {
            return Err(ScanStop::ValidationLimit);
        }
    }
    Ok(writer)
}

fn local_successors(address: u16, node: &Node, pending: &mut Vec<u16>) {
    let next = address.wrapping_add(u16::from(node.length));
    match node.opcode {
        0x00 | 0x40 | 0x60 | 0x6c => {}
        0x4c => pending.extend(node.target),
        0x20 => pending.push(next),
        opcode if is_branch(opcode) => {
            pending.push(next);
            pending.extend(
                node.branch
                    .map(|branch| next.wrapping_add_signed(i16::from(branch))),
            );
        }
        _ => pending.push(next),
    }
}

fn is_branch(opcode: u8) -> bool {
    matches!(
        opcode,
        0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xb0 | 0xd0 | 0xf0
    )
}

fn successors(address: u16, opcode: u8, instruction: &[u8], pending: &mut Vec<u16>) {
    let next = address.wrapping_add(instruction.len() as u16);
    match opcode {
        0x00 | 0x40 | 0x60 | 0x6c => {}
        0x4c => pending.push(word(instruction)),
        0x20 => {
            pending.push(next);
            pending.push(word(instruction));
        }
        0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xb0 | 0xd0 | 0xf0 => {
            pending.push(next);
            pending.push(next.wrapping_add_signed(i16::from(instruction[1] as i8)));
        }
        _ => pending.push(next),
    }
}

fn word(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[1], bytes[2]])
}

fn is_apu_register(address: u16) -> bool {
    matches!(address, 0x4000..=0x4013 | 0x4015 | 0x4017)
}

fn official_length(opcode: u8) -> Option<usize> {
    Some(match opcode {
        0x08 | 0x0a | 0x18 | 0x28 | 0x2a | 0x38 | 0x40 | 0x48 | 0x4a | 0x58 | 0x60 | 0x68
        | 0x6a | 0x78 | 0x88 | 0x8a | 0x98 | 0x9a | 0xa8 | 0xaa | 0xb8 | 0xba | 0xc8 | 0xca
        | 0xd8 | 0xe8 | 0xea | 0xf8 => 1,
        0x00 | 0x01 | 0x05 | 0x06 | 0x09 | 0x10 | 0x11 | 0x15 | 0x16 | 0x21 | 0x24 | 0x25
        | 0x26 | 0x29 | 0x30 | 0x31 | 0x35 | 0x36 | 0x41 | 0x45 | 0x46 | 0x49 | 0x50 | 0x51
        | 0x55 | 0x56 | 0x61 | 0x65 | 0x66 | 0x69 | 0x70 | 0x71 | 0x75 | 0x76 | 0x81 | 0x84
        | 0x85 | 0x86 | 0x90 | 0x91 | 0x94 | 0x95 | 0x96 | 0xa0 | 0xa1 | 0xa2 | 0xa4 | 0xa5
        | 0xa6 | 0xa9 | 0xb0 | 0xb1 | 0xb4 | 0xb5 | 0xb6 | 0xc0 | 0xc1 | 0xc4 | 0xc5 | 0xc6
        | 0xc9 | 0xd0 | 0xd1 | 0xd5 | 0xd6 | 0xe0 | 0xe1 | 0xe4 | 0xe5 | 0xe6 | 0xe9 | 0xf0
        | 0xf1 | 0xf5 | 0xf6 => 2,
        0x0d | 0x0e | 0x19 | 0x1d | 0x1e | 0x20 | 0x2c | 0x2d | 0x2e | 0x39 | 0x3d | 0x3e
        | 0x4c | 0x4d | 0x4e | 0x59 | 0x5d | 0x5e | 0x6c | 0x6d | 0x6e | 0x79 | 0x7d | 0x7e
        | 0x8c | 0x8d | 0x8e | 0x99 | 0x9d | 0xac | 0xad | 0xae | 0xb9 | 0xbc | 0xbd | 0xbe
        | 0xcc | 0xcd | 0xce | 0xd9 | 0xdd | 0xde | 0xec | 0xed | 0xee | 0xf9 | 0xfd | 0xfe => 3,
        _ => return None,
    })
}
