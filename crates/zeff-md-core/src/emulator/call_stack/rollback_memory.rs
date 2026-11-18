use super::*;
use crate::emulator::rollback::memory::{MdAllocationKind as K, MdRetainedAllocations};

impl Tracker {
    pub(in crate::emulator) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> anyhow::Result<()> {
        let Self { recording } = self;
        if let Some(recording) = recording {
            into.boxed(recording.as_ref(), K::CallStack)?;
            let Recording {
                frames,
                unknown: _,
                truncated: _,
            } = recording.as_ref();
            if frames.capacity() != 0 {
                into.payload(
                    K::CallFrames,
                    frames.capacity(),
                    size_of::<CallStackFrame>(),
                )?;
            }
        }
        Ok(())
    }
}
