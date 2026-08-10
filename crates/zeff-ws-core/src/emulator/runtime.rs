use crate::emulator::Emulator;
use crate::hardware::bus::DebugTraceEvent;
use crate::hardware::constants::CYCLES_PER_FRAME;
use crate::hardware::cpu::FetchedInstruction;
use zeff_emu_common::address::Address;

impl Emulator {
    pub fn step_frame(&mut self) {
        if self.cpu.is_suspended() {
            return;
        }
        self.clear_frame_ready();
        let guard = self
            .cpu
            .cycles
            .wrapping_add(u64::from(CYCLES_PER_FRAME) * 2);
        while !self.frame_ready() && self.cpu.cycles < guard {
            if self.step_instruction().is_none() && self.cpu.is_suspended() {
                break;
            }
        }
        self.finish_frame();
    }

    pub fn step_instruction(&mut self) -> Option<FetchedInstruction> {
        self.step_instruction_inner(false, false).0
    }

    pub fn step_instruction_with_bus_trace(
        &mut self,
    ) -> (Option<FetchedInstruction>, Vec<DebugTraceEvent>) {
        self.step_instruction_inner(false, true)
    }

    pub(crate) fn step_instruction_inner(
        &mut self,
        skip_breakpoint_check: bool,
        collect_bus_trace: bool,
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
        let trace_active = watch_active || collect_bus_trace;
        self.bus.debug_trace_enabled = trace_active;
        if trace_active {
            self.bus.debug_trace_events.clear();
        }

        let fetched = self.cpu.step(&mut self.bus);
        if fetched.is_some() {
            self.bus.retire_instruction();
        }
        if let Some(instruction) = fetched {
            self.opcode_log.push(instruction.into());
        }
        let events = if trace_active {
            self.bus.debug_trace_enabled = false;
            self.bus.take_debug_trace_events()
        } else {
            Vec::new()
        };

        if watch_active {
            for event in &events {
                match *event {
                    DebugTraceEvent::Read { addr, value } => {
                        self.debug.check_watch_read(Address::from(addr), value);
                    }
                    DebugTraceEvent::Write {
                        addr,
                        old_value,
                        new_value,
                    } => {
                        self.debug
                            .check_watch_write(Address::from(addr), old_value, new_value);
                    }
                    DebugTraceEvent::IoRead { .. } | DebugTraceEvent::IoWrite { .. } => {}
                }
            }
            if self.debug.hit_watchpoint.is_some() {
                self.cpu.suspend();
            }
        }

        let bus_trace_events = if collect_bus_trace {
            events
        } else {
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
