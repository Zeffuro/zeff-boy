use super::*;
use crate::sound_trace::SoundTraceAttachment;

impl Emulator {
    pub fn passive_sound_trace_rejection(&self) -> Option<&'static str> {
        if self.bus.owned.is_some() {
            Some("SOUND observation requires Standard timing")
        } else if self.bus.cartridge.svp.is_some() {
            Some("SOUND observation is unavailable with SVP")
        } else if !self.concurrent_sound {
            Some("SOUND observation requires concurrent SOUND scheduling")
        } else {
            None
        }
    }
    fn check_standard_sound_trace(&self) -> Result<(), MachineFault> {
        if self.bus.owned.is_some() || self.bus.cartridge.svp.is_some() {
            return Err(MachineFault::Sound(crate::SoundFault::ExpansionUnavailable));
        }
        Ok(())
    }
    pub fn step_instruction_sound_traced(
        &mut self,
        sound: &mut SoundTraceAttachment<'_>,
    ) -> Result<MachineStep, MachineFault> {
        self.check_standard_sound_trace()?;
        self.step_instruction_with(&mut Direct, sound)
    }
    pub fn step_instruction_observers(
        &mut self,
        main: &mut impl InstructionObserver,
        sound: &mut SoundTraceAttachment<'_>,
    ) -> Result<MachineStep, MachineFault> {
        self.check_standard_sound_trace()?;
        self.step_instruction_with(&mut Observer(main), sound)
    }
    pub fn step_field_sound_traced(
        &mut self,
        sound: &mut SoundTraceAttachment<'_>,
    ) -> Result<(), MachineFault> {
        self.check_standard_sound_trace()?;
        let mut execution = field::FieldExecution::begin(self)?;
        loop {
            match execution.resume_captured(self, std::num::NonZeroUsize::MAX, sound)? {
                Some(pending) => execution = pending,
                None => return Ok(()),
            }
        }
    }
    pub fn begin_debug_call_sound_traced(
        &mut self,
        target: u32,
        sound: &mut SoundTraceAttachment<'_>,
    ) -> Result<DebugCallContext, GuestCallError> {
        self.check_standard_sound_trace()
            .map_err(GuestCallError::Execution)?;
        self.begin_debug_call_captured(target, sound)
    }
    pub fn debug_write_ports_sound_traced(
        &mut self,
        writes: &[DebugPortWrite],
        sound: &mut SoundTraceAttachment<'_>,
    ) -> Result<DebugPortReport, DebugPortError> {
        self.check_standard_sound_trace()
            .map_err(|fault| DebugPortError::Execution { index: 0, fault })?;
        self.debug_write_ports_captured(writes, sound)
    }
    pub fn warm_reset_sound_traced(
        &mut self,
        sound: &mut SoundTraceAttachment<'_>,
    ) -> Result<WarmResetReport, WarmResetError> {
        self.warm_reset_captured(sound)
    }
    pub fn reset_cpu_sound_traced(
        &mut self,
        sound: &mut SoundTraceAttachment<'_>,
    ) -> Result<(), TimingFault<BusError>> {
        self.reset_main_cpu_with_capture(sound)
    }
    pub fn reset_sound_cpu_traced(&mut self, sound: &mut SoundTraceAttachment<'_>) -> bool {
        if self.check_standard_sound_trace().is_err() {
            return false;
        }
        self.reset_sound_cpu_captured(sound)
    }
    pub fn reset_sound_traced(
        &mut self,
        sound: &mut SoundTraceAttachment<'_>,
    ) -> Result<(), MachineFault> {
        self.check_standard_sound_trace()?;
        sound.reset();
        self.reset();
        Ok(())
    }
    pub fn step_sound_instruction_traced(
        &mut self,
        sound: &mut SoundTraceAttachment<'_>,
    ) -> Result<zeff_z80::FetchedInstruction, crate::SoundFault> {
        self.check_standard_sound_trace()
            .map_err(|_| crate::SoundFault::ExpansionUnavailable)?;
        self.step_sound_instruction_captured(sound)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod physical_tests;

#[cfg(test)]
mod producer_tests;
