use super::*;
impl Domain<super::super::render::Renderer> {
    pub(in crate::hardware::vdp) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut crate::emulator::rollback::MdRetainedAllocations<'a>,
    ) -> Result<()> {
        let Self {
            pal: _,
            half: _,
            mode128: _,
            clocks: _,
            hv: _,
            irq: _,
            status: _,
            counter: _,
            control: _,
            frontend: _,
            wiring: _,
            ram: _,
            internal: _,
            render,
            old_dclk: _,
            feedback: _,
            zclk_feedback: _,
            board_clocks: _,
            render_dirty: _,
            input_cache: _,
            shared: _,
            ram_bus: _,
            io_data: _,
            memory_active: _,
            fifo_full: _,
            pending,
        } = self;
        if let Some(Receipt {
            half: _,
            effects,
            dma_activity: _,
            pixel: _,
        }) = pending
        {
            into.vector(
                effects,
                crate::emulator::rollback::MdAllocationKind::VdpEffects,
            )?;
        }
        render.classify_retained_fields();
        Ok(())
    }
}
