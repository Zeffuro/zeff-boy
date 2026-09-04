use super::*;

impl PceMachine {
    #[inline]
    pub fn debug_snapshot(&self) -> PceCpuDebugSnapshot {
        let registers = self.cpu.cpu().registers();
        let on_chip_io = self.cpu.on_chip_io();
        PceCpuDebugSnapshot {
            registers,
            mapping_registers: self.cpu.cpu().mapping_registers(),
            physical_pc: self.cpu.cpu().logical_to_physical(registers.pc),
            speed_mode: self.cpu.cpu().speed_mode(),
            timer_counter: on_chip_io.read_timer_counter(),
            timer_reload: on_chip_io.timer_reload(),
            timer_running: on_chip_io.timer_running(),
            timer_prescaler_ticks: on_chip_io.timer_prescaler_ticks(),
            irq_disable: on_chip_io.read_irq(super::super::cpu::IrqPort::Disable),
            irq_request: on_chip_io.read_irq(super::super::cpu::IrqPort::Request),
            sampled_interrupt: self.cpu.sampled_interrupt(),
            master_ticks: self.master_ticks,
            vce_line_index: self.vce_line_index,
            faulted: self.faulted,
            execution_state: self.execution_state,
        }
    }

    #[inline]
    pub const fn execution_state(&self) -> PceExecutionState {
        self.execution_state
    }

    #[inline]
    pub const fn is_cpu_suspended(&self) -> bool {
        matches!(self.execution_state, PceExecutionState::Suspended)
    }

    pub fn debug_continue(&mut self) {
        self.skip_breakpoint_once = self.debug.hit_breakpoint.is_some();
        self.debug.clear_hits();
        self.execution_state = PceExecutionState::Running;
        self.suspend_after_instruction = false;
    }

    pub fn debug_step(&mut self) {
        self.debug.clear_hits();
        self.execution_state = PceExecutionState::Running;
        self.suspend_after_instruction = true;
        self.skip_breakpoint_once = false;
    }

    pub fn debug_suspend(&mut self) {
        self.execution_state = PceExecutionState::Suspended;
        self.suspend_after_instruction = false;
        self.skip_breakpoint_once = false;
    }

    pub fn debug_execute_guest_call(
        &mut self,
        target: u16,
        instruction_budget: u64,
    ) -> Result<u64, String> {
        if !self.is_cpu_suspended() {
            return Err("CPU must be suspended".to_owned());
        }
        if self.faulted {
            return Err("machine is faulted".to_owned());
        }

        let registers = self.cpu.cpu().registers();
        if target == registers.pc || instruction_budget == 0 {
            return Err("invalid call target or budget".to_owned());
        }

        let return_pc = registers.pc;
        let return_sp = registers.sp;
        let saved_interrupt_disable = registers.status.contains(StatusFlags::INTERRUPT);
        let saved_sampled_interrupt = self.cpu.replace_sampled_interrupt(None);
        let return_address = return_pc.wrapping_sub(1);
        self.cpu.debug_write_logical(
            &mut self.bus,
            0x2100 | u16::from(return_sp),
            (return_address >> 8) as u8,
        );
        self.cpu.cpu_mut().registers_mut().sp = return_sp.wrapping_sub(1);
        self.cpu.debug_write_logical(
            &mut self.bus,
            0x2100 | u16::from(return_sp.wrapping_sub(1)),
            return_address as u8,
        );
        let registers = self.cpu.cpu_mut().registers_mut();
        registers.sp = return_sp.wrapping_sub(2);
        registers.pc = target;
        registers.status.insert(StatusFlags::INTERRUPT);
        self.debug.clear_hits();
        self.execution_state = PceExecutionState::Running;
        self.suspend_after_instruction = false;
        self.skip_breakpoint_once = false;

        for instructions in 1..=instruction_budget {
            self.cpu.replace_sampled_interrupt(None);
            self.step_boundary_faulting()
                .map_err(|error| error.to_string())?;

            let registers = self.cpu.cpu().registers();
            if registers.pc == return_pc && registers.sp == return_sp {
                self.cpu
                    .cpu_mut()
                    .registers_mut()
                    .status
                    .set(StatusFlags::INTERRUPT, saved_interrupt_disable);
                self.cpu.replace_sampled_interrupt(saved_sampled_interrupt);
                self.execution_state = PceExecutionState::Suspended;
                return Ok(instructions);
            }
            if self.is_cpu_suspended() {
                return Err("call hit a debugger stop".to_owned());
            }
        }

        self.execution_state = PceExecutionState::Suspended;
        Err("call exceeded its instruction budget".to_owned())
    }

