use zeff_emu_common::address::Address;
use zeff_emu_common::cheats::CheatByteTarget;
use zeff_emu_common::debug::{AddressWatchHit, AddressWatchpoint, DebugEvent, WatchType};

use super::Emulator;

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

    pub fn debug_suspend(&mut self) {
        self.suspend();
    }

    pub fn debug_continue(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.refresh_debug_hooks();
        self.cpu.resume();
    }

    pub fn debug_step(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.refresh_debug_hooks();
        self.cpu.resume();
        let _ = self.step_instruction_inner(true, false);
        self.cpu.suspend();
    }

    pub fn add_breakpoint(&mut self, addr: Address) {
        self.debug.add_breakpoint(addr);
        self.refresh_debug_hooks();
    }

    pub fn add_one_shot_breakpoint(&mut self, addr: Address) {
        self.debug.add_one_shot_breakpoint(addr);
        self.refresh_debug_hooks();
    }

    pub fn add_breakpoint_after(&mut self, addr: Address, target_hits: u64) {
        self.debug.add_breakpoint_after(addr, target_hits);
        self.refresh_debug_hooks();
    }

    pub fn remove_breakpoint(&mut self, addr: Address) {
        self.debug.remove_breakpoint(addr);
        self.refresh_debug_hooks();
    }

    pub fn toggle_breakpoint(&mut self, addr: Address) {
        self.debug.toggle_breakpoint(addr);
        self.refresh_debug_hooks();
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

    pub fn set_event_breakpoint(&mut self, event: DebugEvent, enabled: bool) {
        self.debug.set_event_breakpoint(event, enabled);
        self.refresh_debug_hooks();
    }

    pub fn iter_event_breakpoints(&self) -> impl Iterator<Item = DebugEvent> + '_ {
        self.debug.iter_event_breakpoints()
    }

    pub fn debug_hit_event(&self) -> Option<DebugEvent> {
        self.debug.hit_event
    }

    pub fn add_watchpoint(&mut self, addr: Address, watch_type: WatchType) {
        self.debug.add_watchpoint(addr, watch_type);
        self.refresh_debug_hooks();
    }

    pub fn add_watchpoint_range(&mut self, start: Address, end: Address, watch_type: WatchType) {
        self.debug.add_watchpoint_range(start, end, watch_type);
        self.refresh_debug_hooks();
    }

    pub fn remove_watchpoint(&mut self, start: Address, end: Address, watch_type: WatchType) {
        self.debug.remove_watchpoint(start, end, watch_type);
        self.refresh_debug_hooks();
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
        self.bus.cpu_peek(addr)
    }

    pub fn cpu_read8_debuggable(&mut self, addr: u16) -> u8 {
        let value = self.bus.cpu_peek(addr);
        self.debug.check_watch_read(Address::from(addr), value);
        value
    }

    pub fn cpu_write8(&mut self, addr: u16, value: u8) {
        let old_value = self.bus.cpu_peek(addr);
        self.bus.cpu_write(addr, value);
        let new_value = self.bus.cpu_peek(addr);
        self.debug
            .check_watch_write(Address::from(addr), old_value, new_value);
    }

    pub fn debug_write(&mut self, addr: Address, value: u8) {
        self.cpu_write8(addr as u16, value);
        if self.debug.hit_watchpoint.is_some() {
            self.cpu.suspend();
        }
    }

    pub fn set_opcode_log_enabled(&mut self, enabled: bool) {
        self.opcode_log.set_enabled(enabled);
        self.refresh_debug_hooks();
    }

    pub fn recent_opcodes(&self, count: usize) -> Vec<(u16, u8, u32)> {
        self.opcode_log.recent(count)
    }

    pub fn instruction_trace(&self) -> &zeff_emu_common::debug::InstructionTraceStore {
        &self.instruction_trace
    }

    pub fn set_instruction_trace_enabled(&mut self, enabled: bool) {
        self.instruction_trace.set_enabled(enabled);
        self.refresh_debug_hooks();
    }

    pub fn set_instruction_trace_capacity(&mut self, capacity: usize) {
        self.instruction_trace.set_capacity(capacity);
    }

    pub fn clear_instruction_trace(&mut self) {
        self.instruction_trace.clear();
    }

    pub fn rom_offset_for_cpu_address(&self, addr: u16) -> Option<usize> {
        self.bus.rom_offset_for_cpu_address(addr)
    }

    pub fn rom_mapping_token(&self) -> u64 {
        self.bus.rom_mapping_token()
    }

    pub fn debug_execute_guest_call(
        &mut self,
        target: u16,
        instruction_budget: u64,
    ) -> Result<u64, String> {
        if !self.cpu.is_suspended() {
            return Err("CPU must be suspended".to_owned());
        }
        if target == self.cpu.regs().pc || instruction_budget == 0 {
            return Err("invalid call target or budget".to_owned());
        }
        let (return_pc, return_sp, iff1, iff2, delay) =
            self.cpu.begin_guest_call(&mut self.bus, target);
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.refresh_debug_hooks();

        for instructions in 1..=instruction_budget {
            self.step_instruction();
            let regs = self.cpu.regs();
            if regs.pc == return_pc && regs.sp == return_sp {
                self.cpu.finish_guest_call(iff1, iff2, delay);
                return Ok(instructions);
            }
            if self.cpu.is_suspended() {
                return Err("call hit a debugger stop".to_owned());
            }
        }
        self.cpu.suspend();
        Err("call exceeded its instruction budget".to_owned())
    }

    fn refresh_debug_hooks(&mut self) {
        self.debug_hooks_active = self.debug.break_on_next
            || self.debug.iter_breakpoints().next().is_some()
            || self.debug.iter_event_breakpoints().next().is_some()
            || !self.debug.watchpoints.is_empty()
            || self.opcode_log.enabled
            || self.instruction_trace.is_enabled();
    }
}

