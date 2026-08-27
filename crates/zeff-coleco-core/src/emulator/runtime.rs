use zeff_emu_common::address::Address;
use zeff_emu_common::debug::{
    BusAccessEvent, DebugEvent, InstructionTraceRecord, RegisterDelta, TraceExecMode, TraceWrite,
    TraceWriteKind, TraceWriteWidth,
};
use zeff_z80::FetchedInstruction;

use super::Emulator;

impl Emulator {
    pub fn step_instruction(&mut self) -> Option<FetchedInstruction> {
        if self.debug_hooks_active {
            self.step_instruction_inner(false, false).0
        } else {
            self.step_instruction_fast()
        }
    }

    pub fn step_instruction_with_bus_trace(
        &mut self,
    ) -> (Option<FetchedInstruction>, Vec<BusAccessEvent>) {
        self.step_instruction_inner(false, true)
    }

    pub(crate) fn step_instruction_inner(
        &mut self,
        skip_breakpoint_check: bool,
        collect_bus_trace: bool,
    ) -> (Option<FetchedInstruction>, Vec<BusAccessEvent>) {
        if self.cpu.is_suspended() {
            return (None, Vec::new());
        }

        let pc = Address::from(self.cpu.regs().pc);
        if !skip_breakpoint_check && self.debug.should_break(pc) {
            self.cpu.suspend();
            return (None, Vec::new());
        }

        let watch_active = !self.debug.watchpoints.is_empty();
        let trace_enabled = self.instruction_trace.is_enabled();
        let trace_active = watch_active || collect_bus_trace || trace_enabled;
        if trace_active {
            if watch_active || collect_bus_trace {
                self.bus.begin_cpu_access_trace();
            } else {
                self.bus.begin_cpu_write_trace();
            }
        }

        let pc_before = self.cpu.regs().pc;
        let cycles_before = self.effective_cycles;
        let registers_before = trace_enabled.then(|| z80_registers(self.cpu.regs()));
        let rom_offset = trace_enabled
            .then(|| self.bus.rom_offset_for_cpu_address(pc_before))
            .flatten();
        let instruction = if trace_active {
            self.cpu.step_with_bus(&mut self.bus.tracing())
        } else {
            self.cpu.step_with_bus(&mut self.bus)
        };

        if let Some(instruction) = instruction {
            self.advance_executed_instruction(instruction);
            self.opcode_log
                .push((instruction.pc, instruction.opcode, instruction.cycles));
        }

        if self.cpu.last_step_was_interrupt() && self.debug.check_event(DebugEvent::Interrupt) {
            self.cpu.suspend();
        }

        let mut bus_trace = Vec::new();
        let mut trace_record = trace_enabled.then(|| {
            let interrupt = self.cpu.last_step_was_interrupt();
            let bytes = if interrupt {
                &[]
            } else {
                self.cpu.instruction_bytes()
            };
            let mut record = InstructionTraceRecord::new(
                TraceExecMode::Z80,
                u32::from(pc_before),
                rom_offset.map(|offset| offset as u64),
                self.frame_count(),
                cycles_before,
                bytes,
            );
            if interrupt {
                record.event = Some(DebugEvent::Interrupt);
            }
            record
        });

        if trace_active {
            let events = self.bus.drain_cpu_access_trace();
            if let Some(record) = &mut trace_record {
                append_writes(record, &events);
            }
            if watch_active {
                self.apply_watchpoints(&events);
            }
            if collect_bus_trace {
                bus_trace = events;
            } else {
                self.bus.recycle_cpu_access_trace(events);
            }
            if self.debug.hit_watchpoint.is_some() {
                self.cpu.suspend();
            }
        }

        if let Some(mut record) = trace_record {
            push_register_deltas(
                &mut record,
                &registers_before.expect("trace state"),
                &z80_registers(self.cpu.regs()),
            );
            self.instruction_trace.push(record);
        }

        (instruction, bus_trace)
    }

    pub fn step_frame(&mut self) {
        if self.cpu.is_suspended() {
            return;
        }
        let start = self.frame_count();
        if !self.debug_hooks_active {
            while self.frame_count() == start {
                if self.step_instruction_fast().is_none() {
                    break;
                }
            }
            return;
        }
        while self.frame_count() == start {
            if self.step_instruction().is_none() || self.cpu.is_suspended() {
                break;
            }
        }
    }

    #[inline]
    fn step_instruction_fast(&mut self) -> Option<FetchedInstruction> {
        let instruction = self.cpu.step_with_bus(&mut self.bus);
        if let Some(instruction) = instruction {
            self.advance_executed_instruction(instruction);
        }
        instruction
    }

    #[inline]
    fn advance_executed_instruction(&mut self, instruction: FetchedInstruction) {
        let m1_waits = u32::from(self.cpu.last_m1_fetch_count());
        let Some(cycle) = self.bus.take_pending_psg_write_cycle() else {
            self.advance_machine(instruction.cycles.saturating_add(m1_waits));
            return;
        };

        let cycle_end = cycle.t_states_before.saturating_add(cycle.t_states);
        let after = instruction
            .cycles
            .checked_sub(cycle_end)
            .expect("Z80 I/O cycle exceeds instruction timing");
        self.advance_machine(cycle.t_states_before.saturating_add(m1_waits));
        self.bus.psg_mut().begin_write();
        self.advance_machine(cycle.t_states);
        self.advance_machine(u32::from(self.bus.psg().ready_clocks_remaining()));
        self.bus.psg_mut().complete_write(cycle.value);
        self.advance_machine(after);
    }

