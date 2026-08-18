use crate::emulator::Emulator;
use crate::emulator::WsOpcodeRecord;
use crate::hardware::bus::DebugTraceMode;
use zeff_emu_common::address::Address;
use zeff_emu_common::cheats::CheatByteTarget;
use zeff_emu_common::debug::{AddressWatchHit, AddressWatchpoint, WatchType};

impl Emulator {
    pub fn is_cpu_suspended(&self) -> bool {
        self.cpu.is_suspended()
    }

    pub fn cpu_peek8(&self, addr: u32) -> u8 {
        self.bus.peek8(addr)
    }

    pub fn cpu_read8_debuggable(&mut self, addr: u32) -> u8 {
        let value = self.bus.peek8(addr);
        self.debug.check_watch_read(Address::from(addr), value);
        value
    }

    pub fn cpu_peek16(&self, addr: u32) -> u16 {
        self.bus.peek16(addr)
    }

    pub fn cpu_write8(&mut self, addr: u32, value: u8) {
        let old = self.bus.peek8(addr);
        self.bus.write8(addr, value);
        self.debug
            .check_watch_write(Address::from(addr), old, value);
    }

    pub fn debug_continue(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.cpu.resume();
    }

    pub fn debug_step(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.cpu.resume();
        let _ = self.step_instruction_inner(true, DebugTraceMode::None);
        self.cpu.suspend();
    }

    pub fn debug_suspend(&mut self) {
        self.cpu.suspend();
    }

    pub fn add_breakpoint(&mut self, addr: Address) {
        self.debug.add_breakpoint(addr);
    }

    pub fn add_one_shot_breakpoint(&mut self, addr: Address) {
        self.debug.add_one_shot_breakpoint(addr);
    }

    pub fn add_breakpoint_after(&mut self, addr: Address, target_hits: u64) {
        self.debug.add_breakpoint_after(addr, target_hits);
    }

    pub fn remove_breakpoint(&mut self, addr: Address) {
        self.debug.remove_breakpoint(addr);
    }

    pub fn toggle_breakpoint(&mut self, addr: Address) {
        self.debug.toggle_breakpoint(addr);
    }

    pub fn add_watchpoint(&mut self, addr: Address, watch_type: WatchType) {
        self.debug.add_watchpoint(addr, watch_type);
    }

    pub fn add_watchpoint_range(&mut self, start: Address, end: Address, watch_type: WatchType) {
        self.debug.add_watchpoint_range(start, end, watch_type);
    }

    pub fn remove_watchpoint(&mut self, start: Address, end: Address, watch_type: WatchType) {
        self.debug.remove_watchpoint(start, end, watch_type);
    }

    pub fn iter_breakpoints(&self) -> impl Iterator<Item = Address> + '_ {
        self.debug.iter_breakpoints()
    }

    pub fn iter_one_shot_breakpoints(&self) -> impl Iterator<Item = Address> + '_ {
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

    pub fn debug_watchpoints(&self) -> &[AddressWatchpoint] {
        &self.debug.watchpoints
    }

    pub fn debug_hit_breakpoint(&self) -> Option<Address> {
        self.debug.hit_breakpoint
    }

    pub fn debug_hit_watchpoint(&self) -> Option<&AddressWatchHit> {
        self.debug.hit_watchpoint.as_ref()
    }

    pub fn set_opcode_log_enabled(&mut self, enabled: bool) {
        self.opcode_log.set_enabled(enabled);
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

    pub fn debug_execute_guest_call(
        &mut self,
        target: u32,
        instruction_budget: u64,
    ) -> Result<u64, String> {
        if !self.cpu.is_suspended() {
            return Err("CPU must be suspended".to_owned());
        }
        if target == self.cpu.pc() || instruction_budget == 0 {
            return Err("invalid call target or budget".to_owned());
        }
        let Some((return_ip, return_sp, interrupts_enabled, interrupt_shadow, brk_shadow)) =
            self.cpu.begin_guest_call(&mut self.bus, target)
        else {
            return Err("target is outside the current code segment".to_owned());
        };
        self.debug.clear_hits();
        self.debug.break_on_next = false;

        for instructions in 1..=instruction_budget {
            self.step_instruction();
            if self.cpu.ip == return_ip && self.cpu.regs[4] == return_sp {
                self.cpu
                    .finish_guest_call(interrupts_enabled, interrupt_shadow, brk_shadow);
                return Ok(instructions);
            }
            if self.cpu.is_suspended() {
                return Err("call hit a debugger stop".to_owned());
            }
        }
        self.cpu.suspend();
        Err("call exceeded its instruction budget".to_owned())
    }

    pub fn recent_opcodes(&self, n: usize) -> Vec<WsOpcodeRecord> {
        self.opcode_log.recent(n)
    }
}

impl CheatByteTarget<Address> for Emulator {
    fn cheat_peek8(&self, address: Address) -> u8 {
        self.cpu_peek8(address)
    }

    fn cheat_write8(&mut self, address: Address, value: u8) {
        self.cpu_write8(address, value);
    }
}
