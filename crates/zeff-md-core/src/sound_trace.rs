use zeff_emu_common::debug::{InstructionTraceStore, TraceObservationEnd};
use zeff_z80::{BusState, Cpu, FetchedInstruction};

mod attempt;
mod bus;
mod watch;
pub use watch::{
    SoundAccess, SoundAccessKind, SoundWatchHit, SoundWatchRule, SoundWatchSpace, SoundWatchpoint,
};
mod continuation;
pub(crate) mod physical;
pub(crate) use attempt::{Attempt, AttemptCapture, NoAttempt};
pub(crate) use bus::CapturedBus;
pub use continuation::{SoundTraceAttachment, SoundTraceContinuation, SoundTraceFork};

pub(crate) trait SoundCapture {
    type Physical<'a>: physical::PhysicalCapture
    where
        Self: 'a;
    fn physical(&mut self) -> Self::Physical<'_>;
    type Attempt: AttemptCapture;
    type Checkpoint;
    fn whole(
        cpu: &Cpu,
        memory: &crate::hardware::sound::SoundMemory,
        cartridge: &crate::cartridge::Cartridge,
        now: u64,
    ) -> Result<
        (
            crate::hardware::sound::schedule::PendingSound,
            Self::Attempt,
        ),
        crate::SoundFault,
    >;
    fn prepared(&mut self, attempt: Self::Attempt);
    fn discard_pending(&mut self);
    fn publish(&mut self, cpu: &Cpu);
    fn fault(&mut self);
    fn reset(&mut self);
    fn backup_pending(&mut self);
    fn finish_rmw(&mut self, restored: bool);
    fn begin_epoch(&mut self);
    fn stage_epoch(&mut self, index: usize, attempt: Self::Attempt);
    fn publish_epoch(&mut self, index: usize, cpu: &Cpu);
    fn materialize_epoch(&mut self, prepared: Option<usize>);
    fn checkpoint(&self) -> Self::Checkpoint;
    fn rollback(&mut self, checkpoint: Self::Checkpoint);
}

#[derive(Clone, Copy, Default)]
pub(crate) struct NoSoundCapture;

impl SoundCapture for NoSoundCapture {
    type Physical<'a>
        = ()
    where
        Self: 'a;
    fn physical(&mut self) {}
    type Attempt = NoAttempt;
    type Checkpoint = ();
    fn whole(
        cpu: &Cpu,
        memory: &crate::hardware::sound::SoundMemory,
        cartridge: &crate::cartridge::Cartridge,
        now: u64,
    ) -> Result<(crate::hardware::sound::schedule::PendingSound, NoAttempt), crate::SoundFault>
    {
        Ok((
            crate::hardware::sound::schedule::PendingSound::prepare(cpu, memory, cartridge, now)?,
            NoAttempt,
        ))
    }
    fn prepared(&mut self, _: NoAttempt) {}
    fn discard_pending(&mut self) {}
    fn publish(&mut self, _: &Cpu) {}
    fn fault(&mut self) {}
    fn reset(&mut self) {}
    fn backup_pending(&mut self) {}
    fn finish_rmw(&mut self, _: bool) {}
    fn begin_epoch(&mut self) {}
    fn stage_epoch(&mut self, _: usize, _: NoAttempt) {}
    fn publish_epoch(&mut self, _: usize, _: &Cpu) {}
    fn materialize_epoch(&mut self, _: Option<usize>) {}
    fn checkpoint(&self) {}
    fn rollback(&mut self, _: ()) {}
}

impl<C: SoundCapture> SoundCapture for &mut C {
    type Physical<'a>
        = C::Physical<'a>
    where
        Self: 'a;
    fn physical(&mut self) -> Self::Physical<'_> {
        (**self).physical()
    }
    type Attempt = C::Attempt;
    type Checkpoint = C::Checkpoint;
    fn whole(
        cpu: &Cpu,
        memory: &crate::hardware::sound::SoundMemory,
        cartridge: &crate::cartridge::Cartridge,
        now: u64,
    ) -> Result<(crate::hardware::sound::schedule::PendingSound, C::Attempt), crate::SoundFault>
    {
        C::whole(cpu, memory, cartridge, now)
    }
    fn prepared(&mut self, attempt: Self::Attempt) {
        (**self).prepared(attempt);
    }
    fn discard_pending(&mut self) {
        (**self).discard_pending();
    }
    fn publish(&mut self, cpu: &Cpu) {
        (**self).publish(cpu);
    }
    fn fault(&mut self) {
        (**self).fault();
    }
    fn reset(&mut self) {
        (**self).reset();
    }
    fn backup_pending(&mut self) {
        (**self).backup_pending();
    }
    fn finish_rmw(&mut self, restored: bool) {
        (**self).finish_rmw(restored);
    }
    fn begin_epoch(&mut self) {
        (**self).begin_epoch();
    }
    fn stage_epoch(&mut self, index: usize, attempt: Self::Attempt) {
        (**self).stage_epoch(index, attempt);
    }
    fn publish_epoch(&mut self, index: usize, cpu: &Cpu) {
        (**self).publish_epoch(index, cpu);
    }
    fn materialize_epoch(&mut self, prepared: Option<usize>) {
        (**self).materialize_epoch(prepared);
    }
    fn checkpoint(&self) -> Self::Checkpoint {
        (**self).checkpoint()
    }
    fn rollback(&mut self, checkpoint: Self::Checkpoint) {
        (**self).rollback(checkpoint);
    }
}
