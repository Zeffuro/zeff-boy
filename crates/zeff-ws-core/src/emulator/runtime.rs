use crate::emulator::Emulator;
use crate::hardware::bus::{DebugTraceEvent, DebugTraceMode};
use crate::hardware::constants::CYCLES_PER_FRAME;
use crate::hardware::cpu::FetchedInstruction;
use zeff_emu_common::address::Address;
use zeff_emu_common::debug::{
    DebugEvent, InstructionTraceRecord, RegisterDelta, TraceExecMode, TraceWrite, TraceWriteKind,
    TraceWriteWidth,
};

impl Emulator {
    pub fn step_frame(&mut self) {
        self.step_frame_inner(true);
    }

    #[cfg(test)]
    pub(crate) fn eager_hlt_step_frame(&mut self) {
        self.step_frame_inner(false);
    }

    fn step_frame_inner(&mut self, allow_hlt_fast_forward: bool) {
        if self.cpu.is_suspended() {
            return;
        }
        let hlt_fast_forward_enabled =
            allow_hlt_fast_forward && self.hlt_fast_forward_observers_inactive();
        self.clear_frame_ready();
        let guard = self
            .cpu
            .cycles
            .wrapping_add(u64::from(CYCLES_PER_FRAME) * 2);
        while !self.frame_ready() && self.cpu.cycles < guard {
            if self.step_instruction().is_none()
                && self.step_frame_after_empty_instruction(hlt_fast_forward_enabled, guard)
            {
                break;
            }
        }
        self.finish_frame();
    }

    #[inline(never)]
    fn step_frame_after_empty_instruction(
        &mut self,
        hlt_fast_forward_enabled: bool,
        guard: u64,
    ) -> bool {
        if self.cpu.is_suspended() {
            return true;
        }
        if hlt_fast_forward_enabled
            && self.cpu.state == crate::hardware::cpu::CpuState::Halted
            && !self.frame_ready()
            && self.cpu.cycles < guard
            && self.cpu.can_fast_forward_halt()
            && !self.bus.has_pending_interrupt_signal()
        {
            let advance = u64::from(self.bus.halted_cpu_next_event_cycles())
                .min(guard.wrapping_sub(self.cpu.cycles)) as u32;
            if advance != 0 {
                self.cpu.advance_halted_cycles(&mut self.bus, advance);
                #[cfg(test)]
                {
                    self.hlt_fast_forward_calls = self.hlt_fast_forward_calls.wrapping_add(1);
                }
            }
        }

        false
    }

    fn hlt_fast_forward_observers_inactive(&self) -> bool {
        !self.debug.break_on_next
            && self.debug.iter_breakpoints().next().is_none()
            && self.debug.watchpoints.is_empty()
            && self.debug.iter_event_breakpoints().next().is_none()
            && !self.opcode_log.enabled
            && !self.instruction_trace.is_enabled()
            && self.bus.debug_trace_mode == DebugTraceMode::None
    }

    pub fn step_instruction(&mut self) -> Option<FetchedInstruction> {
        self.step_instruction_inner(false, DebugTraceMode::None).0
    }

    pub fn step_instruction_with_bus_trace(
        &mut self,
    ) -> (Option<FetchedInstruction>, Vec<DebugTraceEvent>) {
        self.step_instruction_inner(false, DebugTraceMode::MemoryAndIo)
    }

    pub fn step_instruction_with_io_trace(
        &mut self,
    ) -> (Option<FetchedInstruction>, Vec<DebugTraceEvent>) {
        self.step_instruction_inner(false, DebugTraceMode::IoOnly)
    }

