use super::*;
use crate::emulator::rollback::memory::{MdAllocationKind as K, MdRetainedAllocations};

impl Audio {
    pub(crate) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> anyhow::Result<()> {
        let Self {
            psg: _,
            opn2: _,
            now: _,
            deferred: _,
            chip_phase: _,
            opn2_phase: _,
            sample_phase: _,
            sample_rate: _,
            master_hz: _,
            generation_enabled: _,
            mutes: _,
            fm_mutes: _,
            output: _,
            trace,
            trace_origin: _,
            samples,
            head: _,
            len: _,
        } = self;
        into.boxed(samples.as_ref(), K::AudioSamples)?;
        let capacity = trace.retained_event_capacity();
        if capacity != 0 {
            into.payload(
                K::AudioTraceEvents,
                capacity,
                size_of::<
                    zeff_emu_common::audio_trace::AudioTraceEvent<
                        zeff_emu_common::audio_trace::MdTraceWrite,
                    >,
                >(),
            )?;
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
impl Audio {
    pub(crate) fn rollback_test_phase(&mut self) {
        self.sample_phase ^= 1;
    }
    pub(crate) fn rollback_test_live_pcm(&mut self) {
        assert!(self.len != 0);
        self.samples[self.head][0] ^= 1;
    }
    pub(crate) fn rollback_test_storage(&mut self) {
        assert!(self.len < self.samples.len());
        self.samples.rotate_right(1);
        self.head = (self.head + 1) % self.samples.len();
        let consumed = (self.head + self.len) % self.samples.len();
        self.samples[consumed][0] ^= 1;
    }
}
