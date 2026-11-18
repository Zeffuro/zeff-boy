use std::mem::size_of;
use std::sync::{Arc, Weak};

use anyhow::{Result, ensure};
use zeff_emu_common::time::{FrameLifecycle, MachineTiming};
use zeff_md_core::Emulator;
use zeff_md_core::emulator::rollback::{MdRetainedAllocations, MdRollbackConfig, MdRollbackHashes};

use super::{MdBackend, MdTasLoad};

pub(crate) struct MdTasEditorOwner {
    token: Arc<()>,
    load: MdTasLoad,
    debug: [u64; 9],
    sound: std::cell::Cell<crate::emu_backend::md::sound_trace::FrozenSound>,
}

pub(crate) struct MdTasEditorCheckpoint {
    owner: Weak<()>,
    runtime: Box<Emulator>,
    config: MdRollbackConfig,
    hashes: MdRollbackHashes,
    field: u64,
    master: u64,
    queued: usize,
    generation: Option<bool>,
    rewind_changed: bool,
}

impl MdTasEditorOwner {
    pub(crate) fn retained_metadata_bytes(&self) -> usize {
        size_of::<Self>() + self.load.retained_source_bytes()
    }
}

impl MdTasEditorCheckpoint {
    pub(crate) fn belongs_to(&self, owner: &MdTasEditorOwner) -> bool {
        self.owner.ptr_eq(&Arc::downgrade(&owner.token))
    }

    pub(crate) fn frame(&self) -> u64 {
        self.field
    }

    pub(crate) fn validate(&self, owner: &MdTasEditorOwner) -> Result<()> {
        ensure!(self.belongs_to(owner), "foreign MD editor checkpoint owner");
        ensure!(
            &self.config == owner.load.config()
                && MdRollbackConfig::capture(&self.runtime)? == self.config
                && self.config.boundary_hashes(&self.runtime)? == self.hashes
                && self.runtime.pacing_field_count() == self.field
                && self.runtime.timing_snapshot().now().get() == self.master
                && self.runtime.pending_audio_frames() == self.queued,
            "MD editor checkpoint runtime proof changed"
        );
        Ok(())
    }

    pub(crate) fn visit_retained<'a>(&'a self, into: &mut MdRetainedAllocations<'a>) -> Result<()> {
        into.visit_emulator(&self.runtime)
    }

    #[cfg(test)]
    pub(crate) fn corrupt_proof_for_test(&mut self) {
        self.queued = self.queued.wrapping_add(1);
    }
}

impl MdBackend {
    #[cfg(test)]
    pub(crate) fn tas_editor_host_witness_for_test(&self) -> Result<[u8; 32]> {
        self.rollback_host_observation()
    }

    fn validate_tas_editor_owner(&self, owner: &MdTasEditorOwner) -> Result<()> {
        ensure!(
            self.tas_editor_owner
                .as_ref()
                .is_some_and(|token| token.ptr_eq(&Arc::downgrade(&owner.token))),
            "MD editor owner belongs to another backend or generation"
        );
        Ok(())
    }

    pub(crate) fn begin_tas_editor(&mut self) -> Result<MdTasEditorOwner> {
        ensure!(
            self.tas_editor_owner
                .as_ref()
                .and_then(Weak::upgrade)
                .is_none(),
            "MD backend already has an offline editor owner"
        );
        let load = self.tas_load()?;
        load.validate_fresh(self)?;
        load.validate_source(self)?;
        ensure!(
            !self.host_persistence_enabled,
            "MD editor requires an offline nonpublishing backend"
        );
        let owner = MdTasEditorOwner {
            token: Arc::new(()),
            load: load.clone(),
            debug: self.debugger.frozen_observation()?,
            sound: std::cell::Cell::new(self.sound_trace.frozen()?),
        };
        self.tas_editor_owner = Some(Arc::downgrade(&owner.token));
        Ok(owner)
    }

    pub(crate) fn validate_tas_editor(&self, owner: &MdTasEditorOwner) -> Result<()> {
        self.validate_tas_editor_owner(owner)?;
        let load = self.tas_load()?;
        ensure!(
            load == &owner.load
                && !self.host_persistence_enabled
                && self.debugger.frozen_observation()? == owner.debug
                && self.sound_trace.frozen()? == owner.sound.get(),
            "MD offline editor provenance or host ownership changed"
        );
        load.validate_fixed(self)?;
        load.validate_source(self)
    }

    pub(crate) fn capture_tas_editor(
        &self,
        owner: &MdTasEditorOwner,
    ) -> Result<MdTasEditorCheckpoint> {
        self.validate_tas_editor_owner(owner)?;
        let load = self.tas_load()?;
        ensure!(
            load == &owner.load
                && !self.host_persistence_enabled
                && self.debugger.frozen_observation()? == owner.debug
                && self.sound_trace.frozen()? == owner.sound.get(),
            "MD editor checkpoint host ownership changed"
        );
        load.validate_fixed(self)?;
        let config = MdRollbackConfig::capture(self.core())?;
        Ok(MdTasEditorCheckpoint {
            owner: Arc::downgrade(&owner.token),
            runtime: Box::new(self.core().clone()),
            hashes: config.boundary_hashes(self.core())?,
            config,
            field: self.frame_count(),
            master: self.core().timing_snapshot().now().get(),
            queued: self.core().pending_audio_frames(),
            generation: self.audio_generation_enabled,
            rewind_changed: self.rewind_audio_changed.get(),
        })
    }

    pub(crate) fn restore_tas_editor(
        &mut self,
        owner: &MdTasEditorOwner,
        checkpoint: &MdTasEditorCheckpoint,
    ) -> Result<()> {
        self.require_normal()?;
        self.validate_tas_editor_owner(owner)?;
        ensure!(
            self.tas_load.as_ref() == Some(&owner.load)
                && !self.host_persistence_enabled
                && self.guest_call_undo.is_none()
                && self.debugger.frozen_observation()? == owner.debug
                && self.sound_trace.frozen()? == owner.sound.get(),
            "MD editor restore host ownership changed"
        );
        checkpoint.validate(owner)?;
        ensure!(
            checkpoint.config == MdRollbackConfig::capture(&checkpoint.runtime)?,
            "MD editor restore fixed configuration changed"
        );
        let candidate = checkpoint.runtime.clone();
        *self.same_branch_core_mut()? = *candidate;
        self.audio_generation_enabled = checkpoint.generation;
        self.rewind_audio_changed.set(checkpoint.rewind_changed);
        self.suspended = false;
        self.runtime_fault = None;
        self.execution_faulted = false;
        self.rotate_trace_stream();
        self.replace_sound_unwitnessed(
            "Execution owner installed a machine; earlier SOUND history is unavailable",
        );
        owner.sound.set(
            self.sound_trace
                .frozen()
                .expect("inactive editor SOUND owner"),
        );
        Ok(())
    }
}
