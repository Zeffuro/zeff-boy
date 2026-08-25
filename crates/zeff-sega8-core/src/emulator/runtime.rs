use crate::emulator::Emulator;
use crate::hardware::bus::CpuAccessTraceEvent;
use crate::hardware::cartridge::Sega8System;
use crate::hardware::constants::{
    GG_SCREEN_H, GG_SCREEN_W, GG_VIEWPORT_X, GG_VIEWPORT_Y, SMS_SCANLINE_Z80_CYCLES, SMS_SCREEN_H,
    SMS_SCREEN_W,
};
use crate::hardware::cpu::FetchedInstruction;
use crate::hardware::vdp::{Mode4ColorMode, Mode4RenderArea, Tms9918ColorMode};
use zeff_emu_common::address::Address;
use zeff_emu_common::cpu::CpuCore;
use zeff_emu_common::debug::{
    DebugEvent, InstructionTraceRecord, RegisterDelta, TraceExecMode, TraceWrite, TraceWriteKind,
    TraceWriteWidth,
};

impl Emulator {
    pub fn step_frame(&mut self) {
        if self.cpu.is_suspended() {
            return;
        }
        let target_cycles = self.next_frame_cycle_target();
        while self.cpu.cycles() < target_cycles {
            if self.step_instruction().is_none() || self.cpu.is_suspended() {
                return;
            }
        }
        self.finish_frame();
    }

    pub fn next_frame_cycle_target(&self) -> u64 {
        let vdp = self.bus.vdp();
        let cycles_into_frame =
            u32::from(vdp.scanline()) * SMS_SCANLINE_Z80_CYCLES + vdp.scanline_cycle();
        let frame_cycles = self.video_standard.cycles_per_frame();
        let remaining = if cycles_into_frame == 0 {
            frame_cycles
        } else {
            frame_cycles - cycles_into_frame
        };
        self.cpu.cycles().wrapping_add(u64::from(remaining))
    }

    pub fn finish_frame(&mut self) {
        self.render_frame();
        self.frame_count = self.frame_count.wrapping_add(1);
    }

    pub fn step_instruction(&mut self) -> Option<FetchedInstruction> {
        self.step_instruction_inner(false, false).0
    }

    pub fn step_instruction_with_bus_trace(
        &mut self,
    ) -> (Option<FetchedInstruction>, Vec<CpuAccessTraceEvent>) {
        self.step_instruction_inner(false, true)
    }

    pub(crate) fn step_instruction_inner(
        &mut self,
        skip_breakpoint_check: bool,
        collect_bus_trace: bool,
    ) -> (Option<FetchedInstruction>, Vec<CpuAccessTraceEvent>) {
        if self.cpu.is_suspended() {
            return (None, Vec::new());
        }

        let pc = Address::from(self.cpu.regs().pc);
        if !skip_breakpoint_check && self.debug.should_break(pc) {
            self.cpu.suspend();
            return (None, Vec::new());
        }

        let watch_active = !self.debug.watchpoints.is_empty();
        let instruction_trace_enabled = self.instruction_trace.is_enabled();
        let trace_active = watch_active || collect_bus_trace || instruction_trace_enabled;
        if trace_active {
            if watch_active || collect_bus_trace {
                self.bus.begin_cpu_access_trace();
            } else {
                self.bus.begin_cpu_write_trace();
            }
        }

        let pc_before = self.cpu.regs().pc;
        let cycles_before = self.cpu.cycles();
        let registers_before = if instruction_trace_enabled {
            Some(sega_registers(self.cpu.regs()))
        } else {
            None
        };
        let physical_rom_offset = if instruction_trace_enabled {
            self.bus.rom_offset_for_cpu_address(pc_before)
        } else {
            None
        };
        let fetched = <crate::hardware::cpu::Cpu as CpuCore<crate::hardware::bus::Bus>>::step_cpu(
            &mut self.cpu,
            &mut self.bus,
        );
        if let Some(instruction) = fetched {
            self.bus.step_cycles(instruction.cycles);
            self.opcode_log
                .push((instruction.pc, instruction.opcode, instruction.cycles));
        }
        if self.cpu.last_step_was_interrupt()
            && self
                .debug
                .check_event(zeff_emu_common::debug::DebugEvent::Interrupt)
        {
            self.cpu.suspend();
        }

        let mut bus_trace_events = Vec::new();
        let mut trace_record = instruction_trace_enabled.then(|| {
            let interrupt = self.cpu.last_step_was_interrupt();
            let instruction = if interrupt {
                &[]
            } else {
                self.cpu.instruction_bytes()
            };
            let mut record = InstructionTraceRecord::new(
                TraceExecMode::Z80,
                u32::from(pc_before),
                physical_rom_offset.map(|offset| offset as u64),
                self.frame_count,
                cycles_before,
                instruction,
            );
            if interrupt {
                record.event = Some(DebugEvent::Interrupt);
            }
            record
        });
        if trace_active {
            let events = self.bus.drain_cpu_access_trace();
            if let Some(record) = &mut trace_record {
                append_sega_writes(record, &events);
            }
            if watch_active {
                self.apply_cpu_access_trace_watchpoints(&events);
            }
            if collect_bus_trace {
                bus_trace_events = events;
            } else {
                self.bus.recycle_cpu_access_trace(events);
            }
            if self.debug.hit_watchpoint.is_some() {
                self.cpu.suspend();
            }
        }

        if let Some(mut record) = trace_record {
            push_sega_register_deltas(
                &mut record,
                &registers_before.expect("trace state"),
                &sega_registers(self.cpu.regs()),
            );
            self.instruction_trace.push(record);
        }

        (fetched, bus_trace_events)
    }

