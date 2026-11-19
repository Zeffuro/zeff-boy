use super::*;
use crate::emulator::rollback::memory::{MdAllocationKind as K, MdRetainedAllocations};

impl SoundOwner {
    pub(crate) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> Result<()> {
        let Self {
            clock: _,
            bus: _,
            semantic,
            effects,
            released: _,
            completed: _,
            last_external: _,
            request_low: _,
            wait_high: _,
            irq_low: _,
            boundary_irq: _,
            reset: _,
            stable_grant: _,
            m1_history: _,
            refresh_hold: _,
            data_hold: _,
            jr_tail: _,
            word_tail: _,
            djnz_tail: _,
            copy_history: _,
        } = self;
        into.vector(effects, K::SoundEffects)?;
        if into.arc(semantic, K::SoundSemantic)? {
            let Semantic {
                cpu: _,
                state: _,
                memory,
            } = semantic.as_ref();
            memory.visit_retained_allocations(into)?;
        }
        Ok(())
    }
}

#[cfg(all(
    test,
    not(any(
        feature = "sound-shadow-observer",
        feature = "first-failure-diagnostic"
    ))
))]
impl SoundOwner {
    pub(crate) fn rollback_test_sound_cow(&mut self, byte: u8) {
        Arc::make_mut(&mut self.semantic).memory.ram[0] = byte;
    }
}
