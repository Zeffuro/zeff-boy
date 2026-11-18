use super::*;
use crate::emulator::rollback::memory::{MdAllocationKind as K, MdRetainedAllocations};

impl Bus {
    pub(crate) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> anyhow::Result<()> {
        let Self {
            region: _,
            tmss: _,
            cartridge,
            ram,
            now: _,
            cpu_clock: _,
            irq_level: _,
            vdp,
            reset_pulses: _,
            reset_output: _,
            rmw: _,
            clock_fault: _,
            sound,
            input: _,
            prefetch: _,
            video,
            audio,
            owned,
        } = self;
        cartridge.visit_retained_allocations(into)?;
        into.boxed(ram.as_ref(), K::WorkRam)?;
        sound.visit_retained_allocations(into)?;
        vdp.visit_retained_allocations(into)?;
        video.visit_retained_allocations(into)?;
        audio.visit_retained_allocations(into)?;
        if let Some(owner) = owned {
            into.boxed(owner.as_ref(), K::OwnedBus)?;
            owner.visit_retained_allocations(into)?;
        }
        Ok(())
    }
}
