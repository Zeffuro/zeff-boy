use super::*;
impl SoundMemory {
    pub(crate) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut crate::emulator::rollback::MdRetainedAllocations<'a>,
    ) -> anyhow::Result<()> {
        let Self {
            ram,
            bank: _,
            requested: _,
            granted: _,
            reset_asserted: _,
            irq_asserted: _,
        } = self;
        ram.visit_retained_allocations(into)
    }
}
