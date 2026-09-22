use serde_json::{Value, json};
use zeff_emu_common::audio_trace::NesAudioTrace;

pub(super) fn identity(evidence: &Value, row: &Value, trace: &NesAudioTrace) -> Value {
    json!({
        "candidate_evidence": {
            "schema": evidence["schema"],
            "qualification": evidence["qualification"],
            "sha256": evidence["sha256"],
        },
        "media": evidence["report"]["media"],
        "capture": {
            "archive_sha256": row.pointer("/capture/archive_sha256"),
            "trace_sha256": row.pointer("/capture/trace_sha256"),
            "context": row.pointer("/capture/context"),
        },
        "trace": {
            "generation": trace.generation,
            "event_count": trace.events.len(),
            "end_cycle": trace.end_cycle,
        },
    })
}

pub(super) fn source_matches_report(source: &[u8], report: &Value) -> bool {
    let hash = zeff_firmware::sha256_hex(source);
    report.pointer("/media/byte_len").and_then(Value::as_u64) == Some(source.len() as u64)
        && report.pointer("/media/sha256").and_then(Value::as_str) == Some(hash.as_str())
}

#[derive(Clone, Copy)]
pub(super) struct Nrom {
    prg_len: usize,
}

impl Nrom {
    pub(super) fn parse(bytes: &[u8]) -> Option<Self> {
        let header = bytes.get(..16)?;
        if &header[..4] != b"NES\x1a"
            || !matches!(header[4], 1 | 2)
            || header[6] & 0xfc != 0
            || header[7] != 0
            || header[8..].iter().any(|value| *value != 0)
        {
            return None;
        }
        let prg_len = usize::from(header[4]) * 0x4000;
        (bytes.len() == 16 + prg_len + usize::from(header[5]) * 0x2000).then_some(Self { prg_len })
    }

    pub(super) fn offset_for(&self, cpu_address: u16) -> Option<u64> {
        let address = usize::from(cpu_address);
        (address >= 0x8000).then(|| {
            let offset = address - 0x8000;
            let mapped = if self.prg_len == 0x4000 {
                offset & 0x3fff
            } else {
                offset
            };
            (16 + mapped) as u64
        })
    }
}
