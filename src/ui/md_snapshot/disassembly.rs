use crate::debug::{DisassemblyTarget, DisassemblyView};
use zeff_md_core::Emulator;

mod sound;
mod storage;

pub(super) fn snapshot_target(
    debug_target: crate::debug::DebugTarget,
    emu: &Emulator,
    show: bool,
    target: Option<DisassemblyTarget>,
) -> Option<DisassemblyView> {
    match debug_target {
        crate::debug::DebugTarget::Primary => snapshot(emu, show, target),
        crate::debug::DebugTarget::Sound => sound::snapshot(emu, show, target),
    }
}

const LINES: usize = 26;

pub(super) fn snapshot(
    emu: &Emulator,
    show: bool,
    target: Option<DisassemblyTarget>,
) -> Option<DisassemblyView> {
    show.then(|| {
        if let Some(DisassemblyTarget::RomStorage(offset)) = target {
            return storage::snapshot(emu.rom_bytes(), offset);
        }
        let target = target.and_then(DisassemblyTarget::as_cpu).filter(|target| {
            target.cpu_address <= 0xff_ffff
                && target.cpu_address & 1 == 0
                && target.storage_offset.is_none()
                && target.bank.is_none()
                && target.thumb.is_none()
        });
        let lines = crate::debug::m68k_disassemble_forward(
            |address| emu.peek(address),
            target.map_or_else(|| emu.registers().pc, |target| target.cpu_address),
            LINES,
        );
        DisassemblyView {
            cursor: crate::debug::DisassemblyCursor::Cpu(lines[0].address),
            mapping: None,
            is_navigation_target: target.is_some(),
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
