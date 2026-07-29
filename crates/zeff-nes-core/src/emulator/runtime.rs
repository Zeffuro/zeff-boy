use super::{CPU_CYCLES_PER_FRAME, Emulator};
use crate::hardware::bus::DebugTraceEvent;
use crate::hardware::cpu::{CpuState, CpuStepKind};

impl Emulator {
    pub fn step_instruction(&mut self) -> (u16, u8, u64) {
        let (pc, opcode, cycles, _) = self.step_instruction_inner(false);
        (pc, opcode, cycles)
    }

    pub fn step_instruction_with_bus_trace(&mut self) -> (u16, u8, u64, Vec<DebugTraceEvent>) {
        self.step_instruction_inner(true)
    }

    fn step_instruction_inner(
        &mut self,
        collect_bus_trace: bool,
    ) -> (u16, u8, u64, Vec<DebugTraceEvent>) {
        if self.cpu.state == CpuState::Suspended {
            return (self.cpu.pc, self.bus.cpu_peek(self.cpu.pc), 0, Vec::new());
        }

        let watch_active = self.debug.has_watchpoints();
        let trace_active = watch_active || collect_bus_trace;
        self.bus.debug_trace_enabled = trace_active;
        if trace_active {
            self.bus.debug_trace_events.clear();
        }

        let pc_before = self.cpu.pc;
        let opcode = self.bus.cpu_peek(pc_before);

        self.opcode_log.push((pc_before, opcode));

        self.bus.cpu_odd_cycle = self.cpu.cycles % 2 == 1;
        self.bus.begin_cpu_step_timing();

        let cycles = self.cpu.step(&mut self.bus);

        let dma_cycles = self.bus.dma_stall_cycles;
        self.bus.dma_stall_cycles = 0;
        let total_cycles = cycles + dma_cycles;
        self.cpu.cycles += dma_cycles;

        self.tick_peripherals_after_cpu_step(total_cycles);

        let mut bus_trace_events = Vec::new();
        if trace_active {
            self.bus.debug_trace_enabled = false;
            let events = std::mem::take(&mut self.bus.debug_trace_events);

            if collect_bus_trace {
                bus_trace_events = events.clone();
            }

            let debug = &mut self.debug;
            if watch_active {
                for event in events {
                    match event {
                        DebugTraceEvent::Read { addr, value, .. } => {
                            debug.check_watch_read(addr, value);
                        }
                        DebugTraceEvent::Write {
                            addr,
                            old_value,
                            new_value,
                            ..
                        } => {
                            debug.check_watch_write(addr, old_value, new_value);
                        }
                    }
                }
            }
            if debug.hit_watchpoint.is_some() {
                self.cpu.state = CpuState::Suspended;
            }
        }

        if self.debug.should_break(self.cpu.pc) {
            self.cpu.state = CpuState::Suspended;
        }

        (pc_before, opcode, total_cycles, bus_trace_events)
    }

    pub fn step_frame(&mut self) {
        if self.cpu.state == CpuState::Suspended {
            return;
        }

        self.bus.ppu.frame_ready = false;
        let start_cycles = self.cpu.cycles;
        let max_cycles = CPU_CYCLES_PER_FRAME * 2;

        if self.debug.any_active() || self.opcode_log.enabled {
            while !self.bus.ppu.frame_ready
                && self.cpu.cycles.wrapping_sub(start_cycles) < max_cycles
                && self.cpu.state == CpuState::Running
            {
                self.step_instruction();
            }
        } else {
            while !self.bus.ppu.frame_ready
                && self.cpu.cycles.wrapping_sub(start_cycles) < max_cycles
            {
                self.bus.cpu_odd_cycle = self.cpu.cycles % 2 == 1;
                self.bus.begin_cpu_step_timing();
                let cycles = self.cpu.step(&mut self.bus);

                let dma_cycles = self.bus.dma_stall_cycles;
                self.bus.dma_stall_cycles = 0;
                let total_cycles = cycles + dma_cycles;
                self.cpu.cycles += dma_cycles;

                self.tick_peripherals_after_cpu_step(total_cycles);
            }
        }
    }

    fn tick_peripherals_after_cpu_step(&mut self, total_cycles: u64) {
        let events = self.bus.finish_cpu_step_timing(total_cycles);
        let final_irq_pending = self.bus.apu.irq_pending() || self.bus.cartridge.irq_pending();
        let nmi_suppressed_by_status_read = self.bus.ppu_nmi_suppressed_by_status_read;
        self.bus.ppu_nmi_suppressed_by_status_read = false;

        let branch_taken_same_page = self.cpu.last_step_branch_taken_same_page;

        if nmi_suppressed_by_status_read {
            self.cpu.nmi_pending = false;
        }

        if events.nmi_raised {
            match self.cpu.last_step_kind {
                CpuStepKind::Instruction if self.cpu.last_opcode == 0x00 => {
                    self.cpu.redirect_to_nmi_vector(&mut self.bus);
                }
                CpuStepKind::Irq => {
                    self.cpu.redirect_to_nmi_vector(&mut self.bus);
                }
                _ => {
                    if !nmi_suppressed_by_status_read {
                        self.cpu.nmi_pending = true;
                        if events.first_nmi_cpu_cycle == Some(0)
                            || nmi_missed_poll(
                                events.first_nmi_cpu_cycle,
                                self.cpu.last_step_cycles,
                            )
                        {
                            self.cpu.delay_nmi_poll_once();
                        }
                    }
                }
            }
        }

        let irq_was_pending = self.cpu.irq_line;
        self.cpu.irq_line = final_irq_pending;

        if !irq_was_pending
            && final_irq_pending
            && irq_missed_poll(
                events.first_irq_cpu_cycle,
                self.cpu.last_step_cycles,
                branch_taken_same_page,
            )
        {
            self.cpu.delay_irq_poll_once();
        }
    }
}

fn nmi_missed_poll(first_interrupt_cpu_cycle: Option<u64>, instruction_cycles: u64) -> bool {
    let Some(first_interrupt_cpu_cycle) = first_interrupt_cpu_cycle else {
        return false;
    };

    let missed_poll_cycle = instruction_cycles.saturating_sub(1);
    missed_poll_cycle != 0 && first_interrupt_cpu_cycle >= missed_poll_cycle
}

fn irq_missed_poll(
    first_interrupt_cpu_cycle: Option<u64>,
    instruction_cycles: u64,
    branch_taken_same_page: bool,
) -> bool {
    if !branch_taken_same_page {
        return false;
    }

    nmi_missed_poll(first_interrupt_cpu_cycle, instruction_cycles)
}