    fn apply_watchpoints(&mut self, events: &[BusAccessEvent]) {
        for &event in events {
            match event {
                BusAccessEvent::Read {
                    space: TraceWriteKind::Memory,
                    addr,
                    value,
                    ..
                } => {
                    let (Ok(addr), Ok(value)) = (u16::try_from(addr), u8::try_from(value)) else {
                        continue;
                    };
                    self.debug.check_watch_read(Address::from(addr), value);
                }
                BusAccessEvent::Write {
                    space: TraceWriteKind::Memory,
                    addr,
                    old_value,
                    new_value,
                    ..
                } => {
                    let (Ok(addr), Ok(old_value), Ok(new_value)) = (
                        u16::try_from(addr),
                        u8::try_from(old_value),
                        u8::try_from(new_value),
                    ) else {
                        continue;
                    };
                    self.debug
                        .check_watch_write(Address::from(addr), old_value, new_value);
                }
                BusAccessEvent::Read { .. } | BusAccessEvent::Write { .. } => {}
            }
            if self.debug.hit_watchpoint.is_some() {
                break;
            }
        }
    }

    fn advance_machine(&mut self, cycles: u32) {
        self.bus.step_cycles(cycles);
        self.effective_cycles = self.effective_cycles.wrapping_add(u64::from(cycles));
    }
}

fn z80_registers(regs: zeff_z80::Registers) -> [u32; 14] {
    [
        u32::from(regs.a),
        u32::from(regs.f),
        u32::from(regs.b),
        u32::from(regs.c),
        u32::from(regs.d),
        u32::from(regs.e),
        u32::from(regs.h),
        u32::from(regs.l),
        u32::from(regs.ix),
        u32::from(regs.iy),
        u32::from(regs.sp),
        u32::from(regs.pc),
        u32::from(regs.i),
        u32::from(regs.r),
    ]
}

fn push_register_deltas(
    record: &mut InstructionTraceRecord,
    before: &[u32; 14],
    after: &[u32; 14],
) {
    for (register, (&before, &after)) in before.iter().zip(after).enumerate() {
        if before != after {
            record.push_register_delta(RegisterDelta {
                register: register as u8,
                value: after,
            });
        }
    }
}

fn append_writes(record: &mut InstructionTraceRecord, events: &[BusAccessEvent]) {
    for event in events {
        let write = match *event {
            BusAccessEvent::Write {
                addr,
                old_value,
                new_value,
                width: TraceWriteWidth::Byte,
                space,
                ..
            } => TraceWrite {
                address: addr,
                old_value,
                new_value,
                width: TraceWriteWidth::Byte,
                kind: space,
            },
            BusAccessEvent::Read { .. } | BusAccessEvent::Write { .. } => continue,
        };
        record.push_write(write);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::BIOS_SIZE;

    fn emulator(program: &[u8]) -> Emulator {
        let mut bios = vec![0; BIOS_SIZE];
        bios[..program.len()].copy_from_slice(program);
        let mut cartridge = vec![0; 8 * 1024];
        cartridge[..2].copy_from_slice(&[0xAA, 0x55]);
        Emulator::new(&cartridge, &bios, 48_000).unwrap()
    }

    #[test]
    fn bus_trace_records_memory_and_io_writes() {
        let mut memory = emulator(&[0x3E, 0x5A, 0x32, 0x00, 0x60]);
        memory.step_instruction();
        let (_, trace) = memory.step_instruction_with_bus_trace();
        assert!(trace.iter().any(|event| matches!(
            event,
            BusAccessEvent::Write {
                space: TraceWriteKind::Memory,
                addr: 0x6000,
                new_value: 0x5A,
                ..
            }
        )));

        let mut io = emulator(&[0x3E, 0x9F, 0xD3, 0xE0]);
        io.step_instruction();
        let (_, trace) = io.step_instruction_with_bus_trace();
        assert!(trace.iter().any(|event| matches!(
            event,
            BusAccessEvent::Write {
                space: TraceWriteKind::Io,
                addr: 0xE0,
                new_value: 0x9F,
                ..
            }
        )));
    }

    #[test]
    fn dormant_debug_hooks_match_instrumented_execution() {
        let program = [0x3E, 0x5A, 0x32, 0x00, 0x60, 0xD3, 0xE0, 0xC3, 0x00, 0x00];
        let mut fast = emulator(&program);
        let mut traced = emulator(&program);
        fast.set_audio_generation_enabled(false);
        traced.set_audio_generation_enabled(false);
        traced.set_instruction_trace_enabled(true);

        fast.step_frame();
        traced.step_frame();

        assert_eq!(fast.save_state().unwrap(), traced.save_state().unwrap());
        assert!(!traced.instruction_trace().is_empty());
    }
}
