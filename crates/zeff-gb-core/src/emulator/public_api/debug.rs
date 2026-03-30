use super::super::Emulator;
use crate::debug::WatchType;
use crate::hardware::types::CpuState;

impl Emulator {
    pub fn set_opcode_log_enabled(&mut self, enabled: bool) {
        self.opcode_log.enabled = enabled;
    }

    pub fn debug_continue(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.cpu.running = CpuState::Running;
    }

    pub fn debug_step(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = true;
        self.cpu.running = CpuState::Running;
    }

    pub fn add_breakpoint(&mut self, addr: u16) {
        self.debug.add_breakpoint(addr);
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

    pub fn iter_breakpoints(&self) -> impl Iterator<Item = u16> + '_ {
        self.debug.iter_breakpoints()
    }
}

