use crate::emulator::Emulator;
use zeff_emu_common::address::Address;
use zeff_emu_common::debug::{AddressWatchHit, AddressWatchpoint, WatchType};

impl Emulator {
    pub fn is_suspended(&self) -> bool {
        self.cpu.is_suspended()
    }

    pub fn is_cpu_suspended(&self) -> bool {
        self.is_suspended()
    }

    pub fn suspend(&mut self) {
        self.cpu.suspend();
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
        let _ = self.step_instruction_inner(true, false);
        self.cpu.suspend();
    }

    pub fn add_breakpoint(&mut self, addr: Address) {
        self.debug.add_breakpoint(addr);
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

    pub fn iter_breakpoints(&self) -> impl Iterator<Item = Address> + '_ {
        self.debug.iter_breakpoints()
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

    pub fn cpu_peek8(&self, addr: u16) -> u8 {
        self.bus.cpu_read(addr)
    }

    pub fn cpu_read8_debuggable(&mut self, addr: u16) -> u8 {
        let value = self.bus.cpu_read(addr);
        self.debug.check_watch_read(Address::from(addr), value);
        value
    }

    pub fn cpu_write8(&mut self, addr: u16, value: u8) {
        let old = self.bus.cpu_read(addr);
        self.bus.cpu_write(addr, value);
        self.debug
            .check_watch_write(Address::from(addr), old, value);
    }

    pub fn debug_write(&mut self, addr: Address, val: u8) {
        let addr16 = addr as u16;
        let old_value = self.bus.cpu_read(addr16);
        self.bus.cpu_write(addr16, val);
        self.debug
            .check_watch_write(Address::from(addr16), old_value, val);
        if self.debug.hit_watchpoint.is_some() {
            self.cpu.suspend();
        }
    }

    pub fn set_opcode_log_enabled(&mut self, enabled: bool) {
        self.opcode_log.set_enabled(enabled);
    }

    pub fn recent_opcodes(&self, n: usize) -> Vec<(u16, u8, u32)> {
        self.opcode_log.recent(n)
    }
}
