use crate::emulator::Emulator;
use crate::hardware::cpu::CpuState;
use zeff_emu_common::cheats::CheatByteTarget;

impl Emulator {
    pub fn set_opcode_log_enabled(&mut self, enabled: bool) {
        if self.opcode_log.enabled != enabled {
            self.call_stack.clear();
        }
        self.opcode_log.set_enabled(enabled);
    }

    pub fn recent_opcodes(&self, n: usize) -> Vec<(u16, u8, Option<usize>)> {
        self.opcode_log.recent(n)
    }

    pub fn is_cpu_suspended(&self) -> bool {
        self.cpu.state == CpuState::Suspended
    }

    pub fn debug_continue(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.cpu.state = CpuState::Running;
    }

    pub fn debug_step(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = true;
        self.cpu.state = CpuState::Running;
    }

    pub fn add_breakpoint(&mut self, addr: u16) {
        self.debug.add_breakpoint(addr);
    }

    pub fn add_one_shot_breakpoint(&mut self, addr: u16) {
        self.debug.add_one_shot_breakpoint(addr);
    }

    pub fn remove_breakpoint(&mut self, addr: u16) {
        self.debug.remove_breakpoint(addr);
    }

    pub fn toggle_breakpoint(&mut self, addr: u16) {
        self.debug.toggle_breakpoint(addr);
    }

    pub fn add_watchpoint(&mut self, addr: u16, watch_type: crate::debug::WatchType) {
        self.debug.add_watchpoint(addr, watch_type);
    }

    pub fn add_watchpoint_range(
        &mut self,
        start: u16,
        end: u16,
        watch_type: crate::debug::WatchType,
    ) {
        self.debug.add_watchpoint_range(start, end, watch_type);
    }

    pub fn remove_watchpoint(&mut self, start: u16, end: u16, watch_type: crate::debug::WatchType) {
        self.debug.remove_watchpoint(start, end, watch_type);
    }

    pub fn iter_breakpoints(&self) -> impl Iterator<Item = u16> + '_ {
        self.debug.iter_breakpoints()
    }

    pub fn iter_one_shot_breakpoints(&self) -> impl Iterator<Item = u16> + '_ {
        self.debug.iter_one_shot_breakpoints()
    }

    pub fn debug_watchpoints(&self) -> &[crate::debug::Watchpoint] {
        &self.debug.watchpoints
    }

    pub fn debug_hit_breakpoint(&self) -> Option<u16> {
        self.debug.hit_breakpoint
    }

    pub fn debug_hit_watchpoint(&self) -> Option<&crate::debug::WatchHit> {
        self.debug.hit_watchpoint.as_ref()
    }

    pub fn cpu_write(&mut self, addr: u16, value: u8) {
        self.bus.cpu_write(addr, value);
    }

    pub fn cpu_peek(&self, addr: u16) -> u8 {
        self.bus.cpu_peek(addr)
    }

    pub fn cpu_peek8(&self, addr: u16) -> u8 {
        self.cpu_peek(addr)
    }

    pub fn cpu_read8_debuggable(&mut self, addr: u16) -> u8 {
        let value = self.bus.cpu_peek(addr);
        self.debug.check_watch_read(addr, value);
        value
    }

    pub fn cpu_write8(&mut self, addr: u16, value: u8) {
        let old = self.bus.cpu_peek(addr);
        self.bus.cpu_write(addr, value);
        self.debug.check_watch_write(addr, old, value);
    }

    pub fn set_cpu_pc(&mut self, pc: u16) {
        self.cpu.pc = pc;
    }
}

impl CheatByteTarget<u16> for Emulator {
    fn cheat_peek8(&self, address: u16) -> u8 {
        self.cpu_peek(address)
    }

    fn cheat_write8(&mut self, address: u16, value: u8) {
        self.cpu_write(address, value);
    }
}
