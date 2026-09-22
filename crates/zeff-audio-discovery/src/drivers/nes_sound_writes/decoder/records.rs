use std::collections::BTreeMap;

use crate::drivers::{
    CandidateEvidence, CodePointerDisposition, CodeRecord, CodeSelectorConsumer, CodeStreamPointer,
    EvidenceKind,
};

use super::{Budget, FileSpan, Node, Nrom, ScanStop};

pub(super) fn inspect(
    nrom: &Nrom<'_>,
    nodes: &BTreeMap<u16, Node>,
    owned: &[Option<usize>],
    consumer: &mut CodeSelectorConsumer,
    budget: &mut Budget<'_>,
) -> Result<(), ScanStop> {
    let mut matcher = Matcher {
        nrom,
        nodes,
        owned,
        budget,
    };
    let Some(shape) = matcher.shape(consumer)? else {
        return Ok(());
    };
    let mut records = Vec::new();
    for pointer in &consumer.pointers {
        matcher.budget.charge()?;
        if pointer.disposition != CodePointerDisposition::Unparsed {
            continue;
        }
        let Some(head) = span(nrom, pointer.target_cpu_address, 1) else {
            continue;
        };
        let start = head.offset as usize - super::PRG_START;
        let header = nrom.prg()[start];
        let triple = header & 0x80 != 0;
        let prefix_len = if triple { 8 } else { 3 };
        let Some(prefix_span) = span(nrom, pointer.target_cpu_address, prefix_len) else {
            continue;
        };
        let mut clear = true;
        for (offset, owner) in owned.iter().enumerate().skip(start).take(prefix_len) {
            matcher.budget.charge()?;
            clear &= owner.is_none() && !contains(consumer.pointer_aperture, nrom.span(offset, 1));
        }
        if !clear {
            continue;
        }
        let offsets: &[usize] = if triple { &[2, 4, 6] } else { &[1] };
        let mut streams = Vec::with_capacity(offsets.len());
        for &relative in offsets {
            matcher.budget.charge()?;
            let entry = start + relative;
            let target_cpu_address = u16::from_le_bytes([nrom.prg()[entry], nrom.prg()[entry + 1]]);
            let target_span = span(nrom, target_cpu_address, 1);
            let disposition = match target_span {
                None => CodePointerDisposition::Unmapped,
                Some(target) if owned[target.offset as usize - super::PRG_START].is_some() => {
                    CodePointerDisposition::DecodedCode
                }
                Some(target) if contains(consumer.pointer_aperture, target) => {
                    CodePointerDisposition::PointerTable
                }
                Some(target) if contains(prefix_span, target) => {
                    CodePointerDisposition::RecordPrefix
                }
                Some(_) => CodePointerDisposition::Unparsed,
            };
            streams.push(CodeStreamPointer {
                entry_span: nrom.span(entry, 2),
                target_cpu_address,
                target_span,
                disposition,
                fetch_binding: None,
                conditional_head_command_edge: None,
            });
        }
        let mut evidence = if triple {
            shape.triple.clone()
        } else {
            shape.single.clone()
        };
        evidence.push(evidence_at(
            nrom,
            "decoded-record-prefix",
            EvidenceKind::DriverData,
            prefix_span,
        ));
        records.push(CodeRecord {
            raw_selector: pointer.raw_selector,
            header,
            prefix_span,
            streams,
            evidence,
        });
    }
    let prefixes = records
        .iter()
        .map(|record| record.prefix_span)
        .collect::<Vec<_>>();
    for record in &mut records {
        for stream in &mut record.streams {
            if stream.disposition != CodePointerDisposition::Unparsed {
                continue;
            }
            for &prefix in &prefixes {
                matcher.budget.charge()?;
                if stream
                    .target_span
                    .is_some_and(|target| contains(prefix, target))
                {
                    stream.disposition = CodePointerDisposition::RecordPrefix;
                    break;
                }
            }
        }
    }
    consumer.records.extend(records);
    Ok(())
}

struct Shape {
    single: Vec<CandidateEvidence>,
    triple: Vec<CandidateEvidence>,
    triple_state_pairs: TripleStatePairs,
}

pub(super) struct TripleStatePairs {
    pub pairs: [u16; 3],
    pub copy_span: FileSpan,
    pub selector_state_address: u16,
}

pub(super) fn triple_state_pairs(
    nrom: &Nrom<'_>,
    nodes: &BTreeMap<u16, Node>,
    owned: &[Option<usize>],
    consumer: &CodeSelectorConsumer,
    budget: &mut Budget<'_>,
) -> Result<Option<TripleStatePairs>, ScanStop> {
    let mut matcher = Matcher {
        nrom,
        nodes,
        owned,
        budget,
    };
    Ok(matcher
        .shape(consumer)?
        .map(|shape| shape.triple_state_pairs))
}

struct Matcher<'a, 'b, 'c> {
    nrom: &'a Nrom<'a>,
    nodes: &'a BTreeMap<u16, Node>,
    owned: &'a [Option<usize>],
    budget: &'b mut Budget<'c>,
}

