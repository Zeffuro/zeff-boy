use serde::{Deserialize, Serialize};

use crate::drivers::{
    CandidateQualification, CodePointerDisposition, DriverCandidate, EvidenceKind,
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeCoverage {
    pub candidates: usize,
    pub sound_register_writes: usize,
    #[serde(default)]
    pub direct_calls: usize,
    #[serde(default)]
    pub selector_consumers: usize,
    #[serde(default)]
    pub unparsed_selector_pointers: usize,
    #[serde(default)]
    pub held_selector_pointers: usize,
    #[serde(default)]
    pub record_prefixes: usize,
    #[serde(default)]
    pub unparsed_stream_pointers: usize,
    #[serde(default)]
    pub held_stream_pointers: usize,
    #[serde(default)]
    pub command_dispatches: usize,
    #[serde(default)]
    pub guarded_command_fetches: usize,
    #[serde(default)]
    pub stream_fetch_bindings: usize,
    #[serde(default)]
    pub conditional_head_command_edges: usize,
}

impl CodeCoverage {
    pub(super) fn add(&mut self, candidate: &DriverCandidate) {
        if candidate.qualification != CandidateQualification::StaticCode {
            return;
        }
        self.candidates += 1;
        self.sound_register_writes += candidate
            .evidence
            .iter()
            .filter(|e| e.kind == EvidenceKind::SoundRegisterWrite)
            .count();
        let Some(inventory) = &candidate.code else {
            return;
        };
        self.direct_calls += inventory.calls.len();
        self.command_dispatches += inventory.command_dispatches.len();
        self.guarded_command_fetches += inventory
            .command_dispatches
            .iter()
            .map(|dispatch| dispatch.fetches.len())
            .sum::<usize>();
        self.selector_consumers += inventory.selector_consumers.len();
        for consumer in &inventory.selector_consumers {
            for pointer in &consumer.pointers {
                let unparsed = pointer.disposition == CodePointerDisposition::Unparsed;
                self.unparsed_selector_pointers += usize::from(unparsed);
                self.held_selector_pointers += usize::from(!unparsed);
            }
            self.record_prefixes += consumer.records.len();
            for stream in consumer.records.iter().flat_map(|record| &record.streams) {
                self.stream_fetch_bindings += usize::from(stream.fetch_binding.is_some());
                self.conditional_head_command_edges +=
                    usize::from(stream.conditional_head_command_edge.is_some());
                let unparsed = stream.disposition == CodePointerDisposition::Unparsed;
                self.unparsed_stream_pointers += usize::from(unparsed);
                self.held_stream_pointers += usize::from(!unparsed);
            }
        }
    }
}
