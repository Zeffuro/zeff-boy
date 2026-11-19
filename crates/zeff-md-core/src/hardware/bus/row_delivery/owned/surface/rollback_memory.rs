use super::*;
use crate::emulator::rollback::memory::{MdAllocationKind as K, MdRetainedAllocations};

impl Surface {
    pub(in crate::hardware::bus) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> Result<()> {
        let Self {
            field: _,
            pixels,
            seen,
            widths: _,
            width: _,
            height: _,
            mode5: _,
            mixed: _,
            active: _,
            closed: _,
            published: _,
            count: _,
            incomplete: _,
            last,
        } = self;
        into.boxed(pixels.as_ref(), K::SurfacePixels)?;
        into.boxed(seen.as_ref(), K::SurfaceSeen)?;
        if let Some((_, _, pixels)) = last {
            into.boxed(pixels.as_ref(), K::SurfaceLastPixels)?;
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
impl Surface {
    pub(in crate::hardware::bus) fn rollback_test_last_frame(&mut self) {
        self.last = Some((320, 240, self.pixels.clone()));
    }
}
