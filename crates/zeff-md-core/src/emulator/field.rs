use super::{Emulator, MachineFault};
use crate::sound_trace::{NoSoundCapture, SoundCapture};
use std::num::NonZeroUsize;

pub(super) struct FieldExecution {
    field: u64,
}

impl FieldExecution {
    pub(super) fn begin(core: &mut Emulator) -> Result<Self, MachineFault> {
        if let Some(fault) = core.input_fault() {
            return Err(MachineFault::Input(fault));
        }
        #[cfg(feature = "first-failure-diagnostic")]
        if core.bus.owned.is_none() && crate::diagnostic::field::enabled() {
            crate::diagnostic::field::call_begin(
                core as *const Emulator as usize,
                &core.bus as *const super::Bus as usize,
                crate::diagnostic::field::Endpoint::read(core),
            );
        }
        let field = core.pacing_field_count();
        core.clear_frame_ready();
        if core.concurrent_sound && core.bus.owned.is_none() {
            core.bus.audio.begin_batch();
        }
        if core.concurrent_sound && core.bus.owned.is_none() && !core.bus.audio.trace.is_enabled() {
            core.sound_schedule.begin_epoch_field();
        }
        Ok(Self { field })
    }

    pub(super) fn resume(
        self,
        core: &mut Emulator,
        budget: NonZeroUsize,
    ) -> Result<Option<Self>, MachineFault> {
        self.resume_with(core, budget, &mut NoSoundCapture, |core, _| {
            core.step_traced_boundary()
        })
    }

    pub(super) fn resume_captured<C: SoundCapture>(
        self,
        core: &mut Emulator,
        budget: NonZeroUsize,
        capture: &mut C,
    ) -> Result<Option<Self>, MachineFault> {
        self.resume_with(core, budget, capture, |core, capture| {
            core.step_traced_boundary_with(&mut super::Direct, capture)
        })
    }

    fn resume_with<C: SoundCapture>(
        self,
        core: &mut Emulator,
        budget: NonZeroUsize,
        capture: &mut C,
        mut step: impl FnMut(&mut Emulator, &mut C) -> Result<super::MachineStep, MachineFault>,
    ) -> Result<Option<Self>, MachineFault> {
        for _ in 0..budget.get() {
            if let Err(fault) = step(core, capture) {
                Self::finish(core, Err(fault), capture)?;
                return Ok(None);
            }
            if core.pacing_field_count() != self.field {
                Self::finish(core, Ok(()), capture)?;
                return Ok(None);
            }
            core.bus.audio.trace_origin = None;
        }
        Ok(Some(self))
    }

    fn finish<C: SoundCapture>(
        core: &mut Emulator,
        result: Result<(), MachineFault>,
        capture: &mut C,
    ) -> Result<(), MachineFault> {
        if core.bus.owned.is_none() {
            core.sound_schedule
                .finish_epoch_field(&mut core.sound_cpu, &core.bus.sound, capture);
        }
        core.finish_execution(result.as_ref().err());
        #[cfg(feature = "first-failure-diagnostic")]
        if core.bus.owned.is_none() && crate::diagnostic::field::enabled() {
            crate::diagnostic::field::call_end(
                crate::diagnostic::field::Endpoint::read(core),
                &result,
            );
        }
        result
    }
}