    pub fn set_opcode_history_enabled(&mut self, enabled: bool) {
        self.opcode_history.enabled = enabled;
    }

    pub fn recent_opcodes(&self, count: usize) -> Vec<PceOpcodeHistoryEntry> {
        self.opcode_history.recent(count)
    }

    pub const fn instruction_trace(&self) -> &InstructionTraceStore {
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

    pub fn set_event_breakpoint(&mut self, event: DebugEvent, enabled: bool) {
        if matches!(event, DebugEvent::Interrupt | DebugEvent::Dma) {
            self.debug.set_event_breakpoint(event, enabled);
        }
    }

    pub fn iter_event_breakpoints(&self) -> impl Iterator<Item = DebugEvent> + '_ {
        self.debug.iter_event_breakpoints()
    }

    pub const fn debug_hit_event(&self) -> Option<DebugEvent> {
        self.debug.hit_event
    }

    pub fn add_breakpoint(&mut self, addr: u16) {
        self.debug.add_breakpoint(Address::from(addr));
    }

    pub fn add_one_shot_breakpoint(&mut self, addr: u16) {
        self.debug.add_one_shot_breakpoint(Address::from(addr));
    }

    pub fn add_breakpoint_after(&mut self, addr: u16, target_hits: u64) {
        self.debug
            .add_breakpoint_after(Address::from(addr), target_hits);
    }

    pub fn remove_breakpoint(&mut self, addr: u16) {
        self.debug.remove_breakpoint(Address::from(addr));
    }

    pub fn toggle_breakpoint(&mut self, addr: u16) {
        self.debug.toggle_breakpoint(Address::from(addr));
    }

    pub fn iter_breakpoints(&self) -> impl Iterator<Item = Address> + '_ {
        self.debug.iter_breakpoints()
    }

    pub fn iter_one_shot_breakpoints(&self) -> impl Iterator<Item = Address> + '_ {
        self.debug.iter_one_shot_breakpoints()
    }

    pub fn iter_breakpoint_hit_conditions(
        &self,
    ) -> impl Iterator<Item = BreakpointHitCondition> + '_ {
        self.debug.iter_breakpoint_hit_conditions()
    }

    pub fn add_watchpoint_range(&mut self, start: u16, end: u16, watch_type: WatchType) {
        self.debug
            .add_watchpoint_range(Address::from(start), Address::from(end), watch_type);
    }

    pub fn remove_watchpoint(&mut self, start: u16, end: u16, watch_type: WatchType) {
        self.debug
            .remove_watchpoint(Address::from(start), Address::from(end), watch_type);
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

    #[inline]
    pub fn debug_peek_cpu8(&self, logical_addr: u16) -> u8 {
        self.bus
            .peek(self.cpu.cpu().logical_to_physical(logical_addr))
    }

    pub fn debug_peek_physical8(&self, physical_addr: u32) -> u8 {
        self.bus
            .peek(physical_addr & super::super::cpu::PHYSICAL_ADDRESS_MASK)
    }

    pub fn debug_write_cpu8(&mut self, logical_addr: u16, value: u8) {
        let old_value = self.debug_peek_cpu8(logical_addr);
        self.cpu
            .debug_write_logical(&mut self.bus, logical_addr, value);
        self.debug
            .check_watch_write(Address::from(logical_addr), old_value, value);
        if self.debug.hit_watchpoint.is_some() {
            self.execution_state = PceExecutionState::Suspended;
            self.suspend_after_instruction = false;
        }
    }
}
