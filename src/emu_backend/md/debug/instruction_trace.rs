use super::*;
use zeff_emu_common::debug::{
    InstructionTraceRecord, RegisterDelta, TraceBoundary, TraceBoundaryKind, TraceExecMode,
    TraceWrite, TraceWriteKind, TraceWriteWidth,
};
use zeff_m68k::Registers;
use zeff_m68k::bus::{BusAccess, ByteLanes};
use zeff_m68k::timing::InstructionInput;
use zeff_md_core::MachineStep;
use zeff_md_core::emulator::InstructionObserver;

struct Capture<'a> {
    debugger: &'a mut PcBreakpoints,
    record: Option<InstructionTraceRecord>,
    first_input: Option<(u32, InstructionInput)>,
    outcome: Option<TimedOutcome>,
}

impl InstructionObserver for Capture<'_> {
    fn needs_previous_values(&self) -> bool {
        self.record.is_some() || !self.debugger.controller.watchpoints.is_empty()
    }

    fn instruction_word(&mut self, _address: u32, value: u16) {
        let Some(record) = &mut self.record else {
            return;
        };
        let length = usize::from(record.instruction_len);
        if let Some(target) = record.instruction.get_mut(length..length + 2) {
            target.copy_from_slice(&value.to_be_bytes());
            record.instruction_len += 2;
        } else {
            record.instruction_overflow = record.instruction_overflow.saturating_add(2);
        }
    }

    fn instruction_input(&mut self, address: u32, input: InstructionInput) {
        if self.first_input.is_none() {
            self.first_input = Some((address, input));
        }
        self.instruction_word(address, input.value);
    }

    fn memory_access(&mut self, access: MemoryAccess) {
        self.debugger.observe(access);
    }

    fn write_with_previous(&mut self, access: BusAccess, value: u16, old: [Option<u8>; 2]) {
        let (address, value, width) = match access.lanes {
            ByteLanes::Both => (access.bus_address, value, TraceWriteWidth::Halfword),
            ByteLanes::Upper => (access.bus_address, value >> 8, TraceWriteWidth::Byte),
            ByteLanes::Lower => (access.bus_address | 1, value & 0xff, TraceWriteWidth::Byte),
        };
        let old_value = match access.lanes {
            ByteLanes::Both => old[0]
                .zip(old[1])
                .map(|(high, low)| u32::from(u16::from_be_bytes([high, low]))),
            ByteLanes::Upper => old[0].map(u32::from),
            ByteLanes::Lower => old[1].map(u32::from),
        };
        let Some(record) = &mut self.record else {
            return;
        };
        record.push_write(TraceWrite {
            address,
            old_value,
            new_value: u32::from(value),
            width,
            kind: TraceWriteKind::Bus,
        });
    }

    fn cpu_boundary(&mut self, outcome: TimedOutcome) {
        self.outcome = Some(outcome);
    }
}

fn registers(registers: &Registers) -> [u32; 19] {
    let mut values = [0; 19];
    values[..8].copy_from_slice(&registers.d);
    for index in 0..7 {
        values[8 + index] = registers.address_register(index).unwrap();
    }
    values[15] = registers.usp;
    values[16] = registers.ssp;
    values[17] = registers.pc;
    values[18] = u32::from(registers.status_register());
    values
}

impl Capture<'_> {
    fn finish(
        self,
        before: &Registers,
        after: &Registers,
        clocks: u64,
        machine_fault: bool,
        dma_activity: bool,
    ) -> Option<u64> {
        let storage_offset = self
            .first_input
            .filter(|(address, _)| *address == before.pc)
            .and_then(|(_, input)| input.origin[0].storage_offset());
        if self.outcome == Some(TimedOutcome::Stopped) && !machine_fault && !dma_activity {
            return storage_offset;
        }
        let Self {
            debugger,
            record,
            outcome,
            ..
        } = self;
        let Some(mut record) = record else {
            return storage_offset;
        };
        record.physical_rom_offset = storage_offset;
        record.event = outcome
            .and_then(debug_event)
            .or(dma_activity.then_some(DebugEvent::Dma));
        let (kind, final_exception) = match outcome {
            Some(TimedOutcome::Instruction(step)) => {
                (TraceBoundaryKind::Instruction, step.exception)
            }
            Some(TimedOutcome::Interrupt { vector, .. }) => {
                (TraceBoundaryKind::Interrupt, Some(vector))
            }
            Some(TimedOutcome::Trace { .. }) => (TraceBoundaryKind::Trace, Some(9)),
            Some(TimedOutcome::Stopped) => (TraceBoundaryKind::Stopped, None),
            None => (TraceBoundaryKind::Fault, None),
        };
        record.boundary = Some(TraceBoundary {
            kind,
            cpu_clocks: clocks,
            final_exception,
            machine_fault,
        });
        for (register, (before, after)) in registers(before)
            .into_iter()
            .zip(registers(after))
            .enumerate()
        {
            if before != after {
                record.push_register_delta(RegisterDelta {
                    register: register as u8,
                    value: after,
                });
            }
        }
        debugger.trace.push(record);
        storage_offset
    }
}

impl MdBackend {
    pub(in crate::emu_backend::md) fn step_captured_instruction(
        &mut self,
    ) -> (Result<MachineStep, MdExecutionError>, Option<u64>) {
        if let Err(error) = self.require_normal() {
            return (Err(error.into()), None);
        }
        let before = self.machine.core().registers();
        let dma_before = self.machine.core().debug_dma_activity_epoch();
        let clock = self.machine.core().cpu_clock().0;
        let mut capture = Capture {
            record: self.debugger.trace.is_enabled().then(|| {
                InstructionTraceRecord::new(
                    TraceExecMode::M68000,
                    before.pc,
                    None,
                    self.machine.core().pacing_field_count(),
                    clock,
                    &[],
                )
            }),
            first_input: None,
            debugger: &mut self.debugger,
            outcome: None,
        };
        let result = match self.machine.normal_mut() {
            Ok(core) => match self.sound_trace.attachment() {
                Some(mut sound) => core.step_instruction_observers(&mut capture, &mut sound),
                None => core.step_instruction_traced(&mut capture),
            }
            .map_err(Into::into),
            Err(error) => return (Err(error.into()), None),
        };
        let after = self.machine.core().registers();
        let storage_offset = capture.finish(
            &before,
            &after,
            self.machine.core().cpu_clock().0.saturating_sub(clock),
            result.is_err(),
            dma_before != self.machine.core().debug_dma_activity_epoch(),
        );
        (result, storage_offset)
    }

    pub(crate) fn instruction_trace(&self) -> &zeff_emu_common::debug::InstructionTraceStore {
        &self.debugger.trace
    }
}

#[cfg(test)]
mod tests;
