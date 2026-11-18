use std::cell::Cell;
use std::path::{Path, PathBuf};

use zeff_emu_common::time::{FrameLifecycle, MachineTiming, Reset, TimingSnapshot};
use zeff_md_core::{ControllerKind, ControllerPort, Emulator};

use super::paths::BackendPaths;
use crate::emu_core_trait::EmulatorCore;

mod debug;
mod guest_call;
mod input;
mod memory;
mod netplay;
mod ownership;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod tas;
pub(crate) use netplay::{MdNetplayAdmission, MdSessionRestore};
mod rollback;
pub(crate) use ownership::{
    MdBackendRollbackLease, MdBackendRollbackSnapshot, MdOwnershipError, MdWarmResetError,
};
use ownership::{MdExecutionError, MdMachine};
mod persistence;
mod sound_trace;
mod timed_ports;
pub(crate) use persistence::try_load_save_ram;

pub(crate) struct MdBackend {
    machine: MdMachine,
    #[cfg(test)]
    start_failure: Option<crate::netplay::session::md_start_failure::Stage>,
    netplay_load: Option<super::loader::netplay::md::MdNetplayLoad>,
    #[cfg(not(target_arch = "wasm32"))]
    tas_load: Option<super::loader::md_tas::MdTasLoad>,
    #[cfg(not(target_arch = "wasm32"))]
    tas_editor_owner: Option<std::sync::Weak<()>>,
    paths: BackendPaths,
    rom_hash: [u8; 32],
    suspended: bool,
    runtime_fault: Option<String>,
    execution_faulted: bool,
    sram_recovery: crate::save_paths::SramRecoverySession,
    host_persistence_enabled: bool,
    rewind_audio_changed: Cell<bool>,
    audio_generation_enabled: Option<bool>,
    trace_stream: crate::ui::TraceStreamId,
    sound_trace: sound_trace::SoundTraceOwner,
    debugger: debug::PcBreakpoints,
    guest_call_undo: Option<guest_call::GuestCallUndo>,
}

