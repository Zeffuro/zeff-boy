use super::*;
use crate::emulator::rollback::memory::{MdAllocationKind as K, MdRetainedAllocations};

impl RowDelivery {
    pub(crate) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> anyhow::Result<()> {
        let Self {
            owned,
            scroll: _,
            pending: _,
            completed: _,
            framebuffer,
            width: _,
            height: _,
            rows: _,
            frame_count: _,
            frame_ready: _,
            raster_error: _,
            field: _,
            valid_fields: _,
            unknown_field: _,
            framebuffer_known: _,
        } = self;
        into.boxed(framebuffer.as_ref(), K::Framebuffer)?;
        if let Some(surface) = owned {
            into.boxed(surface.as_ref(), K::Surface)?;
            surface.visit_retained_allocations(into)?;
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
impl RowDelivery {
    pub(crate) fn rollback_test_last_frame(&mut self) {
        self.owned
            .as_mut()
            .expect("owned test video")
            .rollback_test_last_frame();
    }
}
