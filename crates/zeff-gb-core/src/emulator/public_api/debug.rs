use super::super::Emulator;
use crate::debug::{WatchHit, WatchType, Watchpoint};
use crate::hardware::types::CpuState;
use zeff_emu_common::cheats::CheatByteTarget;

impl Emulator {
    pub fn set_opcode_log_enabled(&mut self, enabled: bool) {
        if self.opcode_log.enabled != enabled {
            self.call_stack.clear();
        }
        self.opcode_log.enabled = enabled;
    }

    pub fn instruction_trace(&self) -> &zeff_emu_common::debug::InstructionTraceStore {
        &self.instruction_trace
    }

    pub fn set_instruction_trace_enabled(&mut self, enabled: bool) {
        self.instruction_trace.set_enabled(enabled);
    }

    pub fn set_instruction_trace_capacity(&mut self, capacity: usize) {
        self.instruction_trace.set_capacity(capacity);
    }

    pub fn clear_instruction_trace(&mut self) {
        self.instruction_trace.clear();
    }

    pub fn debug_continue(&mut self) {
        self.debug.clear_hits();
        self.hit_rom_breakpoint = None;
        self.debug.break_on_next = false;
        self.cpu.running = CpuState::Running;
    }

    pub fn debug_step(&mut self) {
        self.debug.clear_hits();
        self.hit_rom_breakpoint = None;
        self.debug.break_on_next = true;
        self.cpu.running = CpuState::Running;
    }

    pub fn debug_suspend(&mut self) {
        self.cpu.running = CpuState::Suspended;
    }

    pub fn add_breakpoint(&mut self, addr: u16) {
        self.debug.add_breakpoint(addr);
    }

    pub fn add_one_shot_breakpoint(&mut self, addr: u16) {
        self.debug.add_one_shot_breakpoint(addr);
    }

    pub fn add_breakpoint_after(&mut self, addr: u16, target_hits: u64) {
        self.debug.add_breakpoint_after(addr, target_hits);
    }

    pub fn remove_breakpoint(&mut self, addr: u16) {
        self.debug.remove_breakpoint(addr);
    }

    pub fn toggle_breakpoint(&mut self, addr: u16) {
        self.debug.toggle_breakpoint(addr);
    }

    pub fn add_watchpoint(&mut self, addr: u16, watch_type: WatchType) {
        self.debug.add_watchpoint(addr, watch_type);
    }

    pub fn add_watchpoint_range(&mut self, start: u16, end: u16, watch_type: WatchType) {
        self.debug.add_watchpoint_range(start, end, watch_type);
    }

    pub fn remove_watchpoint(&mut self, start: u16, end: u16, watch_type: WatchType) {
        self.debug.remove_watchpoint(start, end, watch_type);
    }

