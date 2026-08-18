use super::{CPU_CYCLES_PER_FRAME, Emulator};
use crate::debug::{CallStackEntry, CallStackKind};
use crate::hardware::bus::DebugTraceEvent;
use crate::hardware::cpu::{CpuState, CpuStepKind};
use zeff_emu_common::cpu::CpuCore;
use zeff_emu_common::debug::{
    DebugEvent, InstructionTraceRecord, RegisterDelta, TraceExecMode, TraceWrite, TraceWriteKind,
    TraceWriteWidth,
};

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
        let instruction_trace_enabled = self.instruction_trace.is_enabled();
        let trace_active = watch_active || collect_bus_trace || instruction_trace_enabled;
        self.bus.debug_trace_enabled = trace_active;
        if trace_active {
            self.bus.debug_trace_reads = watch_active || collect_bus_trace;
            self.bus.debug_trace_events.clear();
        }

        let pc_before = self.cpu.pc;
        let cpu_was_running = self.cpu.state == CpuState::Running;
        let sp_before = self.cpu.sp;
        let cycles_before = self.cpu.cycles;
        let registers_before = if instruction_trace_enabled {
            Some(nes_registers(&self.cpu))
        } else {
            None
        };
        let (opcode, rom_offset) = if cpu_was_running {
            (
                self.bus.cpu_peek(pc_before),
                self.bus.cartridge.cpu_rom_offset(pc_before),
            )
        } else {
            (self.cpu.last_opcode, None)
        };

        if cpu_was_running {
            self.opcode_log.push((pc_before, opcode, rom_offset));
        }

        self.bus.cpu_odd_cycle = self.cpu.cycles % 2 == 1;
        self.bus
            .begin_cpu_step_timing(zeff_emu_common::time::MasterTicks::new(self.cpu.cycles));

        let cycles = CpuCore::step_cpu(&mut self.cpu, &mut self.bus);

        let dma_cycles = self.bus.dma_stall_cycles;
        self.bus.dma_stall_cycles = 0;
        let total_cycles = cycles + dma_cycles;
        self.cpu.cycles += dma_cycles;

        self.tick_peripherals_after_cpu_step(total_cycles);
        self.update_call_stack(pc_before, sp_before, opcode);
        if matches!(self.cpu.last_step_kind, CpuStepKind::Nmi | CpuStepKind::Irq)
            && self
                .debug
                .check_event(zeff_emu_common::debug::DebugEvent::Interrupt)
        {
            self.cpu.state = CpuState::Suspended;
        }
        if dma_cycles != 0
            && self
                .debug
                .check_event(zeff_emu_common::debug::DebugEvent::Dma)
        {
            self.cpu.state = CpuState::Suspended;
        }

        let mut bus_trace_events = Vec::new();
        let mut instruction_record = (instruction_trace_enabled
            && self.cpu.last_step_kind != CpuStepKind::Idle)
            .then(|| {
                let interrupt =
                    matches!(self.cpu.last_step_kind, CpuStepKind::Nmi | CpuStepKind::Irq);
                let mut record = InstructionTraceRecord::new(
                    TraceExecMode::Mos6502,
                    u32::from(pc_before),
                    rom_offset.map(|offset| offset as u64),
                    self.frame_count(),
                    cycles_before,
                    if interrupt {
                        &[]
                    } else {
                        self.cpu.instruction_bytes()
                    },
                );
                record.event = if interrupt {
                    Some(DebugEvent::Interrupt)
                } else if dma_cycles != 0 {
                    Some(DebugEvent::Dma)
                } else {
                    None
                };
                record
            });
        if trace_active {
            self.bus.debug_trace_enabled = false;
            self.bus.debug_trace_reads = true;
            let mut events = std::mem::take(&mut self.bus.debug_trace_events);

            if collect_bus_trace {
                bus_trace_events = events.clone();
            }

            let debug = &mut self.debug;
            if watch_active {
                for event in &events {
                    match *event {
                        DebugTraceEvent::Read { addr, value, .. } => {
                            debug.check_watch_read(addr as u16, value as u8);
                        }
                        DebugTraceEvent::Write {
                            addr,
                            old_value,
                            new_value,
                            ..
                        } => {
                            debug.check_watch_write(addr as u16, old_value as u8, new_value as u8);
                        }
                    }
                }
            }
            if let Some(record) = &mut instruction_record {
                for event in &events {
                    if let DebugTraceEvent::Write {
                        addr,
                        old_value,
                        new_value,
                        ..
                    } = *event
                    {
                        record.push_write(TraceWrite {
                            address: addr,
                            old_value,
                            new_value,
                            width: TraceWriteWidth::Byte,
                            kind: TraceWriteKind::Memory,
                        });
                    }
                }
            }
            events.clear();
            self.bus.debug_trace_events = events;
            if debug.hit_watchpoint.is_some() {
                self.cpu.state = CpuState::Suspended;
            }
        }

        if let Some(mut record) = instruction_record {
            push_nes_register_deltas(
                &mut record,
                &registers_before.expect("trace state"),
                &nes_registers(&self.cpu),
            );
            self.instruction_trace.push(record);
        }

        let should_break = if self.cpu.last_step_kind == CpuStepKind::Idle {
            std::mem::take(&mut self.debug.break_on_next)
        } else {
            self.debug.should_break(self.cpu.pc)
        };
        if should_break {
            self.cpu.state = CpuState::Suspended;
        }

        (pc_before, opcode, total_cycles, bus_trace_events)
    }

    fn update_call_stack(&mut self, pc_before: u16, sp_before: u8, opcode: u8) {
        let pc_after = self.cpu.pc;
        let sp_after = self.cpu.sp;
        let frame = match self.cpu.last_step_kind {
            CpuStepKind::Nmi | CpuStepKind::Irq => {
                Some((pc_after, pc_before, CallStackKind::Interrupt))
            }
            CpuStepKind::Instruction if opcode == 0x20 && sp_after == sp_before.wrapping_sub(2) => {
                Some((pc_after, pc_before.wrapping_add(3), CallStackKind::Call))
            }
            CpuStepKind::Instruction if opcode == 0x00 && sp_after == sp_before.wrapping_sub(3) => {
                Some((
                    pc_after,
                    pc_before.wrapping_add(2),
                    CallStackKind::Interrupt,
                ))
            }
            _ => None,
        };

        if let Some((target, return_address, kind)) = frame {
            if self.call_stack.len() == 256 {
                self.call_stack.remove(0);
            }
            self.call_stack.push(CallStackEntry {
                target,
                return_address,
                target_rom_offset: self.bus.cartridge.cpu_rom_offset(target),
                return_rom_offset: self.bus.cartridge.cpu_rom_offset(return_address),
                kind,
            });
            return;
        }

        let returned = self.cpu.last_step_kind == CpuStepKind::Instruction
            && ((opcode == 0x60 && sp_after == sp_before.wrapping_add(2))
                || (opcode == 0x40 && sp_after == sp_before.wrapping_add(3)));
        if returned {
            if let Some(index) = self
                .call_stack
                .iter()
                .rposition(|frame| frame.return_address == pc_after)
            {
                self.call_stack.truncate(index);
            } else {
                self.call_stack.clear();
            }
        }
    }

    pub fn step_frame(&mut self) {
        if self.cpu.state == CpuState::Suspended {
            return;
        }

        self.bus.ppu.frame_ready = false;
        let start_cycles = self.cpu.cycles;
        let max_cycles = CPU_CYCLES_PER_FRAME * 2;

        if self.debug.any_active() || self.opcode_log.enabled || self.instruction_trace.is_enabled()
        {
            while !self.bus.ppu.frame_ready
                && self.cpu.cycles.wrapping_sub(start_cycles) < max_cycles
                && self.cpu.state != CpuState::Suspended
            {
                self.step_instruction();
            }
        } else {
            while !self.bus.ppu.frame_ready
                && self.cpu.cycles.wrapping_sub(start_cycles) < max_cycles
            {
                self.bus.cpu_odd_cycle = self.cpu.cycles % 2 == 1;
                self.bus
                    .begin_cpu_step_timing(zeff_emu_common::time::MasterTicks::new(
                        self.cpu.cycles,
                    ));
                let cycles = CpuCore::step_cpu(&mut self.cpu, &mut self.bus);

                let dma_cycles = self.bus.dma_stall_cycles;
                self.bus.dma_stall_cycles = 0;
                let total_cycles = cycles + dma_cycles;
                self.cpu.cycles += dma_cycles;

                self.tick_peripherals_after_cpu_step(total_cycles);
            }
        }

        self.bus.finish_vs_system_input_frame();
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

fn nes_registers(cpu: &crate::hardware::cpu::Cpu) -> [u32; 6] {
    [
        u32::from(cpu.regs.a),
        u32::from(cpu.regs.x),
        u32::from(cpu.regs.y),
        u32::from(cpu.sp),
        u32::from(cpu.regs.p.bits()),
        u32::from(cpu.pc),
    ]
}

fn push_nes_register_deltas(
    record: &mut InstructionTraceRecord,
    before: &[u32; 6],
    after: &[u32; 6],
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
