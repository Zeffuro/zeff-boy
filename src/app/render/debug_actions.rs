use super::App;
use crate::debug::DebugUiActions;
use crate::emu_thread::{EmuCommand, TasControlCommandKind};

impl App {
    pub(super) fn merge_debug_actions(&mut self, actions: DebugUiActions) {
        let mut authority_allowed = true;
        if actions.requires_emulator_authority()
            && let Err(error) =
                self.preflight_emu_command_kind(TasControlCommandKind::DebuggerMutation)
        {
            self.toast_manager.error(error.to_string());
            authority_allowed = false;
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
        let mut symbol_changed = false;
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
        if let Some(address) = actions.memory_target {
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
        if self.debug_windows.disasm_target == target {
            return;
        }
        self.debug_windows
            .disasm_back
            .push(self.debug_windows.disasm_target);
        self.debug_windows.disasm_forward.clear();
        self.debug_windows.disasm_target = target;
        self.debug_windows.last_disasm_pc = None;
        self.debug_windows.last_disasm_mapping = None;
    }

    fn navigate_disassembly_history(&mut self, back: bool) {
        let (from, to) = if back {
            (
                &mut self.debug_windows.disasm_back,
                &mut self.debug_windows.disasm_forward,
            )
        } else {
            (
                &mut self.debug_windows.disasm_forward,
                &mut self.debug_windows.disasm_back,
            )
        };
        let Some(target) = from.pop() else {
            return;
        };
        to.push(self.debug_windows.disasm_target);
        self.debug_windows.disasm_target = target;
        self.debug_windows.last_disasm_pc = None;
        self.debug_windows.last_disasm_mapping = None;
    }
}
