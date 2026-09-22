use serde::Serialize;

use crate::tracker::FileSpan;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DriverCandidate {
    pub family: &'static str,
    pub variant: &'static str,
    pub qualification: CandidateQualification,
    pub fingerprint_source: &'static str,
    pub fingerprint_revision: &'static str,
    pub evidence: Vec<CandidateEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inventory: Option<StructuralInventory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<CodeInventory>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateQualification {
    FingerprintOnly,
    StaticCode,
    Structural,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CandidateEvidence {
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
    SoundRegisterWrite,
    DirectCall,
    DriverData,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodeInventory {
    pub writes: Vec<CodeWrite>,
    pub calls: Vec<CodeCall>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub command_dispatches: Vec<CodeCommandDispatch>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub selector_consumers: Vec<CodeSelectorConsumer>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodeCommandDispatch {
    pub entry_cpu_address: u16,
    pub entry_span: FileSpan,
    pub table_cpu_address: u16,
    pub target_pointer_address: u8,
    pub saved_index_address: u8,
    pub fetches: Vec<CodeCommandFetch>,
    pub evidence: Vec<CandidateEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodeCommandFetch {
    pub audio_call: CodeCall,
    pub cpu_address: u16,
    pub span: FileSpan,
    pub source_pointer_address: u8,
    pub call_cpu_address: u16,
    pub call_span: FileSpan,
    pub event_cpu_address: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodeSelectorConsumer {
    pub audio_call: CodeCall,
    pub call_cpu_address: u16,
    pub call_span: FileSpan,
    pub entry_cpu_address: u16,
    pub entry_span: FileSpan,
    pub selector_address: u8,
    pub upper_bound_exclusive: u8,
    pub pointer_address: u8,
    pub header_address: u8,
    pub table_cpu_address: u16,
    pub pointer_aperture: FileSpan,
    pub control_address: u16,
    pub controls: Vec<CodeSelectorControl>,
    pub pointers: Vec<CodeSelectorPointer>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<CodeRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodeRecord {
    pub raw_selector: u8,
    pub header: u8,
    pub prefix_span: FileSpan,
    pub streams: Vec<CodeStreamPointer>,
    pub evidence: Vec<CandidateEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodeStreamPointer {
    pub entry_span: FileSpan,
    pub target_cpu_address: u16,
    pub target_span: Option<FileSpan>,
    pub disposition: CodePointerDisposition,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_binding: Option<CodeStreamBinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditional_head_command_edge: Option<CodeConditionalHeadCommandEdge>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodeConditionalHeadCommandEdge {
    pub head_byte: u8,
    pub head_span: FileSpan,
    pub dispatch_row_span: FileSpan,
    pub handler_cpu_address: u16,
    pub handler_span: FileSpan,
    pub operand_count: u8,
    pub destination_start: u16,
    pub destination_end_inclusive: u16,
    pub evidence: Vec<CandidateEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodeStreamBinding {
    pub state_pointer_address: u16,
    pub scheduler_cpu_address: u16,
    pub scheduler_span: FileSpan,
    pub consumer_entry_cpu_address: u16,
    pub consumer_entry_span: FileSpan,
    pub fetch_cpu_address: u16,
    pub fetch_span: FileSpan,
    pub evidence: Vec<CandidateEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodeSelectorControl {
    pub raw_selector: u8,
    pub value: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodeSelectorPointer {
    pub raw_selector: u8,
    pub entry_span: FileSpan,
    pub target_cpu_address: u16,
    pub target_span: Option<FileSpan>,
    pub disposition: CodePointerDisposition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodePointerDisposition {
    Unparsed,
    Unmapped,
    DecodedCode,
    PointerTable,
    RecordPrefix,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodeWrite {
    pub cpu_address: u16,
    pub register: u16,
    pub span: FileSpan,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodeCall {
    pub cpu_address: u16,
    pub target_cpu_address: u16,
    pub span: FileSpan,
    pub writer_cpu_address: u16,
    pub writer_span: FileSpan,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StructuralInventory {
    pub mapped_window: crate::RomSpan,
    pub descriptor_probe: FileSpan,
    pub selector_input_count: u16,
    pub inspected_selectors: u16,
    pub entries: Vec<StructuralSelection>,
    pub held: Vec<SelectorHold>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StructuralSelection {
    pub raw_selector: u16,
    pub slot_base: u8,
    pub descriptor: FileSpan,
    pub tracks: Vec<StructuralTrack>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StructuralTrack {
    pub channel: u8,
    pub note_count: u32,
    pub source_spans: Vec<FileSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SelectorHold {
    pub raw_selector: u16,
    pub reason: SelectorHoldReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorHoldReason {
    SourceOverlap,
    SequenceOutOfRange,
    PointerRebase,
    UnsupportedCommand,
    NonYieldingLoop,
    Restart,
    CommandBatchLimit,
    WalkLimit,
    NoNotes,
}
