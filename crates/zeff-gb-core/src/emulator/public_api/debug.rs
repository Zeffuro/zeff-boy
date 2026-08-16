use super::super::Emulator;
use crate::debug::{WatchHit, WatchType, Watchpoint};
use crate::hardware::types::CpuState;
use zeff_emu_common::cheats::CheatByteTarget;

impl Emulator {
    pub fn set_opcode_log_enabled(&mut self, enabled: bool) {
        self.opcode_log.enabled = enabled;
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

    pub fn toggle_rom_breakpoint(&mut self, offset: usize) {
        match self.rom_breakpoints.binary_search(&offset) {
            Ok(index) => {
                self.rom_breakpoints.remove(index);
            }
            Err(index) => self.rom_breakpoints.insert(index, offset),
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

    pub fn recent_opcodes(&self, n: usize) -> Vec<(u16, u8, bool)> {
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
}

impl CheatByteTarget<u16> for Emulator {
    fn cheat_peek8(&self, address: u16) -> u8 {
        self.peek_byte_raw(address)
    }

    fn cheat_write8(&mut self, address: u16, value: u8) {
        self.write_byte(address, value);
    }
}
