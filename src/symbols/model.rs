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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecMode {
    Sm83,
    Mos6502,
    Z80,
    Arm,
    Thumb,
    V30,
    Unknown,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DebugAccess {
    Read,
    Write,
}