impl CheatByteTarget<u16> for Emulator {
    fn cheat_peek8(&self, address: u16) -> u8 {
        self.bus.cpu_peek(address)
    }

    fn cheat_write8(&mut self, address: u16, value: u8) {
        self.bus.cpu_write(address, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::BIOS_SIZE;

    fn emulator(program: &[u8]) -> Emulator {
        let mut bios = vec![0; BIOS_SIZE];
        bios[..program.len()].copy_from_slice(program);
        let mut cartridge = vec![0; 8 * 1024];
        cartridge[..2].copy_from_slice(&[0xAA, 0x55]);
        Emulator::new(&cartridge, &bios, 48_000).unwrap()
    }

    #[test]
    fn breakpoint_step_history_and_watchpoint_share_the_runtime_path() {
        let mut emu = emulator(&[0x3E, 0x5A, 0x32, 0x00, 0x60, 0x76]);
        emu.set_opcode_log_enabled(true);
        emu.set_instruction_trace_enabled(true);
        emu.add_breakpoint(0);

        assert_eq!(emu.step_instruction(), None);
        assert_eq!(emu.debug_hit_breakpoint(), Some(0));
        emu.debug_step();
        assert_eq!(emu.cpu().regs().pc, 2);
        assert_eq!(emu.recent_opcodes(1), vec![(0, 0x3E, 7)]);

        emu.add_watchpoint(0x6000, WatchType::Write);
        emu.debug_continue();
        emu.step_instruction();
        assert!(emu.is_suspended());
        assert_eq!(emu.debug_hit_watchpoint().unwrap().new_value, 0x5A);
        assert_eq!(emu.instruction_trace().iter().count(), 2);
    }

    #[test]
    fn guest_call_returns_to_the_suspended_context() {
        let mut bios = vec![0; BIOS_SIZE];
        bios[..4].copy_from_slice(&[0x31, 0x00, 0x70, 0x00]);
        bios[0x100..0x103].copy_from_slice(&[0x3E, 0x42, 0xC9]);
        let mut cartridge = vec![0; 8 * 1024];
        cartridge[..2].copy_from_slice(&[0xAA, 0x55]);
        let mut emu = Emulator::new(&cartridge, &bios, 48_000).unwrap();
        emu.step_instruction();
        emu.debug_suspend();

        assert_eq!(emu.debug_execute_guest_call(0x0100, 10), Ok(2));
        assert_eq!(emu.cpu().regs().pc, 3);
        assert_eq!(emu.cpu().regs().sp, 0x7000);
        assert_eq!(emu.cpu().regs().a, 0x42);
        assert!(emu.is_suspended());
    }

    #[test]
    fn debugger_hook_cache_tracks_enabled_controls() {
        let mut emu = emulator(&[0x00]);
        assert!(!emu.debug_hooks_active);

        emu.add_breakpoint(1);
        assert!(emu.debug_hooks_active);
        emu.remove_breakpoint(1);
        assert!(!emu.debug_hooks_active);

        emu.add_watchpoint(0x6000, WatchType::Write);
        assert!(emu.debug_hooks_active);
        emu.remove_watchpoint(0x6000, 0x6000, WatchType::Write);
        assert!(!emu.debug_hooks_active);

        emu.set_event_breakpoint(DebugEvent::Interrupt, true);
        assert!(emu.debug_hooks_active);
        emu.set_event_breakpoint(DebugEvent::Interrupt, false);
        assert!(!emu.debug_hooks_active);

        emu.set_opcode_log_enabled(true);
        assert!(emu.debug_hooks_active);
        emu.set_opcode_log_enabled(false);
        assert!(!emu.debug_hooks_active);

        emu.set_instruction_trace_enabled(true);
        assert!(emu.debug_hooks_active);
        emu.set_instruction_trace_enabled(false);
        assert!(!emu.debug_hooks_active);
    }
}
