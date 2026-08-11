use crate::debug::z80_disassemble_around;
use crate::emu_thread::{ReusableBuffers, SnapshotRequest};
use zeff_emu_common::address::{Address, narrow_u16};
use zeff_sega8_core::emulator::Emulator;
use zeff_sega8_core::hardware::cartridge::Sega8System;

mod apu;
mod cpu;
mod graphics;
mod input;
mod rom_info;

#[cfg(test)]
mod tests;

const SEGA8_ADDRESS_START: Address = 0x0000;
const SEGA8_ADDRESS_END: Address = 0xFFFF;
const SEGA8_SEARCH_RANGES: &[(Address, Address)] = &[(SEGA8_ADDRESS_START, SEGA8_ADDRESS_END)];

pub(crate) fn collect_sega8_snapshot(
    emu: &Emulator,
    snapshot: &SnapshotRequest,
    mut buffers: ReusableBuffers,
) -> super::UiFrameData {
    let rom_bytes = emu.bus().cartridge.rom();
    let mut data = super::UiFrameData {
        perf_info: snapshot
            .want_perf_info
            .then(|| rom_info::sega8_perf_snapshot(emu)),
        rom_page: super::build_rom_page(
            snapshot.show_rom_viewer,
            snapshot.rom_view_start,
            rom_bytes,
        ),
        rom_size: rom_bytes.len() as u32,
        rom_search_results: super::build_rom_search(snapshot.rom_search.as_ref(), rom_bytes),
        memory_page: super::build_memory_page(
            snapshot.show_memory_viewer,
            snapshot.memory_view_start,
            buffers.memory_page.take(),
            |addr| emu.cpu_peek8(narrow_u16(addr)),
        ),
        memory_search_results: super::build_memory_search_ranges(
            snapshot.memory_search.as_ref(),
            SEGA8_SEARCH_RANGES,
            |addr| emu.cpu_peek8(narrow_u16(addr)),
        ),
        ..Default::default()
    };

    if snapshot.want_debug_info {
        data.cpu_debug = Some(cpu::sega8_cpu_snapshot(emu));
        data.input_debug = Some(input::sega8_input_snapshot(emu));
    }

    data.disassembly_view = super::build_disassembly_view(
        snapshot.show_disassembler,
        snapshot.last_disasm_pc,
        Address::from(emu.cpu().regs().pc),
        || z80_disassemble_around(|addr| emu.cpu_peek8(addr), emu.cpu().regs().pc, 12, 26),
        emu.iter_breakpoints(),
    );

    if snapshot.show_rom_info {
        data.rom_debug = Some(rom_info::sega8_rom_info(emu));
    }

    if snapshot.any_vram_viewer_open {
        data.graphics_data = Some(graphics::sega8_graphics_snapshot(
            emu,
            buffers.vram.take(),
            buffers.oam.take(),
        ));
    }

    if snapshot.show_oam_viewer {
        data.oam_debug = Some(graphics::sega8_oam_snapshot(emu));
    }

    if snapshot.show_apu_viewer {
        data.apu_debug = Some(apu::sega8_apu_snapshot(emu));
    }

    if snapshot.any_viewer_open {
        data.palette_debug = Some(graphics::sega8_palette_snapshot(emu));
    }

    data
}

pub(super) fn sega8_system_label(system: Sega8System) -> &'static str {
    match system {
        Sega8System::MasterSystem => "Sega Master System",
        Sega8System::GameGear => "Game Gear",
        Sega8System::Sg1000 => "SG-1000",
    }
}

pub(super) fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
