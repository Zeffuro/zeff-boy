use std::collections::{BTreeMap, BTreeSet};

use crate::{Budget, ScanStop, tracker::FileSpan};

use super::{
    CandidateEvidence, CandidateQualification, CodeCall, CodeInventory, DriverCandidate,
    EvidenceKind,
};

mod decoder;
#[cfg(test)]
mod tests;

const MAX_NODES: usize = 4096;
const MAX_WRITERS: usize = 64;
const MAX_CALL_SITES: usize = 128;
const MAX_CALL_LINKS: usize = 256;

pub(crate) fn scan(
    bytes: &[u8],
    findings: &mut Vec<DriverCandidate>,
    budget: &mut Budget<'_>,
    capacity: usize,
) -> Result<(), ScanStop> {
    budget.charge()?;
    let Some(rom) = decoder::Rom::parse(bytes) else {
        return Ok(());
    };
    let Some(decoded) = decoder::decode(&rom, budget)? else {
        return Ok(());
    };
    if decoded.writes.is_empty() {
        return Ok(());
    }
    if findings.len() >= capacity {
        return Err(ScanStop::CandidateLimit);
    }
    let calls = reverse_calls(&decoded, budget)?;
    let mut evidence = vec![evidence_at(
        bytes,
        if rom.mbc0 {
            "mbc0-immutable-rom-mapping"
        } else {
            "required-lower-window-bank-zero"
        },
        EvidenceKind::HeaderIdentifier,
        FileSpan {
            offset: 0x147,
            byte_len: 7,
        },
    )];
    for address in decoder::ROOTS {
        if let Some(node) = decoded.nodes.get(&address) {
            budget.charge()?;
            evidence.push(evidence_at(
                bytes,
                "reset-or-interrupt-root",
                EvidenceKind::InstructionBytes,
                node.span(address),
            ));
        }
    }
    for write in &decoded.writes {
        budget.charge()?;
        evidence.push(evidence_at(
            bytes,
            "decoded-gb-sound-write",
            EvidenceKind::SoundRegisterWrite,
            write.span,
        ));
    }
    let mut call_spans = BTreeSet::new();
    for call in &calls {
        if call_spans.insert(call.cpu_address) {
            budget.charge()?;
            evidence.push(evidence_at(
                bytes,
                "decoded-gb-sound-caller",
                EvidenceKind::DirectCall,
                call.span,
            ));
        }
    }
    findings.push(DriverCandidate {
        family: "GB sound access",
        variant: if rom.mbc0 {
            "mbc0-vector-code"
        } else {
            "required-bank-zero-vector-code"
        },
        qualification: CandidateQualification::StaticCode,
        fingerprint_source: "GB reset/interrupt-rooted SM83 decoding and reverse call paths",
        fingerprint_revision: "gb-vector-sound-writes/1",
        evidence,
        inventory: None,
        code: Some(CodeInventory {
            writes: decoded.writes,
            calls,
            command_dispatches: Vec::new(),
            selector_consumers: Vec::new(),
        }),
    });
    Ok(())
}

fn reverse_calls(
    decoded: &decoder::Decoded,
    budget: &mut Budget<'_>,
) -> Result<Vec<CodeCall>, ScanStop> {
    let mut reverse = BTreeMap::<u16, Vec<u16>>::new();
    for (&address, node) in &decoded.nodes {
        budget.charge()?;
        for target in node.successors.iter().flatten() {
            if decoded.nodes.contains_key(target) {
                reverse.entry(*target).or_default().push(address);
            }
        }
    }
    let mut calls = Vec::new();
    for write in &decoded.writes {
        let mut pending = vec![write.cpu_address];
        let mut visited = BTreeSet::new();
        while let Some(address) = pending.pop() {
            budget.charge()?;
            if !visited.insert(address) {
                continue;
            }
            if visited.len() > MAX_NODES || pending.len() > MAX_NODES * 2 {
                return Err(ScanStop::ValidationLimit);
            }
            if let Some(predecessors) = reverse.get(&address) {
                pending.extend(predecessors);
            }
        }
        for (&address, node) in &decoded.nodes {
            budget.charge()?;
            if let Some(target) = node.call_target
                && visited.contains(&target)
            {
                if calls.len() == MAX_CALL_LINKS {
                    return Err(ScanStop::InventoryLimit);
                }
                calls.push(CodeCall {
                    cpu_address: address,
                    target_cpu_address: target,
                    span: node.span(address),
                    writer_cpu_address: write.cpu_address,
                    writer_span: write.span,
                });
            }
        }
    }
    calls.sort_by_key(|call| (call.cpu_address, call.writer_cpu_address));
    Ok(calls)
}

fn evidence_at(
    bytes: &[u8],
    signature: &'static str,
    kind: EvidenceKind,
    span: FileSpan,
) -> CandidateEvidence {
    let start = span.offset as usize;
    let end = start + span.byte_len as usize;
    CandidateEvidence {
        signature,
        kind,
        span,
        sha256: zeff_firmware::sha256_hex(&bytes[start..end]),
    }
}
