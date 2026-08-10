use crate::emulator::Emulator;
use zeff_emu_common::address::Address;
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
}