    fn apply_cpu_access_trace_watchpoints(&mut self, events: &[CpuAccessTraceEvent]) {
        for &event in events {
            match event {
                CpuAccessTraceEvent::Read {
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
                CpuAccessTraceEvent::Write {
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
                CpuAccessTraceEvent::Read { .. } | CpuAccessTraceEvent::Write { .. } => {}
            }
            if self.debug.hit_watchpoint.is_some() {
                break;
            }
        }
    }

    fn render_frame(&mut self) {
        match self.system() {
            Sega8System::MasterSystem => {
                let area = Mode4RenderArea::new(SMS_SCREEN_W, SMS_SCREEN_H, 0, 0);
                if self.bus.vdp().mode4_enabled() {
                    self.bus.vdp().render_mode4_presented_frame_rgba(
                        &mut self.framebuffer,
                        area,
                        Mode4ColorMode::Sms,
                    );
                } else {
                    self.bus.vdp().render_tms9918_presented_area_rgba(
                        &mut self.framebuffer,
                        area,
                        Tms9918ColorMode::Palette,
                    );
                }
            }
            Sega8System::GameGear => {
                let area =
                    Mode4RenderArea::new(GG_SCREEN_W, GG_SCREEN_H, GG_VIEWPORT_X, GG_VIEWPORT_Y);
                if self.bus.vdp().mode4_enabled() {
                    self.bus.vdp().render_mode4_presented_frame_rgba(
                        &mut self.framebuffer,
                        area,
                        Mode4ColorMode::GameGear,
                    );
                } else {
                    self.bus.vdp().render_tms9918_presented_area_rgba(
                        &mut self.framebuffer,
                        area,
                        Tms9918ColorMode::GameGearCram,
                    );
                }
            }
            Sega8System::Sg1000 => {
                self.bus.vdp().render_tms9918_presented_area_rgba(
                    &mut self.framebuffer,
                    Mode4RenderArea::new(SMS_SCREEN_W, SMS_SCREEN_H, 0, 0),
                    Tms9918ColorMode::Palette,
                );
            }
        }
    }
}

fn sega_registers(regs: crate::hardware::cpu::Registers) -> [u32; 14] {
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

fn push_sega_register_deltas(
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

fn append_sega_writes(record: &mut InstructionTraceRecord, events: &[CpuAccessTraceEvent]) {
    for event in events {
        let write = match *event {
            CpuAccessTraceEvent::Write {
                addr,
                old_value,
                new_value,
                width: TraceWriteWidth::Byte,
                space: TraceWriteKind::Memory,
                ..
            } => TraceWrite {
                address: addr,
                old_value,
                new_value,
                width: TraceWriteWidth::Byte,
                kind: TraceWriteKind::Memory,
            },
            CpuAccessTraceEvent::Write {
                addr,
                old_value,
                new_value,
                width: TraceWriteWidth::Byte,
                space: TraceWriteKind::Io,
                ..
            } => TraceWrite {
                address: addr,
                old_value,
                new_value,
                width: TraceWriteWidth::Byte,
                kind: TraceWriteKind::Io,
            },
            CpuAccessTraceEvent::Read { .. } | CpuAccessTraceEvent::Write { .. } => continue,
        };
        record.push_write(write);
    }
}
