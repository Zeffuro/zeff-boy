use super::{CPU_CYCLES_PER_FRAME, Emulator};
use crate::hardware::bus::DebugTraceEvent;
use crate::hardware::cpu::CpuState;

impl Emulator {
    pub fn step_instruction(&mut self) -> (u16, u8, u64) {
        if self.cpu.state == CpuState::Suspended {
            return (self.cpu.pc, self.bus.cpu_read(self.cpu.pc), 0);
        }

        let watch_active = self.debug.has_watchpoints();
        self.bus.debug_trace_enabled = watch_active;
        if watch_active {
            self.bus.debug_trace_events.clear();
        }

        let pc_before = self.cpu.pc;
        let opcode = self.bus.cpu_read(pc_before);

        self.opcode_log.push((pc_before, opcode));

        self.bus.cpu_odd_cycle = self.cpu.cycles % 2 == 1;

        let cycles = self.cpu.step(&mut self.bus);

        let dma_cycles = self.bus.dma_stall_cycles;
        self.bus.dma_stall_cycles = 0;
        let total_cycles = cycles + dma_cycles;
        self.cpu.cycles += dma_cycles;

        let nmi = self.bus.tick_peripherals(total_cycles);

        self.cpu.irq_line = self.bus.apu.irq_pending() || self.bus.cartridge.irq_pending();

        if nmi {
            self.cpu.nmi_pending = true;
        }

        if watch_active {
            self.bus.debug_trace_enabled = false;
            let debug = &mut self.debug;
            for event in self.bus.debug_trace_events.drain(..) {
                match event {
                    DebugTraceEvent::Read { addr, value } => {
                        debug.check_watch_read(addr, value);
                    }
                    DebugTraceEvent::Write {
                        addr,
                        old_value,
                        new_value,
                    } => {
                        debug.check_watch_write(addr, old_value, new_value);
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

        (pc_before, opcode, total_cycles)
    }

    pub fn step_frame(&mut self) {
        if self.cpu.state == CpuState::Suspended {
            return;
        }

        let target = self.cpu.cycles.wrapping_add(CPU_CYCLES_PER_FRAME);

        if self.debug.any_active() || self.opcode_log.enabled {
            while self.cpu.cycles < target && self.cpu.state == CpuState::Running {
                self.step_instruction();
            }
        } else {
            while self.cpu.cycles < target {

                self.bus.cpu_odd_cycle = self.cpu.cycles % 2 == 1;
                let cycles = self.cpu.step(&mut self.bus);

                let dma_cycles = self.bus.dma_stall_cycles;
                self.bus.dma_stall_cycles = 0;
                let total_cycles = cycles + dma_cycles;
                self.cpu.cycles += dma_cycles;

                let nmi = self.bus.tick_peripherals(total_cycles);
                self.cpu.irq_line = self.bus.apu.irq_pending() || self.bus.cartridge.irq_pending();

                if nmi {
                    self.cpu.nmi_pending = true;
                }
            }
        }
    }
}
