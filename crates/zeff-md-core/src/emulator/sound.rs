use super::*;
use crate::hardware::sound::SoundFault;
use zeff_z80::{Cpu, FetchedInstruction};

impl Emulator {
    pub fn set_sound_interrupt(&mut self, asserted: bool) {
        self.invalidate_audio_trace(
            zeff_emu_common::audio_trace::AudioTraceInvalidation::ExternalMutation,
        );
        self.bus.sound.irq_asserted = asserted;
    }

    pub fn sound_cpu(&self) -> &Cpu {
        &self.sound_cpu
    }
    pub fn sound_bus_requested(&self) -> bool {
        self.bus.sound.requested
    }
    pub fn sound_bus_granted(&self) -> bool {
        self.bus.sound.granted
    }
    pub fn sound_reset_asserted(&self) -> bool {
        self.bus.sound.reset_asserted
    }
    pub fn sound_bank(&self) -> u16 {
        self.bus.sound.bank
    }

    /// Boundary-only sound stepping after an observed main STOP.
    pub fn step_sound_instruction(&mut self) -> Result<FetchedInstruction, SoundFault> {
        let result = self.step_sound_boundary();
        if let Err(fault) = result {
            self.invalidate_trace_fault(&MachineFault::Sound(fault));
        }
        result
    }

    fn step_sound_boundary(&mut self) -> Result<FetchedInstruction, SoundFault> {
        if self.concurrent_sound {
            return Err(SoundFault::ConcurrentSchedulingEnabled);
        }
        if !self.main_stopped {
            return Err(SoundFault::MainCpuRunning);
        }
        if self.bus.interrupt_level() != 0 {
            return Err(SoundFault::MainInterruptPending);
        }
        if self.bus.sound.reset_asserted {
            return Err(SoundFault::ResetAsserted);
        }
        if self.bus.sound.requested || self.bus.sound.granted {
            return Err(SoundFault::BusRequested);
        }
        if self.bus.clock_fault {
            return Err(SoundFault::ClockOverflow);
        }
        let mut pending = crate::hardware::sound::schedule::PendingSound::prepare(
            &self.sound_cpu,
            &self.bus.sound,
            &self.bus.cartridge,
            self.bus.now.get(),
        )?;
        if self.bus.audio.trace.is_enabled() {
            pending.trace_origin = Some(crate::hardware::audio::trace::sound_origin(
                self.sound_cpu.regs().pc,
                self.bus.sound.bank,
                &self.bus.cartridge,
            ));
        }
        pending.refresh_opn2(
            &self.sound_cpu,
            &self.bus.sound,
            &self.bus.cartridge,
            &self.bus.audio,
        )?;
        let instruction = pending.instruction;
        self.bus
            .advance_machine(pending.deadline - self.bus.now.get())
            .map_err(|_| SoundFault::ClockOverflow)?;
        pending.publish(
            &mut self.sound_cpu,
            &mut self.bus.sound,
            &mut self.bus.ram,
            &mut self.bus.audio,
        );
        self.bus.prefetch_upper = None;
        Ok(instruction)
    }

    pub fn sound_fault(&self) -> Option<SoundFault> {
        self.sound_schedule.fault
    }
    pub fn sound_prefix_pending(&self) -> bool {
        self.sound_schedule.state.prefix_pending()
    }
    pub fn sound_instruction_pending(&self) -> bool {
        self.sound_schedule.state.pending()
    }
    pub fn sound_deadline(&self) -> Option<MasterTicks> {
        self.sound_schedule
            .pending
            .as_ref()
            .map(|pending| MasterTicks::new(pending.deadline))
    }
    pub fn reset_sound_cpu(&mut self) {
        self.invalidate_audio_trace(
            zeff_emu_common::audio_trace::AudioTraceInvalidation::ExternalMutation,
        );
        self.sound_schedule
            .reset_cpu(&mut self.sound_cpu, &mut self.bus.sound);
    }
}
