use super::*;
use crate::emulator::rollback::memory::{MdAllocationKind as K, MdRetainedAllocations};

impl SoundSchedule {
    pub(crate) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> anyhow::Result<()> {
        let Self {
            pending,
            rmw_checkpoint,
            fault: _,
            state: _,
            epoch: _,
        } = self;
        if let Some(pending) = pending {
            pending.visit_retained_allocations(into)?;
        }
        if let Some(checkpoint) = rmw_checkpoint {
            into.boxed(checkpoint.as_ref(), K::SoundCheckpoint)?;
            checkpoint.visit_retained_allocations(into)?;
        }
        Ok(())
    }
}
impl PendingSound {
    fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> anyhow::Result<()> {
        let Self {
            cpu: _,
            memory,
            instruction: _,
            deadline: _,
            state: _,
            writes: _,
            register_writes: _,
            sf001_writes: _,
            sf002_writes: _,
            psg_writes: _,
            opn2_writes: _,
            bank_access: _,
            observed_bank_access: _,
            any_bank_access: _,
            start: _,
            replay: _,
            trace_origin: _,
            svp_history,
            #[cfg(feature = "sound-shadow-observer")]
                observer: _,
        } = self;
        anyhow::ensure!(
            svp_history.is_none(),
            "SVP history has no rollback admission"
        );
        memory.visit_retained_allocations(into)
    }
}
