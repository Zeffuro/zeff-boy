use super::*;
use crate::emulator::rollback::memory::{MdAllocationKind as K, MdRetainedAllocations};

impl Vdp {
    pub(crate) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> anyhow::Result<()> {
        let Self {
            state: _,
            memory,
            interval_hint: _,
            dma_activity_epoch: _,
            owned,
        } = self;
        memory.visit_retained_allocations(into)?;
        if let Some(runtime) = owned {
            into.boxed(runtime.as_ref(), K::VdpRuntime)?;
            runtime.visit_retained_allocations(into)?;
        }
        Ok(())
    }
}
