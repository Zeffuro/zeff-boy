use crate::emulator::Emulator;
use crate::hardware::bus::DebugTraceEvent;
use crate::hardware::constants::CYCLES_PER_FRAME;
use crate::hardware::cpu::{CpuState, FetchedInstruction};

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
        if self.bus.interrupt_ready() && self.bus.irq_handler_installed() {
            let irq_delay_cycles = self.bus.take_irq_sample_delay_cycles();
            if irq_delay_cycles != 0 {
                self.cpu.cycles = self.cpu.cycles.wrapping_add(u64::from(irq_delay_cycles));
                self.bus.step_cycles(irq_delay_cycles);
            }
            self.cpu.try_service_irq(true);
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

        self.bus.debug_trace_enabled = collect_bus_trace;
        self.bus.debug_trace_reads = trace_reads;
        self.bus.debug_trace_writes = trace_writes;
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

        let bus_trace_events = if collect_bus_trace {
            self.bus.debug_trace_enabled = false;
            self.bus.debug_trace_reads = false;
            self.bus.debug_trace_writes = false;
            std::mem::take(&mut *self.bus.debug_trace_events.borrow_mut())
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
