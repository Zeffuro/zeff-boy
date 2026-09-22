use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};
use zeff_emu_common::audio_trace::{AudioTraceSource, NesAudioTrace, NesTraceWrite};

use super::nes_source::{Nrom, identity, source_matches_report};

const SCHEMA: &str = "zeff-audio-writer-correlation/1";
const QUALIFICATION: &str = "observed_writes_only";
const MAX_CANDIDATES: usize = 64;
const MAX_WRITERS: usize = 256;
const MAX_EVENTS: usize = 262_144;

pub(super) fn summarize(
    evidence: &Value,
    row: &Value,
    source: &[u8],
    trace: &NesAudioTrace,
    cancel: &AtomicBool,
) -> Value {
    let identity = identity(evidence, row, trace);
    let unavailable = |reason| unavailable(reason, identity.clone());
    if cancel.load(Ordering::Relaxed) {
        return unavailable("cancelled");
    }
    if !super::evidence::binding_matches(evidence, row) {
        return unavailable("unbound_capture_or_static_evidence");
    }
    let report = &evidence["report"];
    if report.pointer("/media/system") != Some(&json!("nes"))
        || report.pointer("/status/kind") != Some(&json!("complete"))
    {
        return unavailable("incomplete_or_non_nes_evidence");
    }
    if !source_matches_report(source, report) {
        return unavailable("source_identity_mismatch");
    }
    let Some(nrom) = Nrom::parse(source) else {
        return unavailable("unsupported_nrom_header");
    };
    if trace.events.len() > MAX_EVENTS || trace.validate_complete().is_err() {
        return unavailable("incomplete_trace");
    }
    let candidates = match report.get("driver_candidates") {
        None => return unavailable("no_nrom_static_writers"),
        Some(Value::Array(candidates)) => candidates,
        Some(_) => return unavailable("invalid_candidate_inventory"),
    };
    if candidates.len() > MAX_CANDIDATES {
        return unavailable("candidate_limit");
    }

    let mut writers = BTreeMap::new();
    for (candidate_index, candidate) in candidates.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return unavailable("cancelled");
        }
        if !is_nrom_candidate(candidate) {
            continue;
        }
        let Some(code_writes) = candidate.pointer("/code/writes").and_then(Value::as_array) else {
            return unavailable("invalid_candidate_inventory");
        };
        if code_writes.len() > MAX_WRITERS || writers.len() + code_writes.len() > MAX_WRITERS {
            return unavailable("writer_limit");
        }
        for (writer_index, writer) in code_writes.iter().enumerate() {
            let Some(writer) = StaticWriter::parse(
                candidate_index,
                writer_index,
                candidate,
                writer,
                source,
                nrom,
            ) else {
                return unavailable("invalid_static_writer");
            };
            if writers.insert(writer.key(), writer).is_some() {
                return unavailable("duplicate_static_writer");
            }
        }
    }
    if writers.is_empty() {
        return unavailable("no_nrom_static_writers");
    }

    let mut observed = BTreeMap::<WriterKey, Observation>::new();
    for event in &trace.events {
        if cancel.load(Ordering::Relaxed) {
            return unavailable("cancelled");
        }
        let NesTraceWrite::Register { address, .. } = event.write else {
            continue;
        };
        if event.pc == 0 {
            continue;
        }
        let AudioTraceSource::CartridgeRom {
            offset,
            bit_reversed: false,
        } = event.instruction_source
        else {
            continue;
        };
        let key = WriterKey {
            pc: event.pc,
            offset,
            register: address,
        };
        if writers.contains_key(&key) {
            observed.entry(key).or_default().record(event.cycle);
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return unavailable("cancelled");
    }

    let candidates = writers
        .values()
        .map(|writer| writer.candidate_row(observed.get(&writer.key())))
        .collect::<Vec<_>>();
    json!({
        "schema": SCHEMA,
        "qualification": QUALIFICATION,
        "status": "complete",
        "identity": identity,
        "candidates": group_candidates(candidates),
        "limitations": [
            "Observed writers associate exact instruction-origin APU register accesses only.",
            "Calls and selector metadata remain static references and are not execution evidence.",
            "Observed writers do not establish songs, selectors, durations, loops, or export items.",
        ],
    })
}

fn unavailable(reason: &str, identity: Value) -> Value {
    json!({
        "schema": SCHEMA,
        "qualification": QUALIFICATION,
        "status": "unavailable",
        "reason": reason,
        "identity": identity,
        "candidates": [],
    })
}

