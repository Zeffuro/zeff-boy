use super::*;
use crate::debug::sound_watchpoints::{
    DelayedSoundWatchHit, SoundWatchBinding, SoundWatchRequest, SoundWatchView,
};
use zeff_emu_common::time::MachineTiming;

impl SoundTraceOwner {
    pub(super) fn retain_armed(&mut self) {
        if !self.output.is_enabled() && !self.watches_armed() {
            self.live = None;
        }
    }
    pub(in crate::emu_backend::md) fn watches_armed(&self) -> bool {
        self.live.as_ref().is_some_and(|live| live.watches_armed())
    }
    pub(in crate::emu_backend::md) fn resume_watches(&mut self) {
        self.stop = None;
        if let Some(live) = &mut self.live {
            live.clear_watch_hit();
        }
    }
    fn observe_view(&mut self, view: (crate::debug::DebugTarget, u64)) -> bool {
        if view.1 == u64::MAX || self.watch_view.1 == u64::MAX {
            self.watch_view = (view.0, u64::MAX);
            return false;
        }
        if view.1 < self.watch_view.1 || view.1 == self.watch_view.1 && view.0 != self.watch_view.0
        {
            return false;
        }
        self.watch_view = view;
        true
    }
    fn binding(&self, revision: u64) -> Option<SoundWatchBinding> {
        (revision != u64::MAX && self.watch_view == (crate::debug::DebugTarget::Sound, revision))
            .then_some(SoundWatchBinding {
                owner: self.identity.owner?,
                branch: self.identity.branch?,
                policy: self.identity.policy?,
                revision,
            })
            .filter(|binding| {
                binding.owner.is_live() && binding.branch.is_live() && binding.policy.is_live()
            })
    }
    fn edit(&mut self, machine: &Emulator, request: SoundWatchRequest) {
        if self.binding(request.binding.revision) != Some(request.binding) {
            return;
        }
        if !request.request.is_live() {
            self.watch_notice = Some("SOUND watch request identifiers are exhausted");
            return;
        }
        if self
            .watch_request
            .is_some_and(|last| request.request <= last)
        {
            return;
        }
        self.watch_request = Some(request.request);
        let live = self.live.get_or_insert_with(Box::default);
        match live.edit_watchpoint(machine, request.rule, request.add) {
            Ok(changed) => {
                self.watch_notice = None;
                if changed {
                    self.stop = None;
                    let branch = TraceStreamId::allocate();
                    let policy = TraceStreamId::allocate();
                    self.identity.branch = Some(branch);
                    self.identity.policy = Some(policy);
                    if !branch.is_live() || !policy.is_live() {
                        self.allocate(false);
                    }
                }
                self.retain_armed();
            }
            Err(reason) => {
                self.watch_notice = Some(reason);
                self.retain_armed();
            }
        }
    }
}
impl MdBackend {
    pub(crate) fn sound_watch_view(&mut self, revision: u64) -> SoundWatchView {
        let rejection = self.sound_trace_rejection();
        let current = self.observe_sound_watch_view((crate::debug::DebugTarget::Sound, revision));
        let rejection = rejection
            .or((!current).then_some("SOUND debugger snapshot revision is stale or exhausted"));
        if rejection.is_none() && self.sound_trace.identity.owner.is_none() {
            self.sound_trace.allocate(false);
        }
        let owner = &self.sound_trace;
        SoundWatchView {
            binding: owner.binding(revision),
            watches: owner
                .live
                .as_ref()
                .map_or_else(Vec::new, |live| live.watchpoints().to_vec()),
            hit: owner.stop,
            notice: owner.watch_notice,
            gaps: owner.live.as_ref().map_or(0, |live| live.watch_gaps()),
            blocked: owner.live.as_ref().is_some_and(|live| live.watch_blocked()),
            rejection: rejection.or_else(|| {
                [
                    owner.identity.owner,
                    owner.identity.branch,
                    owner.identity.policy,
                ]
                .into_iter()
                .flatten()
                .any(|id| !id.is_live())
                .then_some("SOUND watch identifiers are exhausted")
            }),
        }
    }
    pub(in crate::emu_backend) fn observe_sound_watch_view(
        &mut self,
        view: (crate::debug::DebugTarget, u64),
    ) -> bool {
        self.sound_trace_rejection().is_none() && self.sound_trace.observe_view(view)
    }
    pub(in crate::emu_backend) fn apply_sound_watch_actions(&mut self, request: SoundWatchRequest) {
        if self.sound_trace_rejection().is_some() {
            return;
        }
        let Ok(core) = self.machine.normal_mut() else {
            return;
        };
        self.sound_trace.edit(core, request);
    }
    pub(in crate::emu_backend::md) fn finish_sound_watch_boundary(&mut self, faulted: bool) {
        let owner = &mut self.sound_trace;
        let Some(live) = &owner.live else {
            return;
        };
        if let Some(observed) = live.watch_hit() {
            owner.stop.get_or_insert(DelayedSoundWatchHit {
                observed,
                main_pc: self.machine.core().registers().pc,
                master_tick: self.machine.core().timing_snapshot().now().get(),
                faulted,
            });
            self.suspended = true;
        }
        if live.watch_blocked() {
            self.suspended = true;
        }
    }
}