impl Matcher<'_, '_, '_> {
    fn shape(&mut self, c: &CodeSelectorConsumer) -> Result<Option<Shape>, ScanStop> {
        let Some(after_entry) = c.entry_cpu_address.checked_add(56) else {
            return Ok(None);
        };
        let Some(pointer_high) = c.pointer_address.checked_add(1) else {
            return Ok(None);
        };
        let source_slots = [
            c.selector_address,
            c.pointer_address,
            pointer_high,
            c.header_address,
        ];
        if !distinct(&source_slots) {
            return Ok(None);
        }
        let Some(dispatch) = self.block(after_entry, &[0x24, 0x10, 0x4c, 0x90, 0x4c])? else {
            return Ok(None);
        };
        // Positive selectors leave carry clear; BIT and the intervening jumps preserve it.
        if dispatch.byte(0) != c.header_address
            || dispatch.branch(1) != Some(dispatch.pc(3))
            || dispatch.branch(3) != Some(dispatch.end())
            || !self.nodes.contains_key(&dispatch.word(4))
        {
            return Ok(None);
        }
        let Some(conditional) = self.block(
            dispatch.end(),
            &[0xae, 0xbd, 0xf0, 0xbd, 0x29, 0xd0, 0xbd, 0x20, 0xb0, 0x60],
        )?
        else {
            return Ok(None);
        };
        let Some(alternate_pc) = conditional.branch(5) else {
            return Ok(None);
        };
        let Some(single) = self.block(
            alternate_pc,
            &[
                0xe0, 0xd0, 0xa2, 0xd0, 0xa2, 0x8e, 0xa0, 0xb1, 0x9d, 0xa5, 0x9d, 0x98, 0x9d, 0x8c,
                0x9d, 0x9d, 0xc8, 0xb1, 0x9d, 0xc8, 0xb1, 0x9d, 0x60,
            ],
        )?
        else {
            return Ok(None);
        };
        if !single_matches(&conditional, &single, c) {
            return Ok(None);
        }
        let Some(helper) = self.block(
            conditional.word(7),
            &[
                0x38, 0xe9, 0x85, 0x4a, 0x4a, 0x4a, 0x85, 0xa5, 0x29, 0xa8, 0xa5, 0x18, 0x69, 0x85,
                0xb9, 0xa4, 0x31, 0x18, 0xf0, 0x38, 0x60,
            ],
        )?
        else {
            return Ok(None);
        };
        if !helper_matches(&helper, &source_slots, c.pointer_address) {
            return Ok(None);
        }
        let Some(mask) = span(self.nrom, helper.word(14), 8) else {
            return Ok(None);
        };
        let mask_offset = mask.offset as usize - super::PRG_START;
        for (i, expected) in [0x80, 0x40, 0x20, 0x10, 8, 4, 2, 1].into_iter().enumerate() {
            self.budget.charge()?;
            if self.nrom.prg()[mask_offset + i] != expected
                || self.owned[mask_offset + i].is_some()
                || contains(c.pointer_aperture, self.nrom.span(mask_offset + i, 1))
            {
                return Ok(None);
            }
        }
        let Some(triple) = self.block(
            dispatch.word(2),
            &[
                0xb0, 0xa0, 0xa5, 0x8d, 0xb1, 0x8d, 0xc8, 0xb1, 0x8d, 0xc8, 0xb1, 0x8d, 0xc8, 0xb1,
                0x8d, 0xc8, 0xb1, 0x8d, 0xc8, 0xb1, 0x8d,
            ],
        )?
        else {
            return Ok(None);
        };
        let Some(pairs) = validated_triple_state_pairs(&triple, c) else {
            return Ok(None);
        };
        if triple
            .branch(0)
            .is_none_or(|target| !self.nodes.contains_key(&target))
        {
            return Ok(None);
        }
        let common = vec![
            evidence_at(
                self.nrom,
                "decoded-record-selector",
                EvidenceKind::InstructionBytes,
                c.entry_span,
            ),
            dispatch.evidence(self.nrom, "decoded-record-header-branch"),
        ];
        let mut single_evidence = common.clone();
        single_evidence.extend([
            conditional.evidence(self.nrom, "decoded-record-conditional-priority"),
            single.evidence(self.nrom, "decoded-record-single-pointer"),
            helper.evidence(self.nrom, "decoded-record-priority-helper"),
            evidence_at(
                self.nrom,
                "decoded-record-priority-mask",
                EvidenceKind::DriverData,
                mask,
            ),
        ]);
        let mut triple_evidence = common;
        triple_evidence.push(triple.evidence(self.nrom, "decoded-record-triple-pointers"));
        Ok(Some(Shape {
            single: single_evidence,
            triple: triple_evidence,
            triple_state_pairs: TripleStatePairs {
                pairs,
                copy_span: triple.source,
                selector_state_address: triple.word(3),
            },
        }))
    }

    fn block(&mut self, start: u16, opcodes: &[u8]) -> Result<Option<Block>, ScanStop> {
        let mut pc = start;
        let mut instructions = Vec::with_capacity(opcodes.len());
        for &opcode in opcodes {
            self.budget.charge()?;
            let Some(node) = self.nodes.get(&pc) else {
                return Ok(None);
            };
            let Some(source) = span(self.nrom, pc, usize::from(node.length)) else {
                return Ok(None);
            };
            let offset = source.offset as usize - super::PRG_START;
            let bytes = &self.nrom.prg()[offset..offset + usize::from(node.length)];
            if node.opcode != opcode
                || bytes[0] != opcode
                || self.owned[offset..offset + bytes.len()]
                    .iter()
                    .any(|owner| *owner != Some(offset))
            {
                return Ok(None);
            }
            let Some(next) = pc.checked_add(u16::from(node.length)) else {
                return Ok(None);
            };
            instructions.push(Instruction {
                pc,
                operand: [
                    bytes.get(1).copied().unwrap_or(0),
                    bytes.get(2).copied().unwrap_or(0),
                ],
                branch: node
                    .branch
                    .and_then(|relative| next.checked_add_signed(i16::from(relative))),
            });
            pc = next;
        }
        let Some(source) = span(self.nrom, start, usize::from(pc - start)) else {
            return Ok(None);
        };
        Ok(Some(Block {
            instructions,
            source,
            end: pc,
        }))
    }
}

