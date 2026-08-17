#![allow(dead_code)]

mod gb;
mod identity;
pub(crate) mod import;
mod model;
mod platform;
mod session;
pub(crate) mod store;
#[cfg(not(target_arch = "wasm32"))]
mod user;

pub(crate) use model::{
    AddressSpaceId, Confidence, CpuLocation, DebugAccess, ExecMode, ImageId, Provenance,
    ProvenanceKind, RegionId, ResolvedDebugLocation, StorageLocation, SymbolId, SymbolKind,
    SymbolLocation, SymbolRecord, SymbolScope, UserSymbolDraft,
};
pub(crate) use session::SymbolSession;

pub(crate) trait DebugAddressResolver {
    fn resolve_exec(&self, cpu: CpuLocation) -> ResolvedDebugLocation;
    fn resolve_data(&self, cpu: CpuLocation, access: DebugAccess) -> ResolvedDebugLocation;
    fn mapping_epoch(&self) -> u64;
}
