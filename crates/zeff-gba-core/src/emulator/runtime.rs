use crate::emulator::Emulator;
use crate::hardware::bus::DebugTraceEvent;
use crate::hardware::constants::CYCLES_PER_FRAME;
use crate::hardware::cpu::{CpuState, FetchedInstruction};
use zeff_emu_common::debug::{
    DebugEvent, InstructionTraceRecord, RegisterDelta, TraceExecMode, TraceWrite, TraceWriteKind,
    TraceWriteWidth,
};

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
        self.step_instruction_inner(false, false, false).0
    }

    pub fn step_instruction_with_bus_trace(
        &mut self,
        trace_reads: bool,
        trace_writes: bool,
    ) -> (Option<FetchedInstruction>, Vec<DebugTraceEvent>) {
        self.step_instruction_inner(trace_reads || trace_writes, trace_reads, trace_writes)
    }

    fn step_instruction_inner(
        &mut self,
        collect_bus_trace: bool,
        trace_reads: bool,
        trace_writes: bool,
    ) -> (Option<FetchedInstruction>, Vec<DebugTraceEvent>) {
        if self.cpu.is_suspended() {
            return (None, Vec::new());
        }
        let trace_enabled = self.instruction_trace.is_enabled();
        let trace_before = if trace_enabled {
            Some((self.cpu.regs, self.cpu.cpsr))
        } else {
            None
        };
        let interrupt_pc = self.cpu.pc();
        let interrupt_cycle = self.cpu.cycles;
        let mut interrupt_serviced = false;
        if self.bus.interrupt_ready() && self.bus.irq_handler_installed() {
            let irq_delay_cycles = self.bus.take_irq_sample_delay_cycles();
            if irq_delay_cycles != 0 {
                self.cpu.cycles = self.cpu.cycles.wrapping_add(u64::from(irq_delay_cycles));
                self.bus.step_cycles(irq_delay_cycles);
            }
            interrupt_serviced = self.cpu.try_service_irq(true);
        }
        if interrupt_serviced && trace_enabled {
            let mut record = InstructionTraceRecord::new(
                if self.cpu.thumb_state() {
                    TraceExecMode::Thumb
                } else {
                    TraceExecMode::Arm
                },
                interrupt_pc,
                self.gba_rom_offset(interrupt_pc),
                self.frame_count,
                interrupt_cycle,
                &[],
            );
            record.event = Some(DebugEvent::Interrupt);
            push_gba_register_deltas(
                &mut record,
                trace_before.expect("trace state"),
                (self.cpu.regs, self.cpu.cpsr),
            );
            self.instruction_trace.push(record);
        }
        if interrupt_serviced
            && self
                .debug
                .check_event(zeff_emu_common::debug::DebugEvent::Interrupt)
        {
            self.cpu.suspend();
            return (None, Vec::new());
        }
        if self.debug.should_break(self.cpu.pc()) {
            self.cpu.suspend();
            return (None, Vec::new());
        }
        if self.cpu.state == CpuState::Halted {
            let cycles = self.bus.cycles_until_next_halt_check();
            self.cpu.cycles = self.cpu.cycles.wrapping_add(u64::from(cycles));
            self.bus.step_cycles(cycles);
            if self.bus.interrupt_ready() {
                self.cpu.resume();
            }
            return (None, Vec::new());
        }
        let instruction_before = if trace_enabled {
            Some((self.cpu.regs, self.cpu.cpsr))
        } else {
            None
        };

        let collect_bus_trace = collect_bus_trace || trace_enabled;
        self.bus.debug_trace_enabled = collect_bus_trace;
        self.bus.debug_trace_reads = trace_reads;
        self.bus.debug_trace_writes = trace_writes || trace_enabled;
        if collect_bus_trace {
            self.bus.debug_trace_events.borrow_mut().clear();
        }

        let before_cycles = self.cpu.cycles;
        let fetched = self.cpu.step(&mut self.bus);
        if let Some(instruction) = fetched {
            self.opcode_log.push(instruction.into());
        }
        let elapsed = self
            .cpu
            .cycles
            .wrapping_sub(before_cycles)
            .min(u64::from(u32::MAX));
        let dma_cycles = self.bus.take_pending_dma_cycles();
        self.cpu.cycles = self.cpu.cycles.wrapping_add(u64::from(dma_cycles));
        self.bus.timers.begin_step_window(elapsed as u32);
        self.bus
            .step_cycles((elapsed as u32).saturating_add(dma_cycles));
        if dma_cycles != 0
            && self
                .debug
                .check_event(zeff_emu_common::debug::DebugEvent::Dma)
        {
            self.cpu.suspend();
        }

        let mut bus_trace_events = if collect_bus_trace {
            self.bus.debug_trace_enabled = false;
            self.bus.debug_trace_reads = false;
            self.bus.debug_trace_writes = false;
            std::mem::take(&mut *self.bus.debug_trace_events.borrow_mut())
        } else {
            Vec::new()
        };

        if trace_enabled && let Some(instruction) = fetched {
            let width = usize::from(instruction.width_bytes);
            let bytes = instruction.raw.to_le_bytes();
            let mut record = InstructionTraceRecord::new(
                match instruction.instruction_set {
                    crate::hardware::cpu::InstructionSet::Arm => TraceExecMode::Arm,
                    crate::hardware::cpu::InstructionSet::Thumb => TraceExecMode::Thumb,
                },
                instruction.pc,
                self.gba_rom_offset(instruction.pc),
                self.frame_count,
                before_cycles,
                &bytes[..width],
            );
            if dma_cycles != 0 {
                record.event = Some(DebugEvent::Dma);
            }
            push_gba_register_deltas(
                &mut record,
                instruction_before.expect("trace state"),
                (self.cpu.regs, self.cpu.cpsr),
            );
            for event in &bus_trace_events {
                if let DebugTraceEvent::Write {
                    addr,
                    old_value,
                    new_value,
                    width,
                } = *event
                {
                    record.push_write(TraceWrite {
                        address: addr,
                        old_value,
                        new_value,
                        width: trace_width(width),
                        kind: TraceWriteKind::Memory,
                    });
                }
            }
            self.instruction_trace.push(record);
        }

        if trace_enabled && !trace_reads && !trace_writes {
            bus_trace_events.clear();
            *self.bus.debug_trace_events.borrow_mut() = bus_trace_events;
            return (fetched, Vec::new());
        }

        (fetched, bus_trace_events)
    }

    pub fn finish_frame(&mut self) {
        if !self.bus.ppu.frame_ready {
            self.bus.render_frame();
        }
        self.frame_count = self.frame_count.wrapping_add(1);
    }

    fn gba_rom_offset(&self, address: u32) -> Option<u64> {
        self.rom_offset_for_cpu_address(address)
            .map(|offset| offset as u64)
    }
}

fn push_gba_register_deltas(
    record: &mut InstructionTraceRecord,
    before: ([u32; 16], u32),
    after: ([u32; 16], u32),
) {
    for (register, (&before, &after)) in before.0.iter().zip(&after.0).enumerate() {
        if before != after {
            record.push_register_delta(RegisterDelta {
                register: register as u8,
                value: after,
            });
        }
    }
    if before.1 != after.1 {
        record.push_register_delta(RegisterDelta {
            register: 16,
            value: after.1,
        });
    }
}

fn trace_width(width: u8) -> TraceWriteWidth {
    match width {
        2 => TraceWriteWidth::Halfword,
        4 => TraceWriteWidth::Word,
        _ => TraceWriteWidth::Byte,
    }
}
