use std::collections::BTreeMap;
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::catalog::{FirmwareId, FirmwareVariantId};
use crate::store::ValidationStatus;

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirmwareSelectionManifest {
    External {
        firmware_id: FirmwareId,
        variant: Option<FirmwareVariantId>,
        sha256: [u8; 32],
    },
    Hle {
        firmware_id: FirmwareId,
        implementation: String,
        compatibility_version: u32,
    },
    BuiltinOpenSource {
        firmware_id: FirmwareId,
        implementation: String,
        compatibility_version: u32,
        sha256: [u8; 32],
    },
    Skipped {
        firmware_id: FirmwareId,
        compatibility_version: u32,
    },
}

#[derive(Clone, Debug)]
pub struct ResolvedFirmware {
    pub id: FirmwareId,
    pub variant: Option<FirmwareVariantId>,
    pub bytes: Arc<[u8]>,
    pub sha256: [u8; 32],
    pub original_filename: Option<String>,
    pub validation: ValidationStatus,
}

impl ResolvedFirmware {
    pub fn selection_manifest(&self) -> FirmwareSelectionManifest {
        FirmwareSelectionManifest::External {
            firmware_id: self.id.clone(),
            variant: self.variant.clone(),
            sha256: self.sha256,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ResolvedFirmwareSet {
    entries: BTreeMap<FirmwareId, ResolvedFirmware>,
}

impl ResolvedFirmwareSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, firmware: ResolvedFirmware) {
        self.entries.insert(firmware.id.clone(), firmware);
    }

    pub fn get(&self, id: &str) -> Option<&ResolvedFirmware> {
        self.entries.get(&FirmwareId::from(id))
    }

    pub fn entries(&self) -> impl Iterator<Item = (&FirmwareId, &ResolvedFirmware)> {
        self.entries.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
