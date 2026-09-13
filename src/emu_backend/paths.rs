use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use zeff_emu_common::replay::ReplayFirmwareManifest;

pub(crate) struct BackendPaths {
    rom_path: PathBuf,
    source_path: PathBuf,
    firmware_manifests: Vec<ReplayFirmwareManifest>,
    audio_discovery_input: Option<Arc<crate::audio_discovery::media::ScanInput>>,
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
            audio_discovery_input: None,
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

    pub(crate) fn audio_discovery_input(
        &self,
    ) -> Option<Arc<crate::audio_discovery::media::ScanInput>> {
        self.audio_discovery_input.clone()
    }

    pub(crate) fn set_audio_discovery_input(
        &mut self,
        input: Arc<crate::audio_discovery::media::ScanInput>,
    ) {
        self.audio_discovery_input = Some(input);
    }
}
