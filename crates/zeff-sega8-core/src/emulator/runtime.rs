use crate::emulator::Emulator;
use crate::hardware::bus::CpuAccessTraceEvent;
use crate::hardware::cartridge::Sega8System;
use crate::hardware::constants::{
    GG_SCREEN_H, GG_SCREEN_W, GG_VIEWPORT_X, GG_VIEWPORT_Y, SMS_SCREEN_H, SMS_SCREEN_W,
};
use crate::hardware::cpu::FetchedInstruction;
use crate::hardware::vdp::{Mode4ColorMode, Mode4RenderArea, Tms9918ColorMode};
use zeff_emu_common::address::Address;

impl Emulator {
    pub fn step_frame(&mut self) {
        if self.cpu.is_suspended() {
            return;
        }
        let target_cycles = self
            .cpu
            .cycles()
            .wrapping_add(u64::from(self.video_standard.cycles_per_frame()));
        while self.cpu.cycles() < target_cycles {
            if self.step_instruction().is_none() || self.cpu.is_suspended() {
                return;
            }
        }
        self.finish_frame();
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
        let trace_active = watch_active || collect_bus_trace;
        if trace_active {
            self.bus.begin_cpu_access_trace();
        }

        let fetched = self.cpu.step(&mut self.bus);
        if let Some(instruction) = fetched {
            self.bus.step_cycles(instruction.cycles);
            self.opcode_log
                .push((instruction.pc, instruction.opcode, instruction.cycles));
        }

        let mut bus_trace_events = Vec::new();
        if trace_active {
            let events = self.bus.drain_cpu_access_trace();
            if collect_bus_trace {
                bus_trace_events = events.clone();
            }
            if watch_active {
                self.apply_cpu_access_trace_watchpoints(events);
            }
            if self.debug.hit_watchpoint.is_some() {
                self.cpu.suspend();
            }
        }

        (fetched, bus_trace_events)
    }

    fn apply_cpu_access_trace_watchpoints(&mut self, events: Vec<CpuAccessTraceEvent>) {
        for event in events {
            match event {
                CpuAccessTraceEvent::Read { addr, value } => {
                    self.debug.check_watch_read(Address::from(addr), value);
                }
                CpuAccessTraceEvent::Write {
                    addr,
                    old_value,
                    new_value,
                } => {
                    self.debug
                        .check_watch_write(Address::from(addr), old_value, new_value);
                }
                CpuAccessTraceEvent::IoRead { .. } | CpuAccessTraceEvent::IoWrite { .. } => {}
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
