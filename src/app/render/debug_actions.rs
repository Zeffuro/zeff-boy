use super::App;
use crate::debug::DebugUiActions;
use crate::emu_thread::{EmuCommand, TasControlCommandKind};

impl App {
    pub(in crate::app) fn merge_debug_actions(&mut self, actions: DebugUiActions) {
        self.merge_debug_actions_in_order(actions, false);
    }

    pub(in crate::app) fn merge_deferred_debug_actions(&mut self, actions: DebugUiActions) {
        self.merge_debug_actions_in_order(actions, true);
    }

    fn merge_debug_actions_in_order(&mut self, mut actions: DebugUiActions, deferred: bool) {
        if actions.target != self.debug_windows.debug_target
            || actions.target_revision.is_some_and(|revision| {
                revision == u64::MAX || revision != self.debug_windows.debug_revision
            })
            || self.debug_windows.debug_revision == u64::MAX
                && actions.target != crate::debug::DebugTarget::Primary
        {
            return;
        }
        if let Some(target) = actions.select_target {
            if target == crate::debug::DebugTarget::Primary
                || self.active_system == crate::emu_backend::ActiveSystem::Md
            {
                self.debug_windows.select_debug_target(target);
            }
            return;
        }
        if actions.target == crate::debug::DebugTarget::Sound {
            let sound_trace = std::mem::take(&mut actions.sound_trace);
            let trace_enabled = actions.trace_enabled.take();
            let trace_capacity = actions.trace_capacity.take();
            let trace_clear = std::mem::take(&mut actions.trace_clear);
            if actions.requires_emulator_authority() {
                self.toast_manager
                    .info("SOUND execution controls are unavailable");
                return;
            }
            actions.sound_trace = sound_trace;
            actions.trace_enabled = trace_enabled;
            actions.trace_capacity = trace_capacity;
            actions.trace_clear = trace_clear;
        }
        let mut authority_allowed = true;
        if actions.requires_emulator_authority()
            && let Err(error) =
                self.preflight_emu_command_kind(TasControlCommandKind::DebuggerMutation)
        {
            self.toast_manager.error(error.to_string());
            if actions.md_timed_ports.is_some() {
                self.debug_windows.md_timed_ports.refused(error.to_string());
            }
            authority_allowed = false;
        }

        if authority_allowed
            && let Some(request) = actions.md_timed_ports.take()
            && let Err(error) = self.debug_windows.md_timed_ports.queue(request)
        {
            self.debug_windows.md_timed_ports.refused(error);
        }
        if authority_allowed
            && !deferred
            && self.debug_windows.md_timed_ports.ordering_pending()
            && actions.requires_emulator_authority()
        {
            self.debug_windows.md_timed_ports.defer(actions);
            return;
        }
        if authority_allowed && self.core_supports_guest_calls() {
            if let Some(request) = actions.guest_call.clone()
                && let Err(error) =
                    self.send_emu_command_checked(EmuCommand::ExecuteGuestCall(request))
            {
                self.toast_manager.error(error.to_string());
                authority_allowed = false;
            }
            if authority_allowed
                && let Some(state) = actions.undo_guest_call.clone()
                && let Err(error) = self.send_emu_command_checked(EmuCommand::UndoGuestCall(state))
            {
                self.toast_manager.error(error.to_string());
                authority_allowed = false;
            }
        }

        if authority_allowed {
            crate::ui::apply_debug_actions(
                &actions,
                &mut self.debug_requests.step,
                &mut self.debug_requests.next_frame,
                &mut self.debug_requests.continue_,
                &mut self.debug_requests.backstep,
            );
            self.merge_authorized_debug_mutations(&actions);
        }
        self.apply_local_debug_actions(actions);
    }

