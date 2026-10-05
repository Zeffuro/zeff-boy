use anyhow::{Result, ensure};
use zeff_emu_common::save_state::StateWriter;
use zeff_pce_core::hardware::{
    ControllerDevice, FivePortMultitap, MultitapDevice, PadButtons, PceControllerMode,
    PceMemoryBaseMode, PceRollbackSession, PceRollbackSnapshot, TwoButtonPad,
};

use super::{BACKEND_STATE_MAGIC, BACKEND_STATE_VERSION, PceBackend};
use crate::emu_core_trait::EmulatorCore;
use crate::settings::{PceOverscanMode, PcePaletteMode};

#[derive(Clone, Copy)]
pub(crate) struct PceNetplayLoadProvenance {
    pub(crate) source: [u8; 32],
    pub(crate) source_len: u64,
    pub(crate) authenticated_source: bool,
    pub(crate) unmodified: bool,
    pub(crate) neutral_input: bool,
    pub(crate) state: [u8; 32],
    pub(crate) runtime: [u8; 32],
    pub(crate) persistent: [u8; 32],
    pub(crate) sample_rate: u32,
    pub(crate) config: [u8; 32],
}

pub(crate) struct PceBackendRollbackSession {
    core: PceRollbackSession,
}

pub(crate) struct PceBackendRollbackSnapshot {
    core: PceRollbackSnapshot,
    frame: u64,
    overscan: PceOverscanMode,
    palette: PcePaletteMode,
}

impl PceBackendRollbackSnapshot {
    pub(crate) fn frame(&self) -> u64 {
        self.frame
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.core.retained_bytes()
    }

    pub(crate) fn native_state_parts(&self) -> ([u8; 25], &[u8]) {
        let state = self.core.native_state();
        let length = u32::try_from(state.len()).expect("owned PC Engine snapshot is bounded");
        let mut header = [0; 25];
        header[..8].copy_from_slice(BACKEND_STATE_MAGIC);
        header[8..12].copy_from_slice(&BACKEND_STATE_VERSION.to_le_bytes());
        header[12..20].copy_from_slice(&self.frame.to_le_bytes());
        header[21..25].copy_from_slice(&length.to_le_bytes());
        (header, state)
    }

    fn checkpoint(&self) -> Vec<u8> {
        let mut writer = StateWriter::with_capacity(self.core.native_state().len() + 32);
        writer.write_bytes(BACKEND_STATE_MAGIC);
        writer.write_u32(BACKEND_STATE_VERSION);
        writer.write_u64(self.frame);
        writer.write_u8(0);
        writer.write_vec(self.core.native_state());
        writer.into_bytes()
    }
}

impl PceBackend {
    pub(crate) fn configure_netplay_controllers(&mut self) -> Result<()> {
        ensure!(
            self.frame_count == 0 && self.machine.frame_count() == 0,
            "netplay controllers must be configured before execution"
        );
        self.machine
            .devices_mut()
            .set_controller_device(ControllerDevice::Multitap(FivePortMultitap::new([
                MultitapDevice::TwoButton(TwoButtonPad::new()),
                MultitapDevice::TwoButton(TwoButtonPad::new()),
                MultitapDevice::Disconnected,
                MultitapDevice::Disconnected,
                MultitapDevice::Disconnected,
            ])));
        self.pce_controller_mode = PceControllerMode::Multitap;
        self.update_memory_base_mode(PceMemoryBaseMode::Disabled);
        self.mouse_host_buttons = PadButtons::empty();
        Ok(())
    }

    pub(crate) fn validate_netplay_boundary(&self) -> Result<()> {
        ensure!(
            self.pending_runtime_fault.is_none() && self.frame_count == self.machine.frame_count(),
            "PC Engine backend is faulted or frame counters differ"
        );
        ensure!(
            self.mouse_host_buttons.is_empty()
                && self.pce_controller_mode == PceControllerMode::Multitap
                && self.pce_memory_base_mode == PceMemoryBaseMode::Disabled,
            "PC Engine netplay excludes host peripherals"
        );
        ensure!(
            self.overscan_mode == PceOverscanMode::Full
                && self.palette_mode == PcePaletteMode::RawRgb,
            "PC Engine netplay requires full raw RGB presentation"
        );
        self.machine.validate_rollback_boundary()
    }

    pub(crate) fn begin_netplay_rollback(&mut self) -> Result<PceBackendRollbackSession> {
        self.validate_netplay_boundary()?;
        Ok(PceBackendRollbackSession {
            core: self.machine.begin_rollback_session()?,
        })
    }

    pub(crate) fn netplay_runtime_state_bytes(&self) -> Vec<u8> {
        let mut bytes = self.machine.encode_rollback_runtime_state();
        bytes.extend_from_slice(&self.frame_count.to_le_bytes());
        bytes.extend_from_slice(&self.netplay_config_bytes());
        bytes
    }

