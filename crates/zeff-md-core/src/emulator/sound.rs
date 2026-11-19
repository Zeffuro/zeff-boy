use super::*;
use crate::hardware::bus::scheduled::reset_entry::{admission, scope};
use crate::hardware::sound::SoundFault;
use zeff_m68k::timing::physical::ResetQualification;
use zeff_z80::{Cpu, FetchedInstruction};

pub use admission::AdmissionError as WarmResetRejection;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WarmResetReport {
    pub elapsed_cpu_clocks: u64,
    pub elapsed_master_ticks: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WarmResetFailure {
    report: Result<Box<scope::Report>, scope::Rejection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WarmResetError {
    Rejected(WarmResetRejection),
    Failed(WarmResetFailure),
}

impl Emulator {
    pub fn check_warm_reset(&self) -> Result<(), WarmResetRejection> {
        if self.bus.tmss.is_some() || self.bus.cartridge.svp.is_some() {
            return Err(WarmResetRejection::CartridgeOwner);
        }
        if self.bus.owned.is_some() {
            return Err(WarmResetRejection::SoundContinuation);
        }
        if self.bus.input.uart_reset_blocked() {
            return Err(WarmResetRejection::InputSerial);
        }
        admission::admit(
            &self.cpu,
            &self.bus,
            &self.sound_schedule,
            ResetQualification::QualifiedSafeAssertionAndRelease,
        )?;
        if self.bus.audio.batch_active() {
            return Err(WarmResetRejection::AudioBatchActive);
        }
        let cartridge = &self.bus.cartridge;
        if cartridge.parallel_adapter.is_some()
            || cartridge.download.is_some()
            || cartridge.linear_rom.is_some()
            || cartridge.save.is_some()
            || cartridge.registers.is_some()
            || cartridge.protection.is_some()
            || cartridge.sf001.is_some()
            || cartridge.sf002.is_some()
            || cartridge.sf004.is_some()
            || cartridge.extended_ssf.is_some()
            || cartridge.realtec.is_some()
            || cartridge.ssf2.is_some()
            || cartridge.lock_on.is_some()
            || cartridge.lock_on_save.is_some()
        {
            return Err(WarmResetRejection::CartridgeOwner);
        }
        let sound = &self.bus.sound;
        if sound.bank != 0 || sound.requested || sound.granted || sound.irq_asserted {
            return Err(WarmResetRejection::SoundLatches);
        }
        if self.sound_schedule.epoch.is_some() {
            return Err(WarmResetRejection::SoundEpoch);
        }
        if self
            .sound_schedule
            .validate_state(&self.sound_cpu, self.bus.now.get())
            .is_err()
            || self.sound_schedule.validate_cartridge(cartridge).is_err()
        {
            return Err(WarmResetRejection::SoundContinuation);
        }
        if self.sound_schedule.pending.as_ref().is_some_and(|pending| {
            pending.start > self.bus.now.get() || pending.deadline <= self.bus.now.get()
        }) || sound.reset_asserted
            && (self.sound_schedule.pending.is_some()
                || self.sound_schedule.state != zeff_z80::BusState::default()
                || self.sound_cpu != Cpu::new())
        {
            return Err(WarmResetRejection::SoundContinuation);
        }
        Ok(())
    }

    /// Functional MAIN boundary event; the physical reset hold interval is not modeled.
    pub fn warm_reset(&mut self) -> Result<WarmResetReport, WarmResetError> {
        self.warm_reset_captured(&mut NoSoundCapture)
    }

    pub(super) fn warm_reset_captured<C: SoundCapture>(
        &mut self,
        capture: &mut C,
    ) -> Result<WarmResetReport, WarmResetError> {
        self.check_warm_reset().map_err(WarmResetError::Rejected)?;
        self.bus.input.retire_uart_reset();
        let asserted = self.bus.active_sound_memory().reset_asserted;
        self.bus.set_sound_reset(true);
        if !asserted {
            // Only the unpublished future sound candidate is canceled.
            self.sound_schedule
                .reset_cpu(&mut self.sound_cpu, &mut self.bus.sound, capture);
        }
        let result = scope::run(
            &mut self.cpu,
            &mut self.bus,
            (&mut self.sound_cpu, &mut self.sound_schedule),
            self.concurrent_sound,
            ResetQualification::QualifiedSafeAssertionAndRelease,
            capture,
        );
        self.main_stopped = self.cpu.stopped();
        self.reset_call_stack();
        match result {
            scope::Outcome::Entered(report) if report.completion.is_ok() => Ok(WarmResetReport {
                elapsed_cpu_clocks: report.elapsed_cpu.expect("qualified reset clocks"),
                elapsed_master_ticks: report.elapsed_master.expect("qualified reset ticks"),
            }),
            outcome => {
                capture.fault();
                self.bus.clock_fault = true;
                self.bus.audio.trace_origin = None;
                Err(WarmResetError::Failed(WarmResetFailure {
                    report: match outcome {
                        scope::Outcome::Entered(report) => Ok(report),
                        scope::Outcome::Rejected(reason) => Err(reason),
                    },
                }))
            }
        }
    }

    pub fn set_sound_interrupt(&mut self, asserted: bool) -> bool {
        if self.bus.owned.is_some() {
            return false;
        }
        self.invalidate_audio_trace(
            zeff_emu_common::audio_trace::AudioTraceInvalidation::ExternalMutation,
        );
        self.bus.sound.irq_asserted = asserted;
        true
    }

    pub fn sound_cpu(&self) -> &Cpu {
        self.bus.owned_sound_cpu().unwrap_or(&self.sound_cpu)
    }
    pub fn peek_sound(&self, address: u16) -> Option<u8> {
        (address <= crate::constants::SOUND_RAM_END)
            .then(|| self.bus.active_sound_memory().read(address))
            .flatten()
    }

    pub fn sound_bus_requested(&self) -> bool {
        self.bus.active_sound_memory().requested
    }
    pub fn sound_bus_granted(&self) -> bool {
        self.bus.active_sound_memory().granted
    }
    pub fn sound_reset_asserted(&self) -> bool {
        self.bus.active_sound_memory().reset_asserted
    }
    pub fn sound_bank(&self) -> u16 {
        self.bus.active_sound_memory().bank
    }

    pub fn step_sound_instruction(&mut self) -> Result<FetchedInstruction, SoundFault> {
        self.step_sound_instruction_captured(&mut NoSoundCapture)
    }

    pub(super) fn step_sound_instruction_captured<C: SoundCapture>(
        &mut self,
        capture: &mut C,
    ) -> Result<FetchedInstruction, SoundFault> {
        if let Some(fault) = self.input_fault() {
            return Err(SoundFault::InputBlocked(fault));
        }
        let result = self.step_sound_boundary(capture);
        if let Err(fault) = result {
            self.invalidate_trace_fault(&MachineFault::Sound(fault));
        }
        result
    }

    fn step_sound_boundary<C: SoundCapture>(
        &mut self,
        capture: &mut C,
    ) -> Result<FetchedInstruction, SoundFault> {
        if self.concurrent_sound {
            return Err(SoundFault::ConcurrentSchedulingEnabled);
        }
        if !self.main_stopped {
            return Err(SoundFault::MainCpuRunning);
        }
        if self.bus.interrupt_level() != 0 {
            return Err(SoundFault::MainInterruptPending);
        }
        if self.bus.active_sound_memory().reset_asserted {
            return Err(SoundFault::ResetAsserted);
        }
        if self.bus.active_sound_memory().requested || self.bus.active_sound_memory().granted {
            return Err(SoundFault::BusRequested);
        }
        if self.bus.clock_fault {
            return Err(SoundFault::ClockOverflow);
        }
        let result = (|| {
            let (mut pending, attempt) = C::whole(
                &self.sound_cpu,
                &self.bus.sound,
                &self.bus.cartridge,
                self.bus.now.get(),
            )?;
            capture.prepared(attempt);
            if self.bus.audio.trace.is_enabled() {
                pending.trace_origin = Some(crate::hardware::audio::trace::sound_origin(
                    self.sound_cpu.regs().pc,
                    self.bus.active_sound_memory().bank,
                    &self.bus.cartridge,
                ));
            }
            pending.refresh_reads_captured(
                &self.sound_cpu,
                &self.bus.sound,
                &self.bus.cartridge,
                &self.bus.audio,
                capture,
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
                &mut self.bus.cartridge,
            );
            capture.publish(&self.sound_cpu);
            self.bus.prefetch = Prefetch::Empty;
            Ok(instruction)
        })();
        if result.is_err() {
            capture.discard_pending();
            capture.fault();
        }
        result
    }

    pub fn sound_fault(&self) -> Option<SoundFault> {
        if self.bus.owned.is_some() {
            return None;
        }
        self.sound_schedule.fault
    }
    pub fn sound_prefix_pending(&self) -> bool {
        self.bus
            .active_sound_state()
            .unwrap_or(self.sound_schedule.state)
            .prefix_pending()
    }
    pub fn sound_instruction_pending(&self) -> bool {
        self.bus
            .active_sound_state()
            .unwrap_or(self.sound_schedule.state)
            .pending()
    }
    pub fn sound_deadline(&self) -> Option<MasterTicks> {
        if self.bus.owned.is_some() {
            return None;
        }
        self.sound_schedule
            .pending
            .as_ref()
            .map(|pending| MasterTicks::new(pending.deadline))
    }
    pub fn reset_sound_cpu(&mut self) -> bool {
        self.reset_sound_cpu_captured(&mut NoSoundCapture)
    }

    pub(super) fn reset_sound_cpu_captured<C: SoundCapture>(&mut self, capture: &mut C) -> bool {
        if self.bus.owned.is_some() {
            return false;
        }
        self.invalidate_audio_trace(
            zeff_emu_common::audio_trace::AudioTraceInvalidation::ExternalMutation,
        );
        self.sound_schedule
            .reset_cpu(&mut self.sound_cpu, &mut self.bus.sound, capture);
        true
    }
}