    fn merge_authorized_debug_mutations(&mut self, actions: &DebugUiActions) {
        let pending = &mut self.pending_debug_actions;
        if actions.requires_emulator_authority() {
            if pending.target != actions.target
                || pending.target_revision != actions.target_revision
            {
                let mut sound_trace = std::mem::take(&mut pending.sound_trace);
                sound_trace.watch = None;
                *pending = DebugUiActions {
                    sound_trace,
                    trace_enabled: pending.trace_enabled,
                    trace_clear: pending.trace_clear,
                    trace_capacity: pending.trace_capacity,
                    ..DebugUiActions::none()
                };
            }
            pending.target = actions.target;
            pending.target_revision = actions.target_revision;
        }
        if actions.sound_trace.watch.is_some() {
            pending.sound_trace.watch = actions.sound_trace.watch;
        }
        if actions.sound_trace.enabled.is_some() {
            pending.sound_trace.enabled = actions.sound_trace.enabled;
        }
        if actions.sound_trace.capacity.is_some() {
            pending.sound_trace.capacity = actions.sound_trace.capacity;
        }
        pending.sound_trace.clear |= actions.sound_trace.clear;
        if actions.add_breakpoint.is_some() {
            pending.add_breakpoint = actions.add_breakpoint;
        }
        if actions.add_one_shot_breakpoint.is_some() {
            pending.add_one_shot_breakpoint = actions.add_one_shot_breakpoint;
        }
        if actions.add_breakpoint_after.is_some() {
            pending.add_breakpoint_after = actions.add_breakpoint_after;
        }
        pending
            .event_breakpoint_changes
            .extend(actions.event_breakpoint_changes.iter().copied());
        if actions.add_watchpoint.is_some() {
            pending.add_watchpoint = actions.add_watchpoint;
        }
        pending
            .remove_watchpoints
            .extend(actions.remove_watchpoints.iter().copied());
        let bp_changed = !actions.remove_breakpoints.is_empty()
            || !actions.toggle_breakpoints.is_empty()
            || !actions.add_rom_breakpoints.is_empty()
            || !actions.remove_rom_breakpoints.is_empty()
            || !actions.toggle_rom_breakpoints.is_empty();
        pending
            .remove_breakpoints
            .extend(actions.remove_breakpoints.iter().copied());
        pending
            .toggle_breakpoints
            .extend(actions.toggle_breakpoints.iter().copied());
        pending
            .remove_rom_breakpoints
            .extend(actions.remove_rom_breakpoints.iter().copied());
        pending
            .add_rom_breakpoints
            .extend(actions.add_rom_breakpoints.iter().copied());
        pending
            .toggle_rom_breakpoints
            .extend(actions.toggle_rom_breakpoints.iter().copied());
        if bp_changed
            || actions.add_breakpoint.is_some()
            || actions.add_one_shot_breakpoint.is_some()
            || actions.add_breakpoint_after.is_some()
        {
            self.debug_windows.last_disasm_pc = None;
            self.debug_windows.last_disasm_mapping = None;
        }
        pending
            .memory_writes
            .extend(actions.memory_writes.iter().copied());
        if actions.apu_channel_mutes.is_some() {
            pending
                .apu_channel_mutes
                .clone_from(&actions.apu_channel_mutes);
        }
        if actions.layer_toggles.is_some() {
            pending.layer_toggles = actions.layer_toggles;
        }
        if actions.gba_bg_layer_toggles.is_some() {
            pending.gba_bg_layer_toggles = actions.gba_bg_layer_toggles;
        }
        if actions.trace_enabled.is_some() {
            pending.trace_enabled = actions.trace_enabled;
        }
        if actions.trace_clear {
            self.debug_windows.execution_coverage.clear();
        }
        pending.trace_clear |= actions.trace_clear;
        if actions.trace_capacity.is_some() {
            pending.trace_capacity = actions.trace_capacity;
        }
    }

    fn apply_local_debug_actions(&mut self, actions: DebugUiActions) {
        #[cfg(not(target_arch = "wasm32"))]
        if actions.open_audio_explorer {
            super::request_native_tool_window(
                &mut self.show_audio_explorer,
                &mut self.focus_audio_explorer_pending,
            );
        }
        let mut symbol_changed = false;
        if self.debug_windows.debug_target == crate::debug::DebugTarget::Sound
            && (actions.user_symbol.is_some() || !actions.remove_user_symbols.is_empty())
        {
            self.toast_manager.info("SOUND symbols are read-only");
            return;
        }
        for name in &actions.remove_user_symbols {
            match self.symbols.remove_user_symbol(name) {
                Ok(Some(path)) => self
                    .toast_manager
                    .success(format!("Removed {name} from {}", path.display())),
                Ok(None) => self.toast_manager.success(format!("Removed {name}")),
                Err(error) => self
                    .toast_manager
                    .error(format!("Could not remove {name}: {error}")),
            }
            symbol_changed = true;
        }
        if let Some(symbol) = actions.user_symbol {
            let name = symbol.name.clone();
            match self.symbols.upsert_user_symbol(symbol) {
                Ok(Some(path)) => self
                    .toast_manager
                    .success(format!("Saved {name} to {}", path.display())),
                Ok(None) => self.toast_manager.success(format!("Added {name}")),
                Err(error) => self
                    .toast_manager
                    .error(format!("Could not add {name}: {error}")),
            }
            symbol_changed = true;
        }
        if symbol_changed {
            self.debug_windows.last_disasm_pc = None;
            self.debug_windows.last_disasm_mapping = None;
        }
        if let Some(address) = actions.memory_target
            && self.debug_windows.debug_target == crate::debug::DebugTarget::Primary
        {
            let memory = &mut self.debug_windows.memory;
            memory.view_start = memory.address_space.clamp_start(address);
            memory.jump_input = memory.address_space.format(memory.view_start);
        }
        if let Some(target) = actions.disasm_target {
            self.navigate_disassembly(Some(target));
        } else if actions.follow_disasm_pc {
            self.navigate_disassembly(None);
        } else if actions.disasm_back {
            self.navigate_disassembly_history(true);
        } else if actions.disasm_forward {
            self.navigate_disassembly_history(false);
        }
    }

    fn navigate_disassembly(&mut self, target: Option<crate::debug::DisassemblyTarget>) {
        if self.debug_windows.debug_target == crate::debug::DebugTarget::Sound
            && target.is_some_and(|target| {
                target.as_cpu().is_none_or(|target| {
                    target.cpu_address > 0xffff
                        || target.storage_offset.is_some()
                        || target.bank.is_some()
                        || target.thumb.is_some()
                })
            })
        {
            return;
        }
        self.debug_windows.navigate_disassembly(target);
    }

    fn navigate_disassembly_history(&mut self, back: bool) {
        self.debug_windows.navigate_disassembly_history(back);
    }
}