    pub(crate) fn netplay_persistent_state_bytes(&self) -> Vec<u8> {
        self.machine
            .hucard_ram()
            .map_or_else(Vec::new, |ram| ram.to_vec())
    }

    pub(crate) fn netplay_config_bytes(&self) -> Vec<u8> {
        format!(
            "{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}",
            self.machine.hucard_board(),
            self.machine.hardware_topology(),
            self.machine.devices().console_wiring(),
            self.machine.devices().psg().revision(),
            self.pce_controller_mode,
            self.overscan_mode,
            self.palette_mode
        )
        .into_bytes()
    }

    pub(crate) fn capture_netplay_load_provenance(
        &mut self,
        source: [u8; 32],
        source_len: usize,
        authenticated_source: bool,
        unmodified: bool,
        neutral_input: bool,
    ) -> Result<()> {
        if self.machine.devices().cdrom2().is_some() {
            return Ok(());
        }
        let extension = self
            .paths
            .rom_path()
            .extension()
            .and_then(|value| value.to_str());
        self.netplay_load_provenance = Some(PceNetplayLoadProvenance {
            source,
            source_len: source_len as u64,
            authenticated_source: authenticated_source
                && extension.is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("pce") || ext.eq_ignore_ascii_case("sgx")
                }),
            unmodified,
            neutral_input,
            state: zeff_firmware::sha256_bytes(&self.encode_state_bytes()?),
            runtime: zeff_firmware::sha256_bytes(&self.netplay_runtime_state_bytes()),
            persistent: zeff_firmware::sha256_bytes(&self.netplay_persistent_state_bytes()),
            sample_rate: self.machine.rollback_sample_rate(),
            config: zeff_firmware::sha256_bytes(&self.netplay_config_bytes()),
        });
        Ok(())
    }

    pub(crate) fn netplay_load_provenance(&self) -> Option<PceNetplayLoadProvenance> {
        self.netplay_load_provenance
    }

    pub(crate) fn host_persistence_enabled(&self) -> bool {
        self.host_persistence_enabled
    }

    pub(crate) fn set_host_persistence_enabled(&mut self, enabled: bool) {
        self.host_persistence_enabled = enabled;
    }
}

impl PceBackendRollbackSession {
    fn validate(&self, backend: &PceBackend) -> Result<()> {
        let result = backend.validate_netplay_boundary();
        if result.is_err() {
            self.core.retire();
        }
        result
    }

    pub(crate) fn capture(&self, backend: &PceBackend) -> Result<PceBackendRollbackSnapshot> {
        self.validate(backend)?;
        Ok(PceBackendRollbackSnapshot {
            core: self.core.capture(&backend.machine)?,
            frame: backend.frame_count,
            overscan: backend.overscan_mode,
            palette: backend.palette_mode,
        })
    }

    pub(crate) fn restore(
        &self,
        backend: &mut PceBackend,
        snapshot: &PceBackendRollbackSnapshot,
    ) -> Result<()> {
        self.validate(backend)?;
        self.core.restore(&mut backend.machine, &snapshot.core)?;
        self.restore_presentation(backend, snapshot);
        Ok(())
    }

    pub(crate) fn restore_after_session(
        &self,
        backend: &mut PceBackend,
        snapshot: &PceBackendRollbackSnapshot,
        checkpoint: &[u8],
    ) -> Result<()> {
        if checkpoint != snapshot.checkpoint() {
            self.core.retire();
            anyhow::bail!("PC Engine rollback restoration checkpoint differs");
        }
        self.core.restore_after_session(
            &mut backend.machine,
            &snapshot.core,
            snapshot.core.native_state(),
        )?;
        self.restore_presentation(backend, snapshot);
        Ok(())
    }

    fn restore_presentation(
        &self,
        backend: &mut PceBackend,
        snapshot: &PceBackendRollbackSnapshot,
    ) {
        backend.frame_count = snapshot.frame;
        backend.overscan_mode = snapshot.overscan;
        backend.palette_mode = snapshot.palette;
        backend.pce_controller_mode = PceControllerMode::Multitap;
        backend.pce_memory_base_mode = PceMemoryBaseMode::Disabled;
        backend.mouse_host_buttons = PadButtons::empty();
        backend.pending_runtime_fault = None;
        backend.memory_base_force_flush = false;
        backend.invalidate_frame_output();
    }

    pub(crate) fn advance_frame(
        &self,
        backend: &mut PceBackend,
        ports: [u8; 2],
    ) -> Result<Vec<f32>> {
        self.validate(backend)?;
        match self.core.advance_frame(&mut backend.machine, ports) {
            Ok(audio) => {
                backend.frame_count = backend.machine.frame_count();
                backend.invalidate_frame_output();
                Ok(audio)
            }
            Err(error) => {
                backend.pending_runtime_fault = Some(error.to_string());
                Err(error)
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
