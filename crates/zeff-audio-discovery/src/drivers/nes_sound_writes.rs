use crate::{Budget, ScanStop, tracker::FileSpan};

use super::{
    CandidateEvidence, CandidateQualification, CodeCall, CodeInventory, CodeWrite, DriverCandidate,
    EvidenceKind,
};

#[cfg(any(test, feature = "test-support"))]
mod binding_tests;
mod decoder;
#[cfg(any(test, feature = "test-support"))]
mod dispatch_tests;
#[cfg(test)]
mod head_edge_tests;
#[cfg(any(test, feature = "test-support"))]
mod record_tests;
#[cfg(any(test, feature = "test-support"))]
mod selector_tests;
#[cfg(any(test, feature = "test-support"))]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub use binding_tests::synthetic_binding_rom;
#[cfg(any(test, feature = "test-support"))]
pub use binding_tests::synthetic_head_edge_rom;
#[cfg(any(test, feature = "test-support"))]
pub use dispatch_tests::synthetic_dispatch_rom;
#[cfg(any(test, feature = "test-support"))]
pub use record_tests::synthetic_record_rom;
#[cfg(any(test, feature = "test-support"))]
pub use selector_tests::synthetic_selector_rom;
#[cfg(any(test, feature = "test-support"))]
pub use tests::synthetic_rom;

const MAX_EVIDENCE: usize = 25;
const MAX_CALL_EVIDENCE: usize = 64;

pub(crate) fn scan(
    bytes: &[u8],
    findings: &mut Vec<DriverCandidate>,
    budget: &mut Budget<'_>,
    capacity: usize,
) -> Result<(), ScanStop> {
    let Some(nrom) = decoder::Nrom::parse(bytes) else {
        return Ok(());
    };
    let Some(decoded) = decoder::decode(&nrom, budget)? else {
        return Ok(());
    };
    if decoded.writes.len() < 2 {
        return Ok(());
    }
    if findings.len() >= capacity {
        return Err(ScanStop::CandidateLimit);
    }

    let mut evidence = decoded
        .vectors
        .iter()
        .map(|vector| {
            evidence_at(
                bytes,
                vector.signature,
                EvidenceKind::DriverData,
                vector.span,
            )
        })
        .collect::<Vec<_>>();
    evidence.extend(decoded.writes.iter().map(|write| {
        evidence_at(
            bytes,
            "decoded-apu-write",
            EvidenceKind::SoundRegisterWrite,
            write.span,
        )
    }));
    debug_assert!(evidence.len() <= MAX_EVIDENCE);
    evidence.extend(decoded.calls.iter().map(|call| {
        evidence_at(
            bytes,
            "decoded-apu-caller",
            EvidenceKind::DirectCall,
            call.span,
        )
    }));
    debug_assert!(decoded.calls.len() <= MAX_CALL_EVIDENCE);
    for consumer in &decoded.selector_consumers {
        evidence.push(evidence_at(
            bytes,
            "decoded-selector-consumer",
            EvidenceKind::InstructionBytes,
            consumer.entry_span,
        ));
        evidence.push(evidence_at(
            bytes,
            "decoded-selector-call",
            EvidenceKind::DirectCall,
            consumer.call_span,
        ));
        evidence.push(evidence_at(
            bytes,
            "decoded-selector-aperture",
            EvidenceKind::DriverData,
            consumer.pointer_aperture,
        ));
    }
    findings.push(DriverCandidate {
        family: "NES APU access",
        variant: "nrom-vector-code",
        qualification: CandidateQualification::StaticCode,
        fingerprint_source: "iNES NROM vector-rooted 6502 decoding",
        fingerprint_revision: "nes-nrom-apu-writes/7",
        evidence,
        inventory: None,
        code: Some(CodeInventory {
            command_dispatches: decoded.command_dispatches,
            selector_consumers: decoded.selector_consumers,
            writes: decoded
                .writes
                .into_iter()
                .map(|write| CodeWrite {
                    cpu_address: write.cpu_address,
                    register: write.register,
                    span: write.span,
                })
                .collect(),
            calls: decoded
                .calls
                .into_iter()
                .map(|call| CodeCall {
                    cpu_address: call.cpu_address,
                    target_cpu_address: call.target_cpu_address,
                    span: call.span,
                    writer_cpu_address: call.writer_cpu_address,
                    writer_span: call.writer_span,
                })
                .collect(),
        }),
    });
    Ok(())
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
