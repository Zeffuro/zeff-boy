use super::*;
use crate::emulator::rollback::memory::{MdAllocationKind as K, MdRetainedAllocations};

impl OwnedState {
    pub(crate) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> Result<()> {
        let Self {
            board: _,
            sound,
            sound_event: _,
            audio_pending,
            epoch_tick: _,
            idle_clocks: _,
            fault,
            transfers: _,
        } = self;
        into.vector(audio_pending, K::AudioPending)?;
        if let Some(fault) = fault {
            into.text(fault, K::FaultText)?;
        }
        sound.visit_retained_allocations(into)
    }
}

#[cfg(all(
    test,
    not(any(
        feature = "sound-shadow-observer",
        feature = "first-failure-diagnostic"
    ))
))]
impl OwnedState {
    pub(crate) fn rollback_test_spare_audio(&mut self, extra: usize) -> (usize, usize) {
        assert!(self.audio_pending.is_empty());
        let before = self.audio_pending.capacity();
        self.audio_pending
            .reserve(before.checked_add(extra).unwrap());
        (before, self.audio_pending.capacity())
    }
    pub(crate) fn rollback_test_sound_cow(&mut self, byte: u8) {
        self.sound.rollback_test_sound_cow(byte);
    }
}
