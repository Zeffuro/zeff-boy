mod watches;

use super::{MdBackend, MdExecutionError, MdOwnershipError};
use crate::ui::TraceStreamId;
use zeff_emu_common::debug::InstructionTraceStore;
use zeff_md_core::sound_trace::{SoundTraceAttachment, SoundTraceContinuation, SoundTraceFork};
use zeff_md_core::{Emulator, MachineStep};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Identity {
    owner: Option<TraceStreamId>,
    branch: Option<TraceStreamId>,
    policy: Option<TraceStreamId>,
    stream: Option<TraceStreamId>,
}

#[derive(Default)]
pub(super) struct SoundTraceOwner {
    identity: Identity,
    live: Option<Box<SoundTraceContinuation>>,
    output: InstructionTraceStore,
    gap: Option<&'static str>,
    watch_notice: Option<&'static str>,
    stop: Option<crate::debug::sound_watchpoints::DelayedSoundWatchHit>,
    watch_request: Option<TraceStreamId>,
    watch_view: (crate::debug::DebugTarget, u64),
}

pub(super) struct SoundTransaction {
    source: Identity,
    child: TraceStreamId,
    fork: SoundTraceFork,
    stop: Option<crate::debug::sound_watchpoints::DelayedSoundWatchHit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FrozenSound {
    identity: Identity,
    inactive: (usize, usize, usize, u64),
}

impl SoundTraceOwner {
    pub(super) fn attachment(&mut self) -> Option<SoundTraceAttachment<'_>> {
        self.live.as_mut().map(|live| live.attach(&mut self.output))
    }

    pub(super) fn store(&self) -> &InstructionTraceStore {
        &self.output
    }

    pub(super) fn stream(&self) -> Option<TraceStreamId> {
        self.identity.stream
    }

    pub(super) fn gap(&self) -> Option<&'static str> {
        self.gap.or_else(|| {
            self.live
                .as_ref()
                .filter(|live| live.gaps() != 0)
                .map(|_| "Some instruction history was not observed")
        })
    }

    pub(super) fn idle(&self) -> bool {
        self.live.is_none() && !self.output.is_enabled() && self.output.is_empty()
    }

    pub(super) fn frozen(&self) -> anyhow::Result<FrozenSound> {
        anyhow::ensure!(self.idle(), "MD SOUND observation owns debugger history");
        Ok(FrozenSound {
            identity: self.identity,
            inactive: self
                .output
                .inactive_observation()
                .ok_or_else(|| anyhow::anyhow!("MD SOUND observation is active"))?,
        })
    }

    fn allocate(&mut self, policy: bool) -> bool {
        self.allocate_with(policy, TraceStreamId::allocate)
    }

    fn allocate_with(&mut self, policy: bool, mut next: impl FnMut() -> TraceStreamId) -> bool {
        if let Some(terminal) = self.exhausted_identity() {
            self.exhaust(terminal);
            return false;
        }
        if self.identity.owner.is_none() {
            self.identity.owner = Some(next());
        }
        self.identity.branch = Some(next());
        self.identity.stream = Some(next());
        if policy || self.identity.policy.is_none() {
            self.identity.policy = Some(next());
        }
        if let Some(terminal) = self.exhausted_identity() {
            self.exhaust(terminal);
            return false;
        }
        true
    }

    fn exhausted_identity(&self) -> Option<TraceStreamId> {
        [
            self.identity.owner,
            self.identity.branch,
            self.identity.policy,
            self.identity.stream,
        ]
        .into_iter()
        .flatten()
        .find(|identity| !identity.is_live())
    }

    fn exhaust(&mut self, terminal: TraceStreamId) {
        self.identity.branch = Some(terminal);
        self.identity.stream = Some(terminal);
        self.stop = None;
        if let Some(mut live) = self.live.take() {
            live.disable(&mut self.output);
        }
        self.output.set_enabled(false);
        self.output.clear();
        self.gap = Some("SOUND observation identifiers are exhausted");
    }

    #[cfg(test)]
    pub(super) fn exhaust_for_test(&mut self) {
        self.allocate_with(true, TraceStreamId::exhausted_for_test);
    }

    fn rotate_trace_stream(&mut self) -> bool {
        if self.identity.owner.is_none() {
            return self.allocate(false);
        }
        if self.identity.stream.is_some_and(|id| !id.is_live()) {
            return false;
        }
        let stream = TraceStreamId::allocate();
        self.identity.stream = Some(stream);
        if !stream.is_live() {
            if let Some(live) = &mut self.live {
                live.disable(&mut self.output);
            }
            self.output.clear();
            self.retain_armed();
            self.gap = Some("SOUND trace identifiers are exhausted");
        }
        stream.is_live()
    }

    pub(super) fn set_enabled(&mut self, enabled: bool) {
        if enabled == self.output.is_enabled() {
            return;
        }
        if enabled {
            if self.rotate_trace_stream() {
                self.output.clear();
                self.output.set_enabled(true);
                if let Some(live) = &mut self.live {
                    live.trace_discontinuity(&mut self.output);
                } else {
                    self.live = Some(Box::default());
                }
                self.gap = Some("Observation started here; earlier SOUND history is unavailable");
            }
        } else {
            if let Some(live) = &mut self.live {
                live.disable(&mut self.output);
            }
            self.retain_armed();
            if self.rotate_trace_stream() {
                self.gap = Some("SOUND observation is disabled");
            }
        }
    }

    pub(super) fn set_capacity(&mut self, capacity: usize) {
        self.output.set_capacity(capacity);
    }

    pub(super) fn replace_unwitnessed(&mut self, reason: &'static str) {
        if self.identity.owner.is_none() {
            return;
        }
        if let Some(live) = &mut self.live {
            live.discontinuity(&mut self.output);
        }
        self.stop = None;
        self.watch_notice = Some(reason);
        self.output.clear();
        self.retain_armed();
        if self.allocate(false) {
            self.gap = Some(reason);
        }
    }

    pub(super) fn accepted_boundary(&mut self, reason: &'static str) {
        if self.identity.owner.is_none() {
            return;
        }
        self.resume_watches();
        self.watch_notice = Some(reason);
        if self.allocate(false) {
            self.gap = Some(reason);
        }
    }

    #[cfg(test)]
    pub(super) fn detach_uncaptured(&mut self, reason: &'static str) {
        if self.identity.owner.is_none() {
            return;
        }
        if self.watches_armed() {
            if let Some(live) = &mut self.live {
                live.discontinuity(&mut self.output);
            }
            self.stop = None;
            if self.allocate(false) {
                self.gap = Some(reason);
            }
            return;
        }
        let enabled = self.output.is_enabled();
        if let Some(mut live) = self.live.take() {
            live.discontinuity(&mut self.output);
        }
        if self.allocate(false) {
            if enabled {
                self.live = Some(Box::default());
            }
            self.gap = Some(reason);
        }
    }

    pub(super) fn clear(&mut self) {
        if self.identity.owner.is_none() {
            return;
        }
        if let Some(live) = &mut self.live {
            live.trace_discontinuity(&mut self.output);
        }
        self.output.clear();
        self.retain_armed();
        if self.rotate_trace_stream() {
            self.gap = Some("Observation cleared; earlier SOUND history is unavailable");
        }
    }

    pub(super) fn accept_forward_transaction(&mut self) {
        if self.identity.owner.is_none() {
            return;
        }
        let child = TraceStreamId::allocate();
        self.identity.branch = Some(child);
        if !child.is_live() {
            self.allocate(false);
        }
    }

    pub(super) fn begin_transaction(&mut self, machine: &mut Emulator) -> Option<SoundTransaction> {
        let live = self.live.as_mut()?;
        Some(SoundTransaction {
            source: self.identity,
            child: TraceStreamId::allocate(),
            fork: live.fork(machine, &mut self.output),
            stop: self.stop,
        })
    }

    pub(super) fn accept_transaction(&mut self, transaction: SoundTransaction) {
        debug_assert_eq!(self.identity, transaction.source);
        self.identity.branch = Some(transaction.child);
        if !transaction.child.is_live() {
            self.allocate(false);
        }
    }

    pub(super) fn abort_transaction(
        &mut self,
        machine: &mut Emulator,
        transaction: SoundTransaction,
    ) {
        debug_assert_eq!(self.identity, transaction.source);
        let live = self
            .live
            .as_mut()
            .expect("exclusive SOUND transaction owner");
        transaction.fork.restore(machine, live, &mut self.output);
        self.stop = transaction.stop;
    }
}

