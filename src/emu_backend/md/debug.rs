use zeff_emu_common::debug::AddressDebugController;
use zeff_emu_common::debug::{DebugEvent, OpcodeLog};
use zeff_emu_common::time::MachineTiming;
use zeff_m68k::timing::TimedOutcome;
use zeff_md_core::emulator::MemoryAccess;
use zeff_md_core::{Emulator, MachineStep};

use super::{MdBackend, MdExecutionError};
use crate::debug::DebugUiActions;

mod instruction_trace;
mod memory_write;
mod rom_breakpoints;
mod trace_stream;
mod watchpoints;

const SUPPORTED_EVENTS: [DebugEvent; 3] = [
    DebugEvent::Interrupt,
    DebugEvent::Dma,
    DebugEvent::Exception,
];

fn debug_event(outcome: TimedOutcome) -> Option<DebugEvent> {
    match outcome {
        TimedOutcome::Interrupt { .. } => Some(DebugEvent::Interrupt),
        TimedOutcome::Instruction(step) if step.exception.is_some() => Some(DebugEvent::Exception),
        TimedOutcome::Trace { .. } => Some(DebugEvent::Exception),
        TimedOutcome::Instruction(_) | TimedOutcome::Stopped => None,
    }
}

#[derive(Clone, Default)]
pub(super) struct PcBreakpoints {
    controller: AddressDebugController,
    rom: rom_breakpoints::RomBreakpoints,
    resume_boundary: Option<u32>,
    history: OpcodeLog<CompletedOpcode>,
    watch_hit: Option<MemoryAccess>,
    trace: zeff_emu_common::debug::InstructionTraceStore,
}

#[derive(Clone, Copy, Default)]
struct CompletedOpcode {
    pc: u32,
    opcode: u16,
    clocks: u64,
    exception: Option<u8>,
    storage_offset: Option<u64>,
}

impl PcBreakpoints {
    pub(super) fn frozen_observation(&self) -> anyhow::Result<[u64; 9]> {
        anyhow::ensure!(self.rollback_idle(), "MD debugger is active");
        let capacities = self.controller.retained_capacities();
        let (policy, capacity, start, sequence) = self
            .trace
            .inactive_observation()
            .ok_or_else(|| anyhow::anyhow!("MD trace is active"))?;
        Ok([
            capacities[0] as u64,
            capacities[1] as u64,
            capacities[2] as u64,
            capacities[3] as u64,
            self.rom.offsets.capacity() as u64,
            policy as u64,
            capacity as u64,
            start as u64,
            sequence,
        ])
    }

    pub(super) fn rollback_idle(&self) -> bool {
        !self.armed()
            && self.controller.iter_event_breakpoints().next().is_none()
            && !self.controller.break_on_next
            && self.resume_boundary.is_none()
            && self.watch_hit.is_none()
            && self.rom.hit.is_none()
            && self.controller.hit_breakpoint.is_none()
            && self.controller.hit_event.is_none()
            && self.controller.hit_watchpoint.is_none()
            && self.controller.iter_one_shot_breakpoints().next().is_none()
            && self
                .controller
                .iter_breakpoint_hit_conditions()
                .next()
                .is_none()
            && !self.history.enabled
            && self.history.recent(1).is_empty()
            && !self.trace.is_enabled()
            && self.trace.is_empty()
    }

    pub(super) fn clear_stop(&mut self) {
        self.controller.clear_hits();
        self.resume_boundary = None;
        self.watch_hit = None;
        self.rom.hit = None;
    }

    pub(super) fn clear_session(&mut self) {
        self.clear_stop();
        self.history.clear();
        self.trace.clear();
        let conditions: Vec<_> = self.controller.iter_breakpoint_hit_conditions().collect();
        for condition in conditions {
            self.controller
                .add_breakpoint_after(condition.address, condition.target_hits);
        }
    }

    pub(super) fn resume_hit(&mut self) {
        self.resume_boundary = self.controller.hit_breakpoint;
        self.controller.clear_hits();
        self.watch_hit = None;
        self.rom.hit = None;
    }

    pub(super) fn check_dma(&mut self, before: u64, emu: &Emulator) -> bool {
        before != emu.debug_dma_activity_epoch() && self.controller.check_event(DebugEvent::Dma)
    }

