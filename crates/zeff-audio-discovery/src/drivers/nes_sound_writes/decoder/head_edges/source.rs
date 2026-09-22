use super::super::{Budget, FileSpan, Node, Nrom, ScanStop};
use crate::drivers::{CandidateEvidence, EvidenceKind};
use std::collections::BTreeMap;

pub(super) struct Instruction {
    address: u16,
    pub(super) opcode: u8,
    pub(super) len: u8,
    operand: [u8; 2],
    branch_offset: Option<i8>,
}
impl Instruction {
    pub(super) fn byte(&self) -> u8 {
        self.operand[0]
    }
    pub(super) fn word(&self) -> u16 {
        u16::from_le_bytes(self.operand)
    }
    pub(super) fn branch(&self) -> Option<u16> {
        self.address
            .checked_add(u16::from(self.len))?
            .checked_add_signed(i16::from(self.branch_offset?))
    }
}
pub(super) struct Block {
    instructions: Vec<Instruction>,
    pub(super) span: FileSpan,
}
impl Block {
    pub(super) fn byte(&self, i: usize) -> u8 {
        self.instructions[i].byte()
    }
    pub(super) fn word(&self, i: usize) -> u16 {
        self.instructions[i].word()
    }
    pub(super) fn branch(&self, i: usize) -> Option<u16> {
        self.instructions[i].branch()
    }
}
pub(super) fn graph_block(
    nrom: &Nrom<'_>,
    nodes: &BTreeMap<u16, Node>,
    owned: &[Option<usize>],
    address: u16,
    opcodes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Option<Block>, ScanStop> {
    let mut at = address;
    let mut instructions = Vec::new();
    for &opcode in opcodes {
        let Some(i) = graph_instruction(nrom, nodes, owned, at, budget)? else {
            return Ok(None);
        };
        if i.opcode != opcode {
            return Ok(None);
        };
        let Some(next) = at.checked_add(u16::from(i.len)) else {
            return Ok(None);
        };
        at = next;
        instructions.push(i)
    }
    let Some(span) = span(nrom, address, usize::from(at - address)) else {
        return Ok(None);
    };
    Ok(Some(Block { instructions, span }))
}
pub(super) fn graph_instruction(
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
    let start = span.offset as usize - super::super::PRG_START;
    for owner in &owned[start..start + usize::from(node.length)] {
        budget.charge()?;
        if *owner != Some(start) {
            return Ok(None);
        }
    }
    let bytes = &nrom.prg()[start..start + usize::from(node.length)];
    if bytes[0] != node.opcode {
        return Ok(None);
    }
    Ok(Some(Instruction {
        address,
        opcode: node.opcode,
        len: node.length,
        operand: [
            bytes.get(1).copied().unwrap_or(0),
            bytes.get(2).copied().unwrap_or(0),
        ],
        branch_offset: node.branch,
    }))
}
pub(super) fn local_instruction(
    nrom: &Nrom<'_>,
    owned: &[Option<usize>],
    address: u16,
    budget: &mut Budget<'_>,
) -> Result<Option<Instruction>, ScanStop> {
    budget.charge()?;
    let Some(offset) = nrom.offset(address) else {
        return Ok(None);
    };
    let Some(opcode) = nrom.prg().get(offset).copied() else {
        return Ok(None);
    };
    let Some(len) = super::super::official_length(opcode) else {
        return Ok(None);
    };
    let Some(span) = span(nrom, address, len) else {
        return Ok(None);
    };
    let start = span.offset as usize - super::super::PRG_START;
    for owner in &owned[start..start + len] {
        budget.charge()?;
        if owner.is_some_and(|owner| owner != start) {
            return Ok(None);
        }
    }
    let bytes = &nrom.prg()[start..start + len];
    Ok(Some(Instruction {
        address,
        opcode,
        len: len as u8,
        operand: [
            bytes.get(1).copied().unwrap_or(0),
            bytes.get(2).copied().unwrap_or(0),
        ],
        branch_offset: is_branch(opcode).then(|| bytes[1] as i8),
    }))
}
pub(super) fn unowned(
    nrom: &Nrom<'_>,
    owned: &[Option<usize>],
    span: FileSpan,
    budget: &mut Budget<'_>,
) -> Result<bool, ScanStop> {
    let start = span.offset as usize - super::super::PRG_START;
    for owner in &owned[start..start + span.byte_len as usize] {
        budget.charge()?;
        if owner.is_some() {
            return Ok(false);
        }
    }
    let _ = nrom;
    Ok(true)
}
pub(super) fn overlaps_any(
    span: FileSpan,
    others: &[FileSpan],
    budget: &mut Budget<'_>,
) -> Result<bool, ScanStop> {
    for other in others {
        budget.charge()?;
        if overlaps(span, *other) {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(super) fn overlaps(a: FileSpan, b: FileSpan) -> bool {
    a.offset < b.offset.saturating_add(b.byte_len) && b.offset < a.offset.saturating_add(a.byte_len)
}
pub(super) fn distinct<T: Eq>(values: &[T]) -> bool {
    values
        .iter()
        .enumerate()
        .all(|(i, x)| !values[..i].contains(x))
}
pub(super) fn is_branch(opcode: u8) -> bool {
    matches!(
        opcode,
        0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xb0 | 0xd0 | 0xf0
    )
}
pub(super) fn span(nrom: &Nrom<'_>, address: u16, len: usize) -> Option<FileSpan> {
    let start = nrom.offset(address)?;
    let last = address.checked_add(u16::try_from(len.checked_sub(1)?).ok()?)?;
    (start.checked_add(len)? <= nrom.prg_len && nrom.offset(last)? == start + len - 1)
        .then(|| nrom.span(start, len))
}
pub(super) fn evidence_at(
    nrom: &Nrom<'_>,
    signature: &'static str,
    kind: EvidenceKind,
    span: FileSpan,
) -> CandidateEvidence {
    super::super::super::evidence_at(nrom.bytes, signature, kind, span)
}
