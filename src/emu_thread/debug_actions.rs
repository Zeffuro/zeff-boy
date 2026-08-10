use crate::debug::DebugUiActions;
use crate::emu_core_trait::DebuggableEmulator;
use zeff_emu_common::address::Address;
use zeff_emu_common::debug::WatchType;

use super::EmuThread;

enum DebugAction {
    AddBreakpoint(Address),
    AddWatchpoint(Address, WatchType),
    RemoveBreakpoint(Address),
    ToggleBreakpoint(Address),
    WriteMemory(Address, u8),
}

fn collect_debug_actions(actions: &DebugUiActions) -> impl Iterator<Item = DebugAction> + '_ {
    actions
        .add_breakpoint
        .iter()
        .map(|&addr| DebugAction::AddBreakpoint(addr))
        .chain(
            actions
                .add_watchpoint
                .iter()
                .map(|&(addr, wt)| DebugAction::AddWatchpoint(addr, wt)),
        )
        .chain(
            actions
                .remove_breakpoints
                .iter()
                .map(|&addr| DebugAction::RemoveBreakpoint(addr)),
        )
        .chain(
            actions
                .toggle_breakpoints
                .iter()
                .map(|&addr| DebugAction::ToggleBreakpoint(addr)),
        )
        .chain(
            actions
                .memory_writes
                .iter()
                .map(|&(addr, value)| DebugAction::WriteMemory(addr, value)),
        )
}

fn apply_debug_actions_to(emu: &mut impl DebuggableEmulator, actions: &DebugUiActions) {
    for action in collect_debug_actions(actions) {
        match action {
            DebugAction::AddBreakpoint(addr) => emu.add_breakpoint(addr),
            DebugAction::AddWatchpoint(addr, wt) => emu.add_watchpoint(addr, wt),
            DebugAction::RemoveBreakpoint(addr) => emu.remove_breakpoint(addr),
            DebugAction::ToggleBreakpoint(addr) => emu.toggle_breakpoint(addr),
            DebugAction::WriteMemory(addr, val) => emu.debug_write(addr, val),
        }
    }
}

fn apply_debug_controls_to(
    emu: &mut impl DebuggableEmulator,
    opcode_log_enabled: bool,
    debug_continue: bool,
    debug_step: bool,
) {
    emu.set_opcode_log_enabled(opcode_log_enabled);
    if emu.is_cpu_suspended() {
        if debug_continue {
            emu.debug_continue();
        } else if debug_step {
            emu.debug_step();
        }
    }
}

impl EmuThread {
    pub(crate) fn apply_debug_controls(
        emu: &mut impl DebuggableEmulator,
        opcode_log_enabled: bool,
        debug_continue: bool,
        debug_step: bool,
    ) {
        apply_debug_controls_to(emu, opcode_log_enabled, debug_continue, debug_step);
    }

    pub(crate) fn apply_debug_actions(
        emu: &mut zeff_gb_core::emulator::Emulator,
        actions: &DebugUiActions,
    ) {
        apply_debug_actions_to(emu, actions);
        if let Some((bg, win, sprites)) = actions.layer_toggles {
            emu.set_ppu_debug_flags(bg, win, sprites);
        }
    }

    pub(crate) fn apply_nes_debug_actions(
        emu: &mut zeff_nes_core::emulator::Emulator,
        actions: &DebugUiActions,
    ) {
        apply_debug_actions_to(emu, actions);
    }

    pub(crate) fn apply_gba_debug_actions(
        emu: &mut zeff_gba_core::emulator::Emulator,
        actions: &DebugUiActions,
    ) {
        apply_debug_actions_to(emu, actions);
        if let Some((bg, win, sprites)) = actions.layer_toggles {
            emu.set_ppu_debug_flags(bg, win, sprites);
        }
        if let Some(layers) = actions.gba_bg_layer_toggles {
            emu.set_ppu_debug_bg_layers(layers);
        }
    }

    pub(crate) fn apply_ws_debug_actions(
        emu: &mut zeff_ws_core::emulator::Emulator,
        actions: &DebugUiActions,
    ) {
        apply_debug_actions_to(emu, actions);
    }

    pub(crate) fn apply_sega8_debug_actions(
        emu: &mut zeff_sega8_core::emulator::Emulator,
        actions: &DebugUiActions,
    ) {
        apply_debug_actions_to(emu, actions);
    }
}
