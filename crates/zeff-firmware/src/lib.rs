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
    FirmwareSpec, FirmwareVariantId, FirmwareVariantSpec, KnownHashes,
    PCE_SYSTEM_CARD_ADPCM_FIXTURE_SHA256, PCE_SYSTEM_CARD_V1_JAPAN_SHA256,
    PCE_SYSTEM_CARD_V2_JAPAN_SHA256, PCE_SYSTEM_CARD_V2_USA_SHA256,
    PCE_SYSTEM_CARD_V3_JAPAN_SHA256, PCE_SYSTEM_CARD_V3_USA_SHA256, PceSystemCardBoard,
    PceSystemCardFirmware, PceSystemCardRegion, PceSystemCardTier, RequirementLevel, SizeRule,
    catalog_specs, classify_pce_system_card_sha256, firmware_plan_for_existing_core,
    firmware_plan_for_famicom_disk_system, firmware_plan_for_pce_cdrom2,
    firmware_plan_for_pce_cdrom2_fixture, firmware_plan_for_pce_cdrom2_region,
};
pub use digest::{DigestSet, sha256_bytes, sha256_hex};
pub use manifest::{FirmwareSelectionManifest, ResolvedFirmware, ResolvedFirmwareSet};
pub use resolver::{
    FirmwareCandidate, FirmwareResolution, FirmwareResolutionEntry, FirmwareResolver,
    FirmwareSelection, ResolveFailure,
};
pub use store::{FirmwareInventory, FirmwareInventoryEntry, ValidationStatus};
