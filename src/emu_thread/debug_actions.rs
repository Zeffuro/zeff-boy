use crate::debug::DebugUiActions;
use crate::emu_core_trait::DebuggableEmulator;
use zeff_emu_common::debug::WatchType;

use super::EmuThread;

enum DebugAction {
    AddBreakpoint(u16),
    AddWatchpoint(u16, WatchType),
    RemoveBreakpoint(u16),
    ToggleBreakpoint(u16),
    WriteMemory(u16, u8),
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

impl EmuThread {
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
}