    fn armed(&self) -> bool {
        !self.rom.offsets.is_empty()
            || self.controller.iter_breakpoints().next().is_some()
            || !self.controller.watchpoints.is_empty()
            || SUPPORTED_EVENTS
                .into_iter()
                .any(|event| self.controller.has_event_breakpoint(event))
    }

    pub(super) fn should_stop(&mut self, emu: &Emulator) -> bool {
        let pc = emu.registers().pc & 0xff_ffff;
        let skip = self.resume_boundary.take() == Some(pc);
        // STOP polls do not visit another instruction boundary.
        !emu.main_cpu_stopped() && !skip && self.controller.should_break(pc)
    }
}

fn valid_pc(address: u32) -> bool {
    address <= 0xff_ffff && address & 1 == 0
}

impl MdBackend {
    pub(in crate::emu_backend) fn set_opcode_history_enabled(&mut self, enabled: bool) {
        if self.rollback_owned() {
            return;
        }
        self.debugger.history.set_enabled(enabled);
        if let Ok(core) = self.same_branch_core_mut() {
            core.set_call_stack_recording(enabled);
        }
    }

    fn apply_sound_trace_actions(&mut self, actions: &crate::debug::SoundTraceActions) {
        if self.rollback_owned() {
            return;
        }
        if let Some(capacity) = actions.capacity {
            self.sound_trace.set_capacity(capacity);
        }
        if actions.clear {
            self.sound_trace.clear();
        }
        if let Some(enabled) = actions.enabled
            && (!enabled || self.sound_trace_rejection().is_none())
        {
            self.sound_trace.set_enabled(enabled);
        }
    }

    pub(in crate::emu_backend) fn apply_debug_actions(&mut self, actions: &DebugUiActions) {
        if self.rollback_owned() {
            return;
        }
        self.apply_sound_trace_actions(&actions.sound_trace);
        if let Some(enabled) = actions.trace_enabled {
            self.debugger
                .trace
                .set_enabled(enabled && self.trace_stream.is_live());
        }
        if let Some(capacity) = actions.trace_capacity {
            self.debugger.trace.set_capacity(capacity);
        }
        if actions.trace_clear {
            self.clear_instruction_trace();
        }
        if actions.target != crate::debug::DebugTarget::Primary {
            return;
        }
        let controller = &mut self.debugger.controller;
        if let Some(address) = actions.add_breakpoint.filter(|&address| valid_pc(address)) {
            controller.add_breakpoint(address);
        }
        if let Some(address) = actions
            .add_one_shot_breakpoint
            .filter(|&address| valid_pc(address))
        {
            controller.add_one_shot_breakpoint(address);
        }
        if let Some((address, hits)) = actions
            .add_breakpoint_after
            .filter(|&(address, _)| valid_pc(address))
        {
            controller.add_breakpoint_after(address, hits);
        }
        for &address in actions
            .remove_breakpoints
            .iter()
            .filter(|&&address| valid_pc(address))
        {
            controller.remove_breakpoint(address);
        }
        for &address in actions
            .toggle_breakpoints
            .iter()
            .filter(|&&address| valid_pc(address))
        {
            controller.toggle_breakpoint(address);
        }
        self.apply_rom_breakpoint_actions(actions);
        self.apply_watchpoint_actions(actions);
        for &(event, enabled) in &actions.event_breakpoint_changes {
            if SUPPORTED_EVENTS.contains(&event) {
                self.debugger
                    .controller
                    .set_event_breakpoint(event, enabled);
            }
        }
    }

    pub(super) fn step_debug_field(&mut self) -> Result<(), MdExecutionError> {
        self.require_normal()?;
        if !self.debugger.armed()
            && !self.debugger.history.enabled
            && !self.debugger.trace.is_enabled()
            && !self.sound_trace.watches_armed()
        {
            self.debugger.resume_boundary = None;
            return self.field_with_sound();
        }
        if self.debugger.should_stop(self.machine.core()) {
            self.suspended = true;
            return Ok(());
        }
        let field = self.machine.core().pacing_field_count();
        self.same_branch_core_mut()?.clear_frame_ready();
        loop {
            self.step_debug_instruction()?;
            if self.suspended {
                return Ok(());
            }
            if self.machine.core().pacing_field_count() != field {
                return Ok(());
            }
            if self.debugger.should_stop(self.machine.core()) {
                self.suspended = true;
                return Ok(());
            }
        }
    }