impl MdBackend {
    pub(crate) fn new(
        emu: Emulator,
        rom_path: PathBuf,
        source_path: PathBuf,
        rom_hash: [u8; 32],
        host_persistence_enabled: bool,
    ) -> Self {
        let sram_recovery = if host_persistence_enabled {
            crate::save_paths::battery_sram_session(
                &rom_path,
                super::ActiveSystem::Md.storage_subdir(),
                rom_hash,
            )
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            {
                crate::save_paths::SramRecoverySession::default()
            }
            #[cfg(target_arch = "wasm32")]
            {
                crate::save_paths::SramRecoverySession
            }
        };
        Self {
            machine: MdMachine::Normal(Box::new(emu)),
            #[cfg(test)]
            start_failure: None,
            netplay_load: None,
            #[cfg(not(target_arch = "wasm32"))]
            tas_load: None,
            #[cfg(not(target_arch = "wasm32"))]
            tas_editor_owner: None,
            paths: BackendPaths::with_source_path(rom_path, source_path),
            rom_hash,
            suspended: false,
            runtime_fault: None,
            execution_faulted: false,
            sram_recovery,
            host_persistence_enabled,
            rewind_audio_changed: Cell::new(false),
            audio_generation_enabled: None,
            trace_stream: crate::ui::TraceStreamId::allocate(),
            sound_trace: sound_trace::SoundTraceOwner::default(),
            debugger: debug::PcBreakpoints::default(),
            guest_call_undo: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn arm_start_failure_for_test(
        &mut self,
        stage: crate::netplay::session::md_start_failure::Stage,
    ) -> Result<(), MdOwnershipError> {
        self.require_normal()?;
        self.start_failure = Some(stage);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn take_start_failure_for_test(
        &mut self,
        stage: crate::netplay::session::md_start_failure::Stage,
    ) -> Option<crate::netplay::session::md_start_failure::Stage> {
        self.start_failure
            .filter(|target| target.triggers_at(stage))?;
        self.start_failure.take()
    }

    fn require_normal(&self) -> Result<(), MdOwnershipError> {
        match self.machine {
            MdMachine::Normal(_) => Ok(()),
            MdMachine::Leased(_) => Err(MdOwnershipError::RollbackOwned),
            MdMachine::Restoring { .. } => Err(MdOwnershipError::Restoring),
            MdMachine::Transfer(_) => Err(MdOwnershipError::Transfer),
            #[cfg(not(target_arch = "wasm32"))]
            MdMachine::TasLeased(_) => Err(MdOwnershipError::TasOwned),
            #[cfg(not(target_arch = "wasm32"))]
            MdMachine::TasCommitting(_) => Err(MdOwnershipError::TasCommitting),
            #[cfg(not(target_arch = "wasm32"))]
            MdMachine::TasRestoring(_) => Err(MdOwnershipError::Restoring),
        }
    }

    #[cfg(test)]
    fn normal_core_mut(&mut self) -> Result<&mut Emulator, MdOwnershipError> {
        self.require_normal()?;
        self.sound_trace
            .detach_uncaptured("Unobserved mutable access; earlier SOUND history is unavailable");
        self.machine.normal_mut()
    }

    pub(crate) fn reset_and_begin_audio_trace(&mut self, max_events: usize) -> anyhow::Result<()> {
        let core = self.machine.normal_mut()?;
        match self.sound_trace.attachment() {
            Some(mut sound) => {
                core.reset_and_begin_audio_trace_sound_traced(max_events, &mut sound)
            }
            None => core.reset_and_begin_audio_trace(max_events),
        }?;
        self.sound_trace
            .accepted_boundary("Audio diagnostic reset began a new SOUND observation");
        self.clear_instruction_trace();
        Ok(())
    }

    pub(crate) fn finish_audio_trace(
        &mut self,
    ) -> anyhow::Result<Option<zeff_emu_common::audio_trace::MdAudioTraceCapture>> {
        Ok(self.same_branch_core_mut()?.finish_audio_trace())
    }

    pub(crate) fn set_controllers(&mut self, kind: ControllerKind) {
        let Ok(core) = self.same_branch_core_mut() else {
            return;
        };
        for port in [ControllerPort::One, ControllerPort::Two] {
            core.input_mut().set_controller_kind(port, kind);
        }
    }

    pub(crate) fn try_warm_reset(
        &mut self,
    ) -> Result<zeff_md_core::WarmResetReport, MdWarmResetError> {
        let core = self
            .machine
            .normal_mut()
            .map_err(MdWarmResetError::Ownership)?;
        let result = match self.sound_trace.attachment() {
            Some(mut sound) => core.warm_reset_sound_traced(&mut sound),
            None => core.warm_reset(),
        };
        match result {
            Ok(report) => {
                self.guest_call_undo = None;
                self.suspended = false;
                self.runtime_fault = None;
                self.execution_faulted = false;
                self.debugger.clear_session();
                self.rotate_trace_stream();
                self.sound_trace
                    .accepted_boundary("Warm reset began a new SOUND observation");
                Ok(report)
            }
            Err(error @ zeff_md_core::WarmResetError::Failed(_)) => {
                self.suspended = true;
                self.execution_faulted = true;
                self.runtime_fault = Some(format!("Mega Drive warm reset failed: {error:?}"));
                Err(MdWarmResetError::Core(error))
            }
            Err(error) => Err(MdWarmResetError::Core(error)),
        }
    }

    pub(super) fn encode_rewind_checkpoint(&self) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(
            !self.suspended,
            "cannot rewind a suspended Mega Drive backend"
        );
        self.core().encode_debugger_checkpoint()
    }

    pub(super) fn restore_rewind_checkpoint(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.require_normal()?;
        anyhow::ensure!(
            bytes.starts_with(b"ZMDFRONT") || bytes.starts_with(b"ZMDDBG01"),
            "invalid Mega Drive rewind checkpoint"
        );
        let mut candidate = self.core().clone();
        candidate.decode_debugger_checkpoint(bytes)?;

        *self.same_branch_core_mut()? = candidate;
        self.replace_sound_unwitnessed(
            "Rewind replaced the machine; earlier SOUND history is unavailable",
        );
        self.guest_call_undo = None;
        self.suspended = false;
        self.runtime_fault = None;
        self.execution_faulted = false;
        self.debugger.clear_session();
        self.rotate_trace_stream();
        Ok(())
    }

    pub(super) fn apply_execution_controls(&mut self, continue_requested: bool, step: bool) {
        if self.rollback_owned() || !self.suspended || self.execution_faulted {
            return;
        }
        if continue_requested {
            self.debugger.resume_hit();
            self.sound_trace.resume_watches();
            self.suspended = false;
        } else if step {
            self.debugger.clear_stop();
            self.sound_trace.resume_watches();
            if let Err(fault) = self.step_debug_instruction() {
                self.record_execution_fault(fault);
            }
        }
    }

    fn record_execution_fault(&mut self, fault: MdExecutionError) {
        if let MdExecutionError::Core(fault) = fault {
            self.suspended = true;
            self.execution_faulted = true;
            self.runtime_fault = Some(format!("Mega Drive machine fault: {fault:?}"));
        }
    }

    pub(crate) fn nominal_frame_duration_ns(&self) -> u64 {
        self.core().nominal_frame_duration_ns()
    }

    pub(crate) fn display_framebuffer(&self) -> (&[u8], Option<(u32, u32)>) {
        let (width, height) = self.core().framebuffer_dimensions();
        (
            self.core().framebuffer(),
            Some((width as u32, height as u32)),
        )
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
        if self.rollback_owned() {
            return;
        }
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
        if self.rollback_owned() {
            return;
        }
        self.paths.set_audio_discovery_input(input);
    }
}

impl EmulatorCore for MdBackend {
    fn framebuffer(&self) -> &[u8] {
        self.core().framebuffer()
    }
    fn drain_audio_samples_into(&mut self, samples: &mut Vec<f32>) {
        if let Ok(core) = self.same_branch_core_mut() {
            core.drain_audio_samples_into(samples);
        }
    }
    fn set_sample_rate(&mut self, rate: u32) {
        if self.rollback_owned() {
            return;
        }
        self.rewind_audio_changed.set(true);
        if let Ok(core) = self.same_branch_core_mut() {
            core.set_sample_rate(rate);
        }
    }
    fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        if self.rollback_owned() {
            return;
        }
        if self.audio_generation_enabled != Some(enabled) {
            self.rewind_audio_changed.set(true);
            self.audio_generation_enabled = Some(enabled);
        }
        if let Ok(core) = self.same_branch_core_mut() {
            core.set_apu_sample_generation_enabled(enabled);
        }
    }
    fn set_apu_channel_mutes(&mut self, mutes: &[bool]) {
        if self.rollback_owned() {
            return;
        }
        let before = self.core().apu_channel_mutes();
        let psg = std::array::from_fn(|index| mutes.get(index).copied().unwrap_or(false));
        let fm = std::array::from_fn(|index| {
            mutes
                .get(index + zeff_md_core::constants::PSG_CHANNEL_COUNT)
                .copied()
                .unwrap_or(false)
        });
        let Ok(core) = self.same_branch_core_mut() else {
            return;
        };
        core.set_apu_channel_mutes(psg);
        core.set_fm_channel_mutes(fm);
        if self.core().apu_channel_mutes() != before {
            self.rewind_audio_changed.set(true);
        }
    }
    fn set_input(&mut self, buttons: u8, dpad: u8) {
        if let Ok(core) = self.same_branch_core_mut() {
            input::set(core, ControllerPort::One, buttons, dpad);
        }
    }
    fn set_input_p2(&mut self, buttons: u8, dpad: u8) {
        if let Ok(core) = self.same_branch_core_mut() {
            input::set(core, ControllerPort::Two, buttons, dpad);
        }
    }
    fn is_suspended(&self) -> bool {
        self.suspended
    }
    fn flush_battery_sram(&mut self) -> anyhow::Result<Option<String>> {
        self.flush_save_ram()
    }
    fn save_ram_kind(&self) -> zeff_emu_common::save_ram::SaveRamKind {
        self.core().save_ram_kind()
    }
    fn encode_state_bytes(&self) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(
            !self.execution_faulted,
            "cannot save a faulted Mega Drive backend"
        );
        self.core().encode_state()
    }
    fn load_state_from_bytes(
        &mut self,
        bytes: Vec<u8>,
    ) -> anyhow::Result<zeff_emu_common::StateRestoreOutcome> {
        self.same_branch_core_mut()?.decode_state(&bytes)?;
        self.replace_sound_unwitnessed(
            "State load replaced the machine; earlier SOUND history is unavailable",
        );
        self.guest_call_undo = None;
        self.suspended = false;
        self.runtime_fault = None;
        self.execution_faulted = false;
        self.debugger.clear_session();
        self.rotate_trace_stream();
        Ok(zeff_emu_common::StateRestoreOutcome::Exact)
    }
    fn state_restores_framebuffer(&self) -> bool {
        true
    }
    fn supports_save_states(&self) -> bool {
        true
    }
    fn supports_rewind(&self) -> bool {
        true
    }
    // Primary M68000 controls; memory writes retain their capability ranges.
    fn supports_debugger(&self) -> bool {
        true
    }
    fn supports_execution_controls(&self) -> bool {
        true
    }
    fn supports_pc_breakpoints(&self) -> bool {
        true
    }
    fn supports_memory_watchpoints(&self) -> bool {
        true
    }
    fn supports_opcode_history(&self) -> bool {
        true
    }
    fn supports_symbol_loading(&self) -> bool {
        true
    }
    fn supports_replay(&self) -> bool {
        true
    }
    fn supports_guest_calls(&self) -> bool {
        true
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
    fn supports_cheats(&self) -> bool {
        true
    }
    fn apply_ram_cheats(&mut self, cheats: &[crate::cheats::CheatPatch]) {
        if let Ok(core) = self.same_branch_core_mut() {
            core.apply_ram_cheats(cheats);
        }
    }
    fn system_ram_len(&self) -> usize {
        self.core().work_ram().len()
    }
    fn video_ram_len(&self) -> usize {
        self.core().vdp().memory().vram().len()
    }
    fn palette_ram_len(&self) -> usize {
        self.core().vdp().memory().cram_word_count() * size_of::<u16>()
    }
    fn memory_regions(&self) -> Vec<zeff_emu_common::memory::MemoryRegionDescriptor> {
        memory::regions(self.core())
    }
    fn copy_memory_region(
        &mut self,
        id_or_alias: &str,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<zeff_emu_common::memory::MemoryRegionDescriptor> {
        memory::copy(self.core(), id_or_alias, out)
    }
    fn cpu_address_bits(&self) -> u8 {
        24
    }
    fn debug_suspend(&mut self) {
        if self.rollback_owned() {
            return;
        }
        self.suspended = true;
    }
    fn take_runtime_fault(&mut self) -> Option<String> {
        if self.rollback_owned() {
            return None;
        }
        self.runtime_fault.take()
    }
}

impl MachineTiming for MdBackend {
    fn timing_snapshot(&self) -> TimingSnapshot {
        self.core().timing_snapshot()
    }
}

impl Reset for MdBackend {
    fn reset(&mut self) {
        if self.rollback_owned() {
            return;
        }
        if let Ok(core) = self.machine.normal_mut() {
            match self.sound_trace.attachment() {
                Some(mut sound) => {
                    let _ = core.reset_sound_traced(&mut sound);
                }
                None => core.reset(),
            }
        }
        self.sound_trace
            .accepted_boundary("Reset began a new SOUND observation");
        self.guest_call_undo = None;
        self.suspended = false;
        self.runtime_fault = None;
        self.execution_faulted = false;
        self.debugger.clear_session();
        self.rotate_trace_stream();
    }
}

impl FrameLifecycle for MdBackend {
    fn step_frame(&mut self) {
        if self.rollback_owned() {
            return;
        }
        if self.suspended {
            return;
        }
        if let Err(fault) = self.step_debug_field() {
            self.record_execution_fault(fault);
        }
    }
    fn frame_count(&self) -> u64 {
        self.core().pacing_field_count()
    }
}
