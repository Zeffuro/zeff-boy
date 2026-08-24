use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct ImageId(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct RegionId(pub(crate) u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct AddressSpaceId(pub(crate) u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct SymbolId(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct SegmentId(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct LoadInstanceId(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct StorageLocation {
    pub(crate) image: ImageId,
    pub(crate) region: RegionId,
    pub(crate) offset: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct CpuLocation {
    pub(crate) space: AddressSpaceId,
    pub(crate) address: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StorageRange {
    pub(crate) start: StorageLocation,
    pub(crate) size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecMode {
    Sm83,
    Mos6502,
    Z80,
    Arm,
    Thumb,
    V30,
    HuC6280,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DebugSegment {
    pub(crate) id: SegmentId,
    pub(crate) name: String,
    pub(crate) storage: StorageRange,
    pub(crate) linked_cpu: Option<CpuLocation>,
    pub(crate) exec_mode: ExecMode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LoadInstance {
    pub(crate) id: LoadInstanceId,
    pub(crate) segment: SegmentId,
    pub(crate) runtime_base: CpuLocation,
    pub(crate) generation: u64,
    pub(crate) created_cycle: u64,
    pub(crate) active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedLoadInstance {
    pub(crate) instance: LoadInstanceId,
    pub(crate) segment: SegmentId,
    pub(crate) cpu: CpuLocation,
    pub(crate) storage: StorageLocation,
    pub(crate) exec_mode: ExecMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResolvedDebugLocation {
    pub(crate) cpu: CpuLocation,
    pub(crate) storage: Option<StorageLocation>,
    pub(crate) bank: Option<u32>,
    pub(crate) exec_mode: ExecMode,
    pub(crate) mapping_epoch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SymbolLocation {
    pub(crate) cpu: Option<CpuLocation>,
    pub(crate) storage: Option<StorageLocation>,
    pub(crate) bank: Option<u32>,
    pub(crate) exec_mode: ExecMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SymbolKind {
    Function,
    Label,
    Data,
    Constant,
    Section,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SymbolScope {
    Global,
    Local,
    Module,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProvenanceKind {
    Platform,
    Build,
    DebugFormat,
    LinkMap,
    ReverseEngineering,
    User,
    RuntimeInference,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Provenance {
    pub(crate) kind: ProvenanceKind,
    pub(crate) source: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Confidence {
    Exact,
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SymbolRecord {
    pub(crate) id: SymbolId,
    pub(crate) name: String,
    pub(crate) location: SymbolLocation,
    pub(crate) value: Option<u64>,
    pub(crate) size: Option<u64>,
    pub(crate) kind: SymbolKind,
    pub(crate) scope: SymbolScope,
    pub(crate) provenance: Provenance,
    pub(crate) confidence: Confidence,
    pub(crate) comment: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceFile {
    pub(crate) path: String,
    pub(crate) crc32: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceLine {
    pub(crate) location: SymbolLocation,
    pub(crate) size: u64,
    pub(crate) source_file: usize,
    pub(crate) line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UserSymbolDraft {
    pub(crate) name: String,
    pub(crate) location: SymbolLocation,
    pub(crate) value: Option<u64>,
    pub(crate) kind: SymbolKind,
    pub(crate) size: Option<u64>,
    pub(crate) comment: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DebugAccess {
    Read,
    Write,
}