    pub(super) fn step_debug_instruction(&mut self) -> Result<MachineStep, MdExecutionError> {
        self.require_normal()?;
        let pc = self.machine.core().registers().pc & 0xff_ffff;
        let dma_before = self.machine.core().debug_dma_activity_epoch();
        let (result, storage_offset) = if self.debugger.trace.is_enabled()
            || self.debugger.history.enabled
            || !self.debugger.rom.offsets.is_empty()
        {
            self.step_captured_instruction()
        } else if self.debugger.controller.watchpoints.is_empty() {
            (self.step_with_sound(), None)
        } else {
            let debugger = &mut self.debugger;
            let core = self.machine.normal_mut()?;
            let mut observer = |access| debugger.observe(access);
            let result = match self.sound_trace.attachment() {
                Some(mut sound) => core.step_instruction_observers(&mut observer, &mut sound),
                None => core.step_instruction_observed(&mut observer),
            };
            (result.map_err(Into::into), None)
        };
        self.finish_sound_watch_boundary(result.is_err());
        if self.debugger.watch_hit.is_some() {
            self.suspended = true;
        }
        let result = result?;
        let cpu_hit = debug_event(result.cpu)
            .is_some_and(|event| self.debugger.controller.check_event(event));
        if cpu_hit || self.debugger.check_dma(dma_before, self.machine.core()) {
            self.suspended = true;
        }
        if let TimedOutcome::Instruction(step) = result.cpu {
            if self.debugger.rom.check(storage_offset) {
                self.suspended = true;
            }
            self.debugger.history.push(CompletedOpcode {
                pc,
                opcode: step.opcode,
                clocks: step.clocks,
                exception: step.exception,
                storage_offset,
            });
        }
        Ok(result)
    }

    pub(crate) fn overlay_pc_breakpoints(&self, data: &mut crate::ui::UiFrameData) {
        if data.debug_target == crate::debug::DebugTarget::Sound {
            return;
        }
        let controller = &self.debugger.controller;
        if let Some(view) = &mut data.md_timed_ports {
            view.can_submit = !self.rollback_owned()
                && self.suspended
                && !self.execution_faulted
                && self.runtime_fault.is_none();
        }
        if let Some(cpu) = &mut data.cpu_debug {
            cpu.rom_breakpoints = self.debugger.rom.offsets.clone();
            cpu.hit_rom_breakpoint = self.debugger.rom.hit;
            cpu.breakpoints = controller.iter_breakpoints().collect();
            cpu.one_shot_breakpoints = controller.iter_one_shot_breakpoints().collect();
            cpu.hit_breakpoint = controller.hit_breakpoint;
            cpu.breakpoint_hit_conditions = controller.iter_breakpoint_hit_conditions().collect();
            cpu.supported_events = SUPPORTED_EVENTS.to_vec();
            cpu.event_breakpoints = controller.iter_event_breakpoints().collect();
            cpu.hit_event = controller.hit_event;
            self.debugger.overlay_watchpoints(cpu);
            cpu.recent_opcodes = self
                .debugger
                .history
                .recent(32)
                .into_iter()
                .map(|entry| crate::debug::RecentOpcodeDisplay {
                    address: entry.pc,
                    storage_offset: entry.storage_offset,
                    bytes: entry.opcode.to_be_bytes().to_vec(),
                    detail: Some(match entry.exception {
                        Some(vector) => format!("{} CPU clocks; exception {vector}", entry.clocks),
                        None => format!("{} CPU clocks", entry.clocks),
                    }),
                    repeat_count: 1,
                    thumb: None,
                })
                .collect();
            cpu.last_opcode_line = cpu.recent_opcodes.first().map_or_else(
                || "No completed opcode captured".into(),
                crate::debug::RecentOpcodeDisplay::line,
            );
        }
        if let Some(disassembly) = &mut data.disassembly_view {
            disassembly.rom_breakpoints = self.debugger.rom.offsets.clone();
            disassembly.hit_rom_breakpoint = self.debugger.rom.hit;
            disassembly.breakpoints = controller.iter_breakpoints().collect();
            disassembly.one_shot_breakpoints = controller.iter_one_shot_breakpoints().collect();
        }
    }
}

impl crate::emu_backend::EmuBackend {
    pub(crate) fn md_execution_time(&self) -> Option<u64> {
        match self {
            Self::Md(md) => Some(md.core().timing_snapshot().now().get()),
            _ => None,
        }
    }
}