impl MdBackend {
    pub(crate) fn sound_instruction_trace(&self) -> &InstructionTraceStore {
        self.sound_trace.store()
    }

    pub(crate) fn sound_instruction_trace_stream(&self) -> Option<TraceStreamId> {
        self.sound_trace.stream()
    }

    pub(crate) fn sound_trace_gap(&self) -> Option<&'static str> {
        self.sound_trace.gap()
    }

    pub(crate) fn sound_trace_rejection(&self) -> Option<&'static str> {
        #[cfg(not(target_arch = "wasm32"))]
        if self
            .tas_editor_owner
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .is_some()
        {
            return Some(
                "SOUND observation is unavailable while the offline editor owns execution",
            );
        }
        if self.rollback_owned() {
            Some("SOUND observation requires normal backend ownership")
        } else {
            self.core().passive_sound_trace_rejection()
        }
    }

    pub(super) fn same_branch_core_mut(&mut self) -> Result<&mut Emulator, MdOwnershipError> {
        self.machine.normal_mut()
    }

    pub(super) fn step_with_sound(&mut self) -> Result<MachineStep, MdExecutionError> {
        let core = self.machine.normal_mut()?;
        match self.sound_trace.attachment() {
            Some(mut sound) => core.step_instruction_sound_traced(&mut sound),
            None => core.step_instruction(),
        }
        .map_err(Into::into)
    }

    pub(super) fn field_with_sound(&mut self) -> Result<(), MdExecutionError> {
        let core = self.machine.normal_mut()?;
        match self.sound_trace.attachment() {
            Some(mut sound) => core.step_field_sound_traced(&mut sound),
            None => core.step_field(),
        }
        .map_err(Into::into)
    }

    pub(super) fn replace_sound_unwitnessed(&mut self, reason: &'static str) {
        self.sound_trace.replace_unwitnessed(reason);
    }
}
