use serde::Serialize;

use crate::{Budget, ScanStop, tracker::FileSpan};

mod patterns;
#[cfg(test)]
mod tests;

use patterns::{FAMILIES, PATTERNS, PatternKind};

pub const SOURCE: &str = "https://github.com/bbbbbr/gbtoolsid";
pub const REVISION: &str = "5ff49ad1282178eaebf47314d0c775c2b13d98b8";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriverCandidate {
    pub family: &'static str,
    pub variant: &'static str,
    pub qualification: FingerprintQualification,
    pub fingerprint_source: &'static str,
    pub fingerprint_revision: &'static str,
    pub evidence: Vec<FingerprintEvidence>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FingerprintQualification {
    FingerprintOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FingerprintEvidence {
    pub signature: &'static str,
    pub kind: EvidenceKind,
    pub span: FileSpan,
    pub sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    TextIdentifier,
    InstructionBytes,
    InstrumentData,
    HeaderIdentifier,
}

impl From<PatternKind> for EvidenceKind {
    fn from(kind: PatternKind) -> Self {
        match kind {
            PatternKind::Text => Self::TextIdentifier,
            PatternKind::Code => Self::InstructionBytes,
            PatternKind::Data => Self::InstrumentData,
            PatternKind::Header => Self::HeaderIdentifier,
        }
    }
}

pub(crate) fn scan(
    bytes: &[u8],
    findings: &mut Vec<DriverCandidate>,
    budget: &mut Budget<'_>,
    capacity: usize,
) -> Result<(), ScanStop> {
    if bytes.len() < 0x150 {
        return Ok(());
    }
    let mut matches = vec![None; PATTERNS.len()];
    let mut buckets: [Vec<usize>; 256] = std::array::from_fn(|_| Vec::new());
    for pattern in PATTERNS {
        let index = pattern.id as usize;
        budget.charge()?;
        if let Some(at) = pattern.at {
            if bytes.get(at..at + pattern.bytes.len()) == Some(pattern.bytes) {
                matches[index] = Some(at);
            }
        } else {
            buckets[usize::from(pattern.bytes[0])].push(index);
        }
    }
    for (block_index, block) in bytes.chunks(64).enumerate() {
        budget.charge()?;
        for (within, &byte) in block.iter().enumerate() {
            let at = block_index * 64 + within;
            for &index in &buckets[usize::from(byte)] {
                if matches[index].is_some() {
                    continue;
                }
                let pattern = PATTERNS[index].bytes;
                if bytes.get(at + 1) != pattern.get(1) {
                    continue;
                }
                budget.charge()?;
                if bytes.get(at..at + pattern.len()) == Some(pattern) {
                    matches[index] = Some(at);
                }
            }
        }
    }
    for group in FAMILIES {
        for rule in group.rules {
            budget.charge()?;
            if !rule
                .required
                .iter()
                .all(|&id| matches[id as usize].is_some())
            {
                continue;
            }
            if findings.len() >= capacity {
                return Err(ScanStop::CandidateLimit);
            }
            let mut evidence = Vec::with_capacity(rule.required.len());
            for &id in rule.required {
                budget.charge()?;
                let pattern = &PATTERNS[id as usize];
                let at = matches[id as usize].expect("matching rule requires every fingerprint");
                evidence.push(FingerprintEvidence {
                    signature: pattern.name,
                    kind: pattern.kind.into(),
                    span: FileSpan {
                        offset: at as u32,
                        byte_len: pattern.bytes.len() as u32,
                    },
                    sha256: zeff_firmware::sha256_hex(&bytes[at..at + pattern.bytes.len()]),
                });
            }
            findings.push(DriverCandidate {
                family: rule.family,
                variant: rule.variant,
                qualification: FingerprintQualification::FingerprintOnly,
                fingerprint_source: SOURCE,
                fingerprint_revision: REVISION,
                evidence,
            });
            break;
        }
    }
    Ok(())
}
