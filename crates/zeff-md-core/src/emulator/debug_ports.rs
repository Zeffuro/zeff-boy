use super::*;
use zeff_emu_common::audio_trace::AudioTraceInvalidation;
use zeff_m68k::timing::{ClockedBus, DebugDataWrite};

mod save;
pub use save::DebugSavePorts;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugPortWrite {
    Byte { address: u32, value: u8 },
    Word { address: u32, value: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebugPortError {
    UnsupportedPort { index: usize },
    Rejected(&'static str),
    Execution { index: usize, fault: MachineFault },
    Checkpoint(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebugPortReport {
    pub elapsed: MasterTicks,
    pub cpu_clocks: CpuClocks,
    pub instructions: u64,
    pub writes: usize,
}

struct Write(DebugDataWrite);

impl CpuOperation for Write {
    type Output = CpuClocks;
    const SAMPLE_IRQ_INPUT: bool = false;

    fn step<B: ClockedBus>(
        &mut self,
        cpu: &mut PrefetchCpu,
        bus: &mut B,
    ) -> Result<Self::Output, TimingFault<B::Error>> {
        cpu.debug_data_write(bus, self.0)
    }
}

impl DebugPortWrite {
    fn transfer(self) -> Option<DebugDataWrite> {
        match self {
            Self::Byte {
                address:
                    address @ (0xa00000..=0xa01fff
                    | 0xa04000..=0xa04003
                    | 0xa06000
                    | 0xa1000f
                    | 0xa10013
                    | 0xa10015
                    | 0xa10019
                    | 0xa11100
                    | 0xa11200
                    | 0xc00011),
                value,
            } => Some(DebugDataWrite::Byte { address, value }),
            Self::Word {
                address: address @ (0xc00000 | 0xc00004),
                value,
            } => Some(DebugDataWrite::Word { address, value }),
            _ => None,
        }
    }
}

impl Emulator {
    pub fn debug_write_ports(
        &mut self,
        writes: &[DebugPortWrite],
    ) -> Result<DebugPortReport, DebugPortError> {
        self.debug_write_ports_captured(writes, &mut NoSoundCapture)
    }

    pub(super) fn debug_write_ports_captured<C: SoundCapture>(
        &mut self,
        writes: &[DebugPortWrite],
        capture: &mut C,
    ) -> Result<DebugPortReport, DebugPortError> {
        let checkpoint = capture.checkpoint();
        let result = self.debug_write_ports_inner(writes, capture);
        if result.is_err() {
            capture.rollback(checkpoint);
        }
        result
    }

    fn debug_write_ports_inner<C: SoundCapture>(
        &mut self,
        writes: &[DebugPortWrite],
        capture: &mut C,
    ) -> Result<DebugPortReport, DebugPortError> {
        if writes.is_empty() {
            return Ok(DebugPortReport {
                elapsed: MasterTicks::ZERO,
                cpu_clocks: CpuClocks(0),
                instructions: 0,
                writes: 0,
            });
        }
        let transfers: Vec<_> = writes
            .iter()
            .enumerate()
            .map(|(index, write)| {
                write
                    .transfer()
                    .or_else(|| self.save_port_transfer(*write))
                    .ok_or(DebugPortError::UnsupportedPort { index })
            })
            .collect::<Result<_, _>>()?;
        self.check_debug_ports()?;
        self.validate_debug_port_checkpoint()?;
        let before = self.bus.now;
        let clock = self.cpu.clock();
        let mut candidate = self.clone();
        candidate.invalidate_audio_trace(AudioTraceInvalidation::ExternalMutation);
        candidate.bus.audio.trace_origin = None;
        if candidate.concurrent_sound && candidate.bus.owned.is_none() {
            candidate.bus.audio.begin_batch();
        }
        let result = transfers
            .into_iter()
            .enumerate()
            .try_for_each(|(index, transfer)| {
                let result = if candidate.bus.owned.is_some() {
                    candidate.step_owned_operation_capturing(
                        &mut Write(transfer),
                        &mut capture.physical(),
                    )
                } else {
                    candidate.step_main_operation_with(&mut Write(transfer), capture)
                };
                result
                    .map(|_| ())
                    .map_err(|fault| DebugPortError::Execution { index, fault })
            });
        let fault = match &result {
            Err(DebugPortError::Execution { fault, .. }) => Some(fault),
            _ => None,
        };
        candidate.finish_execution(fault);
        result?;
        candidate.validate_debug_port_checkpoint()?;
        let report = DebugPortReport {
            elapsed: candidate
                .bus
                .now
                .checked_duration_since(before)
                .expect("monotonic port time"),
            cpu_clocks: CpuClocks(candidate.cpu.clock().0 - clock.0),
            instructions: 0,
            writes: writes.len(),
        };
        *self = candidate;
        Ok(report)
    }

    fn check_debug_ports(&self) -> Result<(), DebugPortError> {
        if self.bus.audio.batch_active() {
            return Err(DebugPortError::Rejected("audio batch is active"));
        }
        #[cfg(feature = "first-failure-diagnostic")]
        if crate::diagnostic::enabled() || crate::diagnostic::field::enabled() {
            return Err(DebugPortError::Rejected(
                "instruction diagnostic capture is active",
            ));
        }
        self.cpu
            .check_debug_data_write::<BusError>()
            .map_err(|fault| DebugPortError::Execution {
                index: 0,
                fault: fault.into(),
            })?;
        if let Some(fault) = self.input_fault() {
            return Err(DebugPortError::Execution {
                index: 0,
                fault: MachineFault::Input(fault),
            });
        }
        if self.bus.clock_fault {
            return Err(DebugPortError::Execution {
                index: 0,
                fault: TimingFault::ClockOverflow.into(),
            });
        }
        if let Some(fault) = self.sound_schedule.fault {
            return Err(DebugPortError::Execution {
                index: 0,
                fault: MachineFault::Sound(fault),
            });
        }
        Ok(())
    }

    fn validate_debug_port_checkpoint(&self) -> Result<(), DebugPortError> {
        let bytes = self
            .encode_debugger_checkpoint()
            .map_err(|error| DebugPortError::Checkpoint(error.to_string()))?;
        // Decode only the validation clone: the live candidate retains output and trace state.
        self.clone()
            .decode_debugger_checkpoint(&bytes)
            .map_err(|error| DebugPortError::Checkpoint(error.to_string()))
    }
}

#[cfg(test)]
mod tests;