    pub fn iter_breakpoints(&self) -> impl Iterator<Item = u16> + '_ {
        self.debug.iter_breakpoints()
    }

    pub fn iter_one_shot_breakpoints(&self) -> impl Iterator<Item = u16> + '_ {
        self.debug.iter_one_shot_breakpoints()
    }

    pub fn iter_breakpoint_hit_conditions(
        &self,
    ) -> impl Iterator<Item = zeff_emu_common::debug::BreakpointHitCondition> + '_ {
        self.debug.iter_breakpoint_hit_conditions()
    }

    pub fn set_event_breakpoint(
        &mut self,
        event: zeff_emu_common::debug::DebugEvent,
        enabled: bool,
    ) {
        self.debug.set_event_breakpoint(event, enabled);
    }

    pub fn iter_event_breakpoints(
        &self,
    ) -> impl Iterator<Item = zeff_emu_common::debug::DebugEvent> + '_ {
        self.debug.iter_event_breakpoints()
    }

    pub fn debug_hit_event(&self) -> Option<zeff_emu_common::debug::DebugEvent> {
        self.debug.hit_event
    }

    pub fn toggle_rom_breakpoint(&mut self, offset: usize) {
        match self.rom_breakpoints.binary_search(&offset) {
            Ok(index) => {
                self.rom_breakpoints.remove(index);
            }
            Err(index) => self.rom_breakpoints.insert(index, offset),
        }
    }

    pub fn add_rom_breakpoint(&mut self, offset: usize) {
        if let Err(index) = self.rom_breakpoints.binary_search(&offset) {
            self.rom_breakpoints.insert(index, offset);
        }
    }

    pub fn remove_rom_breakpoint(&mut self, offset: usize) {
        if let Ok(index) = self.rom_breakpoints.binary_search(&offset) {
            self.rom_breakpoints.remove(index);
        }
    }

    pub fn iter_rom_breakpoints(&self) -> impl Iterator<Item = usize> + '_ {
        self.rom_breakpoints.iter().copied()
    }

    pub fn debug_hit_rom_breakpoint(&self) -> Option<usize> {
        self.hit_rom_breakpoint
    }

    pub fn debug_watchpoints(&self) -> &[Watchpoint] {
        &self.debug.watchpoints
    }

    pub fn debug_hit_breakpoint(&self) -> Option<u16> {
        self.debug.hit_breakpoint
    }

    pub fn debug_hit_watchpoint(&self) -> Option<&WatchHit> {
        self.debug.hit_watchpoint.as_ref()
    }

    pub fn recent_opcodes(&self, n: usize) -> Vec<(u16, u8, bool, Option<usize>)> {
        self.opcode_log.recent(n)
    }

    pub fn cpu_peek8(&self, addr: u16) -> u8 {
        self.bus.read_byte(addr)
    }

    pub fn cpu_read8_debuggable(&mut self, addr: u16) -> u8 {
        let value = self.bus.read_byte(addr);
        self.debug.check_watch_read(addr, value);
        value
    }

    pub fn cpu_write8(&mut self, addr: u16, value: u8) {
        let old = self.bus.read_byte_raw(addr);
        self.bus.write_byte(addr, value);
        self.debug.check_watch_write(addr, old, value);
    }

    pub fn debug_execute_guest_call(
        &mut self,
        target: u16,
        instruction_budget: u64,
    ) -> Result<u64, String> {
        if !matches!(self.cpu.running, CpuState::Suspended) {
            return Err("CPU must be suspended".to_owned());
        }
        let return_pc = self.cpu.pc;
        if target == return_pc || instruction_budget == 0 {
            return Err("invalid call target or budget".to_owned());
        }
        let return_sp = self.cpu.sp;
        let saved_ime = self.cpu.ime;
        let [lo, hi] = return_pc.to_le_bytes();
        self.cpu.sp = self.cpu.sp.wrapping_sub(1);
        self.bus.write_byte(self.cpu.sp, hi);
        self.cpu.sp = self.cpu.sp.wrapping_sub(1);
        self.bus.write_byte(self.cpu.sp, lo);
        self.cpu.pc = target;
        self.cpu.ime = crate::hardware::types::ImeState::Disabled;
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.cpu.running = CpuState::Running;

        for instructions in 1..=instruction_budget {
            self.step_instruction();
            if self.cpu.pc == return_pc && self.cpu.sp == return_sp {
                self.cpu.ime = saved_ime;
                self.cpu.running = CpuState::Suspended;
                return Ok(instructions);
            }
            if matches!(self.cpu.running, CpuState::Suspended) {
                return Err("call hit a debugger stop".to_owned());
            }
        }
        self.cpu.running = CpuState::Suspended;
        Err("call exceeded its instruction budget".to_owned())
    }
}

impl CheatByteTarget<u16> for Emulator {
    fn cheat_peek8(&self, address: u16) -> u8 {
        self.peek_byte_raw(address)
    }

    fn cheat_write8(&mut self, address: u16, value: u8) {
        self.write_byte(address, value);
    }
}
