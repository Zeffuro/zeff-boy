#![allow(dead_code)]

mod coleco;
mod gb;
mod identity;
pub(crate) mod import;
mod model;
mod nes;
pub(crate) mod pce;
mod platform;
mod sega8;
mod session;
pub(crate) mod store;
#[cfg(not(target_arch = "wasm32"))]
mod user;
mod ws;

pub(crate) use model::{
    AddressSpaceId, Confidence, CpuLocation, DebugAccess, DebugSegment, ExecMode, ImageId,
    LoadInstance, LoadInstanceId, Provenance, ProvenanceKind, RegionId, ResolvedDebugLocation,
    ResolvedLoadInstance, SegmentId, SourceFile, SourceLine, StorageLocation, StorageRange,
    SymbolId, SymbolKind, SymbolLocation, SymbolRecord, SymbolScope, UserSymbolDraft,
};
pub(crate) use session::{SourceReference, SymbolSession};

pub(crate) trait DebugAddressResolver {
    fn resolve_exec(&self, cpu: CpuLocation) -> ResolvedDebugLocation;
    fn resolve_data(&self, cpu: CpuLocation, access: DebugAccess) -> ResolvedDebugLocation;
    fn mapping_epoch(&self) -> u64;
}
