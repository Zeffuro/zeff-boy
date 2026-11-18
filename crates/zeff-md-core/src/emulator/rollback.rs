use super::field::FieldExecution;
use super::{Emulator, MachineFault};
use anyhow::{Context, Result, ensure};
use std::num::NonZeroUsize;
use std::sync::{Arc, Weak};

mod config;
mod hash;
mod input;
mod tas;
pub use tas::{MdCandidatePromotion, MdPreparedTasAcquisition};
pub(crate) mod memory;
pub use config::MdRollbackConfig;
pub use hash::MdRollbackHashes;
pub use input::MdRollbackInputs;
pub use memory::{MdAllocationKind, MdRetainedAllocations, MdRetainedPayload};

#[must_use]
pub struct MdRollbackSession {
    owner: Arc<()>,
    baseline: Box<Emulator>,
    current: Box<Emulator>,
    config: MdRollbackConfig,
    retired: bool,
    pending: Option<PendingField>,
}

struct PendingField {
    candidate: Box<Emulator>,
    execution: FieldExecution,
    next: u64,
}

pub struct MdRollbackSnapshot {
    owner: Weak<()>,
    runtime: Box<Emulator>,
    config: MdRollbackConfig,
}

pub struct MdRollbackFrame {
    pub field: u64,
    pub master_tick: u64,
    pub nominal_duration_ns: u64,
    pub audio: Vec<f32>,
}

impl MdRollbackSession {
    // Ownership excludes ordinary mutators; the original baseline is never executed.
    pub fn acquire(
        core: Box<Emulator>,
        expected: &MdRollbackConfig,
    ) -> std::result::Result<Self, (Box<Emulator>, anyhow::Error)> {
        let config = match MdRollbackConfig::capture(&core) {
            Ok(config) if config == *expected => config,
            Ok(_) => return Err((core, anyhow::anyhow!("MD rollback settings mismatch"))),
            Err(error) => return Err((core, error)),
        };
        Ok(Self {
            owner: Arc::new(()),
            current: core.clone(),
            baseline: core,
            config,
            retired: false,
            pending: None,
        })
    }

    pub fn core(&self) -> &Emulator {
        &self.current
    }

    pub fn config(&self) -> &MdRollbackConfig {
        &self.config
    }

    pub fn retire(&mut self) {
        self.abort_frame();
        self.retired = true;
    }

    pub fn release(mut self) -> Box<Emulator> {
        self.abort_frame();
        self.baseline
    }

    fn validate(&self, expected: &MdRollbackConfig) -> Result<()> {
        ensure!(!self.retired, "retired MD rollback session");
        ensure!(self.pending.is_none(), "MD rollback field is pending");
        ensure!(self.config == *expected, "MD rollback settings mismatch");
        ensure!(
            MdRollbackConfig::capture(&self.current)? == self.config,
            "MD rollback runtime settings changed"
        );
        Ok(())
    }

    pub fn capture(&self) -> Result<MdRollbackSnapshot> {
        self.validate(&self.config)?;
        Ok(MdRollbackSnapshot {
            owner: Arc::downgrade(&self.owner),
            runtime: self.current.clone(),
            config: self.config.clone(),
        })
    }

    pub fn restore(
        &mut self,
        snapshot: &MdRollbackSnapshot,
        expected: &MdRollbackConfig,
    ) -> Result<()> {
        self.validate(expected)?;
        ensure!(
            snapshot.owner.ptr_eq(&Arc::downgrade(&self.owner)),
            "stale or foreign MD rollback snapshot"
        );
        ensure!(
            snapshot.config == self.config,
            "MD rollback snapshot settings mismatch"
        );
        let candidate = snapshot.runtime.clone();
        ensure!(
            MdRollbackConfig::capture(&candidate)? == self.config,
            "MD rollback snapshot runtime settings mismatch"
        );
        self.current = candidate;
        Ok(())
    }

    pub fn pending_audio_frames(&self) -> usize {
        self.current.bus.audio.pending_frames()
    }

    // Pending baseline PCM is returned explicitly, never folded into a speculative field.
    pub fn take_pending_audio(&mut self) -> Result<Vec<f32>> {
        self.validate(&self.config)?;
        Ok(self.current.drain_audio_samples())
    }

    pub fn has_pending_frame(&self) -> bool {
        self.pending.is_some()
    }

    pub fn abort_frame(&mut self) {
        self.pending = None;
    }

    pub fn begin_frame(
        &mut self,
        ports: MdRollbackInputs,
        expected: &MdRollbackConfig,
    ) -> Result<()> {
        self.validate(expected)?;
        ports.validate(&self.config)?;
        ensure!(
            self.pending_audio_frames() == 0,
            "MD rollback has pending baseline audio"
        );
        let next = self
            .current
            .pacing_field_count()
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("MD rollback field overflow"))?;
        let mut candidate = self.current.clone();
        ports.apply(&mut candidate);
        let execution = FieldExecution::begin(&mut candidate).map_err(|fault: MachineFault| {
            anyhow::anyhow!("MD rollback field failed: {fault:?}")
        })?;
        self.pending = Some(PendingField {
            candidate,
            execution,
            next,
        });
        Ok(())
    }

    pub fn resume_frame(
        &mut self,
        expected: &MdRollbackConfig,
        budget: NonZeroUsize,
    ) -> Result<Option<MdRollbackFrame>> {
        let mut pending = self
            .pending
            .take()
            .context("no pending MD rollback field")?;
        ensure!(!self.retired, "retired MD rollback session");
        ensure!(self.config == *expected, "MD rollback settings mismatch");
        if let Some(execution) = pending
            .execution
            .resume(&mut pending.candidate, budget)
            .map_err(|fault: MachineFault| anyhow::anyhow!("MD rollback field failed: {fault:?}"))?
        {
            pending.execution = execution;
            self.pending = Some(pending);
            return Ok(None);
        }
        ensure!(
            pending.candidate.pacing_field_count() == pending.next,
            "MD rollback field did not advance once"
        );
        ensure!(
            MdRollbackConfig::capture(&pending.candidate)? == self.config,
            "MD rollback field changed fixed settings"
        );
        let result = MdRollbackFrame {
            field: pending.next,
            master_tick: pending.candidate.bus.now.get(),
            nominal_duration_ns: pending.candidate.nominal_frame_duration_ns(),
            audio: pending.candidate.drain_audio_samples(),
        };
        self.current = pending.candidate;
        Ok(Some(result))
    }

    pub fn advance_frame(
        &mut self,
        ports: MdRollbackInputs,
        expected: &MdRollbackConfig,
    ) -> Result<MdRollbackFrame> {
        self.begin_frame(ports, expected)?;
        loop {
            if let Some(frame) = self.resume_frame(expected, NonZeroUsize::MAX)? {
                return Ok(frame);
            }
        }
    }
}

impl MdRollbackSnapshot {
    pub fn field(&self) -> u64 {
        self.runtime.pacing_field_count()
    }

    pub fn master_tick(&self) -> u64 {
        self.runtime.bus.now.get()
    }
}

#[cfg(all(
    test,
    any(
        feature = "sound-shadow-observer",
        feature = "first-failure-diagnostic"
    )
))]
mod instrumented_tests;
#[cfg(all(
    test,
    not(any(
        feature = "sound-shadow-observer",
        feature = "first-failure-diagnostic"
    ))
))]
mod tests;