    pub(crate) fn step_instruction_inner(
        &mut self,
        skip_breakpoint_check: bool,
        requested_trace_mode: DebugTraceMode,
    ) -> (Option<FetchedInstruction>, Vec<DebugTraceEvent>) {
        if self.cpu.is_suspended() {
            return (None, Vec::new());
        }

        let pc = Address::from(self.cpu.pc());
        if !skip_breakpoint_check && self.debug.should_break(pc) {
            self.cpu.suspend();
            return (None, Vec::new());
        }

        let watch_active = !self.debug.watchpoints.is_empty();
        let instruction_trace_enabled = self.instruction_trace.is_enabled();
        let trace_mode = if watch_active
            || (instruction_trace_enabled && requested_trace_mode != DebugTraceMode::None)
        {
            DebugTraceMode::MemoryAndIo
        } else if instruction_trace_enabled {
            DebugTraceMode::WritesOnly
        } else {
            requested_trace_mode
        };
        let trace_active = trace_mode != DebugTraceMode::None;
        self.bus.debug_trace_mode = trace_mode;
        if trace_active {
            self.bus.debug_trace_events.clear();
        }

        let pc_before = self.cpu.pc();
        let cycles_before = self.cpu.cycles;
        let registers_before = if instruction_trace_enabled {
            Some(ws_registers(&self.cpu))
        } else {
            None
        };
        let physical_rom_offset = if instruction_trace_enabled {
            self.bus.cartridge.rom_offset_for_address(pc_before)
        } else {
            None
        };

        let fetched = self.cpu.step(&mut self.bus);
        if self.cpu.last_step_was_interrupt
            && self
                .debug
                .check_event(zeff_emu_common::debug::DebugEvent::Interrupt)
        {
            self.cpu.suspend();
        }
        if fetched.is_some() {
            self.bus.retire_instruction();
        }
        if let Some(instruction) = fetched {
            self.opcode_log.push(instruction.into());
        }
        let mut events = if trace_active {
            self.bus.debug_trace_mode = DebugTraceMode::None;
            self.bus.take_debug_trace_events()
        } else {
            Vec::new()
        };

        if instruction_trace_enabled {
            let interrupt = self.cpu.last_step_was_interrupt;
            let mut record = InstructionTraceRecord::new(
                TraceExecMode::V30,
                pc_before,
                physical_rom_offset.map(|offset| offset as u64),
                self.frame_count,
                cycles_before,
                if interrupt {
                    &[]
                } else {
                    self.cpu.instruction_bytes()
                },
            );
            if interrupt {
                record.event = Some(DebugEvent::Interrupt);
            }
            append_ws_writes(&mut record, &events);
            push_ws_register_deltas(
                &mut record,
                &registers_before.expect("trace state"),
                &ws_registers(&self.cpu),
            );
            self.instruction_trace.push(record);
        }

        if watch_active {
            for event in &events {
                match *event {
                    DebugTraceEvent::Read {
                        space: TraceWriteKind::Memory,
                        addr,
                        value,
                        width: TraceWriteWidth::Byte,
                        ..
                    } => {
                        if let Ok(value) = u8::try_from(value) {
                            self.debug.check_watch_read(Address::from(addr), value);
                        }
                    }
                    DebugTraceEvent::Write {
                        space: TraceWriteKind::Memory,
                        addr,
                        old_value,
                        new_value,
                        width: TraceWriteWidth::Byte,
                        ..
                    } => {
                        if let (Ok(old_value), Ok(new_value)) =
                            (u8::try_from(old_value), u8::try_from(new_value))
                        {
                            self.debug
                                .check_watch_write(Address::from(addr), old_value, new_value);
                        }
                    }
                    _ => {}
                }
            }
            if self.debug.hit_watchpoint.is_some() {
                self.cpu.suspend();
            }
        }

        let bus_trace_events = if requested_trace_mode != DebugTraceMode::None {
            events
        } else {
            events.clear();
            self.bus.debug_trace_events = events;
            Vec::new()
        };
        (fetched, bus_trace_events)
    }

    pub fn finish_frame(&mut self) {
        if !self.bus.ppu.frame_ready {
            self.bus.render_frame();
        }
        self.frame_count = self.frame_count.wrapping_add(1);
    }
}

fn ws_registers(cpu: &crate::hardware::cpu::Cpu) -> [u32; 14] {
    [
        u32::from(cpu.regs[0]),
        u32::from(cpu.regs[1]),
        u32::from(cpu.regs[2]),
        u32::from(cpu.regs[3]),
        u32::from(cpu.regs[4]),
        u32::from(cpu.regs[5]),
        u32::from(cpu.regs[6]),
        u32::from(cpu.regs[7]),
        u32::from(cpu.segments[0]),
        u32::from(cpu.segments[1]),
        u32::from(cpu.segments[2]),
        u32::from(cpu.segments[3]),
        u32::from(cpu.ip),
        u32::from(cpu.flags),
    ]
}

fn push_ws_register_deltas(
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

fn append_ws_writes(record: &mut InstructionTraceRecord, events: &[DebugTraceEvent]) {
    for event in events {
        let write = match *event {
            DebugTraceEvent::Write {
                space,
                addr,
                old_value,
                new_value,
                width,
                ..
            } => TraceWrite {
                address: addr,
                old_value,
                new_value,
                width,
                kind: space,
            },
            DebugTraceEvent::Read { .. } => continue,
        };
        record.push_write(write);
    }
}
