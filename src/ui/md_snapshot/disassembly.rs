use crate::debug::DisassemblyView;
use zeff_md_core::Emulator;

const LINES: usize = 26;

pub(super) fn snapshot(emu: &Emulator, show: bool) -> Option<DisassemblyView> {
    show.then(|| {
        let lines = crate::debug::m68k_disassemble_forward(
            |address| emu.peek(address),
            emu.registers().pc,
            LINES,
        );
        DisassemblyView {
            pc: lines[0].address,
            mapping: None,
            is_navigation_target: false,
            is_static_target: false,
            location_symbol: None,
            lines,
            breakpoints: Vec::new(),
            one_shot_breakpoints: Vec::new(),
            rom_breakpoints: Vec::new(),
            hit_rom_breakpoint: None,
        }
    })
}

#[cfg(test)]
mod tests;
