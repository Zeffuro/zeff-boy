use std::path::{Path, PathBuf};

use anyhow::Context;

use super::{ActiveSystem, EmuBackend};
use crate::emu_core_trait::EmulatorCore;

impl EmuBackend {
    pub(crate) fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        if !self.supports_state_capture() {
            anyhow::bail!("state capture is not supported by this core");
        }
        dispatch!(self, encode_state_bytes())
    }

    pub(crate) fn encode_external_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        if !self.supports_save_states() {
            anyhow::bail!("save states are not supported by this core");
        }
        match self {
            Self::Coleco(backend) => backend.emu.encode_external_state(),
            _ => self.encode_state_bytes(),
        }
    }

    pub(crate) fn rewind_framebuffer(&self) -> &[u8] {
        if dispatch!(self, state_restores_framebuffer()) {
            &[]
        } else {
            self.framebuffer()
        }
    }

    pub(crate) fn encode_replay_hash_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        let mut bytes = self.encode_state_bytes()?;
        canonicalize_state_bytes_for_replay_hash(self.system(), &mut bytes)?;
        Ok(bytes)
    }

    pub(crate) fn encode_replay_start_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        self.encode_state_bytes()
    }

    pub(crate) fn load_state_from_bytes(
        &mut self,
        bytes: Vec<u8>,
    ) -> anyhow::Result<zeff_emu_common::StateRestoreOutcome> {
        if !self.supports_state_capture() {
            anyhow::bail!("state restore is not supported by this core");
        }
        dispatch!(self, load_state_from_bytes(bytes))
    }

    pub(crate) fn load_external_state_from_bytes(
        &mut self,
        bytes: Vec<u8>,
    ) -> anyhow::Result<zeff_emu_common::StateRestoreOutcome> {
        if !self.supports_save_states() {
            anyhow::bail!("save states are not supported by this core");
        }
        if let Self::Coleco(backend) = self {
            return backend
                .emu
                .load_external_state(&bytes)
                .map(|outcome| match outcome {
                    zeff_coleco_core::save_state::ExternalStateRestoreOutcome::Exact => {
                        zeff_emu_common::StateRestoreOutcome::Exact
                    }
                    zeff_coleco_core::save_state::ExternalStateRestoreOutcome::BestEffortPortable => {
                        zeff_emu_common::StateRestoreOutcome::BestEffortPortable
                    }
                });
        }
        self.load_state_from_bytes(bytes)
    }

    pub(crate) fn slot_path(&self, slot: u8) -> anyhow::Result<PathBuf> {
        if !self.supports_save_states() {
            anyhow::bail!("save states are not supported by this core");
        }
        crate::save_paths::slot_path(
            self.system().storage_subdir(),
            self.state_extension(),
            self.rom_hash(),
            slot,
        )
    }

    pub(crate) fn load_state(
        &mut self,
        slot: u8,
    ) -> anyhow::Result<(String, zeff_emu_common::StateRestoreOutcome)> {
        let path = self.slot_path(slot)?;
        let bytes = crate::platform::read_save_data(&path)
            .with_context(|| format!("failed to read save state: {}", path.display()))?
            .ok_or_else(|| anyhow::anyhow!("save state not found: {}", path.display()))?;
        let outcome = self.load_external_state_from_bytes(bytes)?;
        Ok((path.display().to_string(), outcome))
    }

    pub(crate) fn load_state_from_path(
        &mut self,
        path: &Path,
    ) -> anyhow::Result<zeff_emu_common::StateRestoreOutcome> {
        if !self.supports_save_states() {
            anyhow::bail!("save states are not supported by this core");
        }
        let bytes = crate::platform::read_save_data(path)
            .with_context(|| format!("failed to read save state: {}", path.display()))?
            .ok_or_else(|| anyhow::anyhow!("save state not found: {}", path.display()))?;
        self.load_external_state_from_bytes(bytes)
    }
}

pub(crate) fn canonicalize_state_bytes_for_replay_hash(
    system: ActiveSystem,
    bytes: &mut Vec<u8>,
) -> anyhow::Result<()> {
    if system == ActiveSystem::GameBoy {
        zeff_gb_core::save_state::project_replay_state_bytes(bytes)?;
        zeff_gb_core::save_state::canonicalize_replay_hash_bytes(bytes);
    } else if system == ActiveSystem::Nes {
        zeff_nes_core::save_state::project_replay_state_bytes(bytes)?;
    }
    Ok(())
}
