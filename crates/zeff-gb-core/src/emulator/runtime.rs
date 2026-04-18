use super::{CYCLES_PER_FRAME_DOUBLE, CYCLES_PER_FRAME_NORMAL, Emulator};
use crate::hardware::bus::CpuAccessTraceEvent;
use crate::hardware::types::CpuState;
use crate::hardware::types::hardware_mode::HardwareMode;

impl Emulator {
    pub fn cycles_per_frame(mode: HardwareMode) -> u64 {
        if mode == HardwareMode::CGBDouble {
            CYCLES_PER_FRAME_DOUBLE
        } else {
            CYCLES_PER_FRAME_NORMAL
        }
    }

    pub fn step_instruction(&mut self) -> (u16, u8, bool, u64) {
        if matches!(self.cpu.running, CpuState::Suspended) {
            return (self.cpu.pc, self.bus.read_byte(self.cpu.pc), false, 0);
        }

        let watch_active = self.debug.has_watchpoints();
        self.bus.trace_cpu_accesses = watch_active;
        if watch_active {
            self.bus.begin_cpu_access_trace();
        }

        let pc_before = self.cpu.pc;
        let opcode_at_pc = self.bus.read_byte(pc_before);

        self.cpu.step(&mut self.bus);

        self.hardware_mode = self.bus.hardware_mode;

        if watch_active {
            let hit_watchpoint = {
                let debug = &mut self.debug;
                self.bus.drain_cpu_access_trace(|event| match event {
                    CpuAccessTraceEvent::Read { addr, value } => {
                        debug.check_watch_read(addr, value)
                    }
                    CpuAccessTraceEvent::Write {
                        addr,
                        old_value,
                        new_value,
                    } => debug.check_watch_write(addr, old_value, new_value),
                });
                debug.hit_watchpoint.is_some()
            };

            if hit_watchpoint {
                self.cpu.running = CpuState::Suspended;
            }
        }

        if self.debug.should_break(pc_before) {
            self.cpu.running = CpuState::Suspended;
        }

        let (op, cb_prefix) = if opcode_at_pc == 0xCB {
            (self.bus.read_byte(pc_before.wrapping_add(1)), true)
        } else {
            (opcode_at_pc, false)
        };

        self.opcode_log.push((pc_before, op, cb_prefix));
        self.last_opcode = op;
        self.last_opcode_pc = pc_before;

        debug_assert_eq!(
            self.cpu.timed_cycles_accounted, self.cpu.last_step_cycles,
            "peripheral timing is expected to be fully Cpu-driven (pc={:#06X}, opcode={:#04X}, cb_prefix={})",
            pc_before, opcode_at_pc, cb_prefix
        );

        (pc_before, op, cb_prefix, self.cpu.last_step_cycles)
    }

    pub fn step_frame(&mut self) {
        if matches!(self.cpu.running, CpuState::Suspended) {
            return;
        }

        let frame_cycles = Self::cycles_per_frame(self.hardware_mode);
        let target = self.cpu.cycles.wrapping_add(frame_cycles);

        if self.debug.any_active() || self.opcode_log.enabled {
            while self.cpu.cycles < target && !matches!(self.cpu.running, CpuState::Suspended) {
                let _ = self.step_instruction();
            }
        } else {
            while self.cpu.cycles < target {
                self.cpu.step(&mut self.bus);
                self.hardware_mode = self.bus.hardware_mode;
            }
        }
    }

    pub fn set_mbc7_host_tilt(&mut self, x: f32, y: f32) {
        self.bus.cartridge.set_mbc7_tilt(x, y);
    }

    pub fn set_camera_host_frame(&mut self, frame: &[u8]) {
        self.bus.cartridge.set_camera_frame(frame);
    }
}
