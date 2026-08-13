//! Firmware metadata, validation, and resolution primitives.
//!
//! This crate intentionally stores metadata and user-supplied bytes only. It
//! must not contain proprietary firmware images or download locations.

mod catalog;
mod digest;
mod manifest;
mod resolver;
mod store;

pub use catalog::{
    ExistingCoreSystem, FallbackKind, FirmwareDependency, FirmwareId, FirmwareRequest,
    FirmwareSpec, FirmwareVariantId, FirmwareVariantSpec, KnownHashes, RequirementLevel, SizeRule,
    catalog_specs, firmware_plan_for_existing_core, firmware_plan_for_famicom_disk_system,
};
pub use digest::{DigestSet, sha256_bytes, sha256_hex};
pub use manifest::{FirmwareSelectionManifest, ResolvedFirmware, ResolvedFirmwareSet};
pub use resolver::{
    FirmwareCandidate, FirmwareResolution, FirmwareResolutionEntry, FirmwareResolver,
    FirmwareSelection, ResolveFailure,
};
pub use store::{FirmwareInventory, FirmwareInventoryEntry, ValidationStatus};
