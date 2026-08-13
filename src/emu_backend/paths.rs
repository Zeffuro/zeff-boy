use std::path::{Path, PathBuf};

use zeff_emu_common::replay::ReplayFirmwareManifest;

pub(crate) struct BackendPaths {
    rom_path: PathBuf,
    source_path: PathBuf,
    firmware_manifests: Vec<ReplayFirmwareManifest>,
}

impl BackendPaths {
    pub(crate) fn new(rom_path: PathBuf) -> Self {
        Self::with_source_path(rom_path.clone(), rom_path)
    }

    pub(crate) fn with_source_path(rom_path: PathBuf, source_path: PathBuf) -> Self {
        Self {
            rom_path,
            source_path,
            firmware_manifests: Vec::new(),
        }
    }

    pub(crate) fn rom_path(&self) -> &Path {
        &self.rom_path
    }

    pub(crate) fn source_path(&self) -> &Path {
        &self.source_path
    }

    pub(crate) fn firmware_manifests(&self) -> &[ReplayFirmwareManifest] {
        &self.firmware_manifests
    }

    pub(crate) fn set_firmware_manifests(
        &mut self,
        firmware_manifests: Vec<ReplayFirmwareManifest>,
    ) {
        self.firmware_manifests = firmware_manifests;
    }
}