fn single_matches(conditional: &Block, single: &Block, c: &CodeSelectorConsumer) -> bool {
    let bank = conditional.word(0);
    let active = conditional.word(1);
    let flags = conditional.word(3);
    if ![
        bank,
        active,
        flags,
        single.word(12),
        single.word(13),
        single.word(14),
        single.word(15),
    ]
    .into_iter()
    .all(|address| (0x100..=0x700).contains(&address))
    {
        return false;
    }
    conditional.branch(2) == Some(single.pc(6))
        && conditional.byte(4) == 0x20
        && conditional.word(6) == active
        && conditional.branch(8) == Some(single.pc(6))
        && single.byte(0) == 0
        && single.branch(1) == Some(single.pc(4))
        && single.byte(2) != 0
        && single.branch(3) == Some(single.pc(5))
        && single.byte(4) == 0
        && single.word(5) == bank
        && single.byte(6) == 0
        && single.byte(7) == c.pointer_address
        && single.word(8) == flags
        && single.byte(9) == c.selector_address
        && single.word(10) == active
        && single.word(12) == active + 3
        && single.word(15) == single.word(14) + 1
        && single.byte(17) == c.pointer_address
        && single.word(18) == active + 1
        && single.byte(20) == c.pointer_address
        && single.word(21) == active + 2
        && flags == active + 4
}

fn helper_matches(helper: &Block, source_slots: &[u8], pointer: u8) -> bool {
    let first = helper.byte(2);
    let second = helper.byte(6);
    first != second
        && !source_slots.contains(&first)
        && !source_slots.contains(&second)
        && helper.byte(1) == 1
        && helper.byte(7) == first
        && helper.byte(8) == 7
        && helper.byte(10) == second
        && helper.byte(12) == 3
        && helper.byte(13) == second
        && helper.byte(15) == second
        && helper.byte(16) == pointer
        && helper.branch(18) == Some(helper.pc(20))
}

fn validated_triple_state_pairs(triple: &Block, c: &CodeSelectorConsumer) -> Option<[u16; 3]> {
    if triple.byte(1) != 2
        || triple.byte(2) != c.selector_address
        || !(0x100..0x800).contains(&triple.word(3))
    {
        return None;
    }
    let destinations = [5, 8, 11, 14, 17, 20].map(|i| triple.word(i));
    ([4, 7, 10, 13, 16, 19]
        .into_iter()
        .all(|i| triple.byte(i) == c.pointer_address)
        && destinations
            .iter()
            .all(|address| (0x100..0x800).contains(address))
        && destinations
            .as_chunks::<2>()
            .0
            .iter()
            .all(|pair| pair[0].checked_add(1) == Some(pair[1]))
        && distinct(&destinations))
    .then_some([destinations[0], destinations[2], destinations[4]])
}

struct Instruction {
    pc: u16,
    operand: [u8; 2],
    branch: Option<u16>,
}

struct Block {
    instructions: Vec<Instruction>,
    source: FileSpan,
    end: u16,
}

impl Block {
    fn byte(&self, i: usize) -> u8 {
        self.instructions[i].operand[0]
    }
    fn word(&self, i: usize) -> u16 {
        u16::from_le_bytes(self.instructions[i].operand)
    }
    fn pc(&self, i: usize) -> u16 {
        self.instructions[i].pc
    }
    fn branch(&self, i: usize) -> Option<u16> {
        self.instructions[i].branch
    }
    fn end(&self) -> u16 {
        self.end
    }
    fn evidence(&self, nrom: &Nrom<'_>, signature: &'static str) -> CandidateEvidence {
        evidence_at(nrom, signature, EvidenceKind::InstructionBytes, self.source)
    }
}

fn distinct<T: Eq>(values: &[T]) -> bool {
    values
        .iter()
        .enumerate()
        .all(|(i, value)| !values[..i].contains(value))
}

fn contains(outer: FileSpan, inner: FileSpan) -> bool {
    inner.offset >= outer.offset && inner.offset < outer.offset + outer.byte_len
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
