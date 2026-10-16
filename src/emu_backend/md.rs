use std::path::{Path, PathBuf};

use zeff_emu_common::time::{FrameLifecycle, MachineTiming, Reset, TimingSnapshot};
use zeff_md_core::{ControllerKind, ControllerPort, Emulator};

use super::paths::BackendPaths;
use crate::emu_core_trait::EmulatorCore;

mod input;
mod persistence;
pub(crate) use persistence::try_load_save_ram;

pub(crate) struct MdBackend {
    pub(crate) emu: Emulator,
    paths: BackendPaths,
    rom_hash: [u8; 32],
    suspended: bool,
    runtime_fault: Option<String>,
    sram_recovery: crate::save_paths::SramRecoverySession,
    host_persistence_enabled: bool,
}

impl MdBackend {
    pub(crate) fn new(
        emu: Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
        rom_hash: [u8; 32],
    ) -> Self {
        let sram_recovery = crate::save_paths::battery_sram_session(
            &rom_path,
            super::ActiveSystem::Md.storage_subdir(),
            rom_hash,
        );
        Self {
            emu,
            paths: BackendPaths::with_source_path(rom_path, source_path),
            rom_hash,
            suspended: false,
            runtime_fault: None,
            sram_recovery,
            host_persistence_enabled: true,
        }
    }

    pub(crate) fn set_controllers(&mut self, kind: ControllerKind) {
        for port in [ControllerPort::One, ControllerPort::Two] {
            self.emu.input_mut().set_controller_kind(port, kind);
        }
    }

    pub(crate) fn nominal_frame_duration_ns(&self) -> u64 {
        self.emu.nominal_frame_duration_ns()
    }

    pub(crate) fn display_framebuffer(&self) -> (&[u8], Option<(u32, u32)>) {
        let (width, height) = self.emu.framebuffer_dimensions();
        (self.emu.framebuffer(), Some((width as u32, height as u32)))
    }

    pub(crate) fn source_path(&self) -> &Path {
        self.paths.source_path()
    }
    pub(crate) fn firmware_manifests(&self) -> &[zeff_emu_common::replay::ReplayFirmwareManifest] {
        self.paths.firmware_manifests()
    }
    pub(crate) fn set_firmware_manifests(
        &mut self,
        manifests: Vec<zeff_emu_common::replay::ReplayFirmwareManifest>,
    ) {
        self.paths.set_firmware_manifests(manifests);
    }
    pub(crate) fn audio_discovery_input(
        &self,
    ) -> Option<std::sync::Arc<crate::audio_discovery::media::ScanInput>> {
        self.paths.audio_discovery_input()
    }
    pub(crate) fn set_audio_discovery_input(
        &mut self,
        input: std::sync::Arc<crate::audio_discovery::media::ScanInput>,
    ) {
        self.paths.set_audio_discovery_input(input);
    }
}

impl EmulatorCore for MdBackend {
    fn framebuffer(&self) -> &[u8] {
        self.emu.framebuffer()
    }
    fn drain_audio_samples_into(&mut self, samples: &mut Vec<f32>) {
        self.emu.drain_audio_samples_into(samples);
    }
    fn set_sample_rate(&mut self, rate: u32) {
        self.emu.set_sample_rate(rate);
    }
    fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.emu.set_apu_sample_generation_enabled(enabled);
    }
    fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        let psg = std::array::from_fn(|index| mutes.get(index).copied().unwrap_or(false));
        let fm = std::array::from_fn(|index| {
            mutes
                .get(index + zeff_md_core::constants::PSG_CHANNEL_COUNT)
                .copied()
                .unwrap_or(false)
        });
        self.emu.set_apu_channel_mutes(psg);
        self.emu.set_fm_channel_mutes(fm);
    }
    fn set_input(&mut self, buttons: u8, dpad: u8) {
        input::set(&mut self.emu, ControllerPort::One, buttons, dpad);
    }
    fn set_input_p2(&mut self, buttons: u8, dpad: u8) {
        input::set(&mut self.emu, ControllerPort::Two, buttons, dpad);
    }
    fn is_suspended(&self) -> bool {
        self.suspended
    }
    fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        self.flush_save_ram()
    }
    fn save_ram_kind(&self) -> zeff_emu_common::save_ram::SaveRamKind {
        self.emu.save_ram_kind()
    }
    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(
            !self.suspended,
            "cannot save a suspended Mega Drive backend"
        );
        self.emu.encode_state()
    }
    fn load_state_from_bytes(
        &mut self,
        bytes: Vec<u8>,
    ) -> anyhow::Result<zeff_emu_common::StateRestoreOutcome> {
        self.emu.decode_state(&bytes)?;
        self.suspended = false;
        self.runtime_fault = None;
        Ok(zeff_emu_common::StateRestoreOutcome::Exact)
    }
    fn state_restores_framebuffer(&self) -> bool {
        true
    }
    fn supports_save_states(&self) -> bool {
        true
    }
    fn supports_rewind(&self) -> bool {
        false
    }
    fn supports_replay(&self) -> bool {
        false
    }
    fn rom_path(&self) -> &Path {
        self.paths.rom_path()
    }
    fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }
    fn supports_audio(&self) -> bool {
        true
    }
    fn cpu_address_bits(&self) -> u8 {
        24
    }
    fn debug_suspend(&mut self) {
        self.suspended = true;
    }
    fn take_runtime_fault(&mut self) -> Option<String> {
        self.runtime_fault.take()
    }
}

impl MachineTiming for MdBackend {
    fn timing_snapshot(&self) -> TimingSnapshot {
        self.emu.timing_snapshot()
    }
}

impl Reset for MdBackend {
    fn reset(&mut self) {
        self.emu.reset();
        self.suspended = false;
        self.runtime_fault = None;
    }
}

impl FrameLifecycle for MdBackend {
    fn step_frame(&mut self) {
        if self.suspended {
            return;
        }
        let field = self.emu.pacing_field_count();
        self.emu.clear_frame_ready();
        while self.emu.pacing_field_count() == field {
            if let Err(fault) = self.emu.step_instruction() {
                self.suspended = true;
                self.runtime_fault = Some(format!("Mega Drive machine fault: {fault:?}"));
                break;
            }
        }
    }
    fn frame_count(&self) -> u64 {
        self.emu.pacing_field_count()
    }
}
