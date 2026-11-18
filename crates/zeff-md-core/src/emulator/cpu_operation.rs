use super::*;
use crate::sound_trace::SoundCapture;
use zeff_m68k::timing::ClockedBus;

pub(super) trait CpuOperation {
    type Output;
    const SAMPLE_IRQ_INPUT: bool = true;

    fn step<B: ClockedBus>(
        &mut self,
        cpu: &mut PrefetchCpu,
        bus: &mut B,
    ) -> Result<Self::Output, TimingFault<B::Error>>;

    fn stopped(_output: &Self::Output) -> bool {
        false
    }
}

pub(super) struct OperationStep<T> {
    pub(super) output: T,
    pub(super) elapsed: MasterTicks,
}

impl Emulator {
    pub(super) fn step_main_operation_with<O: CpuOperation, C: SoundCapture>(
        &mut self,
        operation: &mut O,
        capture: &mut C,
    ) -> Result<OperationStep<O::Output>, MachineFault> {
        if let Some(fault) = self.input_fault() {
            return Err(MachineFault::Input(fault));
        }
        if self.bus.clock_fault {
            return Err(TimingFault::ClockOverflow.into());
        }
        if let Some(fault) = self.sound_schedule.fault {
            return Err(MachineFault::Sound(fault));
        }
        self.begin_call_stack_operation();
        let result: Result<OperationStep<O::Output>, MachineFault> = (|| {
            self.main_stopped = false;
            let before = self.bus.now;
            let sound_reset_before = self.bus.sound.reset_asserted;
            if O::SAMPLE_IRQ_INPUT {
                self.cpu
                    .interrupt_input()
                    .set_level(self.bus.interrupt_level());
            }
            let result = if self.concurrent_sound {
                #[cfg(feature = "first-failure-diagnostic")]
                {
                    operation
                        .step(
                            &mut self.cpu,
                            &mut crate::diagnostic::TraceBus::new(&mut CompactBus(ScheduledBus {
                                capture: &mut *capture,
                                bus: &mut self.bus,
                                cpu: &mut self.sound_cpu,
                                schedule: &mut self.sound_schedule,
                            })),
                        )
                        .map_err(expand_fault)
                }
                #[cfg(not(feature = "first-failure-diagnostic"))]
                {
                    operation
                        .step(
                            &mut self.cpu,
                            &mut CompactBus(ScheduledBus {
                                capture: &mut *capture,
                                bus: &mut self.bus,
                                cpu: &mut self.sound_cpu,
                                schedule: &mut self.sound_schedule,
                            }),
                        )
                        .map_err(expand_fault)
                }
            } else {
                #[cfg(feature = "first-failure-diagnostic")]
                {
                    operation.step(
                        &mut self.cpu,
                        &mut crate::diagnostic::TraceBus::new(&mut self.bus),
                    )
                }
                #[cfg(not(feature = "first-failure-diagnostic"))]
                {
                    operation.step(&mut self.cpu, &mut self.bus)
                }
            };
            if !self.concurrent_sound && !sound_reset_before && self.bus.sound.reset_asserted {
                capture.reset();
                self.sound_cpu.reset();
            }
            if !self.concurrent_sound {
                self.bus.sound.complete_boundary();
            }
            let output = result?;
            if self.bus.clock_fault {
                return Err(TimingFault::ClockOverflow.into());
            }
            if O::stopped(&output) {
                let result = if self.concurrent_sound {
                    ScheduledBus {
                        capture: &mut *capture,
                        bus: &mut self.bus,
                        cpu: &mut self.sound_cpu,
                        schedule: &mut self.sound_schedule,
                    }
                    .advance_machine(STOP_POLL_TICKS)
                } else {
                    self.bus.advance_machine(STOP_POLL_TICKS)
                };
                result.map_err(|_| TimingFault::ClockOverflow)?;
                self.bus.prefetch = Prefetch::Empty;
            }
            self.main_stopped = O::stopped(&output);
            if let Some(fault) = self.sound_schedule.fault {
                return Err(MachineFault::Sound(fault));
            }
            let elapsed = self.bus.now.checked_duration_since(before).unwrap();
            Ok(OperationStep { output, elapsed })
        })();
        if result.is_err() {
            capture.fault();
        }
        self.finish_call_stack_operation(result.is_ok());
        result
    }
}
