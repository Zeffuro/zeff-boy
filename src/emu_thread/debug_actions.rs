use crate::debug::DebugUiActions;

use super::EmuThread;

impl EmuThread {
    pub(crate) fn apply_debug_actions(
        emu: &mut zeff_gb_core::emulator::Emulator,
        actions: &DebugUiActions,
    ) {
        if let Some(addr) = actions.add_breakpoint {
            emu.add_breakpoint(addr);
        }
        if let Some((addr, watch_type)) = actions.add_watchpoint {
            emu.add_watchpoint(addr, watch_type);
        }
        for addr in &actions.remove_breakpoints {
            emu.remove_breakpoint(*addr);
        }
        for addr in &actions.toggle_breakpoints {
            emu.toggle_breakpoint(*addr);
        }
        for (addr, value) in &actions.memory_writes {
            emu.write_byte(*addr, *value);
        }
        if let Some((bg, win, sprites)) = actions.layer_toggles {
            emu.set_ppu_debug_flags(bg, win, sprites);
        }
    }

    pub(crate) fn apply_nes_debug_actions(
        emu: &mut zeff_nes_core::emulator::Emulator,
        actions: &DebugUiActions,
    ) {
        if let Some(addr) = actions.add_breakpoint {
            emu.add_breakpoint(addr);
        }
        if let Some((addr, watch_type)) = actions.add_watchpoint {
            emu.add_watchpoint(addr, watch_type);
        }
        for addr in &actions.remove_breakpoints {
            emu.remove_breakpoint(*addr);
        }
        for addr in &actions.toggle_breakpoints {
            emu.toggle_breakpoint(*addr);
        }
        for (addr, value) in &actions.memory_writes {
            emu.cpu_write(*addr, *value);
        }
    }
}