fn is_nrom_candidate(candidate: &Value) -> bool {
    candidate["family"] == "NES APU access"
        && candidate["variant"] == "nrom-vector-code"
        && candidate["qualification"] == "static_code"
}

#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
struct WriterKey {
    pc: u32,
    offset: u64,
    register: u16,
}

struct StaticWriter {
    writer_index: usize,
    identity: Value,
    key: WriterKey,
    span: Value,
    instruction: Vec<u8>,
    instruction_sha256: String,
}

impl StaticWriter {
    fn parse(
        candidate_index: usize,
        writer_index: usize,
        candidate: &Value,
        writer: &Value,
        source: &[u8],
        nrom: Nrom,
    ) -> Option<Self> {
        let cpu_address = u16::try_from(writer["cpu_address"].as_u64()?).ok()?;
        let register = u16::try_from(writer["register"].as_u64()?).ok()?;
        let offset = writer.pointer("/span/offset")?.as_u64()?;
        let byte_len = writer.pointer("/span/byte_len")?.as_u64()?;
        if byte_len != 3
            || (0..3).any(|delta| {
                cpu_address
                    .checked_add(delta)
                    .and_then(|address| nrom.offset_for(address))
                    != offset.checked_add(u64::from(delta))
            })
        {
            return None;
        }
        let end = usize::try_from(offset.checked_add(byte_len)?).ok()?;
        let start = usize::try_from(offset).ok()?;
        let instruction = source.get(start..end)?.to_vec();
        if !matches!(instruction.as_slice(), [0x8c..=0x8e, _, _])
            || u16::from_le_bytes([instruction[1], instruction[2]]) != register
            || !matches!(register, 0x4000..=0x4013 | 0x4015 | 0x4017)
        {
            return None;
        }
        let instruction_sha256 = zeff_firmware::sha256_hex(&instruction);
        let authenticated = candidate["evidence"].as_array()?.iter().any(|evidence| {
            evidence["kind"] == "sound_register_write"
                && evidence["span"] == writer["span"]
                && evidence["sha256"] == instruction_sha256
        });
        authenticated.then_some(Self {
            writer_index,
            identity: json!({
                "candidate_index": candidate_index,
                "family": candidate["family"],
                "variant": candidate["variant"],
                "qualification": candidate["qualification"],
                "fingerprint_revision": candidate["fingerprint_revision"],
            }),
            key: WriterKey {
                pc: u32::from(cpu_address),
                offset,
                register,
            },
            span: writer["span"].clone(),
            instruction,
            instruction_sha256,
        })
    }

    fn key(&self) -> WriterKey {
        self.key.clone()
    }

    fn candidate_row(&self, observation: Option<&Observation>) -> Value {
        json!({
            "candidate": self.identity,
            "writer_index": self.writer_index,
            "pc": self.key.pc,
            "register": self.key.register,
            "instruction_source": {
                "kind": "cartridge_rom", "offset": self.key.offset, "bit_reversed": false,
            },
            "span": self.span,
            "instruction": {
                "bytes": const_hex::encode(&self.instruction), "sha256": self.instruction_sha256,
            },
            "observed": observation.map_or_else(
                || json!({"count": 0, "first_cycle": null, "last_cycle": null}),
                |observation| json!({
                    "count": observation.count, "first_cycle": observation.first_cycle,
                    "last_cycle": observation.last_cycle,
                }),
            ),
        })
    }
}

#[derive(Default)]
struct Observation {
    count: u64,
    first_cycle: u64,
    last_cycle: u64,
}

impl Observation {
    fn record(&mut self, cycle: u64) {
        self.first_cycle = if self.count == 0 {
            cycle
        } else {
            self.first_cycle
        };
        self.last_cycle = cycle;
        self.count = self.count.saturating_add(1);
    }
}

fn group_candidates(rows: Vec<Value>) -> Vec<Value> {
    let mut groups = BTreeMap::<usize, (Value, Vec<Value>)>::new();
    for row in rows {
        let Some(index) = row
            .pointer("/candidate/candidate_index")
            .and_then(Value::as_u64)
        else {
            continue;
        };
        groups
            .entry(index as usize)
            .or_insert_with(|| (row["candidate"].clone(), Vec::new()))
            .1
            .push(row);
    }
    groups
        .into_values()
        .map(|(candidate, writers)| json!({"candidate": candidate, "writers": writers}))
        .collect()
}

#[cfg(test)]
#[path = "correlation/tests.rs"]
mod tests;
