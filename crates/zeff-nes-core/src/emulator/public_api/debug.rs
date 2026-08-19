use crate::emulator::Emulator;
use crate::hardware::constants::STACK_BASE;
use crate::hardware::cpu::CpuState;
use zeff_emu_common::cheats::CheatByteTarget;

impl Emulator {
    pub fn set_opcode_log_enabled(&mut self, enabled: bool) {
        if self.opcode_log.enabled != enabled {
            self.call_stack.clear();
        }
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

    pub fn recent_opcodes(&self, n: usize) -> Vec<(u16, u8, Option<usize>)> {
        self.opcode_log.recent(n)
    }

    pub fn is_cpu_suspended(&self) -> bool {
        self.cpu.state == CpuState::Suspended
    }

    pub fn debug_continue(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.cpu.resume_from_debug();
    }

    pub fn debug_step(&mut self) {
        self.debug.clear_hits();
        self.debug.break_on_next = true;
        self.cpu.resume_from_debug();
    }

    pub fn debug_suspend(&mut self) {
        self.cpu.state = CpuState::Suspended;
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

    pub fn debug_execute_guest_call(
        &mut self,
        target: u16,
        instruction_budget: u64,
    ) -> Result<u64, String> {
        if self.cpu.state != CpuState::Suspended {
            return Err("CPU must be suspended".to_owned());
        }
        if self.cpu.is_jammed() {
            return Err("CPU is jammed".to_owned());
        }
        let return_pc = self.cpu.pc;
        if target == return_pc || instruction_budget == 0 {
            return Err("invalid call target or budget".to_owned());
        }
        let return_sp = self.cpu.sp;
        let saved_interrupt = self
            .cpu
            .regs
            .get_flag(crate::hardware::cpu::StatusFlags::INTERRUPT);
        let saved_nmi = self.cpu.nmi_pending;
        let saved_irq = self.cpu.irq_line;
        let return_addr = return_pc.wrapping_sub(1);
        self.bus.cpu_write(
            STACK_BASE | u16::from(self.cpu.sp),
            (return_addr >> 8) as u8,
        );
        self.cpu.sp = self.cpu.sp.wrapping_sub(1);
        self.bus
            .cpu_write(STACK_BASE | u16::from(self.cpu.sp), return_addr as u8);
        self.cpu.sp = self.cpu.sp.wrapping_sub(1);
        self.cpu.pc = target;
        self.cpu
            .regs
            .set_flag(crate::hardware::cpu::StatusFlags::INTERRUPT, true);
        self.debug.clear_hits();
        self.debug.break_on_next = false;
        self.cpu.state = CpuState::Running;

        for instructions in 1..=instruction_budget {
            self.cpu.nmi_pending = false;
            self.cpu.irq_line = false;
            self.step_instruction();
            if self.cpu.pc == return_pc && self.cpu.sp == return_sp {
                self.cpu.regs.set_flag(
                    crate::hardware::cpu::StatusFlags::INTERRUPT,
                    saved_interrupt,
                );
                self.cpu.nmi_pending = saved_nmi;
                self.cpu.irq_line = saved_irq;
                self.cpu.state = CpuState::Suspended;
                return Ok(instructions);
            }
            if self.cpu.state == CpuState::Suspended {
                return Err("call hit a debugger stop".to_owned());
            }
        }
        self.cpu.state = CpuState::Suspended;
        Err("call exceeded its instruction budget".to_owned())
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
