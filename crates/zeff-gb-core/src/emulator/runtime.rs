use super::{CYCLES_PER_FRAME_DOUBLE, CYCLES_PER_FRAME_NORMAL, Emulator};
use crate::debug::{CallStackEntry, CallStackKind};
use crate::hardware::bus::CpuAccessTraceEvent;
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::hardware::types::{CpuState, ImeState};
use zeff_emu_common::debug::{
    DebugEvent, InstructionTraceRecord, RegisterDelta, TraceExecMode, TraceWrite, TraceWriteKind,
    TraceWriteWidth,
};

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
        let trace_enabled = self.instruction_trace.is_enabled();
        let trace_active = watch_active || trace_enabled;
        self.bus.trace_cpu_accesses = watch_active;
        self.bus.trace_cpu_writes = trace_enabled;
        if trace_active {
            self.bus
                .begin_cpu_access_trace_at(zeff_emu_common::time::MasterTicks::new(
                    self.cycle_count,
                ));
        }

        let pc_before = self.cpu.pc;
        let sp_before = self.cpu.sp;
        let cycles_before = self.cpu.cycles;
        let registers_before = if trace_enabled {
            Some(gb_registers(&self.cpu))
        } else {
            None
        };
        let opcode_at_pc = self.bus.read_byte(pc_before);
        let rom_offset = self.bus.cartridge.rom_offset(pc_before);
        let instruction_len = gb_instruction_len(opcode_at_pc);
        let operand1 = self.bus.read_byte(pc_before.wrapping_add(1));
        let operand2 = self.bus.read_byte(pc_before.wrapping_add(2));
        let instruction = [opcode_at_pc, operand1, operand2];
        let call_target = u16::from_le_bytes([operand1, operand2]);
        let interrupt_pending = self.cpu.ime == ImeState::Enabled
            && if self.cpu.running == CpuState::Halted {
                self.bus.pending_interrupts_for_halt() != 0
            } else {
                self.bus.pending_interrupts_for_cpu() != 0
            };

        self.step_cpu();

        self.update_call_stack(
            pc_before,
            sp_before,
            opcode_at_pc,
            call_target,
            interrupt_pending,
        );
        if interrupt_pending
            && self.cpu.sp == sp_before.wrapping_sub(2)
            && self
                .debug
                .check_event(zeff_emu_common::debug::DebugEvent::Interrupt)
        {
            self.cpu.running = CpuState::Suspended;
        }

        let mut trace_record = trace_enabled.then(|| {
            InstructionTraceRecord::new(
                TraceExecMode::Sm83,
                u32::from(pc_before),
                rom_offset.map(|offset| offset as u64),
                self.frame_count,
                cycles_before,
                if interrupt_pending {
                    &[]
                } else {
                    &instruction[..instruction_len]
                },
            )
        });
        if interrupt_pending && let Some(record) = &mut trace_record {
            record.event = Some(DebugEvent::Interrupt);
        }

        if trace_active {
            let hit_watchpoint = {
                let debug = &mut self.debug;
                self.bus.drain_cpu_access_trace(|event| match event {
                    CpuAccessTraceEvent::Read { addr, value, .. } => {
                        if watch_active {
                            debug.check_watch_read(addr as u16, value as u8);
                        }
                    }
                    CpuAccessTraceEvent::Write {
                        addr,
                        old_value,
                        new_value,
                        ..
                    } => {
                        if watch_active {
                            debug.check_watch_write(addr as u16, old_value as u8, new_value as u8);
                        }
                        if let Some(record) = &mut trace_record {
                            record.push_write(TraceWrite {
                                address: addr,
                                old_value,
                                new_value,
                                width: TraceWriteWidth::Byte,
                                kind: TraceWriteKind::Memory,
                            });
                        }
                    }
                });
                debug.hit_watchpoint.is_some()
            };

            if hit_watchpoint {
                self.cpu.running = CpuState::Suspended;
            }
        }

        let hit_rom_breakpoint = if self.rom_breakpoints.is_empty() {
            None
        } else {
            self.bus
                .cartridge
                .rom_offset(pc_before)
                .filter(|offset| self.rom_breakpoints.binary_search(offset).is_ok())
        };
        if let Some(offset) = hit_rom_breakpoint {
            self.hit_rom_breakpoint = Some(offset);
            self.debug.hit_breakpoint = Some(pc_before);
            self.cpu.running = CpuState::Suspended;
        } else if self.debug.should_break(pc_before) {
            self.cpu.running = CpuState::Suspended;
        }

        let (op, cb_prefix) = if opcode_at_pc == 0xCB {
            (self.bus.read_byte(pc_before.wrapping_add(1)), true)
        } else {
            (opcode_at_pc, false)
        };

        self.opcode_log.push((pc_before, op, cb_prefix, rom_offset));
        self.last_opcode = op;
        self.last_opcode_pc = pc_before;

        if let Some(mut record) = trace_record {
            push_register_deltas(
                &mut record,
                &registers_before.expect("trace state"),
                &gb_registers(&self.cpu),
            );
            self.instruction_trace.push(record);
        }

        debug_assert_eq!(
            self.cpu.timed_cycles_accounted, self.cpu.last_step_cycles,
            "peripheral timing is expected to be fully Cpu-driven (pc={:#06X}, opcode={:#04X}, cb_prefix={})",
            pc_before, opcode_at_pc, cb_prefix
        );

        (pc_before, op, cb_prefix, self.cpu.last_step_cycles)
    }

    fn update_call_stack(
        &mut self,
        pc_before: u16,
        sp_before: u16,
        opcode: u8,
        call_target: u16,
        interrupt_pending: bool,
    ) {
        let pc_after = self.cpu.pc;
        let sp_after = self.cpu.sp;
        let pushed = sp_after == sp_before.wrapping_sub(2);
        let popped = sp_after == sp_before.wrapping_add(2);

        let frame = if interrupt_pending && pushed {
            Some((pc_after, pc_before, CallStackKind::Interrupt))
        } else if pushed && matches!(opcode, 0xC4 | 0xCC | 0xCD | 0xD4 | 0xDC) {
            (pc_after == call_target).then_some((
                call_target,
                pc_before.wrapping_add(3),
                CallStackKind::Call,
            ))
        } else if pushed
            && matches!(
                opcode,
                0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF
            )
        {
            Some((pc_after, pc_before.wrapping_add(1), CallStackKind::Restart))
        } else {
            None
        };

        if let Some((target, return_address, kind)) = frame {
            if self.call_stack.len() == 256 {
                self.call_stack.remove(0);
            }
            self.call_stack.push(CallStackEntry {
                target,
                return_address,
                target_rom_offset: self.bus.cartridge.rom_offset(target),
                return_rom_offset: self.bus.cartridge.rom_offset(return_address),
                kind,
            });
            return;
        }

        if popped && matches!(opcode, 0xC0 | 0xC8 | 0xC9 | 0xD0 | 0xD8 | 0xD9) {
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

    pub fn step_until_frame_or<F>(&mut self, mut should_stop: F)
    where
        F: FnMut(&mut Self) -> bool,
    {
        if matches!(self.cpu.running, CpuState::Suspended) {
            return;
        }

        if self.bus.ppu_lcdc() & 0x80 == 0 {
            let frame_cycles = Self::cycles_per_frame(self.hardware_mode);
            let target = self.cpu.cycles.wrapping_add(frame_cycles);
            while self.cpu.cycles < target && !matches!(self.cpu.running, CpuState::Suspended) {
                self.step_runtime_instruction();
                if should_stop(self) {
                    return;
                }
            }
            if !matches!(self.cpu.running, CpuState::Suspended) {
                self.frame_count = self.frame_count.wrapping_add(1);
            }
            return;
        }

        let max_cycles = Self::cycles_per_frame(self.hardware_mode).saturating_mul(2);
        let start_cycles = self.cpu.cycles;
        let mut previous_ly = self.bus.ppu_ly();

        while self.cpu.cycles.wrapping_sub(start_cycles) < max_cycles
            && !matches!(self.cpu.running, CpuState::Suspended)
        {
            self.step_runtime_instruction();
            if should_stop(self) {
                return;
            }
            if self.reached_vblank_start(&mut previous_ly) {
                break;
            }
        }

        if !matches!(self.cpu.running, CpuState::Suspended) {
            self.frame_count = self.frame_count.wrapping_add(1);
        }
    }

    fn step_runtime_instruction(&mut self) {
        if self.debug.any_active()
            || !self.rom_breakpoints.is_empty()
            || self.opcode_log.enabled
            || self.instruction_trace.is_enabled()
        {
            let _ = self.step_instruction();
        } else {
            self.step_cpu();
        }
    }

    fn step_cpu(&mut self) {
        self.cpu.step(&mut self.bus);
        self.cycle_count = self
            .cycle_count
            .wrapping_add(self.cpu.last_step_master_ticks);
        self.hardware_mode = self.bus.hardware_mode;
    }

    pub fn step_frame(&mut self) {
        if matches!(self.cpu.running, CpuState::Suspended) {
            return;
        }

        if self.bus.ppu_lcdc() & 0x80 == 0 {
            self.step_frame_by_cycle_budget();
            if !matches!(self.cpu.running, CpuState::Suspended) {
                self.frame_count = self.frame_count.wrapping_add(1);
            }
            return;
        }

        let max_cycles = Self::cycles_per_frame(self.hardware_mode).saturating_mul(2);
        let start_cycles = self.cpu.cycles;
        let mut previous_ly = self.bus.ppu_ly();

        if self.debug.any_active()
            || !self.rom_breakpoints.is_empty()
            || self.opcode_log.enabled
            || self.instruction_trace.is_enabled()
        {
            while !matches!(self.cpu.running, CpuState::Suspended) {
                let _ = self.step_instruction();
                if self.reached_vblank_start(&mut previous_ly)
                    || self.cpu.cycles.wrapping_sub(start_cycles) >= max_cycles
                {
                    break;
                }
            }
        } else {
            while self.cpu.cycles.wrapping_sub(start_cycles) < max_cycles {
                self.step_cpu();
                if self.reached_vblank_start(&mut previous_ly) {
                    break;
                }
            }
        }

        if !matches!(self.cpu.running, CpuState::Suspended) {
            self.frame_count = self.frame_count.wrapping_add(1);
        }
    }

    fn step_frame_by_cycle_budget(&mut self) {
        let frame_cycles = Self::cycles_per_frame(self.hardware_mode);
        let target = self.cpu.cycles.wrapping_add(frame_cycles);

        if self.debug.any_active()
            || !self.rom_breakpoints.is_empty()
            || self.opcode_log.enabled
            || self.instruction_trace.is_enabled()
        {
            while self.cpu.cycles < target && !matches!(self.cpu.running, CpuState::Suspended) {
                let _ = self.step_instruction();
            }
        } else {
            while self.cpu.cycles < target {
                self.step_cpu();
            }
        }
    }

    fn reached_vblank_start(&self, previous_ly: &mut u8) -> bool {
        let ly = self.bus.ppu_ly();
        let reached = *previous_ly < 144 && ly >= 144;
        *previous_ly = ly;
        reached
    }

    pub fn set_mbc7_host_tilt(&mut self, x: f32, y: f32) {
        self.bus.cartridge.set_mbc7_tilt(x, y);
    }

    pub fn set_camera_host_frame(&mut self, frame: &[u8]) {
        self.bus.cartridge.set_camera_frame(frame);
    }
}

fn gb_registers(cpu: &crate::hardware::cpu::Cpu) -> [u32; 10] {
    [
        u32::from(cpu.regs.a),
        u32::from(cpu.regs.f),
        u32::from(cpu.regs.b),
        u32::from(cpu.regs.c),
        u32::from(cpu.regs.d),
        u32::from(cpu.regs.e),
        u32::from(cpu.regs.h),
        u32::from(cpu.regs.l),
        u32::from(cpu.sp),
        u32::from(cpu.pc),
    ]
}

fn push_register_deltas<const N: usize>(
    record: &mut InstructionTraceRecord,
    before: &[u32; N],
    after: &[u32; N],
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

fn gb_instruction_len(opcode: u8) -> usize {
    if opcode == 0xCB {
        return 2;
    }
    match opcode {
        0x01 | 0x08 | 0x11 | 0x21 | 0x31 | 0xC2 | 0xC3 | 0xC4 | 0xCA | 0xCC | 0xCD | 0xD2
        | 0xD4 | 0xDA | 0xDC | 0xEA | 0xFA => 3,
        0x06 | 0x0E | 0x10 | 0x16 | 0x18 | 0x1E | 0x20 | 0x26 | 0x28 | 0x2E | 0x30 | 0x36
        | 0x38 | 0x3E | 0xC6 | 0xCE | 0xD6 | 0xDE | 0xE0 | 0xE6 | 0xE8 | 0xEE | 0xF0 | 0xF6
        | 0xF8 | 0xFE => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::CallStackKind;
    use crate::hardware::types::hardware_mode::HardwareModePreference;
    use zeff_emu_common::debug::DebugEvent;

    #[test]
    fn step_frame_presents_at_vblank_start() {
        let rom = vec![0u8; 0x8000];
        let mut emu = Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("test ROM should initialize");

        emu.step_frame();
        assert_eq!(emu.ppu_ly(), 144);

        emu.step_frame();
        assert_eq!(emu.ppu_ly(), 144);
    }

    #[test]
    fn call_stack_tracks_call_and_return() {
        let mut rom = vec![0u8; 0x8000];
        rom[0x100..0x103].copy_from_slice(&[0xCD, 0x05, 0x01]);
        rom[0x105] = 0xC9;
        let mut emu = Emulator::from_rom_data(&rom, HardwareModePreference::Auto).unwrap();
        emu.set_opcode_log_enabled(true);

        emu.step_instruction();
        assert_eq!(emu.call_stack.len(), 1);
        assert_eq!(emu.call_stack[0].target, 0x0105);
        assert_eq!(emu.call_stack[0].return_address, 0x0103);
        assert_eq!(emu.call_stack[0].kind, CallStackKind::Call);

        emu.step_instruction();
        assert!(emu.call_stack.is_empty());
    }

    #[test]
    fn instruction_trace_captures_mapping_bytes_and_deltas() {
        let mut rom = vec![0u8; 0x8000];
        rom[0x100..0x105].copy_from_slice(&[0x3E, 0x42, 0xEA, 0x00, 0xC0]);
        let mut emu = Emulator::from_rom_data(&rom, HardwareModePreference::Auto).unwrap();
        emu.set_instruction_trace_enabled(true);

        emu.step_instruction();

        let entry = emu.instruction_trace().iter().next().unwrap();
        assert_eq!(entry.pc, 0x100);
        assert_eq!(entry.physical_rom_offset, Some(0x100));
        assert_eq!(&entry.instruction[..2], &[0x3E, 0x42]);
        assert!(
            entry
                .register_deltas()
                .iter()
                .any(|delta| delta.register == 0)
        );

        emu.step_instruction();
        let write = emu.instruction_trace().iter().nth(1).unwrap().writes()[0];
        assert_eq!(write.address, 0xC000);
        assert_eq!(write.new_value, 0x42);
    }

    #[test]
    fn call_stack_tracks_interrupt_and_reti() {
        let mut rom = vec![0u8; 0x8000];
        rom[0x40] = 0xD9;
        let mut emu = Emulator::from_rom_data(&rom, HardwareModePreference::Auto).unwrap();
        emu.set_opcode_log_enabled(true);
        emu.cpu.ime = ImeState::Enabled;
        emu.bus.ie = 1;
        emu.bus.if_reg = 1;
        emu.set_event_breakpoint(DebugEvent::Interrupt, true);

        emu.step_instruction();
        assert_eq!(emu.call_stack.len(), 1);
        assert_eq!(emu.call_stack[0].target, 0x0040);
        assert_eq!(emu.call_stack[0].return_address, 0x0100);
        assert_eq!(emu.call_stack[0].kind, CallStackKind::Interrupt);
        assert_eq!(emu.debug_hit_event(), Some(DebugEvent::Interrupt));
        assert_eq!(emu.cpu.running, CpuState::Suspended);

        emu.debug_continue();
        emu.step_instruction();
        assert!(emu.call_stack.is_empty());
    }
}
